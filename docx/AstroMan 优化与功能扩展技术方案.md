# AstroMan 优化与功能扩展技术方案

> 状态：待评审（仅方案，不含已实现代码）
> 覆盖范围：存量问题优化（Part A）+ 新增能力（Part B）+ 竞品替代分析 + 数据模型、接口契约、测试与风险
> 本文只描述"怎么做"，不承诺交付时间；阶段划分仅表示先后顺序。

---

## 0. 结论摘要

### 0.1 产品定位

AstroMan 不应做成"第二个 Postman"，而应把现有四块能力串成闭环：

```
抓包（已有 MITM） → 重放（已有改参重发） → 用例（已有测试套件） → 压测（已有引擎） → CI（已有 CLI）
```

Postman 抓包弱、Charles 测试弱、JMeter 调试体验差，这个中间地带是当前真正的空位。

### 0.2 本轮建议的三个主线

| 主线 | 内容 | 理由 |
|---|---|---|
| 主线一 | 抓包能力深化（断点、Map Local/Remote、过滤、HAR 导入、WebSocket 帧） | MITM 基建已经存在，边际成本最低，替代 Charles/Fiddler 的关键 |
| 主线二 | 补 Postman 硬缺口（凭据加密、集合级继承、OAuth2 完整、Mock、定时巡检） | 影响"能不能长期用"，而不是"能不能用" |
| 主线三 | 独有闭环（抓包会话 → 一键生成测试用例 → CLI/CI → 压测基线） | 竞品抄不走的组合 |

### 0.3 明确不做 / 后置

- 团队协作 / 云端同步（等于换赛道，需要服务端与账号体系）
- 分布式压测（用户规模未到）
- gRPC 自研（Protobuf 反射是独立工程，最多外挂 grpcurl）
- Linux 桌面端（构建链与验证成本高）
- Wireshark 级 L2–L4 抓包（不是同一领域）

---

## 1. 现状盘点

### 1.1 已具备能力（不在本方案重复开发）

| 域 | 已具备 |
|---|---|
| HTTP 调试 | HTTP/1.1、HTTP/2、GraphQL Body、Cookie Jar、全局 mTLS/客户端证书、全局代理、按请求超时/重试/重定向/协议 |
| 请求体 | none / raw / json / form / multipart / binary / graphql，按类型草稿（`body_meta`） |
| 变量 | 环境变量 + 运行时变量 + 内置变量（`{{$timestamp}}`、`uuid`、`randomInt`、`randomString`、`randomEmail`、`randomPhone`、`randomIPv4`、`randomHex`、`randomBool`、时间函数）+ `{{secret:NAME}}` 脱敏采集 |
| 抓包 | 本机 MITM 代理、HTTPS 解密与根证书、系统代理接管、手机同 Wi-Fi、域名分组、HAR/ZIP 导出、改参重发 |
| 测试 | 测试套件、请求/Python/压测三类用例、CSV/JSON 数据集、断言、HTML/JSON/JUnit 报告、CLI、CI、SLA 与基线门禁 |
| 压测 | 单接口压测引擎、实时曲线、报告采样、错误明细、报告请求明细 |
| AI | 自带 Key 的"航仔"：三协议、61 业务工具 + `tool_search`、写操作确认、上下文预算、用量统计 |
| 工程 | 备份、诊断包、命令面板 Ctrl+K、21 套主题、SQLite WAL、AES-GCM 主密钥（系统钥匙串 / 口令模式） |

### 1.2 现状的关键约束（方案必须遵守）

