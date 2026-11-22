# 分布式压测优化清单与功能设计

> 适用版本：v1.1.2（`develop-master-v1.1.2`）
> 范围：`src-tauri/src/services/cluster/*`、`src-tauri/src/commands/cluster.rs`、`src/views/Cluster.vue`、报告页
> 本文分两部分：第一部分的优化项可直接开工；第二部分是三个新功能的落地方案。

## 落地进度（2026-11-22）

| 项 | 状态 | 落点 |
|---|---|---|
| P0-1 采样裁剪 | 已完成 | `server/mod.rs` `SAMPLE_RETAIN = 300` |
| P0-2 落库节流 | 已完成 | `persist_node_throttled`，5 秒一次 |
| P0-3 心跳回写资源 | 已完成 | `apply_engine_stats` + `on_sample` 写 `host_*` |
| P0-4 通道背压 | 已完成（从节点侧） | `client.rs` 有界队列 1024 + 采样主动丢弃并计入 `droppedSamples` |
| P0-5 配对码限速 | 已完成 | `pairing.rs::register_pair_failure`，5 次/10 秒 + 累计 10 次换码 |
| 功能 C 帧聚合 + 节流 | 已完成 | `cluster:sample-frame` 每秒一帧；前端缓冲 + 2 秒节拍 + 60 点抽稀 |
| 功能 B 从机指标 | 已完成 | `HostSample` 随采样上行；节点表展示；报告样本表加列；报告页可切「QPS / 并发 / P95 / 整机 CPU / 进程内存 / 网络」六种节点曲线 |
| 功能 A 历史记录 | 已完成 | `cluster_jobs_list/detail/clear`、`cluster_slave_history`（DB v21）、两个历史表格与详情抽屉 |
| 历史资源峰值 | 已完成 | 从节点历史本地记录峰值（`cpu_peak / mem_peak_mb / host_cpu_peak / net_peak_kbps`），不再等报告回传 |
| P1-5 时钟漂移复测 | 已完成 | 施压期每 60 秒随心跳发一拍 `TimeProbe`，只记 `clock_drift_ms`、不改配对偏移（避免曲线时间轴跳变）；节点表与报告标注漂移 |
| P2 拆文件 | 已完成（第一步） | `server.rs` 4219 行 → `server/{mod,persist,pairing,tests}.rs` |

仍不做（三期路线图）：从节点断线自动重连、跨网段、故障后并发重分配、wss 加密。

> 变基说明：`server.rs` 已拆成目录，路径引用统一写成 `server/mod.rs`；`WorkerRow` 与持久化函数在 `server/persist.rs`，配对码与时钟计算在 `server/pairing.rs`，测试在 `server/tests.rs`。

---

## 0. 现状与瓶颈定位

单任务数据流：

```
从节点引擎 --每秒 SamplePayload--> WS --> 主节点 on_sample
                                          ├─ State 锁内 push 到 node.samples（无上限）
                                          ├─ persist_node：每次采样一次 UPDATE cluster_workers
                                          └─ emit_sample：每节点每秒一条 IPC 事件
                                                └─ 前端 Cluster.vue 重建整个秒表对象
                                                      └─ LineChart 全量重算 SVG path
```

主节点自身也在施压（`role=master` 也是一条 `NodeState`），所以上面这条链路**和压测引擎抢同一个进程的 CPU、同一把 SQLite 写锁**。这是"主机压不出应有性能"的根因，也是本次优化的主线。

---

## 1. 优化清单

优先级定义：**P0** 影响正确性或稳定性，先修；**P1** 影响主节点吞吐与观感；**P2** 可维护性与长期演进。

### P0-1 `node.samples` 无上限增长

| 项 | 内容 |
|---|---|
| 证据 | `server.rs:160` 定义 `samples: Vec<SamplePayload>`；`server.rs:1609` 每个采样 `push`，无裁剪 |
| 影响 | 每条采样含 `latency_buckets`（`Vec<(u32,u64)>`）。30 分钟 × 16 节点约 1.7 万条记录、量级 10 MB+；会话最长保留 30 分钟（`RETENTION_SEC`），期间持续增长，合并时还要全量遍历 |
| 方案 | 改环形缓冲，只保留最近 `SAMPLE_RETAIN = 300` 条（AI 诊断接口 `cluster_node_samples` 默认 30、上限 300，够用）；在 `on_sample` 里 `if len > SAMPLE_RETAIN { drain(..len-RETAIN) }`。`received_samples` 是独立计数，不受影响 |
| 验收 | 60 秒以上任务，单节点 `samples.len() <= 300`；`cluster_node_samples` 返回不空；合并结果与改前一致（用 `acceptance.rs` 的合并吞吐用例比对） |

