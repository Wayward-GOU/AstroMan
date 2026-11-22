# 数据池自定义列 + Excel 模板导入

> 状态：已完成（v1.1.2，DB v20）。实现与用户用法见 [features.md](../../docs/features.md) 与 [load-testing.md](../../docs/load-testing.md)；本文件保留设计与决策记录。

## Context

当前数据池（参数池 / Token 池）的条目列是写死的 `value / account / weight`，只能一行一个值。用户希望：

- 新建池时自己定义要导入的列（如 `phone, password, token, uid`）；
- 下载与列对应的 Excel 模板，填好后直接导入；
- 列表按自定义列展示，而不是固定三列；
- 压测绑定时既支持“按列名自动注入所有列”，也支持“指定某一列映射到某个占位符”；
- 列定义随时可改（重命名 / 增删 / 排序），已有数据按列名迁移。

已确认的产品决策：**两种绑定都支持**、**列随时可改**。

## 目标与非目标

目标：
1. 池级列定义 + Excel 模板下载 / 导入。
2. 条目数据按列存储，列表动态展示。
3. 绑定支持 `column="*"`（自动同名注入）与 `column="具体列"`（指定映射）。
4. 改列时数据迁移可预期；旧池和旧计划保持兼容。

非目标：
- 不做多表 / 关联表；仍是单池单表。
- 不做 Excel 公式、多 sheet 数据导入（只读第一个数据 sheet）。
- 不改变 `weight` 的运行时语义（仍为展示/元数据）。

## 数据模型与迁移

`src-tauri/src/db/mod.rs`
- `DB_VERSION` 19 → 20。
- 在 `COLUMN_MIGRATIONS` 增加：
  - `data_pools.columns_json TEXT NOT NULL DEFAULT '[]'`：按顺序保存列名数组，如 `["phone","password","token"]`。
  - `data_pool_items.data_json TEXT NOT NULL DEFAULT '{}'`：整行数据，如 `{"phone":"138...","password":"...","token":"..."}`。
- 复用现有 `add_column_if_missing`，同时更新 `src-tauri/src/db/schema.sql` 的新库建表定义。
- 旧数据兼容：
  - 旧池 `columns_json` 为空时按旧版固定列处理，读取时若 `data_json` 为空则动态合成 `{value, account}`，避免健康度与展示脱节；
  - 旧池第一次保存列定义时，在同一事务里用 `value/account` 回填 `data_json`；
  - 写路径始终同步 `value/account` 镜像，避免镜像与 data 漂移。

列名规则（服务端统一校验）：
- `trim` 后非空、去重、最多 24 列、长度 ≤ 32；
- 仅允许 `[A-Za-z_][A-Za-z0-9_]*`，保证能直接作为 `{{变量名}}`（前端 JS 的 `\w` 不匹配中文）；
- 首列必填；Token 池默认 `["token","account"]`，参数池默认 `["value"]`（仅作为新建默认值）。

兼容镜像字段：
- `value` = 首列值（保持旧接口 / 状态 / 刷新逻辑可用）；
- `account` = 存在 `account` 列时同步；
- `weight` 仍独立存储，不进入列定义。

## 后端改造

### 1. 池服务 `src-tauri/src/services/pool.rs`

- `PoolRow` 增加 `columns: Vec<String>`；`list_pools` 读取 `columns_json` 并解析。
- `PoolItemView` 增加 `data: HashMap<String, String>`；`pool_items_page`：
  - 查询 `data_json` 并解析；
  - 敏感池对 `data` 的每个值做 `mask_secret_value`；
  - 搜索条件扩展到 `data_json LIKE ?`，状态筛选不变。
- `PoolItem`（`services/cluster/protocol.rs`）增加 `data: HashMap<String,String>`（`#[serde(default)]`）；`load_items` 一并读取。
- `import_items` 改为按池列解析：
  - JSON 数组：按 key 写入 `data_json`，未知列忽略；
  - 文本行：首行若与列名完全匹配则当作表头跳过，否则按列顺序取逗号分隔值；`#` 注释保留；
  - 旧池（`columns` 为空）继续走 `值,账号,权重` 逻辑；
  - 写入时同步 `value`（首列）与 `account`。
- 新增 `save_columns(conn, pool_id, changes)`：
  - 入参 `[{ from?: string, name: string }]`；
  - **先校验后写入**：新列名合法且无重复；同一旧列不能被两个新列同时引用；改名撞上“本批未被改名/删除的旧列”时报冲突；
  - 在单个事务内按旧 `data_json` 一次算出新 JSON（改名取 `from`、同名保留、新增空、删除丢弃），再批量更新条目、列定义与 `value/account` 镜像；任一步失败整体回滚；
  - 旧池首次编辑时先合成旧行数据再迁移；
  - 不自动改写已保存计划里的列名；计划引用了已删除列时，预览 / 开跑必须报明确错误，禁止静默回退 `value`。
