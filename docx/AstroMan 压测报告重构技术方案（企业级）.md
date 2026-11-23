# AstroMan 压测报告重构技术方案（企业级）

| 项目 | 内容 |
| --- | --- |
| 文档名称 | AstroMan 压测报告重构技术方案 |
| 文档版本 | v1.0 |
| 编写日期 | 2026-11-23 |
| 适用版本 | AstroMan v1.1.3 及后续版本 |
| 文档状态 | 待评审 |
| 关联代码 | `src/utils/reportExport.ts`、`src/views/ReportDetail.vue`、`src-tauri/src/commands/report.rs`、`src-tauri/src/commands/stress.rs`、`src-tauri/src/services/cluster/*` |
| 关联文档 | 外部压测报告基准（5 份 taskid 报告）、《AstroMan 项目优化评估报告（2026-11-23）》 |

---

## 1. 背景与问题

### 1.1 现状

AstroMan 当前的压测报告同时承担三种角色：

1. 实测数据记录；
2. 自动分析与诊断；
3. 优化建议与结论输出。

导出报告（HTML / Excel）中包含「关键发现」「优化建议」「四维体检评分」「容量拐点结论」「压测结论/结论依据」等内容。这些内容由规则推断生成，存在三个问题：

- **结论不可复现**：同一份数据在不同版本规则下可能给出不同评分或建议；
- **报告职责过重**：客户或研发阅读报告时，首先需要的是真实数据与结果，而不是系统给出的主观判断；
- **与外部报告口径不一致**：外部报告（5 份基准）只呈现事实数据、明细表与曲线，没有评分、建议和推断性结论。

同时，对照 5 份外部报告，AstroMan 还缺少几类企业级压测报告常见的事实数据：

- 按 URL 聚合的请求统计与错误统计；
- 按施压机（客户端）聚合的请求数、成功/失败、平均耗时、TPS、带宽；
- 错误数曲线、并发曲线；
- 测试机数量、并发间隔、外网总流量等元信息；
- 可编辑的报告标题、报告页再次运行、页面免责声明。

### 1.2 外部 5 份报告基准

本次共分析以下 5 份报告：

| 任务号 | 目标                         | 时长 | 施压机 | 并发 | 总请求 | 失败 | 峰值 QPS | 90% QPS | URL 数 | 客户端加权 RT |
| --- |----------------------------| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 20260129223819 | http://57.136.203.124/test | 90s | 31 | 1550~3100 | 960,593 | 46 | 5,614 | 4,531 | 3 | 305ms |
| 20260129214508 | http://57.136.203.124/test | 400s | 31 | 1550~4650 | 5,070,816 | 973 | 11,161 | 8,910 | 12 | 287ms |
| 20260130005403 | http://57.136.203.124/test | 120s | 31 | 3,100 | 18,000 | 0 | 3,846 | 514 | 3 | 200ms |
| 20260131000700 | http://57.136.203.124/... | 300s | 17 | 85~255 | 1,717,120 | 0 | 6,280 | 3,993 | 1 | 33ms |
| 20260201014306 | http://57.136.203.124/...   | 240s | 30 | 900~2700 | 1,097,389 | 1,330 | 4,370 | 3,123 | 1 | 501ms |

外部报告的数据质量问题（仅作为字段参考，不作为口径参考）：

- 汇总「平均耗时」字段为 0，但客户端加权平均为 0.033~0.501s；
- 客户端 CPU、入站带宽、数据包大小等字段普遍为 0 或空；
- 时间序列只覆盖部分运行时长（例如第一份曲线 59 秒、请求合计 29 万，而总量 96 万）；
- 错误时间曲线数量与状态分布不一致；
- 部分报告 90% QPS 明显受升压段和尾部 0 值影响。

**结论：本次重构只对齐字段与结构，不照抄外部聚合口径。**

### 1.3 外部报告章节

