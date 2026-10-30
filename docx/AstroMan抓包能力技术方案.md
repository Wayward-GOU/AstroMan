# AstroMan 抓包模块技术方案（Charles 全功能对标）

| 项目 | 内容 |
| --- | --- |
| 文档版本 | v3.0 |
| 目标版本 | v1.1.0（P0）→ v1.2.0（P1）→ v1.3.0（P2）→ v1.4.0（P3） |
| 代码基线 | v1.0.7 + v1.0.8 工具箱（`src/tools/` 92 个工具已合入） |
| 交付形态 | 独立页面 `/capture` + 独立 Rust 模块 `src-tauri/src/capture/` |
| 功能范围 | Charles 官方文档 37 项功能：34 项对等、3 项受限 |
| 总工作量 | 85~95 人日（P0 31.5 / P1 22 / P2 18 / P3 12） |

---

## 0. 文档使用约定

本文档按「可被逐任务执行」的标准编写。执行者必须遵守以下约定。

### 0.1 任务即提交单位

- 第 9 章每个任务（`C-xx`）对应**一次代码提交**，禁止把多个任务混在一个提交里。
- 严格按任务编号顺序执行；`C-05` 依赖 `C-01`，`C-09` 依赖 `C-08`，任务表已标注前置。
- 每个任务完成后必须运行 §0.4 的自检命令，全绿才允许进入下一个任务。

### 0.2 硬性约束

| 约束 | 说明 |
| --- | --- |
| 不新增前端依赖 | 虚拟滚动、饼图、Hex 查看器全部手写；图表复用现有 `src/components/LineChart.vue` 的 SVG 思路 |
| 不改无关文件 | 每个任务列出「涉及文件」，只允许改这些文件；确实需要改其它文件时，在提交说明中单独列出 |
| 后端命令必须注册 | 新增 IPC 命令必须同时加入 `src-tauri/src/lib.rs` 的 `generate_handler!`，否则前端调用报 `command not found` |
| 禁止 `unwrap` / `expect` | 项目 `Cargo.toml` 设了 `panic = "abort"`，handler 内任何 panic 都会导致整个应用退出 |
| 时间单位统一毫秒 | 字段名以 `_ms` 结尾的一律毫秒；`created_at` / `started_at` 用 `now_ms()`（项目已有） |
| JSON 字段 camelCase | Rust 侧结构体统一 `#[serde(rename_all = "camelCase")]` |
| SQL 不写进 handler | 所有 SQL 放 `src-tauri/src/db/capture.rs`，handler 只调用 DAO 函数 |

### 0.3 代码风格基线

- Rust：沿用现有风格（`thiserror` 错误枚举、`parking_lot::Mutex`、`r2d2` 连接池、`log::warn!`）。
- Vue：`<script setup lang="ts">` + Composition API，样式用项目设计令牌（`--fm-surface` / `--fm-border` / `--fm-muted` / `--fm-radius`）。
- API 层：`src/api/index.ts` 里一律 `export const xxx = (args) => invoke<T>('cmd_name', {...})`。

### 0.4 自检命令

```bash
# Rust 编译（每次改完 Rust 必跑）
cd src-tauri && cargo check --message-format short

# 前端类型检查（每次改完 Vue/TS 必跑）
npx vue-tsc --noEmit

# 单元测试（改完纯函数必跑；注意本机需串行，否则偶发 EPERM）
npx vitest run --no-file-parallelism

# 全量构建（每个阶段末跑一次）
npx vite build
```

---

## 1. 范围

### 1.1 交付内容

一个独立页面 `/capture`，实现 Charles 的抓包、改包、模拟、会话管理与导出能力，并与 AstroMan 现有的接口编辑、批量运行、压测、AI 打通。

### 1.2 版本切分

| 版本 | 内容 | 人日 |
| --- | --- | --- |
| v1.1.0 / P0 | 代理内核、CA、系统代理、录制、列表/详情、过滤、HAR 导出、存为接口 | 31.5 |
| v1.2.0 / P1 | 断点、Rewrite、Map、DNS Spoofing、Block、Throttle、访问控制、连接诊断 | 22 |
| v1.3.0 / P2 | WebSocket、mTLS、Repeat Advanced、Chart、Protobuf、四种导出/三种导入、外部代理链 | 18 |
| v1.4.0 / P3 | 端口转发、反向代理、透明代理（macOS）、HTTP/3 识别、Gist、Web 界面、CLI | 12 |

### 1.3 三处受限（设计前提，不得视为缺陷）

| 限制 | 技术原因 | 产品表现 |
| --- | --- | --- |
| HTTP/3 只识别不解密 | QUIC 内建加密，MITM 需实现 QUIC 中间人；浏览器配置代理后默认禁用 QUIC | 列表标记 `h3（未解密）`，其余功能不受影响 |
| 不解析 `.chls` | 私有二进制格式 | 用 HAR / XML 双向互通 |
| Windows 不做透明代理 | 需 NDIS/WFP 驱动与签名 | Windows 走系统代理模式 |

---

## 2. 功能对标矩阵

批次列即交付版本；「实现」列为技术路径。

### 2.1 代理与网络（12 项）

| # | 功能 | 实现 | 批次 |
| --- | --- | --- | --- |
| 1 | HTTP/1.1 抓包 | `hudsucker` 内核 + `hyper` 1.x | P0 |
| 2 | HTTPS 解密 | `rcgen` 按 host 动态签发，`RcgenAuthority` 内部缓存 1024 张 | P0 |
| 3 | SSL Proxying 白名单 | `should_intercept_tls` 读 SNI 判定，未命中走隧道直通 | P0 |
| 4 | HTTP/2 | feature `http2` + ALPN 协商 | P0 |
| 5 | WebSocket 帧 | `WebSocketHandler` + 帧表 | P2 |
| 6 | 系统代理 | Win 注册表 + `InternetSetOptionW`；mac `networksetup` | P0 |
| 7 | 外部代理链 | 自定义 `Connect` 串联上游 HTTP/SOCKS5 | P2 |
| 8 | 访问控制 | `ctx.client_addr` + IP 白名单 | P1 |
| 9 | 端口转发 | 独立 listener + 双向 copy | P3 |
| 10 | 反向代理 | 独立 listener + 固定 upstream | P3 |
| 11 | 透明代理 | 仅 macOS `pf` 锚点 | P3 |
| 12 | HTTP/3 | 读 ALPN 标记，不解密 | P3 |

### 2.2 改包与模拟（11 项）

| # | 功能 | 实现 | 批次 |
| --- | --- | --- | --- |
| 13 | 断点 | `oneshot` 等待点 + 60s 超时放行 | P1 |
| 14 | 断点规则 | location 匹配器 | P1 |
| 15 | Rewrite | 自研 4 类规则引擎，语义见 §6.6 | P1 |
| 16 | Rewrite Sets | 两级表（set / rule）+ 独立启用位 | P1 |
| 17 | Map Local | 读本地文件/目录替换响应体 | P1 |
| 18 | Map Remote | 改 URI 后转发，保留原 Host 可选 | P1 |
| 19 | DNS Spoofing | 自定义 DNS Resolver，只改解析结果不改 SNI | P1 |
| 20 | Block List | 命中直接返回 403 或断开 | P1 |
| 21 | Throttle | 双向令牌桶 + 延迟注入 | P1 |
| 22 | mTLS | 按 host 选 PKCS#12 客户端证书 | P2 |
| 23 | Repeat Advanced | 迭代/并发/延迟，接现有压测引擎 | P2 |

### 2.3 会话与数据（11 项）

| # | 功能 | 实现 | 批次 |
| --- | --- | --- | --- |
| 24 | 会话保存/打开 | 自有 `.astrocap`（zip + ndjson） | P0 |
| 25 | Notes / Summary | `capture_sessions.note` + 聚合查询 | P0 |
| 26 | Structure / Sequence | 同一数据的两种投影 | P0 |
| 27 | Focus / Filter | 前端过滤 + 后端条件查询 | P0 |
| 28 | Chart | 手写 SVG 饼图 | P2 |
| 29 | 详情 7 Tab | 概览/内容/头/Query/Cookie/认证/耗时 | P0 |
| 30 | 9 种查看器 | Text/JSON/XML/HTML/JS/CSS/Image/Hex/Raw | P0 |
| 31 | Protobuf | `prost-reflect` 动态解码 | P2 |
| 32 | 导出 4 格式 | HAR / CSV / Trace / XML | P0 / P2 |
| 33 | 导入 3 格式 | HAR / Trace / XML | P2 |
| 34 | Publish Gist | GitHub API + token | P3 |

### 2.4 平台与体验（3 项）

