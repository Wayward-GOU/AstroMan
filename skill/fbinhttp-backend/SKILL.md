---
name: fbinhttp-backend
description: FbinHttp 的 Rust/Tauri 后端开发规范。改数据库 schema、写命令、调压测引擎、改报告结构、排查 SQLite 迁移报错时使用。触发词：后端、Rust、command、SQLite、schema、migrate、压测引擎、报告字段。
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
| `commands/importer.rs` | Postman/Apifox 数据导入 |
| `commands/curl.rs` / `commands/tools.rs` | cURL 解析、哈希/HMAC、IP 查询 |
| `commands/files.rs` / `commands/storage.rs` | 文本文件读写、数据目录迁移 |
| `commands/settings.rs` | 代理与客户端证书设置 |
| `commands/system.rs` / `commands/winstats.rs` | 应用信息、工作区统计、首页数据大屏聚合（`dashboard_metrics`）、Windows 进程指标 |
| `commands/update.rs` | 版本检查与下载 |
| `commands/ai.rs` | AI 助手「小HI」：服务商、流式对话、会话消息、工具流水（详见 `fbinhttp-ai`） |
| `state.rs` | AppState、连接池、reqwest 客户端（普通/忽略证书） |
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

详细口径、前端图表约定与 Runner 首页文案见 skill `fbinhttp-dashboard`。

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