1. 报告头：标题、测试场景、任务编号、开始时间、测试机数量、持续时间、并发间隔、思考时间、目标主机、并发数量；
2. 结果汇总：总请求、成功率、外网总流量（入+出）、最大 QPS、90% 分布 QPS；
3. 统计图：QPS、错误数、并发、平均响应时间；
4. 响应时间与状态分布：按 URL 平均响应、HTTP 状态分布；
5. 测试机（客户端）详情：IP、开始运行时间、总/成功/失败、并发、成功率、平均耗时、TPS、出站带宽平均；
6. 测试地址详情：地址、HTTP Code、请求数、平均耗时、最长耗时、最长耗时时间、最长耗时请求 ID、总耗时、平均包大小、总流量；
7. 错误地址详情：地址、HTTP Code、错误数、平均/最长耗时、最长耗时时间、请求 ID、错误信息；
8. 报告标题/场景/总结可编辑、报告对比、再次运行、免责声明。

---

## 2. 目标与非目标

### 2.1 目标

1. **导出报告只保留事实数据与结果**，删除主观建议、评分、推断性结论；
2. **字段覆盖外部 5 份报告的全部可见内容**；
3. **URL 聚合统计在压测引擎内采集**，在 `recordDetail=off/sample/full` 三种模式下都可用；
4. **施压机明细**覆盖单机与集群，字段可验证、可导出；
5. **HTML / Excel / 页面三端结构一致**，避免同一份报告三套口径；
6. **兼容旧报告与旧从节点**，升级不阻塞、不丢历史数据；
7. **满足企业级要求**：数据可追溯、口径可复现、性能可控、敏感信息脱敏、可测试、可回滚。

### 2.2 非目标

1. 本方案不引入新的压测模型（如多接口事务编排）；
2. 不改变现有 HTTP 压测引擎的调度算法；
3. 不改变 SLA、基线、峰值专题的既有计算口径；
4. 不要求旧报告补齐 URL 统计（旧报告无数据时明确标注“未记录”）；
5. 不提供服务端集中式报告平台（仍为本地桌面应用 + 文件导出）。

### 2.3 能力边界说明

AstroMan 当前计划模型是“单计划单请求”。外部报告存在一次任务 12 个 URL 的场景。

本方案要求：

- 报告结构与导出表**必须支持多 URL**；
- 在当前单请求模型下，URL 表只有 1 行；
- 若后续需要一次压测多 URL，需要新增多请求/事务场景能力，属于独立产品需求，不在本方案强制范围内，但数据模型与导出结构需提前兼容。

---

## 3. 总体设计

### 3.1 设计原则

| 原则 | 说明 |
| --- | --- |
| 事实优先 | 报告只展示实测值、直接计算值、配置快照、原始分布与失败明细 |
| 单一数据源 | 页面、HTML、Excel 使用同一套后端聚合结果，避免三端各算一遍 |
| 采集与展示分离 | URL/节点统计在引擎内计数，展示层只负责渲染 |
| 向后兼容 | 新字段全部可缺省；旧报告、旧从节点不得导致崩溃 |
| 性能可控 | 聚合为 O(1) 更新，锁分片，键数量有上限，导出走后端流式写盘 |
| 安全默认 | URL、错误信息、请求头中的敏感信息默认脱敏 |
| 可回滚 | 数据库只做增量迁移；导出结构调整保留旧格式开关，必要时可回退 |

### 3.2 目标报告结构

#### HTML 报告

| 章节 | 内容 |
| --- | --- |
| 封面 | 标题、场景、报告编号、任务时间、持续时长、施压机数量、并发/加压方式/思考时间、目标主机 |
| 01 结果汇总 | 总请求、成功/失败、成功率、平均/峰值/90% QPS、平均/P95/P99、总流量（入+出）、错误数 |
| 02 测试环境与配置 | 目标、施压端、并发、阶梯/升压、思考时间、超时、采集模式、SLA |
| 03 吞吐与并发 | QPS 曲线、并发曲线、每秒成功/失败数 |
| 04 响应时间 | 平均/P95/P99 趋势、分位表、直方图、TTFB/Body |
| 05 错误与状态码 | 错误数曲线、状态码分布、错误类型、Top 错误 |
| 06 URL 与施压机 | URL 聚合表、错误 URL 表、施压机明细表、集群节点表与节点曲线 |
| 07 采样摘要 | 采样说明、数据完整性、导出说明 |
| 附录 A | 指标口径定义 |
| 附录 B | 数据完整性校验 |
| 附录 C | 免责声明 |