| # | 功能 | 实现 | 批次 |
| --- | --- | --- | --- |
| 35 | Web Interface | 只读 HTTP 服务，默认绑 127.0.0.1 | P3 |
| 36 | CLI 选项 | `astroman capture --headless --export` | P3 |
| 37 | 工具栏全局开关 | 6 个 toggle，与抽屉状态双向同步 | P0 |

---

## 3. 工程结构

### 3.1 新增与修改文件

| 文件 | 类型 | 职责 | 预估行数 |
| --- | --- | --- | --- |
| `src-tauri/src/capture/mod.rs` | 新增 | `CaptureManager`、状态机、启停、runtime | 260 |
| `src-tauri/src/capture/ca.rs` | 新增 | CA 生成/加载/加密/导出/安装判定 | 220 |
| `src-tauri/src/capture/proxy.rs` | 新增 | 组装 `Proxy`、端口探测、优雅关闭 | 180 |
| `src-tauri/src/capture/handler.rs` | 新增 | `HttpHandler` 实现、管道编排、记录 | 380 |
| `src-tauri/src/capture/rules.rs` | 新增 | Location 匹配、规则快照、热更新 | 220 |
| `src-tauri/src/capture/rewrite.rs` | 新增 | Rewrite 引擎 | 300 |
| `src-tauri/src/capture/breakpoint.rs` | 新增 | 断点队列与 resolve | 200 |
| `src-tauri/src/capture/modifiers.rs` | 新增 | Map Local/Remote、Block、DNS 解析器 | 260 |
| `src-tauri/src/capture/throttle.rs` | 新增 | 令牌桶与延迟注入 | 120 |
| `src-tauri/src/capture/body.rs` | 新增 | body 采集、分级、编码、哈希 | 200 |
| `src-tauri/src/capture/pipeline.rs` | 新增 | 事件通道、批量落库、丢弃计数 | 220 |
| `src-tauri/src/capture/sysproxy.rs` | 新增 | 系统代理读写与还原 | 240 |
| `src-tauri/src/capture/har.rs` | 新增 | HAR 导出/导入 | 260 |
| `src-tauri/src/capture/export.rs` | 新增 | CSV / Trace / XML 导出、XML / Trace 导入 | 300 |
| `src-tauri/src/capture/proto.rs` | 新增 | Protobuf 动态解码 | 160 |
| `src-tauri/src/capture/ws.rs` | 新增 | WebSocket 帧记录 | 150 |
| `src-tauri/src/commands/capture.rs` | 新增 | 37 个 IPC 命令 | 520 |
| `src-tauri/src/db/capture.rs` | 新增 | 全部 SQL 与 DAO | 420 |
| `src-tauri/src/db/schema.sql` | 修改 | 追加 8 张表 | +120 |
| `src-tauri/src/state.rs` | 修改 | `AppState` 挂 `CaptureManager` | +10 |
| `src-tauri/src/lib.rs` | 修改 | 模块声明 + 命令注册 | +40 |
| `src-tauri/Cargo.toml` | 修改 | 新增依赖 | +10 |
| `src/views/Capture.vue` | 新增 | 页面容器 | 260 |
| `src/components/capture/CaptureToolbar.vue` | 新增 | 顶部工具条 | 200 |
| `src/components/capture/CaptureTree.vue` | 新增 | 左侧结构/序列树 | 180 |
| `src/components/capture/CaptureList.vue` | 新增 | 中间虚拟滚动列表 | 320 |
| `src/components/capture/CaptureDetail.vue` | 新增 | 右侧详情 + 7 Tab | 300 |
| `src/components/capture/viewers/*.vue` | 新增 | 9 个查看器（Text/Json/Xml/Html/Code/Image/Hex/Raw/Proto） | 520 |
| `src/components/capture/drawers/*.vue` | 新增 | 7 个抽屉（断点/重写/映射/限速/屏蔽/证书/代理） | 780 |
| `src/components/capture/CaptureGuide.vue` | 新增 | 手机/模拟器装证书引导 | 220 |
| `src/stores/capture.ts` | 新增 | 前端状态 | 240 |
| `src/api/capture.ts` | 新增 | IPC 封装 | 180 |
| `src/types/capture.ts` | 新增 | TS 类型 | 200 |
| `src/router/index.ts` | 修改 | 加 `/capture` | +1 |
| `src/App.vue` | 修改 | 侧边栏导航 + 命令面板条目 | +2 |

### 3.2 依赖变更

`src-tauri/Cargo.toml` 的 `[dependencies]` 追加：

```toml
# 抓包内核
hudsucker = { version = "0.24", default-features = false, features = [
  "http2", "rcgen-ca", "rustls-client", "decoder"
] }
rcgen = "0.14"
rustls = { version = "0.23", default-features = false, features = ["aws_lc_rs", "std", "tls12"] }
rustls-native-certs = "0.8"
tokio = { version = "1", features = [
  "time", "sync", "rt", "net", "io-util", "macros", "rt-multi-thread", "fs"
] }
http-body-util = "0.1"
bytes = "1"
prost-reflect = "0.14"
zip = { version = "2", default-features = false, features = ["deflate"] }
```

`[target.'cfg(windows)'.dependencies]` 追加 `winreg = "0.52"`，并给 `windows-sys` 的 features 追加：

```toml
"Win32_Networking_WinInet"
```

前端 `package.json` **不做任何变更**。

### 3.3 注册点

1. `src-tauri/src/lib.rs`：`mod capture;`，并在 `generate_handler!` 追加全部 `commands::capture::*`。
2. `src-tauri/src/state.rs`：`AppState` 增加字段 `pub capture: crate::capture::CaptureManager`，在 `AppState::new` 中初始化。
3. `src/router/index.ts`：`{ path: '/capture', name: 'capture', component: () => import('@/views/Capture.vue') }`。
4. `src/App.vue` 的 `navs` 数组插入（放在 `stress` 之后）：

```ts
{ key: 'capture', label: '抓包', hint: 'HTTP/HTTPS 录制 · 改包 · 弱网', to: '/capture', icon: ApiOutlined },
```

同时在命令面板的条目里加一条同名跳转。

---

## 4. 全局约定

### 4.1 错误码

Rust 侧返回 `Result<T, String>`，错误字符串统一格式 `E_XXX: 中文描述`，前端按前缀分流。

| 码 | 含义 | 触发点 |
| --- | --- | --- |
| `E_PORT_IN_USE` | 端口被占用 | 绑定 8888~8897 全部失败 |
| `E_CA_MISSING` | CA 未生成 | 启动时读取失败 |
| `E_CA_INSTALL` | 证书安装失败 | 安装命令非零退出 |
| `E_SYSPROXY` | 系统代理设置/还原失败 | 注册表或 networksetup 报错 |
| `E_PERMISSION` | 缺少权限 | macOS 授权被拒 |
| `E_BODY_TOO_LARGE` | body 超过 5MB 未记录 | 记录时 |
| `E_DECODE` | 内容解码失败 | gzip/br 解码 |
| `E_HAR` | HAR 解析失败 | 导入时 |
| `E_BUSY` | 已在运行/未运行 | 重复启停 |

### 4.2 日志

```rust
log::info!(target: "capture", "proxy listening on {}", addr);
log::warn!(target: "capture", "dropped {} events", n);
```

target 固定为 `capture`，便于 `RUST_LOG=capture=debug` 单开。

### 4.3 背压

事件通道容量 8192；`try_send` 失败即 `dropped.fetch_add(1)`，绝不阻塞代理线程。丢弃计数在状态栏展示（与压测 `stress.rs:761` 的策略一致）。

---

## 5. 数据层

### 5.1 表结构（追加到 `src-tauri/src/db/schema.sql`）

