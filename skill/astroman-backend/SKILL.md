---
name: astroman-backend
description: AstroMan 的 Rust/Tauri 后端开发规范。改数据库 schema、写命令、调压测引擎、改报告结构、排查 SQLite 迁移报错时使用。触发词：后端、Rust、command、SQLite、schema、migrate、压测引擎、报告字段。
---

# Rust 后端规范

## 模块职责

| 文件 | 职责 |
|---|---|
| `commands/project.rs` | 项目 CRUD |
| `commands/collection.rs` | 集合树（支持无限嵌套、拖拽排序 `collection_reorder`） |
| `commands/request.rs` | 接口 CRUD、搜索、排序 |
| `commands/environment.rs` | 环境与变量、环境级脚本 |
| `commands/http.rs` | 发送请求：变量渲染、认证、Cookie、body（JSON 注释剥离）、历史入库与裁剪 |
| `commands/oauth.rs` | OAuth2 Client Credentials 取 token |
| `commands/cookie.rs` | Cookie 存储 |
| `commands/stress.rs` | 压测引擎（并发/阶梯/RPS 限速/SLA） |
| `commands/plan.rs` | 压测计划持久化 |
| `commands/report.rs` | 报告查询、导出、逐请求明细、写文件 |
| `commands/history.rs` | 历史列表、统计、按范围清理 |
| `commands/run.rs` | 单接口运行记录 |
| `commands/transfer.rs` | 项目 JSON 导入导出、OpenAPI 导入导出 |
| `commands/importer.rs` | Postman / Apifox / HAR 1.2 数据导入（HAR 按 host 分组，query → Params，form → Form） |
| `commands/curl.rs` / `commands/tools.rs` | cURL 解析、哈希/HMAC、IP 查询 |
| `commands/files.rs` / `commands/storage.rs` | 文本文件读写、数据目录迁移 |
| `commands/settings.rs` | 代理与客户端证书设置 |
| `commands/system.rs` / `commands/winstats.rs` | 应用信息、工作区统计、首页数据大屏聚合（`dashboard_metrics`）、Windows 进程指标 |
| `commands/capture.rs` | 抓包启停 / 状态 / 流量清单与详情 / CA / 系统代理 / HAR / ZIP 导出 / 保存 body / 改参重发 / 断点 / 映射 / 弱网 |
| `capture/` | 抓包内核：`mod`（生命周期）/ `proxy` / `handler` / `store` / `ca` / `sysproxy` / `har` / `archive` / `intercept`（断点队列、Map Local / Remote、弱网）（详见 `astroman-capture`） |
| `commands/notes.rs` | 待办 CRUD、计数、标签、设置、附件、截图识别；薄封装，领域逻辑在 `notes/` |
| `notes/` | `mod`（CRUD / 筛选 / 重复推进 / 静默判断）/ `quick`（迷你窗口 + 全局快捷键）/ `reminder`（30 秒调度）/ `attachment`（sha256 去重 BLOB）/ `vision`（视觉模型识别）（详见 `astroman-notes`） |
| `commands/update.rs` | 版本检查与下载 |
| `commands/ai.rs` | AI 助手「航仔」：服务商、流式对话、会话消息、工具流水（详见 `astroman-ai`） |
| `commands/test_suite.rs` / `commands/test_case.rs` | 测试套件、用例、数据集的 CRUD（`test_suite_save` 兼新建与更新） |
| `commands/test_run.rs` | 运行启动 / 停止 / 查询 / 当前运行；`test_run_start` 建 queued 记录后立即返回 `runId`，执行在后台任务 |
| `commands/test_preflight.rs` | 运行前预检：缺接口 / 缺脚本 / 缺变量 / 依赖环，`strictScripts` 时把跳过项升级为 error |
| `commands/test_report.rs` | 运行产物生成（HTML / JSON / JUnit / summary）与附件导出 |
| `commands/test_pack.rs` | 套件包 `*.astromantest.json` 导入导出（含请求快照与数据集，同名冲突 skip / overwrite / rename） |
| `test_runner.rs` | 套件执行内核：依赖跳过、失败策略、重试、并发、数据行、setup / teardown、进度事件；**不依赖 Tauri**，CLI 复用 |
| `test_assert.rs` | 6 类声明式断言：`status` / `durationMs` / `header` / `body` / `regex` / `jsonPath` |
| `test_dataset.rs` | CSV / JSON 数据集解析，嵌套字段摊平成 `a.b.c` 供 `{{a.b.c}}` 渲染 |
| `python_exec.rs` | Python 子进程：解释器探测、`context.json` / `result.json` 协议、超时与输出上限、附件白名单 |
| `report_builder.rs` | 运行报告渲染（HTML / JSON / JUnit / summary），桌面端与 CLI 共用同一份产物 |
| `net_probe.rs` | 建连计时探针：连接器 `call()` 未触发时 `connectMs = 0` 且 `reusedConnection = true`；压测不使用它 |
| `secret.rs` | 主密钥（钥匙串 / 口令 / 文件三种来源）、AES-256-GCM 字段加解密、密钥包导入导出、启动密钥校验 |
| `backup.rs` | 快照（日 / 周 / pre-migrate / pre-restore）、恢复、`quick_check` 自检、诊断包 |
| `logging.rs` | 按天滚动落盘、脱敏、`tail` 取诊断日志 |
| `state.rs` | AppState、连接池、reqwest 客户端（普通/忽略证书）、AI 在途请求取消表 `ai_cancels: HashMap<requestId, AtomicBool>` |
| `models.rs` | 全部前后端数据模型 |
| `util.rs` | `err`、`now_ms`、`render`、`strip_json_comments` 等工具 |
| `netinfo.rs` | 本机 IP/网卡信息 |