**删除章节**：

- 06 容量评估与瓶颈定位（拐点推断、扩展效率结论）；
- 07 结论与优化建议（优化建议、结论封面、评分）；
- 关键发现、四维体检、雷达图、压测结论/结论依据。

#### Excel 工作簿

| 顺序 | 工作表 | 说明 |
| --- | --- | --- |
| 1 | 结果汇总 | 原「概览」，去掉压测结论/结论依据 |
| 2 | 配置快照 | 保留 |
| 3 | URL 统计 | 新增，按 URL 聚合 |
| 4 | 错误地址 | 新增，错误 URL 聚合 |
| 5 | 施压机详情 | 新增，单机/集群通用 |
| 6 | 延迟分位 | 保留 |
| 7 | 采样明细 | 后端流式写入 |
| 8 | 请求明细 | 后端流式写入 |
| 9 | 响应时间分布 | 保留 |
| 10 | 状态码分布 | 保留 |
| 11 | 错误类型 | 保留 |
| 12 | 错误明细 | 保留 |
| 13 | 分阶段汇总 | 保留（仅事实指标，去掉扩展效率结论列） |
| 14 | 基线对比 | 有基线时生成 |
| 15 | 集群峰值 | 集群报告 |
| 16 | 集群节点 | 集群报告 |
| 17 | 节点采样 | 集群报告 |
| 18 | 池统计 | 有池时生成 |
| 19 | 说明 | 口径、完整性、脱敏说明、免责声明 |

**删除工作表**：`优化建议`。

#### 页面（ReportDetail.vue）

- 首屏保留事实指标卡与结果汇总；
- 新增 URL 统计、错误地址、施压机详情；
- 「关键发现 / 自动分析 / 评分」不再常驻展示，可收敛到「分析（可选）」或仅通过「AI 解读」按钮按需触发；
- 导出内容与页面以事实数据为准，分析内容不进入导出。

---

## 4. 数据模型设计

### 4.1 新增表 `report_url_stats`

按 `(report_id, method, url, status_code)` 聚合，避免把状态码塞进单个 JSON 字段导致无法查询。

```sql
CREATE TABLE IF NOT EXISTS report_url_stats (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    report_id     INTEGER NOT NULL REFERENCES test_reports(id) ON DELETE CASCADE,
    method        TEXT    NOT NULL DEFAULT 'GET',
    url           TEXT    NOT NULL,
    status_code   INTEGER NOT NULL DEFAULT 0,
    requests      INTEGER NOT NULL DEFAULT 0,
    success_count INTEGER NOT NULL DEFAULT 0,
    error_count   INTEGER NOT NULL DEFAULT 0,
    sum_ms        REAL    NOT NULL DEFAULT 0,
    max_ms        REAL    NOT NULL DEFAULT 0,
    max_at_ms     INTEGER,
    bytes         INTEGER NOT NULL DEFAULT 0,
    error_message TEXT,
    max_request_id TEXT
);

CREATE INDEX IF NOT EXISTS idx_url_stats_report
    ON report_url_stats(report_id, status_code);
```

字段说明：

| 字段 | 说明 |
| --- | --- |
| status_code | HTTP 状态码；0 表示连接失败/超时等无响应错误 |
| requests | 该 URL 该状态码的请求数 |
| success_count / error_count | 成功/失败计数 |
| sum_ms | 总耗时，用于计算平均耗时与总耗时（小时） |
| max_ms / max_at_ms | 最长耗时及发生时间 |
| bytes | 响应字节数 |
| error_message | 该 URL 的第一条错误样例，截断 500 字符 |
| max_request_id | 最长耗时请求标识；外部报告该字段为空，我们取不到时导出显示“—” |

### 4.2 扩展 `test_reports`

新增标题字段（支持可编辑报告标题）：

```sql
ALTER TABLE test_reports ADD COLUMN report_title TEXT;
```