- 新增 `reveal_item(conn, item_id) -> { id, value, data }`，管理页“显示明文”一次展示整行。
- `preview`（命令层）适配列：
  - `column="*"` 时 `target` 允许为空，汇总里返回 `targets`（所有列名）；
  - 显式列校验必须存在于池列中，否则加入 `problems`；
  - 同一池多个绑定若注入同名变量，加入 `problems`；
  - 样例数据返回 `data`（敏感时逐列脱敏）。

### 2. Excel 模板与导入（新增 `services/pool_excel.rs`）

依赖（`src-tauri/Cargo.toml`）：
- `calamine`：读取 `.xlsx`；
- `rust_xlsxwriter`：生成模板。

命令（`commands/pool.rs`，注册到 `lib.rs`）：
- `pool_excel_template(pool_id, path)`：
  - Sheet「数据」：第一行 = 当前列名，冻结首行、按列宽自适应；
  - Sheet「说明」：填写规则、首列必填、不要改表头、单次上限 5 万行、敏感池提示；
  - 不写示例数据行，避免被误导入。
- `pool_excel_import(pool_id, path) -> { inserted, skipped, ignoredColumns }`：
  - 只读第一个 sheet；首行按列名映射（顺序可变）；
  - 缺少首列 → 直接报错并列出缺失列；
  - 其他列缺失 → 该列留空；未知表头忽略并回传；
  - 全空行跳过；首列为空的行跳过并计入 `skipped`；
  - 5 万行上限；事务批量插入；
  - 所有列在模板中设为文本格式，避免手机号 / 身份证变成科学计数法；导入时整数值去掉 `.0`，超过 15 位的数字返回 `numericWarnings` 提示改用文本格式。

前端文件选择复用 `@tauri-apps/plugin-dialog` 的 `save()` / `open()`，不引入 fs 插件。

### 3. 运行期绑定 `src-tauri/src/commands/stress.rs`

- `models.rs::PoolBinding` 增加 `#[serde(default)] column: String`：
  - `""`：旧行为，取 `value`（仅旧池 / 旧计划）；
  - `"*"`：自动注入该条目所有列（列名即变量名）；
  - 其他：取 `data[column]`；列不存在时在预览 / 预检阶段报错，运行期不做回退。
- `RuntimeItem` 增加 `data: HashMap<String,String>`；`from_pool_item` 与 `apply_refreshed` 同步。
- `PoolRuntime::pick` 改为**按池分组**：
  - 每个池每次只选一条条目（同池多个绑定共用同一条），解决“账号 + 密码不来自同一行”的旧问题；
  - 同一池的 mode 必须一致：不一致时预览报 problem，并阻止开跑（`refreshOn401` 按任意一条为 true 处理）；
  - 应用绑定：自动列全部插入，显式列插入 `target`；
  - 空值列跳过；显式列不存在时开跑前直接报错，不在运行期静默回退。
- 预检缺失变量顺序调整：
  - 当前 `check_unresolved` 早于 `build_pool_runtime` 且不查库；需先读取池列定义，把自动绑定的全部列名与显式绑定的 `target` 一起并入“已提供变量”白名单，再做缺失检查。
- Token 刷新：
  - 刷新只更新 token 列（存在 `token` 列则用它，否则首列）与 `value`；
  - 刷新源读取时按同一规则解析列，并随结果回传 `column`。

### 4. 集群切片

- `PoolItem.data` 随切片下发；`PoolSlice` 结构不变。
- `pool_slices_for`（`services/cluster/server.rs`）改为**按池生成一个切片**，不再按 binding 重复切片：
  - 同池多个绑定共用一份条目，避免重复下发与提前撞 `MAX_POOL_SLICE_BYTES`；
  - `target` 为空仅在 `column="*"` 时允许，否则仍报 `E_POOL_BINDING_INVALID`；
  - 池内 mode 冲突直接报错。
- 刷新列同步：
  - `PoolRefreshResultPayload` 增加 `column: Option<String>`（`#[serde(default)]`）；
  - 主节点刷新结果写明更新的列，从节点 `WsPoolRefreshRelay` / `apply_refreshed` 同步更新 `item.data`；
  - `HelloAck` 增加 `pool_columns: bool` 能力位：任一节点不支持时，含自定义列 / 自动注入的集群计划在 prepare 阶段明确报错，避免旧节点静默用旧值。

## 前端改造

### 1. 类型与 API

`src/types/models.ts`
- `PoolSummary` 增加 `columns: string[]`。
- `PoolItemView` 增加 `data: Record<string, string>`。
- `PoolBinding` 增加 `column?: string`。