```sql
CREATE TABLE IF NOT EXISTS capture_sessions (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    name         TEXT NOT NULL,
    note         TEXT,
    started_at   INTEGER NOT NULL,
    ended_at     INTEGER,
    flow_count   INTEGER DEFAULT 0,
    bytes_total  INTEGER DEFAULT 0,
    dropped      INTEGER DEFAULT 0
);

CREATE TABLE IF NOT EXISTS capture_flows (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id    INTEGER NOT NULL,
    seq           INTEGER NOT NULL,
    started_at    INTEGER NOT NULL,
    method        TEXT NOT NULL,
    scheme        TEXT,
    host          TEXT NOT NULL,
    port          INTEGER,
    path          TEXT NOT NULL,
    status        INTEGER,
    http_version  TEXT,
    content_type  TEXT,
    content_encoding TEXT,
    req_size      INTEGER DEFAULT 0,
    res_size      INTEGER DEFAULT 0,
    duration_ms   INTEGER,
    ttfb_ms       INTEGER,
    timing_json   TEXT,
    client_ip     TEXT,
    is_ws         INTEGER DEFAULT 0,
    blocked       INTEGER DEFAULT 0,
    spoofed       INTEGER DEFAULT 0,
    intercepted   INTEGER DEFAULT 1,
    error         TEXT,
    req_headers   TEXT,
    res_headers   TEXT,
    req_body      TEXT,
    res_body      TEXT,
    req_body_path TEXT,
    res_body_path TEXT,
    body_state    TEXT,
    truncated     INTEGER DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_cap_flow_session ON capture_flows(session_id, seq);
CREATE INDEX IF NOT EXISTS idx_cap_flow_host    ON capture_flows(session_id, host);
CREATE INDEX IF NOT EXISTS idx_cap_flow_status  ON capture_flows(session_id, status);

CREATE TABLE IF NOT EXISTS capture_ws_frames (
    id        INTEGER PRIMARY KEY AUTOINCREMENT,
    flow_id   INTEGER NOT NULL,
    direction TEXT NOT NULL,
    opcode    TEXT,
    payload   TEXT,
    size      INTEGER,
    at        INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_cap_ws_flow ON capture_ws_frames(flow_id, id);

CREATE TABLE IF NOT EXISTS capture_breakpoints (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    enabled    INTEGER DEFAULT 1,
    where_     TEXT NOT NULL,
    location   TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS capture_rewrite_sets (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    name       TEXT NOT NULL,
    enabled    INTEGER DEFAULT 1,
    priority   INTEGER DEFAULT 0
);

CREATE TABLE IF NOT EXISTS capture_rewrite_rules (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    set_id     INTEGER NOT NULL,
    enabled    INTEGER DEFAULT 1,
    rule_type  TEXT NOT NULL,
    where_     TEXT NOT NULL,
    match_json TEXT NOT NULL,
    action_json TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_cap_rule_set ON capture_rewrite_rules(set_id, id);

CREATE TABLE IF NOT EXISTS capture_maps (
    id       INTEGER PRIMARY KEY AUTOINCREMENT,
    kind     TEXT NOT NULL,
    enabled  INTEGER DEFAULT 1,
    location TEXT NOT NULL,
    target   TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS capture_filters (
    id       INTEGER PRIMARY KEY AUTOINCREMENT,
    kind     TEXT NOT NULL,
    enabled  INTEGER DEFAULT 1,
    location TEXT NOT NULL,
    payload  TEXT
);

CREATE TABLE IF NOT EXISTS capture_throttle (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    name        TEXT NOT NULL,
    preset      TEXT,
    down_bps    INTEGER,
    up_bps      INTEGER,
    latency_ms  INTEGER,
    mtu         INTEGER,
    reliability REAL
);
```

`DB_VERSION` **不递增**（新增表不涉及 ALTER 存量表，符合 `db/mod.rs` 现有注释的约定）。

### 5.2 关键 SQL 形态

列表分页（前端虚拟滚动也用这一条，`offset` 由窗口位置算出）：

```sql
SELECT id, seq, started_at, method, host, path, status, duration_ms, res_size, is_ws, blocked
FROM capture_flows
WHERE session_id = ?1
  AND (?2 IS NULL OR host LIKE ?2)
  AND (?3 IS NULL OR method = ?3)
  AND (?4 IS NULL OR status = ?4)
  AND (?5 IS NULL OR (path LIKE ?5 OR host LIKE ?5))
ORDER BY seq ASC
LIMIT ?6 OFFSET ?7;
```

**列表查询禁止 select body 字段**，body 只能由 `capture_flow_body` 按需取。

### 5.3 DAO 函数（`src-tauri/src/db/capture.rs`）

```rust
pub fn session_create(pool: &DbPool, name: &str, ts: i64) -> Result<i64, String>;
pub fn session_end(pool: &DbPool, id: i64, ts: i64) -> Result<(), String>;
pub fn session_list(pool: &DbPool) -> Result<Vec<CaptureSession>, String>;
pub fn session_delete(pool: &DbPool, id: i64) -> Result<(), String>;
pub fn session_touch(pool: &DbPool, id: i64, flows: i64, bytes: i64, dropped: i64) -> Result<(), String>;

pub fn flow_batch_insert(pool: &DbPool, rows: &[CaptureFlowRow]) -> Result<(), String>;
pub fn flow_list(pool: &DbPool, q: &FlowQuery) -> Result<Vec<CaptureFlowBrief>, String>;
pub fn flow_count(pool: &DbPool, session_id: i64) -> Result<i64, String>;
pub fn flow_get(pool: &DbPool, id: i64) -> Result<Option<CaptureFlowRow>, String>;
pub fn flow_bodies(pool: &DbPool, id: i64) -> Result<(Option<String>, Option<String>), String>;
pub fn flow_clear(pool: &DbPool, session_id: i64) -> Result<(), String>;
pub fn flow_purge_over(pool: &DbPool, session_id: i64, keep: i64) -> Result<usize, String>;

pub fn rules_snapshot(pool: &DbPool) -> Result<RuleSnapshot, String>;
```

`flow_batch_insert` 必须在**一个事务**内完成，`rows.len()` 建议 200 一批。

### 5.4 清理策略

- 单会话超过 100,000 条：启动录制与每 10,000 条检查一次，`flow_purge_over` 按 `seq` 升序删多余行，同时删除对应落盘文件；
- 单会话 body 目录超过 2GB：从最旧 flow 开始删文件并把 `body_state` 置 `evicted`；
- 会话删除：先删文件目录 `data_dir/captures/<session_id>/`，再删三张表的行（顺序不能反）。

---

## 6. 代理内核规格

### 6.1 CaptureManager

```rust
// src-tauri/src/capture/mod.rs
pub struct CaptureManager {
    inner: parking_lot::Mutex<Inner>,
    dropped: Arc<AtomicU64>,
}

struct Inner {
    runtime: Option<tokio::runtime::Runtime>,
    shutdown: Option<oneshot::Sender<()>>,
    state: CaptureState,
    port: u16,
    session_id: Option<i64>,
    started_at: i64,
    config: CaptureConfig,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CaptureState { Idle, Starting, Running, Stopping, Error }

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureStatus {
    pub state: CaptureState,
    pub port: u16,
    pub session_id: Option<i64>,
    pub started_at: i64,
    pub flow_count: i64,
    pub dropped: u64,
    pub ca_installed: bool,
    pub sysproxy_on: bool,
}
```

状态迁移（只允许这些边，其它调用直接返回 `E_BUSY`）：

```
Idle ──start──▶ Starting ──ok──▶ Running ──stop──▶ Stopping ──ok──▶ Idle
                    └──err──▶ Error ──start──▶ Starting
```

`start()` 的固定顺序（顺序不能变，否则会留下脏状态）：

1. 校验 CA 存在，否则 `E_CA_MISSING`；
2. 建会话（`session_create`），写入 `session_id`；
3. 建独立 runtime（`worker_threads(2)`，线程名 `astroman-capture`）；
4. `runtime.block_on(proxy::bind(port))` 拿到真实端口（端口冲突时 8888→8897 依次重试，全失败返回 `E_PORT_IN_USE`）；
5. 加载规则快照；
6. `spawn(proxy.start())`；
7. 设置系统代理（`sysproxy::apply`），失败只告警不回滚；
8. 状态置 `Running`。

`stop()`：

1. 状态置 `Stopping`；
2. `sysproxy::restore()`（必须最先做，保证用户网络立刻恢复）；
3. `shutdown.send(())`，`runtime.shutdown_timeout(Duration::from_secs(3))`；
4. `session_end` + `session_touch`；
5. 状态置 `Idle`。

### 6.2 CA 模块

```rust
pub struct CaMaterial {
    pub cert_pem: String,
    pub key_pem: String,   // 加密存储，落盘前 AES-GCM
    pub fingerprint_sha256: String,
    pub created_at: i64,
    pub expires_at: i64,
}
```

生成（`rcgen` 0.14）：

```rust
let mut params = rcgen::CertificateParams::new(Vec::<String>::new())
    .map_err(|e| format!("E_CA_MISSING: {e}"))?;
params.distinguished_name.push(rcgen::DnType::CommonName, "AstroMan Root CA");
params.distinguished_name.push(rcgen::DnType::OrganizationName, "AstroMan");
params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
params.key_usages = vec![
    rcgen::KeyUsagePurpose::KeyCertSign,
    rcgen::KeyUsagePurpose::CrlSign,
    rcgen::KeyUsagePurpose::DigitalSignature,
];
params.validity = rcgen::Validity::from_now(std::time::Duration::days(3650));
let key = rcgen::KeyPair::generate().map_err(err)?;
let cert = params.self_signed(&key).map_err(err)?;
```

落盘位置：`data_dir/certs/astroman-ca.crt`、`astroman-ca.key`。

