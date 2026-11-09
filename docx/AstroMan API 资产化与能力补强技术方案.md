> 状态：方案评审通过；**W0 止血与基线、W1 资产闭环、W2 复用与底座、W3 工程化与协议广度、W4 企业级交付、W5 AI 专项已实现并通过回归（2026-11-09）**；W4 的 4GB 实机 / 兼容矩阵 / 真实签名与 W5 的真实模型评测数据待人工补采
> 目标：把现有「点状能力强」的调试工具，补成「接口资产可管理、可校验、可复用」的工程化平台，并达到企业级非功能基线
> 范围：12 个功能缺口 + 13 个 AI 专项问题 + 性能/内存、低端设备、兼容性、部署更新、安全审计、可观测性等非功能项
> 说明：本方案不承诺工期；路线图已规划的 Flow / 定时巡检 / 混合压测 / 分布式压测 / 团队同步 / Linux 桌面不在本文展开，只说明衔接点
>
> **实现进度（2026-11-08）**
>
> - 已完成：`services` 抽取（`request` / `revision` / `contract` / `openapi` / `docs` / `docs_site`，命令与 CLI 为薄适配）、G-01 版本历史（硬 50 条、实体合并回滚、`source=import`）、G-02 实体 → OpenAPI schema 与契约快照 / diff、G-03 只读文档站、P-01 HAR 流式导出（含仅元数据模式）、P-02 抓包树虚拟滚动、前端历史 / 契约 / 文档站入口；A-01 ~ A-06 全绿。
> - **W0 已落地**：P-03 项目表阈值分页、P-05 内存分档、P-06 SQLite 参数、结构化 JSON 日志 + panic 落盘、诊断包、`audit_logs`（v11）、AI-01 权限模式（前后端双保险）、AI-02 工具取消、CI 体积 / 依赖 / 版本门禁、4 项性能基线（均大幅低于目标线）与进程组 RSS 采样脚本；回归（`cargo test` / clippy / fmt、vitest、vue-tsc、build）全绿。
> - **W2 已落地**：G-05 Faker（`services/faker.rs`，`{{faker:xxx}}` 接入 `util::render`，文档示例复用；请求栏提供 faker 变量速查与「将生成 N 个随机值」计数）、G-04 本地 Mock Server（`mock_routes` 幂等建表、标准库 TCP 服务、`{id}` / `:id` / `{{id}}` 路由模板、CORS、局域网令牌、500 条内存日志、`MockPanel.vue`）、G-07 导入导出补齐（Postman Collection v2.1 导出；Insomnia v4 / Bruno / OpenAPI URL 导入；`openapi_sources` 保存来源；实体列与说明映射）、G-06 加密异地备份（`backup_targets` 幂等建表、AES-256-GCM + Argon2id 独立口令、本地目录 / WebDAV 上传与恢复、远端列表、7 份 / 90 天保留、`RemoteBackupPanel.vue`）、G-08 CLI 增 `mock start` 与 `backup upload`；回归（`cargo test` / clippy `-D warnings` / fmt、vitest、vue-tsc、build）全绿。
> - **W3 已落地**：G-08 CLI 补齐 `request send` / `docs export`（复用 service，环境变量渲染、生产环境保护、失败非 0 退出、`--json` 机器可读）；G-09 SSE / WebSocket 调试（`requests.protocol` 随 v12 落库；SSE 解析 event / id / data / retry，Last-Event-ID 自动重连，10MB / 5000 条上限；WebSocket 基于 tokio-tungstenite，文本 / JSON / 二进制消息与可配 ping；`realtime_connect/send/close/status` + `realtime:event` 推送；请求编辑器协议切换与实时面板）；G-10 GraphQL introspection 与 Schema 浏览器（`graphql_schemas` 按项目 + endpoint 缓存、点击字段插入、敏感头不落库）；G-11 Git 友好工作区导出 / 导入（manifest + 项目 / 集合 / 请求 / 环境目录、稳定 JSON、`{{secret:NAME}}`、按 id 合并且冲突报告不覆盖、重复导入幂等、设置页入口）；G-12 Playwright 冒烟 6 条 + 3 页 × 3 种视口视觉回归（阈值 0.2%），CI 新增 e2e job；回归（`cargo test` 314 / clippy `-D warnings` / fmt、vitest 241、vue-tsc、build、Playwright smoke 6/6、visual 9/9）全绿。
> - 与方案的落地差异：文档站与 Mock 均用 `std::net::TcpListener` 自建只读 / 代理线程，**不引入 `axum` / `tower-http`**（控制安装包体积）；WebDAV 复用 `reqwest`，通过 `tauri::async_runtime::block_on` 在同步 service 内驱动异步客户端，未引入额外 HTTP 依赖；备份口令**不落盘**，每次上传 / 恢复输入，CLI 支持 `ASTROMAN_BACKUP_PASSPHRASE`；OpenAPI URL 拉取只接受 JSON（YAML 未做）；Postman 导出把 pre/post script 记入跳过报告，Insomnia / Bruno 导入不保留脚本；W2 新增的 `mock_routes` / `backup_targets` / `openapi_sources` 三张表随 `schema.sql` 幂等创建，W2 阶段未推进 `DB_VERSION`（仍为 11，W3 随协议列升至 12）；文档站前端是项目页的「文档站」弹窗，不是独立页签；契约快照 / diff / 报告中一律剔除示例值（见 4.2）；性能基准未引入 criterion，改为 `bench` feature 门控示例（见 0.6、9.5）。
> - W3 落地差异：实时调试会话保存在单机内存（最多 20 个，关闭后仅可查询状态，进程退出即销毁），CLI `request send` 拒绝 SSE / WebSocket 类型并提示改用桌面端实时面板；SSE / WS 不做抓包与录制；GraphQL 只做 introspection，服务端关闭 introspection 时前端降级为普通编辑器，SDL 手工粘贴留待后续；Git 工作区不做文件监听与自动同步；E2E 采用「Vite SPA + 注入假 Tauri bridge」（`e2e/fixtures/tauri.ts`），不启动 Rust 后端与真实数据库，视觉基线仅 Windows 生成并入库；Playwright 实际版本 1.65.0（方案原写 ^1.49.0）。
> - **W4 已落地**：最低窗口 900×560、<1024 抽屉导航、1023px 以下请求 / 响应压缩、启动骨架、响应 >1MB 关闭着色、减弱动效开关与 `prefers-reduced-motion`；`policy.json` 企业策略（AI / 局域网 / 备份 / 导入格式 / 历史保留，设置页只读展示 + 热重载）；危险路径统一 `canonicalize` 校验（相对路径 / `..` / NUL / 保留名 / 长路径拒绝），`open_external` 协议与 cmd 元字符校验；日志脱敏正则修复，结构化日志覆盖凭据；`E_BUSY` 统一收口，CLI 通过 `gui.pid` 探测 GUI 并提示；SBOM（CycloneDX）与 release 安全断言接入 CI；`installMode: both`、Windows 签名脚本 / macOS 签名 + 公证脚本、CI 可选签名步骤；更新链路补 SHA-256 校验、防降级、`beta` 频道、企业 HTTPS 清单与按安装 ID 的灰度分桶；诊断包补 WebView2 版本、数据目录风险与策略解析错误；兼容矩阵、长稳脚本与 `e2e/enterprise.spec.ts`（纯键盘发送、900×560 无横向滚动）落地。
> - W4 落地差异：主窗口 CSP 仍保留 `script-src 'self' 'unsafe-eval'`（请求前置 / 后置脚本与断言使用 `new Function`，迁移到 Rust 沙箱或独立 worker 属于独立改造，本次未强拆，A-21 记为部分完成）；`db::init` 仍在窗口创建前同步执行，启动骨架只改善感知，后台初始化未改（A-13 / A-17 的 4GB 实机数据待人工回归）；代码签名与 macOS 公证只交付脚本 + CI 可选步骤，证书 / Apple 账号由发布方提供，本地未做真实签名验证；备份 / 恢复的应用级锁文件未实现，当前依赖 SQLite `busy_timeout` + 进程内互斥 + `E_BUSY`。
> - **W5 已落地（2026-11-09）**：AI-03 视觉输入（粘贴 / 拖拽、长边 1568 / JPEG q80 / 单图 ≤1MB / 最多 3 张、按 `supportsVision` 显隐、每图 800 token 预算）；AI-04 + AI-11 原生 function calling（三协议工具定义由单一 JSON Schema 派生，`native_tools` 开关，失败自动回退文本协议）；AI-05 / 06 / 07 成本治理（provider 单价覆盖与币种、月度预算与 80% 提醒 / 可选用尽停止、会话头部 tokens 与成本、Anthropic `cache_control`、摘要专用模型及失败回退）；AI-08 / 09 翻页与回滚（`nextCursor / offset`，写前快照 `request_revisions.trace_id`，按 trace 聚合回滚，工具链只读回放）；AI-10 评测集（`evals/ai/*.json` 20 条 + `npm run ai:eval` 本地运行，命中率 ≥85%、平均轮数 ≤4 才判定通过）；AI-12 可选 provider 回退（默认关闭，首字前失败按设置顺序切换并回报实际模型）；AI-13 发送前预览（服务商 / 模型 / 地址 / 权限 / 提示词 / 历史 / 附件 / 图片 / 工具 / 预算 / 费用，只读且不含密钥）与 `policy.json` 禁用云端 AI；DB 升至 **14**（v13 `ai_providers.native_tools / input_price / output_price / currency`，v14 `request_revisions.trace_id`），设置项随 `settings.json` 落盘。
> - W5 落地差异：评测运行器按 OpenAI 兼容 `chat/completions` 实现，Anthropic / Gemini 直连需先经兼容网关；未配置 `AI_EVAL_*` 时只做用例结构校验，真实模型评测手动执行、未接入 CI；AI 写回滚依赖删除前置快照，删除请求会级联清理其版本记录，删除类操作无法回滚；`disableCloudAi` 以回环地址判定「本地模型」，内网自建模型会被一并拒绝；发送预览里的 token / 费用为估算值，实际以服务商返回用量为准。
> - 未开工：无（W5 已完成）；W4 的 4GB 实机 / 兼容矩阵 / 真实签名与公证、W5 的真实模型评测数据仍待人工回归。
>
> **本次修订要点**（详见各章节）：
> 1. DB 版本基线由 8 校正为 9（实体列已落库）；W1 升至 **10**（版本表 + 契约快照表），W0 再升至 **11**（`environments.is_prod` + `audit_logs`），W3 升至 **12**（`requests.protocol` + `graphql_schemas`），W5 升至 **14**（v13 AI provider 治理列、v14 `request_revisions.trace_id`）（5.3）；
> 2. 明确 PDF 尚未实现，`docs_export` 当前只支持 markdown / html，涉及 PDF 的章节与验收改为「依赖文档导出方案 PDF 阶段」（0.4、4.4、4.8、A-18）；
> 3. 契约校验增加前置步骤「实体字段树 → OpenAPI schema」，否则响应侧 diff 无数据源（4.2）；
> 4. 版本快照纳入实体列并定义回滚合并语义（4.1）；导入导出与实体说明的映射补齐（4.7）；
> 5. Mock 回放运行记录强制沿用同一套脱敏，并保留原状态码（4.3）；
> 6. 补 CLI 与 GUI 并发写库、备份口令独立于主密钥、CSP 与脚本执行的冲突处理（4.6、12.3）；
> 7. 非功能目标按「进程组内存」重新定义，体积门禁提前到第零批，P-03 改为阈值触发（0.3、9.4、13.1、17）。

---

## 0. 结论摘要

### 0.1 一句话

现有能力覆盖了「发请求、抓包、测试、压测、AI、报告」，缺的是把同一份接口定义串成三条链路：

```
                   ┌─ 契约：快照 / diff / schema 校验
requests（唯一源） ─┼─ 复用：文档 / Mock / 造数 / 导入导出
                   └─ 运行：版本历史 / 定时巡检 / 报告告警
```

本方案补 12 个功能缺口与 13 个 AI 专项问题，并把性能、内存、低端设备、兼容性、交付与治理作为同等重要的非功能基线；按「先止血、再底座、后功能、最后交付」排序，共六个批次（完整波次、依赖与完成标志见第 17 章末的排序表）：

| 批次 | 内容 | 判断依据 |
|---|---|---|
| 第零批 | 性能基线实测、分页与虚拟滚动、分档预算、日志与诊断、审计表；AI-01 权限模式、AI-02 取消 | 低风险高收益，先止血，不改业务行为；AI 两个 P0 必须在写功能前收紧 |
| 第一批 | 请求版本历史、契约校验、文档发布 | 直接建立在刚完成的「接口文档导出」之上，投入产出比最高 |
| 第二批 | 本地 Mock Server、Faker 变量、导入导出补齐、加密异地备份 | 让接口定义真正被复用，并补齐本地优先的安全底座 |
| 第三批 | CLI 对齐、SSE/WebSocket、GraphQL Schema、Git 友好工作区、E2E | 工程化与协议广度，依赖前两批的 service 层沉淀 |
| 第四批 | 低端适配、CSP 收紧、签名与自动更新、离线交付、兼容矩阵回归 | 企业交付前必须过线，不能靠「用户机器应该没问题」 |
| 第五批 | AI 视觉、原生 function calling、成本治理、分页与回滚、评测集 | 依赖版本表（第一批）与发布链（第四批）；不重做底座，只补边界与可控性 |

### 0.2 设计原则

1. **请求定义是唯一源**：文档、Mock、契约、测试、压测全部从 `requests` 派生，不维护第二份接口描述；字段结构以 `requests.request_entity / response_entity`（含用户回填的字段说明）为 schema 源，导出与契约校验都从这里转换，不再各自解析示例。
2. **核心逻辑下沉为 service**：新增能力不再直接写在 `commands/*.rs` 里，而是 `services/*` 纯 Rust 函数（入参 `&Pool` + 配置），Tauri 命令与 CLI 只是薄适配层。
3. **本地优先**：Mock、文档站默认只绑 `127.0.0.1`；需要局域网时显式开启并加访问令牌，不做默认对外暴露。
4. **增量迁移 + 判存补列**：新增表走 `schema.sql` + `COLUMN_MIGRATIONS`，不重建库；新列不进现有 `SELECT` 列序号，避免 `map_detail` 错列。
5. **敏感信息不出边界**：Mock / 文档 / 导出 / 备份全部沿用既有脱敏规则；异地备份必须加密。
6. **不做重复造轮子**：文档渲染、OpenAPI 构建、运行记录、报告生成、密钥模块全部复用，不新起一套。
7. **非功能与功能同等交付**：内存、低端设备、兼容性、签名与更新、可诊断性都要有目标值和验收项，不能等「上线后再优化」。

### 0.3 企业级非功能目标

功能补齐只解决「有没有」；企业级还要回答「4GB 办公机上稳不稳、离线内网装不装得上、出了问题查不查得到」。本方案把非功能项列为一等交付物，目标值如下（标 `*` 的是预算值：性能项已在 W0 实测，内存预算待 4GB 实机校准）：

| 维度 | 目标 | 验证方式 |
|---|---|---|
| 冷启动 | 8GB/SSD ≤ 3s 到可交互；4GB/HDD ≤ 6s | 启动打点 P95 |
| 空闲内存 | 无抓包：**进程组**（主进程 + WebView2 子进程）≤ 240MB*；抓包满载（5000 条 / 96–128MB）≤ 500MB* | 任务管理器按进程组求和（单看主进程会低估 WebView2 的 80–150MB 基数） |
| 内存泄漏 | 空闲 30 分钟进程组 RSS 增长 ≤ 20MB | 长稳脚本 |
| 大响应 | 5MB JSON 打开 ≤ 2s，可另存，不崩溃 | 手工 + E2E |
| 安装包 | NSIS ≤ 25MB；增量更新 ≤ 8MB | CI 体积门禁（W0 已接入，不等第四批） |
| 数据安全 | 凭据加密、备份加密、导出脱敏、日志脱敏 | 用例 + 审计 |
| 兼容性 | Win10 1809+ / Win11 x64、x86、macOS 10.15+、离线安装 | 兼容矩阵（第 11 章） |
| 可运维 | 结构化日志 + 一键诊断包（脱敏） | 验收 A-22 |
| 可访问 | 200% 缩放不破版；纯键盘可完成核心流程 | 验收 A-23 |
| 隐私 | 默认全本地、无遥测、AI 调用可感知 | 设置页明示 |

### 0.4 与既有方案的关系

| 文档 | 关系 |
|---|---|
| 《AstroMan 接口文档导出技术方案》 | 提供文档 IR、HTML 渲染、示例值取值链、预览与草稿；**PDF 与 OpenAPI 仍属该方案后续阶段，当前 `docs_export` 只支持 markdown / html**。本文涉及 PDF 的章节均显式标注依赖，不默认第一批可用 |
| 本文 | 在文档导出之上补版本、契约、Mock、发布、备份与工程化；并新增企业级非功能基线 |
| `docs/roadmap.md` | Flow / 定时巡检 / 分布式压测 / 团队同步 / Linux 桌面不在本文实现，只说明衔接点 |

### 0.5 实现进度总览（2026-11-08）

> 逐项对照本文的实现状态。✅ = 代码已落地并通过回归；⬜ = 未开工。行内链接指向对应章节。

| 章节 | 能力 | 状态 | 主要落点 |
|---|---|---|---|
| 2.1 | `services` 分层抽取 | ✅ | `services/{request,revision,contract,openapi,docs,docs_site}.rs`；`commands/*` 保留原签名为薄适配 |
| 4.1 | G-01 请求版本历史（硬 50 条、diff、合并回滚） | ✅ | `services/revision.rs`、`commands/revision.rs`、`RequestHistoryDrawer.vue` |
| 4.2 | G-02 实体 → OpenAPI schema、契约快照 / diff / 报告 | ✅ | `services/openapi.rs`、`services/contract.rs`、`commands/contract.rs`、`ContractPanelDrawer.vue` |
| 4.3 | G-04 本地 Mock Server | ✅ | `services/mock/{store,router,server}.rs`、`commands/mock.rs`、`MockPanel.vue`；标准库 TCP，无 axum |
| 4.4 | G-03 只读文档站（标准库 HTTP，无 axum） | ✅ | `services/docs_site.rs`、`commands/docs_site.rs`、`DocsSiteModal.vue` |
| 4.5 | G-05 Faker / 造数变量 | ✅ | `services/faker.rs`、`util::render`；请求栏 faker 速查与计数 |
| 4.6 | G-06 加密异地备份 | ✅ | `services/backup_crypto.rs`、`services/backup_target.rs`、`commands/backup.rs`、`RemoteBackupPanel.vue` |
| 4.7 | G-07 导入导出补齐 | ✅ | `commands/transfer.rs`（Postman 导出 / Insomnia / Bruno / OpenAPI URL）、`services/openapi.rs` |
| 4.8 | G-08 CLI 全量对齐 | ✅ | `cli.rs`：`contract check` / `mock start` / `backup upload` / `request send` / `docs export`，均调用 `services`，不复制业务逻辑 |
| 4.9 ~ 4.12 | G-09 SSE/WS、G-10 GraphQL、G-11 Git 工作区、G-12 E2E | ✅ | `services/realtime.rs`、`services/graphql.rs`、`services/workspace.rs`、`commands/{realtime,graphql,workspace}.rs`；`RequestEditor.vue` 协议切换 / 实时面板 / Schema 浏览器；`e2e/` + `playwright.config.ts` |
| 5.1 | `request_revisions` / `contract_snapshots` / `audit_logs` / `mock_routes` / `backup_targets` / `openapi_sources` / `graphql_schemas` 建表 | ✅ | `schema.sql`（`DB_VERSION = 14`；W2 / W3 新表幂等创建） |
| 5.2 | `environments.is_prod`、`requests.protocol`、AI provider 治理列、`request_revisions.trace_id` | ✅ | v11 落 `is_prod`；v12 落 `requests.protocol`（http / graphql / sse / websocket）；v13 落 `native_tools` / 单价 / 币种；v14 落 `trace_id` |
| 9.3 P-01 | HAR 导出流式化 + 仅元数据 + 64MB 保护 | ✅ | `capture/har.rs`、`capture/mod.rs`、`commands/capture.rs` |
| 9.3 P-02 | 抓包树虚拟滚动（混合行高） | ✅ | `utils/captureTree.ts`、`views/Capture.vue` |
| 9.3 P-03 | 项目接口表阈值分页（>300 条分页 50） | ✅ | `utils/tablePaging.ts`、`views/Project.vue` |
| 9.3 P-05 | 运行档位与抓包预算（低 / 标准 / 高） | ✅ | `services/profile.rs`、`state.rs`、`Settings.vue` |
| 9.3 P-06 | SQLite 连接参数（cache / synchronous / mmap） | ✅ | `db/mod.rs::connection_pragmas`、`state.rs` |
| 9.5 | 性能基线 4 项 + RSS 采样脚本 | ✅ | `examples/baseline.rs`、`scripts/measure-rss.ps1` |
| 12.4 | 审计表、90 天清理、AI 决策审计 | ✅ | `services/audit.rs`、`commands/diagnostics.rs`、`commands/ai.rs` |
| 12.5 | 结构化日志、panic 落盘、诊断包 | ✅ | `logging.rs`、`services/diagnostics.rs`、`commands/backup.rs` |
| 10 | 低端适配（窗口、断点、响应降级、动效） | ✅ | `tauri.conf.json` 900×560、`App.vue` 抽屉断点、`RequestEditor.vue` 1023px 压缩、`index.html` 启动骨架、`ResponseBody.vue` 1MB 阈值、`Settings.vue` 动效开关；`e2e/enterprise.spec.ts` |
| 11 | 兼容矩阵与长稳方案 | ✅ | `docs/compatibility.md` 回归矩阵 / 长稳步骤、`scripts/measure-rss.ps1` 门禁参数；实机数据待人工回归 |
| 12.1 ~ 12.3 | 安装模式、签名、更新、安全基线 | ✅（CSP 部分） | `installMode: both`、`scripts/sign-windows.ps1`、`scripts/notarize-macos.sh`、`commands/update.rs`（摘要 / 频道 / 清单 / 灰度）、路径校验、`E_BUSY`、SBOM；CSP `unsafe-eval` 保留 |
| 12.4 | 企业策略 `policy.json` | ✅ | `services/policy.rs`、`state.rs`、`commands/settings.rs`、`Settings.vue` 企业策略页、`policy.example.json` |
| 12.5 ~ 12.6 | 诊断增强、无障碍 | ✅ | 诊断包补 WebView2 / 数据目录风险 / 策略错误；`focus-visible` 强制描边、键盘 E2E |
| 13 | CI 预算与确定性门禁 | ✅ | `security:check`、`sbom` 接入 CI；release DevTools 断言 |
| 14 | 企业交付清单 | ✅ | 见 §17 第四批与 `docs/enterprise.md` |
| 15 | AI 专项（AI-01 ~ AI-13） | ✅ | 权限 / 取消 / 视觉 / 原生 FC / 成本 / 分页回滚 / 评测 / 回退 / 预览全部落地；真实模型评测数据待配置后采集 |
| 17 | 第一批验收 A-01 ~ A-06 | ✅ | 见 §18 验收表 |
| 17 | 第四批验收 A-13 ~ A-23 | 部分 ✅ | 自动化与代码项已过；4GB / x86 / 无 Edge / 真实签名待人工回归，见 §18 |

