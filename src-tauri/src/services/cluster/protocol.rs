//! 主从消息与共享数据结构
//!
//! 传输为 WebSocket + JSON（内网明文，一期不做 wss）。所有消息带 `type` 标签，
//! 字段统一 camelCase，便于前端快照直接复用。时钟与采样都用毫秒时间戳，
//! 避免跨机 `Instant` 不可比的问题。

// W0 先落地协议与快照模型，W2 / W3 接入后自然被使用；
// 增量期间保留 allow，避免 CI 的 clippy -D warnings 因「尚未接线」失败。
#![allow(dead_code)]

use serde::{Deserialize, Serialize};

/// 协议版本：v2 增加能力上报、mDNS、阶段编排与断线重连
pub const PROTOCOL_VERSION: u32 = 2;
/// 主节点仍接受的最低从节点版本：v1 按最小能力集工作
pub const MIN_PROTOCOL_VERSION: u32 = 1;
/// 默认监听端口
pub const DEFAULT_PORT: u16 = 17801;
/// mDNS 服务类型：只做便利发现，手动填 IP 永久保留
pub const MDNS_SERVICE_TYPE: &str = "_astroman-cluster._tcp.local.";
/// 失联节点保留状态、允许重连的时长（秒）
pub const RECONNECT_GRACE_SEC: u64 = 300;
/// 从节点重连退避上限（秒）
pub const RECONNECT_BACKOFF_MAX_SEC: u64 = 30;
/// 编排阶段总数上限，避免一次任务把引擎拖入过多阶段
pub const MAX_ORCHESTRATION_PHASES: usize = 32;
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
/// 一期单任务最多从节点数，避免主节点被大量连接拖垮
pub const MAX_NODES: usize = 16;

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
    pub min_version: u32,
    pub reconnect_grace_sec: u64,
    pub reconnect_backoff_max_sec: u64,
    pub mdns_service: String,
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
            min_version: MIN_PROTOCOL_VERSION,
            reconnect_grace_sec: RECONNECT_GRACE_SEC,
            reconnect_backoff_max_sec: RECONNECT_BACKOFF_MAX_SEC,
            mdns_service: MDNS_SERVICE_TYPE.to_string(),
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
    CapabilityReport(DeviceCapability),
    DeviceList(DeviceListPayload),
    TimeProbe(TimeProbePayload),
    TimeAck(TimeAckPayload),
    Prepare(PreparePayload),
    PrepareAck(PrepareAckPayload),
    Preflight(PreflightPayload),
    Start(StartPayload),
    PhaseStart(PhaseStartPayload),
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
    pub pair_code: String,
    pub node_name: String,
    /// 从节点本机能力检测的饱和并发（v1 字段，v2 改走 capabilityReport）
    #[serde(default)]
    pub capacity_concurrent: Option<u32>,
    /// 稳定的设备标识：重连与设备列表用它区分同一台机器
    #[serde(default)]
    pub device_id: Option<String>,
    /// 断线重连：携带配对时下发的令牌恢复原 job，不再消耗配对码
    #[serde(default)]
    pub session_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HelloAckPayload {
    /// 协商后的协议版本：主节点回显从节点版本，v1 从节点仍能工作
    pub version: u32,
    /// 配对码单次使用：配对成功后只认令牌
    pub token: String,
    pub node_id: String,
    pub job_id: i64,
    /// true 表示这是重连恢复，不会重新下发时钟探测
    #[serde(default)]
    pub resumed: bool,
}

/// 从节点能力上报：复用 machine_capacity 的最近一次检测
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceCapability {
    #[serde(default)]
    pub device_id: Option<String>,
    #[serde(default)]
    pub node_name: String,
    #[serde(default)]
    pub os: Option<String>,
    #[serde(default)]
    pub cores: u32,
    #[serde(default)]
    pub capacity_concurrent: u32,
    #[serde(default)]
    pub capacity_qps: f64,
    #[serde(default)]
    pub probed_at: Option<i64>,
}