- `.key` 用现有 `aes-gcm` 加密（复用 `commands/ai.rs` 的密钥派生方式），文件权限 0600（Unix `std::fs::set_permissions`，Windows 用 `icacls` 收紧到当前用户）；
- 指纹用 `sha2` 对 DER 求 SHA-256，十六进制大写冒号分隔（与 Windows 证书管理器显示一致，便于用户核对）。

`RcgenAuthority` 构造：

```rust
let key_pair = rcgen::KeyPair::from_pem(&key_pem)?;
let ca_cert = rcgen::CertificateParams::from_ca_cert_pem(&cert_pem)?.self_signed(&key_pair)?;
let authority = hudsucker::certificate_authority::RcgenAuthority::new(key_pair, ca_cert, 1024);
```

> 若该构造函数签名与 0.24 文档不一致，以 `cargo doc -p hudsucker --open` 为准，先写最小示例跑通再接入。

### 6.3 代理组装

自己 bind `TcpListener` 才能拿到真实端口并做端口探测：

```rust
pub async fn bind(preferred: u16) -> Result<(tokio::net::TcpListener, u16), String> {
    for port in preferred..preferred.saturating_add(10) {
        if let Ok(l) = tokio::net::TcpListener::bind(("127.0.0.1", port)).await {
            let actual = l.local_addr().map_err(err)?.port();
            return Ok((l, actual));
        }
    }
    Err("E_PORT_IN_USE: 8888~8897 均被占用".into())
}

let proxy = Proxy::builder()
    .with_listener(listener)
    .with_ca(authority)
    .with_rustls_connector(hudsucker::rustls::crypto::aws_lc_rs::default_provider())
    .with_http_handler(handler)
    .with_websocket_handler(ws_handler)
    .with_graceful_shutdown(async move { let _ = rx.await; })
    .build()
    .map_err(|e| format!("E_PROXY: {e}"))?;

tokio::spawn(async move {
    if let Err(e) = proxy.start().await {
        log::error!(target: "capture", "proxy stopped: {e}");
    }
});
```

### 6.4 规则快照与 Location 匹配

规则在启动时与每次保存后重新从库加载成只读快照，热替换用 `ArcSwap` 风格的 `parking_lot::RwLock<Arc<Snapshot>>`（不引入新依赖）。

**Location 语法**（与 Charles 的 location 概念对齐）：

```json
{ "protocol": "https", "host": "*.example.com", "port": 443, "path": "/api/*" }
```

编译规则：

1. 空字段 = 通配；
2. 转义正则元字符，再把 `*` 换成 `.*`、`?` 换成 `.`；
3. `host` 与 `protocol` 不区分大小写，`path` 区分；
4. 端口为空 = 任意端口。

```rust
pub struct LocationMatcher { protocol: Option<Regex>, host: Option<Regex>, port: Option<u16>, path: Option<Regex> }

impl LocationMatcher {
    pub fn compile(raw: &str) -> Result<Self, String>;   // 解析 JSON 并编译
    pub fn matches(&self, scheme: &str, host: &str, port: u16, path: &str) -> bool;
}
```

### 6.5 管道顺序（`handler.rs` 唯一入口）

请求方向：

```
1. 访问控制（IP 白名单）── 不通过 → 444 断开
2. Block List        ── 命中 → 返回 403 / 断开
3. Map Remote        ── 命中 → 改 scheme/host/port/path
4. Rewrite(request)  ── 按规则改写 header / url / query / body
5. Breakpoint(req)   ── 命中 → 挂起等 UI 决策
6. 生成 flow_id、记录开始时间
7. 交给上游（connector 内含 DNS Spoofing）
```

响应方向：

```
8. Rewrite(response)
9. Map Local         ── 命中 → body 换成本地文件
10. Breakpoint(res)  ── 命中 → 挂起
11. Throttle         ── 按速率 sleep
12. 采集 body（分级）→ 记录 ttfb / duration / size
13. pipeline 投递
```

顺序不可调换：Map 在 Rewrite 之前会让「改写过的 URL」被映射掉；Throttle 必须在采集之后，否则记录的耗时会被自己注入的延迟污染。

### 6.6 Rewrite 引擎

数据结构：

```rust
pub struct RewriteRule {
    pub id: i64,
    pub set_priority: i64,
    pub enabled: bool,
    pub rule_type: RuleType,          // Header | Url | Query | Body
    pub scope: Scope,                 // Request | Response | Both
    pub m: MatchSpec,
    pub a: ActionSpec,
}

pub struct MatchSpec {
    pub name: String,          // header 名 / query key；Url 与 Body 规则忽略
    pub value: String,
    pub regex: bool,
    pub whole: bool,           // Match whole value
    pub replace_all: bool,
}

pub struct ActionSpec {
    pub mode: ActionMode,      // Add | Modify | Remove | Replace
    pub name: String,
    pub value: String,
}
```

**匹配语义（必须逐条实现，这是与 Charles 对齐的关键）**：

| 场景 | 规则 |
| --- | --- |
| `name` | 精确匹配；`regex=true` 时按正则做部分匹配；两种模式都**不区分大小写** |
| `value` | 默认**部分匹配**；`whole=true` 时精确匹配 |
| 大小写 | Header / Query 规则：忽略大小写；URL / Body 规则：**区分大小写** |
| 替换 | `replace_all=true` 全替换，否则只替换首个；`regex=true` 时替换串支持 `$1`~`$9` |
| 空 match | name 与 value 都为空 = 匹配全部 |

**四类规则的应用算法**：

```rust
// Header
match mode {
    Add    => headers.append(name, value),                 // 不覆盖同名
    Modify => headers.insert(name, value),                 // 覆盖首个同名
    Remove => headers.remove(name),
    Replace=> { /* 对每个同名 header 的 value 做查找替换 */ }
}

// Query：解析成 Vec<(String, String)> 后按 key 处理，最后重新拼接
// URL：对完整 URL 字符串做查找替换（区分大小写，first/all）
// Body：仅当 content-type 属于文本类且 body 已解码且 ≤ 2MB 时执行
//       文本类 = text/*、application/json、application/xml、application/javascript、x-www-form-urlencoded
```

**执行顺序**：`set.priority` 升序 → `rule.id` 升序，前一条的输出作为后一条的输入（链式）。

**幂等**：单次请求内每条规则只应用一次，不做循环触发。

**失败**：正则编译失败时该规则跳过并记一次 `log::warn`，不影响其它规则。

### 6.7 Breakpoint

```rust
pub struct BreakpointHub {
    pending: parking_lot::Mutex<HashMap<i64, PendingEntry>>,
}

struct PendingEntry {
    tx: oneshot::Sender<BpAction>,
    created_at: i64,
}

pub enum BpAction {
    Execute(Option<BpPatch>),   // None 表示原样放行
    Abort,
}
```

流程：

1. `handle_request` 命中规则后注册 `flow_id`，通过 `app.emit("capture://breakpoint", payload)` 通知前端；
2. `tokio::time::timeout(Duration::from_secs(60), rx).await`；
3. 超时 → 原样放行，并发 `capture://breakpoint-timeout` 事件；
4. `Abort` → 返回 502，body 写 `{"error":"aborted by breakpoint"}`；
5. `Execute(patch)` → 把 patch 应用到请求（header / method / url / body 四个字段）。

并发要求：多个 flow 可同时挂起；`pending` 用 `parking_lot::Mutex`，临界区只做插入/移除，不 `.await`。

前端 resolve 命令：

```
capture_breakpoint_resolve({ flowId, action: "execute" | "abort", patch?: { method?, url?, headers?, body? } })
```

### 6.8 Map Local / Map Remote

```rust
// Map Local：替换响应
pub fn map_local(target: &str, res: &mut Response<Body>) -> Result<(), String> {
    // target 为文件：读全部字节；为目录：拼 path 的 basename
    // Content-Type 按扩展名推断，覆盖原值
    // Content-Length 必须同步改为新长度，否则浏览器挂起
}

// Map Remote：替换请求目标
pub struct MapRemote { pub from: LocationMatcher, pub to: UrlTemplate }
// to 形如 "https://staging.example.com/api"；命中后拼接原 path 的剩余部分（可选开关）
```

### 6.9 DNS Spoofing

只改解析结果，**不改 SNI 与 Host**，否则证书校验与虚拟主机都会错。

实现方式：自定义 `Resolve` 服务，喂给 `HttpConnector::new_with_resolver`，再包一层 `hyper_rustls::HttpsConnector`，最后 `with_http_connector` 传入。

```rust
#[derive(Clone)]
struct SpoofResolver { map: Arc<HashMap<String, IpAddr>> }

impl tower::Service<hyper_util::client::legacy::connect::dns::Name> for SpoofResolver {
    type Response = std::vec::IntoIter<SocketAddr>;
    type Error = std::io::Error;
    type Future = std::future::Ready<Result<Self::Response, Self::Error>>;
    // 命中 map 返回 spoofed IP，未命中回落 GaiResolver
}
```