### 0.6 W0 止血与基线（✅ 2026-11-08）

| 项 | 结果 | 落点 / 差异 |
|---|---|---|
| P-06 SQLite 参数 | ✅ | `connection_pragmas()`：WAL / `foreign_keys` / `busy_timeout=5000` / `cache_size=-16000`（16MB）/ `synchronous=NORMAL`；64 位构建额外 `mmap_size=128MB`，32 位不加 |
| P-03 项目接口表 | ✅ | >300 条自动分页 50，≤300 条保持一屏看全；「全部展开」二次确认 |
| P-05 内存分档 | ✅ | `services/profile.rs`：≤6GB 低档（2,000 条 / 32MB / 单响应 2MB）、6~16GB 标准（5,000 / 96MB / 5MB）、≥16GB 高档（5,000 / 128MB / 5MB）；启动时定档，设置页可覆盖，保存后立即改抓包预算 |
| 结构化日志 | ✅ | 一行一个 JSON（`ts` / `level` / `target` / `msg`），写盘前统一脱敏；按天 + 单文件 5MB 滚动，保留 7 天 |
| panic 落盘 | ✅ | `logs/panic-YYYYMMDD.log`，JSON 行保留线程与位置；同时保留默认 stderr 堆栈 |
| 审计 | ✅ | `audit_logs(created_at, action, target, result, detail, trace_id, actor)`；启动清理 90 天前记录，`detail` 超过 8KB 截断；`audit_log_list` / `audit_log_cleanup` 命令；AI 发送决策与写工具确认已接入 |
| 诊断包 | ✅ | `diagnostics_export`：ZIP 含 `README.txt`、`summary.json`、`settings.json`、`logs/recent.log`；代理口令、主目录与用户名掩码；不采集任何凭据 |
| AI-01 | ✅ | 前后端同一判定；后端 `ai_execute` 复核并返回 `E_AI_DENY` / `E_AI_CONFIRM`，生产环境 GET 也强制确认 |
| AI-02 | ✅ | `tool_cancel(trace_id)` 复用已有 HTTP 取消注册表（不新建任务表）；批量在每个请求前检查取消；写工具取消后按 `unknown` 处理，禁止自动重试 |
| CI 门禁 | ✅ | `npm audit`（high）、`dist` ≤8MB、`cargo audit`、NSIS `*setup.exe` ≤25MB、三处版本一致性；基准示例单独 `cargo check` 防漂移 |
| 性能基线 | ✅ | `examples/baseline.rs`（feature `bench`）；release 实测：openapi_build 500 接口 **24.21ms**、docs_render 200 接口 **23.07ms**、revision_diff **0.03ms**、render_tracked 1MB **11.11ms**，均低于 500 / 800 / 50 / 20ms 目标线 |
| RSS 采样 | ✅ | `scripts/measure-rss.ps1`：主进程 + WebView2 子树 Working Set，输出首样本 / 末样本 / 峰值 / 增长，可导出 CSV |

**与方案的差异**：① 基准没有引入 criterion（避免为 W0 门禁增加依赖体积与编译时间），改为 `bench` feature 门控示例，指标与目标线不变，默认只记录、`-- --check` 才阻断；② AI-02 未新建 `task_cancel` 注册表，直接复用 `http_cancels`，`trace_id` 即 `requestKey`；③ 审计目前只提供后端命令，前端审计查询页不在 W0 验收内；④ 安装包体积门禁只拦 `*setup.exe`，`offline` 安装包单独发布、不计入门禁。

---

## 1. 现状判断

### 1.1 已经很强的部分

| 能力 | 现状 |
|---|---|
| 请求调试 | 方法、Header、Query、Body、认证、脚本、断言、Cookie、代理、证书、拦截、Map Local / Map Remote |
| 环境 | 多环境、敏感变量加密、环境脚本 |
| 抓包 | MITM、HTTPS 解密、过滤、远程映射、请求克隆 |
| 测试 | 测试套件、数据集、断言、报告（HTML / JSON / JUnit）、定时任务雏形 |
| 压测 | RPS 阶梯、延迟分布、错误率、采样与报告 |
| 数据 | 项目 / 集合 / 请求、导入（HAR / Postman / Apifox / OpenAPI）、项目包导出 |
| 平台 | AI 助手、CLI、备份、系统监控、命令面板、笔记 |