### P0-2 每次采样触发一次数据库写

| 项 | 内容 |
|---|---|
| 证据 | `server.rs:1611` 在 `on_sample` 里直接调 `persist_node` → `update_worker`（`server.rs:890`） |
| 影响 | 16 节点 = 16 次 `UPDATE cluster_workers`/秒，autocommit 事务。SQLite 单写者，与引擎的报告落库、主节点自身采样抢锁；`busy_timeout=5000` 下表现为偶发卡顿 |
| 方案 | `NodeState` 增加 `last_persist_at: i64`，`persist_node` 内部按 `PERSIST_INTERVAL_MS = 5000` 节流；以下场景强制写：状态变更（`mark_terminal`、`on_done`、失联）、`prepare` 完成、任务结束。节流写失败要留日志但不清空时间戳 |
| 验收 | 16 节点运行时 `cluster_workers` 写入频率从 16/s 降到 ≤ 4/s；节点状态字段在终态仍准确 |

### P0-3 心跳回传的 CPU/内存没有落到节点状态

| 项 | 内容 |
|---|---|
| 证据 | `server.rs:1313-1315` 收到 `Ping.stats` 后只更新 `last_stats` 与 `qps`；`node.cpu` 仅在 `server.rs:1632`（收到 `Done`）和 `2490` 赋值 |
| 影响 | 运行期的节点表 CPU/内存列一直是 `—`，只有任务结束才出现峰值。现在做"从机参数实时查看"必须先补这一条 |
| 方案 | 心跳带 `stats` 时同步写 `node.cpu / node.mem_mb`（见功能 B 扩展后的字段），并更新 `last_seen_at` |
| 验收 | 运行中 `ClusterNodeInfo.cpu` 非空且随秒变化；`cluster_workers.cpu` 在运行期有值 |

### P0-4 消息通道无背压

| 项 | 内容 |
|---|---|
| 证据 | `server.rs:2847`、`client.rs:578` 均为 `mpsc::unbounded_channel` |
| 影响 | 主节点在合并/落库抖动时，从节点每秒采样会无界堆积；反过来主节点向从节点下发也可能堆积。内存峰值无法预估 |
| 方案 | 采样类消息走独立的有界通道（容量 1024），满了 `try_send` 失败即丢弃并累加 `dropped_samples`，同时把 `SampleHealth.state` 标 `gap`；控制类消息（Prepare/Start/Abort/Ping）仍走无界通道，避免控制指令被丢。从节点侧同理，落一条 `droppedSamples` 进 `Done` |
| 验收 | 人为阻塞主节点 5 秒，进程内存不增长；任务完成后 `droppedSamples > 0` 且报告标 `incomplete` 或不完整提示 |

### P0-5 配对码没有失败次数限制

| 项 | 内容 |
|---|---|
| 证据 | `server.rs:952` 只做字符串比较，失败仅返回 `Err("配对码不正确")` |
| 影响 | 6 位数字（100 万组合）+ 5 分钟有效期，局域网内脚本可高速尝试；一条 `ws` 连接建立成本很低 |
| 方案 | 在 `ClusterState` 里维护 `pair_attempts: HashMap<IpAddr, (count, window_start)>`：同一 IP 10 秒内超过 5 次失败即拒绝并延迟响应；同一个码累计失败 5 次立即作废并生成新码（前端提示"配对码已刷新"）。失败计数不落库，进程内即可 |
| 验收 | 连续错误 5 次后旧码失效；正常配对不受影响；`acceptance.rs` 补一条"错误码限速"用例 |

### P1-1 前端每来一条采样就重建整张秒表

| 项 | 内容 |
|---|---|
| 证据 | `Cluster.vue:1278-1292`：`{...liveSamplesBySec.value, [sec]: {...}}` 是整表浅拷贝，每次事件一次；`Cluster.vue:357-380` 的 computed 再全量排序、映射 |
| 影响 | 16 节点 = 16 次/秒的全表拷贝 + 16 次排序 + 16 次 `LineChart` path 重算。WebView 主线程和 Rust 引擎抢 CPU |
| 方案 | 见功能 C：非响应式缓冲 + 2 秒节拍提交 + 点数降采样 |
| 验收 | 16 节点 60 秒窗口下，`LineChart` 重算次数从 16/s 降到 0.5/s；主节点进程 CPU 占用下降（对比同一场景） |

