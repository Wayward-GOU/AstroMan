//! 主从消息与共享数据结构
//!
//! 传输为 WebSocket + JSON（内网明文，一期不做 wss）。所有消息带 `type` 标签，
//! 字段统一 camelCase，便于前端快照直接复用。时钟与采样都用毫秒时间戳，
//! 避免跨机 `Instant` 不可比的问题。

// W0 先落地协议与快照模型，W2 / W3 接入后自然被使用；
// 增量期间保留 allow，避免 CI 的 clippy -D warnings 因「尚未接线」失败。
#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 协议版本：主从版本不一致时从节点拒绝加入，避免字段缺省导致误算
pub const PROTOCOL_VERSION: u32 = 2;
/// 默认监听端口
pub const DEFAULT_PORT: u16 = 17801;
/// 端口自动递增上限：17801 → 17811..17860，仍占用则交给系统分配临时端口
pub const AUTO_PORT_END: u16 = 17860;
/// 局域网发现广播/监听端口（UDP）
pub const DISCOVERY_PORT: u16 = 17802;
/// EnvBundle 加密版本
pub const ENV_BUNDLE_VERSION: u8 = 1;
/// 采集健康度：首样超时分级（秒）
pub const FIRST_SAMPLE_WARN_SEC: u64 = 5;
pub const FIRST_SAMPLE_BAD_SEC: u64 = 10;
/// 配对码有效期（秒）：默认 5 分钟，创建任务时可在上下限内调整
pub const PAIR_CODE_TTL_SEC: i64 = 300;
/// 配对码有效期下限：1 分钟
pub const MIN_PAIR_CODE_TTL_SEC: i64 = 60;
/// 配对码有效期上限：1 小时
pub const MAX_PAIR_CODE_TTL_SEC: i64 = 3600;
/// 配对码位数
pub const PAIR_CODE_DIGITS: usize = 6;
/// 心跳间隔（秒）
pub const HEARTBEAT_SEC: u64 = 2;
/// 连续丢失心跳次数达到该值判定失联
pub const HEARTBEAT_MISSES: u32 = 3;
/// 时钟对齐往返采样轮数
pub const CLOCK_SAMPLE_ROUNDS: u32 = 5;
/// 时钟对齐误差超过该值（毫秒）时丢弃采样
pub const CLOCK_DROP_UNCERTAINTY_MS: f64 = 200.0;
/// 单个 WebSocket 消息上限：done 会携带三组稀疏直方图
pub const MAX_MESSAGE_BYTES: usize = 8 * 1024 * 1024;
/// 单节点池切片上限（12.5）：超出直接拒绝并提示缩小池条目或增加节点
pub const MAX_POOL_SLICE_BYTES: usize = 64 * 1024 * 1024;
/// 从节点接收 Prepare 的上限：池切片上限 + 配置/环境变量的常规余量
pub const MAX_PREPARE_BYTES: usize = MAX_POOL_SLICE_BYTES + MAX_MESSAGE_BYTES;
/// 一期单任务最多从节点数，避免主节点被大量连接拖垮
pub const MAX_NODES: usize = 16;
/// 已完成集群快照保留上限：最多保留最近 20 个任务
pub const RETENTION_MAX_TASKS: usize = 20;
/// 已完成集群快照保留时长（秒）：超过 30 分钟自动清理
pub const RETENTION_SEC: i64 = 30 * 60;

/// 供前端展示的协议参数（W0 占位页与设置项共用）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClusterProtocolInfo {
    pub version: u32,
    pub default_port: u16,
    pub pair_code_ttl_sec: i64,
    pub min_pair_code_ttl_sec: i64,
    pub max_pair_code_ttl_sec: i64,
    pub pair_code_digits: usize,
    pub heartbeat_sec: u64,
    pub heartbeat_misses: u32,
    pub clock_rounds: u32,
    pub clock_drop_uncertainty_ms: f64,
    pub max_nodes: usize,
}

impl Default for ClusterProtocolInfo {
    fn default() -> Self {
        Self {
            version: PROTOCOL_VERSION,
            default_port: DEFAULT_PORT,
            pair_code_ttl_sec: PAIR_CODE_TTL_SEC,
            min_pair_code_ttl_sec: MIN_PAIR_CODE_TTL_SEC,
            max_pair_code_ttl_sec: MAX_PAIR_CODE_TTL_SEC,
            pair_code_digits: PAIR_CODE_DIGITS,
            heartbeat_sec: HEARTBEAT_SEC,
            heartbeat_misses: HEARTBEAT_MISSES,
            clock_rounds: CLOCK_SAMPLE_ROUNDS,
            clock_drop_uncertainty_ms: CLOCK_DROP_UNCERTAINTY_MS,
            max_nodes: MAX_NODES,
        }
    }
}