读取时回退顺序：`report_title` → `plan_name`。

### 4.3 数据库版本与迁移

- 当前 `DB_VERSION = 22`；
- 本方案升级为 `23`；
- `schema.sql` 增加新表与索引；
- `COLUMN_MIGRATIONS` 增加 `test_reports.report_title`；
- 迁移只做增量，不修改旧表结构；
- 旧报告没有 `report_url_stats` 数据时：
  - 页面显示“该报告生成于旧版本，未记录按 URL 统计”；
  - Excel 的 URL 统计页输出表头 + 说明行；
  - 不阻塞导出。

### 4.4 前端类型扩展

```ts
export interface UrlStat {
  method: string
  url: string
  statusCode: number
  requests: number
  successCount: number
  errorCount: number
  avgMs: number
  maxMs: number
  maxAtMs?: number | null
  totalHours: number
  avgKb: number
  totalMb: number
  errorMessage?: string | null
  statusSummary?: string
}

export interface ReportDetail {
  // 既有字段...
  urlStats?: UrlStat[]
  urlStatsComplete?: boolean
}
```

集群节点类型扩展：

```ts
export interface ClusterReportNode {
  // 既有字段...
  totalRequests?: number
  successCount?: number
  errorCount?: number
  avgRtMs?: number
  tps?: number
  egressAvgKbps?: number
}
```

所有新字段可选，旧报告与旧从节点缺省时显示“—”。

---

## 5. 压测引擎采集设计

### 5.1 为什么必须在引擎内采集

如果只在导出时查询 `report_requests`：

- `recordDetail=off` 时没有明细，URL 表为空；
- `recordDetail=sample` 时只统计 1% 样本，结果失真；
- 大报告需要把几十万条明细加载到内存，导出性能不可控。

因此 URL 聚合必须与请求完成回调同步计数，和是否记录明细无关。

### 5.2 聚合结构

```rust
#[derive(Hash, Eq, PartialEq, Clone)]
struct UrlKey {
    method: String,
    url: String,
    status: u16,
}

#[derive(Default, Clone)]
struct UrlAgg {
    requests: u64,
    success: u64,
    errors: u64,
    sum_ms: f64,
    max_ms: f64,
    max_at_ms: i64,
    bytes: u64,
    error_message: Option<String>,
    max_request_id: Option<String>,
}
```

### 5.3 并发与性能设计

- **分片锁**：`Vec<Mutex<HashMap<UrlKey, UrlAgg>>>`，按 `hash(key) % SHARDS` 分片，默认 16 片，降低单锁竞争；
- **更新路径**：请求完成时只做一次 `entry` 更新，不做字符串格式化、不写日志；
- **键数量上限**：全局最多 `URL_STATS_MAX_KEYS = 2000`；超过后写入 `__other__` 桶，并在报告标记 `urlStatsComplete=false`；
- **URL 长度**：入库前截断到 2048 字符，超出部分截断；
- **采样模式**：聚合不受采样影响；`recordDetail` 只控制逐请求明细；
- **性能目标**：10,000 QPS 下聚合开销小于 1%，压测主流程无额外锁等待毛刺；需用现有基准示例复测。

### 5.4 脱敏

URL 查询参数、错误信息可能包含 token、密码、手机号等敏感信息。

方案：

- 入库前对 URL 中的 `token / access_token / password / secret / authorization / cookie` 等参数值做掩码；
- 错误信息复用现有 `mask_sensitive` 能力，至少掩码 Bearer、sk-、password、token；
- 导出层再做一次兜底掩码，避免旧数据绕过；
- 报告中增加“URL 已脱敏”说明。

### 5.5 单机与集群

- 单机：引擎在报告收尾时输出最终 `Vec<UrlStat>`；
- 集群：每个从节点在 `done` 上报文中携带自己的 `urlStats`，主节点合并；
- 从节点不支持时：主节点只统计本机，报告标记 `urlStatsComplete=false`，并在施压机表标注“该节点未上报 URL 统计”。

---

## 6. 集群协议与合并设计

### 6.1 协议扩展

在节点完成上报中加入可选字段：