### P1-2 采样事件按节点逐条 IPC

| 项 | 内容 |
|---|---|
| 证据 | `server.rs:1612` 每条采样 `emit_sample`；前端 `Cluster.vue:1274` 每个事件一次回调 |
| 影响 | IPC 是跨进程序列化，每帧都带 `latencyBuckets`。16 节点时每秒 16 次序列化 + 16 次跨线程投递 |
| 方案 | 后端改为按秒聚合：`ClusterSampleFrame { jobId, sec, nodes: Vec<NodeSampleLite> }`，每 200~1000 ms 发一帧（见功能 C）。`NodeSampleLite` 只带图表需要的字段：`nodeId, qps, concurrent, p95, errorCount, cpu, memMb`。原始 `buckets` 留在后端，供合并与 AI 诊断 |
| 验收 | 16 节点时 `cluster:sample` 事件数 ≈ 1/s；曲线与节点表数值与改前一致 |

### P1-3 运行期重算采集健康度并全量序列化快照

| 项 | 内容 |
|---|---|
| 证据 | `server.rs:835` `emit_snapshot` → `snapshot()`；`health_of`（`server.rs:278`）每次按 `expected/received` 重算 |
| 影响 | 心跳与状态变更都会触发全量快照（16 个节点的 `engine_stats`、`health`、`capacity`）。字段多、序列化成本高，且前端大部分时候只用其中几项 |
| 方案 | 运行期快照拆两档：`snapshot()` 保持全量（前端主动拉取/任务结束用），事件推送改用 `snapshot_light()`，去掉 `engine_stats` 明细与 `varAudit`，只保留展示必需字段；`health` 改为 5 秒更新一次并缓存 |
| 验收 | 运行期快照 JSON 体积下降 50% 以上；页面数值无缺失 |

### P1-4 压测端 CPU 只统计了进程，不是整机

| 项 | 内容 |
|---|---|
| 证据 | `commands/stress.rs:1655-1685` 的 monitor 只 `ProcessRefreshKind`，`cores = process.cpu_usage()/100` |
| 影响 | 报告里的"施压端峰值 CPU"是进程级，机器被其他进程拖慢时看不出来；分布式场景下更容易误判"从机还有余量" |
| 方案 | monitor 同时 `sys.refresh_cpu_usage()` 取整机使用率，`runtime` 里新增 `hostCpuPercent`、`hostMemUsedMb`；报告节点表与峰值专题用整机口径，进程口径保留为"引擎占用" |
| 验收 | 人为起一个 CPU 占用进程，报告整机 CPU 明显上升、引擎占用不变 |

### P1-5 长时间任务的时钟漂移没有二次校正

| 项 | 内容 |
|---|---|
| 证据 | `CLOCK_SAMPLE_ROUNDS = 5` 只在 `register` 阶段跑一次（`server.rs:991`），之后 `offset_ms` 固定 |
| 影响 | 1 小时以上的任务，两端晶振漂移可达几十毫秒；跨机曲线按秒对齐会出现系统性错位，`elapsed_sec` 相同的点实际不对齐 |
| 方案 | 运行期每 60 秒随心跳做一次轻量重探（复用 `TimeProbe/TimeAck`），新 offset 与旧值差 > 20 ms 时用 EMA（系数 0.3）平滑更新，并记一条 `clock_shift` 事件到会话日志；`uncertainty > 200ms` 的节点仍走丢弃采样逻辑 |
| 验收 | 模拟 2 小时任务（可缩短时钟），曲线错位 < 50 ms；`clock_offset_ms` 有变化记录 |

### P2-1 `server.rs` 单文件 4219 行

按职责拆分，纯移动、不改行为：

- `server/mod.rs`：`ClusterMaster`、状态结构、生命周期入口（create/prepare/launch/abort/close）
- `server/pairing.rs`：`register` / `register_request` / `approve` / `reject` / 时钟探测
- `server/sampling.rs`：`on_sample` / `on_ping` / `health_of` / 采样缓冲
- `server/finalize.rs`：`maybe_finalize` / `merge` 调用 / 报告落库 / `prune_finished`
- `server/repo.rs`：`insert_job` / `update_job` / `update_worker` / `load_*` 等 SQL