/// 1ms 稀疏直方图：只传非零桶，`(毫秒, 次数)`
pub type SparseBuckets = Vec<(u32, u64)>;

/* ---------- 主从消息 ---------- */

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ClusterMessage {
    Hello(HelloPayload),
    HelloAck(HelloAckPayload),
    /// 申请加入已登记，等待主节点用户接受（审批模式）
    HelloPending(HelloPendingPayload),
    /// 加入申请被拒绝
    HelloRejected(HelloRejectedPayload),
    TimeProbe(TimeProbePayload),
    TimeAck(TimeAckPayload),
    Prepare(PreparePayload),
    PrepareAck(PrepareAckPayload),
    /// 从节点请求主节点单飞刷新一条 token（8.10.5）
    PoolRefreshRequest(PoolRefreshRequestPayload),
    /// 主节点回传刷新结果：新值与过期时间用会话密钥加密
    PoolRefreshResult(PoolRefreshResultPayload),
    Preflight(PreflightPayload),
    Start(StartPayload),
    Sample(SamplePayload),
    Done(DonePayload),
    Abort(AbortPayload),
    Ping(PingPayload),
    Pong(PongPayload),
    Error(ErrorPayload),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HelloPayload {
    pub version: u32,
    /// 配对码：审批模式下为空
    #[serde(default)]
    pub pair_code: String,
    pub node_name: String,
    /// 加入方式：code（配对码）/ request（发现后申请，需主节点审批）/ invite（主节点邀请）
    #[serde(default)]
    pub join_mode: Option<String>,
    /// 邀请令牌：joinMode=invite 时携带，主节点校验后直接放行
    #[serde(default)]
    pub invite_token: Option<String>,
    /// 设备指纹：用于审批展示与白名单
    #[serde(default)]
    pub device_id: Option<String>,
    /// 从节点本机能力检测的饱和并发（可选，仅用于启动前提示）
    #[serde(default)]
    pub capacity_concurrent: Option<u32>,
    /// 本机节点能力（核数/内存/预算），供主节点做能力感知分配
    #[serde(default)]
    pub node_capacity: Option<NodeCapacity>,
    /// X25519 临时公钥（base64），用于 EnvBundle 会话密钥协商
    #[serde(default)]
    pub public_key: Option<String>,
    /// 是否支持池自定义列 / 自动注入（v20）；旧节点缺省 false
    #[serde(default)]
    pub pool_columns: bool,
    /// 是否支持运行期时钟复测；旧节点缺省 false，主节点不会在施压中打扰它
    #[serde(default)]
    pub clock_recheck: bool,
    /// 是否支持在 done 中上报按 URL 聚合统计；旧节点缺省 false
    #[serde(default)]
    pub url_stats: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HelloAckPayload {
    pub version: u32,
    /// 配对码单次使用：配对成功后只认令牌
    pub token: String,
    pub node_id: String,
    pub job_id: i64,
    /// 主节点 X25519 临时公钥（base64）
    #[serde(default)]
    pub public_key: Option<String>,
    /// 主节点是否支持统一刷新 token；旧主节点缺省 false，从节点退回本地单飞
    #[serde(default)]
    pub pool_refresh_relay: bool,
    /// 主节点是否支持池自定义列（v20）；旧主节点缺省 false
    #[serde(default)]
    pub pool_columns: bool,
}

/// 节点能力：用于分配阶段按算力加权
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeCapacity {
    pub cores: u32,
    pub mem_gb: f64,
    /// 该节点本机并发预算
    pub machine_limit: u32,
}

/// 加入申请已登记：从节点等待主节点用户接受
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HelloPendingPayload {
    /// 申请 ID：与主节点待接受列表对应
    pub request_id: String,
    #[serde(default)]
    pub message: Option<String>,
    /// 审批最长等待秒数，超时按拒绝处理
    #[serde(default)]
    pub timeout_sec: u64,
}

/// 加入申请被拒绝
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HelloRejectedPayload {
    pub reason: String,
}

/// 主节点待接受节点：前端审批卡片
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingNodeInfo {
    pub request_id: String,
    pub node_id: String,
    pub name: String,
    pub addr: String,
    pub device_id: Option<String>,
    pub capacity: Option<NodeCapacity>,
    pub requested_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimeProbePayload {
    pub seq: u32,
    /// 主节点发送时刻（主节点时钟）
    pub t0: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimeAckPayload {
    pub seq: u32,
    pub t0: i64,
    /// 从节点收到时的本机时刻
    pub t1: i64,
}

/// 下发执行配置：只传模板与变量名，变量值由各节点本地解析
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreparePayload {
    pub job_id: i64,
    pub plan_name: String,
    /// StressConfig 的 JSON 形态；envId 已清空，auth 仅保留变量引用
    pub config: serde_json::Value,
    /// 环境名：从节点按名称在本地库中查找，找不到时以 EnvBundle 为准
    pub env_name: Option<String>,
    /// 变量合并策略：master_wins（默认）/ local_wins
    #[serde(default = "default_var_policy")]
    pub var_policy: String,
    /// 配置里出现的变量名（已排除内置/函数/OS/池变量），用于一次性报出全部缺失项
    pub required_vars: Vec<String>,
    /// 分配到的并发
    pub concurrent: u32,
    /// 集群总并发（展示用）
    pub total_concurrent: u32,
    /// 主节点加密下发的环境变量包（含主节点独有变量与 secret 明文）
    #[serde(default)]
    pub env_bundle: Option<EnvBundle>,
    /// 本节点负责的全局 VU 起始编号（池按 VU 固定身份时使用）
    #[serde(default)]
    pub vu_start: u32,
    /// 本节点负责的 VU 数量
    #[serde(default)]
    pub vu_count: u32,
    /// 参数/Token 池分片（主节点切片，保证不跨机重复）
    #[serde(default)]
    pub pool_slices: Vec<PoolSlice>,
}

/// 变量策略缺省值：主节点优先
fn default_var_policy() -> String {
    "master_wins".to_string()
}

/// 加密后的环境变量包
///
/// 明文为 `{"vars":{...},"secrets":{...},"osVars":[...]}` 的 JSON；
/// 密钥 = X25519(master_sk, slave_pk) → HKDF-SHA256 → 32 字节 AES-256-GCM 密钥。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvBundle {
    /// 版本，当前为 1
    pub version: u8,
    /// 环境名（仅用于展示与诊断，不参与解密）
    pub env_name: Option<String>,
    /// AES-GCM nonce（base64，12 字节）
    pub nonce: String,
    /// 密文（base64）
    pub payload: String,
    /// 明文的 SHA-256（hex），用于解密后校验与日志比对
    pub digest: String,
    /// 包内出现的 secret 变量名，供前端脱敏与诊断
    pub secret_names: Vec<String>,
}

/// 池分片：一个池在本节点可用的条目区间
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PoolSlice {
    pub name: String,
    /// 池类型：parameter / token
    pub kind: String,
    /// 本节点负责的条目
    pub items: Vec<PoolItem>,
    /// 池内全局起始下标
    pub start_index: usize,
    /// 池列定义（v20）：顺序与用户定义一致；旧主节点缺省为空
    #[serde(default)]
    pub columns: Vec<String>,
    /// Token 刷新配置（v19）：由主节点统一单飞刷新，从节点通过
    /// `PoolRefreshRequest` 请求结果，避免多节点轮换 `refreshToken` 互相失效（8.10.5）
    #[serde(default)]
    pub refresh: Option<crate::services::pool::PoolRefreshConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PoolItem {
    pub id: i64,
    /// 参数值或 token 原文（仅在内网点对点下发）
    pub value: String,
    /// token 池专用：账号/备注
    #[serde(default)]
    pub account: Option<String>,
    #[serde(default)]
    pub weight: f64,
    /// 过期时间（毫秒时间戳，0 = 不过期）；v19
    #[serde(default)]
    pub expires_at: i64,
    /// 整行数据（v20）：列名 -> 值；旧主节点缺省为空
    #[serde(default)]
    pub data: HashMap<String, String>,
}

/// 从节点向主节点请求刷新一条 token
///
/// 主节点按 `(pool, item_id)` 加锁单飞，避免多节点同时轮换 refreshToken 互相失效。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PoolRefreshRequestPayload {
    /// 请求标识：由从节点生成，用于把结果派发给等待的 VU
    pub request_id: String,
    /// 池名
    pub pool: String,
    /// 池条目 id：主从切片后同一条目在所有节点上 id 一致
    pub item_id: i64,
    /// 条目在节点本地的下标（仅用于日志与诊断）
    #[serde(default)]
    pub item_index: usize,
}

/// 主节点回传的刷新结果
///
/// 成功时 `nonce/payload/digest` 是加密后的 `{"value":..,"expiresAt":..}`；
/// 失败只回错误文案，绝不回传 token。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PoolRefreshResultPayload {
    pub request_id: String,
    pub ok: bool,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub nonce: String,
    #[serde(default)]
    pub payload: String,
    #[serde(default)]
    pub digest: String,
    /// 本次刷新写入的列名（v20）：从节点据此同步 item.data；旧主节点缺省为空
    #[serde(default)]
    pub column: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrepareAckPayload {
    pub ok: bool,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub missing_vars: Vec<String>,
    /// 从节点实际接受的并发（受本机上限约束）
    #[serde(default)]
    pub accepted_concurrent: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreflightPayload {
    pub ok: bool,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartPayload {
    /// 统一起跑时刻（主节点时钟毫秒）
    pub at_ms: i64,
    /// 从节点时钟相对主节点的偏移（毫秒，正数表示从节点更快）
    ///
    /// 从节点用 `atMs + offsetMs` 换算成本机起跑时刻；缺省 0 兼容旧节点。
    #[serde(default)]
    pub offset_ms: f64,
}

/// 从节点资源快照：跟随每秒采样上行，主节点据此展示与归档
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostSample {
    /// 施压进程占用的核数（与单机报告口径一致）
    pub cpu: f64,
    /// 整机 CPU 使用率（0~100）：识别机器被其他进程拖慢
    #[serde(default)]
    pub host_cpu_percent: f64,
    /// 施压进程 RSS（MB）
    pub mem_mb: u64,
    /// 整机内存使用率（0~100）
    #[serde(default)]
    pub host_mem_percent: f64,
    /// 进程线程数
    #[serde(default)]
    pub threads: u32,
    /// 网卡接收速率（KB/s）
    #[serde(default)]
    pub net_rx_kbps: f64,
    /// 网卡发送速率（KB/s）
    #[serde(default)]
    pub net_tx_kbps: f64,
    /// 1 分钟平均负载（Windows 为 0）
    #[serde(default)]
    pub load1: f64,
    /// 文件描述符 / 句柄数，取不到为 0
    #[serde(default)]
    pub fd_count: u32,
}

/// 每秒采样：计数用于合并曲线，稀疏桶用于重算每秒分位
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SamplePayload {
    /// 相对统一起跑点的秒数
    pub elapsed_sec: u64,
    pub concurrent: u32,
    pub qps: f64,
    pub avg_rt: f64,
    pub ttfb_avg: f64,
    pub body_avg: f64,
    pub bytes: u64,
    pub success_count: u64,
    pub error_count: u64,
    /// 本秒有响应的请求数（TTFB / 响应体均值分母）；旧节点为 0
    #[serde(default)]
    pub response_count: u64,
    /// 本秒总耗时累加，用于跨节点加权平均
    pub sum_rt_ms: u64,
    /// 本秒 TTFB 累加：节点失联时没有 done，只能用每秒累加还原 TTFB 均值
    #[serde(default)]
    pub sum_ttfb_ms: u64,
    /// 本秒响应体耗时累加（仅响应请求）
    #[serde(default)]
    pub sum_body_ms: u64,
    pub latency_buckets: SparseBuckets,
    /// 本秒响应体耗时的稀疏桶：主节点据此重算逐秒 Body P95；
    /// 旧节点为空，合并时该字段按 0 处理
    #[serde(default)]
    pub body_buckets: SparseBuckets,
    /// 本机资源快照；旧节点为 None
    #[serde(default)]
    pub host: Option<HostSample>,
}

/// 从节点最终结果：合并报告的唯一输入
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DonePayload {
    /// 实际起跑 / 结束时刻（已按主节点时钟校正）
    pub start_ms: i64,
    pub end_ms: i64,
    pub total: u64,
    pub success: u64,
    pub errors: u64,
    /// 有响应的请求数；旧节点为 0，合并时回退到桶计数
    #[serde(default)]
    pub responses: u64,
    pub bytes: u64,
    pub bytes_sent: u64,
    pub sum_rt_ms: u64,
    pub sum_ttfb_ms: u64,
    /// 仅响应请求的响应体耗时累加；旧节点为 0
    #[serde(default)]
    pub sum_body_ms: u64,
    /// 指标口径版本：2 = 响应数 / Body 累加 / APDEX 分子均已上报
    #[serde(default)]
    pub metrics_version: u32,
    /// APDEX 分子计数（只含成功请求）；旧节点为 0
    #[serde(default)]
    pub apdex_satisfied: u64,
    #[serde(default)]
    pub apdex_tolerating: u64,
    pub min_rt: f64,
    pub max_rt: f64,
    pub latency_buckets: SparseBuckets,
    pub ttfb_buckets: SparseBuckets,
    pub body_buckets: SparseBuckets,
    /// (状态码, 次数)
    pub status_codes: Vec<(u16, u64)>,
    /// (错误类型, 次数)
    pub error_types: Vec<(String, i64)>,
    pub cpu_peak: f64,
    pub mem_peak_mb: u64,
    pub stopped: bool,
    pub errors_dropped: u64,
    pub records_dropped: u64,
    /// 因时钟误差过大被丢弃的采样数
    pub dropped_samples: u64,
    /// 本节点的池运行统计（不含明文）；旧版本节点可缺省
    #[serde(default)]
    pub pools: Vec<serde_json::Value>,
    /// 本节点按 URL 聚合的统计（已脱敏）；旧版本节点缺省为空
    #[serde(default)]
    pub url_stats: Vec<crate::models::UrlStatRow>,
    /// 本节点的 URL 统计是否完整；旧节点缺省为 false，主节点据此标记不完整
    #[serde(default)]
    pub url_stats_complete: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AbortPayload {
    #[serde(default)]
    pub reason: Option<String>,
}

/// 引擎累计计数：心跳携带，用于丢样兜底与采集健康度判断
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineStats {
    /// 任务已运行秒数（0 表示尚未起跑或已结束）
    pub elapsed_sec: u64,
    pub concurrent: u32,
    pub total: u64,
    pub success: u64,
    pub errors: u64,
    pub qps: f64,
    pub bytes: u64,
    pub cpu: f64,
    pub mem_mb: u64,
}

/// 采集健康度：把「没采到」和「采到了但没显示」区分开
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SampleHealth {
    /// ok / waiting / gap / lost / done
    pub state: String,
    pub first_sample_ms: Option<i64>,
    pub last_sample_ms: Option<i64>,
    /// 期望采样数（按已运行秒数）
    pub expected: u64,
    /// 实际收到采样数
    pub received: u64,
    /// 丢失采样数
    pub lost: u64,
    /// 最近一次连续缺口秒数
    pub gap_sec: u64,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PingPayload {
    pub at_ms: i64,
    /// 累计引擎计数：主节点用它补齐丢样并判断引擎是否仍在跑
    #[serde(default)]
    pub stats: Option<EngineStats>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PongPayload {
    pub at_ms: i64,
    #[serde(default)]
    pub stats: Option<EngineStats>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorPayload {
    pub message: String,
}

/* ---------- 前端快照 ---------- */

/// 节点在线状态：pairing/pairReady/preparing/preflight/ready/running/done/lost/left/rejected
pub type ClusterNodeStatus = String;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClusterNodeInfo {
    pub id: String,
    pub name: String,
    /// master / slave
    pub role: String,
    pub addr: String,
    pub weight: f64,
    pub concurrent: u32,
    pub status: ClusterNodeStatus,
    pub qps: f64,
    pub cpu: Option<f64>,
    pub mem_mb: Option<u64>,
    /// 整机 CPU 使用率（%），旧节点为 None
    #[serde(default)]
    pub host_cpu_percent: Option<f64>,
    /// 整机内存使用率（%）
    #[serde(default)]
    pub host_mem_percent: Option<f64>,
    /// 网卡接收速率（KB/s）
    #[serde(default)]
    pub net_rx_kbps: Option<f64>,
    /// 网卡发送速率（KB/s）
    #[serde(default)]
    pub net_tx_kbps: Option<f64>,
    /// 进程线程数
    #[serde(default)]
    pub threads: Option<u32>,
    /// 1 分钟平均负载
    #[serde(default)]
    pub load1: Option<f64>,
    /// 从节点时钟相对主节点的偏移（毫秒，正数表示从节点更快）
    pub clock_offset_ms: Option<f64>,
    pub clock_uncertainty_ms: Option<f64>,
    /// 运行期复测的漂移（毫秒）：相对配对时刻偏移的变化量
    #[serde(default)]
    pub clock_drift_ms: f64,
    /// 最近一次复测时刻
    #[serde(default)]
    pub clock_recheck_at: Option<i64>,
    pub dropped_samples: u64,
    pub joined_at: i64,
    pub last_seen_at: Option<i64>,
    pub error: Option<String>,
    /// 节点能力（核数/内存/预算）
    pub capacity: Option<NodeCapacity>,
    /// 该节点被分配的并发上限（min/max 与预算共同决定）
    pub quota: u32,
    /// 采集健康度
    pub health: SampleHealth,
    /// 本任务运行期该节点峰值
    pub peak_qps: f64,
    pub peak_concurrent: u32,
    /// 本次配对以来整机 CPU 峰值（%）
    #[serde(default)]
    pub peak_host_cpu_percent: Option<f64>,
    /// 本次配对以来整机内存峰值（%）
    #[serde(default)]
    pub peak_host_mem_percent: Option<f64>,
    /// 本次配对以来网卡收发合计峰值（KB/s）
    #[serde(default)]
    pub peak_net_kbps: Option<f64>,
    /// 全局 VU 区间
    pub vu_start: u32,
    pub vu_count: u32,
    /// 最近一次心跳回传的引擎累计计数（诊断用）
    #[serde(default)]
    pub engine_stats: Option<EngineStats>,
    /// 最近一次 PrepareAck 报告缺失的变量名（含 `os:NAME`），供变量体检展示
    #[serde(default)]
    pub missing_vars: Vec<String>,
    /// 从节点是否支持池自定义列 / 自动注入（v20）
    #[serde(default)]
    pub pool_columns: bool,
    /// 是否提供加密公钥：旧节点为 false 时只能读取本机同名环境
    #[serde(default)]
    pub env_bundle: bool,
    /// 本次下发给该节点的池切片序列化字节数（prepare 后才有）
    #[serde(default)]
    pub pool_slice_bytes: Option<u64>,
}

/// 数据池体量预估：创建任务时按池条目算一次，供下发前提示
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PoolSizeEstimate {
    /// 参与的池数量（同池多个绑定只算一次）
    pub pools: u32,
    /// 条目总数
    pub items: u64,
    /// 估算的切片字节数（含每行的 JSON 与字段开销）
    pub bytes: u64,
}

/// 单节点在某一秒的图表字段
///
/// 只带界面需要的值：直方图桶留在后端做合并与 AI 诊断，避免每秒把桶发给前端。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeSampleLite {
    pub node_id: String,
    pub qps: f64,
    pub concurrent: u32,
    pub avg_rt: f64,
    pub error_count: u64,
    #[serde(default)]
    pub cpu: Option<f64>,
    #[serde(default)]
    pub mem_mb: Option<u64>,
}

/// 采样帧：每秒一帧，把该秒所有节点的图表字段一起下发
///
/// 16 个节点时 IPC 从每秒 16 次降到 1 次，主节点不用为每条采样做一次序列化。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClusterSampleFrame {
    pub job_id: i64,
    pub sec: u64,
    pub nodes: Vec<NodeSampleLite>,
}

/// 主节点任务快照：前端每秒拉取 / 订阅事件
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClusterMasterSnapshot {
    pub job_id: i64,
    pub name: String,
    pub plan_id: i64,
    pub target: String,
    pub method: String,
    /// pairing / preparing / running / finished / failed
    pub status: String,
    pub port: u16,
    pub pair_code: Option<String>,
    pub pair_code_expires_at: Option<i64>,
    /// 本任务创建时选定的配对码有效期（秒），每次配对成功后按此值刷新
    pub pair_code_ttl_sec: i64,
    pub local_addresses: Vec<String>,
    pub total_concurrent: u32,
    /// 分配模式：auto / even / custom
    pub allocation_mode: String,
    /// 本机并发预算（主节点）
    pub budget: u32,
    pub start_at_ms: Option<i64>,
    pub elapsed_sec: u64,
    pub report_id: Option<i64>,
    pub incomplete: bool,
    pub message: Option<String>,
    pub nodes: Vec<ClusterNodeInfo>,
    /// 待接受节点（审批模式）
    #[serde(default)]
    pub pending: Vec<PendingNodeInfo>,
    /// 本次任务是否自动接受加入申请
    #[serde(default)]
    pub auto_accept: bool,
    /// 局域网发现是否开启（关闭后仍可手动配对）
    #[serde(default)]
    pub discovery: bool,
    /// 数据池体量预估；无池绑定时为 None
    #[serde(default)]
    pub pool_estimate: Option<PoolSizeEstimate>,
    /// 单节点池切片字节上限（前端按它显示占用比例）
    #[serde(default)]
    pub pool_slice_limit_bytes: u64,
    /// 本任务发出的设备邀请（等待对方确认 / 已接受 / 已过期）
    #[serde(default)]
    pub invites: Vec<super::presence::MasterInvite>,
    pub created_at: i64,
}

/// 变量体检：单个从节点变量的状态
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VarAuditNodeState {
    /// ok / missing / pending / unknown
    pub state: String,
    pub detail: Option<String>,
}

/// 变量体检：表格里的一行
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VarAuditItem {
    pub name: String,
    /// user / secret / os / pool
    pub kind: String,
    /// 主节点环境里是否有值（os / pool 不参与环境下发，固定 false）
    pub master_has_value: bool,
    /// 取值来源：env / pool / os / none
    pub source: String,
    /// 缺失原因或使用说明
    pub message: Option<String>,
    /// 从节点状态：nodeId → 状态
    pub node_states: std::collections::HashMap<String, VarAuditNodeState>,
}

/// 变量体检：只读报告，不含任何变量值
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VarAuditReport {
    pub job_id: i64,
    pub env_name: Option<String>,
    /// master_wins / local_wins
    pub var_policy: String,
    /// 主节点环境变量数量 / 其中 secret 数量
    pub env_var_count: u32,
    pub secret_count: u32,
    /// 参与体检的从节点（按加入顺序）
    pub nodes: Vec<VarAuditNode>,
    pub items: Vec<VarAuditItem>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VarAuditNode {
    pub id: String,
    pub name: String,
    pub addr: String,
    pub status: String,
}

/// 从节点会话快照
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClusterSlaveSnapshot {
    /// idle / connecting / preparing / ready / running / done / lost / left / rejected
    pub status: String,
    pub master_addr: String,
    pub node_name: String,
    pub node_id: Option<String>,
    pub plan_name: Option<String>,
    pub target: Option<String>,
    pub method: Option<String>,
    pub concurrent: u32,
    pub start_at_ms: Option<i64>,
    pub elapsed_sec: u64,
    pub qps: f64,
    pub error: Option<String>,
    pub missing_vars: Vec<String>,
    pub joined_at: Option<i64>,
    /// 从节点引擎累计计数（心跳回传）
    pub engine_stats: Option<EngineStats>,
}

/// 写入报告 config_json.cluster 的元数据
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClusterReportMeta {
    pub protocol_version: u32,
    pub master_node: String,
    pub total_concurrent: u32,
    /// 有节点失联或数据缺失时为 true
    pub incomplete: bool,
    pub started_at: i64,
    pub finished_at: i64,
    pub dropped_samples: u64,
    pub nodes: Vec<ClusterReportNode>,
    /// 峰值指标专题：合并曲线上的峰值、出现时刻与平台期
    #[serde(default)]
    pub peaks: Option<ReportPeaks>,
    /// 节点级时间序列（已抽稀），用于定位掉速节点
    #[serde(default)]
    pub node_series: Vec<NodeSeries>,
    #[serde(default)]
    pub note: Option<String>,
}

/// 单个峰值指标
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeakMetric {
    pub value: f64,
    /// 出现时刻（主节点时钟毫秒）
    pub at_ms: i64,
    /// 相对起跑点秒数
    pub second: u64,
    /// 平台期持续秒数（≥ 峰值 98% 的连续区间）
    pub held_sec: u64,
    /// 贡献最大的节点名
    #[serde(default)]
    pub top_node: Option<String>,
}

/// 报告峰值专题
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportPeaks {
    pub qps: PeakMetric,
    pub concurrent: PeakMetric,
    /// 发送+接收带宽峰值（字节/秒）
    pub bandwidth: PeakMetric,
    pub p95: PeakMetric,
    pub p99: PeakMetric,
    /// 错误率峰值（0~1）
    pub error_rate: PeakMetric,
}

/// 节点级序列
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeSeries {
    pub node: String,
    pub role: String,
    pub points: Vec<SeriesPoint>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeriesPoint {
    pub sec: u64,
    pub qps: f64,
    pub concurrent: u32,
    pub p95: f64,
    pub errors: u64,
    /// 施压进程占用核数（旧报告为 0）
    #[serde(default)]
    pub cpu: f64,
    /// 整机 CPU 使用率（%）
    #[serde(default)]
    pub host_cpu_percent: f64,
    /// 进程 RSS（MB）
    #[serde(default)]
    pub mem_mb: u64,
    /// 网卡接收 / 发送速率（KB/s）
    #[serde(default)]
    pub net_rx_kbps: f64,
    #[serde(default)]
    pub net_tx_kbps: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClusterReportNode {
    pub name: String,
    pub role: String,
    pub addr: String,
    pub weight: f64,
    pub concurrent: u32,
    pub status: String,
    pub qps_avg: f64,
    pub cpu_peak: f64,
    pub mem_peak_mb: u64,
    /// 整机 CPU 使用率峰值（%）
    #[serde(default)]
    pub host_cpu_peak: f64,
    /// 整机内存使用率峰值（%）
    #[serde(default)]
    pub host_mem_peak_percent: f64,
    /// 网卡收发合计峰值（KB/s）
    #[serde(default)]
    pub net_peak_kbps: f64,
    pub clock_offset_ms: f64,
    pub clock_uncertainty_ms: f64,
    /// 运行期时钟复测得到的漂移（毫秒）
    #[serde(default)]
    pub clock_drift_ms: f64,
    pub dropped_samples: u64,
    pub start_ms: i64,
    pub end_ms: i64,
    /// 运行期峰值
    #[serde(default)]
    pub peak_qps: f64,
    #[serde(default)]
    pub peak_concurrent: u32,
    /// 请求量贡献占比（0~1）
    #[serde(default)]
    pub share: f64,
    /// 采集健康度快照
    #[serde(default)]
    pub health: SampleHealth,
    /// 节点总请求数：旧报告 / 旧节点缺失时为 None，展示「—」，不做推断
    #[serde(default)]
    pub total_requests: Option<u64>,
    /// 成功 / 失败请求数
    #[serde(default)]
    pub success_count: Option<u64>,
    #[serde(default)]
    pub error_count: Option<u64>,
    /// 客户端平均耗时：该节点 sum_rt / 请求数
    #[serde(default)]
    pub avg_rt_ms: Option<f64>,
    /// TPS：总请求数 / 该节点施压时长
    #[serde(default)]
    pub tps: Option<f64>,
    /// 出站带宽平均：发送字节 / 施压时长 / 1024（KB/s）
    #[serde(default)]
    pub egress_avg_kbps: Option<f64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_uses_type_tag_and_camel_case() {
        let message = ClusterMessage::Start(StartPayload {
            at_ms: 1_700_000_000_000,
            offset_ms: 0.0,
        });
        let text = serde_json::to_string(&message).unwrap();
        assert!(text.contains("\"type\":\"start\""), "{text}");
        assert!(text.contains("\"atMs\""), "{text}");

        let back: ClusterMessage = serde_json::from_str(&text).unwrap();
        match back {
            ClusterMessage::Start(payload) => assert_eq!(payload.at_ms, 1_700_000_000_000),
            other => panic!("unexpected message: {other:?}"),
        }
    }

    #[test]
    fn done_payload_roundtrip_keeps_sparse_buckets() {
        let payload = DonePayload {
            start_ms: 1,
            end_ms: 2,
            total: 3,
            success: 2,
            errors: 1,
            responses: 3,
            bytes: 4,
            bytes_sent: 5,
            sum_rt_ms: 6,
            sum_ttfb_ms: 7,
            sum_body_ms: 2,
            metrics_version: 2,
            apdex_satisfied: 2,
            apdex_tolerating: 0,
            min_rt: 1.0,
            max_rt: 9.0,
            latency_buckets: vec![(1, 2), (9, 1)],
            ttfb_buckets: vec![(0, 3)],
            body_buckets: vec![(2, 1)],
            status_codes: vec![(200, 2), (500, 1)],
            error_types: vec![("timeout".into(), 1)],
            cpu_peak: 0.5,
            mem_peak_mb: 128,
            stopped: false,
            errors_dropped: 0,
            records_dropped: 0,
            dropped_samples: 0,
            pools: Vec::new(),
            url_stats: Vec::new(),
            url_stats_complete: true,
        };
        let text = serde_json::to_string(&payload).unwrap();
        let back: DonePayload = serde_json::from_str(&text).unwrap();
        assert_eq!(back.latency_buckets, vec![(1, 2), (9, 1)]);
        assert_eq!(back.status_codes, vec![(200, 2), (500, 1)]);
    }

    #[test]
    fn old_cluster_node_json_defaults_new_load_metrics_to_none() {
        // 旧报告 config.cluster.nodes：没有 v23 的施压机明细字段，读取时保持 None
        let node: ClusterReportNode = serde_json::from_value(serde_json::json!({
            "name": "old-node",
            "role": "slave",
            "addr": "10.0.0.2:17801",
            "weight": 1.0,
            "concurrent": 10,
            "status": "completed",
            "qpsAvg": 100.0,
            "cpuPeak": 1.0,
            "memPeakMb": 128,
            "clockOffsetMs": 0.0,
            "clockUncertaintyMs": 0.0,
            "droppedSamples": 0,
            "startMs": 1000,
            "endMs": 2000
        }))
        .unwrap();
        assert_eq!(node.total_requests, None);
        assert_eq!(node.success_count, None);
        assert_eq!(node.error_count, None);
        assert_eq!(node.avg_rt_ms, None);
        assert_eq!(node.tps, None);
        assert_eq!(node.egress_avg_kbps, None);
    }

    #[test]
    fn protocol_defaults_match_chapter_eight() {
        let info = ClusterProtocolInfo::default();
        assert_eq!(info.default_port, 17801);
        assert_eq!(info.pair_code_ttl_sec, 300);
        assert_eq!(info.min_pair_code_ttl_sec, 60);
        assert_eq!(info.max_pair_code_ttl_sec, 3600);
        assert_eq!(info.heartbeat_sec, 2);
        assert_eq!(info.heartbeat_misses, 3);
        assert_eq!(info.clock_rounds, 5);
        assert_eq!(info.clock_drop_uncertainty_ms, 200.0);
    }
}