```rust
#[serde(default)]
pub url_stats: Vec<NodeUrlStat>,
```

`NodeUrlStat` 字段与 `report_url_stats` 一致（不含 report_id）。

协议版本从当前版本递增；主节点按能力位判断：

```rust
pub struct NodeCapabilities {
    // 既有...
    pub url_stats: bool,
}
```

旧节点缺省为 `false`。

### 6.2 合并规则

- 按 `(method, url, status)` 累加 requests / success / error / sum_ms / bytes；
- `max_ms` 取最大值，并保留对应 `max_at_ms` 与 `max_request_id`；
- `error_message` 保留第一条非空样例；
- 合并后再次执行键数量上限保护；
- 合并结果在报告生成事务内批量写入 `report_url_stats`。

### 6.3 施压机聚合字段

`ClusterReportNode` 增加：

| 字段 | 来源 |
| --- | --- |
| totalRequests | 节点稀疏指标 total |
| successCount / errorCount | 节点稀疏指标 success / errors |
| avgRtMs | sum_rt_ms / total |
| tps | total / 该节点施压时长 |
| egressAvgKbps | bytes_sent / 施压时长 / 1024 |

若节点未上报对应指标，字段为 `null`，导出显示“—”，不推断。

---

## 7. 后端接口与持久化

### 7.1 报告写入

`save_report` 事务内：

1. 写入 `test_reports`；
2. 写入采样、错误、请求明细；
3. 写入 `report_url_stats`（批量 prepare，单事务）；
4. 集群节点采样继续写入 `cluster_report_samples`。

批量写入需设置上限：单报告最多 2000 行 URL 统计，超出部分已归入 `__other__`。

### 7.2 报告读取

`report_get` 增加：

- 查询 `report_url_stats`，按 URL 聚合状态码后返回 `urlStats`；
- `errorUrls` 可由 `error_count > 0` 的聚合结果直接生成；
- `urlStatsComplete` 从 `config.metrics.urlStatsComplete` 读取；
- 旧报告无数据时返回空数组，不报错。

### 7.3 命令与 API

新增或扩展：

| 命令 | 说明 |
| --- | --- |
| `report_get` | 返回值增加 `urlStats`、`urlStatsComplete` |
| `report_export_xlsx` | 新增 URL 统计、错误地址、施压机详情三张表；请求/采样继续后端流式写入 |
| `report_update_texts` | 扩展 `title` 参数，允许修改报告标题 |
| `report_rerun`（可选） | 按报告关联的计划重新发起压测 |

### 7.4 Excel 生成

继续使用 `rust_xlsxwriter`：

- 汇总类工作表由前端传行数据；
- 「URL 统计」「错误地址」「施压机详情」可直接由后端查库生成，避免前端再传一遍；
- 「采样明细」「请求明细」保持后端流式写入；
- 全部工作表统一首行加粗、冻结首行、合理列宽。

---

## 8. 前端改造

### 8.1 报告详情页

新增区块：

1. 结果汇总卡：补“外网总流量（入+出）”“测试机数量”“并发间隔”；
2. URL 统计表：地址、方法、请求数、成功/失败、成功率、平均耗时、最长耗时、最长耗时时间、总耗时、平均包、总流量、状态码汇总；
3. 错误地址表：错误数、平均/最长耗时、最长耗时时间、错误信息；
4. 施压机详情表：IP、开始时间、总/成功/失败、并发、成功率、平均耗时、TPS、出站带宽平均；
5. 错误数曲线、并发曲线。

分析类内容：

- 「关键发现」「四维体检评分」不再常驻；
- 保留「AI 解读」按钮，按需生成，不写入导出报告；
- SLA、基线对比、峰值专题保留，属于事实数据。

### 8.2 导出调用

- HTML：由 `buildReportHtml` 生成新结构，不再调用 `buildAdvice / buildFindings / scoreReport / detectKnee`；
- Excel：`buildReportSheets` 不再生成「优化建议」，新增 URL/错误地址/施压机占位，由后端填充；
- 保持请求明细、采样明细后端流式写入，前端不再拉全量。

### 8.3 交互补齐