### P2-2 `protocol.rs` 顶部的 `#![allow(dead_code)]`

模块级屏蔽会让"协议字段加了但没接线"永远不报警。建议改为对个别确实预留的项加 `#[allow(dead_code)]`，其余交给 clippy。

### P2-3 报告样本表缺资源维度

`cluster_report_samples`（`schema.sql:603`）只有 `concurrent/qps/p95_rt/error_count`，无法在报告里画节点 CPU/内存曲线。随功能 B 一起加列。

### P2-4 前端曲线 x 轴按数组下标等距

`Cluster.vue:357-364` 取"最近 120 个有数据的秒"当 labels，丢秒时会压缩时间轴，看起来像"没掉速"。建议固定窗口 `maxSec-119..maxSec`，缺失秒补 `null`，`LineChart` 断线而不是拉平。

### P2-5 合并阶段全量遍历

`merge.rs` 在结束时遍历所有节点全部采样与直方图。当前 16 节点 × 30 分钟可接受；若后续放开节点数或时长，应改为运行期增量累加全局直方图，结束只做最后一帧。

---

## 2. 功能 A：配对历史与执行历史（主从都能看）

### 2.1 目标

- 主节点：看"这个任务配对了哪些机器、谁被拒了、每个节点干了多少活、报告在哪"。
- 从节点：看"我连过哪些主节点、参与过哪些任务、跑了多少 QPS、什么时候断的"。
- 两边都能点进详情，主节点可跳报告；从节点只展示自己那份数据（主节点报告不落地到从机，避免数据外流）。

### 2.2 主节点（复用现有表，只加读接口）

数据已经在 `cluster_jobs` + `cluster_workers` 里，**不需要新表**，只缺查询与 UI。

新增 IPC（`commands/cluster.rs`，记得注册 `lib.rs::generate_handler!`）：

```rust
cluster_jobs_list(page: Option<u32>, page_size: Option<u32>,
                  status: Option<String>, keyword: Option<String>)
    -> ClusterJobPage { total: i64, items: Vec<ClusterJobSummary> }

cluster_job_detail(job_id: i64)
    -> ClusterJobDetail { job: ClusterJobSummary, nodes: Vec<ClusterWorkerRow>, report_id: Option<i64> }

cluster_jobs_clear(keep_days: Option<i64>) -> u32   // 只删已结束任务，级联删 workers
```

`ClusterJobSummary` 字段：`id / createdAt / finishedAt / status / planName / masterNode / totalConcurrent / qpsAvg / reportId / incomplete / message / nodeCount`。

查询要点：

- 列表按 `created_at DESC` 分页，默认 20 条；`keyword` 匹配 `plan_name`、`master_node`。
- `qpsAvg` 不存表，用 `total_concurrent` 与报告聚合值二选一：优先读 `test_reports.qps_avg`（有 `report_id` 时 JOIN），没有则显示 `—`。
- 删除只允许 `status IN ('finished','failed')`，运行中的任务不删。

UI（`Cluster.vue` 顶部加「历史」tab，或主页下方"最近任务"卡片）：

- 表格列：时间、计划、节点数、总并发、平均 QPS、状态、报告。
- 行展开/抽屉：节点明细（名称/角色/地址/并发/平均 QPS/CPU 峰值/内存峰值/时钟偏差/采集健康度），复用现有 `ClusterNodeInfo` 的展示组件。
- 顶部筛选：状态、时间范围、关键字。

### 2.3 从节点（需要新表）

当前从节点**完全不落库**（`client.rs` 只读环境，不写库），所以要从零加一张表。

`db/mod.rs`：`DB_VERSION` 20 → 21，`schema.sql` 追加：

```sql
-- 从节点配对/执行历史（本机视角）
CREATE TABLE IF NOT EXISTS cluster_slave_history (
    id                INTEGER PRIMARY KEY AUTOINCREMENT,
    created_at        INTEGER NOT NULL,
    master_addr       TEXT NOT NULL DEFAULT '',
    master_node       TEXT NOT NULL DEFAULT '',
    master_job_id     INTEGER,
    node_name         TEXT NOT NULL DEFAULT '',
    plan_name         TEXT NOT NULL DEFAULT '',
    join_mode         TEXT NOT NULL DEFAULT 'code',   -- code / request / discovery
    concurrent        INTEGER NOT NULL DEFAULT 0,
    total_concurrent  INTEGER NOT NULL DEFAULT 0,
    status            TEXT NOT NULL DEFAULT 'connecting',
                        -- connecting/preparing/running/done/failed/left/lost
    qps_avg           REAL NOT NULL DEFAULT 0,
    cpu_peak          REAL NOT NULL DEFAULT 0,
    mem_peak_mb       INTEGER NOT NULL DEFAULT 0,
    started_at        INTEGER,
    ended_at          INTEGER,
    message           TEXT
);
CREATE INDEX IF NOT EXISTS idx_cluster_slave_history_created
    ON cluster_slave_history(created_at DESC);
```