## 错误处理

- 所有 command 签名：`Result<T, String>`
- IO/DB/序列化错误：`.map_err(err)?`（`util::err` 会带上原始错误信息）
- 业务校验：直接 `Err("...".to_string())` 或 `format!`
- 文案中文、面向用户，说明「怎么改」而不只是「失败了」

## 请求体处理（JSON 注释）

请求 Body 允许像 Postman 一样写 `//` 行注释与 `/* */` 块注释，剥离发生在 Rust 侧，前端只负责编辑体验。

- `util::strip_json_comments(input)`：按字符扫描，跟踪字符串与 `\` 转义，字符串里的 `https://a.com/x`、`"a//b"` 不会被误删；支持未闭合的块注释（吃到文本末尾）
- **顺序固定：先剥注释，再 `render()` 渲染 `{{变量}}`**。反过来的话，被注释掉的行里的变量也会被替换，且注释内容进入渲染结果
- 只在 `body_type == "json"` 时处理；`raw` / `form` / `multipart` / `binary` / `graphql` 原样发送（GraphQL 的 body 由前端 `JSON.stringify` 生成，本身不含注释）
- 两个入口都要处理，漏一个就会出现"调试能发、压测报参数解析异常"：
  - `commands/http.rs::execute`（手动发送与 AI `send_request` 共用）
  - `commands/stress.rs::send_once`（压测）
- `sent_body` 必须用剥离后的内容：`request_runs.body`、压测报告的请求明细、运行历史里展示的都应是**实际发出的字节**
- 前端不要提前删注释，否则用户切回编辑器时原文丢失；编辑器里的注释由 `CodeArea` 的 `Ctrl / ⌘ + /` 维护

## 数据库

- 连接：`app.pool.get().map_err(err)?`，用完即释放（drop），不要在命令里长期持有
- 参数化查询一律 `params![]`，禁止字符串拼 SQL
- 连接池初始化在 `state.rs::open_pool`：`journal_mode=WAL`、`foreign_keys=ON`、`busy_timeout=5000`，max_size 8
- 初始化顺序固定：`schema.sql` → `migrate()` → `seed_database()`

### 迁移铁律

1. 新增列写进 `db/mod.rs::migrate()` 的 `ALTER TABLE` 列表，重复执行用 `let _ =` 吞掉"列已存在"
2. **索引依赖新列时，必须在补列之后创建**（历史事故：`idx_plans_project` 引用 `updated_at`，留在 schema.sql 导致老库启动 panic）
3. 老库兼容字段要给 `DEFAULT`，否则 `serde` 反序列化/查询会拿到 NULL 炸掉

示例数据在 `db/seed.rs`：空库 `seed()`，已有库 `upgrade_sample()` 补齐示例场景。

当前数据库结构版本 **6**（`astroman.db` 的 `PRAGMA user_version`）。v1.0.9 新增三张表，均由 `db/mod.rs::migrate()` 幂等创建：