- 报告标题可编辑：新增输入框，保存到 `report_title`；
- 报告页「再次运行」：按报告关联的 `planId` 或配置快照跳转到压测页并预填；
- 页面底部免责声明：与 HTML 报告保持一致。

---

## 9. 导出结构详细设计

### 9.1 HTML 章节与数据来源

| 章节 | 数据来源 |
| --- | --- |
| 封面 | `report` + `config` |
| 01 结果汇总 | `report` + `qpsOverview` + `reportPeaks` |
| 02 配置 | `config` |
| 03 吞吐与并发 | `samples` |
| 04 响应时间 | `report` + `samples` + `histogram` + `bodyHistogram` |
| 05 错误与状态 | `samples.errorCount` + `statusCodes` + `errorTypes` + `errors` |
| 06 URL 与施压机 | `urlStats` + `config.cluster` |
| 07 采样摘要 | `samples` 说明 + `selfCheck` + 丢弃计数 |
| 附录 A/B/C | 口径、自检、免责声明 |

### 9.2 删除清单

| 删除项 | 位置 |
| --- | --- |
| 关键发现 | HTML 01、页面 |
| 优化建议 | HTML 07、Excel「优化建议」 |
| 四维体检/评分/雷达图 | HTML 06、页面 |
| 容量拐点/瓶颈结论 | HTML 06 |
| 压测结论/结论依据 | Excel 概览 |
| 扩展效率/吞吐增幅结论列 | 分阶段表（保留原始阶段指标） |

### 9.3 保留清单

- SLA 验收（有配置才显示）；
- 基线对比（用户选择基线时显示）；
- 峰值专题；
- 数据自洽校验；
- 指标口径定义；
- 分阶段原始指标（并发、时长、请求数、错误数、平均 QPS、平均 RT、P95）。

---

## 10. 兼容性与迁移

### 10.1 旧报告

- `report_url_stats` 为空：页面/导出显示“未记录”，不报错；
- `report_title` 为空：回退 `plan_name`；
- 旧集群报告 `config.cluster.nodeSeries` 已内嵌：保持原读取逻辑；
- 旧报告没有节点聚合字段：显示“—”。

### 10.2 旧从节点

- 新主节点 + 旧从节点：从节点不返回 URL 统计，主节点标记不完整；
- 报告施压机表标注“未上报 URL 统计”；
- 不阻塞任务执行与报告生成。

### 10.3 回滚

- 数据库迁移只增不减；
- 若导出结构调整需要回退，可在 `reportExport` 保留 `legacy` 参数，按需恢复旧章节；
- 新表数据不影响旧版本读取。

---

## 11. 性能、容量与稳定性

| 关注点 | 设计 |
| --- | --- |
| URL 聚合开销 | 分片锁 + O(1) 更新；10k QPS 目标开销 <1% |
| 内存 | 最多 2000 个键，超出归入 `__other__`；URL 截断 2048 字符 |
| 数据库写入 | 单报告最多 2000 行，批量写入同一事务 |
| 报告体积 | HTML 最多展示 500 个 URL，Excel 全量；采样/请求明细不内嵌 HTML |
| Excel 导出 | 后端流式生成，避免 `number[]` 过 IPC |
| 集群传输 | URL 统计按节点上报，主节点合并；单节点上限 2000 键 |
| 长压测 | 采样序列已有上限，URL 聚合不随时长线性增长（受 URL 数量上限约束） |

性能验收：

- 10,000 QPS 压测下，开启 URL 聚合前后的 QPS 偏差在 ±1% 内；
- 报告保存耗时增加不超过 5%；
- 导出 5 万条明细报告时，内存峰值不高于改造前。

---

## 12. 安全与合规

1. **敏感信息脱敏**：URL 参数、错误信息、请求头默认掩码；
2. **报告访问**：本地文件导出，不主动上传；如需分享，由用户自行控制；
3. **导出文件权限**：沿用现有 `prepare_write` 路径校验；
4. **免责声明**：HTML 附录、Excel 说明页、页面底部统一；
5. **数据最小化**：URL 统计只保留展示所需字段，不保存请求/响应体；
6. **审计**：建议记录 URL 统计溢出、节点未上报、旧报告缺失等事件到应用日志。