写入点（都在 `client.rs`，用 `AppState` 已有的 `pool`，不要新开连接）：

| 时机 | 动作 |
|---|---|
| `connect_with` 建立会话 | `INSERT`，`status=connecting`，记 `master_addr`、`join_mode` |
| 收到 `HelloAck` | `UPDATE master_node / node_name / master_job_id` |
| 收到 `Prepare` | `UPDATE plan_name / concurrent / total_concurrent`，`status=preparing` |
| 收到 `Start` | `UPDATE status=running, started_at` |
| 收到 `Done` | `UPDATE status=done, started_at/ended_at, qps_avg, cpu_peak, mem_peak_mb` |
| `disconnect` / 心跳超时 / 引擎报错 | `UPDATE status=left/lost/failed, ended_at, message` |

实现建议：抽一个 `SlaveHistory { id: i64 }` 句柄挂在 `SlaveSession` 上，各阶段调 `update(...)`；更新失败只 `log::warn`，绝不阻塞协议主流程。历史写入放 `spawn_blocking` 或复用现有 DB 线程池，避免卡 WS 读循环。

新增 IPC：

```rust
cluster_slave_history_list(limit: Option<u32>, offset: Option<u32>)
    -> Vec<ClusterSlaveHistoryItem>
cluster_slave_history_clear() -> u32
```

UI：`Cluster.vue` 从节点面板下方"本机参与过的任务"表格（时间、主节点、计划、并发、QPS、状态），支持清空。

### 2.4 不做的事

- 不做跨机同步历史（主节点不把历史推给从机，从机不落主节点报告）。
- 不做云端上报。
- 历史不做实时刷新，进入 tab 时拉一次 + 手动刷新。

---

## 3. 功能 B：从机资源指标采集与展示

### 3.1 采集什么

第一版只做**对压测结论有影响**的指标，避免为了"全"而上高开销采集：

| 指标 | 单位 | 说明 |
|---|---|---|
| `cpu` | 核 | 进程占用（沿用现有），保持报告口径连续 |
| `hostCpuPercent` | % | 整机 CPU，识别"机器被别的进程拖慢" |
| `memMb` | MB | 进程 RSS |
| `hostMemPercent` | % | 整机内存 |
| `threads` | 个 | 进程线程数，判断连接池/协程是否异常膨胀 |
| `netRxKbps` / `netTxKbps` | KB/s | 网卡收发速率，压满千兆时是硬瓶颈 |
| `load1` | — | Linux/macOS 1 分钟负载，Windows 为 0 |
| `fdCount` | 个 | Unix 文件描述符数；Windows 用句柄数，取不到填 0 |

不采集：磁盘 IO（压测机一般不影响）、GPU、温度（跨平台成本高）。

### 3.2 协议扩展（关键：不要 bump 版本）

`EngineStats` 与 `SamplePayload` 加字段，全部 `#[serde(default)]`：

```rust
pub struct HostSample {
    pub cpu: f64,
    #[serde(default)] pub host_cpu_percent: f64,
    pub mem_mb: u64,
    #[serde(default)] pub host_mem_percent: f64,
    #[serde(default)] pub threads: u32,
    #[serde(default)] pub net_rx_kbps: f64,
    #[serde(default)] pub net_tx_kbps: f64,
    #[serde(default)] pub load1: f64,
    #[serde(default)] pub fd_count: u32,
}

pub struct SamplePayload {
    // ... 现有字段不动
    #[serde(default)]
    pub host: Option<HostSample>,
}
```

`EngineStats` 同步加 `host_cpu_percent`、`threads`、`net_rx_kbps`、`net_tx_kbps`、`load1`，供心跳 2 秒一次的实时展示。

**版本策略**：`PROTOCOL_VERSION` 保持 `2`。serde 默认忽略未知字段，旧节点收到新字段不会报错，新主节点读旧节点用 `default` 补 0。只有"必须拒绝旧节点"的语义变更才 bump —— 这里不是。