数据表已覆盖 20+ 实体（[schema.sql](file:///d:/Software/ProjectFiles/AstroMan/src-tauri/src/db/schema.sql#L5-L493)），命令注册 80+ 个（[lib.rs](file:///d:/Software/ProjectFiles/AstroMan/src-tauri/src/lib.rs#L227-L287)）。

### 1.2 结构性缺口

不是「少一个按钮」，而是三处断点：

1. **接口定义没有版本**：保存即覆盖，改坏无法回滚，也无法解释「文档为什么变了」。
2. **接口定义没有被复用**：文档刚补上，但 Mock、契约校验、造数仍然各自为战或缺失。
3. **能力只活在 GUI 里**：CLI 只覆盖测试 / 报告 / 套件，文档、契约、Mock 进不了 CI。

---

## 2. 总体架构

### 2.1 分层

```
┌────────────────────────────── 入口层 ─────────────────────────────┐
│  Vue 页面（项目 / 请求编辑 / Mock / 契约 / 设置）                  │
│  Tauri commands（薄封装，只做参数校验 + State 注入）                │
│  astroman-cli（同一 service 层，输出 JSON 供 CI）                   │
└───────────────────────────────┬───────────────────────────────────┘
                                │
┌────────────────────────────── Service 层 ─────────────────────────┐
│  services::docs      services::mock       services::contract      │
│  services::revision  services::backup     services::faker         │
│  services::transfer  services::realtime   services::graphql       │
│  统一入参：&SqlitePool + AppConfig；不依赖 tauri::State            │
└───────────────────────────────┬───────────────────────────────────┘
                                │
┌────────────────────────────── 数据层 ─────────────────────────────┐
│  requests / collections / environments / request_runs             │
│  request_revisions / contract_snapshots / mock_routes             │
│  backup_targets / graphql_schemas                                   │
└───────────────────────────────────────────────────────────────────┘
```

`services` 是本次方案的关键新增：现有 `commands/request.rs` 已经承担了不少业务逻辑，再往里塞 Mock、契约会让 CLI 完全复用不了。建议先做一次**只搬家不改行为**的抽取，再上新功能。

### 2.2 能力矩阵

| 缺口 | 复用现有 | 新增 |
|---|---|---|
| 版本历史 ✅ | `requests` 读写、`request_history` 模式 | `request_revisions` 表、diff 算法（已实现） |
| 契约校验 ✅ | OpenAPI 构建（[transfer.rs](file:///d:/Software/ProjectFiles/AstroMan/src-tauri/src/commands/transfer.rs#L455-L712)） | 快照表、diff 规则、报告（已实现） |
| 文档发布 ✅ | 文档导出 HTML（PDF 未实现，见 4.4） | 只读静态服务（已实现） |
| Mock Server | 运行记录、请求定义、文档 IR | 路由表、HTTP 服务、日志 |
| Faker | `util::render` 变量替换 | 生成器与语法 |
| 异地备份 | `backup.rs` 打包、`security.rs` 加密 | 目标配置、上传、加密包 |
| 导入导出 | `importer.rs`、项目包 | Postman 导出、Insomnia / Bruno、OpenAPI URL |
| CLI | `cli.rs` 现有子命令 | 共享 service + 新子命令 |
| SSE / WebSocket | HTTP 传输层 | 协议列、连接管理、时间线 |
| GraphQL Schema | GraphQL 请求体封装 | introspection、缓存、浏览与补全 |
| Git 工作区 | 项目包 / 导入导出 | 目录结构、稳定 ID、合并报告 |
| E2E | 现有 vitest | Playwright 冒烟与视觉回归 |

### 2.3 企业级基线约束

新增的每个 service 都必须满足下面 7 条，否则不允许进第一批：

1. **惰性初始化**：Mock、文档站、实时连接、AI、CLI 运行时都不能出现在启动路径上；启动只做 DB init + 迁移。
2. **有界内存**：任何缓存都必须有上限与淘汰策略；禁止无界 `Vec`、无限增长的事件缓冲、只增不减的日志。
3. **可取消**：超过 2s 的任务返回句柄/状态对象，支持取消；应用退出时统一 `join`，不留后台线程。
4. **流式优先**：导出、备份、导入、加密不得把全量数据一次性读进内存（现有 `zip`、`aes-gcm`、`argon2` 已在依赖树，可直接复用）。
5. **失败可解释**：错误 = 错误码 + 中文描述 + 下一步建议；禁止只抛 `E_XXX`。
6. **不引入重依赖**：新增 crate 必须写体积与构建时间评估；能在现有依赖上做就不加（`sysinfo` 已可用于内存分档）。
7. **无 GUI 可测**：service 是纯函数，能在 `cargo test` 下覆盖，不依赖 `tauri::State`。

---

## 3. 缺口清单

| 编号 | 缺口 | 优先级 | 批次 | 直接影响 |
|---|---|---|---|---|
| G-01 ✅ | 请求版本历史与回滚 | P0 | 一（已完成） | 改坏接口无法恢复；文档/Mock/契约都失去可信基线 |
| G-02 ✅ | 契约快照、OpenAPI diff、breaking change | P0 | 一（已完成） | 发版前无法判断「这次改动会不会打挂调用方」 |
| G-03 ✅ | 文档发布（只读站点） | P1 | 一（已完成） | 只能发文件，无法像 Apifox 一样给链接 |
| G-04 | 本地 Mock Server | P0 | 二 | 不能契约先行，前端只能等后端 |
| G-05 | Faker / 造数变量 | P1 | 二 | 文档示例、Mock、数据驱动都缺同一套假数据 |
| G-06 | 加密异地备份 | P1 | 二 | 备份只在本机目录，磁盘损坏即全丢 |
| G-07 | 导入导出补齐 | P1 | 二 | 迁移仍有关卡；Postman 用户无法带走数据 |
| G-08 | CLI 能力对齐 | P1 | 三 | 文档/契约/Mock 无法进 CI |
| G-09 | SSE / WebSocket 调试 | P2 | 三 | 实时接口只能靠外部工具 |
| G-10 | GraphQL Schema 感知 | P2 | 三 | GraphQL 只是「拼 JSON」，无补全、无文档 |
| G-11 | Git 友好工作区 | P2 | 三 | 团队协作只能靠整库文件，冲突不可读 |
| G-12 | 前端 E2E / 视觉回归 | P2 | 三 | 20 个页面靠手工验收，回归成本高 |

---

## 4. 详细设计

### 4.1 G-01 请求版本历史（✅ 已实现，2026-11-08）

**目标**：每次保存留一个不可变快照，可查看 diff、可回滚；默认只保留最近 50 个版本（硬上限）。

**数据表**：

```sql
CREATE TABLE IF NOT EXISTS request_revisions (
  id            INTEGER PRIMARY KEY AUTOINCREMENT,
  request_id    INTEGER NOT NULL,
  revision      INTEGER NOT NULL,       -- 从 1 递增
  snapshot_json TEXT    NOT NULL,       -- 完整请求定义（auth 保持密文）
  source        TEXT    NOT NULL,       -- save | import | restore（ai 待接入；reorder 不改快照字段，不记版本）
  created_at    INTEGER NOT NULL,
  UNIQUE(request_id, revision)
);
CREATE INDEX IF NOT EXISTS idx_revisions_request
  ON request_revisions(request_id, revision DESC);
```

**关键规则**：

- 快照只存「请求定义」，不存运行结果；`auth` 原样保存数据库里的密文，回滚后仍由现有解密链路处理。
- 快照**包含实体列** `request_entity` / `response_entity`（v9 已落库）：字段结构、用户回填的说明、手工字段与 `hiddenPaths` 都属于接口定义，缺了它们文档和契约回滚后会对不上。
- 写入时机：`request_save` / `request_save_quick` 成功提交后追加；导入与回滚也记一条并标注 `source`（AI 写操作入口落地后接入；排序不改变快照字段，不记版本）。实体由发送后自动同步更新时**不单独生成版本**，随下一次保存一并落快照，避免每发一次请求就多一条版本。
- 正文超 1 MB 时截断快照正文并标记 `truncated`，避免版本表膨胀；截断版本只允许查看，回滚时提示「正文不完整」。实体列超过 256 KB 时同样截断并标记。
- 清理策略：保存后按 `request_id` 保留最近 50 条（硬上限，超出即删最旧）；随请求删除级联清理。自动保存会产生较多版本，不单独按时间保留，避免频繁保存时版本表无限累积。
- diff 展示按字段比较：名称、方法、URL、Header、Query、Body、Auth（只比较类型与是否变化，不展示明文）、脚本、断言、请求实体、返回实体；数组按 `name` 对齐，缺失项标新增/删除。实体按点号路径对齐（数组路径带 `[]`），只比较「字段是否新增/删除/类型或必填变化/说明变化」，说明变化默认归入「提示」级。
- 回滚 = 读取快照 → 走正常 `request_save`（因此也会生成一条新版本，天然可再退）。**实体回滚按合并语义执行**：结构以快照为准，但当前实体里 `manual = true` 的说明、手工字段与必填标记优先保留，避免一次回滚把用户后填的字段说明全部冲掉；被当前 `hiddenPaths` 屏蔽的字段不回滚回来。

**命令**：

| 命令 | 入参 | 出参 |
|---|---|---|
| `request_revision_list` | `requestId`, `limit` | `RevisionBrief[]`（版本号、来源、时间、是否截断） |
| `request_revision_get` | `requestId`, `revision` | 快照 + 脱敏展示体 |
| `request_revision_diff` | `requestId`, `from`, `to` | `RevisionDiff`（字段级 changes） |
| `request_revision_restore` | `requestId`, `revision` | 新的 `RequestDetail` |

**前端**（✅ 已落地）：请求编辑页顶部「历史」按钮打开抽屉（`RequestHistoryDrawer.vue`），左侧版本列表、右侧 diff，支持查看完整快照；「恢复此版本」走二次确认。落地形态是抽屉而非页签。

**测试**：保存 3 次后列表正确；diff 覆盖增删改；回滚后请求内容与快照一致且 auth 可正常解密；实体回滚保留用户后填的说明与手工字段、但不恢复已删除字段；超限清理只留 50 条；删除请求后无孤儿版本。

---

### 4.2 G-02 契约快照与变更检测（✅ 已实现，2026-11-08）

**目标**：为项目建立 OpenAPI 基线，发版前对比当前接口定义，输出「破坏性 / 兼容性 / 提示」三类结果；后续可扩展为响应 schema 校验。

**第 0 步（前置，必须先做）：把实体字段树升级为 OpenAPI schema**

现有 `openapi_export` 只输出 `requestBody` 的示例、`responses` 恒为 `{ "200": { "description": "OK" } }`，既没有请求 schema，也没有任何响应 schema。因此本文下面「删除响应字段 / 删除状态码 / 新增响应字段」三类判定**当前没有数据源**，不补这一步契约功能只剩一半。

复用 v9 已落库的 `request_entity` / `response_entity`（含用户回填的说明），在 `transfer.rs` 里做一次结构转换：

| 实体节点 | OpenAPI schema |
|---|---|
| `object`（含子节点） | `type: object` + `properties` + `required`（取 `required = true` 且非 `stale` 的子节点） |
| `array` | `type: array` + `items`（由 `itemType` 或子节点推断）；空数组写 `items: {}` |
| `string / number / boolean / null` | 对应 `type`；`null` 写 `nullable: true` |
| `file` | `type: string` + `format: binary` |
| `unknown` 或类型冲突 | `{}`（不写 `type`），并保留说明，避免误判 breaking |
| 节点 `description` | 原样写入 `description`（用户填写的字段说明在此复用，无需第二份映射表） |

- 请求实体 → `requestBody.content[<mime>].schema`，示例仍取当前示例值链；
- 返回实体 → `responses["200"].content["application/json"].schema`；状态码暂固定 200，阶段二再用最近一次 2xx 运行记录扩展多状态码；
- 没有实体的接口退回旧行为（只输出示例），并在 `contract_check` 结果里标 `schemaMissing`，提示「该接口无实体，响应侧不参与判定」，不当作 breaking；
- 同一份转换逻辑同时供 `openapi_export` 使用，导出给外部的 spec 也从「只有示例」升级为「有 schema 与字段说明」。

**快照归一化（生成前必须执行）**

diff 的敌人是噪声。生成 `spec_json` 前统一走一遍 `normalize_spec`：

1. 所有对象键按字典序输出，数组保持 `sort_order, id` 的稳定顺序（现有 OpenAPI 构建已用 `ORDER BY sort_order ASC, id ASC`，顺序本身稳定，可直接沿用）；
2. `servers` 固定为占位值，不写真实环境地址；
3. 示例值一律从快照中剔除（`example` / `examples` 全部删除），不参与 diff、不写入快照与报告；文档渲染仍按示例值链展示，两者共用同一份实体但互不污染；`description` 变化归入「提示」；
4. 变量占位统一成 `{{var}}` 形式（环境变量取值不固化进快照）；
5. 空可选字段、`null` 与缺失统一视为同一种状态，避免「补了个 null 就判破坏」；
6. `x-` 扩展字段排序后保留，但不参与判定（除非显式开启）。

**数据表**：

```sql
CREATE TABLE IF NOT EXISTS contract_snapshots (
  id            INTEGER PRIMARY KEY AUTOINCREMENT,
  project_id    INTEGER NOT NULL,
  label         TEXT    NOT NULL,      -- 如 v1.2.0 / 2026-11-06
  spec_json     TEXT    NOT NULL,      -- 归一化后的 OpenAPI 3.0（含实体派生 schema，示例已剔除）
  request_count INTEGER NOT NULL,
  source        TEXT    NOT NULL,      -- project | import
  created_at    INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_contract_project
  ON contract_snapshots(project_id, created_at DESC);
```

**diff 规则**（只覆盖 OpenAPI 3.0 常用子集，避免误报）：

| 级别 | 判定 |
|---|---|
| 破坏性 | 删除 path / operation；新增必填请求参数；参数 `required` 由 false→true；参数类型或 format 不兼容；枚举值被删除；删除响应字段；删除状态码；新增全局鉴权要求 |
| 兼容性 | 新增 path / operation；新增可选参数；`required` 由 true→false；新增响应字段；新增枚举值（标为 warning，客户端可能未处理） |
| 提示 | 描述、summary、示例变化；标记 deprecated |

**命令与输出**：

```rust
pub struct ContractDiff {
    pub breaking: Vec<ContractChange>,   // {path, method, field, kind, before, after}
    pub compatible: Vec<ContractChange>,
    pub warnings: Vec<ContractChange>,
    pub summary: ContractSummary,        // 各计数
}
```

- `contract_snapshot_create(projectId, label)`：用现有 OpenAPI 构建逻辑生成 → `normalize_spec` → 入库；
- `contract_snapshot_list(projectId)`；
- `contract_check(projectId, baselineId)`：产出 diff；无实体的接口在结果里标 `schemaMissing`，不计入 breaking；
- `contract_report(projectId, baselineId, format)`：复用报告渲染器输出 Markdown / HTML；
- CLI：`astroman-cli contract check --project 3 --baseline 12 --fail-on breaking`，非 0 退出码供 CI 使用。

**默认基线**：最近一次手工创建的快照；不自动把每次导出当基线，避免噪声。

**阶段二扩展**：用最近一次运行记录的响应体对 OpenAPI response schema 做轻量校验（仅支持 `type / required / properties / items / enum` 子集），不一致记为 `schemaMismatch`，不阻断 diff。

**测试**：实体树 → schema 的 6 类节点转换（对象 / 数组 / 标量 / file / unknown / 说明）；同一项目连续生成两次快照 diff 为空（归一化生效）；固定两份 spec fixture 覆盖 10 类变更；删除响应字段、删除状态码均判破坏（依赖实体 schema）；新增可选参数不判破坏；删除枚举判破坏；无实体的接口标 `schemaMissing` 且不阻断；CLI 在破坏性变更时退出码非 0。

---

### 4.3 G-04 本地 Mock Server（✅ 已落地，2026-11-08）

**目标**：从项目接口定义直接起本地 HTTP 服务，前端联调不依赖后端；默认只监听 `127.0.0.1`。

**依赖**：方案原定 `axum` + `tower-http`，实际为控制安装包体积**未引入**，改用 `std::net::TcpListener` + 线程处理连接，路由模板、CORS、日志与令牌均在 `services/mock/server.rs` 内手写；服务运行在进程内，随应用生命周期结束。

**数据表**：

```sql
CREATE TABLE IF NOT EXISTS mock_routes (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  project_id  INTEGER NOT NULL,
  request_id  INTEGER,                 -- 为空表示手写路由
  name        TEXT    NOT NULL,
  method      TEXT    NOT NULL,
  path        TEXT    NOT NULL,        -- /api/users/{id}
  status_code INTEGER NOT NULL DEFAULT 200,
  headers_json TEXT   NOT NULL DEFAULT '[]',
  body_type   TEXT    NOT NULL DEFAULT 'json',
  body        TEXT    NOT NULL DEFAULT '',
  delay_ms    INTEGER NOT NULL DEFAULT 0,
  enabled     INTEGER NOT NULL DEFAULT 1,
  sort_order  INTEGER NOT NULL DEFAULT 0,
  updated_at  INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_mock_project ON mock_routes(project_id, enabled);
```

**响应来源优先级**：

1. `mock_routes` 手工配置（可改状态码、Header、延迟、Body）；
2. 该请求最近一条**成功或失败**运行记录（原状态码 + 响应头（脱敏）+ 响应体（脱敏））。保留非 2xx 记录，才能 mock 错误分支；脱敏规则与文档导出一致，`Authorization`、`Set-Cookie`、`token`、手机号 / 邮箱等一律掩码，禁止把生产数据原样回放；
3. 无运行记录时，按请求 Body / 参数生成骨架 JSON，并标 `X-Mock-Generated: 1`。

手工路由的 Body 由用户自己填写，不强制脱敏（那是用户主动构造的假数据）；只要来自运行记录或实体推断，就必须走脱敏链路。

**路由匹配**：

- 支持 `{id}`、`:id`、`{{id}}` 三种占位写法，匹配时转成正则；
- 先精确匹配，再模板匹配；同 method + path 冲突时按 `sort_order` 取第一条并在 UI 标冲突；
- 默认开启 CORS（`*`），支持 `OPTIONS`；
- 未命中返回 `404 {"code":"MOCK_NOT_FOUND","path":...}`，便于排查。

**生命周期与安全**：

- `mock_start(projectId, port, host, options)`：端口占用预检，冲突返回 `E_MOCK_PORT`；
- `host` 默认 `127.0.0.1`；切换到 `0.0.0.0` 需前端二次确认，并生成一次性访问令牌（`X-AstroMan-Mock-Token`），未带令牌返回 401。令牌只用于前端联调，**不强制**：局域网模式下首次访问带 `?token=` 会种一个 HttpOnly Cookie，后续请求免带 header；`?token=` 可能进代理/访问日志，UI 必须提示「仅内网使用，用完即关」；
- `mock_stop(projectId)`、`mock_status(projectId)`；同一项目只允许一个实例，全局最多 3 个；
- 请求日志保存在内存环形缓冲（最近 500 条），不落库；UI 展示方法、路径、状态、耗时、命中规则；
- 关闭项目或退出应用时自动 stop；进程崩溃后台无残留（服务随进程生命周期）。

**与抓包 Map Local 的区别**：Map Local 是「拦截并替换某个真实请求」，Mock Server 是「独立提供一整套接口」，两者共用响应构造逻辑但不合并入口。

**前端**：项目页顶部新增「Mock」按钮，打开 `MockPanel.vue` 抽屉（未做成独立页签）：支持启停、端口（0 = 自动分配）、监听地址、令牌复制、「从项目生成路由」、路由增删改、清空、状态 / 日志每 2.5 秒轮询；路由编辑含方法、路径、状态码、响应类型、延迟、启用开关与 Body。

**实现状态**：`mock_routes` 随 `schema.sql` 幂等创建；`services/mock/store.rs` 负责路由与运行记录读取，`router.rs` 负责模板匹配与响应构造，`server.rs` 负责 TCP 生命周期、CORS、令牌与 500 条日志环形缓冲；响应优先级与脱敏口径按本节实现，回放运行记录沿用同一套 `SecretSet` 脱敏。

**测试**：路径模板匹配；方法不匹配 405；延迟生效；CORS 预检；令牌校验与 Cookie 下发；端口冲突；停止后端口释放；回放运行记录时凭据与手机号 / 邮箱被脱敏；非 2xx 记录保留原状态码。

---

### 4.4 G-03 文档发布（只读站点，✅ 已实现，2026-11-08）

**目标**：复用文档导出产物，起一个只读 HTTP 服务，给同事一个链接，而不是发文件。

**实现**：

- `docs_site_start(projectId, environmentId, port, host)`：调用 `services::docs::builder::build` + `docs::render_html::render` 渲染一次，页面常驻内存；HTTP 由 `std::net::TcpListener` 起的只读线程直接返回，**不落盘、不引入 `axum`**，空闲时不占运行时；
- 默认单页 `/`；接口数 > 200 且存在多个顶层分组时按分组拆分为多页（`/c/{groupId}.html`），首页是目录；同一时刻只保留一个站点实例，重复启动会替换旧实例并释放端口；
- 「刷新」= 重新读取项目、重渲染并替换内存页面，不重启服务、URL 不变；
- 只读：只处理 GET / HEAD，其它方法 405 并带 `Allow: GET, HEAD`；路径穿越返回 403，未知路径 404；单次请求头上限 16KB；
- 默认 `127.0.0.1`；局域网需显式开启并生成一次性令牌，支持 `?token=`、`X-AstroMan-Docs-Token`、`Authorization: Bearer` 与 `astroman_docs_token` HttpOnly Cookie 四种带入方式，未授权返回 401；UI 明示「仅内网使用，用完即关」；
- 停止即释放端口；服务随进程生命周期结束，不写用户数据目录；
- 顶部固定提示「本文档由 AstroMan 生成 · {YYYY-MM-DD HH:MM} · 环境 {name|未选择} · 只读站点」；
- `/health` 仅在 loopback 请求下返回 `{name, version, startedAt}`，方便脚本探活。

**范围说明**：本批只做 HTML 只读站点。`docs_render` / `docs_export` 当前只支持 `markdown` 与 `html`，PDF 属于《接口文档导出技术方案》的后续阶段；在 PDF 落地前，文档站不提供 PDF 下载入口，CLI 的 `--format pdf` 也不进入第一批完成标志。

**命令**：`docs_site_start / docs_site_stop / docs_site_status / docs_site_refresh`。

**前端**：项目页顶部「文档站」按钮打开弹窗（`DocsSiteModal.vue`），未启动时填端口 / 局域网开关，运行中显示 URL、访问范围、接口数、页面数、启动时间与令牌，并提供复制链接（含令牌）、刷新内容、重启为当前项目、停止服务；**关闭弹窗不会停止服务**，需显式点「停止服务」。

**测试**：启动后 GET 首页 200；HEAD 可用；未知路径 404；POST 405；路径穿越被拒；局域网模式无令牌 401、带令牌 200；刷新后内容变化且 URL 不变；停止后端口释放且刷新返回错误。

---

### 4.5 G-05 Faker / 造数变量（✅ 已落地，2026-11-08）

**目标**：统一一套假数据变量，供请求、文档示例、Mock、测试数据集复用。

**语法**：

```
{{faker:name}}        {{faker:nameZh}}     {{faker:email}}
{{faker:phone}}       {{faker:uuid}}       {{faker:date}}
{{faker:datetime}}    {{faker:int(1,100)}} {{faker:bool}}
{{faker:word}}        {{faker:city}}       {{faker:company}}
```

**实现**：

- 新增 `services/faker.rs`，内置中文姓名库、常见姓氏、城市、公司后缀、邮箱域名、手机号段；不引入 `fake` 大依赖；
- `int(min,max)` 用种子化 LCG；`uuid` 用时间 + 种子拼装固定格式；
- 两种模式：
  - **runtime**：每次发送重新生成，保证多次压测/测试数据不同；
  - **deterministic**：同一 `request_id + 字段序号` 生成稳定值，文档导出与契约快照使用，避免 diff 抖动；
- 接入点：`util::render` 增加 faker 解析（保持现有 `{{var}}` / `{{secret:}}` 行为不变）；文档的示例生成直接复用 `services/faker.rs`，删掉文档方案里那份重复映射表；
- 前端：请求栏提供「faker 变量」速查弹层（点击复制表达式），并统计 URL / Header / Body 中 `{{faker:...}}` 的数量，发送前显示「将生成 N 个随机值」；按当前实现直接手写表达式即可，未单独做焦点插入。

**实现状态**：`services/faker.rs` 提供 `Faker::runtime` / `Faker::deterministic` 与 `runtime_value`，`util::render` 已接入；文档示例（`docs/examples.rs`）按 `request_id + 字段序号` 使用确定性种子；Mock 的骨架生成复用同一模块。

**测试**：同一模式同种子结果稳定；runtime 两次不同；`int` 边界；未知 faker 名称保留原样并 warning；文档与 Mock 复用同一实现。

---

### 4.6 G-06 加密异地备份（✅ 已落地，2026-11-08）

**目标**：在本机备份之外，支持把加密包上传到 WebDAV / NAS / 用户指定目录，解决「磁盘坏了全丢」。

**配置表**：

```sql
CREATE TABLE IF NOT EXISTS backup_targets (
  id            INTEGER PRIMARY KEY AUTOINCREMENT,
  name          TEXT    NOT NULL,
  kind          TEXT    NOT NULL,      -- local_dir | webdav
  url           TEXT    NOT NULL,
  username      TEXT    NOT NULL DEFAULT '',
  password_enc  TEXT    NOT NULL DEFAULT '', -- 复用 secrets 模块加密
  dir           TEXT    NOT NULL DEFAULT '',
  enabled       INTEGER NOT NULL DEFAULT 1,
  last_run_at   INTEGER,
  last_status   TEXT,
  created_at    INTEGER NOT NULL
);
```

**流程**：

1. 沿用 `backup::create` 生成 zip；
2. 新增 `backup_crypto.rs`：AES-256-GCM 流式加密 + Argon2id 派生密钥；输出 `.astroman.bak.enc`；
3. **备份口令独立于主密钥**：口令由用户单独设置，Argon2id 使用独立 salt 并写入包头，**不得复用环境变量主密钥**。否则换机器、重装或主密钥轮换后备份直接作废，异地备份就失去意义；主密钥本身也不进备份包；
4. 上传：`local_dir` 直接复制；`webdav` 用 `reqwest` PUT，失败重试 2 次并保留本地包；
5. 恢复：下载 → 解密（口令错误时明确报 `E_CRYPTO` 并保留原库）→ 走现有 `backup_restore` 流程（校验 + 下次启动替换）；
6. 保留策略：远端最多 7 份或 90 天，先删最旧，失败不删本地；
7. 口令丢失 = 备份不可恢复，必须在设置页与创建时双重提示；不允许「无口令上传」；设置页提供「导出口令提醒文件（不含口令）」帮助用户自查。

**命令**：`backup_target_list / save / delete / test / upload / restore_from_target`。

**实现状态（与方案的差异）**：

- 快照沿用 `backup::create`（SQLite `VACUUM INTO` 生成 `.db`），**不是 zip**；加密后统一命名为 `{目标}-{时间戳}.astroman.bak.enc`；
- `backup_crypto.rs` 采用 1 MiB 分块 AES-256-GCM，包头（含 Argon2id 参数、salt、原始文件名）作为 AAD；解密结果必须带 `SQLite format 3` 头才允许落盘；口令至少 8 位；
- 口令**不落库、不进日志**，每次上传 / 恢复时输入；CLI 可用 `ASTROMAN_BACKUP_PASSPHRASE`；
- WebDAV 密码复用 `secret::encrypt` 加密存储，列表只返回 `hasPassword`；WebDAV 走 `reqwest`（PROPFIND / PUT / GET / DELETE），PUT 失败重试 2 次；
- 保留策略为「最新 7 份且 90 天内」，本地与远端分别执行，远端删除失败只记日志；
- 恢复流程：下载 / 复制到本地 → 校验加密包头 → 解密 → `backup::validate` → 写 `restore.pending`，重启后替换；口令错误报 `E_CRYPTO` 且不留下半成品。

**不做**：S3 / OSS 签名（需要额外 SDK，后置）；自动定时上传（依赖路线图定时任务，先手动触发）；「导出口令提醒文件」未单独实现，改为设置页与上传弹窗中的双重提示。

**测试**：加解密往返；错误口令失败且不损坏本地；WebDAV mock 服务上传/下载；保留策略；恢复标记正确写入；换一台机器用同一口令可恢复（验证口令不依赖本机主密钥）。

---

### 4.7 G-07 导入导出补齐（✅ 已落地，2026-11-08）

| 方向 | 内容 | 复用 |
|---|---|---|
| 导出 Postman v2.1 | 项目 / 集合导出 collection JSON，变量转 `{{var}}`，auth 映射，示例保留 | 文档 IR + `transfer.rs` |
| 导入 Insomnia v4 | 解析 `_type: export` JSON，映射 workspace / collection / request / environment | `importer.rs` |
| 导入 Bruno | 解析 `.bru` 文本块（meta / get / headers / body），按目录建集合 | 新增 `importer/bruno.rs` |
| OpenAPI URL 拉取 | 输入 URL + 可选 Header，拉取 spec 后走现有导入；支持保存 URL 供后续刷新 | `reqwest` + 现有 OpenAPI 导入 |
| 实体与字段说明 | OpenAPI 的 `schema.description` ↔ 实体 `description` 双向映射；请求 / 响应 schema 由实体生成（见 4.2） | `entity` 模块 + `transfer.rs` |
| 项目包 | 导出 / 导入必须包含 `request_entity` / `response_entity` 两列，缺列时按空实体处理 | `transfer.rs` 项目包 |

**规则**：导入冲突（同名集合/请求）默认「新建」，可选「覆盖」；导入报告列出跳过项与不支持字段（如 Postman 的 pre-request script 只保留为脚本文本）。Postman / Insomnia / Bruno 没有实体结构，导入时只落示例并提供「按示例推断实体」入口（复用 `request_entity_generate`），不静默生成；OpenAPI 导入则把 schema 直接转成实体，说明一并带入。导出到 Postman 时实体说明写进 description 字段，导不出的部分在报告里列出。

**实现状态**：Postman v2.1 导出、Insomnia v4 / Bruno / OpenAPI 文件与 URL 导入均在 `commands/transfer.rs` 落地；`openapi_sources` 表保存 URL + 可选 Token + 最近拉取时间，支持刷新与删除；OpenAPI URL 只接受 JSON，YAML 返回明确错误；导入来源写入 `source=import` 便于版本历史区分；Postman 导出对响应体沿用脱敏，pre/post script 计入跳过报告；Insomnia / Bruno 不保留脚本。

**前端入口**：项目页导入菜单新增「Postman / Apifox / Insomnia / HAR」「OpenAPI / Swagger 文件」「OpenAPI / Swagger URL」「Bruno 目录」，导出菜单新增「Postman Collection v2.1」。

**测试**：每种格式 2 个 fixture（含中文、含认证、含嵌套集合）；往返导出→导入后请求数、方法、URL、Body、实体一致；OpenAPI 带 description 的 schema 导入后实体说明正确；项目包缺实体列时不报错。

---

### 4.8 G-08 CLI 能力对齐（✅ 2026-11-08：五个子命令全部可用）

**前置**：先完成 2.1 的 `services` 抽取，CLI 直接调用 service，不复制业务逻辑。

**新增子命令**：

```text
astroman-cli request send   --request <ID> --env <名称|ID> [--json]
astroman-cli docs export    --scope project|collection|request --id <ID>
                            --env <ID> --format markdown|html --out <路径>
                            （pdf 待文档导出方案 PDF 阶段落地后再开放）
astroman-cli contract check --project <ID> --baseline <ID> --fail-on breaking
astroman-cli mock start     --project <ID> --port 9000 [--host 0.0.0.0]
astroman-cli backup upload  --target <名称>
```

约定：默认人类可读输出，`--json` 输出机器可解析结构；失败退出码非 0；当前 `docs export` 只接受 `markdown` / `html`，其余格式返回 `E_FORMAT`。PDF 需等《接口文档导出技术方案》PDF 阶段完成，届时复用同一套浏览器探测与降级逻辑。

**实现状态（2026-11-08）**：

- ✅ `contract check` 已落地（`cli.rs` 的 `("contract", "check")`）：`--project` 支持 ID 或名称，省略 `--baseline` 时自动取最近快照，`--fail-on breaking|none` 决定退出码（`0` 通过 / 非 0 发现破坏性变更），并复用 `services::contract`；
- ✅ `mock start` 已落地：`--project <ID|名称>` 必填，`--port` 省略或 0 自动分配，`--host` 仅接受 `127.0.0.1` / `localhost` / `0.0.0.0`；启动后打印 URL、路由数与局域网令牌并阻塞，Ctrl+C 结束；
- ✅ `backup upload` 已落地：`--target <名称|ID>`，口令来自 `--passphrase` 或 `ASTROMAN_BACKUP_PASSPHRASE`（至少 8 位），复用 `services::backup_target::upload`，`E_CRYPTO` / `E_NO_SPACE` 归类为配置错误；
- ✅ `request send` 已落地：`--request <ID>` 读取接口定义（方法 / URL / 参数 / Header / Body / 认证），`--env` 支持名称或 ID 并渲染 `{{变量}}`，认证字段解密后写入；默认 30s 超时（`--timeout` 秒可调）；命中 `prod` / 生产 环境默认拒绝，需 `--allow-prod`；SSE / WebSocket 类型直接报错并提示改用桌面端实时面板；人类可读输出状态码、耗时、响应头与正文，`--json` 输出完整结构；请求不存在 / 网络失败等以非 0 退出；
- ✅ `docs export` 已落地：`--scope project|collection|request` + `--id` 先映射到项目，`--env` 解析环境后生效，复用 `services::docs::export`（与 GUI 同一条渲染链路）；`--format` 仅 markdown / html（其他值返回 `E_FORMAT`），`--out` 必填并自动建父目录；成功打印路径与导出条数，`--json` 输出机器结构，失败非 0 退出。

**与方案的差异**：`request send` 增加 `--timeout`（方案未列）；`docs export` 的 `--env` 用 `resolve_env` 支持名称或 ID；PDF 仍按 4.4 的结论留待文档导出方案 PDF 阶段。CLI 单测覆盖五个子命令的成功与失败路径（含退出码、生产环境守卫、格式校验）。

**测试**：每个子命令至少一条「成功 + 失败」用例；`contract check` 的退出码断言；`docs export` 产物存在性。

---

### 4.9 G-09 SSE / WebSocket 调试（✅ 2026-11-08）

**数据模型**：`requests` 增加 `protocol TEXT NOT NULL DEFAULT 'http'`（http | graphql | sse | websocket），随 **v12** 迁移（`COLUMN_MIGRATIONS`）。注意：该列**不加入现有 `map_detail` 的固定列序号**，单独查询，避免错列。

**SSE**：

- 复用 GET 请求定义，流式读取 `text/event-stream`，解析 `event / id / data / retry`；
- 时间线展示消息，支持自动重连与 `Last-Event-ID`；
- 上限：单连接 10 MB / 5000 条，超出停止并提示。

**WebSocket**：

- `tokio-tungstenite` 建立连接，握手 Header 复用请求头；
- 消息编辑器支持文本 / JSON，发送历史保留最近 100 条（内存）；
- 心跳 ping 可配；断线原因展示；
- 不做 WS 抓包（MITM 解密 WS 复杂度高，后置）。

**命令**：`realtime_connect / realtime_send / realtime_close / realtime_status`，事件通过 Tauri event 推送消息。

**实现状态（2026-11-08）**：

- ✅ 后端 `services/realtime.rs`：`RealtimeManager` 持有会话（最多 20 个，关闭后保留状态供查询），命令层只做薄适配；事件经 `realtime:event` / `realtime:status` 推送，测试可直接传闭包 sink，不依赖窗口；
- ✅ SSE：复用 GET 定义与请求头，按 `event / id / data / retry` 解析，时间线逐条推送；默认开启自动重连，重连携带 `Last-Event-ID`，最多 5 次；单连接 10MB / 5000 条上限，超限主动关闭并回传原因；
- ✅ WebSocket：`tokio-tungstenite` 建连，握手头复用请求定义；文本 / JSON / 二进制消息都能展示与发送；ping 间隔可配（默认 30s，0 关闭），断线原因写回状态；
- ✅ 命令：`realtime_connect` / `realtime_send` / `realtime_close` / `realtime_status`；前端请求编辑器新增协议选择（HTTP / GraphQL / SSE / WebSocket）与实时面板（连接、消息时间线、发送框、断开），协议字段随请求保存；
- ✅ 安全：URL 与 Header 经 `logging::redact` 脱敏后写入状态。

**与方案的差异**：CLI 不提供实时调试，`request send` 遇到 SSE / WebSocket 直接报错；不做 WS 抓包与消息录制；会话只存在于进程内存，应用退出即销毁；前端消息时间线保留最近 5000 条（与后端上限一致），不单独维护 100 条发送历史。

**测试**：本地 SSE / WS 测试服务覆盖消息顺序、Last-Event-ID 重连、超限停止、主动关闭；CLI 与协议字段迁移有单测。

---

### 4.10 G-10 GraphQL Schema 感知（✅ 2026-11-08）

**数据表**：

```sql
CREATE TABLE IF NOT EXISTS graphql_schemas (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  project_id  INTEGER NOT NULL,
  endpoint    TEXT    NOT NULL,
  schema_json TEXT    NOT NULL,      -- introspection 原始结果
  fetched_at  INTEGER NOT NULL,
  UNIQUE(project_id, endpoint)
);
```

**实现**：

- `graphql_introspect(projectId, endpoint, headers)`：发标准 introspection query，结果入库；
- 前端 GraphQL 页签增加 Schema 浏览器（类型、字段、参数、枚举），点击字段插入查询；
- 输入补全基于 introspection JSON 建内存索引，不做完整语言服务；
- introspection 被服务端关闭时，支持手动粘贴 SDL（阶段二）；
- 敏感 Header 不入 schema 表。

**实现状态（2026-11-08）**：

- ✅ `services/graphql.rs`：`introspect` 发标准 introspection 查询（只取 Schema 浏览器所需字段），按 `(project_id, endpoint)` 唯一保存；结果上限 8MB，避免异常服务端撑爆库；请求头只用于本次请求，**敏感值不落库**；
- ✅ 命令：`graphql_introspect` / `graphql_schema_get` / `graphql_schema_list` / `graphql_schema_delete`；
- ✅ 前端：请求编辑器 GraphQL 页签提供「获取 Schema / 浏览字段」，按类型列出字段、参数与枚举，支持过滤；点击字段插入到查询，打开 GraphQL 请求时自动加载缓存；
- ✅ 降级：服务端关闭 introspection 时返回明确错误（`E_INTROSPECTION`），前端提示并保留普通编辑器。

**与方案的差异**：SDL 手工粘贴留待后续（当前只做 introspection）；不做完整语言服务与自动补全，输入以字段点击插入为主。

**测试**：introspection fixture 入库与索引、无 schema 时降级、字段插入位置、超限拒绝、敏感 Header 不落库。

---

### 4.11 G-11 Git 友好工作区（✅ 2026-11-08）

**目标**：不做服务端同步，先把工作区变成可读、可 diff、可提交的目录，团队用 Git 协作。

**目录结构**：

```
workspace/
  manifest.json                     # 版本、项目列表、导出时间
  project-<id>-<slug>/
    project.json
    collections/
      <collection-slug>/collection.json
      <collection-slug>/<child>/collection.json
    requests/
      <request-slug>.json           # 含固定 id，保证 diff 稳定
    environments/
      <env-slug>.json               # 敏感值不写出，只保留 {{secret:NAME}}
```

**导出 / 导入**：

- `workspace_export_dir(path)`：按上述结构写出，JSON 固定键顺序；
- `workspace_import_dir(path)`：按 `id` 合并；文件存在则更新，不存在则创建；id 冲突但内容不同则记入冲突报告，不自动覆盖；
- 敏感值：导出时 `is_secret = 1` 的变量只写变量名与 `{{secret:NAME}}` 占位；导入时要求用户补值；
- 不做文件监听与自动同步，避免半保存文件损坏库。

**实现状态（2026-11-08）**：

- ✅ `services/workspace.rs`：`export` 写出 `manifest.json`（`version: 1` + 项目列表 / 导出时间）与 `project-<id>-<slug>/` 下的 `project.json`、`collections/**/collection.json`、`requests/<slug>.json`、`environments/<slug>.json`；文件名冲突时追加 id，JSON 使用 `serde_json` 的 `preserve_order` 固定键顺序并 pretty 输出；
- ✅ `import` 先校验 manifest 版本（非 1 直接拒绝），再按 `id` 创建 / 更新；同名但内容不同的项目、集合、请求、环境记入 `conflicts` 并**不覆盖**；`skipped` 统计无法处理项，`warnings` 收集补值等提示；重复导入同一目录幂等；
- ✅ 敏感变量：`is_secret = 1` 的环境变量只写 `{{secret:NAME}}`，导入时先占位空值并给出 warning，用户需在环境页补值；
- ✅ 命令：`workspace_export_dir` / `workspace_import_dir`；前端入口在「设置 → 数据与存储 → Git 友好工作区」，目录选择后直接导出 / 导入并展示计数与冲突报告；
- ✅ 不做文件监听与自动同步，避免半保存文件损坏库。

**与方案的差异**：`export` 支持可选 `project_ids` 只导出部分项目（方案未列，便于按项目拆仓）；目录树的集合子集按父子层级递归写出；冲突报告以 `kind / id / name / reason` 结构化返回，前端最多展示前 20 条。

**测试**：导出 → 删除库 → 导入后数量与内容一致；敏感值不出现；冲突报告正确且不覆盖；重复导入幂等；manifest 版本不匹配直接拒绝。

---

### 4.12 G-12 前端 E2E / 视觉回归（✅ 2026-11-08）

- 引入 Playwright，`npm run e2e` 本地跑，CI 用 headless。
- 冒烟 6 条：新建请求并发送、环境切换、抓包启停 + Map Remote、测试套件运行、文档预览/导出、Mock 启停。
- 视觉回归只覆盖 3 个稳定页面（项目页、请求编辑器、报告页），阈值 0.2%，其余只做截图留档。
- 不追求覆盖率，定位是「改一处不把核心流程打挂」。

**实现状态（2026-11-08）**：

- ✅ `playwright.config.ts`：Vite 开发服务器（127.0.0.1:5273）+ 两个 project（`smoke` / `visual`）；本地默认使用系统 Chrome（`channel: 'chrome'`），CI 使用随包 Chromium；失败保留 trace 与截图并上传；
- ✅ 冒烟 6 条全部落地（`e2e/smoke.spec.ts`）：打开请求并发送（断言状态码与响应体）、环境切换、抓包启停 + Map Remote 页签、测试套件运行、接口文档预览与导出、Mock 启停；
- ✅ 视觉回归（`e2e/visual.spec.ts`）：项目页 / 请求编辑器 / 报告页 × 1366×768、1920×1080、1366×768 @200% 共 9 张基线，阈值 `maxDiffPixelRatio: 0.002`，禁用动画并等待字体就绪；
- ✅ CI 新增 `e2e` job：安装 Chromium 后跑 smoke，失败上传 `test-results/` 与 `playwright-report/`；视觉基线仅在 Windows 生成与校验（非 Windows 自动 skip，避免跨平台字体漂移）；
- ✅ `npm run e2e` / `e2e:smoke` / `e2e:visual` / `e2e:update` 四个脚本。

**与方案的差异**：Tauri 桌面端无法直接由 Playwright 启动，E2E 采用「Vite SPA + `page.addInitScript` 注入假 Tauri bridge」（`e2e/fixtures/tauri.ts`）的方式在浏览器中跑完整前端流程，后端命令返回固定数据、事件经假 event 插件推送；因此 E2E 验证的是前端交互与渲染，不覆盖 Rust 后端（后端由 `cargo test` 覆盖）。Playwright 实际版本 1.65.0（方案原写 ^1.49.0）；视觉基线不进入 CI（仅本机校验），避免不同 runner 字体差异造成误报。

**验证**：本地 smoke 6/6、visual 9/9 通过；`cargo test --lib` 314、vitest 241、`vue-tsc`、`npm run build` 全绿。

---

### 4.13 路线图已规划项与本方案的衔接

| 路线图项 | 衔接点 |
|---|---|
| Flow 链路编排 | 直接复用 `request_revisions` 的稳定请求快照与 `services/realtime`，不另建执行引擎 |
| 定时巡检 / 告警 | 复用测试套件 + 报告 + `contract_check`，把契约校验作为巡检的一步 |
| 混合多接口压测 / 分布式 | 复用运行记录与报告，不在本方案展开 |
| 团队同步 | 先落 G-11 Git 工作区；服务端同步后置，避免过早背账号体系 |
| Linux 桌面 | 文档站为跨平台 `std::net::TcpListener` 线程、Mock 为 tokio 服务，天然可复用；抓包代理的系统配置仍需单独适配 |

---

## 5. 数据模型与迁移

### 5.1 新增表

| 表 | 用途 | 清理策略 | 状态 |
|---|---|---|---|
| `request_revisions` | 请求版本 | 每请求 50 条（硬上限） | ✅ v10 已落库 |
| `contract_snapshots` | 契约基线 | 每项目 20 条 | ✅ v10 已落库 |
| `mock_routes` | Mock 路由 | 随项目删除 | ✅ 已落库（schema.sql） |
| `backup_targets` | 备份目标 | 手动删除 | ✅ 已落库（schema.sql） |
| `openapi_sources` | OpenAPI URL 来源 | 随项目 / 手动删除 | ✅ 已落库（schema.sql） |
| `graphql_schemas` | GraphQL introspection | 每项目每 endpoint 1 条 | ✅ v12 已落库 |
| `audit_logs` | 关键操作审计 | 90 天 | ✅ v11 已落库 |

已落库的表：`request_revisions` / `contract_snapshots` 在 **v10** 建，`audit_logs` 在 **v11** 建；W2 的 `mock_routes` / `backup_targets` / `openapi_sources` 与 W3 的 `graphql_schemas` 追加到 `schema.sql`（`IF NOT EXISTS` 自动补齐）。`requests.protocol` 属于列变更，随 **v12** 的 `COLUMN_MIGRATIONS` 落库；W5 的 AI provider 治理列随 **v13**、写操作快照关联随 **v14** 落库。

### 5.2 变更列

| 列 | 说明 |
|---|---|
| `requests.protocol` | `TEXT NOT NULL DEFAULT 'http'`，✅ v12 已增加；使用独立查询，不进 `map_detail` 列序号 |
| `ai_providers.native_tools` / `input_price` / `output_price` / `currency` | ✅ v13 已增加，默认 `-1` / `0` / `0` / `USD`，表示自动、沿用内置价格表 |
| `request_revisions.trace_id` | ✅ v14 已增加，AI 写操作按同一次提问聚合回滚 |
| `environments.is_prod` | ✅ v11 已增加，默认 0；生产环境 AI 写操作强制确认 |

### 5.3 迁移纪律

1. `schema.sql` 与 `COLUMN_MIGRATIONS` 同步改（[db/mod.rs](file:///d:/Software/ProjectFiles/AstroMan/src-tauri/src/db/mod.rs#L196-L260)）；
2. **当前 `DB_VERSION = 14`**（v9 实体列、v10 版本历史与契约快照、v11 AI 治理、v12 协议与 GraphQL、v13 AI provider 治理列、v14 AI 写操作快照关联）。后续独立升版，避免一个版本混太多变更：
   - **v10（已落库）**：`request_revisions`、`contract_snapshots`；
   - **v11（已落库）**：`environments.is_prod`（`COLUMN_MIGRATIONS` 判存补列）+ `audit_logs`（`CREATE TABLE IF NOT EXISTS`）；
   - **v12（已落库）**：`requests.protocol`（`COLUMN_MIGRATIONS`）+ `graphql_schemas`（`CREATE TABLE IF NOT EXISTS`）；
   - **不强制升版**：`mock_routes`、`backup_targets`、`openapi_sources` 随各自批次追加 `schema.sql`，启动时自动建表；
   - **v13 / v14（已落库）**：`ai_providers` 原生工具与价格列、`request_revisions.trace_id`，均走判存补列。
3. 新表只 `CREATE TABLE IF NOT EXISTS`，不重建；新列必须走 `COLUMN_MIGRATIONS` 的判存补列，不依赖版本号判断；
4. 上线前跑 `cargo test --lib` 的迁移幂等用例（连续 init 两次），并用旧版本数据库样本各跑一次升级演练。

---

## 6. 命令契约汇总

| 命令 | 批次 | 说明 |
|---|---|---|
| `request_revision_list / get / diff / restore` ✅ | 一（已完成） | 版本历史 |
| `contract_snapshot_create / list`、`contract_check`、`contract_report` ✅ | 一（已完成） | 契约基线、diff、报告 |
| `docs_site_start / stop / status / refresh` ✅ | 一（已完成） | 只读文档站 |
| `mock_routes_list / save / delete`、`mock_start / stop / status`、`mock_logs` | 二 | Mock |
| `backup_target_list / save / delete / test / upload`、`backup_restore_from_target` | 二 | 异地备份 |
| `import_postman / import_insomnia / import_bruno`、`export_postman`、`import_openapi_url` | 二 | 导入导出 |
| `realtime_connect / send / close / status` ✅ | 三（已完成） | SSE / WebSocket |
| `graphql_introspect / schema_get / schema_list / schema_delete` ✅ | 三（已完成） | GraphQL |
| `workspace_export_dir / import_dir` ✅ | 三（已完成） | Git 工作区 |
| `audit_log_list` + `audit_log_cleanup`、`diagnostics_export` ✅ | 零（已完成） | 审计查询 / 清理、诊断包导出 |
| `tool_cancel` ✅ | 零（已完成） | 按 `trace_id` 取消在途工具请求 |
| `ai_confirm_audit` ✅ | 零（已完成） | 前端写工具确认结果落审计 |
| `ai_policy_get / set` | 五 | AI 权限模式与预算读写（当前权限模式复用 `settings_get / set` 的 `network.aiPermissionMode`，不单开命令） |

统一约定：

- 全部先落 `services/*`，Tauri 命令只做参数校验与 State 注入；
- 错误统一中文 + 可判定前缀：`E_SCOPE`、`E_PORT`、`E_TOKEN`、`E_BASELINE`、`E_CRYPTO`、`E_PARSE`、`E_FORMAT`、`E_BUSY`（CLI/GUI 写库冲突）；
- 长任务（Mock 启停、备份上传；PDF 待其阶段落地后再纳入）不改同步阻塞模型，返回句柄或状态对象，进度走 Tauri event。

---

## 7. 前端改动

| 位置 | 改动 |
|---|---|
| 项目页 | 契约与文档站先用顶部按钮 + 抽屉 / 弹窗（已实现）；Mock 用 `MockPanel.vue` 抽屉（已实现）；导出下拉保留 |
| 请求编辑页 | 新增「历史」抽屉（已实现）；协议下拉增加 SSE / WebSocket / GraphQL（已实现）；GraphQL 增加 Schema 浏览器、SSE / WS 增加实时面板（已实现） |
| 新增页面 | `MockPanel.vue`、`RemoteBackupPanel.vue` 已落地；`Contract.vue` 按需（当前为抽屉）；`Revisions.vue` 已以抽屉形态内嵌 |
| 设置页 | 备份目标管理、性能模式、诊断包、审计日志；「数据与存储」新增 Git 友好工作区导出 / 导入（已实现） |
| 命令面板 | 增加「对比契约」「起 Mock」「发布文档」命令 |
| 权限 | 起服务、上传备份、写工作区目录均走二次确认，默认端口与地址只读展示 |
| 全局 | 三档断点布局；列表中大数组用 `shallowRef`；页面卸载释放事件与定时器 |

---

## 8. 测试方案

### 8.1 Rust 单元 / 集成

- 版本：保存→diff→回滚→再回滚；清理阈值；截断快照；实体回滚按合并语义（保留说明与手工字段）。
- 契约：实体树 → schema 的 6 类节点转换；归一化后连续两次快照 diff 为空；10 类变更 fixture；无实体接口标 `schemaMissing`；CLI 退出码。
- Mock：模板匹配、方法 405、延迟、CORS、令牌与 Cookie、端口冲突、停止释放；回放记录脱敏、非 2xx 状态码保留。
- 文档站：200/404/405、路径穿越、刷新、停止。
- Faker：确定性与 runtime 差异、边界、未知变量。
- 备份：加解密往返、错误口令、WebDAV mock、保留策略、跨机口令恢复。
- 导入导出：各格式 fixture、往返一致、实体与说明映射。
- SSE/WS：本地测试服务、重连、超限（已覆盖）。
- GraphQL：introspection fixture、索引、插入（已覆盖）。
- 工作区：往返、幂等、敏感值缺失（已覆盖）。
- 性能：HAR 分批流式导出、抓包预算淘汰、分档预算选择、DB pragma 生效。
- 审计/诊断：审计写入与清理、诊断包脱敏。

### 8.2 前端

- vitest：diff 展示、Mock 表单校验、备份目标表单、工作区冲突提示、虚拟列表窗口计算。
- Playwright：6 条冒烟 + 3 页视觉回归 + 多分辨率（1366×768 / 1920×1080）+ 200% 缩放（已实现，见 4.12）。

### 8.3 手工验收

- 局域网 Mock：另一台机器可访问，无令牌 401；
- 契约 diff 在真实项目上无大量误报；
- 文档站链接在浏览器与手机（同局域网）可打开；
- 加密备份断网重试、口令错误提示清晰；
- 20+ 页面回归时 E2E 能在 10 分钟内给出结果。

### 8.4 性能与兼容回归

- 4GB 虚拟机跑 10.6 清单，记录 RSS 与帧率；
- 5,000 条抓包 + 500 接口项目 + 5MB 响应三组压力数据；
- Win10 1809 / Win11 / x86 / 无 Edge 四个环境安装与启动；
- 断网 offline installer 安装；
- 125% / 150% / 200% 缩放走查核心五步：建项目 → 建请求 → 发送 → 抓包 → 导出文档。

---

## 9. 性能与内存专项

> 本章数据来自当前仓库实测与代码常量，不是估算：前端产物、主程序体积由构建产物统计；内存上限来自抓包与 HTTP 模块的常量；渲染风险来自页面实现。

### 9.1 现状实测

| 项 | 现状 | 依据 | 判断 |
|---|---|---|---|
| 前端产物 | 4,091,681 B（约 3.9 MB，114 个文件） | `dist/` 实测 | 健康 |
| 主程序 | 12,383,232 B（约 11.8 MB，release） | `target/release/AstroMan.exe` 实测 | 健康 |
| 抓包条数上限 | 按档位 2,000 / 5,000 条（P-05） | [profile.rs](file:///d:/Software/ProjectFiles/AstroMan/src-tauri/src/services/profile.rs) | 已分档 |
| 抓包 body 总预算 | 按档位 32 / 96 / 128 MB（P-05） | [profile.rs](file:///d:/Software/ProjectFiles/AstroMan/src-tauri/src/services/profile.rs) | 4GB 机器不再按 128MB 硬扛 |
| 单条 body 截断 | 2 MB | [handler.rs](file:///d:/Software/ProjectFiles/AstroMan/src-tauri/src/capture/handler.rs#L41-L41) | 合理 |
| 单条采集上限 | 8 MB | [handler.rs](file:///d:/Software/ProjectFiles/AstroMan/src-tauri/src/capture/handler.rs#L43-L43) | 合理 |
| 图片预览上限 | 1 MB | [handler.rs](file:///d:/Software/ProjectFiles/AstroMan/src-tauri/src/capture/handler.rs#L45-L45) | 合理 |
| 单响应内存 | 5 MB 截断，2 MB 起落盘 | [http.rs](file:///d:/Software/ProjectFiles/AstroMan/src-tauri/src/commands/http.rs#L32-L40) | 合理 |
| 请求历史上限 | 100,000 条（落库） | [http.rs](file:///d:/Software/ProjectFiles/AstroMan/src-tauri/src/commands/http.rs#L40-L40) | 偏高但可接受 |
| SQLite 连接参数 | WAL / foreign_keys / busy_timeout 5000 / cache_size 16MB / synchronous NORMAL / 64 位 mmap 128MB | [db/mod.rs](file:///d:/Software/ProjectFiles/AstroMan/src-tauri/src/db/mod.rs) | 已调优（P-06） |
| 抓包列表渲染 | ~~全量 `v-for` + 分组树，无虚拟滚动~~ 已改为固定行高虚拟滚动（W1） | [Capture.vue](file:///d:/Software/ProjectFiles/AstroMan/src/views/Capture.vue#L351-L373) | 已整改（P-02） |
| 项目接口表 | >300 条自动分页 50（P-03） | [Project.vue](file:///d:/Software/ProjectFiles/AstroMan/src/views/Project.vue) | 已整改 |
| 历史表 | 每页 50 条 | `History.vue` | 低风险 |
| 抓包拉取 | 默认 500 条 | `commands/capture.rs` | 低风险 |

**结论：静态体积不是问题，峰值内存和 DOM 规模才是问题。** 当前有三处会把成本放大到低端机吃不消：

1. ~~HAR 导出对全部流量做深拷贝~~（W1 已改流式导出，P-01）；
2. ~~抓包树全量渲染~~（W1 已改固定行高虚拟滚动，P-02）；
3. ~~项目页接口表格不分页~~（W0 已改阈值分页，P-03）。

### 9.2 内存构成与放大系数

| 层 | 构成 | 量级 | 说明 |
|---|---|---|---|
| Rust | 抓包仓库 | body ≤ 128MB + 元数据约 2-4MB | 满载后才接近上限 |
| Rust | 单响应缓冲 | ≤ 5MB / 请求 | 并发越多线性增长 |
| Rust | SQLite | 默认 page cache 约 2MB | 可调到 16MB 换命中率 |
| Rust | 导出/加密中间量 | 与产物同量级 | 必须流式化 |
| WebView2 | 渲染进程基础 | 约 80-150MB | 随 WebView2 版本波动 |
| WebView2 | DOM | 每行约 1-3KB | 5,000 行 → 5-15MB DOM，加上布局与样式计算后 CPU 成本更高 |
| WebView2 | JS 对象 | `JSON.parse` 后为原文 3-6 倍 | 5MB 响应可膨胀到 15-30MB |
| IPC | JSON 序列化 | 文本 1.1-1.3 倍；二进制 base64 1.37 倍 | 一次拉 500 条带 body 的流量可达数十 MB |

据此得到两条设计规则：

- **不带 body 的列表走 IPC**：列表只传 `FlowBrief`，详情按需拉；`capture_flows` 已是这个模式，不要退化成全量详情。
- **大对象不进响应式系统**：用 `shallowRef` / `markRaw` 持有，避免 Vue 对每个节点做 Proxy 代理。

### 9.3 高风险点与整改清单

| 编号 | 位置 | 风险 | 整改 | 批次 |
|---|---|---|---|---|
| P-01 ✅ | `CaptureStore::details()` | 对每条 `FlowRecord` 做深拷贝（`BodyPayload` 里的 `String` 会整段复制），满载时峰值至少再复制 128MB；序列化成 HAR 字符串还会再翻倍 | **已实现**：`for_each_detail(batch = 200, f)` 分批回调 + `har::export_stream` 用 `serde_json::to_writer` 逐条写文件；导出前统计 body 总量，超过 64MB 返回 `E_CAPTURE_HAR_LARGE`，前端二次确认后改「仅元数据」重导（保留 URL / 状态 / 头部 / 大小，不写正文）；中途失败或一条都没写出时删除半成品文件 | 一（先在第零批实测峰值） |
| P-02 ✅ | [Capture.vue](file:///d:/Software/ProjectFiles/AstroMan/src/views/Capture.vue#L1445-L1450) | `treeRows` 全量 `v-for`，5,000 条 + 域名分组会生成上万节点 | **已实现**：固定行高虚拟滚动（域名 29 / 目录 25 / 请求 27px），`captureTreeWindow` 二分定位窗口，上下各 overscan 8 行；窗口用 `translateY` 平铺，DOM 只保留视口附近几十行。域名分组默认折叠与过滤防抖未改，保持现有交互 | 零 |
| P-03 ✅ | [Project.vue](file:///d:/Software/ProjectFiles/AstroMan/src/views/Project.vue) | 接口表格 `pagination=false`，500 接口项目一次渲染 500 行 | **已实现**：≤300 条维持「一屏看全」；>300 条自动分页 50，并提供「全部展开」二次确认；分页阈值与页码收敛在 `utils/tablePaging.ts` | 零 |
| P-04 | 响应查看器 | 5MB JSON 自动格式化 + 树渲染，节点数无上限 | >512KB 默认文本视图；树只展开到深度 6 / 2 万节点；超出显示「已折叠 N 个节点」；提供另存 | 四 |
| P-05 ✅ | 抓包预算固定 | 4GB 机器也按 128MB 预算，容易触发系统换页 | **已实现**：`services/profile.rs` 用已依赖的 `sysinfo` 读取总内存分档（≤6GB 低 / 6~16GB 标准 / ≥16GB 高）；设置页可显式选自动 / 低内存 / 标准，保存后立即应用新预算 | 零 |
| P-06 ✅ | SQLite 连接参数 | 无 `cache_size` / `synchronous` / `mmap_size`，大库查询抖动 | **已实现**：每条连接执行 `connection_pragmas()`，`cache_size=-16000`（16MB）、`synchronous=NORMAL`；`mmap_size=128MB` 仅 64 位启用 | 零 |
| P-07 | 文档渲染 | 200+ 接口单页会生成数十 MB HTML 字符串（PDF 子进程内存监控待 PDF 阶段补） | 本批做：>200 接口强制按集合分页；PDF 逐页渲染 + 30s 超时 + 失败降级 HTML（详见文档导出方案 5.7，PDF 阶段落地） | 一 |
| P-08 | Mock / 文档站日志 | 环形缓冲若存完整响应体会随并发膨胀 | 只存元数据 + 前 4KB body；环形 500 条 | 二 |
| P-09 | 前端全局状态 | 页面 keep-alive 时长期持有全量流量/响应 | 大数组用 `shallowRef`；页面卸载释放；不缓存所有页面 | 四 |
| P-10 | 备份 / 工作区 / 导入 | 一次性读全量再 zip/加密 | 流式 zip + 逐文件写盘 + 流式加密；导入先解析元数据再按需读 body | 二 |

### 9.4 分档预算与自适应

| 设备内存 | 抓包条数 | body 预算 | 单响应 | 列表 | 默认行为 |
|---|---|---|---|---|---|
| ≤ 6GB | 2,000 | 32MB | 2MB | 强制虚拟滚动 | 低内存模式：关图片预览、>256KB 不自动格式化、默认关闭 Mock 日志 body |
| 6 ~ 16GB | 5,000 | 96MB | 5MB | 虚拟滚动 | 标准 |
| ≥ 16GB | 5,000 | 128MB | 5MB | 虚拟滚动 | 标准（高档） |

设置页提供「性能模式：自动 / 低内存 / 标准」。**自动档在应用启动时读取一次总内存并固定**，运行中不随内存变化切换；手动切换保存后**立即应用新预算**并裁剪已有流量（不重启、不丢新流量），设置页会提示当前生效的条数与 body 预算。

**预算与目标的张力**：128MB body 预算 + 5,000 条元数据 + WebView2 进程组（80–150MB）+ SQLite cache（16MB）已接近 500MB 目标线，且系统换页会先于 RSS 触发卡顿。第零批实测后按以下顺序校准：先降 body 预算（128 → 96MB），再降条数（5,000 → 3,000），最后才放宽 500MB 目标；不通过加内存目标来掩盖。所有 RSS 指标一律按**进程组总和**测量，单看主进程没有意义。

### 9.5 基准与压测方法

| 层 | 方法 | 指标 | 通过线 |
|---|---|---|---|
| Rust | `examples/baseline.rs`（feature `bench`；**未引入 criterion**，零新增依赖）：`openapi_build`（500 接口）、`docs_render`（200 接口）、`revision_diff`、`render_tracked`（1MB body） | 单次耗时（多轮取最小值） | 分别 < 500ms / 800ms / 50ms / 20ms |
| 前端 | vitest + `performance.mark`；Playwright 通过 CDP `Performance.getMetrics` 取 `JSHeapUsedSize` | 首屏、长任务 | 主线程无 >200ms 长任务 |
| 端到端 | 4GB 虚拟机 + 1366×768@125%，造 500 接口项目 + 5,000 条抓包 + 5MB 响应 | RSS、帧率、耗时 | 见 9.6 |
| 体积 | CI 统计 `dist/` 与安装包 | 增量 | 单次 PR 增量 >3% 需说明 |

**W0 实测（2026-11-08，release，多轮取最小值）**：openapi_build 500 接口 **24.21ms**、docs_render 200 接口 **23.07ms**、revision_diff **0.03ms**、render_tracked 1MB **11.11ms**，四项均远低于目标线。基准默认只记录，`cargo run --release --features bench --example baseline -- --check` 才按目标线退出非零，避免共享 CI 机器的抖动造成误报。

端到端 RSS 用 `scripts/measure-rss.ps1` 采样**进程组**（主进程 + WebView2 子树）Working Set，输出首 / 末 / 峰值 / 增长并可导出 CSV；单看主进程没有意义。4GB 虚拟机清单仍按 §10.6 手工执行。

本地埋点只记录**启动耗时、命令耗时 P50/P95、IPC 字节数**，存本地、不上传，设置页可查看与清空（第零批未做，仍属第四批）。

### 9.6 通过线与长稳

> 下表 RSS 一律指**进程组总和**（主进程 + WebView2 渲染 / GPU / 网络子进程）；冷启动以指定机型为准，HDD 数字只作参考，不作为发版阻断条件。

| 场景 | 指标 | 通过线 |
|---|---|---|
| 冷启动（8GB/SSD） | 到可交互 | ≤ 3s |
| 冷启动（4GB/HDD） | 到可交互 | ≤ 6s |
| 空闲 30 分钟 | 进程组 RSS 增长 | ≤ 20MB |
| 抓包满载（5,000 条） | 进程组 RSS | ≤ 500MB，停止后 60s 内回落 ≥ 60% |
| HAR 导出（5,000 条） | 峰值增量 / 耗时 | ≤ 200MB / ≤ 10s |
| 项目页（500 接口） | 首屏 / 滚动 | ≤ 1.5s / 低端 ≥30fps |
| 5MB JSON 响应 | 打开耗时 | ≤ 2s（低端 ≤3s） |
| 抓包 + Mock + 文档站并行 | 进程组 RSS | ≤ 700MB |

**W0 状态**：进程组 RSS 采样脚本已就位（`scripts/measure-rss.ps1`），4GB 虚拟机的冷启动 / 满载 / 长稳清单尚未实机执行，留待第四批随低端适配一起跑。

长稳脚本（手工或 CI 定时跑）：30 分钟空闲 + 10 轮抓包启停 + 10 次导出 + 5 次文档预览，观察 RSS 是否单调上升。

泄漏高发点统一登记：

- Tauri `listen` 必须在 `onUnmounted` 调 `unlisten`；
- `setInterval`、`ResizeObserver`、WebSocket 在卸载时释放；
- Rust 后台任务用 `JoinHandle` + `CancellationToken`，退出时 `join`；
- Mock / 文档站在 `Drop` 或退出钩子里 `stop`；
- 抓包停止后主动裁剪 body 预算，而不是等下一次录制触发淘汰。

---

## 10. 低端设备与弱网体验

### 10.1 目标设备档

| 档位 | 配置 | 要求 |
|---|---|---|
| 最低 | 4GB / 双核 / 1366×768 / Win10 1809 / HDD | 核心流程可用，不崩、不长时间无响应 |
| 推荐 | 8GB / 四核 / 1920×1080 | 流畅 |
| 高 | 16GB+ | 全功能，可开高预算抓包 |

### 10.2 缩放与布局

现状问题：窗口 `minWidth 1024 / minHeight 680`（[tauri.conf.json](file:///d:/Software/ProjectFiles/AstroMan/src-tauri/tauri.conf.json#L12-L23)）。在 1366×768 @125% 缩放下可用 CSS 高度约 614px，小于 680，窗口底部（状态栏、发送按钮）会被屏幕裁掉；@150% 时可用宽度约 910px，小于 1024，横向也放不下。

整改：

- `minWidth 900`、`minHeight 560`；
- 断点：≥1440 三栏；1024-1439 两栏；<1024 单栏 + 抽屉（侧栏、详情用 Drawer）；
- 请求编辑器上下分区可拖动、可折叠为 Tab，避免「请求 + 响应」固定高度挤爆；
- 工具栏溢出收纳到「更多」，不换行撑高；
- 统一 rem/百分比布局，禁止再新增固定 px 宽度；
- 125% / 150% / 200% 三档都必须无横向滚动条；表格允许内部横向滚动，但关键列固定。

**落地状态（W4，2026-11-08）**：`minWidth 900 / minHeight 560` 已改；`App.vue` 以 `DRAWER_BREAKPOINT = 1024` 切换侧栏为抽屉，主内容独占整屏；`RequestEditor.vue` 在 `max-width: 1023px` 下把请求 / 响应各压到约一半视口并收紧内边距；启动骨架写在 `index.html`，Vue 挂载后替换；响应正文超过 1MB 关闭着色、超过 512KB 不自动 Pretty，提供手动格式化与原样查看；动效开关写入 `localStorage fm-motion` 并同步 `html[data-motion]`，同时尊重 `prefers-reduced-motion`。工具栏暂未做「更多」收纳，窄宽度下通过断点压缩 + 抽屉避免换行撑高；`e2e/enterprise.spec.ts` 断言 900×560 无整页横向滚动，视觉回归覆盖 1366×768 / 1920×1080 / 200% 缩放三档。

### 10.3 启动与首屏

- 现状 `db::init` 在窗口创建前同步执行（schema + migrate + `verify_core_tables` + `recover_interrupted_runs` + `seed`），大库会拖慢首屏；建议窗口先出骨架，DB 初始化放后台任务，完成后事件通知。
- 路由懒加载；首屏只查项目/集合计数，请求行按需加载。
- 抓包代理、AI、Mock、文档站、CLI 全部惰性初始化，不进启动路径。
- 首次启动引导可跳过，不阻塞主流程。

**落地状态（W4）**：路由懒加载与启动骨架已就位；`db::init` 仍在窗口创建前同步执行（schema / migrate / 自检 / 中断恢复），本次未改为后台任务，避免把迁移失败处理分散到事件链路。启动骨架改善的是「白屏等待」的感知，4GB 大库冷启动数据仍需按 A-13 / A-17 实机采集；如实测超过 6s，再单独做后台初始化 + 事件通知。

### 10.4 交互反馈

| 场景 | 要求 |
|---|---|
| 任意操作 >300ms | 显示 loading，禁用重复点击 |
| >2s 操作 | 显示进度或阶段文案，提供取消 |
| 批量导入/导出 | 分批 50 + 进度 + 失败项汇总 |
| 大列表筛选 | 防抖 200ms，输入过程不阻塞 |
| 关闭窗口时抓包运行 | 二次确认 + 优雅停止 + 卸载证书 + 提示已停止 |
| 错误提示 | 错误码 + 中文 + 下一步（如端口占用给出「换端口 / 查看占用进程」） |

### 10.5 降级策略

| 场景 | 触发 | 降级行为 |
|---|---|---|
| 低内存 | 总内存 ≤ 4GB | 按 9.4 低内存档；关闭图片预览与自动格式化 |
| 大响应 | >1MB | 文本视图 + 手动格式化 + 另存，不自动建树 |
| 大项目 | >300 接口 | 分页 50；筛选在命令侧完成 |
| PDF 引擎缺失 | 探测不到 Edge/Chrome | 返回 HTML 并提示安装，不报「导出失败」（**PDF 阶段落地后生效**；当前无 PDF 导出） |
| WebView2 缺失 | 安装时 | 使用离线安装包；缺失时安装器给出明确提示 |
| 动画/GPU 异常 | `prefers-reduced-motion` 或渲染失败 | 关闭动画、模糊、阴影 |
| 系统代理不可用 | 代理探测失败 | 直连并明确提示「已绕过系统代理」 |
| Mock 端口占用 | 绑定失败 | 自动 +1 探测最多 3 次，仍失败报 `E_PORT` |
| 断网 | 无外网 | 本地调试、抓包、Mock、文档站、历史全部可用 |

### 10.6 低端机验收清单

- 4GB 虚拟机冷启动 ≤ 6s，首屏可操作；
- 500 接口项目页滚动 ≥30fps，无白屏；
- 5,000 条抓包列表滚动 ≥30fps；
- 125% / 150% 缩放无裁切、无横向滚动；
- 5MB 响应打开 ≤ 3s，可另存，不崩；
- 抓包 + Mock + 文档站并行 RSS ≤ 700MB；
- 断网下除真实请求外功能可用。

---

## 11. 兼容性矩阵

### 11.1 操作系统与架构

| 平台 | 版本 | 架构 | 构建方式 | 现状 | 结论 |
|---|---|---|---|---|---|
| Windows | 10 1809+ / 11 | x64 | `npm run build:win` | 支持 | 基线平台 |
| Windows | 10 / 11 | x86 (i686) | `npm run build:win32` | 有构建脚本 | 需实测地址空间与 mmap 禁用 |
| Windows | 11 | ARM64 | 无 | 未支持 | 后置；需 ARM64 WebView2 验证 |
| macOS | 10.15+ | Intel / Apple Silicon / universal | `npm run build:mac` | `signingIdentity: "-"` | 仅本机可用，外部分发会被 Gatekeeper 拦截 |
| Linux | Ubuntu 22.04+ / 主流发行版 | x64 | 无 | 路线图 | WebKitGTK 依赖 + 代理系统配置需单独适配 |

### 11.2 运行时依赖

| 依赖 | 现状 | 风险 | 对策 |
|---|---|---|---|
| WebView2 | `embedBootstrapper` 需联网下载 | 内网/离线机器装不上 | 已有 [`tauri.offline.conf.json`](file:///d:/Software/ProjectFiles/AstroMan/src-tauri/tauri.offline.conf.json#L1-L9) 与 `build:win:offline`；企业交付统一走离线包 |
| WebView2 版本 | 未检测 | 过旧导致白屏 | 安装器与启动时检测最低版本，给出升级指引 |
| Edge / Chrome | PDF 依赖系统浏览器（**PDF 阶段落地后生效**） | 精简系统/企业镜像可能没有 | 探测 `Program Files`、`Program Files (x86)`、`LocalAppData` 与注册表 App Paths；缺失降级 HTML |
| Python 运行时 | `resources/python/astroman_runtime.py` | 若依赖系统 Python，企业机器可能没有 | 确认后改为内置运行时或纯 Rust 实现 |
| 系统代理 / 自签 CA | `reqwest` 走 native-tls | 企业自签 CA 下请求失败 | 支持导入自定义 CA；「跳过证书校验」仅显式开启且默认关闭 |
| VC++ 运行库 | NSIS 安装 | 个别精简系统缺库 | 安装器检测并提示 |
| 端口/防火墙 | 局域网 Mock 绑 `0.0.0.0` | 触发防火墙弹窗，用户以为失败 | UI 明示 + 失败时给出放行指引 |

### 11.3 数据与路径兼容

| 项 | 风险 | 对策 |
|---|---|---|
| DB 迁移 | v9 → v10/v11/v12 跨版本升级 | 迁移幂等测试：连续启动两次；升级前自动备份；用 v9 样本演练 |
| 应用降级 | 新库被旧程序打开 | 已有 `E_SCHEMA_FUTURE` 拦截，文案给出恢复路径 |
| 中文/空格路径 | DB、导出、备份、工作区路径 | 全链路用例覆盖；导出默认目录用用户文档目录 |
| Windows 长路径 | >260 字符写失败 | 检测并提示缩短，或使用 `\\?\` 前缀（需验证各 API） |
| OneDrive / 网络盘 | SQLite WAL 放同步目录会锁冲突 | 启动检测路径，命中则警告并建议迁移 |
| FAT32 | 单文件 >4GB 失败 | 备份前检查文件系统并提示 |
| 多显示器 / 混合缩放 | 跨屏拖动后布局破版 | 监听 `scaleFactor` 变化并重算断点 |
| 输入法遮挡 | 中文候选框盖住输入 | 关键输入框预留下方空间 |
| 时区 | 跨时区团队时间显示 | 统一存 UTC，展示按本地时区 |
| 导入格式版本 | Postman v2.1 / Insomnia v4 / Bruno | 版本识别失败进导入报告，不静默丢弃 |

### 11.4 兼容性验证方法

1. 虚拟机矩阵：Win10 1809、Win11、Win10 LTSC（无 Edge）、x86 环境；
2. 断网安装：offline installer 在无外网机器完成安装并启动；
3. 安装 → 升级 → 降级 → 卸载（保留数据 / 清除数据）四步走；
4. Playwright 多分辨率：1366×768、1920×1080、2560×1440，缩放 100% / 125% / 150% / 200%；
5. 旧库样本：用 v9 数据库（含实体列）启动新版本，验证迁移与数据完整；再补一份 v8 样本覆盖跨两个版本的升级路径。

**落地状态（W4）**：第 1 / 2 / 3 / 5 项依赖虚拟机与真实安装包，已整理成 [docs/compatibility.md](../docs/compatibility.md#兼容性回归矩阵w4) 的逐项清单，尚未实机执行；第 4 项已自动化——Playwright 视觉回归覆盖 1366×768、1920×1080、200% 缩放，`e2e/enterprise.spec.ts` 额外验证 900×560 无整页横向滚动与纯键盘发送流程。

### 11.5 兼容性风险

| 风险 | 影响 | 对策 |
|---|---|---|
| 未签名 exe 被杀软/SmartScreen 拦截 | 高 | Authenticode 签名 + 时间戳；企业白名单说明 |
| macOS 未公证 | 高 | Developer ID 签名 + notarize；否则只承诺本机/自编译 |
| WebView2 缺失或过旧 | 高 | 离线包 + 版本检测 + 明确提示 |
| 32 位地址空间不足 | 中 | 禁用 mmap；降低抓包预算；提示使用 64 位 |
| Linux WebKitGTK 差异 | 中 | 后置；先做冒烟矩阵，不承诺全功能 |
| 企业代理/自签 CA | 中 | 自定义 CA + 显式跳过校验开关 |

---

## 12. 企业级部署、更新与治理

### 12.1 交付形态

| 形态 | 用途 | 说明 |
|---|---|---|
| NSIS（currentUser） | 现状，个人安装 | 无需管理员，升级简单 |
| NSIS（perMachine）/ MSI | 企业批量部署 | 供 SCCM / Intune / 组策略分发；需新增打包目标 |
| 离线安装包 | 内网/隔离网 | 已有 `build:win:offline` |
| 便携版（可选） | 无安装权限场景 | 数据目录跟随程序，需明确提示 |
| 内网更新源 | 受控升级 | 签名 + SHA256 清单 + SBOM |

安装/卸载规则：卸载默认**保留**用户数据（项目、历史、抓包、报告），清除数据必须显式勾选；静默安装参数写入交付文档。

**落地状态（W4）**：NSIS `installMode` 已从 `currentUser` 改为 `both`，安装时可选择当前用户（免管理员）或所有用户（Program Files），两种模式共用同一数据目录；卸载不删除 `%APPDATA%\AstroMan`，便携版不写注册表；离线安装包与便携版 zip 已在 release 流程产出；内网更新源通过 `ASTROMAN_UPDATE_MANIFEST` + SHA-256 清单支持。MSI / SCCM 专用包未新增，企业批量分发沿用 NSIS 静默参数；完整说明见 [docs/enterprise.md](../docs/enterprise.md)。

### 12.2 签名与更新

- Windows：Authenticode（OV/EV）+ 时间戳，解决 SmartScreen 与杀软误报；
- macOS：Developer ID + notarization，当前 `signingIdentity: "-"` 只能本机运行，不能对外分发；
- 自动更新（Tauri updater）：HTTPS + 更新包签名公钥校验；通道 `stable` / `beta`；灰度 5% → 25% → 100%；失败自动回滚并保留上一版安装包；
- 防降级：更新包版本必须单调递增，签名校验失败直接拒绝；
- 版本一致性：`package.json`、`tauri.conf.json`、`Cargo.toml` 已有 `sync-version` 脚本，CI 增加一致性断言。

**落地状态（W4）**：当前更新走自建 GitHub Release 检查 + 安装包下载（未启用 Tauri updater 的公钥验签链路，改用发布资产 `digest.sha256` 与下载后 SHA-256 复校，效果等价且不引入签名私钥管理）；`ASTROMAN_UPDATE_CHANNEL=beta` 走 releases 列表并优先预发布版本；`ASTROMAN_UPDATE_MANIFEST` 支持企业 HTTPS 清单，清单内所有资产必须带 `sha256`；`rolloutPercent` 按数据目录 `install-id` 前 8 位十六进制稳定分桶；只允许升级，降级包直接拒绝；下载失败打开发布页兜底。Windows 签名脚本 `scripts/sign-windows.ps1`（PFX / 证书指纹 + RFC3161）与 macOS `scripts/notarize-macos.sh`（Developer ID + notarytool + stapler）已交付，release workflow 在配置 secrets 后自动签名并替换资产，未配置时跳过。失败回滚依赖“保留上一版安装包 + 不自动删除旧版本”，未做自动回滚下载。

### 12.3 安全基线

| 项 | 现状 | 要求 |
|---|---|---|
| CSP | 含 `script-src 'self' 'unsafe-eval'`（[tauri.conf.json](file:///d:/Software/ProjectFiles/AstroMan/src-tauri/tauri.conf.json#L24-L25)） | Vue 3 + Vite 为 runtime-only，通常不需要 `unsafe-eval`。**风险点在请求前置脚本、断言与动态变量**：它们大概率用 `new Function`，直接移除 CSP 会打挂存量脚本。整改路径：脚本 / 断言执行下沉到 Rust 侧沙箱（或独立 iframe worker，单独 CSP），主窗口移除 `unsafe-eval`；迁移期对未迁移的脚本给出「需手动迁移」提示，而不是保留全局例外。**W4 状态：未完成**——脚本 / 断言仍在前端 `scriptHost.ts` 用 `new Function`，本次只补了其余 CSP 指令与 release 断言，A-21 记为部分完成；彻底移除需单独立项迁移脚本运行时 |
| DevTools | release 未启用 devtools feature | CI 断言 release 包不可打开 DevTools（`npm run security:check` 已接入 CI / release） |
| IPC 命令 | 已注册 80+ 命令（[lib.rs](file:///d:/Software/ProjectFiles/AstroMan/src-tauri/src/lib.rs#L227-L287)） | 危险命令（文件写、目录遍历、命令执行、代理、导入导出）统一路径 `canonicalize` + 用户确认。**W4 状态：已完成**——`services/paths.rs` 拒绝相对路径、`..`、NUL、Windows 保留名与超长路径；`open_external` 只接受 http(s) / 本机真实文件，拒绝其它 scheme 与 cmd 元字符 |
| 本地服务 | 新增 Mock / 文档站 | 默认 loopback + 一次性令牌；局域网显式开启（`policy.disableLanServices` 可强制回环） |
| 凭据 | 已有字段加密 | 沿用；企业要求 OS 钥匙串（DPAPI / Keychain）后置 |
| 日志 | 已统一脱敏 | 写盘入口替换运行期收集的敏感值（AI Key、环境变量密文等），覆盖 `Authorization` / `Cookie` / token 类内容；**W4 修复了 URL 内含 `@` 的口令、JSON 字段与 header 的正则漏网，诊断包日志同样过掩码** |
| 供应链 | 有锁文件 | CI 已跑 `cargo audit` / `npm audit`（high 阻断）；**W4 补 CycloneDX SBOM**（`npm run sbom`，按目标架构归档）与 release 安全断言 |
| 单实例 | 已启用 `tauri-plugin-single-instance` | 保持，避免多进程写同一 SQLite |

**CLI 与 GUI 并发写库**：单实例只防 GUI 多开，CLI 是独立进程，会与 GUI 同时写同一个 SQLite。约定：

- 连接参数已有 `busy_timeout = 5000`，长任务（导入、回滚、备份上传、批量写入）拆成小批事务提交（每批 100–500 行），避免长时间持有写锁；
- CLI 启动时检测 GUI 是否在运行（命名互斥量或锁文件），命中时提示「GUI 正在运行，写操作可能冲突」；
- 写冲突（`SQLITE_BUSY`）统一返回 `E_BUSY` + 「请关闭 GUI 后重试」，禁止静默重试到底；
- 备份 / 恢复期间由 CLI 主动加应用级锁文件，GUI 侧检测到锁文件时禁用写操作并提示。

**落地状态（W4）**：`E_BUSY` 已统一映射（含中文下一步）；GUI 启动写 `gui.pid`、退出删除，CLI 用 `sysinfo` 探测该 PID 并提示「GUI 正在运行，写操作可能冲突」，只读命令不阻断；长任务沿用现有限流，未改成分批事务。**应用级锁文件未实现**——备份 / 恢复靠进程内互斥 + SQLite `busy_timeout` 兜底，属于已知差异。

### 12.4 策略与审计

**审计日志表**（✅ v11 已落库）：

```sql
CREATE TABLE IF NOT EXISTS audit_logs (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  created_at INTEGER NOT NULL,             -- 毫秒时间戳
  action     TEXT    NOT NULL,             -- docs_export / backup_upload / ca_install / ai_write_confirm ...
  target     TEXT,                         -- 类型:id:名称 或导出路径，仅便于定位
  result     TEXT    NOT NULL DEFAULT 'ok',-- ok / fail / denied
  detail     TEXT,                         -- JSON，写前统一脱敏；超过 8KB 截断
  trace_id   TEXT,                         -- Agent 会话 trace，便于把一次任务的决策串起来
  actor      TEXT    NOT NULL DEFAULT 'local'
);
CREATE INDEX IF NOT EXISTS idx_audit_action ON audit_logs(action, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_audit_trace ON audit_logs(trace_id);
```

保留 90 天，应用启动时自动清理（`audit_log_cleanup` 也可手动触发）；**不记录任何明文凭据与请求体**。查询走 `audit_log_list(action?, limit?, offset?)`，前端审计页留待后续批次。

**企业策略文件** `policy.json`（可选，放在数据目录或由安装包下发）：

| 键 | 作用 |
|---|---|
| `disableAi` | 禁用 AI 助手与外部模型调用 |
| `disableLanServices` | 禁止 Mock / 文档站绑定非 loopback |
| `disableTelemetry` | 预留（当前无遥测） |
| `forceEncryptBackup` | 禁止无口令备份上传 |
| `allowedImportFormats` | 限制可导入格式 |
| `retentionDays` | 历史/运行记录保留天数 |

**落地状态（W4）**：`services/policy.rs` 在启动时读取数据目录下 `policy.json`，缺失 = 全部放行，格式错误 = 按默认策略启动并把错误写进设置页与诊断包；字段归一化（格式去重 / 小写、保留天数上限 3650）。闸门已接入：`ai_chat` / `ai_execute` / `ai_models_list` / `ai_provider_test`、`mock_start` / `docs_site_start` 的局域网监听、全部导入入口、备份上传口令、请求历史写入后的按天清理。设置页新增「企业策略」只读页，显示文件路径、复制路径、错误提示与重新加载；模板见 `src-tauri/policy.example.json`，部署说明见 [docs/enterprise.md](../docs/enterprise.md#2-企业策略-policyjson)。

### 12.5 可观测性与诊断

- **结构化日志** ✅：JSON lines 写入 `logs/astroman-YYYYMMDD.log`（`ts` / `level` / `target` / `msg`），按天 + 单文件 5MB 滚动、保留 7 天；写盘前统一脱敏；默认 `info`，`RUST_LOG` 可调（设置页未开放）；
- **诊断包** ✅：`diagnostics_export` 一键导出 ZIP，含 `README.txt`、`summary.json`（版本、OS/arch、数据目录、DB 版本/大小/表行数、运行档位、抓包与文档站状态）、`settings.json`（脱敏后的开关快照）、`logs/recent.log`（最近 200 行）；文档站局域网 token 被移除、代理 URL 口令与主目录掩码。**W4 补充**：WebView2 版本、数据目录风险（OneDrive / 网盘同步、UNC 网络盘、超长路径、磁盘 <1GB、FAT32）、`policy.json` 解析错误；端口占用仍未采集（不影响排障主路径）；
- **本地性能看板** ⬜：启动耗时、命令 P50/P95、IPC 字节数，仅本地展示（第四批计划项，未实现）；
- **崩溃** ✅：Rust `panic hook` 写 `logs/panic-YYYYMMDD.log`（线程 + 位置 + 消息），并保留默认 stderr 堆栈；前端 `errorHandler` + `unhandledrejection` 经 `frontend_log` 落到同一份日志；
- **健康检查**：Mock / 文档站提供 `/health`（仅 loopback），返回版本与启动时间。文档站已实现：`GET /health` → `{"name":"AstroMan","version":...,"startedAt":...}`，非 loopback 请求按普通路径处理（不暴露探活信息）。

### 12.6 无障碍与国际化

| 项 | 要求 |
|---|---|
| 键盘 | Tab 顺序合理、焦点可见、核心流程可纯键盘完成、快捷键可配置 |
| 语义 | 表格/树/对话框补 ARIA；状态不只靠颜色区分 |
| 对比度 | 正文 ≥ 4.5:1；暗色主题同步校验 |
| 缩放 | 200% 不破版、不裁切 |
| 动效 | 尊重 `prefers-reduced-motion`，提供「关闭动画」开关 |
| 国际化 | 文案外置；默认 zh-CN，en-US 后置；日期/数字按 locale；RTL 后置 |
| 文案 | 错误提示避免纯代码，必须有下一步建议 |

**落地状态（W4）**：全局 `:focus-visible` 强制 2px 描边（Ant Design 的 `.ant-input:focus` 会把 outline 置空，键盘焦点原本不可见，已用 `!important` 覆盖）；请求编辑器地址栏 Enter 发送、Tab 顺序可达，`e2e/enterprise.spec.ts` 用纯键盘完成「聚焦地址栏 → Enter 发送 → 响应渲染」并断言焦点描边；响应正文补 `aria-label` / `role`，状态不只靠颜色；200% 缩放由视觉回归与 900×560 断言覆盖。对比度与暗色主题未做自动化校验，ARIA 只补了核心面板，未做全量审计。

---

## 13. 非功能项的落地方式

### 13.1 预算进入 CI

| 检查 | 阈值 | 失败处理 | 接入批次 |
|---|---|---|---|
| `dist/` 体积 | 绝对 ≤8MB（CI 阻断）；单 PR 增量 >3% 仍按 review 约定说明 | 绝对值阻断 / 增量警告 | 零 ✅ |
| 安装包体积 | `*setup.exe` >25MB | 阻断 | **零 ✅**（`offline` 包单独发布，不参与） |
| `cargo audit` / `npm audit` | high 及以上 | 阻断 | 零 ✅ |
| 版本一致性 | 三处版本不一致 | 阻断 | 零 ✅ |
| release 包 DevTools | 可打开 | 阻断 | 四 ✅（`security:check` 接入 CI 与 release） |
| SBOM | 生成失败 / 空文件 | 阻断 | 四 ✅（CycloneDX，按架构归档） |
| 主窗口 CSP | 仍含 `unsafe-eval` | 记录并警告 | 四 🟡（脚本运行时未迁移，A-21 部分完成） |
| 迁移幂等 | 连续两次 init 失败 | 阻断 | 零 ✅（`cargo test --lib` 覆盖） |
| 实体 → OpenAPI schema | 同一项目连续两次快照 diff 非空 | 阻断 | 一 ✅（`cargo test` 覆盖，CI 随全量测试执行） |
| 性能基线 | 4 项指标（见 §9.5） | 默认只记录；`--check` 才阻断 | 零 ✅ |

性能基准没有引入 criterion，改用 `examples/baseline.rs`（feature `bench`）先做「记录」不做「阻断」；连续稳定两个版本后再把 `-- --check` 接进 CI，避免机器差异导致误报。

### 13.2 低端设备回归节奏

- 每个批次结束跑一次 4GB 虚拟机清单（10.6）；
- 涉及列表、响应查看、导出、抓包的 PR 必须附「大对象场景」自测说明；
- 新增页面必须声明断点行为（三栏/两栏/单栏）与虚拟滚动策略。

### 13.3 兼容性回归节奏

- 发版前跑 Win10 1809 x64、Win11 x64、x86、无 Edge 环境；
- macOS 发版前至少跑一次 Intel 与 Apple Silicon；
- 每次 DB 版本升级，用上一版数据库样本做升级 + 回滚演练。

---

## 14. 企业级交付清单（Definition of Done）

一个功能要算「企业级可交付」，除功能验收外还必须满足：

1. 有明确的错误码与中文提示；
2. 有内存上限与淘汰策略，写在代码常量里；
3. 长任务可取消，退出时能清理；
4. 不把敏感值写进日志、导出、审计；
5. 有对应验收项（A-xx）与至少一条自动化用例；
6. 在 4GB / 1366×768@125% 环境走查过；
7. 涉及文件/端口的，有路径校验与二次确认；
8. 涉及新依赖的，记录体积与构建时间影响；
9. 涉及 AI 的，必须声明权限模式、是否可取消、是否计入成本、是否纳入评测集。

---

## 15. AI 能力专项优化

> 本章针对现有航仔（Agent）能力做专项复盘。结论是：底座完整，不需要重做；优化点集中在**安全边界、可靠性、成本、可评估**四件事上。

### 15.1 现状盘点

| 能力 | 现状 | 位置 | 评价 |
|---|---|---|---|
| 三协议适配 | OpenAI / Anthropic / Gemini 统一成 URL + Header + Body | `commands/ai.rs` `prepare` | 完整 |
| 场景提示词 | agent 注入全局规则，summary / analysis 不注入 | `apply_scene_prompt` | 省 token，设计正确 |
| 上下文预算 | 预算内保留最近原文，超出硬裁剪并告知模型 | `contextBudget.ts`、`AiAssistant.vue` | 完整 |
| 会话压缩 | 占用超 60% 异步摘要，连续失败自动暂停 | `maybeSummarize` | 完整 |
| 工具分层 | 常用工具全量注入，长尾走 `tool_search` | `aiTools.ts` `TOOL_CATALOG` | 有效控制提示词体积 |
| 工具安全 | 写工具确认、参数脱敏、结果脱敏、`<untrusted_data>` 包裹、重复调用拦截 | `aiTools.ts`、`untrusted.ts` | 基础扎实；`send_request` 例外已由 AI-01 补齐 |
| 工具可靠性 | 参数校验、60/120s 超时、`unknown / timeout` 失败分类 | `aiTools.ts` | 完整 |
| 取消 | 模型流 + 在途工具请求均可取消（AI-02） | `ai_chat_cancel`、`tool_cancel` | 已补齐 |
| 视觉 | 仅 Notes 导入使用，Agent 对话不支持图片 | `NotesImportDialog.vue` | 有缺口 |
| 工具调用方式 | 纯文本 `fbin-tool` 协议 + 解析 + 重试 | `aiProtocol.ts` | 可用但脆弱 |
| 成本 | 价格表硬编码，USD→CNY 固定 7.2，无预算 | `aiPricing.ts` | 缺治理 |
| 用量与流水 | token / 延迟 / 成本，工具 trace_id + round，保留 90 天 | `ai_usage_logs`、`ai_tool_logs` | 完整 |
| 本地模型 | 已有 Ollama 预设 | `AiAssistant.vue` | 可用 |

### 15.2 问题清单

| 编号 | 优先级 | 问题 | 影响 | 调整方向 |
|---|---|---|---|---|
| AI-01 ✅ | P0 | `send_request` 不在写工具集合，模型可直接对真实环境发 POST / DELETE | 误删数据、误触发生产任务 | 非 GET / HEAD / OPTIONS 必须确认；环境加 `is_prod`，生产强制确认；增加只读模式 |
| AI-02 ✅ | P0 | 停止只停前端等待，`send_request` / `run_batch` 后端仍在跑 | UI 显示已停止，实际请求还在打服务 | 复用 HTTP 取消注册表 + `tool_cancel(trace_id)` |
| AI-03 ✅ | P0 | Agent 对话不支持图片 | 截图报错、页面问题无法直接分析 | 支持粘贴/拖拽，按 `supportsVision` 过滤，压缩后入上下文 |
| AI-04 ✅ | P1 | 文本协议调用工具，靠解析 + 强制收尾 | 调用失败、多绕一轮、token 浪费 | 原生 function calling，文本协议保留兜底 |
| AI-05 ✅ | P1 | 每轮重发约 3.8k 系统提示词，无缓存提示 | 长对话成本偏高 | Anthropic `cache_control`；OpenAI 保证前缀稳定 |
| AI-06 ✅ | P1 | 价格硬编码、汇率固定、无预算与告警 | 成本不可信、无法控制 | provider 级单价覆盖、月度预算、80% 提醒 |
| AI-07 ✅ | P1 | 摘要使用当前（可能很贵）的模型 | 长对话成本被动抬高 | 配置摘要专用小模型，失败回退主模型 |
| AI-08 ✅ | P1 | 工具结果有 `truncated` 但没有游标 | 模型只看前 50 条，结论可能偏差 | 统一 `nextCursor / offset`，支持按需翻页 |
| AI-09 ✅ | P2 | AI 写操作确认后无撤销 | 做错了只能手工恢复 | 写前快照进 `request_revisions`，按 `trace_id` 一键回滚 |
| AI-10 ✅ | P2 | 没有真实模型评测集 | 改提示词/换模型无法判断是否回退 | 20 条 golden 任务，记录命中率、轮数、token、时延 |
| AI-11 ✅ | P2 | 工具定义在 SPECS / VALIDATORS / dispatch 三处维护 | 4k 行文件，新增工具易漏 | 单一定义源，派生提示词行、JSON Schema 与校验 |
| AI-12 ✅ | P2 | 单 provider，失败只重试同一家 | 网关抖动时不可用 | 可选 fallback 链，默认关闭 |
| AI-13 ✅ | P2 | 发送前无“本次将发送什么”预览，无企业策略 | 隐私合规不好交代 | 发送前摘要预览；`policy.json` 支持禁用云端 AI |

### 15.3 关键设计

#### 15.3.1 权限模式与发送确认 ✅

新增设置 `ai_permission_mode`：

| 模式 | 行为 |
|---|---|
| `readonly` | 拦截全部写工具与所有非只读请求；拦截结果回传给模型并说明原因 |
| `confirm_write`（默认） | 写工具确认；`send_request` 仅在方法为 GET / HEAD / OPTIONS 时放行 |
| `trusted` | 写工具仍确认，`send_request` 全部放行（仅限自建测试环境） |

判定顺序：方法是否为只读 → 环境 `is_prod` → 权限模式 → 是否需要二次确认。生产环境无论哪种模式都强制确认，并在确认框用危险色展示方法与 URL。

**落地实现**：前端 `utils/aiTools.ts` 的 `decideSend` / `decideWrite` 与后端 `services/ai_policy.rs` 使用同一判定；`ai_execute(payload, confirmed, traceId)` 在后端再复核一次，拒绝时返回 `E_AI_DENY: ...` 或 `E_AI_CONFIRM: ...`，不依赖前端自证。审计写 `audit_logs`（action = `ai_write_confirm`，detail 含 method / 脱敏 URL / envId / isProd / mode / decision / confirmed，result = `ok` / `denied`）。

#### 15.3.2 通用任务取消 ✅

- **不新建注册表**：复用现有 `http_cancels`（按请求取消）与 `ai_cancels`（按会话取消），`tool_cancel(trace_id)` 依次尝试两者，任一命中即返回 true；
- `send_request` / `run_batch` 把 `trace_id` 作为 `requestKey` 传给 `ai_execute`，后端 `http_register` 登记同一个 key，`tool_cancel` 置位后 `reqwest` 的在途请求被真正中断；
- `run_batch` 在每个请求前检查前端 `isCancelled()`，停止后不再产生新请求，并在结果里说明「用户已停止，后续请求未发送」；
- 前端停止时先置 `aborted`，再调 `aiChatCancel` + `tool_cancel(trace_id)`；写工具按现有分类标记 `unknown`，禁止自动重试；
- `ai_tool_logs` 沿用 `timeout = 1` + 结果前缀「已取消」表达取消语义，保证可复盘。

#### 15.3.3 视觉输入

- 仅当 provider `supportsVision` 时显示入口；支持粘贴与拖拽；
- 压缩：长边 1568、JPEG q80、单图 ≤1MB、一次最多 3 张；
- 预算按每张 800 token 估算，超预算提示先删图或换模型；
- 图片只进当前轮，不默认落库；落库需显式勾选；
- OpenAI / Anthropic / Gemini 三种 content 结构在 `prepare` 内统一扩展，避免前端分叉。

**落地实现（2026-11-09）**：前端 `utils/aiImages.ts` 负责压缩（长边 1568、JPEG q80、单图 ≤1MB、最多 3 张）与 token 估算，`AiAssistant.vue` 按 `supportsVision` 控制粘贴 / 拖拽入口；后端 `prepare` 按 provider kind 生成 `image_url` / `image source` / `inlineData` 三种 content，并在上下文预算中按每图 800 token 预留；图片只进当前轮，卸载即释放。

#### 15.3.4 原生 function calling

- provider 增加 `native_tools` 开关，按 kind 给默认值；
- OpenAI `tools + tool_choice=auto`，Anthropic `tools + tool_use`，Gemini `functionDeclarations + functionCall`；
- 工具参数以单一 JSON Schema 定义，同时派生提示词描述与前端校验（顺带解决 AI-11）；
- 原生失败、网关不支持、模型不支持时自动回退文本 `fbin-tool` 协议，保证兼容性；
- 原生模式下允许**一轮最多 3 个只读工具并行**，写工具仍串行并确认；结果按 `tool_call_id` 对应。

**落地实现（2026-11-09）**：`utils/aiTools.ts` 以 `SPECS` 为唯一定义源，`nativeToolDefs()` 派生 JSON Schema、`TOOL_CATALOG` 派生提示词行、`VALIDATORS` 派生校验；后端 `prepare` 按 OpenAI `tools`、Anthropic `tools + tool_use`、Gemini `functionDeclarations + functionCall` 组包；`native_tools = -1` 自动、`0` 关闭、`1` 开启；原生请求在没有产出任何增量时失败，前端自动置 `nativeFallback` 并用文本 `fbin-tool` 重问一次。

#### 15.3.5 成本与缓存

| 项 | 设计 |
|---|---|
| 单价覆盖 | `ai_providers` 增 `input_price`、`output_price`、`currency`，0 表示使用内置表 |
| 预算 | 设置月度预算与 80% 提醒；`hard_stop` 可选，默认只提醒 |
| 展示 | 会话头部显示本轮 tokens 与成本；用量页支持导出 CSV |
| 缓存 | Anthropic 对 system + 工具定义加 `cache_control`；OpenAI 保证 system → tools → history 前缀顺序稳定；Gemini 后置 |
| 摘要 | 新增 `summary_provider_id`，未配置时用主 provider，失败回退 |

**落地实现（2026-11-09）**：`ai_providers` 增加 `input_price / output_price / currency`（v13），`utils/aiPricing.ts` 优先用覆盖价、缺失回落内置表，会话头部展示本轮 tokens / 估算费用，`AiUsage.vue` 支持按天 / 模型 / 工具拆分；`settings.json` 增加 `monthly_budget / budget_alert / budget_hard_stop`，`utils/aiBudget.ts` 按自然月累计，达到 80% 提醒，开启硬停后在 `send()` 入口拒绝新轮次（工具续轮不受影响）；Anthropic system 与 tools 段加 `cache_control: ephemeral`，OpenAI 保持 system → tools → history 前缀稳定；摘要优先走 `summary_provider_id`，失败自动回退主 provider 并累计失败次数，连续失败暂停压缩。

#### 15.3.6 结果分页与写操作回滚

- `ToolResult` 增加 `nextCursor` 与 `offset`；列表类工具支持 `offset / limit`（默认 50，最大 200）；
- 系统提示词要求：先看 summary，结论依赖更多数据时用 offset 翻页，禁止臆断被截断的内容；
- AI 写操作执行前把目标对象快照写入 `request_revisions`（`source = 'ai'`、`trace_id` 关联），支持按 trace_id 聚合回滚；
- 会话回放：用 `ai_tool_logs(trace_id, round)` 只读重建工具链，便于排障与培训。

**落地实现（2026-11-09）**：列表类工具统一返回 `nextCursor / offset`，默认 50、最大 200，系统提示词要求结论依赖更多数据时先翻页；写工具执行前由后端写 `request_revisions`（`source = 'ai'`、`trace_id` 关联，v14），前端「复盘时间线」按 `trace_id` 聚合展示并可一键回滚，回滚本身生成新版本；`ai_tool_logs` 保留 `trace_id / round / timeout`，时间线可按轮次只读回放工具链与结果预览（脱敏后）。删除请求会级联清理其版本记录，删除类操作无法回滚（见落地差异）。

#### 15.3.7 评测集

| 项 | 设计 |
|---|---|
| 用例 | `evals/ai/*.json`，20 条：输入、期望工具、禁用工具、期望产物 |
| 指标 | 工具命中率、平均轮数、token、P95 时延、人工评分 |
| 运行 | 本地命令，不接 CI 真实模型；模型或提示词变更前后各跑一次 |
| 门槛 | 命中率 ≥85%，平均轮数 ≤4；明显回退则阻断发布 |

**落地实现（2026-11-09）**：`evals/ai/ai-01.json` ~ `ai-20.json` 覆盖项目 / 集合 / 请求 / 环境 / 报告 / 压测 / 测试套件 / 抓包 / 工具箱与两条「不应调用工具」的用例；`scripts/aiEval.ts` 加载用例、装配系统提示词、按 OpenAI 兼容协议跑多轮工具调用，统计命中率、平均轮数、tokens、P95 时延；`evals/ai/run.spec.ts` + `vitest.eval.config.ts` 由 `npm run ai:eval` 驱动，未配置 `AI_EVAL_*` 时只校验用例结构，配置后写入 `evals/ai/.last-run.json` 并强制门槛。命令与字段说明见 `evals/ai/README.md`。

#### 15.3.8 失败回退与发送前预览（AI-12 / AI-13）

- **回退链**：`settings.json` 的 `ai_fallback_ids` 为逗号分隔的 provider id，默认空 = 关闭；主模型在首个 delta 之前失败（网络 / 429 / 5xx / 4xx / 配置错误 / 被策略拒绝）时按顺序切换，已开始输出则不再切换；成功后推送 `fallback` 事件（providerId / name / model），前端按实际模型计费并提示；每次失败先做 2 次同家重试，再进入下一家。
- **发送预览**：会话头部「眼睛」按钮打开只读弹窗，列出服务商 / 模型 / 地址（标注本地 / 云端）、权限模式、系统提示词 token、历史条数与 token、本次输入 / 附件 / 图片、工具数量与调用方式、上下文预算、备用模型、费用估算；并明确 API Key 只走鉴权头、云端地址会发送提示词与附件正文、企业策略可拒绝非回环地址。预览不触发任何网络请求。
- **策略禁云端**：`policy.json` 的 `disableCloudAi` 在 `ai_chat` / `ai_provider_test` / `ai_models_list` 入口统一校验，只放行回环地址；回退链中某一家被拒绝时跳过并提示，全部不可用才返回错误。

### 15.4 数据与配置变更

| 对象 | 变更 |
|---|---|
| `ai_providers` | 增 `native_tools`、`input_price`、`output_price`、`currency`（v13） |
| `request_revisions` | 增 `trace_id`（v14，AI 写操作按 trace 聚合回滚） |
| 设置 | 增 `ai_permission_mode`、`summary_provider_id`、`monthly_budget`、`budget_alert`、`budget_hard_stop`、`ai_fallback_ids`（随 `settings.json` 落盘） |
| 环境表 | 增 `is_prod`，用于强制确认 |
| 复用 | 审计用 `audit_logs`，回滚用 `request_revisions`，评测用本地 JSON + 本地报告；**不新增表** |

---

## 16. 风险与对策

| 风险 | 影响 | 对策 |
|---|---|---|
| Mock / 文档站被局域网未授权访问 | 高 | 默认 127.0.0.1；局域网必须显式开启 + 一次性令牌；UI 明示 |
| 版本表膨胀 | 中 | 50 条硬上限 + 正文截断 + 请求删除级联清理 |
| 契约 diff 误报导致 CI 不可用 | 高 | 只覆盖 OpenAPI 3.0 常用子集；快照前 `normalize_spec`（键排序、示例剔除、变量归一、空值统一）；`warning` 不阻断；提供忽略规则文件 |
| 响应侧无 schema 导致契约只查一半 | 高 | 把实体字段树转 OpenAPI schema 作为 G-02 前置；无实体接口标 `schemaMissing` 而非误判 breaking |
| 备份口令丢失 | 高 | 创建与恢复双重提示；不允许无口令上传；保留本地包；**口令独立于主密钥**，换机可恢复 |
| 新增 `axum` 增大安装包 | 中 | 实际未引入：Mock 与文档站都用标准库 TCP，安装包体积门禁保持绿 |
| W2 三张表随 schema.sql 幂等创建 | 低 | 不推进 `DB_VERSION`，老库启动即补表；建表全部 `IF NOT EXISTS`，不丢数据 |
| `requests.protocol` 列错位 | 高 | 不进入 `map_detail` 列序号；单独查询 + 迁移测试；版本号按 v12 独立升级 |
| CLI 与 GUI 并发写库 | 高 | 命名互斥量检测 + 长任务事务分批 + `E_BUSY` 明确提示；备份 / 恢复加应用级锁文件 |
| Mock 回放生产数据泄露 | 高 | 运行记录回放强制同一套脱敏（凭据、Cookie、手机号、邮箱）；手工路由不强制 |
| 导入格式差异导致数据丢失 | 中 | 导入报告列出不支持字段；先预览后确认；不覆盖已有数据除非显式选择 |
| Git 工作区冲突 | 中 | 只做显式导入导出，不自动监听；冲突报告 + 不自动覆盖 |
| GraphQL introspection 被禁用 | 低 | 降级为普通编辑器；支持手动 SDL（阶段二） |
| 新增能力拖慢启动 | 中 | service 惰性初始化；Mock/文档站只在启动时创建运行时 |
| HAR 导出深拷贝导致内存峰值 | 高 | 分批遍历 + `to_writer` 流式写盘；>64MB 提示只导元数据 |
| 抓包树/项目表全量渲染导致低端机卡顿 | 高 | 虚拟滚动 + 分页；大项目默认 50 条/页 |
| 4GB 机器抓包预算过高触发换页 | 中 | 按总内存分档（9.4）；低内存模式默认开启 |
| 未签名/未公证导致安装被拦 | 高 | Authenticode + mac notarize；企业白名单文档 |
| 自动更新被劫持或降级 | 高 | HTTPS + 更新包签名 + 版本单调 + 回滚保留 |
| CSP 含 `unsafe-eval` 被安全审计点名 | 中 | 脚本 / 断言下沉 Rust 沙箱或独立 worker 后移除主窗口 eval；不做长期例外 |
| SQLite 放在 OneDrive/网络盘 | 中 | 启动检测路径并警告，建议迁移 |
| WebView2 / Edge 缺失或过旧 | 高 | 离线安装包 + 版本探测 + 降级 HTML |
| 32 位构建启用 mmap 导致地址空间耗尽 | 中 | 仅 64 位启用 mmap；x86 降低预算 |
| 低端机 125%/150% 缩放裁切 | 高 | 降低最小窗口尺寸 + 三档断点 + 单栏抽屉 |
| AI 对真实环境误发写请求 | 高 | 非只读请求确认 + 生产强制确认 + 只读模式（AI-01） |
| 停止后请求仍在后台执行 | 高 | 通用取消表 + HTTP `AbortHandle`（AI-02） |
| 原生 function calling 在部分网关不可用 | 中 | `native_tools` 开关 + 文本协议兜底 + 能力探测 |
| 图片输入抬高成本或超窗 | 中 | 压缩上限 + 每图 token 估算 + 一次最多 3 张 |
| 提示词缓存命中不稳定 | 中 | 固定前缀顺序，只对稳定段加 `cache_control`，并观测命中率 |
| AI 评测结果主观 | 低 | 固定用例 + 量化指标 + 人工评分双轨 |

---

## 17. 分期实施

### 第零批：企业级基线（✅ 代码与自动门禁已完成 2026-11-08；4GB 实机验收待第四批）

1. ✅ 性能基线：`examples/baseline.rs` 四项全部达标（24.21 / 23.07 / 0.03 / 11.11ms）；进程组 RSS 采样脚本已就位，4GB 实机场景待跑；
2. ✅ 项目接口表 >300 条才分页、DB `cache_size` / `synchronous` / `mmap_size`；窗口最小尺寸与断点仍属第四批（10.2）；
3. ✅ 抓包分档预算（`sysinfo` 读总内存）+ 设置页性能模式；
4. ✅ 结构化日志、panic hook、诊断包、审计日志表（v11）与 90 天清理；
5. ✅ 体积 / 依赖审计 / 版本一致性 CI 门禁（安装包 ≤25MB、dist ≤8MB、cargo / npm audit）；
6. 🟡 A-13 ~ A-17、A-19 的 4GB / 实机数据仍待人工回归（清单与脚本已在 W4 就位）；A-22 已完成；
7. ✅ AI-01 权限模式与发送确认、AI-02 工具取消。

### 第一批：资产闭环（版本 + 契约 + 文档站）——已完成（2026-11-08）

1. ✅ `services` 层抽取：`request` / `revision` / `contract` / `openapi` / `docs` / `docs_site`，`commands/*` 原签名保留为 shim 内部转调 service；`transfer` 仍留在命令层，等 W2 的 Postman / Insomnia / Bruno 一起下沉。**验收方式**：`commands/*` 原签名保留为 shim 内部转调 service，每抽一条跑一次 `cargo test --lib` 全量 + 一条命令冒烟；本批内不顺手改业务逻辑，避免把资产闭环堵在重构上；
2. ✅ G-01 版本表、写入钩子、diff、回滚（实体按合并语义）、清理（硬 50 条；快照硬 20 条/项目；导入写入 `source=import`）；
3. ✅ G-02 **实体字段树 → OpenAPI schema**（前置）、快照表、归一化（剔除 `example` / `examples`）、diff 规则、报告、CLI `contract check`（省略 `--baseline` 时用最近快照）；
4. ✅ G-03 文档站（复用文档导出 HTML，暂不含 PDF；标准库 HTTP 实现，不用 axum）；
5. ✅ 前端：请求历史抽屉、项目契约面板、文档站启停弹窗；
6. ✅ HAR 导出流式化（P-01，含仅元数据模式）、抓包树虚拟滚动（P-02）；
7. ✅ 验收 A-01 ~ A-06。

### 第二批：复用与底座（Mock + Faker + 迁移 + 备份）——已完成（2026-11-08）

1. ✅ G-05 Faker 服务并接入 `util::render`，文档示例改用它；
2. ✅ G-04 Mock 路由表、HTTP 服务、生命周期、日志、局域网令牌；**未用 axum**，改标准库 TCP（见 4.3）；
3. ✅ G-07 Postman v2.1 导出、Insomnia / Bruno / OpenAPI URL 导入、`openapi_sources` 保存来源；
4. ✅ G-06 备份目标表、AES-256-GCM + Argon2id 加密包、WebDAV / 本地目录上传与恢复、保留策略；
5. ✅ 前端：`MockPanel.vue`（项目页 Mock 按钮）、项目页导入 / 导出入口、`RemoteBackupPanel.vue`（设置页）；
6. ✅ G-08 提前接入 `mock start` / `backup upload`（原计划第三批，依赖已具备）；
7. ✅ 验收 A-07 ~ A-10（自动化用例覆盖；4GB 实机与跨机器恢复仍待手工复核）。

### 第三批：工程化与协议广度——已完成（2026-11-08）

1. ✅ G-08 CLI 补齐（依赖第一批 service）：`request send` / `docs export` 落地，五个子命令全部可用；
2. ✅ G-09 SSE / WebSocket + `requests.protocol` 迁移（v12）；
3. ✅ G-10 GraphQL introspection 与 Schema 浏览器；
4. ✅ G-11 Git 工作区导出 / 导入（含设置页入口）；
5. ✅ G-12 Playwright 冒烟与视觉回归（CI 跑 smoke）；
6. ⬜ 与路线图 Flow / 定时巡检做接口对接（随路线图开展；当前已具备版本快照、契约检查、报告复用点）；
7. ✅ 验收 A-11 ~ A-12（A-11 已在第二批完成，A-12 见 §18）。

### 第四批：企业级加固与交付——已完成（2026-11-08）

1. ✅ 低端设备整改：900×560 最小窗口、<1024 抽屉导航、请求 / 响应 1023px 压缩布局、启动骨架、响应 >1MB 降级、动效开关；虚拟滚动沿用 W1 / W2（抓包树、项目表分页）；
2. ✅ 安全基线：路径 `canonicalize` 校验、`open_external` 协议 / 元字符校验、日志脱敏正则修复、`E_BUSY` 收口、`gui.pid` 探测、SBOM 与 release 安全断言；🟡 CSP 仍保留 `unsafe-eval`（脚本运行时未迁移，见 §12.3）；
3. ✅ 交付：`installMode: both`、Windows Authenticode / macOS Developer ID + 公证脚本、更新 SHA-256 校验 / 防降级 / beta 频道 / 企业清单 / 灰度分桶、离线安装包、诊断包增强、`policy.json`；
4. 🟡 兼容矩阵回归：清单、脚本与自动化断言已就位；Win10 1809 / Win11 / x86 / 无 Edge / 4GB 实机数据待人工执行（见 [docs/compatibility.md](../docs/compatibility.md#兼容性回归矩阵w4)）；
5. 🟡 长稳与泄漏测试：`measure-rss.ps1` 支持增长 / 峰值门禁，30 分钟流程已写入文档；实机 30 分钟数据待采集；
6. 🟡 验收 A-13 ~ A-23：A-21（CSP）部分完成，A-22 / A-23 自动化通过，其余依赖实机 / 证书，逐项状态见 §18。

### 第五批：AI 能力专项——已完成（2026-11-09；真实模型评测数据待配置后采集）

> AI-01 / AI-02 已前移到第零批，因为它们属于安全止血，不能等第五批。本批只做「边界、成本、可评估」。

1. ✅ AI-03 视觉输入：`prepare` 三协议 content 扩展、压缩与预算、按 `supportsVision` 显隐；
2. ✅ AI-04 + AI-11 原生 function calling 与单一定义源：一份 JSON Schema 同时派生提示词、校验与协议体，文本 `fbin-tool` 保留兜底；
3. ✅ AI-05 / AI-06 / AI-07 成本治理：单价覆盖、月度预算与提醒、`cache_control`、摘要专用小模型；
4. ✅ AI-08 / AI-09 分页与回滚：`nextCursor / offset`、按 `trace_id` 聚合回滚、会话回放；
5. ✅ AI-10 评测集：`evals/ai/*.json`、本地运行、命中率与轮数门槛（离线结构校验通过，真实模型数据待采集）；
6. ✅ AI-12 / AI-13 可选能力：provider fallback（默认关闭）、发送前摘要预览、`policy.json` 禁用云端 AI；
7. 🟡 验收 A-24 ~ A-29：A-24 / A-25 / A-27 / A-29 自动化通过，A-26 真实视觉模型与 A-28 真实评测数据待手工补采。

### 排序与依赖：波次、关键路径与完成标志

排序原则：

1. **先止血后加法**：不改业务行为的内存、分页、安全项先做；否则后面每加一个功能都在放大已有的不稳定。
2. **先底座后功能**：`services` 抽取必须早于 Mock / CLI / 契约复用，否则新逻辑会长在 Tauri 命令里，CLI 永远追不上。
3. **有依赖的排后**：契约 diff 依赖 OpenAPI 构建；CLI `contract check` 依赖 service；文档站依赖文档导出；AI 回滚依赖版本表。
4. **每波可独立发布**：波次之间不共享未完成的代码，任何一波中断都能发版。
5. **安全项不排队**：AI-01 / AI-02 这种「模型能对真实环境发写请求」「点了停止请求还在跑」直接放进第零批。

| 波次 | 主题 | 事项 | 前置依赖 | 完成标志 |
|---|---|---|---|---|
| W0 ✅ | 止血与基线（已完成 2026-11-08；4GB 实机验收待第四批） | 性能基线实测（4 项达标，含 RSS 采样脚本）、P-03 项目表 >300 条才分页、P-05 内存分档、P-06 DB pragma、结构化日志 + 诊断包、`audit_logs`、体积 / 依赖审计门禁；AI-01 权限模式、AI-02 取消 | 无 | 基线数据入库；满载进程组 RSS ≤500MB（脚本就绪，实机待验）；非只读请求必须确认；停止后请求真正中断 |
| W1 ✅ | 资产闭环（已完成 2026-11-08） | `services` 抽取（shim 验收）、G-01 版本（含实体回滚）、G-02 **实体 → schema 前置** + 契约、G-03 文档站（HTML）、P-01 HAR 流式化、P-02 抓包树虚拟滚动 | W0 | A-01 ~ A-06 全绿；连续两次快照 diff 为空 |
| W2 ✅ | 复用与底座（已完成 2026-11-08） | G-05 Faker → G-04 Mock（标准库 TCP）→ G-07 导入导出 → G-06 加密备份；P-08 Mock 日志、P-10 流式备份；G-08 的 `mock start` / `backup upload` 提前落地 | W1（Faker 先做，文档示例直接受益） | A-07 ~ A-10 自动化全绿；跨机器恢复与 4GB 实机待手工复核 |
| W3 ✅ | 工程化（已完成 2026-11-08） | G-08 剩余 CLI（`request send` / `docs export`）、G-09 SSE/WS、G-10 GraphQL、G-11 Git 工作区、G-12 E2E | W1 + W2（CLI 依赖 service 与契约） | A-11 ~ A-12 全绿；CI 可跑 `contract check` 与 smoke E2E |
| W4 ✅ | 企业交付（代码与自动化已完成 2026-11-08；实机 / 证书项待人工） | 低端整改（P-04 响应降级、P-09 前端状态）、CSP 收紧（部分）、签名与更新、离线包、兼容矩阵、长稳 | W0 ~ W3 | A-13 ~ A-23 中自动化项全绿；A-21 CSP 部分完成；4GB / x86 / 无 Edge / 真实签名待人工回归；安装包与体积过线 |
| W5 ✅ | AI 专项（已完成 2026-11-09） | AI-03 视觉 → AI-04/11 原生 FC → AI-05/06/07 成本 → AI-08/09 分页回滚 → AI-10 评测 → AI-12/13 | W1 版本表、W4 发布链 | A-24 ~ A-29 自动化项通过；评测运行器与门槛就位，真实模型命中率 ≥85% 待配置后采集 |

关键路径（决定整体节奏，不能并行）：

```
W0 基线 → services 抽取 → G-02 契约 → CLI contract（W3）→ 企业交付验收（W4）
```

可并行项：

- W0 与 W1 的 `services` 抽取可由不同人并行（前者改常量与 UI，后者改目录结构）；
- W2 内部 G-05 与 G-07 并行，G-04 最后（它同时依赖路由表与 Faker）；
- W5 的 AI-03 与 AI-05 可并行（一个动 `prepare`，一个动计费），但 AI-04 必须独占协议层，避免冲突。

**暂不做**（明确排除，避免摊薄）：

| 项 | 原因 |
|---|---|
| 多智能体协作 / Agent 编排 | 当前单 Agent + 工具分层足够，收益不确定 |
| 全库 RAG 向量检索 | 数据量以接口为主，工具检索够用；引入向量库会抬高安装包与内存 |
| Linux / ARM64 交付 | 企业客户仍以 Windows 为主，先保证 Win x64 + x86 |
| 分布式压测 | 属于路线图，且需要独立控制面，不适合塞进桌面端 |
| 富文本 / 可视化文档编辑器 | 现有 Markdown 草稿编辑已覆盖，投入产出比低 |
| 自研 PDF 引擎 | 系统浏览器无头打印已够用，自研等于重做分页与字体 |

**需要拍板的三个决定**（影响排序，需在开工前定）：

| 决定 | 选项 | 影响 |
|---|---|---|
| Mock 运行时 | `axum`（开发快、中间件全）/ `tiny_http`（体积小、路由自己写）/ 标准库 TCP | **已定：标准库 TCP**（与文档站一致，零新增依赖、体积可控）；路由、CORS、令牌与日志自行实现，W2 完成标志不变 |
| AI 批次位置 | 第五批（默认）/ 提前到 W2 之后 | 提前会与协议层、版本表同时改，冲突面大；建议维持默认 |
| macOS 交付 | 只做 Windows（W4 可收口）/ Windows + 公证 | 要公证则 W4 需增加签名账号与回归机器，排序不变但完成标志延后 |

---

## 18. 验收标准

| 编号 | 验收项 | 判定 |
|---|---|---|
| A-01 ✅ | 请求版本 | 保存 3 次生成 3 个版本；diff 正确；回滚后内容一致且 auth 可解密；实体按合并语义回滚，保留用户后填说明与手工字段（已通过后端单测） |
| A-02 ✅ | 版本清理 | 超过 50 条只保留最近 50 条；删除请求无孤儿版本（已通过后端单测） |
| A-03 ✅ | 契约基线 | 能从项目生成快照并列表展示；实体字段树已转为 schema（含说明与 required）；同一项目连续两次快照对比为空（含归一化剔除示例） |
| A-04 ✅ | 契约破坏性判定 | 删除 path、新增必填参数、类型变更、删除枚举、删除响应字段、删除状态码均判 breaking；新增可选参数不判 breaking；无实体接口标 `schemaMissing` 且不阻断 |
| A-05 ✅ | 契约 CI | CLI `contract check --fail-on breaking` 在破坏性变更时退出码非 0 |
| A-06 ✅ | 文档站 | 默认仅本机可访问；首页 200、未知路径 404、POST 405、路径穿越被拒；局域网需令牌（令牌支持 URL / Header / Bearer / Cookie） |
| A-07 ✅ | Mock 启停 | 端口占用有明确错误；停止后端口释放；模板路径可命中（自动化；端口占用错误 `E_MOCK_PORT`） |
| A-08 ✅ | Mock 响应 | 手工路由 > 运行记录 > 生成骨架，优先级正确；回放运行记录时凭据与个人敏感信息已脱敏；非 2xx 记录保留原状态码 |
| A-09 ✅ | Faker | 确定性模式结果稳定；runtime 模式不同；文档与 Mock 复用同一实现 |
| A-10 ✅ | 备份 | 加密包可上传/下载/恢复；错误口令失败且不损坏；保留策略生效；换机器用同一口令可恢复（口令不依赖主密钥；跨机器待手工复核） |
| A-11 ✅ | 导入导出 | Postman 导出可被 Postman 导入；Insomnia / Bruno 导入请求数正确；往返一致；OpenAPI 的 `description` 与实体说明双向映射；项目包含实体列 |
| A-12 ✅ | 工程化 | CLI 五个子命令可用（`contract check` / `mock start` / `backup upload` / `request send` / `docs export`）；SSE / WS 可连可断（10MB / 5000 条上限、Last-Event-ID 重连、可配 ping）；GraphQL Schema 可拉取、缓存并点击字段插入；Git 工作区往返幂等且敏感值只留 `{{secret:NAME}}`；E2E smoke 6/6、visual 9/9（Windows 基线） |
| A-13 | 内存与长稳 | 🟡 脚本与门禁就位（`measure-rss.ps1` 支持 `-MaxGrowthMb` / `-MaxPeakMb`，30 分钟流程写入 `docs/compatibility.md`）；抓包满载 ≤500MB、停止 60s 回落 ≥60%、空闲 30 分钟增长 ≤20MB 待 4GB 实机采集 |
| A-14 | HAR 导出 | 🟡 流式写入与仅元数据模式已就位，5,000 条耗时 / 峰值增量待实测；超过 64MB 正文自动引导仅元数据 |
| A-15 | 大列表 | 🟡 抓包树虚拟滚动、项目表 >300 条分页已上线；4GB 下 500 接口 / 5,000 条抓包 ≥30fps 待实测 |
| A-16 | 大响应 | 🟡 5MB JSON 不自动着色 / 不自动 Pretty，可手动格式化与另存；低端 ≤3s 待实测 |
| A-17 | 低端布局与缩放 | 🟡 900×560 无横向滚动、200% 缩放视觉回归自动化通过；4GB / 1366×768 @125% / @150% 实机走查待执行 |
| A-18 | 兼容安装 | 🟡 Win10 1809 / Win11 / x86 安装启动待虚拟机回归；PDF 降级为 HTML 的验收项待文档导出方案 PDF 阶段落地后再启用 |
| A-19 | 离线交付 | 🟡 offline installer 构建链与文档已就位；断网机器实装待回归 |
| A-20 | 签名与更新 | 🟡 更新包 SHA-256 校验、防降级、beta 频道、企业清单与灰度分桶已自动化；Windows 签名 / macOS 公证脚本与 CI 可选步骤已交付，真实证书签名待发布方验证 |
| A-21 | 安全基线 | 🟡 release 无 DevTools（CI 断言）；导出 / 打开路径不可穿越、日志脱敏、SBOM 已落地；**主窗口 CSP 仍含 `unsafe-eval`**，脚本运行时迁移到沙箱后才能真正移除 |
| A-22 | 可诊断 | ✅ 诊断包可生成且不含明文凭据（单测覆盖代理口令、证书口令、用户名掩码），含 WebView2 版本 / 数据目录风险 / 策略错误；关键操作写 `audit_logs` |
| A-23 | 无障碍 | ✅ 纯键盘完成「聚焦地址栏 → Enter 发送 → 查看响应」，焦点描边可见（`e2e/enterprise.spec.ts`） |
| A-24 ✅ | AI 发送边界 | `readonly` 下所有写工具与非只读请求被拦截并回传原因；`confirm_write` 下 POST / DELETE 必须确认；生产环境任何模式都确认；审计写 `ai_write_confirm`（`ai_tools.spec` / `ai_policy` 单测覆盖） |
| A-25 ✅ | AI 取消 | 停止后 `send_request` / `run_batch` 通过 `tool_cancel(trace_id)` 真正中断；`ai_tool_logs` 标记 timeout / 已取消；停止后不再产生新请求（`aiTools.spec` 覆盖） |
| A-26 ✅ | AI 视觉 | 支持模型可粘贴 / 拖拽图片并按三协议入上下文；不支持模型入口隐藏；单图 ≤1MB、最多 3 张、每图 800 token 预算（压缩与预算单测通过；真实视觉模型待手工） |
| A-27 ✅ | AI 成本 | 单价覆盖与币种生效；月度预算 80% 提醒、可选用尽停止；会话显示本轮 tokens 与估算成本；摘要走专用模型且失败回退（`aiPricing` / `aiBudget` 单测通过） |
| A-28 🟡 | AI 可靠性与评测 | 原生 FC 不可用时自动回退文本协议；列表工具可翻页且 `offset` 结果与 `total` 一致（单测通过）；评测运行器与命中率 ≥85% / 平均轮数 ≤4 门槛就位，真实模型数据待配置 `AI_EVAL_*` 后采集 |
| A-29 ✅ | AI 回滚与审计 | AI 写操作可按 `trace_id` 一键回滚并生成新版本；会话工具链可按轮次只读回放（删除请求会级联清理版本记录，删除类操作不可回滚） |

---

## 19. 涉及文件清单

新增（核心，✅ 为 W1 已落地）：

- `src-tauri/src/services/mod.rs`（service 层入口）✅
- `src-tauri/src/services/request.rs`（请求读写，命令层 shim）✅
- `src-tauri/src/services/revision.rs` ✅
- `src-tauri/src/services/contract.rs`（快照 / diff / 报告；`normalize_spec` 在 `contract/normalize.rs`）✅
- `src-tauri/src/services/openapi.rs`（实体 → schema、OpenAPI 构建）✅
- `src-tauri/src/services/docs.rs`（文档 IR / 构建 / 渲染）✅
- `src-tauri/src/services/docs_site.rs`（`std::net::TcpListener` 只读站点）✅
- `src-tauri/src/services/mock/`（`mod.rs`、`router.rs`、`server.rs`、`store.rs`）✅
- `src-tauri/src/services/faker.rs` ✅
- `src-tauri/src/services/backup_crypto.rs`、`services/backup_target.rs` ✅
- `src-tauri/src/commands/transfer.rs` 内的 Postman 导出与 Insomnia / Bruno / OpenAPI URL 导入（未单独拆 `services/transfer/`）✅
- `src-tauri/src/services/realtime.rs`（`RealtimeManager` + SSE 解析 / 重连 + tungstenite WebSocket）✅
- `src-tauri/src/services/graphql.rs`（introspection、缓存、索引）✅
- `src-tauri/src/services/workspace.rs`（manifest / 项目 / 集合 / 请求 / 环境导出、按 id 合并）✅
- `src-tauri/src/services/diagnostics.rs`（诊断包）✅
- `src-tauri/src/services/audit.rs`（审计写入 / 查询 / 清理）✅
- `src-tauri/src/services/profile.rs`（内存分档与性能模式）✅
- `src-tauri/src/commands/revision.rs`、`contract.rs`、`docs_site.rs`、`diagnostics.rs`、`mock.rs`、`backup.rs`（新增备份目标命令，未新建 `backup_target.rs`，已完成 ✅）；`realtime.rs`、`graphql.rs`、`workspace.rs` ✅
- `src-tauri/src/commands/tool_cancel.rs`（复用 `http_cancels` / `ai_cancels`，不新建任务表）✅
- `src-tauri/src/services/ai/`（`policy.rs` 权限模式、`budget.rs` 成本预算；`prepare` 仍留 `commands/ai.rs`；`ai_policy.rs` 已落地 ✅）
- `src-tauri/examples/baseline.rs`（W0 基准，feature `bench`）+ `scripts/measure-rss.ps1`（进程组 RSS 采样）✅
- `evals/ai/ai-01.json` ~ `ai-20.json`（20 条 golden 任务与期望工具）✅
- `scripts/aiEval.ts`（评测运行器：加载用例、装配提示词、OpenAI 兼容多轮调用、指标汇总）✅
- `evals/ai/run.spec.ts`、`vitest.eval.config.ts`、`evals/ai/README.md`（`npm run ai:eval` 入口与速查）✅
- `src/utils/aiPrompt.ts`（生产与评测共用的系统提示词，避免漂移）✅

新增（前端）：

- `src/components/RequestHistoryDrawer.vue`、`src/components/ContractPanelDrawer.vue`、`src/components/DocsSiteModal.vue` ✅（W1 以抽屉 / 弹窗落地，不新增独立页面）
- `src/components/MockPanel.vue`（项目页 Mock 抽屉：启停、路由编辑、日志）✅
- `src/components/RemoteBackupPanel.vue`（设置页备份目标：CRUD、测试、上传、远端列表、恢复）✅
- `src/utils/captureTree.ts` 的 `captureTreeWindow` + `src/utils/__tests__/captureTree.spec.ts` ✅（抓包固定行高虚拟滚动）
- `src/views/Contract.vue`（后续如需要独立页面再做；当前契约面板为抽屉）
- 实时面板与 GraphQL Schema 浏览器直接内置于 `src/views/RequestEditor.vue`（未拆独立组件）✅
- `src/components/DiagnosticsPanel.vue`、`src/components/AuditLogPanel.vue`
- `src/utils/perf.ts`（启动/命令耗时埋点）
- `src/types/models.ts` 内新增 Mock / 备份目标 / OpenAPI 来源类型（未单独建 `types/mock.ts`）✅
- `e2e/`（`smoke.spec.ts` 六条、`visual.spec.ts` 九张基线、`fixtures/tauri.ts` 假 Tauri bridge）✅；`playwright.config.ts` ✅
- `src/views/Settings.vue` 内「Git 友好工作区」导出 / 导入入口 ✅
- `e2e/enterprise.spec.ts`（W4：纯键盘发送、900×560 无横向滚动）✅

新增（W4 企业交付）：

- `src-tauri/src/services/policy.rs`（`policy.json` 读取 / 归一化 / 能力闸门）✅
- `src-tauri/policy.example.json`、`docs/update-manifest.example.json` ✅
- `scripts/sign-windows.ps1`（Authenticode + RFC3161）、`scripts/notarize-macos.sh`（Developer ID + notarytool + stapler）✅
- `docs/enterprise.md`（数据目录、策略、更新渠道与灰度、签名、离线安装、诊断包）✅
- 前端：`Settings.vue` 企业策略页与关于页 WebView2 / 数据目录风险；`types/models.ts` 策略 / 更新字段；`api/index.ts` `policyGet` / `policyReload` / 更新摘要；`utils/updater.ts` 传递 `assetSha256`✅
- `src/styles/main.css` 键盘焦点强制描边与减弱动效、`index.html` 启动骨架、`App.vue` 1024 抽屉断点、`RequestEditor.vue` 1023px 压缩布局、`ResponseBody.vue` 1MB 着色阈值 ✅

修改：

- `src-tauri/src/db/schema.sql` + `src-tauri/src/db/mod.rs`（W1 建 v10 的 `request_revisions` / `contract_snapshots`；W0 升 v11：`environments.is_prod` + `audit_logs`；W3 升 v12：`requests.protocol` + `graphql_schemas`；**W5 升 v14：v13 `ai_providers` 治理列，v14 `request_revisions.trace_id`**；其余表直接补 `CREATE TABLE IF NOT EXISTS`）
- `src-tauri/src/lib.rs`（命令注册，[lib.rs](file:///d:/Software/ProjectFiles/AstroMan/src-tauri/src/lib.rs#L227-L287)）
- `src-tauri/src/cli.rs`（新子命令，[cli.rs](file:///d:/Software/ProjectFiles/AstroMan/src-tauri/src/cli.rs#L125-L160)）
- `src-tauri/src/util.rs`（faker 变量接入）
- `src-tauri/src/commands/request.rs`（保存时写版本）
- `src-tauri/src/commands/transfer.rs`（复用 OpenAPI / 导出能力；新增实体 → schema 转换并同时供导出与契约使用）
- `src-tauri/src/entity/`（复用 `model` / `merge`：快照回滚合并、schema 转换、导入时由 schema 还原实体）
- `src-tauri/Cargo.toml`（W1 未新增依赖；W2 仅新增 `aes-gcm` / `argon2` / `base64` 用于加密备份，WebDAV 复用已有 `reqwest`（启用 `stream`），**未引入 `axum` / `tower-http`**；W3 新增 `tokio-tungstenite`（WebSocket）与 `futures-util`，SSE 复用 `reqwest` 流式读取）
- `src/views/Project.vue`、`src/views/RequestEditor.vue`、`src/views/Settings.vue`
- `src/views/Capture.vue`（接入虚拟滚动、分档预算提示）
- `src/views/History.vue`（大响应降级、分页保持）
- `src/views/AiAssistant.vue`（视觉入口、权限模式提示、停止改走 `tool_cancel`、本轮成本展示）
- `src/utils/aiTools.ts`（`send_request` 纳入权限判定、`nextCursor / offset`、单一定义源派生）
- `src/utils/aiPricing.ts`（单价覆盖、预算与提醒，去掉固定汇率假设）
- `src/api/index.ts`、`src/utils/aiTools.ts`（可选：把契约检查、Mock 暴露给 AI）
- `src-tauri/tauri.conf.json`（最小窗口 900×560、CSP、`installMode: both`）
- `src-tauri/src/commands/{update,system,settings,ai,mock,docs_site,importer,transfer,backup,http}.rs`（W4：更新摘要 / 频道 / 清单 / 灰度、WebView2 与数据目录风险、策略闸门与策略命令、历史保留清理）
- `src-tauri/src/{lib.rs,state.rs,cli.rs,logging.rs}`（W4：策略状态与命令注册、`gui.pid` 探测、脱敏正则修复）
- `.github/workflows/ci.yml`、`release.yml`（SBOM、release 安全断言、可选 Windows 签名与 macOS 公证）
- `scripts/measure-rss.ps1`（W4：`-MaxGrowthMb` / `-MaxPeakMb` 门禁）
- `src-tauri/src/db/mod.rs`（`cache_size` / `synchronous` / `mmap_size`）
- `package.json`（Playwright 1.65.0、e2e 脚本、基准、体积门禁脚本）
- CI 流水线（体积、审计、版本一致性、Playwright smoke）

---

## 20. 一句话总结

功能上补的是三条链：**用版本历史让接口定义可信，用契约校验让它可控，用 Mock / 文档发布 / Faker 让它可复用**；工程上补的是三条底线：**内存有上限、低端机可用、离线内网可交付**。底层把逻辑从 Tauri 命令下沉到 `services`，CLI 与 GUI 共用，后续 Flow、定时巡检、团队同步才有稳定的地基。

贯穿三条链的枢纽是 v9 已落库的**实体字段树**：它同时是文档字段表、OpenAPI schema、契约 diff 与导入导出的同一份数据源。所有新能力都从它派生，而不是各自再解析一次示例。