```sql
notes(id, project_id?, ref_type, ref_id?, title, content, kind, status, priority,
      tags, due_at, remind_at, remind_lead_min, remind_repeat, snooze_until,
      notified_at, pinned, created_at, updated_at, done_at, deleted_at,
      deleted_from_status)
note_files(id, sha256, file_name, mime, size, data BLOB, created_at)
  -- idx_note_files_sha UNIQUE(sha256)
note_file_links(note_id, file_id, created_at)   -- 联合主键 + ON DELETE CASCADE
```

- 抓包流量、抓包拦截规则**不进数据库**：流量在内存，规则在 `<数据目录>/capture/intercept.json`。
- 待办图片附件存 `note_files.data` BLOB，按 `sha256` 去重；`note_file_links` 解绑时只删链接，不立即删文件，由每日清理回收孤儿。
- 改表结构时 `DB_VERSION` 递增，并同时补 `schema.sql` 与迁移；老库升级不丢数据。

## 主密钥与钥匙串（`secret.rs`）

敏感字段（`environment_variables.value`、`ai_providers.api_key`）全部走同一套加密，**不要自己拼 AES**。

- 密文格式 `enc:v1:base64(nonce12 || ciphertext)`，AAD 固定为 `表名:列名`（`format!("{table}:{col}")`）。AAD 绑死列，密文搬到别的列解密必然失败
- 模块级入口：`secret::encrypt(aad, plain)` / `secret::decrypt(aad, stored)`；业务里不要直接用 `SecretStore` 内部方法
- 三种来源 `KeySource`：`Keyring`（Windows 凭据管理器 / macOS Keychain，推荐）、`Passphrase`（Argon2id 派生，密钥不落盘）、`File`（`数据目录/keys/master.key`，界面上明确写「等同于没有加密」）
- 钥匙串条目按数据目录派生：`AstroMan/master-key/<数据目录 sha256 前 8 字节>`，`KEYRING_USER = "master-key"`。多份安装 / 开发实例 / 测试库不再共用一条，避免互相覆盖
- `SecretStore::bootstrap(data_dir, pool)` 顺序固定：取密钥 → 迁移旧共享条目 → `verify_key` → `encrypt_plaintext` 补齐历史明文
- `verify_key` 三重确认：`app_meta.key_check` 哨兵密文 → `app_meta.key_fingerprint`（sha256 前 8 字节）→ 对已有 `enc:v1:` 样本抽样解密（老库兜底）。**校验失败时绝不能写 `key_check`**，否则会把错钥匙固化成「已验证」
- 校验失败只告警一次并置 `mismatch`，不再逐条刷 `解密失败` 日志；`store()?.mismatch()` 非空时设置页显示受影响条数 + 两条恢复路径（导入密钥包 / 重新填写）
- `import_keypack` / `set_passphrase` 在重加密失败时不整体报错——密钥已丢失时，这条路是唯一恢复手段，不能因为「用错钥匙重加密」直接失败
- **测试与 CI 永不写钥匙串**：`keyring_allowed()` 在 `cfg!(test)` 或 `ASTROMAN_DISABLE_KEYRING=1` 时返回 false，自动降级文件密钥。历史事故：`AppState::headless` 用临时库跑测试，`bootstrap` 在新库上生成新主密钥并写进全局共享条目，把真实安装的密钥覆盖了
- 改密钥相关代码后必须跑 `cargo test --lib secret::`（10 条），并确认没有新增写钥匙串的路径

新增敏感列的做法：写入前 `secret::encrypt`，读取时 `secret::decrypt`；列表接口只返回掩码 `••••••` + `hasValue`，提交值为掩码或空时沿用旧密文。

请求级认证（`requests.auth`）不是普通列，而是整段 JSON 经 `secret::encrypt_auth_json` / `decrypt_auth_json` 处理：token、password、value、client_secret 逐字段加密，读取时解不开的字段置空但不影响其余字段。`commands/request.rs` 的读写入口必须成对使用，**不要**把 `auth` 当普通 JSON 直接 `serde_json::to_string` 入库。

## 关键上限常量