### 3.3 采集实现

- 复用 `commands/stress.rs` 里已有的 `SysMonitor`（`sysinfo::System`），扩展为 `SysSampler`：
  - `refresh_cpu_usage()` 取整机；
  - `ProcessRefreshKind::new().with_cpu().with_memory()` 取进程；
  - 网络速率用 `sysinfo::Networks`，**保存上一帧字节数**做差分，除以真实间隔（不是写死 1 秒）；
  - `threads` 从 `process.tasks()` 或 `Process::tasks()` 长度取；
  - `fd_count`：Unix 用 `libc::fcntl` 或读 `/proc/self/fd`，Windows 用 `GetProcessHandleCount`，取不到填 0；
  - 采样间隔与引擎统计同一拍（1 秒），避免额外线程。
- 主节点自身也走同一个采集器，`role=master` 的节点同样上报，报告口径统一。

### 3.4 主节点侧

- `NodeState` 增加 `host: Option<HostSample>`（最近一帧）与 `host_series: Vec<HostSample>`（与 `samples` 同寿命、同裁剪）。
- `ClusterNodeInfo` 增加 `threads / netRxKbps / netTxKbps / hostCpuPercent / hostMemPercent / load1`，全部 `Option`，旧节点为 `None`。
- 前端节点表增加列：CPU（整机 / 引擎）、内存、线程、网络（收/发）。列多时用"资源"折叠列，默认显示 CPU + 网络。
- 采样帧（功能 C）里带上这些值，图表可切换指标：QPS / 并发 / P95 / CPU / 内存 / 网络。

### 3.5 报告侧

- `SeriesPoint` 增加 `#[serde(default)] cpu: f64, mem_mb: u64, host_cpu_percent: f64, net_rx_kbps: f64, net_tx_kbps: f64`。
- `ClusterReportNode` 增加 `cpu_avg / host_cpu_avg / mem_avg_mb / net_rx_avg / net_tx_avg`。
- `cluster_report_samples` 加列（迁移用 `ALTER TABLE`，失败 `let _ =`）：

```sql
ALTER TABLE cluster_report_samples ADD COLUMN cpu REAL DEFAULT 0;
ALTER TABLE cluster_report_samples ADD COLUMN host_cpu_percent REAL DEFAULT 0;
ALTER TABLE cluster_report_samples ADD COLUMN mem_mb INTEGER DEFAULT 0;
ALTER TABLE cluster_report_samples ADD COLUMN threads INTEGER DEFAULT 0;
ALTER TABLE cluster_report_samples ADD COLUMN net_rx_kbps REAL DEFAULT 0;
ALTER TABLE cluster_report_samples ADD COLUMN net_tx_kbps REAL DEFAULT 0;
```

放在 `migrate()` 里，`DB_VERSION = 21`。

- `ReportDetail.vue` 的集群区块：
  - 节点表加"资源"列（CPU 峰值、整机 CPU、内存峰值、网络峰值）；
  - 节点级曲线支持指标切换，默认 QPS；
  - "峰值专题"加一张资源卡片：哪台机器 CPU 先到瓶颈、网络是否打满（`netTxKbps` 接近网卡带宽时给提示）。

### 3.6 兼容性

- 旧从节点：`host` 为 `None`，报告里资源列显示 `—`，并在 `cluster.note` 里注明"部分节点未上报资源指标（旧版本）"。
- 旧主节点接入新从节点：新字段被忽略，不影响压测。
- 混合集群：以节点为粒度展示，不做整体降级。

---

## 4. 功能 C：主节点渲染节流

目标：**把"展示"从压测关键路径上摘出去**，主节点自己施压时不再被图表拖慢。

### 4.1 三层节流

**第一层：后端按帧聚合（降 IPC 次数）**

- 新增 `ClusterSampleFrame { job_id, sec, nodes: Vec<NodeSampleLite> }`，200 ms 攒一帧（可配 500/1000 ms）。
- `ClusterSampleLite`：`node_id, qps, concurrent, p95, error_count, cpu, mem_mb, net_rx_kbps, net_tx_kbps`。
- 原始 `SamplePayload`（含 buckets）**只留在后端**做合并与 AI 诊断，不再逐条发给前端。
- 事件数：16 节点 × 1/s = 16/s → 1/s（1s 帧）。