---

## 13. 可观测性与数据质量

报告应明确标注数据完整性：

- `urlStatsComplete`：是否存在 URL 键溢出或节点未上报；
- 采样覆盖率：采样点数、被剔除的升压/末点；
- 明细丢弃数、错误丢弃数；
- 旧报告缺失项统一显示“未记录”，不使用 0 代替；
- 报告内不出现“估算”“推断”的容量结论。

---

## 14. 测试策略

### 14.1 单元测试

- URL 聚合：并发更新、分片、键溢出、最大耗时及时间；
- 脱敏：URL 参数、错误信息；
- 集群合并：多节点累加、最大耗时选择、错误样例保留；
- 数据库：批量写入、旧报告空数据、迁移幂等。

### 14.2 集成测试

- 单机 full / sample / off 三种采集模式均能生成 URL 统计；
- 集群新旧节点混跑：报告标记不完整但不失败；
- 报告读取与导出：HTML/Excel 字段与数据库一致。

### 14.3 前端测试

- 导出 HTML 不包含“优化建议/关键发现/评分/结论”；
- 新表格渲染与空态；
- URL/施压机数据缺失时显示“—”；
- 分页、长 URL、窄屏样式。

### 14.4 端到端测试

- 单机压测 → 报告 → HTML/Excel 导出；
- 集群压测 → 合并报告 → 节点明细与 URL 统计；
- 旧报告打开与导出。

### 14.5 性能测试

- 使用现有基准示例与 `perf_node_series_24h_16_nodes` 同类方式，验证聚合开销；
- 大报告导出内存与耗时回归。

---

## 15. 验收标准

1. 外部 5 份报告的**全部可见字段**在 AstroMan 报告中均有对应位置；
2. 导出 HTML / Excel 不再包含优化建议、关键发现、评分、推断性结论；
3. 单机与集群、full/sample/off 模式下均可输出 URL 统计；
4. 施压机明细覆盖单机 1 行与集群 N 行，字段完整；
5. 旧报告、旧从节点不崩溃，缺失字段显示“未记录/—”；
6. 现有单测、Rust 测试、类型检查、构建、clippy、fmt 全部通过；
7. 10,000 QPS 下聚合开销 <1%，导出内存不高于改造前；
8. 敏感信息脱敏测试通过。

---

## 16. 实施阶段与任务拆分

### 阶段一：报告去主观化与结构重排

- `reportExport.ts`：删除建议/发现/评分/拐点/结论章节与 Excel 建议页；
- 调整 HTML 目录与章节编号；
- 补错误数曲线、并发曲线、总流量卡、测试机数量、并发间隔；
- 页面分析内容收敛到可选；
- 更新单测断言。

### 阶段二：URL 聚合

- 引擎新增 URL 聚合器与脱敏；
- 数据库 v23：`report_url_stats`、`report_title`；
- `save_report` 批量落库；
- `report_get` 返回 `urlStats`；
- 页面、HTML、Excel 三端渲染；
- 集群协议增加 `url_stats` 能力位与合并逻辑；
- 单元/集成测试。

### 阶段三：施压机明细

- `ClusterReportNode` 增加 totals/avgRt/tps/egress；
- 单机报告输出本机行；
- 页面与导出表；
- 旧节点缺失处理。

### 阶段四：交互与体验

- 报告标题编辑；
- 报告页再次运行；
- 页面免责声明；
- 导出说明页与口径文档同步。

### 阶段五： hardening

- 性能基准；
- 旧报告/旧节点兼容回归；
- 脱敏审计；
- 文档与 CHANGELOG 更新。

---

## 17. 风险与应对