> `tower` 与 `hyper-util` 的 feature 要开全（`client-legacy`、`tokio`）。若编译时提示 `Resolve` 未实现，先 `cargo tree -i hyper-util` 确认只有一份版本。

### 6.10 Block List

命中后的两种策略：

- `respond`：直接返回 `403`，body 固定文本，并标记 `blocked=1`；
- `drop`：返回 `444`（hyper 允许），连接随后关闭。

Block 判定在 Map/Rewrite 之前，保证「屏蔽」语义优先。

### 6.11 Throttle

```rust
pub struct Throttle {
    pub down_bps: u64,
    pub up_bps: u64,
    pub latency_ms: u64,
    pub mtu: usize,
    pub reliability: f64,   // 0.0~1.0，模拟丢包（仅影响延迟抖动，不真丢包）
}
```

P1 实现（可接受且可测）：
- 请求方向在转发前 `sleep(len / up_bps)`；
- 响应方向在返回前 `sleep(len / down_bps + latency_ms)`；
- 长度取 `Content-Length`，缺失时用已采集字节数；
- 加上 ±10% 抖动（`rand` 已在依赖中）。

P2 优化（流式）：把响应 body 包成「每 16KB 睡一次」的流，用 `Body::from(StreamBody)` 实现；改动点只在 `throttle.rs`，接口不变。

预设档位见附录 B。

### 6.12 body 采集与分级

```rust
pub const MAX_INLINE: usize = 64 * 1024;
pub const MAX_FILE: usize = 5 * 1024 * 1024;
```

决策表：

| 条件 | 行为 | `body_state` |
| --- | --- | --- |
| 有 `Content-Length` 且 ≤ 64KB | 收集 → 入库 | `inline` |
| 有 `Content-Length` 且 ≤ 5MB | 收集 → 落盘 `captures/<sid>/<flow>.<ext>` | `file` |
| 有 `Content-Length` 且 > 5MB | **不消费 body**，只记录长度 | `skipped_size` |
| 无 `Content-Length`（chunked/SSE） | **不消费 body**，直通 | `skipped_chunked` |
| 内容编码为 gzip/br/zstd | 用 `hudsucker::decode_response` 解码后再判定 | 同上 |

> 关键约束：只有当决定记录时才 `collect()`；一旦 collect 就必须把 `Bytes` 重新塞回 `Body::from(bytes)` 转发，否则客户端拿到空响应。

解码失败不影响转发：记 `E_DECODE` 到 `error` 字段，body 按原始字节记录（`content_encoding` 保留）。

哈希：`sha256` 前 8 字节十六进制，存进 `timing_json` 的 `bodyHash`，供回放 diff 用。

### 6.13 记录与批量落库

```rust
pub struct FlowEvent {
    pub row: CaptureFlowRow,          // 不含 body
    pub req_body: Option<Bytes>,
    pub res_body: Option<Bytes>,
}
```

- `pipeline.rs` 起一个常驻任务，`mpsc::Receiver<FlowEvent>` 每 200 条或 500ms 触发一次 flush；
- flush 时先落 body 文件（异步），再 `flow_batch_insert`；
- 每 flush 完向 `session_touch` 更新计数（每 10 次 flush 更新一回，避免写放大）；
- 丢弃计数在此处汇总。

### 6.14 系统代理

**Windows（免提权）**

```rust
use winreg::enums::*;
let hkcu = winreg::RegKey::predef(HKEY_CURRENT_USER);
let path = r"Software\Microsoft\Windows\CurrentVersion\Internet Settings";
let key = hkcu.open_subkey_with_flags(path, KEY_READ | KEY_WRITE)?;

// 1) 备份原值到 capture-proxy.lock（JSON：proxy_enable, proxy_server, proxy_override）
// 2) 写入
key.set_value("ProxyEnable", &1u32)?;
key.set_value("ProxyServer", &format!("127.0.0.1:{port}"))?;
key.set_value("ProxyOverride", &"<local>")?;

// 3) 通知系统刷新（39 = SETTINGS_CHANGED，37 = REFRESH）
unsafe {
    InternetSetOptionW(0, 39, std::ptr::null_mut(), 0);
    InternetSetOptionW(0, 37, std::ptr::null_mut(), 0);
}
```

**macOS（需要管理员）**

```bash
# 1) 列出网络服务，取第一个处于 Connected 的（通常 Wi-Fi / Ethernet）
networksetup -listallnetworkservices
# 2) 设置（web + secure 都要设）
networksetup -setwebproxy "$SERVICE" 127.0.0.1 8888
networksetup -setsecurewebproxy "$SERVICE" 127.0.0.1 8888
# 3) 关闭
networksetup -setwebproxystate "$SERVICE" off
networksetup -setsecurewebproxystate "$SERVICE" off
```

提权执行：`osascript -e 'do shell script "<cmd>" with administrator privileges'`，用户取消时返回 `E_PERMISSION`。

**锁文件**：`data_dir/capture-proxy.lock`，内容为备份 JSON + 进程 PID。应用启动时检查：

- 文件存在且 PID 不在运行 → 弹窗「检测到上次未还原的系统代理，是否恢复？」；
- 文件存在且 PID 正在运行 → 视为另一个实例占用，拒绝启动抓包。

**自动还原**：录制超过 4 小时，前端弹一次确认，不操作则不强制。

### 6.15 证书安装

| 平台 | 安装 | 卸载 | 提权 |
| --- | --- | --- | --- |
| Windows | `certutil -user -addstore Root astroman-ca.crt` | `certutil -user -delstore Root <SHA1>` | 否 |
| macOS | `security add-trusted-cert -r trustRoot -k ~/Library/Keychains/login.keychain-db astroman-ca.crt` | `security delete-certificate -c "AstroMan Root CA" ~/Library/Keychains/login.keychain-db` | 需钥匙串授权 |

判定是否已安装：

- Windows：`certutil -user -store Root <SHA1>` 退出码为 0；
- macOS：`security find-certificate -c "AstroMan Root CA" ~/Library/Keychains/login.keychain-db` 退出码为 0。

子进程统一用 `std::process::Command`，捕获 stdout/stderr，失败时把 stderr 原文带进错误信息（用户排障需要）。

### 6.16 handle_error → 连接诊断

```rust
fn handle_error(&mut self, ctx: &HttpContext, err: Error) -> impl Future<Output = Response<Body>> {
    // 记录一条 error flow：method=CONNECT 或原始 URI 不可得时用 host 占位
    // 按 err 文本分类：
    //   含 "certificate" / "UnknownIssuer" → "目标可能启用了证书固定（pinning），或上游证书不被信任"
    //   含 "dns" / "failed to lookup" → "域名解析失败，检查网络或 DNS Spoofing 配置"
    //   含 "timeout" → "连接超时"
    // 统一返回 502，body 为 JSON：{ error, hint }
}
```

分类结果同时写进 `capture_flows.error`，前端详情页顶部用黄色提示条展示 `hint`。

---

## 7. IPC 规格

### 7.1 通用 TS 类型（`src/types/capture.ts`）

```ts
export type CaptureState = 'idle' | 'starting' | 'running' | 'stopping' | 'error'

export interface CaptureStatus {
  state: CaptureState
  port: number
  sessionId: number | null
  startedAt: number
  flowCount: number
  dropped: number
  caInstalled: boolean
  sysproxyOn: boolean
}

export interface CaptureFlowBrief {
  id: number; seq: number; startedAt: number
  method: string; host: string; path: string
  status: number | null; durationMs: number | null
  resSize: number; isWs: boolean; blocked: boolean
}

export interface CaptureFlowDetail extends CaptureFlowBrief {
  scheme: string; port: number; httpVersion: string
  contentType: string; contentEncoding: string
  reqSize: number; ttfbMs: number | null
  timingJson: string; clientIp: string; spoofed: boolean
  intercepted: boolean; error: string | null
  reqHeaders: string; resHeaders: string
  bodyState: 'inline' | 'file' | 'skipped_size' | 'skipped_chunked' | 'evicted'
  truncated: boolean
}
```

### 7.2 命令明细

所有命令返回 `Result<T, String>`；错误字符串见 §4.1。