**第二层：前端非响应式缓冲 + 节拍提交**

```ts
// 非响应式，写入零开销
const rawSamples = new Map<number, Record<string, NodeSampleLite>>()

// 2 秒提交一次到响应式 state
const commitTimer = window.setInterval(() => {
  if (document.hidden) return          // 最小化到托盘时零渲染
  liveSamplesBySec.value = buildWindow(rawSamples)
}, 2000)
```

- 窗口固定 120 秒，滑动裁剪。
- `LineChart` 用 `shallowRef` 接收数据；点数 > 120 时按步长抽稀到 60~80 点再生成 path。
- 图表容器加 `v-memo`，避免无关状态变化触发重绘。

**第三层：运行期降级**

- 压测运行中，非关键面板（变量体检、协议参数、日志）默认折叠并暂停刷新。
- 节点明细表 2 秒刷新一次，与图表同一节拍（现在跟着快照每秒刷）。
- 页面隐藏（`document.hidden`）时停止全部渲染节拍，只保留数据缓冲；恢复可见时一次性补齐。

### 4.2 可配置

在 `Cluster.vue` 图表卡片右上角加一个下拉：`实时（1s） / 平衡（2s，默认） / 省资源（5s）`，存 `localStorage`。默认"平衡"。

### 4.3 预期收益

| 指标 | 改前 | 改后 |
|---|---|---|
| `cluster:sample` IPC 事件 | 16/s | 1/s |
| 图表 path 重算 | 16/s | 0.5/s |
| 前端秒表对象整表拷贝 | 16/s | 0.5/s |
| 页面隐藏时渲染开销 | 持续 | 0 |

数值来自当前 16 节点上限（`MAX_NODES = 16`）推算，落地后应实测对比主节点进程 CPU 与压测 QPS 波动。

---

## 5. 实施顺序

| 阶段 | 内容 | 涉及文件 | 风险 |
|---|---|---|---|
| PR1 | P0-1 ~ P0-5（采样裁剪、落库节流、心跳写 CPU、通道背压、配对码限速） | `server.rs` | 低，不改协议、不改 UI |
| PR2 | 功能 C（帧聚合 + 前端节拍） | `server.rs`、`protocol.rs`、`Cluster.vue`、`LineChart.vue` | 中，事件结构变更，需同步 `types/cluster.ts` |
| PR3 | 功能 B（采集 + 协议字段 + 报告列 + 报告页） | `stress.rs`、`client.rs`、`protocol.rs`、`merge.rs`、`schema.sql`、`db/mod.rs`、`ReportDetail.vue` | 中，DB 迁移需测老库升级 |
| PR4 | 功能 A（主节点历史接口 + 从节点历史表 + 两个 UI） | `commands/cluster.rs`、`client.rs`、`schema.sql`、`db/mod.rs`、`Cluster.vue` | 中低，新增表不影响既有数据 |

顺序理由：先把稳定性与热点修掉（PR1），再做节流（PR2，收益立竿见影且能验证 PR1），然后加指标（PR3，需要 PR1 的心跳修复与 PR2 的帧通道），最后做历史（PR4，独立性强，可以并行）。

### 每阶段门禁

```bash
cargo test --lib cluster::
cargo test --lib db::
npm run test
npm run build
cargo fmt --check
cargo clippy --all-targets -- -D warnings
npm run ui:check
```

DB 变更额外验证：

1. 用 v20 老库启动，确认自动迁移到 21、旧数据可读；
2. 重复启动两次，确认 `ALTER TABLE` 不报错（`let _ =` 吞重复列）；
3. 检查依赖新列的索引确实建在补列之后（历史事故：`idx_plans_project`）。

### 协议变更检查表

1. `protocol.rs` 字段加好后，确认 `server.rs` 与 `client.rs` 两侧都读/写了；
2. `src/types/cluster.ts` 同步（camelCase）；
3. 新增字段全部 `#[serde(default)]`，旧节点不炸；
4. `PROTOCOL_VERSION` 只在"旧节点语义不兼容"时改，本次三个功能都不需要。

---

## 6. 待确认

1. 历史记录保留策略：主节点历史是否跟随报告一起清理（建议保留全部，只做分页），从节点历史建议保留最近 200 条。
2. 资源指标是否需要"网络打满"预警阈值（按网卡速率推算），还是只展示不判断。
3. 图表默认节拍选 2 秒是否可以接受（曲线会有 2 秒延迟感）。