| 风险 | 影响 | 应对 |
| --- | --- | --- |
| URL 基数爆炸 | 内存增长、报告膨胀 | 2000 键上限、溢出桶、URL 截断、报告标记不完整 |
| 分片锁竞争 | 高 QPS 下吞吐下降 | 分片、O(1) 更新、压测前后基准对比 |
| 旧从节点不返回 URL 统计 | 集群数据不完整 | 能力位、报告标注、不阻塞任务 |
| 旧报告无数据 | 页面/导出空表 | 明确“未记录”，不推断、不补 0 |
| 脱敏遗漏 | 敏感信息泄露 | 入库与导出双层掩码、测试用例覆盖 |
| 多 URL 能力缺失 | 无法完全复现外部 12 URL 场景 | 报告结构预留；多请求场景单独立项 |
| 导出结构变化影响用户习惯 | 阅读成本 | 保留章节编号映射、说明页、版本记录 |

---

## 18. 附录 A：报告字段清单（外部对齐）

| 外部字段 | AstroMan 字段/来源 | 状态 |
| --- | --- | --- |
| 标题 | `report_title` / `plan_name` | 新增 |
| 测试场景 | `scenarioText` | 已有 |
| 任务编号 | `report.id` | 已有 |
| 开始时间 | `report.startTime` | 已有 |
| 测试机数量 | 集群节点数 / 单机 1 | 新增展示 |
| 持续时间 | `start/end` | 已有 |
| 并发间隔 | `stages` / `rampUp` | 新增展示 |
| 思考时间 | `thinkTimeMs` | 已有 |
| 目标主机 | `config.url/target` | 已有 |
| 并发数量 | `concurrency` | 已有 |
| 总请求 | `totalRequests` | 已有 |
| 成功率 | `errorRate` | 已有 |
| 外网总流量 | `bytesTotal + bytesSent` | 新增展示 |
| 最大 QPS | `reportPeaks/qpsOverview` | 已有 |
| 90% QPS | `qpsOverview.ninety` | 已有 |
| QPS 曲线 | `samples.qps` | 已有 |
| 错误数曲线 | `samples.errorCount` | 新增 |
| 并发曲线 | `samples.concurrent` | 导出补齐 |
| 平均响应曲线 | `samples.avgRt` | 已有 |
| URL 平均响应对比 | `urlStats` | 新增 |
| HTTP 状态分布 | `statusCodes` | 已有 |
| 客户端 IP/开始时间 | `ClusterReportNode.addr/startMs` | 已有 |
| 客户端总/成功/失败 | `ClusterReportNode` 扩展 | 新增 |
| 客户端并发/成功率/平均耗时/TPS/出站带宽 | `ClusterReportNode` 扩展 | 新增 |
| URL 请求数/平均/最长/最长日期/总耗时/包大小 | `report_url_stats` | 新增 |
| 错误 URL 明细 | `report_url_stats` + error_message | 新增 |
| 报告总结 | `summaryText` | 已有 |
| 报告对比 | 报告列表对比 | 已有 |
| 再次运行 | 新增 | 新增 |
| 免责声明 | HTML/Excel/页面 | 补齐页面 |

## 19. 附录 B：指标口径

- 平均 QPS：后端按总请求 / 实际施压时长聚合；
- 峰值 QPS：每秒采样最大值；
- 90% 分布 QPS：稳态窗口内每秒 QPS 的 P10；
- 平均响应时间：所有请求耗时算术平均；
- URL 平均耗时：该 URL 所有请求耗时总和 / 请求数；
- URL 总耗时（小时）：该 URL 耗时总和 / 3600；
- URL 平均包大小：该 URL 响应字节总数 / 请求数 / 1024；
- 客户端平均耗时：该施压机 sum_rt / 请求数；
- 客户端出站带宽平均：该施压机发送字节 / 施压时长 / 1024；
- 成功率：成功请求数 / 总请求数；
- 错误 URL：`error_count > 0` 的 URL 统计行。

## 20. 附录 C：文档修订记录

| 版本 | 日期 | 修改内容 | 修改人 |
| --- | --- | --- | --- |
| v1.0 | 2026-11-23 | 初版：报告去主观化、URL/施压机聚合、导出结构、兼容与测试方案 | AstroMan 团队 |

---

> 本方案为报告重构的实施基线。阶段一可不改数据协议先落地；阶段二、三涉及引擎与数据库变更，需按本文的兼容、性能和测试要求执行。
