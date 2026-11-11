
> 状态：待评审（仅方案，不含实现代码）
> 覆盖范围：单接口 / 子集合 / 整个项目三个层级的接口文档导出；接口请求 / 返回实体（Schema）的自动推断、编辑与文档输出
> 原则：不承诺工期，阶段划分只表示先后顺序
> 修订 v2（2026-11-06）：脱敏改为值级替换并扩大凭据来源；草稿改为「后端缓存 IR + 前端稀疏覆盖层」；示例值与所选环境对齐；三个命令统一异步；修正 `DocsOptions` 默认值、PDF 跨卷落地、超时与打印折叠等工程细节
> 修订 v3（2026-11-06）：新增「接口实体」——请求体 / 响应体 JSON 自动推断字段与类型、字段说明由用户回填、实体随接口保存并进入文档；响应示例（最近一次成功运行）改为默认开启；单接口文档采用独立版式（不再套集合标题）；实体列需 `DB_VERSION` 8 → 9，原描述字段顺延到 v10

---

## 0. 结论摘要

### 0.1 推荐方案

不要让三个入口各写一套导出逻辑，而是先做一份与渲染无关的中间模型（下称 IR），再由不同渲染器输出：

```
SQLite（projects / collections / requests / environments）
        │  scope = request | collection | project
        ▼
   docs::builder  ──►  DocBundle（IR：树结构 + 规范化请求 + 实体字段 + 脱敏后的值）
        │
        ▼
   DocsCache（draft_id）  ◄── DocEditOverlay（前端只回传改过的字段）
        │
        ├─► Markdown 渲染器   → *.md
        ├─► HTML 渲染器       → 自包含 *.html（PDF 的唯一排版来源）
        ├─► PDF 渲染器        → HTML 交给系统 Edge/Chrome 无头打印 → *.pdf
        └─► OpenAPI 渲染器    → *.openapi.json
```

接口实体（请求 / 返回字段树）在发送请求时由后端自动推断并合并进 `requests.request_entity / response_entity`，在接口详情页内编辑；`builder` 只读取已保存的实体，不在导出时重新访问网络。实体是文档的**结构化补充**：类型自动识别，字段说明由用户回填，空白说明不阻断导出。

三个层级、四种格式最终收敛为「准备 → 预览 → 导出」三个 Rust 命令，前端只多一个统一的「预览后导出」抽屉。

### 0.2 推荐输出格式

| 格式 | 定位 | 建议 |
|---|---|---|
| Markdown | 进 Git、进 Wiki、给 AI 读；纯文本可 diff | **默认格式** |
| 独立 HTML | 单文件分享、可离线打开；同时作为 PDF 的排版来源 | **主力交付格式** |
| OpenAPI 3.0 | 给 Apifox / Swagger / Mock / SDK 生成器消费 | 复用已有实现，扩展到集合/单接口 |
| PDF | 对外交付、归档、打印 | **直接生成 PDF**：HTML 交给系统 Edge/Chrome 无头 `--print-to-pdf`，不新增 Rust 依赖 |