| 常量 | 位置 | 值 |
|---|---|---|
| `MAX_HISTORY` | `commands/http.rs` | 20 000（超出后按时间裁掉最老记录） |
| `MAX_REQUEST_RECORDS` | `commands/stress.rs` | 50 000（报告逐请求明细） |
| `MAX_ERRORS` | `commands/stress.rs` | 500 |
| `APDEX_T` | `commands/stress.rs` | 500ms |
| `MAX_RT_MS` / `MAX_TTFB_MS` | `commands/stress.rs` | 60 000 / 10 000（直方图上限） |

请求明细入库前会截断文本：错误信息 500 字、请求体 1000 字、响应体 2000 字符。

## 压测引擎要点（`commands/stress.rs`）

- 全局互斥：`AppState.stress_running`（`AtomicBool`），同一时刻只允许一个压测任务
- 停止信号：`AppState.stress_cancel`，任务内定期检查
- 并发模型：`tokio::task::JoinSet` + `mpsc` 汇总指标，指标用 `AtomicU64` 累加
- 进度推送：`app.emit("stress:progress", &progress)`，结构体 `StressProgress` 需实现 `Clone + Serialize`
- 结束后写 `test_reports` / `report_samples` / `report_requests`，分位、状态码分布、错误类型、直方图以 JSON 列存储
- 压测请求自身也会写历史，注意不要绕过 `MAX_HISTORY` 裁剪
- 请求体与手动发送走同一套规则：`body_type == "json"` 时先 `strip_json_comments` 再渲染变量，保证「请求编辑器能发、压测也能发」

## 抓包后端（`capture/` + `commands/capture.rs`）

- **不写库**：流量全部在 `capture/store.rs` 的内存 `VecDeque` 里，上限 5000 条 / 128 MB body，超出淘汰最旧；重启即清空。不要顺手加 `INSERT`，需要留档走 HAR / ZIP 导出。
- 代理跑独立线程 + `current_thread` tokio runtime（线程名 `capture-proxy`）；`Runtime` 不能在 async 上下文 drop，也不要和 Tauri 主 runtime 混用。
- 默认端口 8888，占用时向上扫 10 个；`bind_addr` 默认 `127.0.0.1`，手机抓包显式传 `0.0.0.0`。
- 单条 body 上限 2 MB、完整读取上限 8 MB、图片预览上限 1 MB；SSE 不缓冲；压缩响应先解码。
- CA 文件在 `<数据目录>/certs/`；Windows 一键安装走 `certutil -user -f -addstore Root`，macOS / Linux 只支持导出。
- 系统代理由 `capture/sysproxy.rs` 接管 Windows `HKCU` WinINET，改动前备份到数据目录，停止 / 线程退出 / 下次启动都会还原；非 Windows 返回明确错误。
- 改配置结构（`CaptureConfig` / `CaptureStatus` / `CaptureFlow*`）要同步 `src/types/capture.ts`；新增命令注册到 `lib.rs::generate_handler!`。
- 改录制或重发逻辑后跑 `cargo test --lib capture`（代理转发 + 未录制重发覆盖参数），再用 `npm run tauri dev` 手工冒烟。
- 断点 / 映射 / 弱网在 `capture/intercept.rs`：挂起队列上限 `MAX_PENDING = 32`、`PAUSE_TIMEOUT = 120s`，Map Local 文件上限 `MAX_MAP_FILE = 50MB`，弱网单次最多等 120 秒后返回 504；规则持久化到 `<数据目录>/capture/intercept.json`，改规则后同步 `capture_intercept_state`，解析失败的规则要跳过而不是让整个抓包启动失败。
- 手机 / 微信边界、HTTPS 排错与命令清单见 skill `astroman-capture`。

## 记事本后端（`notes/` + `commands/notes.rs`）

- 领域函数全部在 `notes/mod.rs`，`commands/notes.rs` 只做参数透传与事件广播；新增逻辑先加领域函数与单测，再包命令。
- 提醒只发应用内事件：`reminder::tick` 每 30 秒查 `due_notes`，`mark_fired` 事务写入后再 `emit("notes:reminder")`。**先写库、后通知**，顺序反了会重复弹。
- 暂停 / 免打扰期间整轮 return 且不写库；结束后只补一条聚合提醒。
- 稍后提醒必须写 `snooze_until` 并清 `notified_at`；重复条目完成时顺延下一次 `due_at` / `remind_at`，状态保持 `open`。
- 附件 `attachment::add` 先按 sha256 查重，命中只加链接；删除只删链接，孤儿由每日清理回收（累计超过 32MB 才 VACUUM）。
- 截图识别 `vision::extract` 只接受 `supports_vision = 1` 的服务商；失败保留 `rawText`，原图仍作为附件保存。
- 快速记录窗口必须独立线程创建；全局快捷键注册失败要把 `globalShortcut` 回退为 false 并提示占用。
- 改提醒 / 附件语义后跑 `cargo test --lib notes::`；表结构与 UI 约定见 skill `astroman-notes`。