| 命令 | 入参 | 出参 |
| --- | --- | --- |
| `capture_start` | `{ port?: number, sslHosts?: string[], ignorePatterns?: string[] }` | `CaptureStatus` |
| `capture_stop` | — | `void` |
| `capture_status` | — | `CaptureStatus` |
| `capture_settings_save` | `{ port, sslHosts, ignorePatterns, captureStatic }` | `void` |
| `capture_ca_info` | — | `{ exists, fingerprint, createdAt, expiresAt, installed }` |
| `capture_ca_generate` | `{ force?: boolean }` | `{ fingerprint, expiresAt }` |
| `capture_ca_export` | `{ path: string }` | `void`（导出 .crt 供手机安装） |
| `capture_ca_install` | — | `{ installed: boolean }` |
| `capture_ca_uninstall` | — | `{ installed: boolean }` |
| `capture_sysproxy_get` | — | `{ enabled, server, backedUp }` |
| `capture_sysproxy_set` | `{ on: boolean }` | `{ enabled, server }` |
| `capture_sysproxy_restore` | — | `void` |
| `capture_session_list` | — | `CaptureSession[]` |
| `capture_session_create` | `{ name }` | `{ id }` |
| `capture_session_rename` | `{ id, name, note? }` | `void` |
| `capture_session_delete` | `{ id }` | `void` |
| `capture_flow_list` | `{ sessionId, host?, method?, status?, keyword?, offset, limit }` | `{ total, items: CaptureFlowBrief[] }` |
| `capture_flow_get` | `{ id }` | `CaptureFlowDetail` |
| `capture_flow_body` | `{ id, side: 'req' 或 'res' }` | `{ data: string, encoding: 'text' 或 'base64', truncated }` |
| `capture_flow_clear` | `{ sessionId }` | `void` |
| `capture_flow_to_request` | `{ flowId, projectId, collectionId }` | `{ requestId: number }` |
| `capture_flow_curl` | `{ flowId }` | `{ curl: string }` |
| `capture_breakpoint_list` / `_save` / `_delete` | `{ id?, enabled?, where?, location? }` | 列表 / `{ id }` / `void` |
| `capture_breakpoint_pending` | — | `BpPending[]` |
| `capture_breakpoint_resolve` | `{ flowId, action, patch? }` | `void` |
| `capture_rewrite_set_save` | `{ id?, name, enabled, priority }` | `{ id }` |
| `capture_rewrite_rule_save` / `_delete` | `{ ... }` | `{ id }` / `void` |
| `capture_rewrite_list` | — | `{ sets, rules }` |
| `capture_map_save` / `_delete` / `_list` | `{ id?, kind, enabled, location, target }` | `{ id }` / `void` / 列表 |
| `capture_filter_save` / `_delete` / `_list` | `{ id?, kind, enabled, location, payload? }` | 同上 |
| `capture_throttle_list` | — | `ThrottleProfile[]` |
| `capture_throttle_apply` | `{ id: number 或 null }` | `void`（null = 关闭） |
| `capture_export_har` | `{ sessionId, path, redact?: boolean }` | `{ count }` |
| `capture_export_csv` / `_trace` / `_xml` | 同上 | `{ count }` |
| `capture_import_har` | `{ path }` | `{ imported: number, sessionId }` |
| `capture_gist_publish` | `{ sessionId, token, public? }` | `{ url }` |

事件（后端 → 前端）：

| 事件名 | 载荷 | 频率 |
| --- | --- | --- |
| `capture://flows` | `{ items: CaptureFlowBrief[] }` | 200ms 一批 |
| `capture://status` | `CaptureStatus` | 状态变更时 |
| `capture://breakpoint` | `{ flowId, direction, method, url, headers, bodyPreview }` | 命中时 |
| `capture://breakpoint-timeout` | `{ flowId }` | 超时时 |
| `capture://dropped` | `{ total }` | 每 5s |

---

## 8. 前端规格

### 8.1 页面结构

`src/views/Capture.vue` 只做容器与布局，逻辑全部下沉到 store 与子组件：

```
Capture.vue
├─ CaptureToolbar.vue        顶部：录制按钮 / 会话 / 过滤 / Focus / 六个开关 / 导出
├─ 中部 flex
│  ├─ CaptureTree.vue        左：结构树（host 分组）或序列视图
│  ├─ CaptureList.vue        中：虚拟滚动表格
│  └─ CaptureDetail.vue      右：7 Tab 详情
└─ 底部状态栏（录制条数 / 丢弃 / 磁盘 / 端口）
```

抽屉组件挂在 `Capture.vue` 根部，用 `a-drawer` 承载（Ant Design Vue 4 已在用）。

### 8.2 store（`src/stores/capture.ts`）

```ts
interface CaptureStoreState {
  status: CaptureStatus
  sessions: CaptureSession[]
  activeSessionId: number | null
  flows: CaptureFlowBrief[]        // 上限 20000，超出丢弃头部
  selectedId: number | null
  filter: { host: string | null; method: string | null; status: number | null; keyword: string }
  sslHosts: string[]
  ignorePatterns: string[]
  breakpoints: BpRule[]
  rewrites: { sets: RewriteSet[]; rules: RewriteRule[] }
  maps: MapRule[]
  filters: FilterRule[]
  throttle: { profiles: ThrottleProfile[]; activeId: number | null }
  drawer: null | 'breakpoint' | 'rewrite' | 'map' | 'throttle' | 'block' | 'cert' | 'proxy'
}
```

actions：`refreshStatus` / `start` / `stop` / `loadSessions` / `loadFlows` / `appendFlows` / `select` / `saveRules`。

`appendFlows` 必须在**同一 tick 内合并**（`flows.push(...items)` 后再截断），避免每 200ms 触发整表 diff。

### 8.3 虚拟滚动（手写）

不引入依赖，行高固定 28px：

```ts
const ROW_H = 28
const overscan = 10
const scrollTop = ref(0)
const viewportH = ref(600)

const start = computed(() => Math.max(0, Math.floor(scrollTop.value / ROW_H) - overscan))
const end = computed(() => Math.min(flows.value.length, Math.ceil((scrollTop.value + viewportH.value) / ROW_H) + overscan))
const visible = computed(() => flows.value.slice(start.value, end.value))
const spacerStyle = computed(() => ({
  height: `${flows.value.length * ROW_H}px`,
  transform: `translateY(${start.value * ROW_H}px)`,
}))
```

模板：外层固定高度 `overflow:auto`，内层 spacer 撑总高，内容绝对定位平移到 `start * ROW_H`。

**自动跟随**：仅当 `scrollTop + viewportH >= totalH - 40` 时，新数据到达后 `scrollTop = totalH`；用户向上翻阅时不打扰。

### 8.4 增量推送

```ts
const channel = new Channel<{ items: CaptureFlowBrief[] }>()
channel.onmessage = (e) => store.appendFlows(e.items)
await invoke('capture_subscribe', { onEvent: channel })
```

`capture_subscribe` 内部把 channel 存进 `CaptureManager`，flush 时 `channel.send(...)`。

### 8.5 九种查看器

| 查看器 | 判定条件 | 实现要点 |
| --- | --- | --- |
| JSON | `content-type` 含 json 且能 `JSON.parse` | 复用 `JsonTree.vue`；解析失败自动降级 Text |
| XML | 含 xml | 简单缩进格式化（自写 40 行），不引入解析器 |
| HTML | 含 html | 只读展示源码 + 「在新窗口预览」按钮（`srcdoc` + `sandbox`） |
| CSS / JS | MIME 命中 | 复用 `CodeArea` 只读模式 |
| Text | `text/*` | 原样，超过 200KB 截断并提示 |
| Image | `image/*` | base64 → `data:` URL，超过 2MB 提示「过大，需另存」 |
| Hex | 二进制 | 每行 16 字节 + 偏移 + ASCII 列，虚拟滚动 |
| Raw | 兜底 | 原始字节转 UTF-8（`TextDecoder('utf-8', {fatal:false})`） |
| Proto | 有 descriptor | P2 实现 |

统一入口：`viewers/index.ts` 导出 `pickViewer(contentType, bodyState)`。

### 8.6 详情 7 个 Tab

| Tab | 数据来源 | 展示 |
| --- | --- | --- |
| 概览 | `flow_get` | URL 拆分、状态色块、耗时条（ttfb / 剩余）、大小、客户端 IP、是否解密 |
| 内容 | `flow_body` | 九种查看器 + 搜索 + 另存 |
| 头 | `flow_get` | 两列表格，`复制为 curl` |
| Query | 解析 `path` | 解码后的键值表 |
| Cookie | 解析 req/res headers | 两个表格，Set-Cookie 展开属性 |
| 认证 | `Authorization` | Basic 解码、Bearer 解 JWT（复用 `src/tools/jwtverify.ts`） |
| 耗时 | `timing_json` | 横向瀑布条 |

### 8.7 七个抽屉

每个抽屉的结构统一为：`标题 + 启用开关 + 规则列表 + 新增/编辑表单`。表单字段与 §5.1 的列一一对应，保存即调 `capture_*_save`，保存后 store 重新拉取规则并热替换（下次请求生效，无需重启代理）。

### 8.8 样式与交互