/// 设备列表项：来自 cluster_devices，online 由当前任务节点状态推导
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClusterDevice {
    pub node_id: String,
    pub node_name: String,
    pub last_addr: String,
    pub last_seen_at: i64,
    pub os: Option<String>,
    pub cores: u32,
    pub capacity_concurrent: u32,
    pub capacity_qps: f64,
    pub probed_at: Option<i64>,
    #[serde(default)]
    pub online: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceListPayload {
    pub devices: Vec<ClusterDevice>,
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

/// 计划级编排：主节点把每个阶段的集群总并发按权重拆到各节点
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Orchestration {
    #[serde(default = "orchestration_version")]
    pub version: u32,
    pub phases: Vec<OrchestrationPhase>,
    /// 仅支持 weighted；保留字段便于后续扩展
    #[serde(default = "default_assignment")]
    pub assignment: String,
}

fn orchestration_version() -> u32 {
    1
}

fn default_assignment() -> String {
    "weighted".to_string()
}

impl Default for Orchestration {
    fn default() -> Self {
        Self {
            version: orchestration_version(),
            phases: Vec::new(),
            assignment: default_assignment(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrchestrationPhase {
    /// ramp / hold / step
    pub kind: String,
    pub duration_sec: u32,
    #[serde(default)]
    pub from_concurrent: Option<u32>,
    #[serde(default)]
    pub to_concurrent: Option<u32>,
    #[serde(default)]
    pub concurrent: Option<u32>,
}

impl Orchestration {
    /// 校验并展开为逐秒的集群总并发序列；ramp 每秒线性插值，hold/step 保持定值
    pub fn expand_totals(&self) -> Result<Vec<u32>, String> {
        if self.version != 1 {
            return Err(format!("不支持的编排版本：{}", self.version));
        }
        if self.assignment != "weighted" {
            return Err(format!("不支持的分配方式：{}", self.assignment));
        }
        if self.phases.is_empty() {
            return Err("编排至少需要 1 个阶段".to_string());
        }
        if self.phases.len() > MAX_ORCHESTRATION_PHASES {
            return Err(format!(
                "编排最多 {MAX_ORCHESTRATION_PHASES} 个阶段，当前 {} 个",
                self.phases.len()
            ));
        }
        let mut totals: Vec<u32> = Vec::new();
        for (index, phase) in self.phases.iter().enumerate() {
            if phase.duration_sec == 0 {
                return Err(format!("第 {} 个阶段时长必须大于 0", index + 1));
            }
            match phase.kind.as_str() {
                "ramp" => {
                    let from = phase
                        .from_concurrent
                        .ok_or_else(|| format!("第 {} 个 ramp 阶段缺少起始并发", index + 1))?;
                    let to = phase
                        .to_concurrent
                        .ok_or_else(|| format!("第 {} 个 ramp 阶段缺少目标并发", index + 1))?;
                    if from == 0 || to == 0 {
                        return Err("阈值并发必须大于 0".to_string());
                    }
                    let steps = phase.duration_sec.max(1) as u64;
                    for step in 0..steps {
                        let ratio = if steps <= 1 {
                            1.0
                        } else {
                            step as f64 / (steps - 1) as f64
                        };
                        let value = from as f64 + (to as f64 - from as f64) * ratio;
                        totals.push(value.round().max(1.0) as u32);
                    }
                }
                "hold" | "step" => {
                    let value = phase
                        .concurrent
                        .ok_or_else(|| format!("第 {} 个阶段缺少并发值", index + 1))?;
                    if value == 0 {
                        return Err("阶段并发必须大于 0".to_string());
                    }
                    for _ in 0..phase.duration_sec {
                        totals.push(value);
                    }
                }
                other => return Err(format!("不支持的阶段类型：{other}")),
            }
        }
        if totals.len() > 86_400 {
            return Err("编排总时长不能超过 24 小时".to_string());
        }
        Ok(totals)
    }

    pub fn peak(&self) -> Result<u32, String> {
        self.expand_totals()?
            .into_iter()
            .max()
            .ok_or_else(|| "编排没有有效阶段".to_string())
    }
}

/// 阶段切换通知：从节点据此刷新展示，并发已随配置里的阶段表由引擎执行
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PhaseStartPayload {
    pub index: u32,
    /// 主节点时钟毫秒
    pub at_ms: i64,
    /// 该阶段集群总并发
    pub concurrent: u32,
    /// 本节点权重与分配到的并发
    pub weight: f64,
    pub node_concurrent: u32,
    pub kind: String,
    pub duration_sec: u32,
}

/// 下发执行配置：只传模板与变量名，变量值由各节点本地解析
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreparePayload {
    pub job_id: i64,
    pub plan_name: String,
    /// StressConfig 的 JSON 形态；envId 已清空，auth 仅保留变量引用
    pub config: serde_json::Value,
    /// 环境名：从节点按名称在本地库中查找，找不到则拒绝加入
    pub env_name: Option<String>,
    /// 配置里出现的变量名，用于一次性报出全部缺失项
    pub required_vars: Vec<String>,
    /// 分配到的并发
    pub concurrent: u32,
    /// 集群总并发（展示用）
    pub total_concurrent: u32,
    /// 计划级编排；为 None 表示固定并发或单场景
    #[serde(default)]
    pub orchestration: Option<Orchestration>,
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
    /// 本秒总耗时累加，用于跨节点加权平均
    pub sum_rt_ms: u64,
    /// 本秒 TTFB 累加：节点失联时没有 done，只能用每秒累加还原 TTFB 均值
    #[serde(default)]
    pub sum_ttfb_ms: u64,
    pub latency_buckets: SparseBuckets,
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
    pub bytes: u64,
    pub bytes_sent: u64,
    pub sum_rt_ms: u64,
    pub sum_ttfb_ms: u64,
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AbortPayload {
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PingPayload {
    pub at_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PongPayload {
    pub at_ms: i64,
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
    /// 从节点时钟相对主节点的偏移（毫秒，正数表示从节点更快）
    pub clock_offset_ms: Option<f64>,
    pub clock_uncertainty_ms: Option<f64>,
    pub dropped_samples: u64,
    pub joined_at: i64,
    pub last_seen_at: Option<i64>,
    pub error: Option<String>,
    /// 协商后的协议版本（1 或 2）
    pub protocol_version: u32,
    pub device_id: Option<String>,
    pub capacity_concurrent: Option<u32>,
    pub capacity_qps: Option<f64>,
    pub cores: Option<u32>,
    pub os: Option<String>,
    /// 按能力归一化得到的建议权重；节点未上报能力时为 None
    pub suggested_weight: Option<f64>,
    /// 最近一次断线时刻；用于 5 分钟重连宽限与缺口展示
    pub disconnected_at: Option<i64>,
    /// 断线累计秒数（含已重连恢复的历史缺口）
    pub gap_seconds: u64,
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
    pub start_at_ms: Option<i64>,
    pub elapsed_sec: u64,
    pub report_id: Option<i64>,
    pub incomplete: bool,
    pub message: Option<String>,
    pub nodes: Vec<ClusterNodeInfo>,
    pub created_at: i64,
    /// 计划级编排（若有）
    pub orchestration: Option<Orchestration>,
    /// 当前阶段序号（编排任务）
    pub current_phase: Option<u32>,
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
    pub device_id: Option<String>,
    pub protocol_version: u32,
    /// 断线重连中：引擎继续跑，采样等恢复后继续上报
    pub reconnecting: bool,
    pub phase_index: Option<u32>,
    pub phase_concurrent: u32,
    pub reconnect_attempts: u32,
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
    #[serde(default)]
    pub note: Option<String>,
    /// 编排定义：报告可据此还原阶段边界
    #[serde(default)]
    pub orchestration: Option<Orchestration>,
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
    pub clock_offset_ms: f64,
    pub clock_uncertainty_ms: f64,
    pub dropped_samples: u64,
    pub start_ms: i64,
    pub end_ms: i64,
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
            bytes: 4,
            bytes_sent: 5,
            sum_rt_ms: 6,
            sum_ttfb_ms: 7,
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
        };
        let text = serde_json::to_string(&payload).unwrap();
        let back: DonePayload = serde_json::from_str(&text).unwrap();
        assert_eq!(back.latency_buckets, vec![(1, 2), (9, 1)]);
        assert_eq!(back.status_codes, vec![(200, 2), (500, 1)]);
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
        assert_eq!(info.version, 2);
        assert_eq!(info.min_version, 1);
        assert_eq!(info.reconnect_grace_sec, 300);
    }

    #[test]
    fn v1_hello_ack_still_parses_without_v2_fields() {
        let text = r#"{"type":"helloAck","version":1,"token":"t","nodeId":"node-1","jobId":9}"#;
        let message: ClusterMessage = serde_json::from_str(text).unwrap();
        match message {
            ClusterMessage::HelloAck(ack) => {
                assert_eq!(ack.version, 1);
                assert!(!ack.resumed);
                assert_eq!(ack.job_id, 9);
            }
            other => panic!("unexpected message: {other:?}"),
        }
    }

    #[test]
    fn orchestration_expands_per_second_totals() {
        let orchestration = Orchestration {
            version: 1,
            assignment: "weighted".to_string(),
            phases: vec![
                OrchestrationPhase {
                    kind: "hold".to_string(),
                    duration_sec: 2,
                    from_concurrent: None,
                    to_concurrent: None,
                    concurrent: Some(5),
                },
                OrchestrationPhase {
                    kind: "ramp".to_string(),
                    duration_sec: 3,
                    from_concurrent: Some(5),
                    to_concurrent: Some(15),
                    concurrent: None,
                },
            ],
        };
        assert_eq!(
            orchestration.expand_totals().unwrap(),
            vec![5, 5, 5, 10, 15]
        );
        assert_eq!(orchestration.peak().unwrap(), 15);
    }

    #[test]
    fn orchestration_rejects_unknown_kind_and_too_many_phases() {
        let mut orchestration = Orchestration::default();
        orchestration.phases.push(OrchestrationPhase {
            kind: "spike".to_string(),
            duration_sec: 1,
            from_concurrent: None,
            to_concurrent: None,
            concurrent: Some(1),
        });
        assert!(orchestration.expand_totals().is_err());

        let orchestration = Orchestration {
            phases: vec![
                OrchestrationPhase {
                    kind: "hold".to_string(),
                    duration_sec: 1,
                    from_concurrent: None,
                    to_concurrent: None,
                    concurrent: Some(1),
                };
                MAX_ORCHESTRATION_PHASES + 1
            ],
            ..Default::default()
        };
        assert!(orchestration.expand_totals().is_err());
    }
}