## 自动化测试执行内核（v1.0.8 新增）

七张表：`test_suites` / `test_cases` / `test_datasets` / `test_runs` / `test_run_cases` / `test_run_assertions` / `test_artifacts`。老库由 `db/mod.rs::migrate()` 逐列补齐，不改存量数据。

- 入口固定两条：`test_runner::create_queued_run(state, suite_id, trigger)` 建 queued 记录并返回 `runId`，`test_runner::run_suite(...)` 后台执行；桌面端与 CLI 都走这里，**不要另写一套执行循环**
- 引擎不依赖 Tauri：进度与用例结果通过 `TestEvent` 回调交给命令层，命令层再 `emit`；CLI 直接把回调打到 stdout
- 运行状态：`queued → running → completed / stopped / error`；用例状态：`passed / failed / skipped / error`。统计写 `test_runs.passed/failed/skipped/errored/duration_ms`
- 快照与日志上限：请求 256KB、响应 1MB、日志 64KB，进度事件 200ms 一次（`PROGRESS_INTERVAL`），别把全量快照塞进事件
- Python 边界常量集中在 `python_exec.rs`：协议 `1.0`、默认超时 60s、输出 1MB、`result.json` 5MB、单附件 10MB、附件合计 100MB、最低 Python 3.9；解释器路径、授权开关与超时由设置页控制
- 与压测互斥：`kind=stress` 用例跑前要等 `stress_running` 释放，`waitTimeoutSec` 控制最长等待；停止运行会在一秒内中断压测
- 产物目录 `<数据目录>/test-reports/<runId>/`（`report.html` / `report.json` / `junit.xml` / `summary.txt` / `logs/` / `files/`），**库里只存相对路径**，切换数据目录后历史仍可读；每套件保留 50 份，启动时清理无主目录
- 改断言、钩子或数据行语义后至少跑 `cargo test --lib test_runner:: test_assert:: test_dataset:: python_exec::`（合计 32 条）

完整流程（预检项、退出码、套件包、AI 22 个测试域工具）见 skill `astroman-automation`。

## 统计聚合

`commands/system.rs` 里两个只读统计命令：

| 命令 | 用途 |
|---|---|
| `workspace_stats` | 项目/集合/接口/环境/历史/报告总数与近 7 天增量 |
| `dashboard_metrics` | 首页大屏：14 天趋势、状态码分布、Top 5 接口、最近 6 份压测报告 |

`dashboard_metrics` 的 SQL 惯例：

- 按本地日期分桶：`date(created_at / 1000, 'unixepoch', 'localtime')`，时间起点用 `chrono::Local` 算本地零点，两边口径必须一致
- SQL 只返回有数据的日期，Rust 侧按 14 天循环补 0，保证图表格子数固定
- 聚合列统一 `COALESCE(..., 0)`；`COUNT/SUM` 读 `i64`、`AVG` 读 `f64`
- 状态码分布用 `SUM(CASE WHEN status_code BETWEEN ... THEN 1 ELSE 0 END)` 一次查出五路
- 报告倒序取 6 条后 `reverse()`，让图表按时间从左到右
- 返回结构 `DashboardMetrics` 在 `models.rs`，前端类型在 `types/models.ts`，两边字段必须同步

详细口径、前端图表约定与 Runner 首页文案见 skill `astroman-dashboard`。

## 模型约定

`models.rs` 里每个结构体：

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Demo {
    #[serde(default)]
    pub new_field: Option<String>,   // 新增可选字段必须加 default，保证老数据/老前端不炸
}
```

- 新增字段一律 `#[serde(default)]`
- 时间统一用毫秒时间戳 `i64`（`util::now_ms()`）
- 前端对应类型在 `src/types/models.ts`，字段名用 camelCase