理由：项目已有自包含 HTML 报告先例（[report_builder.rs](file:///d:/Software/ProjectFiles/AstroMan/src-tauri/src/report_builder.rs#L1-L13)），因此把 **HTML 模板作为唯一排版来源**：同一份 HTML 既能独立分享，又能无头打印成 PDF，Markdown / HTML / PDF 三者内容天然一致。

不引入 PDF 渲染 crate（`printpdf` / `genpdf`）的原因：中文字体得随包内置，A4 分页、表格跨页、页眉页脚都要自己排版，安装包增加 5–10MB，且版式与 HTML 预览无法对齐；Windows 10/11 自带 Edge（Chromium 内核），复用它的排版与中文字体能力，成本最低。未检测到 Edge/Chrome 时降级为「导出 HTML + 提示用浏览器打印」，不阻断导出。

### 0.3 明确不做 / 后置

- 不做文档站点托管、在线同步、团队共享（需要服务端与账号体系，和现有本地优先定位冲突）
- 不做文档版本历史与增量 diff（导出是一次性产物，版本管理交给 Git）
- 不做全局实体库与跨接口复用：实体只随所属接口保存，仅在接口详情与本文档导出中使用，不提供「共享数据模型 / 引用其它接口实体」
- 不做实体字段的取值校验与运行时校验：实体描述的是「字段长什么样」，不是断言；不参与发送失败判定
- 不做定时自动导出（CLI 侧后置）
- 不内置 PDF 排版引擎、不打包 Chromium / wkhtmltopdf（PDF 由系统 Edge/Chrome 无头渲染，见 5.7）
- 不做任意 HTML/富文本编辑：预览里的编辑只改结构化字段（标题、说明、示例值、收录开关、顺序），产物永远由后端渲染器生成，避免前端注入

---

## 1. 现状盘点与约束

### 1.1 可直接复用的数据与能力

| 能力 | 现状 | 位置 |
|---|---|---|
| 项目 | `id / name / description / color / sort_order` | [schema.sql](file:///d:/Software/ProjectFiles/AstroMan/src-tauri/src/db/schema.sql#L11-L19) |
| 集合 | 支持 `parent_id` 无限嵌套，`collection_tree` 已能构建树 | [schema.sql](file:///d:/Software/ProjectFiles/AstroMan/src-tauri/src/db/schema.sql#L22-L30)、[collection.rs](file:///d:/Software/ProjectFiles/AstroMan/src-tauri/src/commands/collection.rs#L44-L65) |
| 请求 | `name / method / url / headers / params / body_type / body / body_meta / auth / pre_script / post_script / assertions / sort_order`；阶段一 C 增加 `request_entity / response_entity`（JSON 文本） | [schema.sql](file:///d:/Software/ProjectFiles/AstroMan/src-tauri/src/db/schema.sql#L33-L52)、[models.rs](file:///d:/Software/ProjectFiles/AstroMan/src-tauri/src/models.rs#L106-L136) |
| 请求读取 | `find_request` 已统一解密 `auth` | [request.rs](file:///d:/Software/ProjectFiles/AstroMan/src-tauri/src/commands/request.rs#L51-L63) |
| 发送与运行记录 | 手工发送后可拿到实际请求体与响应体；`record_run` 已落运行快照，可作为实体推断的旁路来源 | [http.rs](file:///d:/Software/ProjectFiles/AstroMan/src-tauri/src/commands/http.rs)、[run.rs](file:///d:/Software/ProjectFiles/AstroMan/src-tauri/src/commands/run.rs) |
| 环境变量 | `is_secret` 敏感值、环境脚本 | [transfer.rs](file:///d:/Software/ProjectFiles/AstroMan/src-tauri/src/commands/transfer.rs#L164-L223) |
| OpenAPI 导出 | 已有项目级导出与 path / body / 重复路径处理 | [transfer.rs](file:///d:/Software/ProjectFiles/AstroMan/src-tauri/src/commands/transfer.rs#L455-L712) |
| 文件写入 | `file_write_text` 自动建目录、`file_exists` 覆盖提示 | [files.rs](file:///d:/Software/ProjectFiles/AstroMan/src-tauri/src/commands/files.rs#L18-L38) |
| Markdown | 前端已有 `markdown-it`，仅用于页面渲染，不参与导出 | [package.json](file:///d:/Software/ProjectFiles/AstroMan/package.json#L19-L31) |

### 1.2 必须遵守的约束

1. **`requests` 与 `collections` 没有 `description` 字段**：文档的「接口说明」「集合说明」目前只能取 `name`。阶段一 A/B 不为此改库；阶段一 C 先为实体补列（`DB_VERSION` 8 → 9），原计划的描述字段与 `request_runs.environment_id` 顺延到阶段二（9 → 10）。两次变更都必须同时写进 `schema.sql` 与 `COLUMN_MIGRATIONS`，见 [db/mod.rs](file:///d:/Software/ProjectFiles/AstroMan/src-tauri/src/db/mod.rs#L6-L15) 与 [db/mod.rs](file:///d:/Software/ProjectFiles/AstroMan/src-tauri/src/db/mod.rs#L196-L260)。
2. **`map_detail` 按列序号取值**：任何新增列若进入 `SELECT` 顺序，必须同步修改序号，否则会静默错列，见 [request.rs](file:///d:/Software/ProjectFiles/AstroMan/src-tauri/src/commands/request.rs#L24-L49)。实体列追加在 `SELECT` 末尾，序号顺延；补列必须走 `add_column_if_missing` 判存，不能吞掉「库只读 / 表不存在」等真实错误。
3. **凭据已加密落库**：`auth` 在库里是密文，读取时由 `find_request` 解密。文档导出属于「跨边界输出」，默认必须脱敏，不能直接把解密后的 `token / password / clientSecret` 写进文档。
4. **不带 `id` 的请求无法被定位**：三个 scope 都以数据库主键定位，前端入口必须保证 `form.id` 已存在（未保存请求先保存，或按「本次编辑内容」内存导出，后者后置）。
5. **现有导出入口不能回退**：`openapi_export(project_id, path)` 仍被 [api/index.ts](file:///d:/Software/ProjectFiles/AstroMan/src/api/index.ts#L293-L295) 与 AI 工具调用，改造时必须保留该命令或保持其输出兼容。

---

## 2. 三级导出的范围定义

| 层级 | scope | targetId | 包含内容 | 递归规则 |
|---|---|---|---|---|
| 单个接口 | `request` | `requests.id` | 该请求本身 | 无 |
| 子集合 | `collection` | `collections.id` | 该集合 + 其所有后代集合 + 所有请求 | 深度优先，`sort_order ASC, id ASC` |
| 整个项目 | `project` | `projects.id` | 项目下全部集合与请求 + 环境变量表 | 同上 |

补充规则：

- **导出必须带环境（可为空）**：`environmentId` 决定变量解析。单接口导出默认沿用编辑器当前选中的环境；集合 / 项目导出由用户在预览面板里选择。为空时保留 `{{变量}}` 原样并记 warning，不猜测。
- **文档里的 URL 是「解析后的真实地址」**：选择环境后，`{{baseUrl}}/api/v1/login` 输出为 `https://api.example.com/api/v1/login`；未解析成功的占位符原样保留并高亮提示。
- **集合是「子树」而非「单层」**：导出父集合必须包含所有子集合，否则用户需要逐个导出。
- **空集合**：项目级导出跳过完全空的子树（避免文档出现大量空标题），但**集合级导出必须保留目标集合本身**，即使它只有子集合没有请求，并在文档中显示「（该集合暂无接口）」。
- **请求顺序**：严格沿用界面顺序（`sort_order ASC, id ASC`），不按方法或 URL 重排，保证文档与工作区所见一致。
- **跨层级路径**：集合级与项目级文档中，每个接口都标出其完整集合路径（如 `用户中心 / 认证 / 登录`），避免同名接口无法区分。
- **请求级文档**：仍显示所属集合路径与项目名，便于脱离项目单独分享时保留上下文。

---

## 3. 文档信息架构

### 3.1 单接口文档

````markdown
# 用户登录

> 项目：开放平台 · 集合：用户中心 / 认证
> 环境：测试环境 · 生成日期：2026-11-06 · AstroMan 1.1.0
> 说明：本文档中的地址已按所选环境解析为真实地址；示例值来源标注在表格中。
> 本文档含请求 / 返回实体，字段说明由项目维护者填写，空白表示未填写。

## 基本信息
- 方法：POST
- 地址：https://api.example.com/api/v1/auth/login
- 认证：Bearer Token（值已隐藏）
- 请求格式：application/json

## 请求参数
| 名称 | 位置 | 必填 | 类型 | 示例 | 说明 | 示例来源 |
|---|---|---|---|---|---|---|
| username | body | 是 | string | zhangsan | 登录账号 | 上次运行 |
| password | body | 是 | string | •••••• | 登录密码 | — |
| clientId | query | 否 | string | web | 客户端标识 | 自动生成 |

## 请求头
| Header | 值 |
（跳过 Host / Content-Length / Cookie，Authorization 显示为「已隐藏」）

## 请求体
（JSON 美化后输出；表单输出字段表；二进制标注类型）

## 请求实体
| 字段 | 类型 | 必填 | 说明 |
|---|---|---|---|
| username | string | 是 | 登录账号 |
| password | string | 是 | 登录密码 |
| rememberMe | boolean | 否 | 下次自动登录 |
| profile.nickname | string | 否 | 昵称 |

## 返回实体
| 字段 | 类型 | 必填 | 说明 |
|---|---|---|---|
| code | number | 是 | 业务状态码，0 表示成功 |
| data.token | string | 是 | 访问令牌（示例值已隐藏） |
| data.user.id | number | 是 | 用户 ID |
| data.user.roles[] | string | 否 | 角色列表 |

## 响应示例
状态码：200 OK
**响应头**（已脱敏）
| Header | 值 |
|---|---|
| content-type | application/json |
| set-cookie | •••••• |
**响应体**
```json
{ "code": 0, "data": { "token": "••••••", "user": { "id": 10001 } } }
```

## 测试断言
（开启 includeAssertions 时输出 assertions 原文）
````

- 单接口文档**不再套集合标题**：接口直接作为 H1，正文各节使用 H2，避免出现「一级标题是集合、二级标题才是接口」的层级倒挂。
- 请求实体 / 返回实体只在实体存在且非空时输出；两者都为空时整节省略，不输出空表。
- 实体表格中的「字段」用点号表示嵌套路径，数组元素使用 `[]` 后缀；说明列为空时保留空白单元格，不填「暂无说明」之类的占位噪音。
- 响应示例来自最近一次成功运行（host 与环境一致、30 天内），响应头与响应体均已按 4.2.1 脱敏，响应体超过 64 KB 时截断并标注。

### 3.2 集合 / 项目文档

按集合树组织，集合标题层级与树深度对齐：

```markdown
# 开放平台接口文档

## 用户中心
### 认证
#### 登录
- `POST /api/v1/auth/login` — 用户名密码换取 Token
（接口详情折叠为统一小节）
### 用户
...
```

每个接口详情在「请求参数 / 请求头 / 请求体」之后追加「请求实体 / 返回实体」两张字段表（存在且非空时才输出），字段类型与必填自动识别，说明来自用户回填；再往后是「响应示例」。集合级与项目级文档的接口数量多，实体表默认**只输出字段名、类型、必填与说明**，不重复输出完整请求体原文（请求体仍按 64 KB 截断输出一次）。

HTML 版本额外提供：顶部目录、按集合分组锚点、每个接口前分页打印（`page-break-before: always`）、`<details>` 折叠长请求体。**不引入任何 JS**，避免分享后被安全策略拦截或引入 XSS 面。实体表与其它表格共用同一套 `escape_html`，字段名中的 `<` / `&` 不会破坏结构。

---

## 4. 数据采集与敏感信息处理

### 4.1 采集字段

| 文档字段 | 来源 | 处理 |
|---|---|---|
| 名称 | `requests.name` | 去首尾空白，空值兜底「未命名请求」（与保存逻辑一致，见 [request.rs](file:///d:/Software/ProjectFiles/AstroMan/src-tauri/src/commands/request.rs#L171-L177)） |
| 方法 / URL | `requests.method / url` | URL 按 4.4 做环境解析；未解析的 `{{变量}}` 保留原样并记 warning |
| Path | 复用 `openapi_path` | 仅用于 OpenAPI 与文档标题的路径展示 |
| Query | `requests.params` | 只取 `enabled = true` 且 `key` 非空；值命中 4.2 敏感特征时掩码 |
| Headers | `requests.headers` | 同上；额外过滤 `host / content-length`，`authorization` 转为认证小节；`cookie / set-cookie / x-api-key / api-key / proxy-authorization / *-token / *-secret` 的值一律掩码 |
| Body | `body_type / body / body_meta` | 当前类型正文；`body_meta` 中的其它类型草稿**不输出**，避免把临时草稿当正式文档 |
| 请求实体 | `requests.request_entity` | 字段树（名称 / 类型 / 必填 / 说明）；只输出字段结构与说明，**不保存也不输出任何字段值**；说明文本同样过 4.2.1 的敏感子串替换 |
| 返回实体 | `requests.response_entity` | 同上；响应头 / 响应体中的真实值不进入实体，只保留字段形状 |
| 响应示例 | `request_runs` 最近一次成功运行 | 状态码 / 状态文本 / 响应头 / 响应体；头与体按 4.2.1 脱敏后输出，体超过 64 KB 截断 |
| Auth | `requests.auth`（已解密） | 只输出 `kind / addTo / 参数名`；令牌类值一律掩码，`clientId` 属非密标识、原样输出（与 4.2 口径一致） |
| 脚本 / 断言 | `pre_script / post_script / assertions` | 默认不输出，选项开启后输出 |
| 环境变量 | `environments` + `env_variables` | 默认输出变量表；`is_secret = 1` 的值掩码 |

表中的掩码都在**字段产出阶段**完成，不依赖最终渲染文本；实现口径见 4.2.1。

### 4.2 默认脱敏规则（不可关闭）

| 场景 | 输出 |
|---|---|
| `auth.kind = bearer` | `Authorization: Bearer ••••••`（不输出 token 任何前缀/后缀） |
| `auth.kind = basic` | `Authorization: Basic ••••••`（用户名也视为敏感，整体隐藏） |
| `auth.kind = apikey` | 输出 `key` 名称，值输出 `••••••` |
| `auth.kind = oauth2` | 输出 `tokenUrl / scope`，`clientId` 原样、`clientSecret` 掩码 |
| `Cookie` 头 | 整个值替换为 `••••••`（与其它掩码一致） |
| 敏感环境变量 | 值替换为 `••••••`；变量名保留 |
| 手工添加的认证类头（`x-api-key / api-key / proxy-authorization / *-token / *-secret`） | 值替换为 `••••••`，头名保留 |
| 名称命中 `token / password / passwd / secret / key / credential` 的 query / form / header 参数 | 值替换为 `••••••`，与来源无关（定义值、运行记录、自动生成一律掩码） |
| URL userinfo（`https://user:pass@host/...`） | 输出 `https://••••••@host/...` |
| 敏感值清单中的任意值在 URL / 头 / Body / 脚本 / 断言中以**子串**形式出现 | 替换为 `••••••`（按长度降序替换，避免短值先替换把长值切碎） |

名称匹配规则：大小写不敏感，按 `_ / - / camelCase` 分词后做整词比较——`api_key / apiKey / accessToken` 命中，`keyword / tokenizer` 不命中；`*-token / *-secret` 指头名以 `-token / -secret` 结尾。

脱敏针对的是**值**，不是字段：`auth` 里的令牌被复制到 Body 或其它头里再次出现，同样会被掩码。`clientId` 是 OAuth2 的公开标识，按非密字段输出；如果后续有用户要求连它一起隐藏，再单独加配置，阶段一不做。

### 4.2.1 替换时机与实现口径

- **在字段产出阶段替换，不在渲染后的文本上替换**：`render_tracked` 解析出一个字段的明文后，立即把命中片段替换成 `••••••`，再放进 `DocRequest`。渲染后替换会漏掉三种情况：HTML 转义把 `& / < / > / "` 变成实体、JSON 美化把 `" / \` 转义、变量被拼成 `prefix-{{token}}-suffix` 后整值不再相等。
- **敏感值清单**统一收集、去重、按长度降序：
  1. `render_tracked` 收集的 `used_secrets`；
  2. 所选环境中 `is_secret = 1` 的变量值；
  3. `auth` 解出的令牌类值（bearer token / basic 口令 / apiKey 值 / oauth2 clientSecret）；
  4. 名称命中敏感特征的头与参数值（见 4.2）。
- 长度 ≥ 4 的敏感值参与子串替换；短于 4 的只做整值等值替换，避免误伤 `1`、`true` 之类。命中敏感名称的字段不受长度限制，整值掩码。
- JSON Body 优先在**解析后的字符串值**上替换，再格式化输出；只有非 JSON 的纯文本 Body 才整体替换。否则一个恰好等于凭据的普通词可能把 JSON 的键或结构改坏。
- 掩码统一为 `••••••`，不保留首尾字符、不输出长度。
- 脚本 / 断言默认不输出；开启时同样先过清单再输出。
- 实体字段说明是自由文本，可能被用户粘贴进真实令牌：输出前同样过 4.2.1 的敏感值子串替换；字段名与类型本身不是凭据，原样输出（`password` 这类字段名保留，值不会进实体）。
- 已知取舍：按值替换可能把正文里恰好等于凭据的普通文本一起掩码。宁可多掩码，不可漏。

### 4.3 不输出真实凭据的理由

项目级 JSON 导出（[transfer.rs](file:///d:/Software/ProjectFiles/AstroMan/src-tauri/src/commands/transfer.rs#L196-L204)）采用「换机搬迁」语义，必须还原明文；而接口文档的常见去向是发同事、进 Git、给外部联调，两者语义不同，不能复用同一套导出策略。若未来确有内网需要，再单独加「包含真实凭据」开关并置于二次确认之后，默认关闭。

### 4.4 环境变量解析（导出即真实地址）

目标：文档里不再出现 `{{baseUrl}}`、`{{version}}` 这类占位符，而是用户选定环境下的真实值。

- **复用现有渲染器**：直接调用 [`util::render_tracked`](file:///d:/Software/ProjectFiles/AstroMan/src-tauri/src/util.rs#L124-L145)，与发送请求时的替换规则完全一致（自定义变量优先、内置变量、`{{secret:NAME}}` 语义都保持一致），避免「文档地址和实际请求地址不一致」。
- **解析范围**：`url`、`params.value`、`headers.value`、`body`（JSON / form / multipart 的文本字段）、`assertions`（开启时）。`pre_script / post_script` 默认不输出，开启脚本时同样解析。
- **变量来源**：所选环境的全部变量，加上内置变量（`{{$timestamp}}` 等）。不合并运行时变量，保证同一次导出可复现；需要固定时间类变量的，统一用导出时刻。
- **敏感值仍掩码**：`{{secret:NAME}}` 由 `render_tracked` 收集进 `used_secrets`；环境里 `is_secret = 1` 的普通 `{{token}}` 不进入该清单，由 `builder` 放进 4.2.1 的敏感值清单。两者都在**字段产出阶段**替换，不允许「渲染完整文档后再做字符串替换」，原因见 4.2.1。
- **未解析项**：保持 `{{name}}` 原样，并在 `warnings` 记录「N 处变量未解析」，预览面板对应字段标黄；不阻断导出。
- **环境为空**：整份文档不做替换，等同于「模板文档」模式，此时不猜默认环境。

### 4.5 示例值取值链

请求参数、Path 参数、Body 字段的「示例值」按以下优先级取值，逐字段独立判断：

| 优先级 | 来源 | 说明 |
|---|---|---|
| 1 | 用户在预览面板手动编辑 | 最高优先级，编辑结果保存在覆盖层里，导出时使用；命中敏感名称的字段仍强制走 4.2.1 掩码 |
| 2 | 请求定义里的非空值 | `params.value` / `headers.value` / 原始 Body 中已有值，且不是未解析占位符；命中 4.2 敏感特征的字段不取定义值，直接输出 `••••••` |
| 3 | **最近一条运行记录** | 在「host 与环境一致、30 天内」的记录里取该接口最新一条；优先 `error IS NULL AND response_status > 0` 的成功记录，没有成功记录时退回最新一条 |
| 4 | **自动生成** | 运行记录不存在或字段缺失时，按参数名/类型确定性生成，并在文档中标注「示例值（自动生成）」 |

运行记录读取说明：

- 表结构见 [schema.sql](file:///d:/Software/ProjectFiles/AstroMan/src-tauri/src/db/schema.sql#L197-L219)，每接口仅保留最近 200 条；读取方式可直接走 [run_list](file:///d:/Software/ProjectFiles/AstroMan/src-tauri/src/commands/run.rs#L120-L178) 或 `run_get`。为了取「每个接口最新一条」而在循环里逐接口查询会形成 N+1，批量场景用一条 `ROW_NUMBER() OVER (PARTITION BY request_id ORDER BY created_at DESC)` 查询。
- **只取与所选环境一致的记录**：解析运行记录 `url` 的 host，与当前环境解析后的 host 比对；不一致时不取该记录，退回自动生成并记 warning。`request_runs` 目前没有 `environment_id`，阶段一按 host 近似判断；阶段二补列后改为精确匹配（`SendRequestPayload.env_id` 已存在，落库成本低）。
- **只取最近 30 天内的记录**（由 `DocsOptions.runExamplesMaxAgeDays` 调整，0 表示不限制），过期记录不取。
- **环境为空（模板模式）时一律不取运行记录**，避免把某个环境的真实数据混进模板文档。
- Query 示例：优先解析运行记录 `url` 的查询串，缺失时读 `params` 快照。
- Path 示例：把 `requests.url` 模板与运行记录 `url` 按 `/` 分段对齐；段数一致且模板段为 `{{...}}` 时取实际段，段数不一致则退回自动生成（不硬猜）。
- Body 示例：取运行记录 `body` 的实际发送内容，按 `body_type` 美化；`request_body_truncated = 1` 时追加「（已截断）」。
- **不取运行记录的请求头**：其中可能含真实 Cookie / Authorization，头信息始终以请求定义 + 脱敏规则为准。
- 只有在 host 与环境一致时，运行记录里已解析的真实域名才会进入文档（符合「真实地址」诉求）；其中命中的敏感值仍按 4.2.1 替换。

自动生成规则（确定性，相同接口反复导出结果稳定）：

| 参数特征 | 生成值 |
|---|---|
| 名称含 `id` / `Id` / `no`，或类型为整数 | `10001`（同请求内递增：10001、10002…） |
| `page` / `offset` | `1` |
| `size` / `limit` / `pageSize` | `10` |
| 名称含 `email` | `user10001@example.com` |
| 名称含 `phone` / `mobile` | `13800138000` |
| 名称含 `date`（非 datetime） | `2026-01-01` |
| 名称含 `time` / `datetime` | `2026-01-01T12:00:00+08:00` |
| 名称含 `name` / `userName` | `张三` |
| 名称含 `price` / `amount` / `total` | `99.90` |
| 布尔型 | `true` |
| 名称含 `uuid` / `guid` | 由种子生成的固定 UUID 形态字符串 |
| 其它字符串 | `示例值1`、`示例值2`… |
| 名称命中 `token` / `password` / `secret` / `key` | 不生成真实值，输出 `••••••` |

种子取 `request_id × 131 + 字段序号 × 17`，用简单 LCG 生成，保证同一接口重复导出结果一致，也便于单测断言；不使用系统随机，避免每次导出 diff 抖动。

接口实体**不提供示例值**：实体只描述字段结构与说明，示例值仍按上表取值；实体字段与 Body 字段同名时是两条独立信息（实体表输出「字段 / 类型 / 必填 / 说明」，请求参数表输出「名称 / 位置 / 必填 / 类型 / 示例 / 说明 / 来源」）。

---

## 5. 后端设计

### 5.1 模块划分（新增）

```
src-tauri/src/docs/
├─ mod.rs            # 对外命令：docs_prepare / docs_render / docs_export
├─ model.rs          # DocBundle / DocOutline / DocEditOverlay / 结果与错误
├─ builder.rs        # scope + 环境解析 + SQL 读取 → DocBundle（唯一碰 DB 的地方）
├─ examples.rs       # 示例值取值链：定义值 → 运行记录 → 确定性生成
├─ overlay.rs        # 覆盖层字段定位、稳定 key、编辑合并
├─ cache.rs          # draft_id → 缓存 IR（TTL / LRU / revision 校验）
├─ render_markdown.rs
├─ render_html.rs
├─ render_pdf.rs     # Edge/Chrome 无头打印 + 临时文件与清理
└─ render_openapi.rs # 阶段二：从 transfer.rs 抽出，供两个命令共用

src-tauri/src/entity/
├─ mod.rs            # 命令：request_entity_generate / request_entities_get
├─ infer.rs          # JSON / 表单值 → 字段树（类型识别、数组元素、深度 / 节点上限）
├─ merge.rs          # 新旧字段树合并：保说明、保必填、保留消失字段、保留手工字段
└─ model.rs          # EntityDoc / EntityNode（序列化到请求实体列）
```

`entity` 与 `docs` 解耦：实体只负责「字段结构 + 说明」，不感知文档格式；`docs::builder` 读取实体列后转换为 `DocEntity`，渲染器不直接访问实体模块。

渲染器是**纯函数**（`&DocBundle → String / Vec<u8>`），不读库、不读环境、不写文件（`render_pdf` 只做「HTML → PDF」的外部进程编排），便于单测与快照。

Markdown 渲染器必须做自己的转义：表格单元格里的 `|` 转成 `\|`、换行转成 `<br>`、行首的 `#` 与列表符号转义、标题里的换行压成空格。HTML 的 `escape_html` 解决不了 Markdown 结构被破坏的问题（例如接口名带 `|` 会把整张参数表切乱），两者不能互相替代。

### 5.2 IR 定义

```rust
pub struct DocBundle {
    pub generated_at: i64,
    pub app_version: String,
    pub revision: String,             // 源数据摘要（集合/请求数量 + 最大 id / 更新时间），用于校验覆盖层是否失配
    pub scope: DocScopeInfo,          // kind: request/collection/project, title, project_name
    pub groups: Vec<DocGroup>,        // 根集合（请求级导出时也包一层所属集合）
    pub environments: Vec<DocEnvironment>, // includeEnvironment 时填充
    pub warnings: Vec<String>,        // 跳过项、截断项、重复路径
}

pub struct DocGroup {
    pub id: i64,
    pub name: String,
    pub path: Vec<String>,            // ["用户中心", "认证"]
    pub requests: Vec<DocRequest>,
    pub children: Vec<DocGroup>,
}

pub struct DocRequest {
    pub id: i64,
    pub name: String,
    pub method: String,
    pub url: String,
    pub path: String,
    pub query: Vec<DocParam>,
    pub headers: Vec<DocParam>,
    pub auth: DocAuth,
    pub body: DocBody,
    pub request_entity: Option<DocEntity>,   // 请求体字段树（无实体时不输出该节）
    pub response_entity: Option<DocEntity>,  // 返回体字段树
    pub response: Option<DocResponse>,       // 最近一次成功运行的响应示例
    pub scripts: Option<DocScripts>,
    pub assertions: Option<String>,
}

pub struct DocParam {
    pub name: String,
    pub value: String,
    pub required: bool,
    pub location: String,
    pub example_source: String,       // defined | run | auto | manual，对应大纲里的 source
}
pub struct DocAuth { pub kind: String, pub add_to: String, pub key_name: String, pub note: String }
pub enum DocBody { None, Json(String), Text(String), Form(Vec<DocParam>), Multipart(Vec<DocParam>), Binary(String) }

/// 字段树：只描述结构，不携带任何字段值
pub struct DocEntity {
    pub fields: Vec<DocEntityField>,  // 深度优先展开后的扁平列表，渲染器无需递归
}
pub struct DocEntityField {
    pub name: String,                 // 相对根节点的路径，如 data.user.roles[]（数组元素带 []）
    pub type_name: String,            // object | array | string | number | boolean | null | file | unknown
    pub required: bool,               // 推断口径见 13.3；用户可在详情页改写
    pub description: String,          // 用户回填；空字符串保持空白
    pub stale: bool,                  // 最近一次发送未出现；文档在说明后追加「（最近一次发送未出现）」
    pub depth: u16,                   // 从 0 开始的层级，用于渲染缩进 / 排序
}

/// 最近一次成功运行的响应示例（响应头与响应体均已脱敏、截断）
pub struct DocResponse {
    pub status: i64,
    pub status_text: String,
    pub headers: Vec<DocParam>,
    pub body: String,
    pub truncated: bool,
}
```

`DocEntity` 在 IR 里已经是**扁平有序列表**：渲染器按 `depth` 缩进输出表格，不需要理解嵌套结构；实体推断侧的树形结构只在 `entity::infer` 内部存在，进入 IR 前拍平。这样 Markdown / HTML / PDF / OpenAPI 四个渲染器共享同一份实体视图。

每个节点额外携带稳定 `key`（规则见 5.2.1），前端覆盖层按 `key` 回填，删除或调整顺序不会让后面的 key 整体位移。

IR 一旦固定，Markdown / HTML / PDF / OpenAPI 的差异都只影响渲染器，后续加「Postman 集合导出」不需要再碰数据采集。

`generated_at` 默认只格式化为**日期**（如 `2026-11-06`）；只有 `DocsOptions.includeGeneratedAt = true` 时才带时分。否则同一份内容每次导出都会因时间戳不同而产生 diff，和「Markdown 进 Git 可 diff」的定位冲突。

### 5.2.1 草稿：后端缓存 IR + 前端稀疏覆盖层

**不能把整份 IR 交给前端往返。** 5000 个请求、单 Body 上限 64 KB 时，整份 `DocBundle` 的 JSON 可达数百 MB；300 ms 防抖会反复搬运，`localStorage` 只有约 5 MB，项目级草稿也存不下。改为两层：

1. **IR 留在后端**：`docs_prepare` 构建 IR 后放进 `AppState` 的 `DocsCache`（`Mutex<HashMap<draft_id, CachedDraft>>`），`draft_id` 为随机 UUID；缓存 TTL 30 分钟、最多 3 份、LRU 淘汰。IR 里只有掩码后的值，缓存本身不含明文凭据。
2. **前端只拿大纲与覆盖层**：`docs_prepare` 返回瘦身大纲（树结构、节点名称、参数行、示例值、warning、`revision`），不含脚本 / 断言 / 完整 Body 原文；前端编辑后只回传改过的字段。

```rust
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocOutline {                 // 给编辑器用，体积与节点数相关，与正文大小无关
    pub groups: Vec<DocOutlineGroup>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocOutlineGroup {
    pub key: String,                    // g-{collection_id}
    pub name: String,
    pub path: Vec<String>,
    pub requests: Vec<DocOutlineRequest>,
    pub children: Vec<DocOutlineGroup>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocOutlineRequest {
    pub key: String,                    // r-{request_id}
    pub name: String,
    pub method: String,
    pub url: String,
    pub description: String,
    pub included: bool,
    pub params: Vec<DocOutlineField>,   // 参数 / 头 / Body 字段（名称 + 当前示例值 + 来源）
    pub warnings: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocOutlineField {
    pub key: String,                    // p-... / b-... / f-...
    pub name: String,
    pub location: String,               // query | header | body | form
    pub value: String,                  // 当前示例值（已脱敏）
    pub source: String,                 // defined | run | auto | manual
    pub sensitive: bool,                // true 时编辑框只允许写掩码，不接受真实值
}

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct DocEditOverlay {
    pub revision: String,                    // prepare 时拿到的源数据摘要（重新 prepare 后需刷新）
    #[serde(default)]
    pub meta: DocEditMeta,                   // 标题 / 副标题
    #[serde(default)]
    pub nodes: BTreeMap<String, DocNodeEdit>,// 节点 key → 说明 / 示例值
    #[serde(default)]
    pub excluded: BTreeSet<String>,          // 不收录的节点 key
    #[serde(default)]
    pub order: BTreeMap<String, Vec<String>>,// 父节点 key → 子节点顺序（仅调整过的集合）
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DocNodeEdit {
    pub description: Option<String>,
    #[serde(default)]
    pub example_values: BTreeMap<String, String>, // 字段 key → 示例值
}

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct DocEditMeta { pub title: Option<String>, pub subtitle: Option<String> }

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DocMetaDefaults { pub title: String, pub subtitle: String } // prepare 返回的默认标题
```

前端只传改过的字段，`meta` / `nodes` / `excluded` / `order` 缺省时按空处理（`#[serde(default)]`），空覆盖层就是 `{"revision": "..."}`，避免每次防抖都序列化整棵树的空壳。

**节点 key 必须与序号无关**，删一行不能让后面的 key 整体位移：

| 节点 | key 规则 |
|---|---|
| 集合 | `g-{collection_id}` |
| 请求 | `r-{request_id}` |
| 参数 / 头 | `p-{request_id}-{location}-{name}-{occurrence}`（`occurrence` 为同名项的第几个，从 1 开始） |
| Body 字段 | `b-{request_id}-{json_pointer}`（路径做 URL-safe 编码；表单 / 多部分用 `f-{request_id}-{name}-{occurrence}`） |

- 参数名在文档编辑中不可改，因此 `name + occurrence` 是稳定的；用户在预览里只改说明、示例值、收录与顺序。
- 源数据在预览期间被别处修改导致 key 失配时，忽略该编辑并记 warning，不整单失败。
- 覆盖层只含用户改动，通常几 KB；`localStorage` 按 `astroman:docs-overlay:{scope}:{targetId}:{environmentId}` 暂存。覆盖层**不保存 `draft_id`**（它属于一次 prepare 的会话），重新 prepare 后刷新 `revision`、原样复用覆盖层。
- 渲染 / 导出时后端把覆盖层合并进缓存的 IR，**不重新读库**；只有「重新生成」或切换环境才重建 IR。
- 可编辑项固定为：文档标题 / 副标题、集合说明、接口说明、请求顺序、是否收录、参数与 Body 的示例值。**Markdown / HTML 原文一律不可编辑**，产物始终由后端渲染器从 IR + 覆盖层生成。

### 5.3 命令契约

导出拆成「准备 → 预览渲染 → 导出」三步，预览与导出共用同一套渲染器：

```rust
// 1) 准备：读库 + 环境解析 + 示例取值 → 缓存 IR，返回大纲与 draftId
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocsPreparePayload {
    pub scope: String,                 // "request" | "collection" | "project"
    pub target_id: i64,
    pub environment_id: Option<i64>,   // 为空则不做变量解析，保留 {{var}}
    #[serde(default)]
    pub options: DocsOptions,
}

#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase", default)]
pub struct DocsOptions {
    pub include_environment: bool,       // 默认 true：输出环境变量表
    pub include_scripts: bool,           // 默认 false
    pub include_assertions: bool,        // 默认 false
    pub include_response_example: bool,  // 默认 true：输出最近一次成功运行的响应示例
    pub resolve_environment: bool,       // 默认 true：把变量替换为真实值
    pub use_run_examples: bool,          // 默认 true：缺示例时取最近一条运行记录
    pub auto_examples: bool,             // 默认 true：无运行记录时确定性生成示例
    pub run_examples_max_age_days: u32,  // 默认 30；0 表示不限制
    pub page_break_per_request: bool,    // 默认 true：PDF / 打印时每接口另起一页
    pub include_generated_at: bool,      // 默认 false：只输出日期；true 时输出时分
}

// 注意：不能 derive(Default)。derive 出的 bool 全是 false，与上面的默认值相反。
impl Default for DocsOptions {
    fn default() -> Self {
        Self {
            include_environment: true,
            include_scripts: false,
            include_assertions: false,
            include_response_example: true,
            resolve_environment: true,
            use_run_examples: true,
            auto_examples: true,
            run_examples_max_age_days: 30,
            page_break_per_request: true,
            include_generated_at: false,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocsPrepareResult {
    pub draft_id: String,
    pub revision: String,
    pub meta: DocMetaDefaults,      // 标题 / 副标题默认值
    pub outline: DocOutline,        // 树 + 节点 + 参数 + 示例值，不含正文原文
    pub warnings: Vec<String>,
}

// 2) 渲染：缓存 IR + 覆盖层 → Markdown / HTML 字符串（仅预览，不落盘、不读库）
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocsRenderPayload {
    pub draft_id: String,
    pub overlay: DocEditOverlay,
    pub format: String,             // markdown | html
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocsRenderResult { pub content: String, pub bytes: u64, pub warnings: Vec<String> }

// 3) 导出：既可「重新读库」，也可「用缓存 IR + 覆盖层」
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum DocsExportSource {
    Scope(DocsPreparePayload),                       // kind = "scope"：后端重新构建，丢弃未提交编辑
    Draft { draft_id: String, overlay: DocEditOverlay }, // kind = "draft"：按用户编辑结果导出
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocsExportPayload {
    pub format: String,          // "markdown" | "html" | "openapi" | "pdf"
    pub path: String,            // 目标绝对路径，由前端保存对话框提供
    pub source: DocsExportSource,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocsExportResult {
    pub path: String,
    pub format: String,
    pub scope: String,
    pub request_count: usize,
    pub collection_count: usize,
    pub bytes: u64,
    pub warnings: Vec<String>,
    pub pdf_engine: Option<String>, // "edge" | "chrome" | "none"
    pub fallback_html_path: Option<String>, // PDF 引擎不可用时的降级产物
}

#[tauri::command(async)]
pub fn docs_prepare(state: State<'_, AppState>, payload: DocsPreparePayload) -> Result<DocsPrepareResult, String>;

#[tauri::command(async)]
pub fn docs_render(state: State<'_, AppState>, payload: DocsRenderPayload) -> Result<DocsRenderResult, String>;

#[tauri::command(async)]
pub fn docs_export(state: State<'_, AppState>, payload: DocsExportPayload) -> Result<DocsExportResult, String>;
```

约定：

- **三个命令统一异步**：Tauri 的同步命令默认在主线程执行，大项目导出和 PDF 转换会直接冻住 UI。三个命令都加 `#[tauri::command(async)]`（与 `capture.rs` 现有做法一致），PDF 转换不得占用主线程。
- **三命令职责单一**：`docs_prepare` 是唯一读库入口，负责构建 IR 并写入 `DocsCache`；`docs_render` 不碰 DB，只把覆盖层合并进缓存 IR 后渲染，可被前端以 300ms 防抖高频调用；`docs_export` 要么重新读库，要么用缓存的 IR + 覆盖层，两者结果都由同一渲染器产出。
- **覆盖层只回传结构化字段**：不接受前端拼好的 Markdown/HTML，避免绕过脱敏与转义。
- **缓存失效可恢复**：`draft_id` 过期或不存在时返回 `E_DRAFT_EXPIRED`，前端自动重跑 `docs_prepare` 并用本地覆盖层继续；`revision` 不一致只记 warning，不阻断导出。
- 前端传完整输出路径，后端不做「另存为」弹窗，文件覆盖由前端用 `file_exists` 预先提示，沿用现有做法（[files.rs](file:///d:/Software/ProjectFiles/AstroMan/src-tauri/src/commands/files.rs#L17-L22)）。
- 扩展名与 `format` 不匹配时后端直接报错，不做自动改名，避免用户拿到 `.md` 里其实是 HTML。
- 错误信息统一中文，可供 UI 直接展示；新增可判定前缀：`E_SCOPE`、`E_FORMAT`、`E_SECRET_LOCKED`、`E_PDF_ENGINE`、`E_TOO_LARGE`、`E_DRAFT_EXPIRED`。
- 若主密钥处于口令模式且未解锁，`auth` 解密会失败：此时**不导出**，返回 `E_SECRET_LOCKED`，提示用户先在设置页解锁，而不是静默输出空认证信息。
- `format = pdf` 且系统无 Edge/Chrome 时：不返回硬失败，改为在同目录写出同名 `.html`，`pdfEngine = "none"`、`fallbackHtmlPath` 指向该文件，前端提示「PDF 引擎不可用，已导出 HTML，可用浏览器打印」。

### 5.3.1 接口实体命令

实体是接口详情的一部分，不进入 `docs_*` 的三段式协议；它有自己的轻量读写命令，避免为了编辑一个字段说明而重建整个文档 IR。

```rust
// 读取：打开接口详情时随 find_request 一起返回，也可单独刷新
#[tauri::command(async)]
pub fn request_entities_get(state: State<'_, AppState>, request_id: i64) -> Result<RequestEntities, String>;

// 生成 / 刷新：从当前请求体与最近一次成功响应重新推断，按规则合并
#[tauri::command(async)]
pub fn request_entity_generate(
    state: State<'_, AppState>,
    request_id: i64,
    target: String,                  // "request" | "response"
    body: Option<String>,            // 前端当前编辑中的请求体；为空时读库 / 最近运行记录
    body_type: Option<String>,       // json | form | text | ...
) -> Result<RequestEntities, String>;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestEntities {
    pub request_entity: Option<EntityDoc>,
    pub response_entity: Option<EntityDoc>,
    pub warnings: Vec<String>,   // 解析失败、字段数截断、无运行记录等
}
```

约定：

- **生成即落库**：`request_entity_generate` 合并后直接写回对应实体列（与自动同步一致），避免「生成了但没保存」；前端拿到返回值后刷新页签即可。
- **保存随 `request_save` 走**：`HttpRequestDetail` 增加 `request_entity / response_entity`（JSON 字符串），保存时与其它字段同一事务写入；不新增单独保存命令，避免两次请求之间的半保存状态。
- **自动同步不是命令**：手工发送成功后，后端在 `record_run` 旁路调用 `entity::sync_after_send`（见 5.8），best-effort 执行，失败只记日志，不影响请求结果。
- **生成是幂等的**：同样的请求体 + 同样的响应体，连续调用两次得到相同的字段树；字段顺序按「首次出现顺序」稳定排序，不按字母序重排。
- **并发安全**：发送回调与用户编辑可能交错。合并按字段路径进行、只更新类型 / 必填、不覆盖说明；后到的旧结果最多把类型改回旧值，不会清空用户说明。真正冲突时以「说明永远优先」为准。
- **错误可判定**：请求体不是 JSON / 表单、响应不是 2xx 或不是 JSON 时返回 `E_ENTITY_PARSE`（中文可读），前端只提示不弹错；字段数或深度超限返回 `E_ENTITY_LIMIT`，保留已推断的部分并在 `warnings` 标注。
- **实体不参与发送**：实体不写回 `body`，不改变请求内容；删除实体字段也不会删除请求体里的字段。

### 5.4 生成与写入流程

**准备（`docs_prepare`）**

1. 校验 `scope / targetId`，并确认目标存在；
2. `builder` 在一个只读连接内一次性读取所需的 `projects / collections / requests / environments`；`auth` 复用 `find_request` 的解密函数，不能直接读密文列（批量读取仍保持单连接）；请求实体 / 返回实体随 `requests` 同一条 `SELECT` 读取，解析失败记 warning 并跳过该实体，不阻断导出；运行记录用一条 `ROW_NUMBER() OVER (PARTITION BY request_id ...)` 查询取每个接口最新一条，避免循环查询造成 N+1；
3. 加载所选环境的变量表，按 4.4 解析 URL / 参数 / 头 / Body；敏感值进入 4.2.1 的清单；
4. 按 4.5 逐字段取示例值（定义值 → 最近运行记录 → 确定性生成），运行记录先按 host 与 30 天窗口过滤，记录 `example_source`；
5. 按 4.2.1 做**值级**脱敏与规范化（URL、Body、表头），产出 `DocBundle` + `revision` + `warnings`；
6. 写入 `DocsCache`（TTL 30 分钟、最多 3 份、LRU），返回 `draft_id` + `outline` + `warnings`。

**预览（`docs_render`）**

7. 前端只回传 `draft_id` + 覆盖层（通常几 KB）；后端把覆盖层合并进缓存 IR 后渲染 Markdown / HTML，300ms 防抖；期间不落盘、不读库。

**导出（`docs_export`）**

8. `source.kind = scope` 时重跑 1–5 并直接渲染（丢弃未提交编辑，不写缓存）；`source.kind = draft` 时取缓存 IR + 覆盖层，缓存过期返回 `E_DRAFT_EXPIRED`，前端重跑 prepare 后继续；
9. 交给对应渲染器产出字符串 / HTML；PDF 走 5.7 生成临时 PDF；大小检查（见 5.5）后写入目标路径，PDF 用 `fs::copy` + 删除落地，不用跨卷 `rename`；
10. 清理临时 HTML / 临时 PDF / 临时浏览器 profile，任何分支都不留敏感中间文件；缓存按 TTL 淘汰，应用退出时整体清空。

### 5.5 上限与截断

| 项 | 上限 | 超限行为 |
|---|---|---|
| 单次导出请求数 | 5000 | `docs_prepare` 阶段直接报错，提示按集合分批导出 |
| 预计产物大小 | HTML 20 MB / PDF 50 MB | 在 `docs_prepare` 阶段按「正文 + 示例 + 固定开销」预估，超限直接提示分批，不进入渲染后再失败 |
| 单个 Body 输出 | 64 KB | 截断并在文档中标注「（已截断）」 |
| HTML 产物 | 20 MB | 渲染后兜底检查；报错并建议改用 Markdown |
| PDF 产物 | 50 MB | 渲染后兜底检查；报错并建议按集合分批或改用 HTML |
| 预览单次渲染 | 5 MB | 预览只截断超大 Body 与示例并提示「预览已简化」，导出仍完整执行；由于不再回传整份草稿，正常项目不会触发 |
| 示例响应（已实现） | 256 KB | 与运行记录入库截断口径一致 |
| 实体推断 | 深度 12 / 节点 500 / 正文 256 KB | 保留已推断部分并记 warning，不阻断；文档只输出字段表 |

### 5.6 与现有 OpenAPI 导出的关系

阶段一不碰 [transfer.rs](file:///d:/Software/ProjectFiles/AstroMan/src-tauri/src/commands/transfer.rs#L659-L712)。阶段二把 `openapi_path / openapi_body / openapi_operation / collect_openapi_paths` 从 `transfer.rs` 抽到 `docs/render_openapi.rs`，改为接收 IR：

- `transfer::openapi_export` 保留签名，内部改为「按 project scope 构建 IR → 渲染」，输出保持向后兼容；
- `docs_export(format = openapi)` 支持 `request / collection` 两个新 scope；
- 同时补齐 `components.securitySchemes`（bearer / basic / apiKey），并按请求挂 `security`，这是现有实现缺失的部分；
- 同名 `path + method` 冲突继续沿用 `-2 / -3` 后缀，并在 `warnings` 中记录冲突，避免用户无感知。

### 5.7 PDF 生成（HTML → PDF）

**引擎选择顺序**：

1. `%ProgramFiles(x86)%\Microsoft\Edge\Application\msedge.exe` / `%ProgramFiles%\Microsoft\Edge\Application\msedge.exe`
2. `%ProgramFiles%\Google\Chrome\Application\chrome.exe` / `%ProgramFiles(x86)%\...`
3. 以上都不存在 → `pdfEngine = "none"`，降级导出 HTML（见 5.3 约定）

**执行方式**：

```text
msedge.exe --headless=new --disable-gpu --no-pdf-header-footer
           --print-to-pdf=<临时路径>.pdf
           --user-data-dir=<临时目录>
           --virtual-time-budget=8000
           <临时路径>.html
```

- HTML 由 `render_html.rs` 用 PDF 模式产出（`@page { size: A4; margin: 16mm }`，每个接口 `page-break-before` 可配，目录页单独分页）。
- **PDF 模式强制展开 `<details>`**：输出 `<details open>`，或加 `@media print { details > *:not(summary) { display: block } }`。Chromium 打印时闭合的 `details` 不会展开，否则「折叠长请求体」在 PDF 里会直接看不到；浏览器版 HTML 保留折叠。
- 临时 HTML / PDF / `user-data-dir` 放在系统临时目录。**不再假设临时目录是 ASCII**：中文用户名下 `%TEMP%` 本身就是中文，Chromium 能处理 Unicode 路径；若遇到个别引擎不接受非 ASCII 路径，回退到 `%ProgramData%\AstroMan\tmp`。
- **落地用 `fs::copy` + `fs::remove_file`，不用 `fs::rename`**：临时目录通常在 C 盘、目标可能在 D 盘，跨卷 rename 会失败。
- 超时按规模放大：`30s + 1s × min(request_count / 10, 150)`，上限 180s。超时或退出码非 0 时杀掉进程并清理临时目录，返回 `E_PDF_ENGINE: 转换为 PDF 失败（原因）`；失败不删除已成功写出的 HTML 降级产物。
- 同一时间只允许一个 PDF 任务（全局 `Mutex`），避免两个无头实例共用 profile 冲突。
- 启动时及每次转换前清理超过 1 小时的 `astroman-pdf-*` 临时目录，避免崩溃残留堆积。
- PDF 不做二次解析与校验，只检查文件存在且 `size > 0`；页数/书签等高级能力不承诺。
- 安全：临时 HTML 已完成脱敏；进程结束后立即删除临时文件；不把 HTML 暴露到可长期访问的目录。

**为什么不用 WebView2 的 `PrintToPdf`**：当前 wry/Tauri 未暴露该 COM 接口，自行调用 `webview2-com` 需要额外管理环境与回调，收益仅是省掉一次进程启动；无头 Edge 更简单、可测试（可用固定 HTML fixture 做集成测试）。

**预览与 PDF 的关系**：预览用同一份 HTML（iframe `sandbox` 无脚本）呈现的分页样式，不每次预览都启动 Edge；仅点击「导出 PDF」时才真正转换。因此 PDF 预览是「排版近似」，最终以导出文件为准，UI 需标注这一点。

### 5.8 接口实体的推断、存储与自动同步

#### 5.8.1 存储

`requests` 追加两列，均为可空 `TEXT`，存 JSON：

```sql
request_entity  TEXT
response_entity TEXT
```

```json
{
  "version": 1,
  "updatedAt": 1790000000000,
  "hiddenPaths": ["debug.traceId"],
  "root": {
    "name": "$",
    "type": "object",
    "itemType": "",
    "required": false,
    "description": "",
    "manual": false,
    "stale": false,
    "children": [
      {
        "name": "data",
        "type": "object",
        "itemType": "",
        "required": true,
        "description": "业务数据",
        "manual": false,
        "stale": false,
        "children": []
      },
      {
        "name": "roles",
        "type": "array",
        "itemType": "string",
        "required": false,
        "description": "",
        "manual": false,
        "stale": false,
        "children": []
      }
    ]
  }
}
```

- **只存结构**：节点只含 `name / type / itemType / required / description / manual / stale / children`，顶层另有 `version / updatedAt / hiddenPaths` 元信息；**不存字段值**，因此实体本身不会成为凭据泄漏源。
- `stale = true` 表示字段在最近一次发送中已消失，但为防误删仍保留；用户可在详情页确认后删除，或点「清理过期字段」一次性移除所有 `stale` 节点。
- `hiddenPaths` 记录用户手工删除过的字段路径，自动同步不再加回；「重新生成」时清空。
- `version = 1` 用于后续结构升级；读取到不认识的版本时按「无实体」处理并记 warning，不崩溃、不整单失败。
- `manual = true` 表示用户手工新增的字段（请求体里没有），自动同步时永远保留，不参与「消失字段」判定。
- 列允许为空：旧请求没有实体，文档跳过实体节；首次发送后自动生成。
- 迁移：`DB_VERSION` 8 → 9，同时改 `schema.sql` 与 `COLUMN_MIGRATIONS`（`add_column_if_missing`），不改 `SELECT *` 的列序号假设，实体列追加在既有 `SELECT` 末尾。

#### 5.8.2 推断规则

| 输入 | 输出 |
|---|---|
| JSON 对象 | `object` 节点，`children` 按首次出现顺序递归 |
| JSON 数组 | `array` 节点，`itemType` 取元素类型的合并结果；元素为对象时，把**所有元素字段的并集**作为子节点，缺失字段 `required = false` |
| string / number / boolean / null | 对应标量类型；`null` 记为 `null`，不猜具体类型 |
| 同一路径在不同元素中类型冲突 | 统一为 `unknown`，保留首个类型作为提示，并记 warning「字段 X 类型不一致」 |
| 表单（`x-www-form-urlencoded`） | 扁平 `object`，所有字段 `type = string` |
| multipart | 只推断文本字段；文件字段记为 `file`，不读取文件内容 |
| 空对象 / 空数组 | 保留节点，`children` 为空，文档输出「（空对象）」「（空数组）」 |
| 非 JSON 文本 / HTML / 二进制 | 不推断，返回 `E_ENTITY_PARSE`，不覆盖已有实体 |

推断上限：最大深度 12、最多 500 个节点、输入正文 ≤ 256 KB（与运行记录入库上限一致）；超限时保留前 N 个节点并记 warning，不直接失败。

#### 5.8.3 必填判定

- 单次请求体：JSON 中出现的字段一律 `required = true`；`null` 值也视为出现。
- 响应数组：字段在**所有元素**中都出现才 `required = true`，部分元素缺失为 `false`。
- 用户可在详情页用复选框改写；改写后该节点标记为 `manual`，后续自动同步不再改它的必填状态。
- 「必填」描述的是**观察到的事实**，不是接口契约；文档中按实体现状输出，不额外推断业务规则。

#### 5.8.4 自动同步（发送成功后）

在 [http.rs](file:///d:/Software/ProjectFiles/AstroMan/src-tauri/src/commands/http.rs) 的发送成功分支、`record_run_snapshot` 之后调用：

```rust
// best-effort：失败只记日志，不影响请求结果
let _ = crate::entity::sync_after_send(
    &state.pool,
    request_id,
    &payload.body,        // 实际发出的请求体（已做模板解析）
    &result,              // 状态码 / 响应头 / 响应体
    payload.body_type,
);
```

同步规则：

1. **只同步手工发送**（`origin = manual`）且必须带 `request_id`：批量 / 套件 / 压测 / CLI 发送、以及未保存请求都会跳过；这些来源可能使用脚本生成的随机体，会把实体冲成噪声。
2. **请求实体**：请求体是 JSON / 表单时推断；不是 JSON 时保留原实体不动。
3. **返回实体**：仅当响应状态为 2xx、`Content-Type` 含 `json` 或响应体可解析为 JSON 时推断；失败响应（4xx / 5xx）不覆盖已有实体，避免把错误结构写进文档。
4. **合并而非替换**：按字段路径匹配旧节点，保留 `description`、`manual`、用户改过的 `required`；新字段追加到父节点末尾；旧字段在新体中消失时**保留**并标记 `stale = true`（文档中照常输出，说明用户需要确认），不自动删除。
5. **用户手工删除的字段**：记入 `hiddenPaths`（随实体 JSON 保存），后续同步不再自动加回；用户点「重新生成」时清空 `hiddenPaths`。
6. **请求保存与自动同步的竞态**：自动同步只写实体列，不碰 `body`；如果用户在这期间保存了旧实体，下一次保存以用户提交的实体为准。实体说明永远优先，类型 / 必填可以回退到最新推断。

#### 5.8.5 手动生成

接口详情页的「从当前请求体生成 / 从最近响应生成」按钮调用 `request_entity_generate`：

- 请求实体：优先取当前编辑中的 `body`（未保存也可），其次取最近一条运行记录的请求体；
- 返回实体：取最近一条成功运行（不要求 host 与环境一致，因为这是接口自身的调试数据）；
- 生成结果按 5.8.4 的合并规则套用，不直接覆盖用户说明；
- 用户点「重新生成」时二次确认「将重新推断类型与必填，字段说明保留，手工新增字段保留」。

---

## 6. 前端设计

### 6.1 入口

| 位置 | 改动 |
|---|---|
| 项目页「导出」下拉 | 新增「接口文档（预览后导出）」，进入预览抽屉；保留 OpenAPI 3.0 入口（内部改走新命令），见 [Project.vue](file:///d:/Software/ProjectFiles/AstroMan/src/views/Project.vue#L676-L688) |
| 集合树节点操作区 | 在「+子集 / 改名 / 删除」旁增加「导出文档」，作用于该集合子树，见 [Project.vue](file:///d:/Software/ProjectFiles/AstroMan/src/views/Project.vue#L716-L733) |
| 请求编辑页头部 | 在「代码 / 保存 / 压测」一组按钮中增加「导出文档」，直接带当前 `envId` 打开预览，见 [RequestEditor.vue](file:///d:/Software/ProjectFiles/AstroMan/src/views/RequestEditor.vue#L2216-L2245) |
| 工作台项目卡片（可选） | 与 [Dashboard.vue](file:///d:/Software/ProjectFiles/AstroMan/src/views/Dashboard.vue#L796) 的导出入口并列 |

### 6.2 交互：预览 + 编辑 + 导出

三个入口统一打开同一个全屏抽屉 `DocsPreviewDrawer.vue`，不再先选格式再弹保存框：

```
┌──────────────────────────────────────────────────────────────┐
│ 导出接口文档 · 开放平台                        [重新生成] [导出▾] │
├───────────────┬──────────────────────────────────────────────┤
│ 左：设置/编辑  │ 右：实时预览                                  │
│ 环境选择 ▾     │ [渲染效果] [Markdown] [HTML] [打印分页]        │
│ ☑ 环境变量表   │ ┌──────────────────────────────────────────┐ │
│ ☑ 变量转真实值 │ │ 文档标题 / 目录 / 接口详情 ...            │ │
│ ☐ 脚本         │ │                                          │ │
│ ☐ 断言         │ │                                          │ │
│ ☑ 取上次运行示例│ └──────────────────────────────────────────┘ │
│ ☑ 自动补示例   │                                              │
│ ☑ 每接口分页   │                                              │
│ 集合树（可折叠）│                                              │
│  · 接口说明 ✎  │                                              │
│  · 示例值 ✎    │                                              │
└───────────────┴──────────────────────────────────────────────┘
```

- **打开即预览**：进入抽屉先调 `docs_prepare`（scope + 默认环境），拿到 `draftId + outline` 后立即用空覆盖层调 `docs_render` 出 HTML；不要求用户先选格式。
- **环境选择**：下拉来自 `env_list(projectId)`（[api/index.ts](file:///d:/Software/ProjectFiles/AstroMan/src/api/index.ts#L130-L131)）。切换环境后重新 `docs_prepare`，丢弃当前覆盖层前弹确认「切换环境会重新生成示例值，已编辑内容将丢失」。
- **编辑能力**（全部是结构化字段，不是富文本）：
  - 文档标题、副标题
  - 集合说明、接口说明
  - 每个请求的「收录 / 排除」开关
  - 参数、Header、Body 的示例值逐行编辑；编辑过的字段标记 `manual`，点「重新生成」时清除（需二次确认）
  - 集合内请求拖动排序
  - 预览区右上角显示「已编辑 N 处」与「恢复为默认」
- **实时预览**：编辑后 300ms 防抖调 `docs_render`，只传 `draftId + 覆盖层`；Markdown 用现有 `markdown-it` 渲染，HTML 用 `<iframe sandbox="" :srcdoc="html">`（禁用脚本），「打印分页」页签是同一 HTML 的 A4 宽度模拟，顶部注明「PDF 预览为排版近似」。
- **导出**：右上角下拉给 Markdown / HTML / OpenAPI / PDF 四项；选 PDF 时按钮显示「转换中…」。保存对话框由前端调用现有文件对话框能力，用户确认覆盖后调 `docs_export`（`source: { kind: "draft", draftId, overlay }`）。若返回 `E_DRAFT_EXPIRED`，自动重跑一次 `docs_prepare` 并用本地覆盖层继续，不丢编辑。
- **草稿留存**：只暂存覆盖层（几 KB），key = `astroman:docs-overlay:{scope}:{targetId}:{environmentId}`；抽屉意外关闭可恢复，点「重新生成」或切换环境时清除。`localStorage` 写入失败（配额满 / 隐私模式）时退化为内存暂存，不阻断导出。不写后端数据库，不产生额外文件。
- **结果反馈**：toast「已导出 32 个接口到 xxx.pdf」；`warnings` 用 `message.warning` 汇总（最多 3 条 + 「等 N 条」）；PDF 降级时提示「未检测到 Edge/Chrome，已导出 HTML」并给「打开文件」按钮。

### 6.3 前端 API 与工具

在 [api/index.ts](file:///d:/Software/ProjectFiles/AstroMan/src/api/index.ts#L287-L299) 增加：

```ts
export const docsPrepare = (payload: DocsPreparePayload) =>
  invoke<DocsPrepareResult>('docs_prepare', { payload })

export const docsRender = (payload: DocsRenderPayload) =>
  invoke<DocsRenderResult>('docs_render', { payload })

export const docsExport = (payload: DocsExportPayload) =>
  invoke<DocsExportResult>('docs_export', { payload })
```

类型定义集中在 `src/types/docs.ts`，与 5.2.1 / 5.3 的 Rust 结构一一对应（camelCase）。

实体命令放在同一文件（或新建 `src/api/entity.ts`）：

```ts
export const requestEntitiesGet = (requestId: number) =>
  invoke<RequestEntities>('request_entities_get', { requestId })

export const requestEntityGenerate = (payload: {
  requestId: number
  target: 'request' | 'response'
  body?: string
  bodyType?: string
}) =>
  invoke<RequestEntities>('request_entity_generate', {
    requestId: payload.requestId,
    target: payload.target,
    body: payload.body,
    bodyType: payload.bodyType,
  })
```

新增 `src/utils/docsExport.ts` 统一处理：

- 文件名净化（`[\\/:*?"<>|]` → `_`，保留中文）与默认文件名拼装（项目名 / 集合路径 / 接口名 + 格式后缀）；
- `format` 与扩展名映射（`.md` / `.html` / `.openapi.json` / `.pdf`）；
- 覆盖层本地暂存：`localStorage` key = `astroman:docs-overlay:{scope}:{targetId}:{environmentId}`，含 `load / save / clear`；只存改过的字段，不存 IR；
- 编辑差异统计（用于「已编辑 N 处」徽标，按覆盖层节点数 + 字段数计算）；
- 防抖封装（300ms）；
- 覆盖层体积自检：序列化后超过 1 MB 时提示「编辑内容过多，建议先导出或重新生成」，并停止写入 `localStorage`（内存中仍保留，不影响导出）。

以上均放在 `src/utils/__tests__/docsExport.spec.ts` 单测，避免每个入口重复写正则。

### 6.4 AI 工具（可选，阶段三）

把 `docs_export` 注册进 [aiTools.ts](file:///d:/Software/ProjectFiles/AstroMan/src/utils/aiTools.ts) 的写操作清单（与 `project_export / openapi_export` 同组），复用现有二次确认与路径白名单校验，不额外开辟新通道。AI 调用固定走 `source.kind = scope`，不依赖前端的 `draft_id` 缓存。

### 6.5 接口详情：请求实体 / 返回实体

实体编辑器放在接口详情页内部，与「参数 / 请求头 / 认证 / 请求体」并列，不新开页面、不做全局实体库。

**入口与布局**

| 区域 | 改动 |
|---|---|
| 请求区 Tab | 在「参数 / 请求头 / 认证 / 请求体 / 脚本」后增加「请求实体」，见 [RequestEditor.vue](file:///d:/Software/ProjectFiles/AstroMan/src/views/RequestEditor.vue#L2445-L2735) |
| 响应区 Tab | 在「响应体 / 响应头 / 耗时 / 测试」后增加「返回实体」，见 [RequestEditor.vue](file:///d:/Software/ProjectFiles/AstroMan/src/views/RequestEditor.vue#L2824-L3035) |
| 工具栏 | 「从请求体生成 / 从最近响应生成」「展开全部 / 折叠全部」「搜索字段」「清理过期字段」 |

**字段表列**

```
┌──────────────┬──────────┬──────┬────────────────────────────┬──────┐
│ 字段          │ 类型      │ 必填  │ 说明（用户回填）             │ 操作  │
├──────────────┼──────────┼──────┼────────────────────────────┼──────┤
│ ▾ data       │ object   │ ☑    │ 业务数据                    │ + ✕  │
│   id         │ number   │ ☑    │ 用户 ID                     │ + ✕  │
│   nickname   │ string   │ ☐    │                             │ + ✕  │
│ ▸ roles[]    │ string   │ ☐    │ 角色列表                    │ + ✕  │
└──────────────┴──────────┴──────┴────────────────────────────┴──────┘
```

- **类型只读**：自动识别，不允许手改，避免用户把 `number` 写成 `int` 后文档与真实结构不一致；类型冲突显示为 `unknown` 并给出 tooltip。
- **必填可改**：复选框；改动后该字段标记 `manual`，后续同步不再覆盖。
- **说明可填**：失焦即本地暂存，点保存随接口一起落库；支持多行但导出时压成一行（表格单元格）。
- **可增删字段**：`+` 在父节点下增加子字段（默认 `string`、必填关闭、说明为空），`✕` 删除；删除自动推断字段时记入 `hiddenPaths`，删除手工新增字段时直接移除。手工新增时可在常用类型中选择，自动推断出的类型仍只读。
- **无实时响应时的返回实体**：如果接口已保存返回实体但当前没有实时 / 运行记录可恢复，响应区在空状态下仍展示返回实体面板（标题注明「暂无实时响应」），说明与必填照常可编辑。
- **手工字段标记**：手工新增的节点带「手工」小标签；自动同步永远保留。
- **过期字段**：`stale = true` 的字段整行浅灰 + 「已消失」标签，可单独删除或「清理过期字段」批量删除。
- **空状态**：没有实体时显示「发送一次请求即可自动生成，或点上方按钮手动生成」；请求体不是 JSON 时说明「仅支持 JSON / 表单请求体」。
- **未保存请求**：允许先在本地编辑，但「从最近响应生成」需 `request_id`；提示「先保存接口才能关联运行记录」。
- **保存语义**：实体的说明 / 必填 / 增删与请求体一起通过 `request_save` 保存；切换接口或关闭页面时若有未保存改动，沿用现有「未保存确认」流程。

**发送后的刷新**

1. 手工发送成功 → 后端 best-effort 同步实体（5.8.4）；
2. 前端在 `send` 成功后静默调用 `request_entities_get(request_id)`，只更新当前接口的实体页签；
3. 刷新时把后端结果与本地未保存编辑合并（`mergeEntityEdits`）：说明以本地为准、手工字段补回、本地删除记入 `hiddenPaths`，不弹「合并 / 放弃」确认，避免一次发送冲掉正在填写的内容；
4. 批量 / 压测 / 套件发送不触发实体刷新。

**与文档预览编辑的边界**

- 接口详情里的实体编辑改的是**接口数据**，会持久化并影响后续所有导出；
- 文档抽屉里的「接口说明 / 示例值」编辑改的是**本次文档覆盖层**，不写回接口；
- 文档抽屉的实体节只读，提供「去接口详情编辑实体」链接，避免两处都能改说明造成认知冲突。

---

## 7. 边界规则汇总

| 情况 | 处理 |
|---|---|
| 请求 URL 为空 | 项目 / 集合级：跳过并记 warning；单接口级：输出「地址未填写」占位 |
| 集合只有子集合没有请求 | 保留标题，正文「该集合暂无接口」 |
| 项目没有任何集合 | 允许导出，正文「该项目暂无接口」 |
| 同名接口（同集合） | 保留全部，按顺序输出；HTML / Markdown 用「名称（2）」区分锚点 |
| 同 path 同 method（仅 OpenAPI） | `-2 / -3` 后缀 + warning |
| `enabled = false` 的 header / query | 不输出 |
| Body 为空但 `body_type = json` | 输出「（空 JSON 体）」 |
| Body 非合法 JSON | 原样文本输出，并记 warning，不阻断导出 |
| 请求体不是 JSON / 表单 | 不生成请求实体，保留已有实体；文档跳过请求实体节 |
| 响应不是 2xx 或不是 JSON | 不生成 / 不覆盖返回实体；文档仍输出已有实体 |
| 实体 JSON 损坏 / 版本不认识 | 按「无实体」处理并记 warning，不阻断导出 |
| 实体字段数 / 深度超限 | 保留前 N 个节点与已推断部分，记 warning，不整单失败 |
| 实体字段类型冲突 | 类型记为 `unknown`，保留首个类型提示，文档照常输出 |
| 实体说明为空 | 文档中保留空白单元格，不填「暂无」 |
| 用户手工删除的字段再次出现在响应中 | 命中 `hiddenPaths`，不自动加回；「重新生成」时清空 |
| 自动同步时字段消失 | 保留并标记 `stale`，文档照常输出；用户确认后删除 |
| 批量 / 压测 / 套件发送 | 不自动同步实体，避免随机数据污染 |
| 删除实体字段 | 只改实体，不删除请求体中的对应字段 |
| 实体页签「重新生成」 | 二次确认；保留说明与手工字段，清空 `hiddenPaths`，类型 / 必填按最新观测刷新 |
| 脚本 / 断言含敏感字面量 | 输出前按 4.2.1 的值级清单替换；脚本 / 断言默认关闭 |
| 敏感值含 `& < > " \` 或中文 | 在字段产出时替换，不依赖渲染后文本匹配；标记为必测项 |
| 手工添加 `X-API-Key` / token 参数 / URL userinfo | 按 4.2 掩码，不需要用户额外声明 |
| 集合 / 请求已被删除 | 导出开始时一次性读取，中途不存在返回 `E_SCOPE: 目标不存在` |
| 输出路径已存在 | 前端确认覆盖；后端直接覆盖（与现有导出行为一致） |
| 特殊字符文件名 | 前端净化，后端不额外改名 |
| 未保存请求（无 id） | 单接口导出提示「请先保存接口」；内存导出后置 |
| 未选择环境 | 保留 `{{变量}}`，记 warning「N 处变量未解析」，仍可导出 |
| 环境变量名不存在 | 同「未解析」，预览中该值标黄，不阻断 |
| 变量命中敏感值 | 解析后立即替换为 `••••••`，搜索产物不得出现真实值 |
| 接口无运行记录 | 按 4.5 确定性生成示例，并标注「自动生成」 |
| 运行记录 host 与所选环境不一致 | 不取该记录，退回自动生成并记 warning |
| 运行记录超过 30 天 | 不取该记录（`runExamplesMaxAgeDays = 0` 时关闭限制） |
| 未选环境（模板模式） | 不取运行记录，保留占位符 |
| 运行记录全部失败 | 取最新一条记录（状态码可为 0），不输出错误信息 |
| 运行记录 Body 被截断 | 示例体末尾标注「（已截断）」，不做补全猜测 |
| 自动生成示例关闭 | 无定义值且无运行记录时留空，不编造 |
| 用户编辑后点「重新生成」 | 弹确认，明确「将丢弃手动编辑」，确认后重跑 `docs_prepare` |
| 覆盖层 key 对不上（接口已删除 / 源数据变更） | 忽略该编辑并记 warning，不整单失败 |
| `draft_id` 过期 / 不存在 | 返回 `E_DRAFT_EXPIRED`，前端重跑 `docs_prepare` 并用本地覆盖层继续 |
| 导出前预估超过大小上限 | 直接提示按集合分批，不进入渲染 |
| 前端提交非法 format | 返回 `E_FORMAT: 不支持的格式`，不落盘 |
| PDF 引擎缺失 | 导出 HTML 并返回 `pdfEngine = "none"` + `fallbackHtmlPath` |
| PDF 目标路径与临时目录不同卷 | `fs::copy` + 删除，不用 `fs::rename` |
| PDF 中的折叠内容 | PDF 模式强制 `<details open>`，否则打印看不到 |
| 同时触发两次 PDF 导出 | 全局锁串行执行，第二次等待或直接提示「正在转换」 |

---

## 8. 测试方案

### 8.1 Rust 单测（`docs` 模块，纯渲染器）

用构造好的 `DocBundle` 断言文本，不依赖数据库：

1. Markdown：项目级包含全部集合标题、请求标题、完整集合路径；
2. Markdown：单接口级只有一节，且带项目 / 集合上下文；
3. Markdown：请求名 / 说明含 `|`、换行、行首 `#` 时，表格与标题结构不被破坏；
4. HTML：请求名含 `<script>alert(1)</script>` 时输出被转义，产物中不出现可执行 `<script` 标签；
5. HTML：不包含外部 CSS / JS 引用（除请求 URL 本身），确认自包含；
6. 脱敏：Bearer token、Basic password、OAuth2 clientSecret、`Cookie` 头、敏感环境变量值均不出现在任意格式产物中；
7. 脱敏：敏感值长度 < 4 时不参与子串替换（防止误伤），但命中敏感名称的字段仍整值掩码；
8. 脱敏：敏感值包含 `& < > " \ /` 与中文时，在 Markdown / HTML / JSON 产物中均不出现原值（验证值级替换，不依赖渲染后匹配）；
9. 脱敏：`X-API-Key` 头、名称为 `access_token` 的 query、`https://user:pass@host` 的 URL userinfo 均被掩码；`keyword` / `tokenizer` 等包含敏感词但非整词的参数不被误掩码；
10. 脱敏：auth 令牌被复制到 Body 后再次出现，仍被掩码；
11. 空集合：项目级跳过、集合级保留；
12. Body 超 64 KB 时输出含「（已截断）」；
13. OpenAPI：同 path 同 method 产生 `-2` 且 `warnings` 记录；
14. OpenAPI：bearer 认证生成 `components.securitySchemes` 并在 operation 上挂 `security`；
15. 环境解析：`{{baseUrl}}/api/login` + 环境变量输出真实域名；未定义变量保留原样并产生「未解析」warning；
16. 敏感解析：`{{secret:token}}` 解析后输出 `••••••`，产物全文不含原值；
17. 示例链：已有值优先；空值时取运行记录；无运行记录时自动生成，`example_source` 分别为 `defined / run / auto`；
18. 示例链：运行记录 host 与所选环境不一致、或超过 30 天时不采用，退回自动生成；
19. Path 对齐：模板与运行记录段数一致时取实际段，不一致时退回生成，不产生错位值；
20. 确定性：同一 fixture 连续渲染两次，示例值完全一致；默认只输出日期，两次导出全文一致；
21. 覆盖层合并：改标题 / 说明 / 排除开关 / 示例值 / 顺序后产物随之变化，未编辑字段保持默认；
22. 覆盖层失配：源数据变化导致 key 不存在时忽略该编辑并记 warning，不整单失败；
23. 缓存：prepare 后连续 render 不读库；`draft_id` 过期返回 `E_DRAFT_EXPIRED`；同一 draft 连续 render 结果一致；
24. 选项默认值：`DocsOptions::default()` 与 serde 缺省字段一致——include / resolve / useRun / auto / responseExample / pageBreak 均为 `true`，`runExamplesMaxAgeDays = 30`，脚本 / 断言 / 时分均为 `false`；
25. PDF HTML 模式：包含 `@page` 与 `page-break-before`，`<details>` 为 `open`，且仍无 `<script>` 标签；
26. 实体推断：嵌套对象、数组、`null`、空对象、空数组分别得到正确的 `type / itemType / children`；
27. 实体推断：数组元素字段并集正确，部分元素缺失的字段 `required = false`，全元素存在为 `true`；
28. 实体推断：类型冲突记为 `unknown` 并产生 warning，不丢失已识别字段；
29. 实体合并：新字段追加、旧字段消失保留并标 `stale`、`description / manual / 用户改过的 required` 不被覆盖；
30. 实体合并：重复推断幂等，两次结果完全一致；`hiddenPaths` 中的字段不自动加回；
31. 实体上限：深度 > 12、节点 > 500、正文 > 256 KB 时截断并记 warning，不 panic；
32. 文档渲染：实体存在时 Markdown / HTML 输出字段表，说明为空保持空白；实体不存在时整节省略；
33. 文档渲染：实体字段名 / 说明含 `<script>`、`|`、换行时被正确转义，HTML 无 `<script` 标签；
34. 脱敏：实体说明中粘贴真实 token 时，Markdown / HTML 产物中不出现原值；
35. 存储兼容：旧请求 `request_entity / response_entity` 为 NULL 时读取、导出、保存均不报错；损坏 JSON 视为无实体。

响应示例（阶段一 A 已实现）的补充用例：

- 只取最近一次成功运行（`error` 为空且 `response_status > 0`），host 与所选环境不一致时不采用；
- 响应头按 4.2.1 脱敏，`Set-Cookie` 等敏感头整值掩码；响应体中的 token 子串被替换；
- 响应体超过 64 KB 时截断并输出「（已截断）」；非 JSON 响应体按 `text` 代码块输出；
- 单接口文档不出现集合标题，HTML 中该接口的 `<details>` 为 `open`；多接口文档仍按集合树组织。

### 8.2 集成测试（临时 SQLite）

沿用现有 `temp_pool` 模式构造「项目 → 2 层集合 → 3 个请求 → 每个请求 1 条运行记录」，断言：

- `docs_prepare` 返回 `draft_id + outline`，大纲包含正确树结构与每个字段的 `source`（defined / run / auto / manual），且不含脚本 / 断言 / 完整 Body 原文；
- `docs_export` 三次（request / collection / project）文件存在、扩展名与 `format` 匹配；
- `request_count / collection_count` 正确；
- 集合级不包含兄弟集合的请求；项目级包含全部；
- `docs_render(draft_id, overlay, "html")` 与 `docs_export(kind = draft, format = html)` 的内容一致；
- 编辑覆盖层后导出，产物包含编辑后的标题、说明与示例值，未编辑节点不受影响；
- 大项目载荷：构造 2000 个请求的项目，`docs_prepare` 响应体（大纲）< 5 MB，`docs_render` 入参（覆盖层）< 1 MB，且都显著小于整份 IR；
- 缓存：TTL 过期 / LRU 淘汰后 render 返回 `E_DRAFT_EXPIRED`；`revision` 变化只产生 warning，不阻断；
- 运行记录 host 与环境不一致、或超过 30 天时不参与示例取值；
- 环境变量在导出产物中已解析为真实域名，敏感变量仍为掩码；
- PDF：检测到 Edge/Chrome 时导出真实 PDF（文件存在且大小 > 0），检测不到时断言降级 HTML 与 `pdfEngine = "none"`；本机存在其它盘符时，导出到与临时目录不同卷的路径并断言文件成功落地（无其它卷则跳过）。
- 迁移：用 v8 库启动，断言实体列被补齐、原数据不丢、`DB_VERSION = 9`；重复启动幂等；
- 实体链路：保存带实体的请求 → `find_request` 读回一致 → `docs_prepare` 的大纲 / `docs_export` 产物含实体表；
- 自动同步：模拟手工发送成功后同步请求 / 返回实体；响应为 4xx 时返回实体保持不变；
- 保存兼容：不带实体字段的旧前端 payload 保存时不把实体清空（`Option` 缺省 = 不修改，而不是写 NULL）；
- 文档渲染：同一请求连续两次导出内容一致（实体顺序稳定）。

### 8.3 前端单测

`src/utils/__tests__/docsExport.spec.ts`：

- 中文名 + 非法字符 → 合法文件名；
- 超长名称截断到合理长度；
- 四种格式的扩展名映射；
- 覆盖层 `load / save / clear` 按 `scope + targetId + environmentId` 隔离；
- 1000 个节点下覆盖层序列化 < 1 MB，且只包含被改过的字段；
- `E_DRAFT_EXPIRED` 时自动重跑 prepare 并重新套用本地覆盖层（编辑不丢）；
- 编辑差异计数准确（改标题、改示例、切换收录、调整顺序）。

新增 `src/utils/__tests__/entitySchema.spec.ts`（实体树的前端纯函数）：

- 扁平化 / 还原：树 ↔ 表格行顺序稳定，展开折叠不改变导出顺序；
- 按路径查找、插入子字段、删除字段（含子孙）正确；
- 删除后的路径进入 `hiddenPaths`，手工新增字段不进入；
- 搜索字段时保留祖先行（否则缩进断裂）；
- 合并后端刷新结果时，本地未保存说明不丢；
- 实体 JSON 序列化 < 1 MB（500 节点上限内），不含任何字段值。

### 8.4 手工验收清单

- Windows 保存对话框输入中文文件名，Markdown / HTML / PDF 三种均落盘正常；
- 2000 个接口的项目：打开预览与连续编辑不卡顿，DevTools 中 `docs_render` 的载荷只有覆盖层（< 1 MB）；
- 切换环境后，预览中的 URL 立即变为该环境真实域名，敏感值仍为掩码；
- 未选择环境时预览保留 `{{var}}` 并出现「未解析」提示，且不取运行记录示例；
- 编辑标题、接口说明、示例值、排除某个接口后，预览与最终文件与编辑一致；
- 关闭抽屉再打开，未导出的覆盖层能恢复；`localStorage` 满时退化为内存暂存且不影响导出；
- 生成的 HTML 用 Chrome / Edge 打开，目录跳转与打印预览正常；
- 生成的 PDF 为 A4、中文正常、每个接口另起一页，目录页单独一页；长请求体的折叠内容在 PDF 中可见；导出到 D 盘（临时文件在 C 盘）正常落地；
- 生成的 Markdown 在 GitHub / VS Code 预览中表格与标题正常；接口名含 `|` / 换行的测试数据不破坏表格；
- 导出的 OpenAPI 可被 Swagger Editor 打开、可被现有 `openapi_import` 重新导入；
- 删除 Edge/Chrome 模拟（或改名）后导出 PDF，得到 HTML 降级产物与可读提示；
- 文档中全局搜索真实 token / 密码 / Cookie，零命中。
- 打开一个已有 JSON 请求体的接口，发送成功后「请求实体」自动出现字段，类型正确；响应为 JSON 时「返回实体」自动出现；
- 给若干字段填写说明并保存，重启应用后说明仍在；再次发送后说明不被覆盖，新字段自动追加；
- 手工新增一个字段、删除一个字段后保存，再次发送不被自动加回；「重新生成」后手工字段与说明按约定保留 / 清理；
- 响应返回 4xx 时，返回实体保持原样；响应为非 JSON 时只提示不覆盖；
- 接口名 / 字段说明含 `|`、换行、`<script>` 时，Markdown 表格与 HTML 渲染正常且无可执行脚本；
- 在字段说明里粘贴一个真实 token，导出文档中为掩码、原值零命中；
- 单接口文档包含「请求实体 / 返回实体 / 响应示例」三节，且没有集合标题；
- 项目级文档中实体表顺序与界面一致，空说明保持空白。

---

## 9. 风险与对策

| 风险 | 影响 | 对策 |
|---|---|---|
| 凭据泄漏 | 最高 | 值级替换（4.2.1）+ 扩大凭据来源（auth 值 / 敏感头名 / 敏感参数名 / URL userinfo / 子串）；单测把「真实值不出现」作为硬断言，覆盖 `& " \` 转义场景；不提供「含真实凭据」开关（后置且需二次确认） |
| HTML / Markdown 注入 | 高 | 所有来自库的文本统一 `escape_html`；Markdown 单独转义 `\|`、换行与行首标记；不输出 `<script>`；加 `Content-Security-Policy` meta 禁用内联脚本 |
| 大项目导出卡顿 / 内存峰值 / IPC 过大 | 中 | IR 留后端、前端只传覆盖层；单连接读取 + 运行记录用窗口函数一次取完；请求数上限 5000 + Body 截断 + prepare 阶段预估大小；三个命令统一 `#[tauri::command(async)]` |
| 与现有 OpenAPI 输出不兼容 | 中 | 阶段二抽取时保留 `openapi_export` 签名与输出结构；为项目级输出补一个回归测试，对比关键字段而非整串 |
| 无 `description` 导致文档信息量不足 | 中 | 阶段一以 `name` + URL + 参数 + 实体说明为主；阶段二补 `requests.description` / `collections.description` / `request_runs.environment_id`（DB v10，判存补列） |
| 主密钥未解锁时认证信息不可读 | 中 | 返回 `E_SECRET_LOCKED` 并阻止导出，不静默降级 |
| 文件名非法 / 覆盖 | 低 | 前端净化和 `file_exists` 确认；后端直接写入 |
| 文档与界面顺序不一致 | 低 | 统一 `sort_order ASC, id ASC`，与 `collection_tree` / `request_list` 口径一致 |
| 真实地址写入文档 | 中 | 这是需求本身；导出前提示「本文档包含解析后的真实域名」，共享前自行确认；凭据仍强制掩码 |
| 运行记录跨环境 / 带出真实业务数据 | 中 | 只取 host 与环境一致、且 30 天内的记录；未选环境一律不取；预览面板可关闭；文档标注示例来源 |
| PDF 引擎缺失 / 被企业策略禁用 | 中 | 自动降级导出 HTML 并给出「用浏览器打印」提示，不阻断主流程 |
| PDF 转换挂起 | 中 | 超时按请求数放大（30s–180s）+ 杀进程 + 清理临时目录；全局锁保证单任务 |
| PDF 跨卷落地失败 | 中 | 临时文件与目标可能不同盘，统一 `fs::copy` + 删除，不用 `rename` |
| PDF 打印丢折叠内容 | 低 | PDF 模式强制 `<details open>`，浏览器 HTML 保留折叠 |
| 预览与 PDF 版式有差异 | 低 | 两者共用同一 HTML；预览只模拟 A4 宽度，UI 明确标注「PDF 为排版近似」，导出后可「打开文件」核对 |
| 手动编辑与源数据漂移 | 中 | 覆盖层按稳定 key 合并；`revision` 不一致记 warning；「重新生成」必须二次确认；导出文件写入生成日期 |
| 自动推断结果误导读者（必填 / 类型只是观测） | 中 | 文档标注「字段类型与必填由最近一次请求推断」；用户可改必填；不宣称是契约校验 |
| 实体说明与真实接口漂移 | 中 | 字段消失保留并标 `stale`；文档生成日期可见；「从最近响应生成」一键刷新类型 |
| 随机数据 / 压测数据污染实体 | 中 | 只同步手工发送；批量 / 套件 / 压测跳过；说明与手工字段永远优先 |
| 实体树过大影响编辑与导出 | 低 | 深度 12 / 节点 500 / 正文 256 KB 上限；超限截断 + warning；文档只输出字段表，不重复输出体 |
| 旧前端保存时清空实体 | 中 | `HttpRequestDetail` 的新字段用 `Option` 语义：缺省 = 不修改；集成测试覆盖 |
| 实体列迁移失败导致打不开库 | 中 | 走 `add_column_if_missing` + 事务；失败回滚并给出 `E_SCHEMA`；启动幂等 |
| 草稿缓存占用内存 / 残留 | 低 | TTL 30 分钟 + LRU 3 份 + 退出清空；缓存里只有掩码后的值，不含明文凭据 |
| 覆盖层本地缓存含敏感信息 | 低 | 覆盖层只含说明与示例值，不含令牌；设置页提供「清空文档草稿」；切换加密状态时清空对应覆盖层 |

---

## 10. 分期实施

### 阶段一 A：文档核心 + 环境解析 + 示例链（只读预览与导出）

1. 新增 `docs/` 模块：`model.rs`、`builder.rs`、`examples.rs`、`cache.rs`、`render_markdown.rs`、`render_html.rs`、`mod.rs`；
2. 新增 `commands/docs.rs`，在 [commands/mod.rs](file:///d:/Software/ProjectFiles/AstroMan/src-tauri/src/commands/mod.rs#L1-L34) 与 `lib.rs` 的 `invoke_handler` 注册 `docs_prepare / docs_render / docs_export`（全部 `#[tauri::command(async)]`）；`AppState` 增加 `DocsCache`（含 TTL / LRU / `revision`）；
3. `builder` 接入 `util::render_tracked`，实现 4.2.1 的值级脱敏、4.4 的环境解析与 4.5 的示例链（host + 30 天过滤、窗口函数一次取运行记录）；
4. 前端：`api/index.ts` 增加三个命令与 `DocsPrepareResult / DocEditOverlay` 类型；新增 `src/utils/docsExport.ts`（文件名、扩展名、覆盖层缓存、差异统计）+ 单测；
5. 前端：`Project.vue` 导出下拉与集合节点操作、`RequestEditor.vue` 头部入口（带当前 `envId`）；
6. 前端：新增 `DocsPreviewDrawer.vue` 的只读版（环境选择、选项、Markdown / HTML 预览、导出下拉）；
7. 阶段一 A 验收：三种 scope × Markdown/HTML，含环境解析、示例链、脱敏，通过 8.1 的 1–20、8.2 与 8.4 中非编辑项。

### 阶段一 B：覆盖层编辑 + 草稿恢复

1. 新增 `docs/overlay.rs`：稳定 key（5.2.1）、默认值定位、编辑合并、失配忽略与 warning；
2. 覆盖层失配处理：`revision` 变化只记 warning；`E_DRAFT_EXPIRED` 由前端自动重跑 prepare 并重新套用覆盖层；
3. 前端：`DocsPreviewDrawer.vue` 增加标题 / 说明 / 示例值 / 收录 / 排序编辑、覆盖层 localStorage、差异徽标、「重新生成」二次确认；
4. 前端：`E_DRAFT_EXPIRED` 自动重跑 prepare 并重新套用覆盖层；`docsExport.spec.ts` 补齐覆盖层与重试用例；
5. 阶段一 B 验收：8.1 的 21–24、8.3、8.4 中编辑与草稿恢复条目全部通过。

### 阶段一 C：接口实体（请求 / 返回字段树）

1. 新增 `entity/` 模块：`model.rs`（EntityDoc / EntityNode / 版本与 `hiddenPaths`）、`infer.rs`（JSON / 表单推断、数组并集、类型冲突、深度与节点上限）、`merge.rs`（保说明 / 必填 / 手工字段 / `stale` / `hiddenPaths`）；
2. `DB_VERSION` 8 → 9：`requests.request_entity`、`requests.response_entity`，同时改 `schema.sql` 与 `COLUMN_MIGRATIONS`；`map_detail` / `find_request` / `update_existing` / `request_save` 追加实体列（追加在 `SELECT` 末尾，序号顺延）；
3. 新增 `commands/entity.rs`：`request_entities_get`、`request_entity_generate`；在 `commands/mod.rs` 与 `lib.rs` 注册；
4. 发送成功旁路：`http.rs` 在 `record_run_snapshot` 后调用 `entity::sync_after_send`（只 `origin = manual`、只 2xx JSON 响应、best-effort）；
5. `HttpRequestDetail` 增加 `request_entity / response_entity`（`Option` 语义，缺省不修改），同步 `src/types/models.ts`；
6. `docs/model.rs` 增加 `DocEntity / DocEntityField`，`builder` 读取实体列并转成扁平字段表；Markdown / HTML 渲染「请求实体 / 返回实体」；
7. 前端：`RequestEditor.vue` 请求区增加「请求实体」页签、响应区增加「返回实体」页签（字段表、类型只读、必填开关、说明编辑、增删字段、搜索、过期字段清理、手工字段标记）；
8. 前端：发送成功后静默刷新实体；有本地未保存改动时提示合并 / 放弃；未保存接口提示先保存；
9. 前端：新增 `src/utils/entitySchema.ts`（树 ↔ 行、按路径增删、`hiddenPaths`）+ 单测；
10. 阶段一 C 验收：8.1 的 26–35、8.2 的实体与迁移条目、8.3 的实体工具用例、8.4 中实体相关条目全部通过。

### 阶段二：PDF 直出 + OpenAPI 三级 + 描述字段

1. 新增 `docs/render_pdf.rs`：Edge/Chrome 探测、无头 `--print-to-pdf`、按规模放大的超时、全局锁、临时目录清理、`copy + remove` 跨卷落地、引擎缺失降级 HTML；
2. `render_html.rs` 增加 PDF 模式（A4 `@page`、目录页分页、每接口分页开关、强制展开 `details`）；前端预览增加「打印分页」页签与「导出 PDF」；
3. 抽取 `transfer.rs` 的 OpenAPI 构建逻辑到 `docs/render_openapi.rs`，`openapi_export` 改为委托；
4. `docs_export` 支持 `format = openapi`，覆盖 request / collection / project，补 `securitySchemes` 与冲突 warning；
5. `DB_VERSION` 升到 10：`requests.description`、`collections.description`、`request_runs.environment_id`，同时改 `schema.sql` 与 `COLUMN_MIGRATIONS`；发送路径写入 env_id，示例链改为精确匹配；同时检查 `request_runs` 的列序号读取；
6. `request.rs` 的读取 / 保存与前端请求编辑页、集合重命名弹窗增加说明输入；渲染优先 `description`，为空回退 `name`；
7. 阶段二验收：PDF 与 OpenAPI 相关测试、8.4 中 PDF 条目全部通过。

### 阶段三：可选增强

1. CLI：`astroman-cli docs export --scope project --id 3 --environment 2 --format pdf --out docs/api.pdf`，复用同一 `builder`；
2. AI 工具注册 `docs_export`（写操作确认）；
3. Postman Collection v2.1 导出（同一 IR 新增一个渲染器）；
4. 文档模板（封面 / 水印 / 公司 Logo）作为渲染器的可选参数；
5. 实体增强：从 OpenAPI / JSON Schema 反向导入实体、实体字段级示例值、跨接口实体引用（需先解决「实体随接口保存」的定位冲突，默认不做）。

> 已完成（原阶段三第 1 项）：`include_response_example` 默认开启，取最近一次成功运行的状态码 / 响应头（脱敏）/ 截断响应体；单接口文档采用独立版式。

---

## 11. 验收标准

| 编号 | 验收项 | 判定 |
|---|---|---|
| D-01 | 单接口可导出 Markdown / HTML / OpenAPI | 三种文件均生成，内容含该接口的方法、URL、参数、Body、认证说明 |
| D-02 | 集合可导出，且包含全部后代 | 导出结果中的接口数 = 该子树请求总数；不含兄弟集合接口 |
| D-03 | 项目可导出 | 接口数 = 项目全部请求数；环境变量表存在且敏感值为掩码 |
| D-04 | 顺序一致 | 文档中接口顺序与界面 `sort_order` 一致 |
| D-05 | 默认脱敏 | 在三种产物中全文搜索真实 token / 密码 / clientSecret / Cookie / `X-API-Key` / token 参数 / URL userinfo，零命中；敏感值含 `& " \` 与中文时同样零命中 |
| D-06 | HTML 自包含 | 断网打开正常渲染；无外部 CSS / JS；无 `<script>` 标签 |
| D-07 | 空集合规则 | 项目级跳过空子树；集合级保留目标集合并显示占位文案 |
| D-08 | 异常可判定 | 目标不存在、格式非法、主密钥未解锁、请求数超限、`draft_id` 过期均返回中文可读错误，UI 可展示；过期时自动恢复不丢编辑 |
| D-09 | OpenAPI 可回灌 | 导出的项目级 OpenAPI 能被现有 `openapi_import` 成功导入，接口数量一致 |
| D-10 | 不回归 | `cargo test --lib`、`npm test`、`npm run build`、`cargo clippy -- -D warnings` 全绿 |
| D-11 | 环境解析 | 选环境后文档 URL 为真实域名；未选环境保留占位符并提示；敏感变量仍为 `••••••` |
| D-12 | 示例链 | 逐字段按「定义值 → 最近运行记录 → 自动生成」取值，`example_source` 正确；运行记录 host 与环境一致且不超过 30 天；连续两次导出结果一致（默认只输出日期） |
| D-13 | 预览与编辑 | 实时预览可在 Markdown / HTML / 打印分页间切换；编辑标题、说明、示例、收录后导出文件与预览一致；编辑只回传覆盖层 |
| D-14 | 草稿恢复 | 关闭抽屉后重开可恢复未导出的覆盖层；「重新生成」有二次确认并清除编辑；缓存过期时能自动恢复 |
| D-15 | PDF 直出 | 能直接生成 A4 PDF，中文正常，目录页与接口分页正确；折叠内容可见；跨卷目标路径落地成功 |
| D-16 | PDF 降级 | 无 Edge/Chrome 时导出 HTML 并返回 `pdfEngine = "none"`，UI 给出可操作提示 |
| D-17 | PDF 无残留 | 转换成功或失败后，临时 HTML / PDF / profile 目录均被清理；崩溃残留不超过 1 小时 |
| D-18 | 大项目不撑爆 IPC | 2000 接口项目下 `docs_prepare` 响应 < 5 MB、`docs_render` 入参 < 1 MB，UI 不卡顿 |
| D-19 | 选项默认值 | 不传 `options` 时环境表 / 变量解析 / 运行记录示例 / 自动示例 / 每接口分页默认开启，脚本 / 断言 / 时分默认关闭 |
| D-20 | Markdown 结构安全 | 请求名或说明含 `\|`、换行、行首 `#` 时，表格与标题仍正确渲染 |
| E-01 | 请求实体自动生成 | 保存并手工发送含 JSON Body 的请求后，「请求实体」出现字段与类型；嵌套对象、数组、`null`、空对象均有正确表现 |
| E-02 | 返回实体自动生成 | 响应为 2xx JSON 时「返回实体」自动生成；4xx / 非 JSON 不覆盖已有实体；批量 / 压测发送不同步 |
| E-03 | 说明保留 | 填写说明后再次发送，说明不被覆盖；新字段自动追加，消失字段保留并标「已消失」 |
| E-04 | 手工编辑 | 可新增 / 删除字段、切换必填；手工字段与说明在自动同步后仍在；删除的字段不被自动加回 |
| E-05 | 类型只读与冲突 | 类型不可手改；类型冲突显示 `unknown` 并给出提示，不丢字段 |
| E-06 | 持久化 | 实体随接口保存，重启应用后仍可读；旧请求（无实体）打开、保存、导出均不报错 |
| E-07 | 文档输出 | 单接口 / 集合 / 项目文档包含「请求实体 / 返回实体」表；说明为空保持空白；无实体时整节省略 |
| E-08 | 脱敏 | 字段说明中粘贴真实 token / 密码，三种产物中零命中；实体本身不存储任何字段值 |
| E-09 | 迁移 | v8 库升级到 v9 后实体列存在、原数据不丢、重复启动幂等；`cargo test --lib` / `clippy` 全绿 |
| E-10 | 大实体 | 500 节点 / 深度 12 / 256 KB 上限生效，超限截断并给出 warning，UI 不卡死 |

---

## 12. 涉及文件清单

新增：

- `src-tauri/src/docs/mod.rs`
- `src-tauri/src/docs/model.rs`
- `src-tauri/src/docs/builder.rs`
- `src-tauri/src/docs/examples.rs`
- `src-tauri/src/docs/overlay.rs`
- `src-tauri/src/docs/cache.rs`
- `src-tauri/src/docs/render_markdown.rs`
- `src-tauri/src/docs/render_html.rs`
- `src-tauri/src/docs/render_pdf.rs`（阶段二）
- `src-tauri/src/docs/render_openapi.rs`（阶段二）
- `src-tauri/src/commands/docs.rs`
- `src-tauri/src/entity/mod.rs`（阶段一 C）
- `src-tauri/src/entity/model.rs`（阶段一 C）
- `src-tauri/src/entity/infer.rs`（阶段一 C）
- `src-tauri/src/entity/merge.rs`（阶段一 C）
- `src-tauri/src/commands/entity.rs`（阶段一 C）
- `src/utils/entitySchema.ts`（阶段一 C）
- `src/utils/__tests__/entitySchema.spec.ts`（阶段一 C）
- `src/types/docs.ts`
- `src/utils/docsExport.ts`
- `src/utils/__tests__/docsExport.spec.ts`
- `src/components/DocsPreviewDrawer.vue`（替换原 `DocsExportModal.vue` 方案）

修改：

- `src-tauri/src/commands/mod.rs`（模块注册：`docs` / `entity`）
- `src-tauri/src/lib.rs`（`invoke_handler` 注册；`AppState` 增加 `DocsCache` 并在退出时清空）
- `src-tauri/src/commands/transfer.rs`（阶段二抽取 OpenAPI 构建逻辑，保留 `openapi_export`；阶段一 C 项目导入 / 导出可选携带实体列）
- `src-tauri/src/db/schema.sql` + `src-tauri/src/db/mod.rs`（阶段一 C：实体列，`DB_VERSION` 9；阶段二：描述字段 + `request_runs.environment_id`，`DB_VERSION` 10）
- `src-tauri/src/models.rs` + `src-tauri/src/commands/request.rs`（阶段一 C 实体列读写；阶段二描述字段读写）
- `src-tauri/src/commands/http.rs`（阶段一 C：发送成功后的实体 best-effort 同步）
- `src-tauri/src/commands/run.rs`（阶段二写入 / 读取 `environment_id`，检查列序号映射）
- `src/types/models.ts`（阶段一 C `HttpRequestDetail.requestEntity / responseEntity`；阶段二 `description`）
- `src/api/index.ts`（`docsPrepare / docsRender / docsExport`；阶段一 C `requestEntitiesGet / requestEntityGenerate`）
- `src/views/RequestEditor.vue`（阶段一 C 请求实体 / 返回实体页签；头部导出入口带 `envId`）
- `src/views/Project.vue`（导出下拉 + 集合节点入口）
- `src/views/Dashboard.vue`（可选入口）
- `src/utils/aiTools.ts`（阶段三）

---

## 13. 接口实体（请求 / 返回字段树）设计

### 13.1 目标与定位

对标 Apifox 的「请求参数 / 返回响应」结构面板，但范围收窄：

- **只服务于当前接口**：实体随 `requests` 行保存，不建全局数据模型库，不做跨接口引用；
- **只覆盖请求体 / 响应体**：Query / Path / Header 已由「请求参数 / 请求头」表描述，不重复进入实体；表单与 multipart 文本字段按对象处理；响应头仍由「响应示例」输出，不进入返回实体。
- **类型自动、说明人工**：字段名与类型由最近一次真实请求 / 响应推断；字段说明默认留空，由用户回填；必填可人工改写；
- **只描述结构，不存值**：实体 JSON 里没有字段值，字段示例值仍走 4.5 的示例链；
- **进入文档**：实体作为接口文档的「请求实体 / 返回实体」表输出，说明为空保持空白。

### 13.2 数据模型

```ts
interface EntityDoc {
  version: 1
  updatedAt: number
  hiddenPaths: string[]          // 用户手工删除、不再自动加回的字段路径
  root: EntityNode
}

interface EntityNode {
  name: string                   // 相对父节点的字段名；数组的元素字段直接挂在数组节点下
  type: 'object' | 'array' | 'string' | 'number' | 'boolean' | 'null' | 'file' | 'unknown'
  itemType: string               // type = array 时的元素类型
  required: boolean
  description: string            // 用户回填，默认 ''
  manual: boolean                // 手工新增 / 手工改过必填
  stale: boolean                 // 最近一次发送中已消失
  children: EntityNode[]
}
```

- 路径采用点号拼接（`data.user.roles[].name`），与文档表格的「字段」列一致；
- 根节点固定 `name = "$"`、`type = "object"`，不参与文档输出；
- 字段顺序 = 首次出现顺序；手工新增追加到父节点末尾。

### 13.3 类型与必填

| 观察 | `type` | `required` |
|---|---|---|
| JSON 对象 | `object` | 出现即 `true`；数组元素内仅当所有元素都出现才为 `true` |
| JSON 数组 | `array`，`itemType` 为元素类型；元素对象字段取并集 | 同上 |
| 字符串 / 数字 / 布尔 | `string / number / boolean` | 同上 |
| `null` | `null` | 出现即 `true` |
| 类型冲突 | `unknown` | 保持字段存在，记 warning |
| 表单字段 | `string` | 出现即 `true` |
| multipart 文件字段 | `file` | 出现即 `true` |

「必填」表示**最近一次观测中字段是否出现**，不是接口契约；文档页脚会标注这一点，避免读者把它当强约束。

### 13.4 合并算法（伪代码）

```text
merge(old, incoming, hiddenPaths):
  for each path in incoming (深度优先):
    if path in hiddenPaths: skip
    if old has path:
      node.type      = incoming.type        # 类型以最新观测为准
      node.itemType  = incoming.itemType
      node.required  = node.manual ? node.required : incoming.required
      node.description = old.description    # 说明永远保留
      node.manual    = old.manual
      node.stale     = false
      merge children
    else:
      append incoming node (manual = false, stale = false)
  for each old path not in incoming and not manual:
    old.stale = true                        # 保留，不删除
```

- `manual = true` 的字段不参与「消失」判定，也不会被自动删除；
- 用户改过必填的字段 `manual = true`，后续同步不再改它的 `required`；
- 用户删除字段时写入 `hiddenPaths`，同时从树中移除；「重新生成」时清空 `hiddenPaths` 并重新推断。

### 13.5 与文档的关系

- `builder` 读取实体列 → `DocEntity { fields: Vec<DocEntityField> }`，按深度优先拍平，`depth` 用于缩进；
- Markdown / HTML 输出「字段 / 类型 / 必填 / 说明」四列表格；说明为空就是空单元格；
- `stale = true` 的字段在说明末尾追加「（最近一次发送未出现）」，不删除该行，提醒读者确认；
- 实体不存在或两个实体都为空时整节省略；
- 实体字段说明在输出前过 4.2.1 的敏感值替换；字段名与类型原样输出；
- 文档抽屉只读实体，提供「去接口详情编辑实体」入口。

---

## 14. 一句话总结

三个层级共用一份 IR：**环境解析成真实地址、示例值按「定义 → 运行记录（同环境、30 天内）→ 确定性生成」回退、预览里用稀疏覆盖层编辑结构化字段、同一份 HTML 既交付也直出 PDF**。接口实体把「字段长什么样」固化成接口自身的结构：发送时自动推断、说明人工回填、随接口保存，再以字段表进入文档。IR 留在后端，前端只传几百 KB 以内的覆盖层；凭据在字段产出时按值脱敏，渲染器保持纯函数可测；DB 变更分两次（v9 实体列、v10 描述字段）——新增能力只是「多一个渲染器 + 一张字段表」，而不是「三套导出逻辑」。