- 全部用现有令牌：`--fm-surface` / `--fm-border` / `--fm-muted` / `--fm-radius`；
- 状态色：2xx 绿、3xx 蓝、4xx 橙、5xx 红、未解密灰；
- 列表选中行用 `--fm-accent` 左边框 + 浅底；
- 快捷键：`Space` 启停、`Ctrl/⌘+F` 聚焦过滤、`Ctrl/⌘+L` 清空、`Ctrl/⌘+E` 导出、`↑/↓` 移动选中、`Esc` 关抽屉。

---

## 9. 任务清单

格式：**目标 / 涉及文件 / 实现要点 / 验收 / 前置**。验收全部可手工复现。

### 9.1 P0（v1.1.0，31.5 人日）

#### C-01 代理骨架（3 人日，无前置）

- 目标：能起一个空代理，浏览器设代理后能访问 HTTP 站点并原样转发。
- 文件：`capture/{mod,proxy}.rs`、`Cargo.toml`、`state.rs`、`lib.rs`
- 要点：独立 runtime、端口 8888~8897 探测、状态机、`NoopHandler` 先跑通再换真 handler。
- 验收：`capture_start` 返回 `running` 与真实端口；`curl -x 127.0.0.1:8888 http://example.com` 返回 200。

#### C-02 CA 生成与存储（2 人日，依赖 C-01）

- 文件：`capture/ca.rs`
- 要点：rcgen 生成、AES-GCM 加密私钥、0600 权限、SHA-256 指纹。
- 验收：`capture_ca_generate` 后再调 `capture_ca_info` 指纹一致；重启应用指纹不变。

#### C-03 证书安装/卸载（2.5 人日，依赖 C-02）

- 文件：`capture/ca.rs`、`commands/capture.rs`
- 要点：三平台命令、退出码判定、stderr 透传、已装检测。
- 验收：Windows 装完在「受信任的根证书颁发机构」可见；卸载后消失；macOS 装完 Chrome 无证书警告。

#### C-04 系统代理（3 人日，依赖 C-01）

- 文件：`capture/sysproxy.rs`
- 要点：备份 → 写入 → 刷新 → 还原；lock 文件；崩溃检测。
- 验收：开启后系统设置里代理指向 127.0.0.1:端口；停止后恢复为原值（原值为空则恢复为空）。

#### C-05 录制管道（3 人日，依赖 C-01/C-02）

- 文件：`capture/{handler,pipeline}.rs`、`db/capture.rs`
- 要点：`HttpHandler` 实现、`should_intercept_tls` 白名单、事件通道、批量 flush。
- 验收：访问 HTTPS 站点能在库中看到 method/host/path/status/duration；HTTPS 明文可读。

#### C-06 body 分级（1.5 人日，依赖 C-05）

- 文件：`capture/body.rs`
- 要点：§6.12 决策表；`collect` 后必须 `Body::from(bytes)` 回填。
- 验收：64KB 以下 body 入库；1MB body 落盘且详情可读；6MB 响应 `body_state=skipped_size` 且浏览器仍能完整接收。

#### C-07 表与清理（1.5 人日，可与 C-05 并行）

- 文件：`db/schema.sql`、`db/capture.rs`
- 要点：8 张表、索引、保留策略。
- 验收：重启应用表结构存在；`capture_flow_clear` 后列表为空且文件目录被清空。

#### C-08 IPC 命令（2 人日，依赖 C-05/C-07）

- 文件：`commands/capture.rs`、`lib.rs`
- 要点：12 个 P0 命令 + 事件订阅命令；参数校验；错误码。
- 验收：前端 devtools 里 `invoke('capture_status')` 返回结构正确。

#### C-09 页面骨架（5 人日，依赖 C-08）

- 文件：`views/Capture.vue`、`components/capture/{CaptureToolbar,CaptureTree,CaptureList}.vue`、`stores/capture.ts`、`api/capture.ts`
- 要点：三栏布局、虚拟滚动、自动跟随、200ms 合批。
- 验收：灌入 10000 条假数据后滚动 60fps；新流量到达时自动滚到底部。

#### C-10 详情（2.5 人日，依赖 C-09）

- 文件：`components/capture/CaptureDetail.vue`、`viewers/*`
- 要点：7 Tab + 9 查看器；大 body 截断提示。
- 验收：JSON 请求展示树形、图片显示缩略图、二进制显示 Hex。

#### C-11 过滤与白名单（2 人日，依赖 C-09）

- 文件：`components/capture/CaptureToolbar.vue`、`capture/rules.rs`
- 要点：host/method/status/关键字过滤；SSL 白名单通配；静态资源忽略。
- 验收：只填 `api.` 时列表只剩该域名；未在白名单的 HTTPS 站点为「隧道直通（未解密）」。

#### C-12 HAR 导出与存为接口（2 人日，依赖 C-10）

- 文件：`capture/har.rs`、`commands/capture.rs`
- 要点：HAR 1.2 字段映射（附录 A）；Cookie/Token 抽变量。
- 验收：导出的 HAR 能被 Chrome DevTools 导入；存为接口后请求编辑器发送成功。

#### C-13 引导页（1.5 人日，依赖 C-03）

- 文件：`components/capture/CaptureGuide.vue`
- 要点：本机 IP（复用 `netinfo.rs`）、二维码、iOS/Android 分步图文。
- 验收：手机扫码能下载 cer；iOS 按图操作后能抓到 HTTPS。

### 9.2 P1（v1.2.0，22 人日）

| 编号 | 任务 | 人日 | 前置 | 验收 |
| --- | --- | --- | --- | --- |
| C-14 | Breakpoint 引擎 + 队列 + 编辑面板 | 5 | C-10 | 命中暂停、改 Header 后放行生效、60s 超时自动放行 |
| C-15 | Rewrite 引擎 | 5 | C-11 | 四类规则全部生效；`Match whole value`、大小写、`$1` 语义正确 |
| C-16 | Rewrite 管理 UI | 3 | C-15 | 增删改查 + 启用开关；保存后无需重启即生效 |
| C-17 | Map Local / Map Remote | 2.5 | C-11 | 本地文件替换成功且 Content-Type 正确；远端改写路径正确 |
| C-18 | DNS Spoofing + Block List | 1.5 | C-17 | spoof 后握手不失败；block 命中返回 403 且不出网 |
| C-19 | Throttle | 3 | C-06 | 3G 档位下 1MB 下载耗时与理论值误差 < 15% |
| C-20 | 访问控制 | 1 | C-05 | 非白名单 IP 连接被 444 |
| C-21 | 连接诊断 | 1 | C-05 | 访问固定证书的站点时详情页显示「可能启用 pinning」 |

### 9.3 P2（v1.3.0，18 人日）

| 编号 | 任务 | 人日 | 验收 |
| --- | --- | --- | --- |
| C-22 | WebSocket 帧 | 3 | WS 连接出现「帧」Tab，双向帧正确 |
| C-23 | mTLS | 2 | 双向认证站点抓包成功 |
| C-24 | Repeat / Repeat Advanced | 2.5 | 100 次迭代、10 并发执行完成并出统计 |
| C-25 | Chart + Summary | 2 | 饼图按 host 展示大小占比，与列表合计一致 |
| C-26 | Protobuf | 4 | 传 descriptor 后 gRPC body 解出字段树 |
| C-27 | CSV/Trace/XML 导出 + HAR/Trace/XML 导入 | 3 | 导入 HAR 后条数与源文件一致 |
| C-28 | 外部代理链 | 1.5 | 串到另一个代理后仍能抓包 |

### 9.4 P3（v1.4.0，12 人日）

| 编号 | 任务 | 人日 |
| --- | --- | --- |
| C-29 | 端口转发 + 反向代理 | 3 |
| C-30 | 透明代理（macOS PF） | 3 |
| C-31 | HTTP/3 识别透传 | 1 |
| C-32 | Publish Gist | 0.5 |
| C-33 | Web Interface | 3 |
| C-34 | CLI 选项 | 1.5 |

---

## 10. 测试计划

### 10.1 Rust 单元测试（`src-tauri/src/capture/tests.rs`）