`src/api/index.ts`
- `poolCreate` 入参增加 `columns?: string[]`。
- 新增 `poolColumnsSet(poolId, columns)`、`poolExcelTemplate(poolId, path)`、`poolExcelImport(poolId, path)`。
- `poolItemReveal` 返回 `{ id, value, data }`。

### 2. 数据池页 `src/views/DataPools.vue`

- 新建池改为小弹窗：名称、类型、敏感、列编辑（默认列随类型变化；可增删 / 排序）。
- 详情头部显示列定义，新增「编辑列」按钮：
  - 可重命名、增删、排序；
  - 保存前提示：重命名按列迁移、删除列会丢弃该列数据、影响条目数；
  - 有数据时仍允许改，但需二次确认。
- 表格改为动态列：
  - `activePool.columns` 逐列渲染 `record.data[col]`，敏感值显示掩码；
  - 保留系统列：状态、权重、失败、最近使用、操作；
  - `revealed[itemId]` 从字符串改为“整行 data”，显示明文时所有列一起显示。
- 导入区改为三块：
  - 「下载 Excel 模板」；
  - 「导入 Excel」（文件选择 → 调命令 → 展示 inserted / skipped / ignoredColumns）；
  - 「文本导入」保留为折叠项，按当前列提示 JSON 数组或“列顺序逗号分隔”。
- 提示文案同步：Excel 表头必须与列名一致，首列必填。

### 3. 绑定 UI

`StressTest.vue`（新建压测弹窗 + 配置页）、`Runner.vue`、`Automation.vue`：
- 绑定行增加「列」选择：
  - `* 全部列（同名注入）`；
  - 当前池的每个列名；
  - 旧池无列定义时隐藏该选择。
- 选 `*` 时隐藏/禁用占位符输入，提示“列名即变量名”；选具体列时占位符默认填列名，可改。
- 预览调用带 `column`；预览结果展示自动注入的变量名列表。
- 409/401 说明仍按池维度处理。

## AI 工具、提示词与工作台

- `src/utils/aiTools.ts`：
  - `pool_create` 增加 `columns: array`；
  - `pool_get` / `pool_preview` 输出列与 data（保持脱敏）；
  - `stress_start` / `plan_save` 的 `poolBindings` 支持 `column`（`*` 或列名），并做类型校验。
- `src-tauri/src/prompts/hangzai_system_prompt.md` 第 4 节：
  - 建池先定列 → 下载模板 → Excel 导入 → 动态列展示 → 绑定（自动 / 指定列）→ 预览；
  - 说明改列规则与数据迁移风险；
  - 同一池多个绑定共用同一条目。
- `Dashboard.vue`：数据池步骤与 FAQ 补充自定义列 / Excel 模板 / 自动注入。

## 实施顺序

1. DB 迁移 + 列字段 + `pool_create` / `pool_columns_set`。
2. 文本 / JSON 导入按列写入，`pool_items_page` 动态 data，reveal 返回整行。
3. Excel 依赖与模板 / 导入命令。
4. 运行期按池分组 + 自动注入 / 指定列 + 预检 + 集群刷新列。
5. 数据池页动态表格、新建 / 编辑列、Excel 导入 UI。
6. 三处绑定 UI 与预览。
7. AI 工具、提示词、工作台、文档。
8. 回归测试与手工验证。

## 验证

Rust：
- `cargo check --tests` 0 警告；
- `cargo test --lib` 全绿，新增用例覆盖：
  - v19→v20 迁移、旧池首次改列回填、镜像与 data 一致性；
  - 列定义校验与迁移（改名 / 互换 / 删除 / 排序 / 冲突）及失败回滚；
  - 文本按列导入与 JSON 导入；
  - Excel 模板生成 → 读取回导（roundtrip），手机号数字 / 前导零 / 空行 / 缺表头 / 5 万边界；
  - 自动注入所有列、显式列映射、显式列缺失报错；
  - 同池多绑定共用同一条目、同池 mode 冲突；
  - `*` 绑定的预检白名单、列名与环境变量 / 他池冲突；
  - 集群同池只生成一个切片、刷新列同步、能力位缺失时的报错；
  - 旧池 / 旧 `column=""` 绑定行为不变。

前端：
- `npx vue-tsc --noEmit`；
- `npx vitest run`（当前基线 34 文件 / 318 项）。

手工端到端：
1. 新建参数池，列设为 `phone,password,token`；
2. 下载 Excel 模板，填 3 行，导入，确认动态列与脱敏；
3. 编辑列：把 `password` 改名 `pwd`，确认数据迁移；删除 `token` 列前出现风险提示；
4. 压测绑定一行 `*`，再绑定一行 `pwd → {{pwd}}`，预览无 problems；
5. 开跑确认 `{{phone}}` / `{{pwd}}` 均能渲染，401 时同池绑定同步换条目；
6. 旧池打开、旧计划载入，界面与运行均无变化。