1. **数据库迁移是"判存式补列"**：结构版本 `DB_VERSION = 3`，见 [db/mod.rs](file:///d:/Software/ProjectFiles/AstroMan/src-tauri/src/db/mod.rs)。`schema.sql` 每次启动都以 `IF NOT EXISTS` 执行，因此：
   - **新增表**：只写进 `schema.sql` 即可，老库也会建出来；
   - **新增列**：必须同时写进 `schema.sql`（新库）与 `COLUMN_MIGRATIONS`（老库），只写一处会导致新老库结构不一致。
2. **详情读取使用列序号**：`map_detail` 按 `row.get(...)` 的固定序号取值，见 [commands/request.rs](file:///d:/Software/ProjectFiles/AstroMan/src-tauri/src/commands/request.rs)。任何调整 SELECT 顺序的改动，都必须同步修正序号，否则会静默错列。
3. **已存在一处结构漂移**：`requests.net_json` 只出现在 `COLUMN_MIGRATIONS`，不在 `schema.sql`。功能可用（迁移会补），但新库结构定义不完整，应在本轮修正。
4. **前端超大组件**：[RequestEditor.vue](file:///d:/Software/ProjectFiles/AstroMan/src/views/RequestEditor.vue) 已 3200+ 行，所有请求编辑与 Body 草稿逻辑集中于此。新功能继续加会失控。
5. **标签上限固定 20**：`MAX_REQUEST_TABS = 20`，见 [stores/tabs.ts](file:///d:/Software/ProjectFiles/AstroMan/src/stores/tabs.ts)，且持久化在 localStorage。
6. **代码编辑器是 textarea + 着色层叠加**：[CodeArea.vue](file:///d:/Software/ProjectFiles/AstroMan/src/components/CodeArea.vue) 用透明 textarea 覆盖 `<pre>`，支持 JSON 着色与 `{{变量}}` 高亮，但没有查找、折叠与虚拟滚动。
7. **测试覆盖偏科**：前端 134 个测试集中在 utils/tools，Rust 103 个单测，**没有组件测试、没有 E2E、没有主题截图回归**。本轮涉及 21 套主题与大规模组件拆分，必须先补测试地基。

---

## 2. 总体设计原则

| 原则 | 具体要求 |
|---|---|
| 不破坏旧库 | 所有结构变更走判存补列；禁止直接 `DROP` / 重建表 |
| 不静默失败 | 迁移、解密、加密封装错误必须可判定（参考 `E_SCHEMA` / `E_LOCKED` / `E_KEY_MISMATCH` 约定） |
| 敏感值不落明文 | 新增任何凭据字段必须复用 `secret::encrypt(aad, plain)`，并为每个列定义独立 AAD |
| 先测后拆 | 拆分 SFC 前先补组件测试与 E2E 冒烟，否则回归不可发现 |
| 默认关闭 | 新协议、新服务端（Mock/定时）默认关闭，用户显式开启，避免影响既有轻量体验 |
| 可观测 | 新增后台能力（Mock、定时、WebSocket）必须有启动/停止/错误日志与状态查询命令 |

---

## Part A · 存量优化方案

> 编号 O-xx 用于后续建 issue；每项含"问题—证据—方案—涉及文件—验收"。

### A1 交互体验

#### O-01 响应体查找（高优先）

- **问题**：响应体只展示，无 Ctrl+F，长 JSON 无法定位字段。
- **证据**：[CodeArea.vue](file:///d:/Software/ProjectFiles/AstroMan/src/components/CodeArea.vue) 无查找状态；响应区只有展示。
- **方案**：
  1. 新增通用 `FindBar.vue`（输入框、上一个/下一个、匹配计数、区分大小写、仅当前响应内搜索）。
  2. `CodeArea` 增加 `find` 相关 props 与 `matchRanges` 计算，在着色层叠加 `<mark>` 高亮；同时用 `textarea.setSelectionRange(start, end)` + `scrollTop` 估算定位，保证滚动跟随。
  3. 响应体若走只读模式（二进制/超大响应落盘），查找降级为"仅文本响应可用"，按钮置灰并提示。
- **涉及文件**：`src/components/CodeArea.vue`、新增 `src/components/common/FindBar.vue`、`src/views/RequestEditor.vue` 响应区。
- **验收**：10MB 文本响应中输入关键字，首个匹配 200ms 内可见并滚动到位；匹配计数正确；Esc 关闭后恢复原滚动位置。

#### O-02 响应体折叠 / 大响应虚拟化（中高）

- **问题**：超大 JSON 一次性 `v-html` 渲染，节点数上万时卡顿。
- **方案**：
  1. 阈值策略：文本 ≤ 256KB 走现有 CodeArea；> 256KB 默认进入"精简模式"，只渲染首 512 行并在顶部提示。
  2. 折叠：在着色层按行首 `{` / `[` 做括号配对，生成可折叠区间；折叠状态只存区间偏移，不做全文重排。
  3. 虚拟滚动：仅在"结构化树视图"（JSON 树）中启用，按可视行窗口渲染；文本视图保留 textarea 原生滚动，避免与透明叠加层冲突。
- **涉及文件**：`CodeArea.vue`、`JsonTree.vue`、响应区。
- **验收**：5MB JSON 首次渲染 ≤ 400ms，滚动稳定 60fps，折叠/展开无整页重排。

#### O-03 图片预览缩放与二进制另存为（中）

- **方案**：预览容器加缩放（滚轮 + 按钮，1x–8x）与拖拽平移；工具栏增加"另存为"，调用已有文件保存命令把 `body_file` 落盘到用户选择路径。
- **涉及**：响应区工具栏、`commands/files.rs` 复用。
- **验收**：PNG/PDF 可缩放查看；另存为后文件字节数与响应 `body_bytes` 一致。

#### O-04 请求编辑易用性（中）

| 子项 | 方案 |
|---|---|
| Header 自动补全 | 内置常用 Header 词表（Content-Type、Accept、Authorization、User-Agent、X-Request-Id 等），输入时下拉补全 |
| 路径参数识别 | URL 中 `:id` / `{id}` 高亮，并在 Params 面板提示"未填充的路径参数" |
| JSON 工具条 | 编辑区上方增加 格式化 / 压缩 / 转义 / 复制，复用现有格式化工具函数 |
| 变量未定义提示 | 发送前用 `collect_placeholders` + 环境变量表求差集，未定义变量在 URL/Body 下方黄条提示，不阻塞发送 |

- **涉及**：[RequestEditor.vue](file:///d:/Software/ProjectFiles/AstroMan/src/views/RequestEditor.vue) 顶部与 Params/Body 面板、`src/utils` 工具。
- **验收**：占位符差集与 Rust 侧 `collect_placeholders` 语义一致（含 `$` 前缀与函数型占位符）。

#### O-05 快捷键体系（中）

现有仅 Ctrl+K。补齐：

| 快捷键 | 行为 |
|---|---|
| Ctrl+S | 保存当前请求（有未保存改动时提示） |
| Ctrl+Enter | 发送当前请求 |
| Ctrl+F | 响应体查找（O-01） |
| Ctrl+W | 关闭当前标签（多标签时） |
| Ctrl+Tab / Ctrl+Shift+Tab | 下一个 / 上一个标签 |
| Alt+Enter | 在 Params 表格新增一行 |

- **实现**：统一快捷键注册模块（`src/utils/hotkeys.ts`），按焦点区域判定，避免在输入框内误触发；命令面板保留。
- **验收**：在 textarea 内 Ctrl+Enter 仍触发发送；Ctrl+S 不与浏览器默认冲突。

#### O-06 标签策略（低中）

- **问题**：固定 20 个，超出直接拒绝，用户无感知。
- **方案**：改为"软上限"：仍默认 20，但提供设置项；超出时弹出确认"关闭最久未使用标签并打开"，按 `lastActiveAt` 做 LRU；持久化结构增加该字段（localStorage 兼容旧格式，缺失视为当前顺序）。
- **验收**：旧 localStorage 数据升级后不丢标签；LRU 选择正确。

### A2 可靠性

#### O-07 网络错误诊断建议（高）

- **问题**：错误只有原始字符串，用户不知道下一步。
- **方案**：新增错误分类映射（前端纯函数 + Rust 错误前缀共存）：
  - 证书类 → 提示"可临时开启『忽略证书校验』"；
  - DNS 解析失败 → 提示检查域名/代理/DNS；
  - 连接超时 / 读取超时 → 区分 connect vs read，给出对应超时项；
  - 代理连接失败 → 提示检查代理地址与端口；
  - HTTP/2 被服务端拒绝 → 提示切 HTTP/1.1。
  响应区错误卡片展示"原因 + 建议操作 + 一键应用建议"。
- **涉及**：响应区错误展示、`commands/http.rs` 错误构造。
- **验收**：五类典型错误均有可读建议，且有单测覆盖映射。

#### O-08 历史与运行记录自动保留（中）

- **问题**：schema 注释写明"最多保留 10 万条历史 / 每接口 200 条运行"，但需确认是否真有清理任务；长期使用库会膨胀。
- **方案**：启动后延迟执行（不阻塞首屏）：
  - `request_history` 按 `created_at` 保留最近 N 天且总数 ≤ 上限；
  - `request_runs` 按 `request_id` 保留每接口最近 200 条；
  - 清理后按需 `PRAGMA incremental_vacuum`（需确认是否 auto_vacuum，否则跳过）。
  - 设置页展示当前占用与"上次清理时间"，支持手动"立即清理"。
- **涉及**：`db/mod.rs` 或新 `commands/maintenance.rs`。
- **验收**：清理为幂等，单次执行在 2 万条量级下不阻塞 UI 超过 300ms（后台线程）。

#### O-09 更新流程（中）

- **现状**：`update_check` + `update_download` + `open_external`，下载后打开安装包，见 [commands/update.rs](file:///d:/Software/ProjectFiles/AstroMan/src-tauri/src/commands/update.rs)。
- **方案**：
  1. 下载进度事件（`update://progress`），当前无进度反馈；
  2. 校验 `asset_size` 与实际字节数，不一致则报错而不是打开；
  3. 可选 beta 通道（读取 release 列表而非 latest）；
  4. 更新失败保留旧版本可用（安装包在临时目录，失败不写系统目录）。
- **验收**：断网、磁盘满、文件被占用三类失败均有明确提示且不影响当前运行。

### A3 安全

#### O-10 请求级凭据加密（高优先，必须做）

- **问题**：`requests.auth` 直接 `serde_json::to_string` 明文落库。环境变量与 AI Key 已用 AES-GCM，请求级没有。
- **方案**（字段级加密，避免整块不可读）：
  1. 在 [secret.rs](file:///d:/Software/ProjectFiles/AstroMan/src-tauri/src/secret.rs) 增加 AAD 常量：
     ```rust
     pub const AAD_REQUEST_AUTH: &str = "requests:auth";
     ```
  2. `RequestAuth` 内敏感字段分别加密：`token`、`password`、`value`、`client_secret`；`kind`、`username`、`key`、`addTo`、`token_url`、`client_id`、`scope` 保持明文（便于 UI 展示与校验）。
  3. 写入路径（`request_save` / 更新）：解析 JSON → 逐字段 `encrypt(AAD_REQUEST_AUTH, ...)` → 序列化写库。已是 `enc:v1:` 前缀则跳过（幂等）。
  4. 读取路径（`request_get`）：逐字段 `decrypt`，前端拿到的仍是明文结构，UI 无需改动。
  5. 启动迁移：在 `encrypt_plaintext` 中增加一段"扫描 requests.auth 并就地加密"，与现有逻辑同样幂等、同样在密钥不匹配时跳过。
  6. 运行快照 `request_runs.headers` 中的 `Authorization` / `Cookie` 在落库前脱敏为 `••••••`（与响应快照一致）。
- **涉及**：`secret.rs`、`commands/request.rs`、`commands/history.rs`（快照落库处）。
- **验收**：新保存的请求在库中肉眼不可见 token；老库启动后自动加密且可正常发送；密钥不匹配时返回空串并提示重填（沿用现有 `E_KEY_MISMATCH` 语义）。

#### O-11 导出脱敏与告警（中）

- **方案**：项目 JSON / 集合导出时识别含敏感字段（auth、环境 secret、HAR 的 `Authorization`），弹出三选项："含明文导出 / 脱敏导出 / 取消"；默认脱敏。导出文件写入 `app_meta` 记录"最近一次脱敏导出时间"。
- **验收**：脱敏导出后重新导入，请求仍可用但凭据为空并明确标记"需重填"。

#### O-12 日志与报告脱敏补全（中）

- **现状**：`render_tracked` 已收集本次使用的敏感值用于日志/报告脱敏。
- **方案**：把该清单扩展到：请求头、响应头、报告 `req_headers` / `res_headers`、错误消息、诊断包。统一走一处 `mask_secrets(text, &[String])`。
- **验收**：诊断包中搜索已配置的 secret 值应为 0 命中。

### A4 性能

| 编号 | 项 | 方案 | 验收 |
|---|---|---|---|
| O-13 | 集合树与请求列表虚拟滚动 | 树节点 > 300 或列表 > 200 时启用窗口化渲染 | 5000 请求项目展开 ≤ 200ms |
| O-14 | 首屏与迁移埋点 | 记录 `db_init_ms`、`first_paint_ms` 到 `app_meta`，设置页"启动诊断"可看 | 冷启动可定位到具体阶段 |
| O-15 | 响应体落盘阈值可配 | 当前阈值写死；改为设置项（默认保持现状） | 阈值为 0 时强制落盘 |
| O-16 | 主题切换性能 | 主题变量集中在 `:root`，切换只改 CSS 变量；检查是否有组件在 `watch` 主题做重计算 | 21 套主题切换 ≤ 100ms |

### A5 工程质量

#### O-17 拆分 RequestEditor（高优先，后续功能的前置）

- **现状**：3200+ 行，Body 草稿、Auth、脚本、断言、响应渲染全在一个文件。
- **目标结构**：
  ```text
  src/components/request/
    RequestHeaderBar.vue      # 名称 / 方法 / URL / 发送
    RequestTabNav.vue         # Params / Headers / Body / Auth / Scripts / Assertions
    ParamsPanel.vue
    HeadersPanel.vue
    BodyPanel.vue             # 仅负责类型切换与分发
      BodyRawEditor.vue
      BodyFormEditor.vue      # urlencoded Key/Value 表格
      BodyMultipartEditor.vue
      BodyGraphqlEditor.vue
    AuthPanel.vue
    ScriptPanel.vue
    AssertionsPanel.vue
  src/components/response/
    ResponsePanel.vue
    ResponseToolbar.vue
    ResponseMeta.vue          # 状态 / 耗时 / 大小 / 协议
    ResponseBodyView.vue
    ResponseHeaders.vue
    ResponseTimings.vue
  ```
- **拆分规则**：先补组件测试（O-19）→ 按"纯展示组件"到"含状态组件"顺序搬 → 每步保持行为不变（无功能改动）→ 合并后再做新功能。
- **验收**：`RequestEditor.vue` ≤ 600 行；拆分前后测试结果一致；无新增全局状态。

#### O-18 Rust ↔ TS 类型自动生成（中）

- **问题**：`models.rs` 与 `types/models.ts` 手写双份，长期漂移。
- **方案**：引入 `ts-rs`，对 `HttpRequestDetail`、`RequestAuth`、`KeyValueItem`、`HttpResult`、`UpdateInfo` 等导出 TS 类型到 `src/types/generated.ts`；`npm run check` 中校验生成文件是否最新（CI 阻断）。
- **风险**：`serde` 重命名与 `ts-rs` 注解需一致；先在 3–5 个核心模型试点。

#### O-19 测试地基（高优先，先于拆分）

| 层 | 方案 |
|---|---|
| 组件测试 | Vitest + `@vue/test-utils`，优先 `CodeArea`、`KeyValueEditor`、`BodyPanel`、`FindBar` |
| 应用 E2E | 方案 A：`tauri-driver` + WebdriverIO（Windows，需 msedgedriver），覆盖"新建请求 → 发送 → 断言"；方案 B：Vite 模式注入 `VITE_MOCK_API=1` 的内存 mock，跑纯 UI 流程，速度快 |
| 主题回归 | Playwright 对 21 套主题 × 5 个关键页面截图，像素差异阈值 0.1%；启动前强制 `prefers-reduced-motion` |
| 迁移回归 | 固定 v3 库 fixture → `db::init` → 断言新列存在、auth 已加密、重复执行幂等 |
| 性能基线 | 记录首屏、5MB JSON 渲染、5000 请求树加载三条基线，超阈值即失败 |

---

## Part B · 新增能力方案

### B1 快速见效（低成本、高感知）

#### F-01 代码生成（curl / fetch / axios / Python / Go / Java / C#）

- **目标**：当前请求一键生成可运行代码。
- **方案**：
  - 纯前端模块 `src/utils/codegen/`，输入 `HttpRequestDetail` + 变量解析结果的"预览值"（默认使用 `{{变量}}` 原样，提供开关"替换为实际值"）。
  - 各语言实现独立函数，统一接口：
    ```ts
    interface CodegenTarget {
      id: 'curl' | 'fetch' | 'axios' | 'python' | 'go' | 'java' | 'csharp'
      label: string
      generate(req: CodegenInput): string
    }
    ```
  - 处理要点：`multipart` 生成各语言的文件字段写法；`graphql` 包装为 JSON body；auth 按 `kind` 生成 header/query；二进制 body 生成占位文件路径。
  - UI：请求区右上角"代码"按钮 → 抽屉，语言下拉 + 复制。
- **验收**：7 种目标对 6 类 Body 全组合均有稳定输出；curl 生成结果可被现有 cURL 导入器无损解析回来（往返测试）。

#### F-02 响应→断言一键生成（低成本）

- **方案**：读取当前响应（状态码、JSON 字段类型/路径、响应时间），生成断言列表填入 `assertions`；字段枚举上限 30 个，避免断言爆炸。
- **验收**：生成的断言在测试套件中可直接通过（对同一响应重放）。

#### F-03 内置变量 UI 化（低成本）

- **现状**：Rust 侧已支持 `{{$timestamp}}`、`{{$uuid}}`、`{{$randomInt}}`、`{{$randomString}}`、`{{$randomEmail}}`、`{{$randomPhone}}`、`{{$randomIPv4}}`、`{{$randomHex}}`、`{{$randomBool}}`、时间函数等，见 [util.rs](file:///d:/Software/ProjectFiles/AstroMan/src-tauri/src/util.rs)。
- **缺口**：用户不知道存在，也没有插入入口。
- **方案**：URL/Body/Header 输入框旁增加"变量"下拉，按分类（时间 / 随机 / 环境）列出并可一键插入；未定义变量提示与 O-04 合并。
- **验收**：插入的占位符能被 `is_builtin_placeholder` 识别，发送时不报"缺少变量"。

### B2 协议与调试能力

#### F-04 WebSocket 客户端

- **目标**：连接、发消息、帧列表、心跳、连接状态。
- **技术方案**：
  - Rust 侧新增 `tokio-tungstenite` 依赖；`commands/ws.rs` 维护会话注册表：
    ```rust
    struct WsHandle { stop: oneshot::Sender<()> }
    struct WsRegistry { sessions: Mutex<HashMap<String, WsHandle>> }
    ```
  - 命令：`ws_connect(config) -> session_id`、`ws_send(session_id, payload)`、`ws_close(session_id)`、`ws_status(session_id)`。
  - 事件：`ws://message`（`{ sessionId, direction, opcode, text, bytes, at }`）、`ws://state`（connecting/open/closing/closed/error）。
  - 复用现有网络配置：代理、`insecure`、mTLS、请求头（含 Cookie）。
  - 前端：新的请求类型 `protocol = ws`，响应区替换为帧列表（时间、方向、opcode、大小、预览，展开看完整帧）。
- **性能约束**：高频消息（> 200 帧/秒）按 100ms 批量推送，帧列表保留最近 5000 条，超出提示并支持落盘。
- **验收**：`wss://` 自签证书可连；断线后状态正确；关闭标签时自动 `ws_close`，无泄漏。

#### F-05 SSE 客户端

- **方案**：复用 HTTP 客户端流式读取，`text/event-stream` 解析为事件列表（event/id/data/retry）；命令 `sse_open` / `sse_close`；事件 `sse://message`。
- **验收**：断线重连按服务端 `retry` 执行；手动关闭立即停止。
- **备注**：与 F-04 共用"实时请求"Tab 与帧/事件列表组件，避免两套 UI。

#### F-06 完整 OAuth2（授权码 + PKCE + 自动刷新）

- **现状**：[oauth.rs](file:///d:/Software/ProjectFiles/AstroMan/src-tauri/src/commands/oauth.rs) 只支持 Client Credentials，只返回 token。
- **方案**：
  1. 新增 `OAuth2FlowConfig`：`grantType`（client_credentials / authorization_code / password / refresh_token）、`authUrl`、`tokenUrl`、`clientId`、`clientSecret`、`scope`、`redirectUri`（固定 `http://127.0.0.1:<随机端口>/callback`）、`usePkce`。
  2. 授权码流程：本地起临时 HTTP 监听 → 打开系统浏览器 → 收 code → 换 token；PKCE 用 `S256`。
  3. Token 存储：`requests.auth` 内新增 `accessToken`、`refreshToken`、`expiresAt`、`tokenEndpoint`（全部加密，见 O-10）；发送前若过期且有 refreshToken 则自动刷新并回写。
  4. 并发约束：同一请求的刷新需加锁，避免并发送多个刷新请求。
- **验收**：标准授权码 + PKCE 服务端可完成全流程；token 过期后发送自动刷新；刷新失败返回明确错误且不覆盖旧 token。

#### F-07 Digest / NTLM / AWS SigV4 / JWT

- **方案**：
  - Digest：`WWW-Authenticate` 解析 + `MD5`/`SHA-256` 计算，401 后重试一次。
  - NTLM：`ntlm` crate，支持 NTLMv2；仅 Windows 优先，非 Windows 可标记不支持。
  - SigV4：`aws-sigv4` 语义自实现（canonical request + HMAC 链），支持 region/service/session token。
  - JWT：生成器（HS256/RS256）+ 解析器（仅展示 claims，不验证签名）。
- **排序**：SigV4 → JWT → Digest → NTLM（按目标用户与企业场景收益排序）。
- **验收**：每类各一组对照测试向量（公开测试用例），结果逐字节一致。

#### F-08 抓包深化（替代 Charles 的关键）

| 子能力 | 方案 |
|---|---|
| 断点（Breakpoint） | 命中规则后在 `capture/handler.rs` 暂停：将请求/响应放入待决队列并 `oneshot` 等待；前端通过 `capture_resolve(id, action)` 放行/修改/丢弃；超时（默认 60s）自动放行并记日志 |
| Map Local | 规则表 `capture_rules(scope, match_type, pattern, target, enabled)`；命中后用本地文件或固定响应替代 |
| Map Remote | 同上，替换目标 URL（保留原 query 可选） |
| 过滤 | 按域名、方法、状态码、MIME、进程（Windows 尽力获取）、耗时阈值过滤；过滤只在展示层生效，不丢原始会话 |
| WebSocket 帧 | 抓包代理补 WebSocket 升级帧解析，帧记入会话并在 UI 以"帧"展示 |
| 会话管理 | 会话命名、备注、保存到项目、会话间对比（已有历史对比可复用思路） |
| HAR 导入 | 见 F-09 |

- **数据表**：
  ```sql
  CREATE TABLE IF NOT EXISTS capture_rules (
      id          INTEGER PRIMARY KEY AUTOINCREMENT,
      project_id  INTEGER,
      kind        TEXT NOT NULL,      -- breakpoint / map_local / map_remote / rewrite
      scope       TEXT NOT NULL,      -- request / response / both
      match_type  TEXT NOT NULL,      -- url_regex / host / method
      pattern     TEXT NOT NULL,
      target      TEXT,               -- 本地路径或目标 URL
      replace     TEXT,               -- 文本替换内容（rewrite）
      enabled     INTEGER NOT NULL DEFAULT 1,
      priority    INTEGER NOT NULL DEFAULT 0,
      created_at  INTEGER NOT NULL,
      updated_at  INTEGER NOT NULL
  );
  ```
- **验收**：断点在 60s 内可放行；Map Local 返回后客户端行为与服务端一致；规则冲突按 priority 稳定命中。

#### F-09 HAR 导入

- **方案**：`importer.rs` 增加 `import_har(path)`：解析 `log.entries[]` → 生成集合树（按 host 分组）→ 每 entry 生成一个请求（method/url/headers/query/postData）。
  - `postData.mimeType = application/x-www-form-urlencoded` → `bodyType = form`；
  - `multipart/form-data` → `multipart`（文件字段保留文件名占位）；
  - `application/json` → `json`；
  - 其他 → `raw`。
  - 敏感头（`authorization`、`cookie`）默认不导入，可选开启。
- **验收**：Chrome 导出的 HAR 导入后，条目数与集合结构正确；敏感头默认被剥离并有提示。

### B3 服务端能力（本机）

#### F-10 本地 Mock Server

- **目标**：从集合/请求直接起一个本机 HTTP 服务，替代 Mockoon/WireMock 的基础场景。
- **架构**：
  - Rust 侧用 `axum`（需新增依赖，`tokio` 已在）；运行时注册表 `MockRegistry { servers: Mutex<HashMap<i64, MockHandle>> }`。
  - 路由匹配优先级：精确 path → 路径参数 → 通配；方法精确匹配。
  - 每个路由可配：状态码、响应头、响应体、延迟、随机失败率、动态变量（复用 `util::render`）。
- **数据表**：
  ```sql
  CREATE TABLE IF NOT EXISTS mock_servers (
      id          INTEGER PRIMARY KEY AUTOINCREMENT,
      project_id  INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
      name        TEXT NOT NULL,
      port        INTEGER NOT NULL,
      base_path   TEXT NOT NULL DEFAULT '/',
      cors        INTEGER NOT NULL DEFAULT 1,
      enabled     INTEGER NOT NULL DEFAULT 0,
      created_at  INTEGER NOT NULL,
      updated_at  INTEGER NOT NULL
  );

  CREATE TABLE IF NOT EXISTS mock_routes (
      id          INTEGER PRIMARY KEY AUTOINCREMENT,
      server_id   INTEGER NOT NULL REFERENCES mock_servers(id) ON DELETE CASCADE,
      request_id  INTEGER REFERENCES requests(id) ON DELETE SET NULL,
      method      TEXT NOT NULL,
      path        TEXT NOT NULL,
      status      INTEGER NOT NULL DEFAULT 200,
      delay_ms    INTEGER NOT NULL DEFAULT 0,
      headers     TEXT,
      body        TEXT,
      fail_rate   REAL NOT NULL DEFAULT 0,
      enabled     INTEGER NOT NULL DEFAULT 1,
      priority    INTEGER NOT NULL DEFAULT 0,
      created_at  INTEGER NOT NULL,
      updated_at  INTEGER NOT NULL
  );
  ```
- **命令**：`mock_start(server_id)`、`mock_stop(server_id)`、`mock_status(server_id)`、`mock_routes_from_collection(collection_id)`。
- **约束**：
  - 端口占用必须预检并给出可读错误，禁止 panic；
  - 仅监听 `127.0.0.1`，不默认对外暴露；
  - 进程退出时注册表 Drop 自动停止。
- **验收**：起服务 → 请求命中 → 延迟/失败率生效 → 停止后端口释放。

#### F-11 定时巡检 + 告警

- **目标**：按 Cron 执行测试套件/单请求，失败告警到飞书/钉钉/企业微信/通用 Webhook。
- **数据表**：
  ```sql
  CREATE TABLE IF NOT EXISTS schedules (
      id           INTEGER PRIMARY KEY AUTOINCREMENT,
      project_id   INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
      name         TEXT NOT NULL,
      target_kind  TEXT NOT NULL,        -- request / suite
      target_id    INTEGER NOT NULL,
      env_id       INTEGER,
      cron_expr    TEXT NOT NULL,
      enabled      INTEGER NOT NULL DEFAULT 0,
      notify_json  TEXT,                 -- [{kind:'feishu',url:'...',on:'failure|always'}]
      last_run_at  INTEGER,
      next_run_at  INTEGER,
      created_at   INTEGER NOT NULL,
      updated_at   INTEGER NOT NULL
  );

  CREATE TABLE IF NOT EXISTS schedule_runs (
      id          INTEGER PRIMARY KEY AUTOINCREMENT,
      schedule_id INTEGER NOT NULL REFERENCES schedules(id) ON DELETE CASCADE,
      status      TEXT NOT NULL,        -- passed / failed / error
      total       INTEGER DEFAULT 0,
      passed      INTEGER DEFAULT 0,
      failed      INTEGER DEFAULT 0,
      duration_ms INTEGER DEFAULT 0,
      message     TEXT,
      started_at  INTEGER,
      finished_at INTEGER
  );
  ```
- **调度实现**：`tokio` 定时循环（默认 10s tick）计算到期任务；单任务互斥，避免重叠执行；Cron 解析用成熟 crate，不自研。
- **桌面端限制（必须在 UI 明说）**：应用关闭即停止巡检。不做"承诺 7×24"的营销表述；后续可选方案为注册系统计划任务，但需单独设计安装/卸载流程。
- **验收**：Cron 到点触发、失败告警可送达、重叠执行被跳过、应用重启后 `next_run_at` 正确恢复。

#### F-12 OpenAPI 文档生成

- **现状**：已有 OpenAPI 导入与项目导出，见 [commands/transfer.rs](file:///d:/Software/ProjectFiles/AstroMan/src-tauri/src/commands/transfer.rs)、[commands/importer.rs](file:///d:/Software/ProjectFiles/AstroMan/src-tauri/src/commands/importer.rs)。
- **方案**：反向生成 `openapi: 3.0.3`：
  - 集合 → `tags` + `paths`，请求名 → `summary`，`operationId` 由 method+path 生成；
  - `params` → query，`headers` → header，`bodyType` → `requestBody`（json 时尝试解析 schema，失败则 `type: string`）；
  - 环境变量不导出为示例值，避免泄密；
  - 生成后可内置预览（本地静态页面）并导出 YAML/JSON/Markdown。
- **验收**：生成文档能被 Swagger Editor 校验通过；导入回 AstroMan 后请求数与路径一致。

### B4 闭环能力（高价值、较高复杂度）

#### F-13 抓包会话 → 一键生成测试用例（差异化核心）

- **方案**：选中一个或多个抓包会话 → "生成测试用例"：
  - 按 host/path 生成请求，保留 headers/body；
  - 敏感头剥离并标记待填；
  - 断言默认生成：状态码 + 响应时间上限 + 响应 JSON 顶层字段存在性；
  - 可选直接创建套件并加入数据集占位。
- **复用**：会话数据已在抓包侧，只需转换器 + 套件写入命令。
- **验收**：生成的套件可直接运行并通过（对原始响应回放）；不产生明文凭据。

#### F-14 Flow 编排

- **范围收敛（重要）**：第一版只做 **串行 + 条件分支 + 变量传递 + 断言**，不做并行、循环、子流程。
- **数据表**：
  ```sql
  CREATE TABLE IF NOT EXISTS flows (
      id          INTEGER PRIMARY KEY AUTOINCREMENT,
      project_id  INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
      name        TEXT NOT NULL,
      description TEXT,
      graph_json  TEXT NOT NULL,
      created_at  INTEGER NOT NULL,
      updated_at  INTEGER NOT NULL
  );

  CREATE TABLE IF NOT EXISTS flow_runs (
      id          INTEGER PRIMARY KEY AUTOINCREMENT,
      flow_id     INTEGER NOT NULL REFERENCES flows(id) ON DELETE CASCADE,
      env_id      INTEGER,
      status      TEXT NOT NULL,
      started_at  INTEGER,
      finished_at INTEGER,
      summary_json TEXT
  );
  ```
- **graph_json 结构**：
  ```json
  {
    "nodes": [
      { "id": "n1", "type": "start" },
      { "id": "n2", "type": "request", "requestId": 12, "extract": [{"name":"token","path":"$.data.token"}] },
      { "id": "n3", "type": "condition", "expr": "{{status}} == 200" },
      { "id": "n4", "type": "end" }
    ],
    "edges": [ { "from": "n1", "to": "n2" }, { "from": "n2", "to": "n3", "when": "true" } ]
  }
  ```
- **执行**：复用现有 `test_runner` 的变量与断言机制，避免两套执行器。
- **验收**：三节点串行 + 条件分支可跑通；提取变量在后续请求中生效；失败节点记录原因。

#### F-15 混合多接口压测（后置增强）

- 在现有单接口压测上增加"场景配比"：`[{requestId, weight}]` + 总并发/总时长，按权重分流。
- 报告增加按接口维度拆分（复用 `report_requests`）。
- **验收**：两种接口按 7:3 配比发送，实际比例偏差 ≤ 5%。

---

## 3. 数据模型与迁移设计

### 3.1 结构版本规划

| 版本 | 内容 |
|---|---|
| 3（现状） | 按 Body 类型草稿 `requests.body_meta` |
| 4 | 请求级凭据加密（无新列）、集合级配置列、抓包规则表、`schema.sql` 补 `net_json` |
| 5 | Mock 表、定时巡检表、Flow 表 |
| 6（可选） | WebSocket 会话持久化、混合压测场景配置 |

> 每个版本只做一件事，便于出问题回滚到上一版本；不合并发布。

### 3.2 v4 具体变更

```sql
-- 集合（含文件夹）级配置
ALTER TABLE collections ADD COLUMN variables   TEXT;
ALTER TABLE collections ADD COLUMN auth        TEXT;
ALTER TABLE collections ADD COLUMN pre_script  TEXT;
ALTER TABLE collections ADD COLUMN post_script TEXT;

-- 新表：抓包规则
CREATE TABLE IF NOT EXISTS capture_rules ( /* 见 F-08 */ );
```

同时：
- 在 `schema.sql` 的 `collections` 定义中补齐上述四列；
- 在 `schema.sql` 的 `requests` 定义中补 `net_json TEXT`（修正既有漂移）；
- `COLUMN_MIGRATIONS` 追加上述四列；
- `DB_VERSION = 4`。

### 3.3 继承解析顺序（集合级配置）

发送时变量的优先级从高到低：

1. 请求级（脚本运行时变量、`{{secret:}}`）
2. 最近父文件夹
3. 集合根
4. 环境变量
5. 内置变量

Auth 采用"就近覆盖"：请求显式设置了 `kind != none` 则用请求；否则向上找最近的集合节点；都没有则不注入。
脚本同理：先父后子执行 `pre_script`，`post_script` 反向。

**实现位置**：发送前统一解析（`commands/http.rs` 组装请求处），禁止在前端做继承合并，避免 CLI 与 UI 行为分叉。

---

## 4. 接口契约

### 4.1 Rust 命令清单（新增）

| 域 | 命令 | 入参 | 返回 |
|---|---|---|---|
| 凭据 | `request_save`（改造） | `HttpRequestDetail` | `HttpRequestDetail` |
| WebSocket | `ws_connect` / `ws_send` / `ws_close` / `ws_status` | 见 F-04 | `String` / `void` / 状态结构 |
| SSE | `sse_open` / `sse_close` | 同 HTTP 配置 | `String` |
| OAuth2 | `oauth2_authorize` / `oauth2_refresh` | `OAuth2FlowConfig` | `OAuth2Token` |
| Mock | `mock_start` / `mock_stop` / `mock_status` / `mock_routes_from_collection` | `server_id` 等 | 状态结构 |
| 定时 | `schedule_list` / `schedule_save` / `schedule_delete` / `schedule_run_now` / `schedule_runs` | — | 列表 / 结果 |
| 抓包 | `capture_rules_list` / `capture_rule_save` / `capture_rule_delete` / `capture_resolve` | 规则 / 会话 | 列表 / `void` |
| 导入 | `import_har` | `path`、`projectId` | 导入统计 |
| 文档 | `openapi_export` | `projectId`、`format` | 文件路径 |
| 代码 | 前端实现，无需命令 | — | — |

### 4.2 前端 API 包装规范

沿用 [src/api/index.ts](file:///d:/Software/ProjectFiles/AstroMan/src/api/index.ts) 的写法，一个命令一个导出函数，返回类型必须来自生成类型（O-18）：

```ts
export const mockStart = (serverId: number) =>
  invoke<MockStatus>('mock_start', { serverId })
```

事件统一命名：`domain://event`（如 `ws://message`、`mock://request`、`schedule://run-finished`），前端集中在一个 `src/api/events.ts` 注册，避免散落。

### 4.3 错误码约定

| 前缀 | 含义 |
|---|---|
| `E_SCHEMA` | 结构/迁移失败 |
| `E_LOCKED` | 主密钥未解锁 |
| `E_KEY_MISMATCH` | 主密钥与密文不匹配 |
| `E_PORT` | Mock 端口占用 |
| `E_WS` | WebSocket 连接/协议错误 |
| `E_OAUTH` | OAuth2 流程失败 |
| `E_CRON` | Cron 表达式非法 |

前端按前缀映射到中文建议，与 O-07 共用同一张映射表。

---

## 5. 前端结构方案

### 5.1 目录调整

```text
src/
  api/
    index.ts          # 现有命令
    events.ts         # 新增：事件注册
  components/
    common/
      FindBar.vue
      StatusPill.vue
    request/          # O-17 拆分产物
    response/         # O-17 拆分产物
  composables/
    useRequestBodyDrafts.ts   # 从 RequestEditor 抽出的 bodyDrafts 逻辑
    useHotkeys.ts             # O-05
    useResponseFind.ts        # O-01
  utils/
    codegen/          # F-01
    errorHints.ts     # O-07
  types/
    models.ts         # 保留手写兼容层
    generated.ts      # O-18 自动生成
```

### 5.2 状态归属原则

| 状态 | 归属 |
|---|---|
| 请求编辑态、Body 草稿 | `RequestEditor` 容器（后续可下沉到 `useRequestBodyDrafts`） |
| 响应结果、查找状态 | `ResponsePanel` |
| WebSocket 帧、SSE 事件 | `RealtimePanel`（独立组件，按会话 id 订阅） |
| Mock / 定时 / 抓包规则 | 各自 Pinia store，独立于请求编辑 |
| 全局设置（主题、代理、证书） | 现有 settings store 不变 |

---

## 6. 测试与验收

### 6.1 分层测试要求

| 变更类型 | 必备测试 |
|---|---|
| 解析/加密/变量/编码 | Rust 单测（含往返、异常、幂等） |
| 组件拆分 | Vitest 组件测试，覆盖 props/emits 契约 |
| 新增命令 | Rust 单测 + 前端 API 冒烟 |
| 数据迁移 | v3 fixture → v4 迁移 → 二次执行幂等 |
| 实时协议 | 本地起测试服务端（Rust 内嵌 axum），断言消息顺序与关闭清理 |
| UI 主题 | Playwright 截图回归（21 主题 × 5 页） |
| 抓包规则 | 规则命中优先级单测 + 端到端回放 |

### 6.2 性能预算

| 指标 | 预算 |
|---|---|
| 冷启动到可交互 | ≤ 1.5s（本地 SSD，不含云同步） |
| 5MB JSON 首次渲染 | ≤ 400ms |
| 5000 请求项目树加载 | ≤ 200ms |
| 主题切换 | ≤ 100ms |
| 响应查找首个命中 | ≤ 200ms |

### 6.3 验收口径

每个功能必须同时给出：
1. 正常路径演示；
2. 至少 3 条异常路径（网络失败、非法输入、资源占用）；
3. 老库升级路径（用 v3 备份库实测）；
4. 回归结论（既有 134 前端测试 + 103 Rust 测试全绿）。

---

## 7. 竞品替代矩阵

### 7.1 当前即可替代

| 软件 | 程度 | 依据 | 主要缺口 |
|---|---|---|---|
| Postman / Insomnia / Hoppscotch / Bruno | ~70% | HTTP 调试、环境变量、历史、集合、导入、测试套件、CLI/CI | Mock、Monitors、团队协作、代码生成、集合级继承 |
| Newman / Postman Runner | ~80% | 测试套件 + CLI + CI + JUnit 报告 | 数据驱动广度、并行执行 |
| curl / HTTPie | ~70% | cURL 导入 + CLI | 纯脚本场景仍建议保留 |
| JMeter / k6 / Locust（单机） | ~60% | 单接口压测、SLA、基线门禁、报告 | 混合场景、分布式、脚本化 |
| Charles / Fiddler / Whistle | ~50–60% | MITM、HTTPS 解密、系统代理、手机抓包、HAR/ZIP 导出、改参重发 | 断点、Map Local/Remote、改写规则、进程过滤、WebSocket 帧 |
| Paw / Proxyman / HTTP Toolkit | ~40–50% | 同类客户端 + 抓包 | 生态、文档、插件 |
| Swagger UI / Redoc | ~30% | OpenAPI 导入/导出 | 文档渲染站点 |

### 7.2 补齐后可替代

| 软件 | 需要补 | 预期覆盖 |
|---|---|---|
| Mockoon / WireMock / Postman Mock | F-10 本地 Mock（延迟、失败率、动态响应） | 中小团队 80% |
| UptimeRobot / Postman Monitors / Pingdom | F-11 定时巡检 + 告警（必须说明"应用关闭即停"） | 本机常驻场景 70% |
| Charles / Fiddler（进阶） | F-08 断点/Map/过滤/帧 | 80% |
| JMeter（中小团队） | F-15 混合配比场景 | 80% |
| SoapUI | WSDL/SOAP 支持 + XML 断言（仅传统企业用户需要） | 60% |
| Apifox | 文档 + Mock + 团队 + 代码生成 | ~70%（团队协作除外） |

### 7.3 不建议替代

| 软件 | 原因 |
|---|---|
| Wireshark | L2–L4 与深度协议解析，非同一领域 |
| gRPC 客户端（BloomRPC / grpcurl） | Protobuf 反射是独立工程，最多外挂命令行 |
| Postman 云端协作 / 分享 | 需服务端与账号体系 |
| Burp / ZAP | 渗透测试与合规范畴，误报成本高 |

---

## 8. 实施阶段划分

> 仅表示顺序与依赖关系，不含时间承诺。每个阶段可独立发布、独立回滚。

### 阶段一：修短板（低风险，先建立可信度）

- O-17 拆分 `RequestEditor`（前置：O-19 测试地基）
- O-01 响应查找、O-02 大响应处理
- O-05 快捷键、O-07 错误诊断
- O-10 请求级凭据加密（v4 迁移）
- F-01 代码生成、F-02 响应生成断言、F-03 内置变量 UI
- O-08 历史保留、O-09 更新进度

**完成判据**：迁移可回滚、测试全绿、无用户可感知的既有功能回归。

### 阶段二：抓包深化（替代 Charles 的关键）

- F-08 断点 / Map Local / Map Remote / 过滤 / 规则表
- F-09 HAR 导入
- F-13 抓包会话生成测试用例（先做基础版）

**完成判据**：Chrome HAR 导入可用；断点与 Map 规则在真实 HTTPS 场景通过。

### 阶段三：服务端与协议

- F-10 Mock Server
- F-11 定时巡检 + 告警
- F-04 WebSocket、F-05 SSE
- F-06 OAuth2 完整流程、F-07 认证算法

**完成判据**：四条能力各自有独立开关与独立关闭路径，互不牵连。

### 阶段四：闭环与规模化

- F-14 Flow（串行 + 条件）
- F-12 OpenAPI 文档生成
- F-15 混合多接口压测

### 暂缓（本轮不排）

团队协作/同步、分布式压测、gRPC、Linux 桌面端、Wireshark 级抓包。

---

## 9. 风险与应对

| 风险 | 影响 | 应对 |
|---|---|---|
| 拆分 SFC 引入回归 | 高 | 先补测试（O-19）；拆分阶段禁止夹带功能改动；按组件小步提交 |
| 凭据加密导致老数据不可用 | 高 | 迁移幂等 + 密钥不匹配时跳过不写；保留"导入密钥包"恢复路径；发布前用 v3 真实备份库演练 |
| 列序号错位 | 中 | O-18 之后逐步改为按列名/结构体映射；本轮新增列一律追加到末尾并同步序号 |
| Mock 端口冲突/被防火墙拦 | 中 | 端口预检、仅监听本机、错误码 `E_PORT` |
| 定时巡检在桌面端"假在线" | 中 | UI 明确"应用关闭即停止"；后续再做系统计划任务 |
| WebSocket 高频事件拖慢 UI | 中 | 批量推送 + 帧上限 + 落盘 |
| Flow 复杂度失控 | 中 | 第一版仅串行 + 条件；并行/循环需单独评审 |
| 主题回归漏检 | 中 | 截图回归纳入 CI 门禁 |
| CRON 表达式误配 | 低 | 保存前校验并给出下一次触发时间预览 |

---

## 10. 附录

### 10.1 术语

| 术语 | 说明 |
|---|---|
| AAD | AES-GCM 附加认证数据，本项目绑定"表名:列名"防止密文搬运 |
| 判存式补列 | 执行 `ALTER` 前先 `PRAGMA table_info` 判断列是否存在 |
| Body 草稿 | 按 Body 类型分别保存的内容，存于 `requests.body_meta` |
| 就近覆盖 | 变量/认证/脚本从请求向上查找到最近定义的集合节点 |

### 10.2 参考文件索引

| 文件 | 用途 |
|---|---|
| [db/mod.rs](file:///d:/Software/ProjectFiles/AstroMan/src-tauri/src/db/mod.rs) | 结构版本与判存式迁移 |
| [db/schema.sql](file:///d:/Software/ProjectFiles/AstroMan/src-tauri/src/db/schema.sql) | 表结构与索引 |
| [secret.rs](file:///d:/Software/ProjectFiles/AstroMan/src-tauri/src/secret.rs) | 主密钥、AES-GCM、AAD、口令模式 |
| [util.rs](file:///d:/Software/ProjectFiles/AstroMan/src-tauri/src/util.rs) | 变量渲染、内置变量、占位符收集 |
| [commands/request.rs](file:///d:/Software/ProjectFiles/AstroMan/src-tauri/src/commands/request.rs) | 请求详情读写与列序号 |
| [commands/oauth.rs](file:///d:/Software/ProjectFiles/AstroMan/src-tauri/src/commands/oauth.rs) | 当前仅 Client Credentials |
| [commands/importer.rs](file:///d:/Software/ProjectFiles/AstroMan/src-tauri/src/commands/importer.rs) | Postman/OpenAPI/HAR 导入 |
| [commands/update.rs](file:///d:/Software/ProjectFiles/AstroMan/src-tauri/src/commands/update.rs) | 更新检查与下载 |
| [CodeArea.vue](file:///d:/Software/ProjectFiles/AstroMan/src/components/CodeArea.vue) | 编辑器与着色层 |
| [stores/tabs.ts](file:///d:/Software/ProjectFiles/AstroMan/src/stores/tabs.ts) | 标签上限与持久化 |
| [RequestEditor.vue](file:///d:/Software/ProjectFiles/AstroMan/src/views/RequestEditor.vue) | 待拆分主组件 |
| [docs/roadmap.md](file:///d:/Software/ProjectFiles/AstroMan/docs/roadmap.md) | 既有规划口径 |

### 10.3 评审待确认项

1. 优先级是否认可"抓包深化优先于 Mock/定时"？
2. 请求级凭据加密是否接受"字段级加密 + 启动迁移"（而非整块加密）？
3. 定时巡检是否接受"应用关闭即停"的产品口径？
4. 集合级变量/Auth 的继承规则是否按本文 3.3 执行？
5. Flow 第一版是否认可"仅串行 + 条件"的收敛范围？