| 用例 | 输入 | 期望 |
| --- | --- | --- |
| `location_exact` | `{host:"api.a.com",path:"/v1"}` vs `/v1/x` | false（path 精确匹配不含子路径，除非写 `/v1/*`） |
| `location_wildcard` | `{host:"*.a.com"}` vs `x.a.com` | true |
| `location_case` | `{host:"API.A.COM"}` vs `api.a.com` | true |
| `rewrite_header_add` | 加 `X-Test: 1` | 头存在且原头未被覆盖 |
| `rewrite_header_modify` | 改 `Content-Type` | 只改首个同名 |
| `rewrite_query_remove` | 删 `sign` | query 中无 `sign`，其它保留 |
| `rewrite_url_replace_all` | `a=1&a=2` → `a=9` | 两处都替换 |
| `rewrite_body_regex_group` | `"id":(\d+)` → `"id":$1$1` | 组引用正确 |
| `rewrite_body_case_sensitive` | `Token` vs `token` | 不匹配 |
| `rewrite_header_case_insensitive` | name `x-token` vs `X-Token` | 匹配 |
| `body_inline_threshold` | 65536 字节 | `body_state = inline` |
| `body_file_threshold` | 65537 字节 | `body_state = file` |
| `body_skip_large` | 6MB | `body_state = skipped_size` |
| `har_roundtrip` | 导出一条 flow → 解析 | 字段一致 |
| `throttle_duration` | 1MB @ 512kbps | 15.5s ~ 17.5s |

### 10.2 前端单元测试（`src/components/capture/__tests__/`）

| 用例 | 期望 |
| --- | --- |
| `virtualScroll.slice` | 视口 600px、scrollTop 2800 时 start=90（2000/28≈100，减 overscan） |
| `viewer.pick` | `application/json` → Json；`image/png` → Image；未知 → Raw |
| `jwt.decode` | Bearer 中的 JWT 能取出 `exp` |

### 10.3 手工验收

1. 打开 Chrome，访问 `https://httpbin.org/json`，能看到 JSON 明文；
2. 访问 `https://www.http2demo.io/`，`http_version` 为 `h2`；
3. 打开 `https://webSocket.org/`（或自建 echo），帧可见（P2）；
4. 手机同 Wi-Fi 设代理，抓到一个 App 的请求；
5. 断点改 `Authorization` 后放行，服务端返回 200（改错则 401）；
6. 导出 HAR → 拖回 Chrome DevTools → 能回放。

### 10.4 性能基线

| 指标 | 阈值 |
| --- | --- |
| 1000 请求/分钟 × 10 分钟 | 内存增长 < 100MB，滚动 60fps |
| 10 万条会话切换 | < 500ms |
| 5MB 响应 | 列表不阻塞（摘要先出） |
| CPU（空载录制） | < 3% |

---

## 11. 已知坑

1. **`hudsucker::Body` 不支持 `From<&[u8]>`（非 'static）**：用 `Body::from(bytes.to_vec())` 或 `Bytes`。
2. **collect 后必须回填**：`req.into_body().collect()` 会移动 body，忘记 `Body::from` 就会把空请求发出去。
3. **`should_intercept_tls` 是唯一可靠的按域名解密入口**：只改 `should_intercept_connect` 会在 HTTP/2 的 TLS 层漏掉判定。
4. **`RcgenAuthority` 缓存必须给**（1024），否则每个请求都签一次证书，CPU 直接拉满。
5. **Windows 注册表类型**：`ProxyEnable` 必须是 DWORD，写成字符串不报错但不生效。
6. **改完注册表必须 `InternetSetOptionW`**，否则已打开的 Chrome 仍走旧配置。
7. **macOS `networksetup` 必须同时设 web 与 secureweb**，只设 http 会导致 HTTPS 不走代理。
8. **`Content-Length` 与 `body` 必须同步**：Map Local 改 body 后忘记改长度，浏览器会一直转圈。
9. **不要在 handler 里做阻塞 IO**：读本地文件用 `tokio::fs`，否则卡住整个 runtime 线程。
10. **`panic = "abort"`**：handler 里任何 `unwrap()` 都会杀进程，全部改 `match` / `?`。
11. **Tauri 事件名带 `://`** 是合法的，但前端 `listen` 必须与后端完全一致（区分大小写）。
12. **Channel 要在 `invoke` 里传，不能 `window.channel`**，且同一个 channel 可复用。
13. **虚拟滚动的行高必须固定**：不同高度会让 `translateY` 错位，宁可截断文本。
14. **`vue-tsc` 对 `defineProps` 的泛型推断严格**：props 用 `interface` 显式声明。
15. **`Uint8Array<ArrayBufferLike>` 不能直接喂 `crypto.subtle`**（v1.0.8 已踩）：需要 `.slice()` 成 `ArrayBuffer`。
16. **SQLite 并发写**：批量插入必须走同一个连接与事务，并发 `execute` 会 `database is locked`。
17. **zip 导出 `.astrocap` 时 body 是二进制**，必须用 `ZipWriter` 的 raw 模式，不能转字符串。
18. **HAR 的 `time` 是毫秒但 `startedDateTime` 是 ISO8601 带时区**，格式错了 DevTools 会静默丢条目。
19. **证书指纹格式**：Windows 显示为 40 位十六进制（SHA-1）或 64 位（SHA-256），冒号分隔；UI 展示要与之一致，否则用户无法核对。
20. **`tokio::runtime::Runtime` 不能在 async 上下文里 drop**：`stop()` 要放到 `spawn_blocking` 或独立线程。

---

## 12. 验收标准

1. Chrome / Edge / Safari 访问 HTTPS，明文完整（含 gzip / br）；
2. HTTP/2 站点 `http_version=h2`；
3. SSL 白名单外的站点隧道直通，不影响访问；
4. 手机可抓包，Android 7+ 的限制有提示；
5. 断点可改请求并放行，超时自动放行；
6. Rewrite 四类规则 + 匹配语义与 Charles 一致；
7. Map Local / Remote 均生效；
8. DNS Spoofing 后 TLS 握手不失败；
9. Throttle 速率误差 < 15%；
10. Block List 命中不出网；
11. HAR 导出可被 DevTools 导入；
12. 导入 HAR 与实时流量同视图可看；
13. `.astrocap` 保存后重开完整；
14. Protobuf body 可解；
15. 1000 请求/分钟 × 10 分钟稳定；
16. 10 万条会话操作 < 500ms；
17. 强杀后重启检测并还原代理；
18. 停止录制代理精确恢复；
19. 存为接口后可直接发送；
20. 诊断包导出已脱敏；
21. WebSocket 双向帧可见；
22. 双击会话可重命名并保存备注。

---

## 13. 附录

### A. HAR 1.2 字段映射

| HAR 字段 | 来源 |
| --- | --- |
| `log.version` | 固定 `"1.2"` |
| `log.creator` | `{ name: "AstroMan", version: env!("CARGO_PKG_VERSION") }` |
| `entry.startedDateTime` | `chrono` 转 RFC3339 带本地时区 |
| `entry.time` | `duration_ms` |
| `entry.request.method/url/httpVersion` | 对应列 |
| `entry.request.headers[]` | `req_headers` 解析 |
| `entry.request.queryString[]` | path 的 query 解析 |
| `entry.request.postData.text` | `req_body`（二进制用 `encoding: "base64"`） |
| `entry.response.status/statusText` | `status` + 标准短语 |
| `entry.response.content.size/mimeType/text` | `res_size` / `content_type` / `res_body` |
| `entry.timings` | `{ send: 0, wait: ttfb_ms, receive: duration_ms - ttfb_ms }` |
| `entry.serverIPAddress` | DNS 解析结果（P1 起记录） |

### B. 限速预设

| 名称 | down | up | 延迟 |
| --- | --- | --- | --- |
| 56k | 56 kbps | 28 kbps | 300 ms |
| 256k | 256 kbps | 128 kbps | 200 ms |
| 512k | 512 kbps | 256 kbps | 150 ms |
| 1M | 1 Mbps | 512 kbps | 100 ms |
| 3G | 1.6 Mbps | 768 kbps | 300 ms |
| 4G | 4 Mbps | 3 Mbps | 80 ms |
| DSL | 2 Mbps | 512 kbps | 50 ms |
| Cable | 8 Mbps | 1 Mbps | 30 ms |

### C. 快捷键

| 快捷键 | 动作 |
| --- | --- |
| `Space` | 开始 / 停止录制 |
| `Ctrl/⌘ + F` | 聚焦过滤框 |
| `Ctrl/⌘ + L` | 清空当前会话流量 |
| `Ctrl/⌘ + E` | 导出 HAR |
| `↑` / `↓` | 列表上下移动 |
| `Enter` | 打开详情（焦点进右侧） |
| `Esc` | 关闭抽屉 / 取消搜索 |

### D. 依赖版本确认方法

```bash
cargo tree -i rustls            # 确认只有一份 rustls
cargo tree -i tokio             # 确认 tokio feature 已合并
cargo doc -p hudsucker --open   # 以实际 API 为准
```

### E. 相关标准

- RFC 7230/7231：HTTP/1.1 报文与语义
- RFC 7540/9113：HTTP/2
- RFC 8441：HTTP/2 上的 WebSocket（不支持）
- RFC 6265：Cookie
- HAR 1.2 Spec：<http://www.softwareishard.com/blog/har-12-spec/>
