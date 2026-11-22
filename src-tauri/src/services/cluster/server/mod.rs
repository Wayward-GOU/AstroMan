//! 主节点服务（一期）
//!
//! 主节点监听 WebSocket 端口，从节点反向连接后用配对码加入：
//!
//! 1. `hello{pairCode}` → 单次配对码换令牌，登记节点；
//! 2. 主节点连发 5 个 `timeProbe`，取 RTT 最小样本计算时钟偏移；
//! 3. `prepare` 下发配置模板与分配并发，从节点本地解析环境变量并回执；
//! 4. 从节点完成目标预检后回 `preflight`，全部就绪才允许 `start{atMs}`；
//! 5. 运行中每秒收 `sample`，结束时收 `done`；心跳 2s，6s 无消息判失联。
//!
//! 采样合并与报告落库在 W4，本模块先把会话、状态机与持久化打通。
//! 令牌在一期与连接绑定：消息本身不带令牌，断线重连（三期）再补。

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use parking_lot::Mutex;
use rusqlite::{params, OptionalExtension};
use serde::Deserialize;
use tauri::AppHandle;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::tungstenite::Message;
use uuid::Uuid;

use super::host::ClusterApp;
use super::merge::{self, MergeNode, MergeRequest};
use super::pool_refresh::{DbRefreshSource, LocalPoolRefreshRelay, PoolRefreshCore};
use super::protocol::{
    AbortPayload, ClusterMasterSnapshot, ClusterMessage, ClusterNodeInfo, ClusterSampleFrame,
    DonePayload, EngineStats, ErrorPayload, HelloAckPayload, HelloPayload, HelloPendingPayload,
    HelloRejectedPayload, NodeCapacity, NodeSampleLite, PendingNodeInfo, PingPayload, PongPayload,
    PoolRefreshRequestPayload, PoolRefreshResultPayload, PoolSizeEstimate, PoolSlice,
    PreflightPayload, PrepareAckPayload, PreparePayload, SamplePayload, StartPayload,
    TimeAckPayload, TimeProbePayload, CLOCK_SAMPLE_ROUNDS, DEFAULT_PORT, HEARTBEAT_MISSES,
    HEARTBEAT_SEC, MAX_MESSAGE_BYTES, MAX_NODES, MAX_PAIR_CODE_TTL_SEC, MAX_POOL_SLICE_BYTES,
    MIN_PAIR_CODE_TTL_SEC, PAIR_CODE_DIGITS, PAIR_CODE_TTL_SEC, PROTOCOL_VERSION,
};
use crate::commands::stress::{
    prepare_stress_with_relay, run_engine, save_report, EngineOptions, EngineOutcome,
    EngineSampleSink, PoolRefreshRelay, PreparedStress, ReportPayload,
};
use crate::models::{ReportError, ReportRequestRecord, SlowRequest, StressConfig};
use crate::state::{DbPool, StressJobDraft};
use crate::util::{err, now_ms};

mod pairing;
mod persist;
#[cfg(test)]
mod tests;

pub(crate) use pairing::clock_metrics;
use pairing::{new_pair_code, register_pair_failure};
pub(crate) use persist::recover_stale_jobs;
use persist::{
    insert_job, save_cluster_samples, update_job, update_job_finished, update_job_start,
    update_pair_expiry, update_weights, update_worker, upsert_worker, WorkerRow,
};

/// 任务开跑前的统一提前量：留出网络单程与从节点建引擎的时间
pub const START_LEAD_MS: i64 = 2_000;
/// 准备阶段等待从节点回执的上限
pub const SETUP_TIMEOUT: Duration = Duration::from_secs(15);
/// 审批模式：从节点等待主机接受的最长时间（秒）
pub const JOIN_WAIT_SEC: u64 = 120;
/// 审批模式：同一 IP 两次申请的最小间隔（毫秒）
const JOIN_RATE_MS: i64 = 5_000;
/// 审批模式：待接受申请上限
const MAX_PENDING_JOINS: usize = 3;
/// 发现广播间隔（秒）
const BEACON_SEC: u64 = 2;
/// 单节点保留的每秒采样上限：AI 诊断最多取 300 条，超出即丢最旧的
const SAMPLE_RETAIN: usize = 300;
/// 运行期节点状态落库的最小间隔，避免每来一条采样就写一次库
const PERSIST_INTERVAL_MS: i64 = 5_000;
/// 配对码失败限速：统计窗口
const PAIR_FAIL_WINDOW_MS: i64 = 10_000;
/// 配对码失败限速：同一来源在窗口内允许的失败次数
const PAIR_FAIL_PER_WINDOW: u32 = 5;
/// 配对码失败上限：当前码累计错误达到该值即作废并换新码
const PAIR_FAIL_TOTAL: u32 = 10;
/// 运行期时钟复测间隔（秒）：长任务靠它发现晶振漂移
const CLOCK_RECHECK_SEC: u64 = 60;
/// 复测 seq 起点：与配对阶段的 0..CLOCK_SAMPLE_ROUNDS 区分
const RECHECK_SEQ_BASE: u32 = 900_000;
pub const EVENT_MASTER: &str = "cluster:master";
/// 采样帧事件：每秒一帧，替代逐节点逐条推送
pub const EVENT_SAMPLE_FRAME: &str = "cluster:sample-frame";

/// 主节点启动参数
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClusterMasterStartPayload {
    /// 计划 ID：仅用于报告归属，可空
    #[serde(default)]
    pub plan_id: Option<i64>,
    /// 计划名：为空时取配置里的 name，再退到 URL
    #[serde(default)]
    pub plan_name: Option<String>,
    pub config: StressConfig,
    /// 环境名：从节点按名称在本地库查找，主节点不下发变量值
    #[serde(default)]
    pub env_name: Option<String>,
    #[serde(default)]
    pub port: Option<u16>,
    /// 配对码有效期（秒）：缺省 5 分钟，每次配对成功后按该值刷新
    #[serde(default)]
    pub pair_code_ttl_sec: Option<i64>,
    /// 主节点自身在集群中的显示名
    #[serde(default)]
    pub master_name: Option<String>,
}

/// 校验并归一化配对码有效期：缺省 300 秒，允许 1 分钟到 1 小时
fn resolve_pair_code_ttl(value: Option<i64>) -> Result<i64, String> {
    let ttl = value.unwrap_or(PAIR_CODE_TTL_SEC);
    if !(MIN_PAIR_CODE_TTL_SEC..=MAX_PAIR_CODE_TTL_SEC).contains(&ttl) {
        return Err(format!(
            "配对码有效期需在 {} 到 {} 分钟之间",
            MIN_PAIR_CODE_TTL_SEC / 60,
            MAX_PAIR_CODE_TTL_SEC / 60
        ));
    }
    Ok(ttl)
}

/// 单节点权重与并发边界：缺省权重 1，主节点与从节点一视同仁
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClusterNodeWeight {
    pub node_id: String,
    pub weight: f64,
    /// 该节点至少分配的并发；缺省 1
    #[serde(default)]
    pub min_concurrent: Option<u32>,
    /// 该节点最多分配的并发；缺省取本机预算
    #[serde(default)]
    pub max_concurrent: Option<u32>,
}

/// 采样帧缓冲：按秒累积各节点的图表字段，由 watchdog 每秒 flush 一次
#[derive(Default)]
struct SampleFrameBuffer {
    sec: u64,
    nodes: HashMap<String, NodeSampleLite>,
}

/* ---------- 会话数据 ---------- */

struct PairCode {
    code: String,
    expires_at: i64,
}

struct NodeState {
    id: String,
    name: String,
    role: String,
    addr: String,
    weight: f64,
    concurrent: u32,
    status: String,
    qps: f64,
    cpu: Option<f64>,
    mem_mb: Option<u64>,
    /// 最近一帧整机资源（从节点每秒随采样上报）
    host_cpu_percent: Option<f64>,
    host_mem_percent: Option<f64>,
    net_rx_kbps: Option<f64>,
    net_tx_kbps: Option<f64>,
    node_threads: Option<u32>,
    load1: Option<f64>,
    clock_offset_ms: Option<f64>,
    clock_uncertainty_ms: Option<f64>,
    /// 时钟对齐时 RTT 最小的那一轮的误差（毫秒）
    clock_best_error_ms: f64,
    clock_probes: HashMap<u32, i64>,
    clock_samples: u32,
    /// 从节点是否支持运行期时钟复测（Hello 声明；旧节点缺省 false）
    clock_recheck: bool,
    /// 运行期复测的漂移：相对配对时刻偏移的变化量（毫秒）
    clock_drift_ms: f64,
    /// 最近一次复测时刻
    clock_recheck_at: Option<i64>,
    dropped_samples: u64,
    joined_at: i64,
    last_seen_at: Option<i64>,
    error: Option<String>,
    tx: Option<mpsc::UnboundedSender<ClusterMessage>>,
    prepare_ok: Option<bool>,
    preflight_ok: Option<bool>,
    samples: Vec<SamplePayload>,
    /// 最近一次落库时刻：运行期按 PERSIST_INTERVAL_MS 节流
    last_persist_at: i64,
    done: Option<DonePayload>,
    /// 节点能力：核数/内存/本机预算
    capacity: Option<NodeCapacity>,
    /// 本次分配并发上限（预算与 min/max 共同决定）
    quota: u32,
    /// 全局 VU 区间
    vu_start: u32,
    vu_count: u32,
    /// 运行期峰值
    peak_qps: f64,
    peak_concurrent: u32,
    /// 采集健康度
    first_sample_ms: Option<i64>,
    last_sample_ms: Option<i64>,
    received_samples: u64,
    last_stats: Option<EngineStats>,
    /// X25519 对端公钥（主节点侧保存从节点公钥）
    peer_public_key: Option<[u8; 32]>,
    /// 审批模式的申请 ID（接受后清空）
    request_id: Option<String>,
    /// 设备指纹：审批展示与白名单
    device_id: Option<String>,
    /// 最近一次 PrepareAck 报告的缺失变量（含 `os:NAME`）
    missing_vars: Vec<String>,
    /// 从节点是否支持数据池自定义列（v20）；旧节点缺省 false
    pool_columns: bool,
    /// 本次下发的池切片字节数（prepare 时写入）
    pool_slice_bytes: Option<u64>,
}

impl NodeState {
    fn master(name: String, addr: String, joined_at: i64) -> Self {
        Self {
            id: "master".to_string(),
            name,
            role: "master".to_string(),
            addr,
            weight: 1.0,
            concurrent: 0,
            status: "pairing".to_string(),
            qps: 0.0,
            cpu: None,
            mem_mb: None,
            host_cpu_percent: None,
            host_mem_percent: None,
            net_rx_kbps: None,
            net_tx_kbps: None,
            node_threads: None,
            load1: None,
            clock_offset_ms: None,
            clock_uncertainty_ms: None,
            clock_best_error_ms: f64::INFINITY,
            clock_probes: HashMap::new(),
            clock_samples: 0,
            clock_recheck: false,
            clock_drift_ms: 0.0,
            clock_recheck_at: None,
            dropped_samples: 0,
            joined_at,
            last_seen_at: Some(joined_at),
            error: None,
            tx: None,
            prepare_ok: None,
            preflight_ok: None,
            samples: Vec::new(),
            last_persist_at: 0,
            done: None,
            capacity: None,
            quota: 0,
            vu_start: 0,
            vu_count: 0,
            peak_qps: 0.0,
            peak_concurrent: 0,
            first_sample_ms: None,
            last_sample_ms: None,
            received_samples: 0,
            last_stats: None,
            peer_public_key: None,
            request_id: None,
            device_id: None,
            missing_vars: Vec::new(),
            pool_columns: true,
            pool_slice_bytes: None,
        }
    }

    fn info(&self, start_at_ms: Option<i64>, now: i64) -> ClusterNodeInfo {
        ClusterNodeInfo {
            id: self.id.clone(),
            name: self.name.clone(),
            role: self.role.clone(),
            addr: self.addr.clone(),
            weight: self.weight,
            concurrent: self.concurrent,
            status: self.status.clone(),
            qps: self.qps,
            cpu: self.cpu,
            mem_mb: self.mem_mb,
            host_cpu_percent: self.host_cpu_percent,
            host_mem_percent: self.host_mem_percent,
            net_rx_kbps: self.net_rx_kbps,
            net_tx_kbps: self.net_tx_kbps,
            threads: self.node_threads,
            load1: self.load1,
            clock_offset_ms: self.clock_offset_ms,
            clock_uncertainty_ms: self.clock_uncertainty_ms,
            clock_drift_ms: self.clock_drift_ms,
            clock_recheck_at: self.clock_recheck_at,
            dropped_samples: self.dropped_samples,
            joined_at: self.joined_at,
            last_seen_at: self.last_seen_at,
            error: self.error.clone(),
            capacity: self.capacity.clone(),
            quota: self.quota,
            health: self.health(start_at_ms, now),
            peak_qps: self.peak_qps,
            peak_concurrent: self.peak_concurrent,
            vu_start: self.vu_start,
            vu_count: self.vu_count,
            engine_stats: self.last_stats.clone(),
            missing_vars: self.missing_vars.clone(),
            pool_columns: self.pool_columns,
            env_bundle: self.role == "master" || self.peer_public_key.is_some(),
            pool_slice_bytes: self.pool_slice_bytes,
        }
    }

    /// 采集健康度：区分「还没开始」「首样超时」「中途缺口」「已结束」「失联」
    fn health(
        &self,
        start_at_ms: Option<i64>,
        now: i64,
    ) -> crate::services::cluster::protocol::SampleHealth {
        use crate::services::cluster::protocol::{
            SampleHealth, FIRST_SAMPLE_BAD_SEC, FIRST_SAMPLE_WARN_SEC,
        };
        let elapsed = start_at_ms
            .map(|start| (now - start).max(0) as u64 / 1000)
            .unwrap_or(0);
        let expected = if self.status == "running" || self.status == "done" {
            elapsed
        } else {
            0
        };
        let received = self.samples.len() as u64;
        let lost = expected.saturating_sub(received);
        let gap_sec = self
            .last_sample_ms
            .map(|last| (now - last).max(0) as u64 / 1000)
            .unwrap_or(elapsed);
        let (state, message) = match self.status.as_str() {
            "running" => {
                if received == 0 {
                    if elapsed >= FIRST_SAMPLE_BAD_SEC {
                        (
                            "lost",
                            Some(format!("首样超时 {elapsed}s，检查从节点引擎是否启动")),
                        )
                    } else if elapsed >= FIRST_SAMPLE_WARN_SEC {
                        ("waiting", Some(format!("等待首样（{elapsed}s）")))
                    } else {
                        ("waiting", None)
                    }
                } else if gap_sec > 2 {
                    ("gap", Some(format!("已 {gap_sec}s 没有新采样")))
                } else if lost > 0 {
                    ("gap", Some(format!("丢失 {lost} 个采样点")))
                } else {
                    ("ok", None)
                }
            }
            "done" => {
                if lost > 0 {
                    ("gap", Some(format!("丢失 {lost} 个采样点")))
                } else {
                    ("done", None)
                }
            }
            "lost" => ("lost", self.error.clone()),
            "left" | "rejected" => ("lost", self.error.clone()),
            _ => ("waiting", None),
        };
        SampleHealth {
            state: state.to_string(),
            first_sample_ms: self.first_sample_ms,
            last_sample_ms: self.last_sample_ms,
            expected,
            received,
            lost,
            gap_sec,
            message,
        }
    }

    fn is_active(&self) -> bool {
        !matches!(self.status.as_str(), "lost" | "left" | "rejected" | "done")
    }
}

/// 心跳回传的引擎计数：顺手把节点资源用量写回状态
///
/// 之前只在收到 `Done` 时才赋值，导致运行期节点表的 CPU/内存一直是空的。
fn apply_engine_stats(node: &mut NodeState, stats: EngineStats) {
    if node.status == "running" {
        node.qps = stats.qps;
        if stats.cpu > 0.0 {
            node.cpu = Some(stats.cpu);
        }
        if stats.mem_mb > 0 {
            node.mem_mb = Some(stats.mem_mb);
        }
    }
    node.last_stats = Some(stats);
}

/// 运行期时钟复测：只记录与配对偏移的差值，不改 offset
///
/// 施压中途改 offset 会让已上报的 `elapsed_sec` 和对齐的起跑时刻一起跳变，
/// 曲线会出现垂直断层；因此这里只量化漂移，用告警提示用户时钟跑偏了。
fn apply_clock_recheck(node: &mut NodeState, offset: f64, at: i64) {
    if let Some(baseline) = node.clock_offset_ms {
        node.clock_drift_ms = offset - baseline;
    }
    node.clock_recheck_at = Some(at);
}

struct SessionState {
    status: String,
    start_at_ms: Option<i64>,
    report_id: Option<i64>,
    incomplete: bool,
    message: Option<String>,
    pair_code: Option<PairCode>,
    nodes: Vec<NodeState>,
    next_node: u32,
    /// 加入申请限流：IP → 最近一次申请时刻
    join_requests: HashMap<String, i64>,
    /// 配对码错误限速：IP → (窗口内次数, 窗口起点)
    pair_failures: HashMap<String, (u32, i64)>,
    /// 当前配对码累计错误次数：达到上限即作废换码
    pair_fail_total: u32,
}

/// 主节点本地引擎的逐请求明细：集群报告只收主节点的明细，需在报告里标注
struct LocalDetails {
    requests: Vec<ReportRequestRecord>,
    errors: Vec<ReportError>,
    slow_requests: Vec<SlowRequest>,
    config_snapshot: String,
    request_id: Option<i64>,
    run_group_id: Option<i64>,
}

struct MasterSession {
    job_id: i64,
    plan_id: i64,
    plan_name: String,
    target: String,
    method: String,
    config: serde_json::Value,
    /// 主节点本地执行用的原始配置（保留 envId 等，仅供本机解析变量）
    local_config: StressConfig,
    env_name: Option<String>,
    required_vars: Vec<String>,
    total_concurrent: u32,
    port: u16,
    pair_code_ttl_sec: i64,
    created_at: i64,
    local_addresses: Vec<String>,
    db: DbPool,
    app: Arc<dyn ClusterApp>,
    shutdown: Arc<AtomicBool>,
    state: Mutex<SessionState>,
    /// 本地引擎已准备但未启动；启动时取走
    local_prepared: Mutex<Option<PreparedStress>>,
    /// 本地引擎在任务注册表中的 id（准备阶段登记，结束时释放）
    local_job: Mutex<Option<u64>>,
    /// 本地引擎取消位：与注册表共用，停止按钮能直接停掉主节点本地压测
    local_watch: Mutex<Option<Arc<AtomicBool>>>,
    /// 本地引擎结束后的明细，等所有节点结算后一起落库
    local_details: Mutex<Option<LocalDetails>>,
    /// 采样帧缓冲：每秒推送一次图表数据，避免逐条 IPC
    sample_frame: Mutex<SampleFrameBuffer>,
    /// 手动中止：引擎仍会产出 stopped 结果，报告要标注不完整
    aborting: AtomicBool,
    /// 合并落库只允许执行一次
    finalizing: AtomicBool,
    /// 本机并发预算（主节点）
    budget: u32,
    /// 分配模式：auto / even / custom
    allocation_mode: Mutex<String>,
    /// 创建任务时按池条目估算的体量，供下发前提示；无池绑定为 None
    pool_estimate: Option<PoolSizeEstimate>,
    /// 本会话 X25519 私钥（EnvBundle 加密）
    x25519_secret: x25519_dalek::StaticSecret,
    /// 本会话 X25519 公钥（base64）
    x25519_public: String,
    /// Token 刷新单飞核心：从节点与本地引擎共用（8.10.5）
    pool_refresh: Arc<PoolRefreshCore>,
    /// accept 任务句柄：终态时中止并释放端口
    accept: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
    /// 进入终态的时间（毫秒）：用于完成后快照的保留清理
    terminal_at: Mutex<Option<i64>>,
    /// 本次任务是否自动接受加入申请
    auto_accept: AtomicBool,
    /// 是否广播局域网发现
    discovery: bool,
    /// 运行期时钟复测的序号：跨节点共享，回包按 seq 认领
    recheck_seq: AtomicU64,
}

struct RunningMaster {
    session: Arc<MasterSession>,
}

/// 主节点会话管理器：随 AppState 存活
#[derive(Default)]
pub struct ClusterMaster {
    sessions: Mutex<HashMap<i64, RunningMaster>>,
}

impl ClusterMaster {
    /// 建任务、绑端口并开始等待从节点连接
    pub async fn create(
        &self,
        pool: DbPool,
        app: AppHandle,
        payload: ClusterMasterStartPayload,
    ) -> Result<ClusterMasterSnapshot, String> {
        self.create_with(pool, Arc::new(app), payload).await
    }

    /// 与 `create` 相同，但接受解耦后的宿主接口：验收测试可注入 mock 应用句柄
    pub(crate) async fn create_with(
        &self,
        pool: DbPool,
        app: Arc<dyn ClusterApp>,
        payload: ClusterMasterStartPayload,
    ) -> Result<ClusterMasterSnapshot, String> {
        // v1.1.2：不再限制同时保留的集群任务数；终态会释放监听，完成快照仍可查看
        self.prune_finished();
        let (config, required_vars) = shared_config(&payload.config)?;
        let total_concurrent = payload.config.concurrent;
        let plan_name = payload
            .plan_name
            .as_deref()
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_string)
            .or_else(|| {
                payload
                    .config
                    .name
                    .as_deref()
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                    .map(str::to_string)
            })
            .unwrap_or_else(|| payload.config.url.trim().to_string());
        let listener = bind_cluster_listener(payload.port).await?;
        let port = listener.local_addr().map_err(err)?.port();
        let budget = app
            .try_state()
            .map(|state| state.budget().machine_limit)
            .unwrap_or(1000);
        let discovery = app
            .try_state()
            .map(|state| state.settings().cluster_discovery_enabled)
            .unwrap_or(true);
        let x25519_secret = x25519_dalek::StaticSecret::random();
        let x25519_public = crate::commands::http::base64_encode(
            x25519_dalek::PublicKey::from(&x25519_secret).as_bytes(),
        );
        let pair_code_ttl = resolve_pair_code_ttl(payload.pair_code_ttl_sec)?;
        let created_at = now_ms();
        let pair_code = PairCode {
            code: new_pair_code(),
            expires_at: created_at + pair_code_ttl * 1000,
        };
        let local_addresses = super::local_addresses();
        let master_name = payload
            .master_name
            .as_deref()
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| local_node_name("master"));
        let master_addr = local_addresses.first().cloned().unwrap_or_default();
        let job_config = serde_json::json!({
            "config": config.clone(),
            "envName": payload.env_name.clone(),
            "requiredVars": required_vars.clone(),
            "weights": {},
        });
        let job_id = insert_job(
            &pool,
            created_at,
            &master_name,
            payload.plan_id,
            &plan_name,
            port,
            &job_config.to_string(),
            pair_code.expires_at,
            total_concurrent,
        )?;
        let master_node = NodeState::master(master_name, master_addr, created_at);
        upsert_worker(&pool, job_id, &master_node)?;
        let pool_estimate = estimate_pool_bytes(&pool, &payload.config.pool_bindings);
        // 刷新用的环境变量：与主节点本地引擎同源，供刷新地址/请求体渲染
        let refresh_vars = master_refresh_vars(&pool, &payload.config, payload.env_name.as_deref());
        let pool_refresh = Arc::new(PoolRefreshCore::new(
            Box::new(DbRefreshSource::new(pool.clone())),
            refresh_vars,
        ));
        let session = Arc::new(MasterSession {
            job_id,
            plan_id: payload.plan_id.unwrap_or(0),
            plan_name,
            target: payload.config.url.trim().to_string(),
            method: payload.config.method.clone(),
            config,
            local_config: payload.config,
            env_name: payload.env_name,
            required_vars,
            total_concurrent,
            port,
            pair_code_ttl_sec: pair_code_ttl,
            created_at,
            local_addresses,
            db: pool,
            app,
            shutdown: Arc::new(AtomicBool::new(false)),
            state: Mutex::new(SessionState {
                status: "pairing".to_string(),
                start_at_ms: None,
                report_id: None,
                incomplete: false,
                message: None,
                pair_code: Some(pair_code),
                nodes: vec![master_node],
                next_node: 1,
                join_requests: HashMap::new(),
                pair_failures: HashMap::new(),
                pair_fail_total: 0,
            }),
            local_prepared: Mutex::new(None),
            local_job: Mutex::new(None),
            local_watch: Mutex::new(None),
            local_details: Mutex::new(None),
            sample_frame: Mutex::new(SampleFrameBuffer::default()),
            aborting: AtomicBool::new(false),
            finalizing: AtomicBool::new(false),
            budget,
            allocation_mode: Mutex::new("auto".to_string()),
            pool_estimate,
            x25519_secret,
            x25519_public,
            pool_refresh,
            accept: Mutex::new(None),
            terminal_at: Mutex::new(None),
            auto_accept: AtomicBool::new(false),
            discovery,
            recheck_seq: AtomicU64::new(0),
        });
        let accept = tauri::async_runtime::spawn(accept_loop(session.clone(), listener));
        *session.accept.lock() = Some(accept);
        tauri::async_runtime::spawn(watchdog_loop(session.clone()));
        if discovery {
            MasterSession::spawn_beacon(session.clone());
        }
        let snapshot = session.snapshot();
        self.sessions.lock().insert(
            job_id,
            RunningMaster {
                session: session.clone(),
            },
        );
        session.emit_snapshot();
        Ok(snapshot)
    }

    pub fn snapshot(&self, job_id: i64) -> Result<ClusterMasterSnapshot, String> {
        Ok(self.session(job_id)?.snapshot())
    }

    pub fn snapshots(&self) -> Vec<ClusterMasterSnapshot> {
        self.prune_finished();
        let sessions: Vec<Arc<MasterSession>> = self
            .sessions
            .lock()
            .values()
            .map(|running| running.session.clone())
            .collect();
        sessions.iter().map(|session| session.snapshot()).collect()
    }

    /// 清理已完成的集群快照：超时（30 分钟）或超出数量上限（20 个）时移除
    ///
    /// 运行中的任务不受影响；终态会话在进入终态时已释放监听端口，
    /// 这里只回收内存里的快照，任务与报告仍在数据库里。
    fn prune_finished(&self) {
        let now = now_ms();
        let mut sessions = self.sessions.lock();
        let mut finished: Vec<(i64, i64)> = sessions
            .iter()
            .filter_map(|(id, running)| {
                let session = &running.session;
                let terminal =
                    matches!(session.state.lock().status.as_str(), "finished" | "failed");
                if !terminal {
                    return None;
                }
                let at = session.terminal_at.lock().unwrap_or(session.created_at);
                Some((*id, at))
            })
            .collect();
        finished.sort_by_key(|(_, at)| *at);
        let mut remaining = finished.len();
        for (id, at) in finished {
            let expired = now - at > crate::services::cluster::protocol::RETENTION_SEC * 1000;
            let overflow = remaining > crate::services::cluster::protocol::RETENTION_MAX_TASKS;
            if expired || overflow {
                sessions.remove(&id);
                remaining = remaining.saturating_sub(1);
            }
        }
    }

    fn session(&self, job_id: i64) -> Result<Arc<MasterSession>, String> {
        self.sessions
            .lock()
            .get(&job_id)
            .map(|running| running.session.clone())
            .ok_or_else(|| format!("分布式压测任务 {job_id} 不存在或已关闭"))
    }

    /// 按分配模式下发配置并等待从节点回执
    pub async fn prepare(
        &self,
        job_id: i64,
        mode: Option<String>,
        weights: Vec<ClusterNodeWeight>,
    ) -> Result<ClusterMasterSnapshot, String> {
        let session = self.session(job_id)?;
        session.prepare(mode, weights).await?;
        Ok(session.snapshot())
    }

    pub fn launch(&self, job_id: i64) -> Result<ClusterMasterSnapshot, String> {
        let session = self.session(job_id)?;
        session.launch()?;
        Ok(session.snapshot())
    }

    pub fn abort(
        &self,
        job_id: i64,
        reason: Option<String>,
    ) -> Result<ClusterMasterSnapshot, String> {
        let session = self.session(job_id)?;
        session.abort(reason)?;
        Ok(session.snapshot())
    }

    /// 接受一个待审批的加入申请
    pub fn approve(&self, job_id: i64, node_id: &str) -> Result<ClusterMasterSnapshot, String> {
        let session = self.session(job_id)?;
        session.approve_join(node_id)?;
        Ok(session.snapshot())
    }

    /// 拒绝一个待审批的加入申请
    pub fn reject(
        &self,
        job_id: i64,
        node_id: &str,
        reason: Option<String>,
    ) -> Result<ClusterMasterSnapshot, String> {
        let session = self.session(job_id)?;
        session.reject_join(node_id, reason)?;
        Ok(session.snapshot())
    }

    /// 本次任务自动接受加入申请
    pub fn set_auto_accept(
        &self,
        job_id: i64,
        enabled: bool,
    ) -> Result<ClusterMasterSnapshot, String> {
        let session = self.session(job_id)?;
        session.set_auto_accept(enabled);
        Ok(session.snapshot())
    }

    /// 节点最近 N 个每秒采样（默认 30，最多 300），用于 AI 诊断
    pub fn node_samples(
        &self,
        job_id: i64,
        node_id: &str,
        limit: Option<usize>,
    ) -> Result<Vec<SamplePayload>, String> {
        let session = self.session(job_id)?;
        let limit = limit.unwrap_or(30).clamp(1, 300);
        let state = session.state.lock();
        let Some(node) = state.nodes.iter().find(|node| node.id == node_id) else {
            return Err("节点不存在".to_string());
        };
        let start = node.samples.len().saturating_sub(limit);
        Ok(node.samples[start..].to_vec())
    }

    /// 变量体检：只读检查下发前的变量来源与各节点状态（不含变量值）
    pub fn var_audit(&self, job_id: i64) -> Result<super::protocol::VarAuditReport, String> {
        let session = self.session(job_id)?;
        Ok(session.var_audit())
    }

    /// 关闭并移除会话（运行中会先中止）
    pub fn close(&self, job_id: i64) -> Result<(), String> {
        let running = self
            .sessions
            .lock()
            .remove(&job_id)
            .ok_or_else(|| format!("分布式压测任务 {job_id} 不存在或已关闭"))?;
        running.session.shutdown.store(true, Ordering::SeqCst);
        running.session.release_transport();
        if matches!(
            running.session.state.lock().status.as_str(),
            "pairing" | "preparing" | "ready" | "running"
        ) {
            let _ = running.session.abort(Some("任务已关闭".to_string()));
        }
        Ok(())
    }
}

impl MasterSession {
    /// 配对阶段每 2 秒广播一次发现 Beacon；离开配对阶段或进程关闭即自动结束
    fn spawn_beacon(session: Arc<MasterSession>) {
        tauri::async_runtime::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_secs(BEACON_SEC));
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                ticker.tick().await;
                if session.shutdown.load(Ordering::SeqCst) {
                    break;
                }
                let snapshot = session.snapshot();
                if snapshot.status != "pairing" {
                    break;
                }
                super::discovery::broadcast_master(&snapshot);
            }
        });
    }

    /// 释放监听端口并停止 accept / 心跳 / 看门狗：
    /// 任务进入终态时调用，已完成快照仍保留在会话表里可查看
    fn release_transport(&self) {
        self.shutdown.store(true, Ordering::SeqCst);
        if let Some(handle) = self.accept.lock().take() {
            handle.abort();
        }
    }

    /// 记录终态时间：完成后快照按「30 分钟 / 20 个任务」保留
    fn mark_terminal(&self) {
        let mut at = self.terminal_at.lock();
        if at.is_none() {
            *at = Some(now_ms());
        }
    }

    fn snapshot(&self) -> ClusterMasterSnapshot {
        let now = now_ms();
        let state = self.state.lock();
        let (pair_code, pair_expires) = match &state.pair_code {
            Some(code) if code.expires_at > now => (Some(code.code.clone()), Some(code.expires_at)),
            _ => (None, None),
        };
        let elapsed_sec = state
            .start_at_ms
            .map(|at| (now - at).max(0) as u64 / 1000)
            .unwrap_or(0);
        ClusterMasterSnapshot {
            job_id: self.job_id,
            name: self.plan_name.clone(),
            plan_id: self.plan_id,
            target: self.target.clone(),
            method: self.method.clone(),
            status: state.status.clone(),
            port: self.port,
            pair_code,
            pair_code_expires_at: pair_expires,
            pair_code_ttl_sec: self.pair_code_ttl_sec,
            local_addresses: self.local_addresses.clone(),
            total_concurrent: self.total_concurrent,
            allocation_mode: self.allocation_mode.lock().clone(),
            budget: self.budget,
            start_at_ms: state.start_at_ms,
            elapsed_sec,
            report_id: state.report_id,
            incomplete: state.incomplete,
            message: state.message.clone(),
            nodes: state
                .nodes
                .iter()
                .map(|node| node.info(state.start_at_ms, now))
                .collect(),
            pending: state
                .nodes
                .iter()
                .filter(|node| node.status == "pending")
                .filter_map(|node| {
                    Some(PendingNodeInfo {
                        request_id: node.request_id.clone()?,
                        node_id: node.id.clone(),
                        name: node.name.clone(),
                        addr: node.addr.clone(),
                        device_id: node.device_id.clone(),
                        capacity: node.capacity.clone(),
                        requested_at: node.joined_at,
                    })
                })
                .collect(),
            auto_accept: self.auto_accept.load(Ordering::SeqCst),
            discovery: self.discovery,
            pool_estimate: self.pool_estimate.clone(),
            pool_slice_limit_bytes: MAX_POOL_SLICE_BYTES as u64,
            created_at: self.created_at,
        }
    }

    fn emit_snapshot(&self) {
        self.app.emit_master(self.snapshot());
    }

    fn set_job_status(
        &self,
        status: &str,
        message: Option<&str>,
        incomplete: bool,
        finished: bool,
    ) {
        let finished_at = finished.then(now_ms);
        if let Err(error) = update_job(
            &self.db,
            self.job_id,
            status,
            message,
            incomplete,
            finished_at,
        ) {
            log::warn!("更新集群任务状态失败：{error}");
        }
        // 终态即释放监听：用户不必点「关闭」，同端口可立即重建下一个任务
        if matches!(status, "finished" | "failed") {
            // 终态前把最后一秒的采样发出去，曲线不缺尾点
            self.flush_sample_frame();
            self.mark_terminal();
            self.release_transport();
        }
    }

    fn persist_node(&self, node_id: &str) {
        let node = {
            let state = self.state.lock();
            state
                .nodes
                .iter()
                .find(|node| node.id == node_id)
                .map(|node| WorkerRow {
                    name: node.name.clone(),
                    addr: node.addr.clone(),
                    role: node.role.clone(),
                    weight: node.weight,
                    concurrent: node.concurrent,
                    status: node.status.clone(),
                    qps: node.qps,
                    cpu: node.cpu,
                    mem_mb: node.mem_mb,
                    clock_offset_ms: node.clock_offset_ms,
                    clock_uncertainty_ms: node.clock_uncertainty_ms,
                    dropped_samples: node.dropped_samples,
                    joined_at: node.joined_at,
                    last_seen_at: node.last_seen_at,
                    error: node.error.clone(),
                })
        };
        let Some(row) = node else { return };
        if let Err(error) = update_worker(&self.db, self.job_id, &row) {
            log::warn!("更新集群节点失败：{error}");
        }
    }

    /// 运行期节流落库：节点 qps/并发每秒都在变，没必要每条采样都写一次库
    ///
    /// 状态变更（就绪、完成、失联）仍走 `persist_node` 立即写，只有采样路径用这里。
    fn persist_node_throttled(&self, node_id: &str) {
        let now = now_ms();
        {
            let mut state = self.state.lock();
            let Some(node) = state.nodes.iter_mut().find(|node| node.id == node_id) else {
                return;
            };
            if now - node.last_persist_at < PERSIST_INTERVAL_MS {
                return;
            }
            node.last_persist_at = now;
        }
        self.persist_node(node_id);
    }

    fn send(&self, node_id: &str, message: ClusterMessage) -> bool {
        let state = self.state.lock();
        state
            .nodes
            .iter()
            .find(|node| node.id == node_id)
            .and_then(|node| node.tx.as_ref())
            .map(|tx| tx.send(message).is_ok())
            .unwrap_or(false)
    }

    fn reject_hello(&self, tx: &mpsc::UnboundedSender<ClusterMessage>, message: impl Into<String>) {
        let _ = tx.send(ClusterMessage::Error(ErrorPayload {
            message: message.into(),
        }));
    }

    fn register(
        &self,
        peer: SocketAddr,
        hello: HelloPayload,
        tx: mpsc::UnboundedSender<ClusterMessage>,
    ) -> Result<String, String> {
        if hello.version != PROTOCOL_VERSION {
            return Err(format!(
                "协议版本不一致：主节点 v{}，从节点 v{}，请升级后重试",
                PROTOCOL_VERSION, hello.version
            ));
        }
        if hello.join_mode.as_deref() == Some("request") {
            return self.register_request(peer, hello, tx);
        }
        let now = now_ms();
        let ttl_sec = self.pair_code_ttl_sec;
        let peer_public_key = match hello.public_key.as_deref() {
            Some(raw) => Some(super::env_bundle::decode_public(raw)?),
            None => None,
        };
        let capacity = hello.node_capacity.clone();
        let (node_id, token, probes, pair_expires) = {
            let mut state = self.state.lock();
            if state.status != "pairing" {
                return Err("任务已开始或已结束，不能再加入节点".to_string());
            }
            if hello.version != PROTOCOL_VERSION {
                return Err(format!(
                    "协议版本不一致：主节点 v{}，从节点 v{}，请升级后重试",
                    PROTOCOL_VERSION, hello.version
                ));
            }
            let code = state
                .pair_code
                .as_ref()
                .filter(|code| code.expires_at > now)
                .ok_or_else(|| "配对码已过期，请在主节点刷新配对码".to_string())?
                .code
                .clone();
            if hello.pair_code.trim() != code {
                // 配对码错误：按来源限速；累计错误达到上限直接作废当前码，避免 6 位码被枚举
                return Err(register_pair_failure(
                    &mut state,
                    &peer.ip().to_string(),
                    now,
                    ttl_sec,
                ));
            }
            state.pair_fail_total = 0;
            state.pair_failures.remove(&peer.ip().to_string());
            let name = hello.node_name.trim();
            if name.is_empty() {
                return Err("节点名不能为空".to_string());
            }
            if name.chars().count() > 64 {
                return Err("节点名不能超过 64 个字符".to_string());
            }
            if let Some(index) = state.nodes.iter().position(|node| node.name == name) {
                if state.nodes[index].is_active() {
                    return Err(format!("节点名「{name}」已被占用，请更换节点名"));
                }
                state.nodes.remove(index);
            }
            let live = state
                .nodes
                .iter()
                .filter(|node| node.role == "slave" && node.is_active())
                .count();
            if live >= MAX_NODES {
                return Err(format!("一期单任务最多 {MAX_NODES} 个从节点"));
            }
            let id = format!("node-{}", state.next_node);
            state.next_node += 1;
            let next_code = {
                let previous = code.as_str();
                let mut candidate = new_pair_code();
                while candidate == previous {
                    candidate = new_pair_code();
                }
                candidate
            };
            let pair_expires = now + ttl_sec * 1000;
            state.pair_code = Some(PairCode {
                code: next_code,
                expires_at: pair_expires,
            });
            let mut probes = Vec::with_capacity(CLOCK_SAMPLE_ROUNDS as usize);
            let mut clock_probes = HashMap::new();
            for seq in 0..CLOCK_SAMPLE_ROUNDS {
                let at = now_ms();
                clock_probes.insert(seq, at);
                probes.push(TimeProbePayload { seq, t0: at });
            }
            state.nodes.push(NodeState {
                id: id.clone(),
                name: name.to_string(),
                role: "slave".to_string(),
                addr: peer.to_string(),
                weight: 1.0,
                concurrent: 0,
                status: "pairing".to_string(),
                qps: 0.0,
                cpu: None,
                mem_mb: None,
                host_cpu_percent: None,
                host_mem_percent: None,
                net_rx_kbps: None,
                net_tx_kbps: None,
                node_threads: None,
                load1: None,
                clock_offset_ms: None,
                clock_uncertainty_ms: None,
                clock_best_error_ms: f64::INFINITY,
                clock_probes,
                clock_samples: 0,
                dropped_samples: 0,
                joined_at: now,
                last_seen_at: Some(now),
                error: None,
                tx: Some(tx.clone()),
                prepare_ok: None,
                preflight_ok: None,
                samples: Vec::new(),
                last_persist_at: 0,
                done: None,
                capacity,
                quota: 0,
                vu_start: 0,
                vu_count: 0,
                peak_qps: 0.0,
                peak_concurrent: 0,
                first_sample_ms: None,
                last_sample_ms: None,
                received_samples: 0,
                last_stats: None,
                peer_public_key,
                request_id: None,
                device_id: hello.device_id.clone(),
                missing_vars: Vec::new(),
                pool_columns: hello.pool_columns,
                clock_recheck: hello.clock_recheck,
                clock_drift_ms: 0.0,
                clock_recheck_at: None,
                pool_slice_bytes: None,
            });
            (
                id,
                Uuid::new_v4().simple().to_string(),
                probes,
                pair_expires,
            )
        };

        let ack = ClusterMessage::HelloAck(HelloAckPayload {
            version: PROTOCOL_VERSION,
            token,
            node_id: node_id.clone(),
            job_id: self.job_id,
            public_key: Some(self.x25519_public.clone()),
            pool_refresh_relay: true,
            pool_columns: true,
        });
        if tx.send(ack).is_err() {
            return Err("连接已断开".to_string());
        }
        for probe in probes {
            if tx.send(ClusterMessage::TimeProbe(probe)).is_err() {
                return Err("连接已断开".to_string());
            }
        }
        if let Err(error) = update_pair_expiry(&self.db, self.job_id, pair_expires) {
            log::warn!("更新配对码有效期失败：{error}");
        }
        self.persist_node(&node_id);
        self.emit_snapshot();
        Ok(node_id)
    }

    /// 审批模式加入：登记待接受申请，接受后仍走同一套时钟对齐
    fn register_request(
        &self,
        peer: SocketAddr,
        hello: HelloPayload,
        tx: mpsc::UnboundedSender<ClusterMessage>,
    ) -> Result<String, String> {
        let now = now_ms();
        let peer_public_key = match hello.public_key.as_deref() {
            Some(raw) => Some(super::env_bundle::decode_public(raw)?),
            None => None,
        };
        let capacity = hello.node_capacity.clone();
        let name = hello.node_name.trim();
        if name.is_empty() {
            return Err("节点名不能为空".to_string());
        }
        if name.chars().count() > 64 {
            return Err("节点名不能超过 64 个字符".to_string());
        }
        let device_id = hello
            .device_id
            .clone()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| peer.ip().to_string());
        let auto = self.auto_accept.load(Ordering::SeqCst);
        let (node_id, request_id) = {
            let mut state = self.state.lock();
            if state.status != "pairing" {
                return Err("任务已开始或已结束，不能再加入节点".to_string());
            }
            let ip = peer.ip().to_string();
            if let Some(last) = state.join_requests.get(&ip) {
                if now - *last < JOIN_RATE_MS {
                    return Err("加入申请过于频繁，请 5 秒后重试".to_string());
                }
            }
            state.join_requests.insert(ip.clone(), now);
            // 限流表只保留最近 1 分钟，避免长期任务无限增长
            state.join_requests.retain(|_, at| now - *at <= 60_000);
            let pending = state
                .nodes
                .iter()
                .filter(|node| node.role == "slave" && node.status == "pending")
                .count();
            if pending >= MAX_PENDING_JOINS {
                return Err("待接受申请过多，请先在主节点处理".to_string());
            }
            let live = state
                .nodes
                .iter()
                .filter(|node| node.role == "slave" && node.is_active() && node.status != "pending")
                .count();
            if live + pending >= MAX_NODES {
                return Err(format!("单任务最多 {MAX_NODES} 个从节点"));
            }
            if let Some(index) = state.nodes.iter().position(|node| node.name == name) {
                if state.nodes[index].is_active() {
                    return Err(format!("节点名「{name}」已被占用，请更换节点名"));
                }
                state.nodes.remove(index);
            }
            let id = format!("node-{}", state.next_node);
            state.next_node += 1;
            let request_id = Uuid::new_v4().simple().to_string();
            state.nodes.push(NodeState {
                id: id.clone(),
                name: name.to_string(),
                role: "slave".to_string(),
                addr: peer.to_string(),
                weight: 1.0,
                concurrent: 0,
                status: "pending".to_string(),
                qps: 0.0,
                cpu: None,
                mem_mb: None,
                host_cpu_percent: None,
                host_mem_percent: None,
                net_rx_kbps: None,
                net_tx_kbps: None,
                node_threads: None,
                load1: None,
                clock_offset_ms: None,
                clock_uncertainty_ms: None,
                clock_best_error_ms: f64::INFINITY,
                clock_probes: HashMap::new(),
                clock_samples: 0,
                dropped_samples: 0,
                joined_at: now,
                last_seen_at: Some(now),
                error: None,
                tx: Some(tx.clone()),
                prepare_ok: None,
                preflight_ok: None,
                samples: Vec::new(),
                last_persist_at: 0,
                done: None,
                capacity,
                quota: 0,
                vu_start: 0,
                vu_count: 0,
                peak_qps: 0.0,
                peak_concurrent: 0,
                first_sample_ms: None,
                last_sample_ms: None,
                received_samples: 0,
                last_stats: None,
                peer_public_key,
                request_id: Some(request_id.clone()),
                device_id: Some(device_id),
                missing_vars: Vec::new(),
                pool_columns: hello.pool_columns,
                clock_recheck: hello.clock_recheck,
                clock_drift_ms: 0.0,
                clock_recheck_at: None,
                pool_slice_bytes: None,
            });
            (id, request_id)
        };
        let pending = ClusterMessage::HelloPending(HelloPendingPayload {
            request_id,
            message: Some("等待主机接受".to_string()),
            timeout_sec: JOIN_WAIT_SEC,
        });
        if tx.send(pending).is_err() {
            self.remove_pending(&node_id);
            return Err("连接已断开".to_string());
        }
        self.emit_snapshot();
        if auto {
            if let Err(error) = self.approve_join(&node_id) {
                log::warn!("自动接受节点失败：{error}");
            }
        }
        Ok(node_id)
    }

    /// 接受待审批节点：发送 HelloAck 与 5 轮时钟探测
    fn approve_join(&self, node_id: &str) -> Result<(), String> {
        let now = now_ms();
        let (token, probes) = {
            let mut state = self.state.lock();
            if state.status != "pairing" {
                return Err("当前任务不在配对阶段".to_string());
            }
            let Some(index) = state.nodes.iter().position(|node| node.id == node_id) else {
                return Err("节点不存在或连接已断开".to_string());
            };
            if state.nodes[index].status != "pending" {
                return Err("该节点不在待接受状态".to_string());
            }
            let mut probes = Vec::with_capacity(CLOCK_SAMPLE_ROUNDS as usize);
            for seq in 0..CLOCK_SAMPLE_ROUNDS {
                let at = now_ms();
                state.nodes[index].clock_probes.insert(seq, at);
                probes.push(TimeProbePayload { seq, t0: at });
            }
            state.nodes[index].status = "pairing".to_string();
            state.nodes[index].request_id = None;
            state.nodes[index].joined_at = now;
            state.nodes[index].last_seen_at = Some(now);
            state.nodes[index].error = None;
            (Uuid::new_v4().simple().to_string(), probes)
        };
        let ack = ClusterMessage::HelloAck(HelloAckPayload {
            version: PROTOCOL_VERSION,
            token,
            node_id: node_id.to_string(),
            job_id: self.job_id,
            public_key: Some(self.x25519_public.clone()),
            pool_refresh_relay: true,
            pool_columns: true,
        });
        if !self.send(node_id, ack) {
            return Err("节点连接已断开".to_string());
        }
        for probe in probes {
            if !self.send(node_id, ClusterMessage::TimeProbe(probe)) {
                return Err("节点连接已断开".to_string());
            }
        }
        self.persist_node(node_id);
        self.emit_snapshot();
        Ok(())
    }

    /// 拒绝待审批节点：通知原因并关闭连接
    fn reject_join(&self, node_id: &str, reason: Option<String>) -> Result<(), String> {
        let message = reason
            .filter(|text| !text.trim().is_empty())
            .unwrap_or_else(|| "主机拒绝了加入申请".to_string());
        let _ = self.send(
            node_id,
            ClusterMessage::HelloRejected(HelloRejectedPayload {
                reason: message.clone(),
            }),
        );
        let mut state = self.state.lock();
        let Some(node) = state.nodes.iter_mut().find(|node| node.id == node_id) else {
            return Err("节点不存在".to_string());
        };
        node.status = "rejected".to_string();
        node.error = Some(message);
        node.request_id = None;
        node.tx = None;
        drop(state);
        self.emit_snapshot();
        Ok(())
    }

    /// 删除未被接受的申请（连接断开时清理）
    fn remove_pending(&self, node_id: &str) {
        let mut state = self.state.lock();
        state
            .nodes
            .retain(|node| !(node.id == node_id && node.status == "pending"));
        drop(state);
        self.emit_snapshot();
    }

    /// 本次任务自动接受开关
    fn set_auto_accept(&self, enabled: bool) {
        self.auto_accept.store(enabled, Ordering::SeqCst);
        self.emit_snapshot();
    }

    fn touch(&self, node_id: &str) {
        let mut state = self.state.lock();
        if let Some(node) = state.nodes.iter_mut().find(|node| node.id == node_id) {
            node.last_seen_at = Some(now_ms());
        }
    }

    fn handle_message(self: &Arc<Self>, node_id: &str, message: ClusterMessage) {
        self.touch(node_id);
        match message {
            ClusterMessage::TimeAck(payload) => self.on_time_ack(node_id, payload),
            ClusterMessage::PrepareAck(payload) => self.on_prepare_ack(node_id, payload),
            ClusterMessage::PoolRefreshRequest(payload) => {
                self.spawn_pool_refresh(node_id.to_string(), payload)
            }
            ClusterMessage::Preflight(payload) => self.on_preflight(node_id, payload),
            ClusterMessage::Sample(payload) => self.on_sample(node_id, payload),
            ClusterMessage::Done(payload) => self.on_done(node_id, payload),
            ClusterMessage::Ping(payload) => {
                if let Some(stats) = payload.stats.clone() {
                    let mut state = self.state.lock();
                    if let Some(node) = state.nodes.iter_mut().find(|node| node.id == node_id) {
                        apply_engine_stats(node, stats);
                    }
                }
                let _ = self.send(
                    node_id,
                    ClusterMessage::Pong(PongPayload {
                        at_ms: payload.at_ms,
                        stats: None,
                    }),
                );
            }
            ClusterMessage::Pong(payload) => {
                if let Some(stats) = payload.stats {
                    let mut state = self.state.lock();
                    if let Some(node) = state.nodes.iter_mut().find(|node| node.id == node_id) {
                        apply_engine_stats(node, stats);
                    }
                }
            }
            ClusterMessage::Abort(payload) => self.on_remote_abort(node_id, payload.reason),
            ClusterMessage::Hello(_) => {
                let _ = self.send(
                    node_id,
                    ClusterMessage::Error(ErrorPayload {
                        message: "重复的 hello".to_string(),
                    }),
                );
            }
            _ => {}
        }
    }

    /// 从节点请求刷新 token：后台单飞，避免阻塞连接读取循环
    fn spawn_pool_refresh(self: &Arc<Self>, node_id: String, payload: PoolRefreshRequestPayload) {
        let session = self.clone();
        tauri::async_runtime::spawn(async move {
            let result = session
                .pool_refresh
                .refresh(&payload.pool, payload.item_id, payload.item_index)
                .await;
            // 刷新结果要写明写入列：优先按池列定义解析（token 优先否则首列）
            let column = session
                .db
                .get()
                .ok()
                .and_then(|conn| crate::services::pool::pool_columns(&conn, &payload.pool).ok())
                .map(|columns| crate::commands::stress::refresh_column_of(&columns))
                .or_else(|| match &result {
                    Ok((_, _, Some(column))) => Some(column.clone()),
                    _ => None,
                })
                .unwrap_or_else(|| "value".to_string());
            let (tx, peer) = {
                let state = session.state.lock();
                let Some(node) = state.nodes.iter().find(|node| node.id == node_id) else {
                    return;
                };
                (node.tx.clone(), node.peer_public_key)
            };
            let Some(tx) = tx else {
                return;
            };
            let message = session.pool_refresh_result(payload.request_id, peer, result, &column);
            let _ = tx.send(ClusterMessage::PoolRefreshResult(message));
        });
    }

    /// 刷新结果：成功用会话密钥加密回传，失败只回错误文案（不含 token）
    ///
    /// `column` 是本次刷新写入的列名，成功与失败都带上，便于从节点对齐 `data`。
    fn pool_refresh_result(
        &self,
        request_id: String,
        peer: Option<[u8; 32]>,
        result: Result<(String, i64, Option<String>), String>,
        column: &str,
    ) -> PoolRefreshResultPayload {
        let (value, expires_at) = match result {
            Ok((value, expires_at, _)) => (value, expires_at),
            Err(error) => {
                return PoolRefreshResultPayload {
                    request_id,
                    ok: false,
                    error: Some(error),
                    nonce: String::new(),
                    payload: String::new(),
                    digest: String::new(),
                    column: Some(column.to_string()),
                }
            }
        };
        let Some(peer) = peer else {
            return PoolRefreshResultPayload {
                request_id,
                ok: false,
                error: Some("缺少会话密钥，无法回传刷新结果".to_string()),
                nonce: String::new(),
                payload: String::new(),
                digest: String::new(),
                column: Some(column.to_string()),
            };
        };
        let plaintext = serde_json::json!({
            "value": value,
            "expiresAt": expires_at,
        })
        .to_string();
        match super::env_bundle::encrypt_bundle(&self.x25519_secret, &peer, plaintext.as_bytes()) {
            Ok((nonce, payload, digest)) => PoolRefreshResultPayload {
                request_id,
                ok: true,
                error: None,
                nonce,
                payload,
                digest,
                column: Some(column.to_string()),
            },
            Err(error) => PoolRefreshResultPayload {
                request_id,
                ok: false,
                error: Some(error),
                nonce: String::new(),
                payload: String::new(),
                digest: String::new(),
                column: Some(column.to_string()),
            },
        }
    }

    /// 时钟回包：配对阶段取最优样本，运行期只更新漂移（不动机器已对齐的起跑时刻）
    fn on_time_ack(&self, node_id: &str, payload: TimeAckPayload) {
        let mut paired = false;
        let mut drift_updated = false;
        {
            let mut state = self.state.lock();
            let recheck = payload.seq >= RECHECK_SEQ_BASE;
            if state.status != "pairing" && !recheck {
                return;
            }
            let Some(node) = state.nodes.iter_mut().find(|node| node.id == node_id) else {
                return;
            };
            let Some(t0) = node.clock_probes.remove(&payload.seq) else {
                return;
            };
            if t0 != payload.t0 {
                return;
            }
            let t2 = now_ms();
            let (offset, uncertainty) = clock_metrics(t0, payload.t1, t2);
            if recheck {
                apply_clock_recheck(node, offset, t2);
                drift_updated = true;
            } else {
                node.clock_samples += 1;
                if uncertainty < node.clock_best_error_ms {
                    node.clock_best_error_ms = uncertainty;
                    node.clock_offset_ms = Some(offset);
                    node.clock_uncertainty_ms = Some(uncertainty);
                }
                if node.clock_samples >= CLOCK_SAMPLE_ROUNDS {
                    node.status = "pairReady".to_string();
                    paired = true;
                }
            }
        }
        if paired {
            self.persist_node(node_id);
            self.emit_snapshot();
        } else if drift_updated {
            self.emit_snapshot();
        }
    }

    /// 向单个节点发一拍时钟复测：只在施压中用，判定漂移是否超过告警阈值
    fn probe_clock_drift(&self, node_id: &str) {
        let seq = RECHECK_SEQ_BASE + self.recheck_seq.fetch_add(1, Ordering::Relaxed) as u32;
        let at = now_ms();
        let allowed = {
            let mut state = self.state.lock();
            if !matches!(state.status.as_str(), "ready" | "running" | "preparing") {
                return;
            }
            match state.nodes.iter_mut().find(|node| node.id == node_id) {
                Some(node) if node.clock_recheck => {
                    node.clock_probes.insert(seq, at);
                    true
                }
                _ => false,
            }
        };
        if allowed {
            self.send(
                node_id,
                ClusterMessage::TimeProbe(TimeProbePayload { seq, t0: at }),
            );
        }
    }

    fn on_prepare_ack(&self, node_id: &str, payload: PrepareAckPayload) {
        let mut failure: Option<String> = None;
        let mut ready = false;
        {
            let mut state = self.state.lock();
            if state.status != "preparing" {
                return;
            }
            let Some(node) = state.nodes.iter_mut().find(|node| node.id == node_id) else {
                return;
            };
            let node_name = node.name.clone();
            if !payload.ok {
                let detail = payload
                    .error
                    .clone()
                    .filter(|text| !text.trim().is_empty())
                    .unwrap_or_else(|| "从节点拒绝执行".to_string());
                let message = if payload.missing_vars.is_empty() {
                    detail
                } else {
                    format!("{detail}；缺少变量：{}", payload.missing_vars.join("、"))
                };
                node.status = "rejected".to_string();
                node.tx = None;
                node.error = Some(message.clone());
                node.missing_vars = payload.missing_vars.clone();
                failure = Some(format!("节点「{node_name}」准备失败：{message}"));
            } else {
                node.prepare_ok = Some(true);
                node.missing_vars.clear();
                if payload.accepted_concurrent > 0 {
                    node.concurrent = payload.accepted_concurrent;
                }
                // 预检可能先于回执到达，避免把已就绪节点退回 preflight
                node.status = if node.preflight_ok == Some(true) {
                    "ready".to_string()
                } else {
                    "preflight".to_string()
                };
            }
            if setup_ready(&state) {
                state.status = "ready".to_string();
                ready = true;
            }
        }
        self.persist_node(node_id);
        if let Some(message) = failure {
            self.fail(message);
            return;
        }
        if ready {
            self.set_job_status("ready", None, false, false);
            self.emit_snapshot();
        }
    }

    fn on_preflight(&self, node_id: &str, payload: PreflightPayload) {
        let mut failure: Option<String> = None;
        let mut ready = false;
        {
            let mut state = self.state.lock();
            if !matches!(state.status.as_str(), "preparing" | "ready") {
                return;
            }
            let Some(node) = state.nodes.iter_mut().find(|node| node.id == node_id) else {
                return;
            };
            let node_name = node.name.clone();
            if !payload.ok {
                let detail = payload
                    .error
                    .clone()
                    .filter(|text| !text.trim().is_empty())
                    .unwrap_or_else(|| "目标预检失败".to_string());
                node.status = "rejected".to_string();
                node.tx = None;
                node.error = Some(detail.clone());
                failure = Some(format!("节点「{node_name}」无法访问目标：{detail}"));
            } else {
                node.preflight_ok = Some(true);
                if node.prepare_ok == Some(true) {
                    node.status = "ready".to_string();
                }
            }
            if setup_ready(&state) {
                state.status = "ready".to_string();
                ready = true;
            }
        }
        self.persist_node(node_id);
        if let Some(message) = failure {
            self.fail(message);
            return;
        }
        if ready {
            self.set_job_status("ready", None, false, false);
            self.emit_snapshot();
        }
    }

    fn on_sample(&self, node_id: &str, payload: SamplePayload) {
        {
            let mut state = self.state.lock();
            let Some(node) = state.nodes.iter_mut().find(|node| node.id == node_id) else {
                return;
            };
            if !matches!(node.status.as_str(), "running" | "done") {
                return;
            }
            node.qps = payload.qps;
            node.concurrent = payload.concurrent;
            node.received_samples += 1;
            let now = now_ms();
            node.first_sample_ms.get_or_insert(now);
            node.last_sample_ms = Some(now);
            if payload.qps > node.peak_qps {
                node.peak_qps = payload.qps;
            }
            if payload.concurrent > node.peak_concurrent {
                node.peak_concurrent = payload.concurrent;
            }
            // 顺带把资源用量写回节点状态：运行期节点表才有 CPU/内存/网络可看
            if let Some(host) = &payload.host {
                node.cpu = Some(host.cpu);
                node.mem_mb = Some(host.mem_mb);
                node.host_cpu_percent = Some(host.host_cpu_percent);
                node.host_mem_percent = Some(host.host_mem_percent);
                node.net_rx_kbps = Some(host.net_rx_kbps);
                node.net_tx_kbps = Some(host.net_tx_kbps);
                if host.threads > 0 {
                    node.node_threads = Some(host.threads);
                }
                node.load1 = Some(host.load1);
            }
            // 只保留最近 SAMPLE_RETAIN 秒：AI 诊断最多取 300 条，长任务不再无限增长
            if node.samples.len() >= SAMPLE_RETAIN {
                let overflow = node.samples.len() + 1 - SAMPLE_RETAIN;
                node.samples.drain(..overflow);
            }
            node.samples.push(payload.clone());
        }
        self.persist_node_throttled(node_id);
        // 先攒进帧缓冲：由 watchdog 每秒统一发一次，避免逐条 IPC
        {
            let mut frame = self.sample_frame.lock();
            frame.sec = payload.elapsed_sec;
            frame.nodes.insert(
                node_id.to_string(),
                NodeSampleLite {
                    node_id: node_id.to_string(),
                    qps: payload.qps,
                    concurrent: payload.concurrent,
                    avg_rt: payload.avg_rt,
                    error_count: payload.error_count,
                    cpu: payload.host.as_ref().map(|host| host.cpu),
                    mem_mb: payload.host.as_ref().map(|host| host.mem_mb),
                },
            );
        }
    }

    /// 把本秒累积的采样发成一帧；没有新采样时什么都不做
    fn flush_sample_frame(&self) {
        let (sec, nodes) = {
            let mut buffer = self.sample_frame.lock();
            if buffer.nodes.is_empty() {
                return;
            }
            let mut nodes: Vec<NodeSampleLite> =
                buffer.nodes.drain().map(|(_, node)| node).collect();
            nodes.sort_by(|a, b| a.node_id.cmp(&b.node_id));
            (buffer.sec, nodes)
        };
        self.app.emit_sample_frame(ClusterSampleFrame {
            job_id: self.job_id,
            sec,
            nodes,
        });
    }

    fn on_done(&self, node_id: &str, payload: DonePayload) {
        {
            let mut state = self.state.lock();
            let Some(node) = state.nodes.iter_mut().find(|node| node.id == node_id) else {
                return;
            };
            let elapsed_ms = payload.end_ms.saturating_sub(payload.start_ms).max(0) as f64;
            node.qps = if elapsed_ms > 0.0 {
                payload.total as f64 / (elapsed_ms / 1000.0)
            } else {
                0.0
            };
            node.dropped_samples = payload.dropped_samples;
            node.cpu = Some(payload.cpu_peak);
            node.mem_mb = Some(payload.mem_peak_mb);
            node.status = "done".to_string();
            node.done = Some(payload);
            node.tx = None;
        }
        self.persist_node(node_id);
        self.emit_snapshot();
        // 从节点全部结算且主节点本地已结束时，在这里触发合并落库
        self.maybe_finalize();
    }

    fn on_remote_abort(&self, node_id: &str, reason: Option<String>) {
        let detail = reason
            .clone()
            .filter(|text| !text.trim().is_empty())
            .unwrap_or_else(|| "从节点主动中止".to_string());
        let mut failure: Option<String> = None;
        {
            let mut state = self.state.lock();
            let in_setup = matches!(state.status.as_str(), "preparing" | "ready");
            let running = state.status == "running";
            let Some(node) = state.nodes.iter_mut().find(|node| node.id == node_id) else {
                return;
            };
            node.error = Some(detail.clone());
            node.status = "left".to_string();
            node.tx = None;
            if in_setup {
                failure = Some(format!("节点「{}」中止：{detail}", node.name));
            } else if running {
                state.incomplete = true;
            }
        }
        self.persist_node(node_id);
        if let Some(message) = failure {
            self.fail(message);
        } else {
            self.emit_snapshot();
            self.maybe_finalize();
        }
    }

    fn on_disconnect(&self, node_id: &str) {
        {
            let mut state = self.state.lock();
            if matches!(state.status.as_str(), "finished" | "failed") {
                return;
            }
            let Some(index) = state.nodes.iter().position(|node| node.id == node_id) else {
                return;
            };
            // 待审批节点还没配对：直接移除，避免占用待接受列表
            if state.nodes[index].status == "pending" {
                state.nodes.remove(index);
                drop(state);
                self.emit_snapshot();
                return;
            }
            let node = &mut state.nodes[index];
            if !node.is_active() {
                return;
            }
            node.status = "lost".to_string();
            node.tx = None;
            node.error = Some("连接断开".to_string());
            if state.status == "running" {
                state.incomplete = true;
            }
        }
        self.persist_node(node_id);
        self.emit_snapshot();
        self.maybe_finalize();
    }

    async fn prepare(
        &self,
        mode: Option<String>,
        weights: Vec<ClusterNodeWeight>,
    ) -> Result<(), String> {
        let allocations = {
            let mut state = self.state.lock();
            if state.status != "pairing" {
                return Err(match state.status.as_str() {
                    "preparing" => "节点配置已下发，请等待回执".to_string(),
                    "ready" => "节点已全部就绪，可以直接启动".to_string(),
                    _ => "当前任务状态不允许重新准备节点".to_string(),
                });
            }
            let slaves = state
                .nodes
                .iter()
                .filter(|node| node.role == "slave")
                .count();
            if slaves == 0 {
                return Err("至少需要加入一个从节点后才能准备".to_string());
            }
            let not_paired: Vec<&str> = state
                .nodes
                .iter()
                .filter(|node| node.role == "slave" && node.status != "pairReady")
                .map(|node| node.name.as_str())
                .collect();
            if !not_paired.is_empty() {
                return Err(format!(
                    "以下节点尚未完成时钟对齐：{}",
                    not_paired.join("、")
                ));
            }
            // 能力感知默认权重：主节点用本机预算，从节点用上报预算；
            // 显式传入权重时以用户为准（custom）。
            let participants: Vec<(String, f64)> = state
                .nodes
                .iter()
                .map(|node| {
                    let default = if node.role == "master" {
                        self.budget as f64
                    } else {
                        node.capacity
                            .as_ref()
                            .map(|capacity| capacity.machine_limit as f64)
                            .filter(|value| *value > 0.0)
                            .unwrap_or(512.0)
                    };
                    (node.id.clone(), default)
                })
                .collect();
            // 显式模式优先；旧前端未传时按是否携带权重推断
            let requested = weights;
            let mode = normalize_allocation_mode(mode.as_deref(), !requested.is_empty())?;
            *self.allocation_mode.lock() = mode.to_string();
            let weights = resolve_weights(&participants, &requested, mode)?;
            // 每节点硬上限：主节点预算 / 从节点上报预算
            let caps: Vec<(String, u32)> = state
                .nodes
                .iter()
                .map(|node| {
                    let cap = if node.role == "master" {
                        self.budget
                    } else {
                        node.capacity
                            .as_ref()
                            .map(|capacity| capacity.machine_limit)
                            .filter(|value| *value > 0)
                            .unwrap_or(u32::MAX / 4)
                    };
                    (node.id.clone(), cap)
                })
                .collect();
            // 每节点 min/max：custom 由用户填写，其余模式为 1..本机预算
            let limits: Vec<(String, u32, u32)> = state
                .nodes
                .iter()
                .map(|node| {
                    let cap = caps
                        .iter()
                        .find(|(id, _)| id == &node.id)
                        .map(|(_, cap)| *cap)
                        .unwrap_or(u32::MAX);
                    let requested = requested.iter().find(|item| item.node_id == node.id);
                    let min = requested
                        .and_then(|item| item.min_concurrent)
                        .unwrap_or(1)
                        .max(1);
                    let max = requested
                        .and_then(|item| item.max_concurrent)
                        .unwrap_or(cap)
                        .min(cap);
                    (node.id.clone(), min, max)
                })
                .collect();
            let allocations = allocate_concurrency_capped(
                self.total_concurrent,
                &participants,
                &weights,
                &caps,
                &limits,
            )?;
            let mut vu_cursor = 0u32;
            for node in &mut state.nodes {
                if let Some((_, weight)) = weights.iter().find(|(id, _)| id == &node.id) {
                    node.weight = *weight;
                }
                if let Some((_, concurrent)) = allocations.iter().find(|(id, _)| id == &node.id) {
                    node.concurrent = *concurrent;
                }
                node.quota = caps
                    .iter()
                    .find(|(id, _)| id == &node.id)
                    .map(|(_, cap)| *cap)
                    .unwrap_or(0);
                node.vu_start = vu_cursor;
                node.vu_count = node.concurrent;
                vu_cursor = vu_cursor.saturating_add(node.concurrent);
                node.prepare_ok = None;
                node.preflight_ok = None;
                node.error = None;
                node.missing_vars.clear();
                node.pool_slice_bytes = None;
                if node.role == "master" {
                    node.status = "ready".to_string();
                } else {
                    node.status = "preparing".to_string();
                }
            }
            state.status = "preparing".to_string();
            allocations
        };

        // 主节点本地引擎与从节点同一时点准备：环境缺失、变量未解析等问题在启动前暴露
        let master_concurrent = allocations
            .iter()
            .find(|(id, _)| id == "master")
            .map(|(_, concurrent)| *concurrent)
            .unwrap_or(1);
        if let Err(message) = self.prepare_local_engine(master_concurrent) {
            self.fail(message.clone());
            return Err(message);
        }

        let weight_map: HashMap<String, f64> = {
            let state = self.state.lock();
            state
                .nodes
                .iter()
                .map(|node| (node.id.clone(), node.weight))
                .collect()
        };
        if let Err(error) = update_weights(&self.db, self.job_id, &weight_map) {
            log::warn!("记录节点权重失败：{error}");
        }
        self.set_job_status("preparing", None, false, false);

        // 自定义列能力校验：列出全部不支持的从节点，给出升级或降级绑定方式两种处置
        let needs_columns = self
            .local_config
            .pool_bindings
            .iter()
            .any(|binding| !binding.column.trim().is_empty());
        if needs_columns {
            let legacy: Vec<String> = {
                let state = self.state.lock();
                state
                    .nodes
                    .iter()
                    .filter(|node| node.role == "slave" && node.is_active() && !node.pool_columns)
                    .map(|node| node.name.clone())
                    .collect()
            };
            if !legacy.is_empty() {
                let message = format!(
                    "从节点 {} 版本较低，不支持数据池自定义列或自动注入。请升级后重新加入；\
或把池绑定改为「取 value 列」的旧式绑定后重试",
                    legacy.join("、")
                );
                self.fail(message.clone());
                return Err(message);
            }
        }

        for (node_id, concurrent) in &allocations {
            let payload = {
                let mut state = self.state.lock();
                let node_count = state.nodes.len();
                let total_vus = state.nodes.iter().map(|node| node.concurrent).sum::<u32>();
                let Some((index, node)) = state
                    .nodes
                    .iter()
                    .enumerate()
                    .find(|(_, node)| node.id == *node_id)
                else {
                    continue;
                };
                if node.role == "master" {
                    continue;
                }
                let pool_check: Result<(Vec<PoolSlice>, u64), String> =
                    if self.local_config.pool_bindings.is_empty() {
                        Ok((Vec::new(), 0))
                    } else {
                        validate_vu_ranges(&state.nodes, total_vus)
                            .and_then(|_| self.pool_slices_for(index, node, node_count, total_vus))
                    };
                let env_bundle = self.env_bundle_for(node);
                let (vu_start, vu_count) = (node.vu_start, node.vu_count);
                pool_check.map(|(pool_slices, slice_bytes)| {
                    state.nodes[index].pool_slice_bytes = Some(slice_bytes);
                    PreparePayload {
                        job_id: self.job_id,
                        plan_name: self.plan_name.clone(),
                        config: self.config.clone(),
                        env_name: self.env_name.clone(),
                        var_policy: crate::util::normalize_var_policy(
                            self.local_config.var_policy.as_deref(),
                        )
                        .to_string(),
                        required_vars: self.required_vars.clone(),
                        concurrent: *concurrent,
                        total_concurrent: self.total_concurrent,
                        env_bundle,
                        vu_start,
                        vu_count,
                        pool_slices,
                    }
                })
            };
            let payload = match payload {
                Ok(payload) => payload,
                Err(message) => {
                    self.fail(message.clone());
                    return Err(message);
                }
            };
            if !self.send(node_id, ClusterMessage::Prepare(payload)) {
                self.fail(format!("节点连接已断开，无法下发配置：{node_id}"));
                return Err("节点连接已断开，无法下发配置".to_string());
            }
        }
        self.emit_snapshot();

        let deadline = Instant::now() + SETUP_TIMEOUT;
        loop {
            let (status, message, waiting) = {
                let state = self.state.lock();
                let waiting: Vec<String> = state
                    .nodes
                    .iter()
                    .filter(|node| {
                        node.role == "slave"
                            && !matches!(
                                node.status.as_str(),
                                "ready" | "lost" | "left" | "rejected"
                            )
                    })
                    .map(|node| node.name.clone())
                    .collect();
                (state.status.clone(), state.message.clone(), waiting)
            };
            match status.as_str() {
                "ready" => return Ok(()),
                "failed" | "finished" => {
                    return Err(message.unwrap_or_else(|| "节点准备失败".to_string()))
                }
                _ => {}
            }
            if Instant::now() >= deadline {
                let detail = if waiting.is_empty() {
                    "等待节点回执超时".to_string()
                } else {
                    format!("等待节点回执超时：{}", waiting.join("、"))
                };
                self.fail(detail.clone());
                return Err(detail);
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    fn launch(self: &Arc<Self>) -> Result<(), String> {
        if self.local_prepared.lock().is_none() {
            return Err("主节点本地压测尚未准备完成，请重新下发配置".to_string());
        }
        let at_ms = now_ms() + START_LEAD_MS;
        {
            let mut state = self.state.lock();
            if state.status != "ready" {
                return Err(match state.status.as_str() {
                    "pairing" => "请先配对从节点并下发配置".to_string(),
                    "preparing" => "节点仍在准备中，请等待全部就绪".to_string(),
                    "running" => "任务已经在运行".to_string(),
                    _ => "当前任务状态不允许启动".to_string(),
                });
            }
            state.status = "running".to_string();
            state.start_at_ms = Some(at_ms);
            // 每节点带上自己的时钟偏移：从节点按本机时间换算起跑时刻
            let mut senders: Vec<(mpsc::UnboundedSender<ClusterMessage>, f64)> = Vec::new();
            for node in state.nodes.iter_mut() {
                if node.role != "slave" || node.status != "ready" {
                    continue;
                }
                node.status = "running".to_string();
                if let Some(tx) = node.tx.clone() {
                    senders.push((tx, node.clock_offset_ms.unwrap_or(0.0)));
                }
            }
            for (tx, offset_ms) in senders {
                let _ = tx.send(ClusterMessage::Start(StartPayload { at_ms, offset_ms }));
            }
            if let Some(master) = state.nodes.iter_mut().find(|node| node.role == "master") {
                master.status = "running".to_string();
            }
        }
        if let Err(error) = update_job_start(&self.db, self.job_id, at_ms) {
            log::warn!("记录集群任务起跑时间失败：{error}");
        }
        // 主节点自己也施压：与从节点同一 atMs 起跑，采样通过 sink 进入合并
        if let Some(prepared) = self.local_prepared.lock().take() {
            let watch = self.local_watch.lock().clone();
            let session = Arc::clone(self);
            tauri::async_runtime::spawn(async move {
                session.run_local(prepared, at_ms, watch).await;
            });
        }
        self.emit_snapshot();
        Ok(())
    }

    /// 主节点本地引擎：每秒采样并入主节点节点状态，结束后结算报告
    async fn run_local(
        self: Arc<Self>,
        prepared: PreparedStress,
        at_ms: i64,
        watch: Option<Arc<AtomicBool>>,
    ) {
        let session = Arc::clone(&self);
        let sink: EngineSampleSink = Arc::new(move |sample| {
            session.on_sample("master", sample);
        });
        let outcome = run_engine(
            prepared,
            0,
            None,
            watch.as_ref(),
            EngineOptions {
                start_at_ms: Some(at_ms),
                sink: Some(sink),
            },
        )
        .await;
        self.on_local_done(outcome);
    }

    /// 主节点环境变量（解密后的明文只用于本次下发，不落盘）
    fn master_env(&self) -> Option<(HashMap<String, String>, std::collections::HashSet<String>)> {
        let conn = self.db.get().ok()?;
        let env_id = match self.local_config.env_id {
            Some(id) => Some(id),
            None => {
                let name = self.env_name.as_deref()?.trim();
                if name.is_empty() {
                    return None;
                }
                conn.query_row(
                    "SELECT id FROM environments WHERE name = ?1
                     ORDER BY is_active DESC, id ASC LIMIT 1",
                    params![name],
                    |row| row.get::<_, i64>(0),
                )
                .optional()
                .ok()
                .flatten()
            }
        }?;
        crate::commands::http::load_vars_with_secrets(&conn, env_id).ok()
    }

    /// 变量体检：只读汇总每个变量的来源、主节点是否有值和各从节点状态。
    /// 不返回任何变量值，也不触发下发。
    fn var_audit(&self) -> super::protocol::VarAuditReport {
        use super::protocol::{VarAuditItem, VarAuditNode};

        let texts = config_texts(&self.local_config);
        let classified = crate::util::required_vars_in_texts(texts.iter().map(String::as_str));
        let master_env = self.master_env();
        let os_allow = self
            .app
            .try_state()
            .map(|state| state.settings().stress_os_env_allow)
            .unwrap_or_default();

        let state = self.state.lock();
        let nodes: Vec<VarAuditNode> = state
            .nodes
            .iter()
            .filter(|node| node.role == "slave")
            .map(|node| VarAuditNode {
                id: node.id.clone(),
                name: node.name.clone(),
                addr: node.addr.clone(),
                status: node.status.clone(),
            })
            .collect();

        let probe_state = |names: &[String]| {
            let mut map = HashMap::new();
            for node in state.nodes.iter().filter(|node| node.role == "slave") {
                map.insert(node.id.clone(), var_node_state(node, names));
            }
            map
        };

        let mut items: Vec<VarAuditItem> = Vec::new();
        let mut seen: Vec<String> = Vec::new();

        for name in &classified.user {
            if seen.contains(name) {
                continue;
            }
            seen.push(name.clone());
            let has = master_env
                .as_ref()
                .map(|(vars, _)| vars.contains_key(name))
                .unwrap_or(false);
            items.push(VarAuditItem {
                name: name.clone(),
                kind: "user".to_string(),
                master_has_value: has,
                source: if has { "env" } else { "none" }.to_string(),
                message: if has {
                    None
                } else {
                    Some("主节点环境里没有这个变量；从节点会按缺失拦截".to_string())
                },
                node_states: probe_state(&[name.clone()]),
            });
        }

        for name in &classified.secret {
            if seen.contains(name) {
                continue;
            }
            seen.push(name.clone());
            let has = master_env
                .as_ref()
                .map(|(vars, _)| vars.contains_key(name))
                .unwrap_or(false);
            items.push(VarAuditItem {
                name: name.clone(),
                kind: "secret".to_string(),
                master_has_value: has,
                source: if has { "env" } else { "none" }.to_string(),
                message: Some(if has {
                    "值通过加密环境包下发，报告与日志不会出现明文".to_string()
                } else {
                    "主节点环境里没有这个 secret；从节点会按缺失拦截".to_string()
                }),
                node_states: probe_state(&[name.clone()]),
            });
        }

        for name in &classified.os {
            let allowed = crate::util::os_var_allowed(&os_allow, name);
            items.push(VarAuditItem {
                name: name.clone(),
                kind: "os".to_string(),
                master_has_value: allowed && std::env::var(name).is_ok(),
                source: "os".to_string(),
                message: Some(if allowed {
                    "由各节点读取本机环境变量".to_string()
                } else {
                    "未加入 OS 变量白名单（设置 → 压测 → OS 变量），各节点会按缺失拦截".to_string()
                }),
                node_states: probe_state(&[format!("os:{name}")]),
            });
        }

        for name in &classified.pools {
            let binding = self
                .local_config
                .pool_bindings
                .iter()
                .find(|binding| binding.target == *name);
            let message = match binding {
                Some(binding) => {
                    let mode = match binding.mode.as_str() {
                        "per_vu" => "每 VU 固定",
                        "per_request" => "逐请求轮换",
                        "random" => "随机",
                        _ => "默认策略",
                    };
                    let refresh = if binding.refresh_on_401 {
                        "，401 时自动切换"
                    } else {
                        ""
                    };
                    Some(format!("由池「{}」按{mode}注入{refresh}", binding.pool))
                }
                None => Some("未绑定数据池，运行时会按缺少变量处理".to_string()),
            };
            items.push(VarAuditItem {
                name: name.clone(),
                kind: "pool".to_string(),
                master_has_value: binding.is_some(),
                source: if binding.is_some() { "pool" } else { "none" }.to_string(),
                message,
                node_states: probe_state(&[name.clone()]),
            });
        }

        let (env_var_count, secret_count) = master_env
            .as_ref()
            .map(|(vars, secrets)| (vars.len() as u32, secrets.len() as u32))
            .unwrap_or((0, 0));

        super::protocol::VarAuditReport {
            job_id: self.job_id,
            env_name: self.env_name.clone(),
            var_policy: crate::util::normalize_var_policy(self.local_config.var_policy.as_deref())
                .to_string(),
            env_var_count,
            secret_count,
            nodes,
            items,
        }
    }

    /// 报告配置快照追加环境元信息（名称、摘要、变量数、secret 数、策略），不记录变量值
    fn decorate_env_snapshot(&self, merged: &mut merge::MergedReport) {
        let Some((vars, secrets)) = self.master_env() else {
            return;
        };
        let mut pairs: Vec<(&String, &String)> = vars.iter().collect();
        pairs.sort_by(|left, right| left.0.cmp(right.0));
        let mut canonical = String::new();
        for (key, value) in pairs {
            let kind = if secrets.contains(key) {
                "secret"
            } else {
                "var"
            };
            canonical.push_str(kind);
            canonical.push(':');
            canonical.push_str(key);
            canonical.push('=');
            canonical.push_str(value);
            canonical.push('\n');
        }
        let Ok(mut config) = serde_json::from_str::<serde_json::Value>(&merged.config_snapshot)
        else {
            return;
        };
        let Some(object) = config.as_object_mut() else {
            return;
        };
        object.insert(
            "envInfo".to_string(),
            serde_json::json!({
                "name": self.env_name,
                "bundleDigest": super::env_bundle::digest_hex(canonical.as_bytes()),
                "varCount": vars.len(),
                "secretCount": secrets.len(),
                "policy": crate::util::normalize_var_policy(
                    self.local_config.var_policy.as_deref()
                ),
            }),
        );
        merged.config_snapshot = config.to_string();
    }

    /// 为指定从节点构建加密环境包；没有公钥或没有环境时返回 None（退回旧流程）
    fn env_bundle_for(&self, node: &NodeState) -> Option<super::protocol::EnvBundle> {
        let peer = node.peer_public_key?;
        let (vars, secrets) = self.master_env()?;
        let mut plain_vars = serde_json::Map::new();
        let mut plain_secrets = serde_json::Map::new();
        for (key, value) in &vars {
            if secrets.contains(key) {
                plain_secrets.insert(key.clone(), serde_json::Value::String(value.clone()));
            } else {
                plain_vars.insert(key.clone(), serde_json::Value::String(value.clone()));
            }
        }
        let plaintext = serde_json::to_vec(&serde_json::json!({
            "vars": plain_vars,
            "secrets": plain_secrets,
        }))
        .ok()?;
        let (nonce, payload, digest) =
            super::env_bundle::encrypt_bundle(&self.x25519_secret, &peer, &plaintext).ok()?;
        Some(super::protocol::EnvBundle {
            version: super::protocol::ENV_BUNDLE_VERSION,
            env_name: self.env_name.clone(),
            nonce,
            payload,
            digest,
            secret_names: secrets.into_iter().collect(),
        })
    }

    /// 为指定从节点切出池数据
    ///
    /// 按池去重：同一池的多个绑定共用一份条目，只生成一个切片；
    /// - `per_vu`：条目数 ≥ 总 VU 时按全局 VU 区间切连续窗口；不足时下发全量并带 `startIndex`，
    ///   由从节点按 `(本地 VU + startIndex) % len` 取值，保证全局同一 VU 对应同一条；
    /// - `per_request` / `random`：按节点序号做分片，避免跨机重复。
    fn pool_slices_for(
        &self,
        node_index: usize,
        node: &NodeState,
        node_count: usize,
        total_vus: u32,
    ) -> Result<(Vec<PoolSlice>, u64), String> {
        use crate::commands::stress::binding_mode;
        use crate::services::pool::{E_POOL_BINDING_INVALID, E_POOL_EMPTY, E_POOL_SHARD_EMPTY};
        let conn = self
            .db
            .get()
            .map_err(|error| format!("{E_POOL_SHARD_EMPTY}: 读取本机池失败：{error}"))?;
        let bindings = &self.local_config.pool_bindings;
        if bindings.is_empty() {
            return Ok((Vec::new(), 0));
        }
        // 按池去重：同一池的多个绑定只能有一份切片
        let mut pools: Vec<(&str, Vec<&crate::models::PoolBinding>)> = Vec::new();
        for binding in bindings {
            let name = binding.pool.trim();
            // target 为空仅在 column == "*" 时允许
            if name.is_empty() || (binding.target.trim().is_empty() && binding.column.trim() != "*")
            {
                return Err(format!(
                    "{E_POOL_BINDING_INVALID}: 池绑定缺少池名或目标变量"
                ));
            }
            if let Some((_, group)) = pools.iter_mut().find(|(pool, _)| *pool == name) {
                group.push(binding);
            } else {
                pools.push((name, vec![binding]));
            }
        }
        let mut slices = Vec::with_capacity(pools.len());
        for (name, group) in pools {
            // 同池多个绑定的轮换模式必须一致
            let mode = binding_mode(group[0]);
            if group.iter().any(|binding| binding_mode(binding) != mode) {
                return Err(format!(
                    "{E_POOL_BINDING_INVALID}: 池「{name}」的多条绑定轮换模式必须一致"
                ));
            }
            let items = crate::services::pool::load_items(&conn, name)?;
            if items.is_empty() {
                return Err(format!("{E_POOL_EMPTY}: 池「{name}」没有可用条目"));
            }
            let kind = crate::services::pool::pool_kind(&conn, name);
            let columns = crate::services::pool::pool_columns(&conn, name)?;
            let refresh = crate::services::pool::pool_refresh_configs(&conn, &[name.to_string()])?
                .remove(name);
            let (items, start_index) = if mode == "per_vu" {
                if items.len() >= total_vus as usize && (node.vu_start as usize) < items.len() {
                    let start = node.vu_start as usize;
                    let end = (start + node.vu_count as usize).min(items.len());
                    (items[start..end].to_vec(), 0)
                } else {
                    (items, node.vu_start as usize)
                }
            } else if node_count > 0 {
                (
                    items
                        .into_iter()
                        .enumerate()
                        .filter(|(index, _)| index % node_count == node_index)
                        .map(|(_, item)| item)
                        .collect(),
                    0,
                )
            } else {
                (items, 0)
            };
            if items.is_empty() {
                return Err(format!(
                    "{E_POOL_SHARD_EMPTY}: 池「{name}」在节点 {node_index} 上的分片为空"
                ));
            }
            slices.push(PoolSlice {
                name: name.to_string(),
                kind,
                items,
                start_index,
                columns,
                refresh,
            });
        }
        ensure_pool_slice_size(&slices, &node.name).map(|bytes| (slices, bytes))
    }

    /// 准备主节点本地引擎：与从节点一样受本机任务注册表约束
    fn prepare_local_engine(&self, concurrent: u32) -> Result<(), String> {
        let app_state = self
            .app
            .try_state()
            .ok_or_else(|| "应用状态不可用".to_string())?;
        if app_state.capacity.is_running() {
            return Err(
                "本机能力检测正在运行，检测期间不能启动压测；请等待检测结束或先取消检测"
                    .to_string(),
            );
        }
        let mut config = self.local_config.clone();
        config.concurrent = concurrent.max(1);
        if config
            .name
            .as_deref()
            .map(str::trim)
            .unwrap_or("")
            .is_empty()
        {
            config.name = Some(self.plan_name.clone());
        }
        // 先用占位取消位准备，拿到注册表取消位后再替换，停止按钮才能停掉引擎
        let placeholder = Arc::new(AtomicBool::new(false));
        // 本地引擎与从节点共用同一个单飞核心：进程内直接调用，不再各刷一次
        let relay: Arc<dyn PoolRefreshRelay> =
            Arc::new(LocalPoolRefreshRelay::new(self.pool_refresh.clone()));
        let mut prepared =
            prepare_stress_with_relay(app_state.inner(), config.clone(), placeholder, Some(relay))?;
        let draft = StressJobDraft {
            name: prepared.name.clone(),
            target: prepared.url_text.clone(),
            target_key: prepared.target_key.clone(),
            plan_id: config.plan_id,
            request_id: config.request_id,
            concurrent: prepared.peak,
        };
        let (max_jobs, max_total) = app_state.stress_limits();
        let (job_id, cancel, _overlaps) =
            app_state
                .stress_jobs
                .try_reserve(draft, max_jobs, max_total, config.ignore_overlap)?;
        prepared.replace_cancel(cancel.clone());
        *self.local_prepared.lock() = Some(prepared);
        *self.local_job.lock() = Some(job_id);
        *self.local_watch.lock() = Some(cancel);
        Ok(())
    }

    /// 释放本地引擎的任务登记（未启动 / 启动失败 / 结束）
    fn release_local_job(&self) {
        let job = self.local_job.lock().take();
        drop(self.local_prepared.lock().take());
        drop(self.local_watch.lock().take());
        if let (Some(job), Some(state)) = (job, self.app.try_state()) {
            state.stress_jobs.finish(job);
        }
    }

    fn on_local_done(&self, outcome: EngineOutcome) {
        let payload = merge::done_from_outcome(&outcome);
        let stopped = payload.stopped;
        let aborted = self.aborting.load(Ordering::SeqCst);
        {
            let mut state = self.state.lock();
            if let Some(node) = state.nodes.iter_mut().find(|node| node.role == "master") {
                let elapsed = (payload.end_ms - payload.start_ms).max(0) as f64 / 1000.0;
                node.qps = if elapsed > 0.0 {
                    payload.total as f64 / elapsed
                } else {
                    0.0
                };
                node.dropped_samples = payload.dropped_samples;
                node.cpu = Some(payload.cpu_peak);
                node.mem_mb = Some(payload.mem_peak_mb);
                // 中止时仍保留 done：合并报告按 stopped 统计到停止时刻
                node.status = if aborted { "left" } else { "done" }.to_string();
                node.done = Some(payload);
            }
        }
        *self.local_details.lock() = Some(LocalDetails {
            requests: outcome.requests,
            errors: outcome.errors,
            slow_requests: outcome.slow_requests,
            config_snapshot: outcome.config_snapshot,
            request_id: outcome.report.request_id,
            run_group_id: outcome.report.run_group_id,
        });
        self.persist_node("master");
        self.release_local_job();
        self.emit_snapshot();
        // 本地被停止或自动停止：同步中止从节点，避免两边结果时长不一致
        if stopped && !aborted {
            let running = self.state.lock().status == "running";
            if running {
                let _ = self.abort(Some("主节点本地压测已停止，集群任务随之结束".to_string()));
            }
        }
        self.maybe_finalize();
    }

    /// 所有节点结算后合并落库：只执行一次
    fn maybe_finalize(&self) {
        {
            let state = self.state.lock();
            if state.report_id.is_some() || !nodes_settled(&state) {
                return;
            }
            if !matches!(state.status.as_str(), "running" | "finished") {
                return;
            }
        }
        if self.finalizing.swap(true, Ordering::SeqCst) {
            return;
        }
        let finished_at = now_ms();
        // 先在锁外取本地明细快照：同一个 let 语句里的临时 MutexGuard 会活到语句末尾，
        // 若在 MergeRequest 字面量里反复 local_details.lock() 会撞上不可重入锁自锁
        let local = {
            let details = self.local_details.lock();
            details.as_ref().map(|details| {
                (
                    details.request_id,
                    details.run_group_id,
                    details.config_snapshot.clone(),
                )
            })
        };
        let request = {
            let mut state = self.state.lock();
            if state.report_id.is_some() || !nodes_settled(&state) {
                return;
            }
            let unavailable: Vec<String> = state
                .nodes
                .iter()
                .filter(|node| matches!(node.status.as_str(), "lost" | "left" | "rejected"))
                .map(|node| node.name.clone())
                .collect();
            let incomplete = state.incomplete || !unavailable.is_empty();
            let mut notes = vec!["逐请求明细仅来自主节点本地压测".to_string()];
            if let Some(message) = state.message.clone() {
                notes.insert(0, message);
            }
            if !unavailable.is_empty() {
                let completed = state
                    .nodes
                    .iter()
                    .filter(|node| node.done.is_some())
                    .count();
                notes.push(format!(
                    "结果不完整（{completed}/{} 节点）：{}",
                    state.nodes.len(),
                    unavailable.join("、")
                ));
            }
            state.status = "finished".to_string();
            state.incomplete = incomplete;
            state.start_at_ms = state.start_at_ms.or(Some(finished_at));
            self.mark_terminal();
            let nodes = state
                .nodes
                .iter()
                .map(|node| MergeNode {
                    name: node.name.clone(),
                    role: node.role.clone(),
                    addr: node.addr.clone(),
                    weight: node.weight,
                    concurrent: node.concurrent,
                    status: node.status.clone(),
                    clock_offset_ms: node.clock_offset_ms.unwrap_or(0.0),
                    clock_uncertainty_ms: node.clock_uncertainty_ms.unwrap_or(0.0),
                    clock_drift_ms: node.clock_drift_ms,
                    samples: node.samples.clone(),
                    done: node.done.clone(),
                })
                .collect();
            MergeRequest {
                plan_id: (self.plan_id > 0).then_some(self.plan_id),
                plan_name: self.plan_name.clone(),
                request_id: local.as_ref().and_then(|(request_id, _, _)| *request_id),
                run_group_id: local
                    .as_ref()
                    .and_then(|(_, run_group_id, _)| *run_group_id),
                total_concurrent: self.total_concurrent,
                started_at: state.start_at_ms.unwrap_or(self.created_at),
                finished_at,
                incomplete,
                note: Some(notes.join("；")),
                config_snapshot: local
                    .as_ref()
                    .map(|(_, _, snapshot)| snapshot.clone())
                    .unwrap_or_else(|| self.config.to_string()),
                nodes,
            }
        };
        let details = self.local_details.lock().take();
        let mut merged = merge::merge(request);
        self.decorate_env_snapshot(&mut merged);
        let empty_errors: Vec<ReportError> = Vec::new();
        let empty_requests: Vec<ReportRequestRecord> = Vec::new();
        let empty_slow: Vec<SlowRequest> = Vec::new();
        let empty_stages: Vec<crate::models::StageSummary> = Vec::new();
        let (errors, requests, slow) = match details {
            Some(details) => (details.errors, details.requests, details.slow_requests),
            None => (empty_errors, empty_requests, empty_slow),
        };
        let saved = save_report(
            &self.db,
            ReportPayload {
                report: &merged.report,
                samples: &merged.samples,
                errors: &errors,
                requests: &requests,
                stages: &empty_stages,
                status_codes: &merged.status_codes,
                histogram: &merged.histogram,
                body_histogram: &merged.body_histogram,
                slow_requests: &slow,
                error_types: &merged.error_types,
                config: &merged.config_snapshot,
            },
        );
        let incomplete = merged.meta.incomplete;
        if let Ok(report_id) = &saved {
            if let Err(error) = save_cluster_samples(&self.db, *report_id, &merged.meta.node_series)
            {
                log::warn!("写入集群节点采样失败：{error}");
            }
        }
        let message = match &saved {
            Ok(report_id) => format!("合并报告 #{report_id} 已生成"),
            Err(error) => format!("报告保存失败：{error}"),
        };
        let report_id = saved.as_ref().ok().copied();
        {
            let mut state = self.state.lock();
            state.report_id = report_id;
            state.incomplete = incomplete;
            state.message = Some(message.clone());
        }
        if let Err(error) =
            update_job_finished(&self.db, self.job_id, &message, incomplete, report_id)
        {
            log::warn!("更新集群任务报告编号失败：{error}");
        }
        self.emit_snapshot();
    }

    fn abort(&self, reason: Option<String>) -> Result<(), String> {
        let detail = reason
            .filter(|text| !text.trim().is_empty())
            .unwrap_or_else(|| "任务已取消".to_string());
        let was_running;
        {
            let mut state = self.state.lock();
            if !matches!(
                state.status.as_str(),
                "pairing" | "preparing" | "ready" | "running"
            ) {
                return Err("任务已结束，无需中止".to_string());
            }
            was_running = state.status == "running";
            state.status = "finished".to_string();
            state.message = Some(detail.clone());
            state.pair_code = None;
            if was_running {
                state.incomplete = true;
            }
            let senders: Vec<mpsc::UnboundedSender<ClusterMessage>> = state
                .nodes
                .iter_mut()
                .filter(|node| node.is_active())
                .filter_map(|node| {
                    if node.role == "slave" {
                        node.status = "left".to_string();
                    }
                    node.tx.take()
                })
                .collect();
            for tx in senders {
                let _ = tx.send(ClusterMessage::Abort(AbortPayload {
                    reason: Some(detail.clone()),
                }));
            }
        }
        if was_running {
            // 本地引擎可能在跑：置取消位，等它产出 stopped 结果后再合并报告
            self.aborting.store(true, Ordering::SeqCst);
            if let Some(flag) = self.local_watch.lock().as_ref() {
                flag.store(true, Ordering::SeqCst);
            }
        } else {
            self.release_local_job();
        }
        self.set_job_status("finished", Some(&detail), was_running, true);
        self.emit_snapshot();
        self.maybe_finalize();
        Ok(())
    }

    fn fail(&self, message: String) {
        let mut already = false;
        {
            let mut state = self.state.lock();
            if matches!(state.status.as_str(), "finished" | "failed") {
                already = true;
            } else {
                state.status = "failed".to_string();
                state.message = Some(message.clone());
                state.pair_code = None;
                let senders: Vec<mpsc::UnboundedSender<ClusterMessage>> = state
                    .nodes
                    .iter_mut()
                    .filter(|node| node.is_active())
                    .filter_map(|node| {
                        if node.role == "slave" && node.status != "rejected" {
                            node.status = "left".to_string();
                        }
                        node.tx.take()
                    })
                    .collect();
                for tx in senders {
                    let _ = tx.send(ClusterMessage::Abort(AbortPayload {
                        reason: Some(message.clone()),
                    }));
                }
            }
        }
        if already {
            return;
        }
        self.release_local_job();
        self.set_job_status("failed", Some(&message), true, true);
        self.emit_snapshot();
    }

    fn check_lost(&self) {
        let now = now_ms();
        let threshold = (HEARTBEAT_SEC * HEARTBEAT_MISSES as u64 * 1000) as i64;
        let mut lost: Vec<String> = Vec::new();
        let mut setup_failure: Option<String> = None;
        {
            let mut state = self.state.lock();
            if matches!(state.status.as_str(), "finished" | "failed") {
                return;
            }
            for node in state.nodes.iter_mut() {
                if node.role != "slave" || !node.is_active() || node.status == "pending" {
                    continue;
                }
                let last_seen = node.last_seen_at.unwrap_or(node.joined_at);
                if now - last_seen <= threshold {
                    continue;
                }
                node.status = "lost".to_string();
                node.tx = None;
                node.error = Some(format!("心跳超时（{}s 未收到消息）", threshold / 1000));
                lost.push(node.name.clone());
            }
            if lost.is_empty() {
                return;
            }
            let running = state.status == "running";
            if running {
                state.incomplete = true;
            } else if matches!(state.status.as_str(), "preparing" | "ready") {
                setup_failure = Some(format!("节点失联，启动已取消：{}", lost.join("、")));
            }
        }
        self.emit_snapshot();
        if let Some(message) = setup_failure {
            self.fail(message);
        } else {
            self.maybe_finalize();
        }
    }
}

/* ---------- 连接处理 ---------- */

async fn accept_loop(session: Arc<MasterSession>, listener: TcpListener) {
    loop {
        if session.shutdown.load(Ordering::SeqCst) {
            break;
        }
        let accepted = listener.accept().await;
        let (stream, peer) = match accepted {
            Ok(item) => item,
            Err(error) => {
                if session.shutdown.load(Ordering::SeqCst) {
                    break;
                }
                log::warn!("集群端口 accept 失败：{error}");
                continue;
            }
        };
        if !super::is_private(peer.ip()) {
            log::warn!("拒绝非内网地址连接集群端口：{peer}");
            drop(stream);
            continue;
        }
        let session = session.clone();
        tauri::async_runtime::spawn(async move {
            handle_connection(session, stream, peer).await;
        });
    }
}

async fn handle_connection(session: Arc<MasterSession>, stream: TcpStream, peer: SocketAddr) {
    let mut config = WebSocketConfig::default();
    config.max_message_size = Some(MAX_MESSAGE_BYTES);
    config.max_frame_size = Some(MAX_MESSAGE_BYTES);
    let ws = match tokio::time::timeout(
        Duration::from_secs(10),
        tokio_tungstenite::accept_async_with_config(stream, Some(config)),
    )
    .await
    {
        Ok(Ok(ws)) => ws,
        Ok(Err(error)) => {
            log::warn!("集群 WebSocket 握手失败（{peer}）：{error}");
            return;
        }
        Err(_) => {
            log::warn!("集群 WebSocket 握手超时（{peer}）");
            return;
        }
    };
    let (mut sink, mut read) = ws.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<ClusterMessage>();
    let mut writer = tauri::async_runtime::spawn(async move {
        while let Some(message) = rx.recv().await {
            let text = match serde_json::to_string(&message) {
                Ok(text) => text,
                Err(error) => {
                    log::warn!("集群消息序列化失败：{error}");
                    continue;
                }
            };
            if sink.send(Message::Text(text.into())).await.is_err() {
                break;
            }
            // 立即刷出：拒绝对端前的最后一条消息不能输给随后的 close 帧
            if sink.flush().await.is_err() {
                break;
            }
        }
        let _ = sink.close().await;
    });

    let mut node_id: Option<String> = None;
    let mut hello_deadline = Box::pin(tokio::time::sleep(Duration::from_secs(10)));
    loop {
        let frame = if node_id.is_none() {
            tokio::select! {
                _ = &mut hello_deadline => {
                    let _ = tx.send(ClusterMessage::Error(ErrorPayload {
                        message: "等待 hello 超时".to_string(),
                    }));
                    break;
                }
                frame = read.next() => frame,
            }
        } else {
            read.next().await
        };
        let Some(frame) = frame else { break };
        let text = match frame {
            Ok(Message::Text(text)) => text,
            Ok(Message::Close(_)) => break,
            Ok(Message::Ping(_)) | Ok(Message::Pong(_)) | Ok(Message::Frame(_)) => continue,
            Ok(Message::Binary(_)) => {
                let _ = tx.send(ClusterMessage::Error(ErrorPayload {
                    message: "集群协议只接受 JSON 文本消息".to_string(),
                }));
                continue;
            }
            Err(error) => {
                log::debug!("集群连接读取结束（{peer}）：{error}");
                break;
            }
        };
        let message: ClusterMessage = match serde_json::from_str(&text) {
            Ok(message) => message,
            Err(error) => {
                let _ = tx.send(ClusterMessage::Error(ErrorPayload {
                    message: format!("消息解析失败：{error}"),
                }));
                continue;
            }
        };
        if node_id.is_none() {
            match message {
                ClusterMessage::Hello(hello) => match session.register(peer, hello, tx.clone()) {
                    Ok(id) => {
                        let heartbeat_session = session.clone();
                        let heartbeat_tx = tx.clone();
                        let heartbeat_id = id.clone();
                        tauri::async_runtime::spawn(async move {
                            heartbeat_loop(heartbeat_session, heartbeat_id, heartbeat_tx).await;
                        });
                        node_id = Some(id);
                    }
                    Err(error) => {
                        session.reject_hello(&tx, error);
                        break;
                    }
                },
                _ => {
                    session.reject_hello(&tx, "第一条消息必须是 hello");
                    break;
                }
            }
            continue;
        }
        session.handle_message(node_id.as_deref().unwrap_or_default(), message);
    }

    if let Some(id) = node_id {
        session.on_disconnect(&id);
    }
    drop(tx);
    if tokio::time::timeout(Duration::from_secs(2), &mut writer)
        .await
        .is_err()
    {
        writer.abort();
    }
}

async fn heartbeat_loop(
    session: Arc<MasterSession>,
    node_id: String,
    tx: mpsc::UnboundedSender<ClusterMessage>,
) {
    let mut ticker = tokio::time::interval(Duration::from_secs(HEARTBEAT_SEC));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    ticker.tick().await;
    // 复测节拍：心跳是 2s 一次，60s 复测等于每 30 拍
    let recheck_every = (CLOCK_RECHECK_SEC / HEARTBEAT_SEC.max(1)).max(1);
    let mut ticks: u64 = 0;
    loop {
        if session.shutdown.load(Ordering::SeqCst) {
            break;
        }
        if tx
            .send(ClusterMessage::Ping(PingPayload {
                at_ms: now_ms(),
                stats: None,
            }))
            .is_err()
        {
            break;
        }
        if session
            .state
            .lock()
            .nodes
            .iter()
            .find(|node| node.id == node_id)
            .map(|node| !node.is_active())
            .unwrap_or(true)
        {
            break;
        }
        ticks += 1;
        if ticks % recheck_every == 0 {
            session.probe_clock_drift(&node_id);
        }
        ticker.tick().await;
    }
}

async fn watchdog_loop(session: Arc<MasterSession>) {
    let mut ticker = tokio::time::interval(Duration::from_secs(1));
    ticker.tick().await;
    loop {
        ticker.tick().await;
        if session.shutdown.load(Ordering::SeqCst) {
            break;
        }
        session.check_lost();
        session.flush_sample_frame();
    }
}

/* ---------- 规则与算法 ---------- */

/// 主节点刷新的渲染变量：环境变量（含 secret）+ 配置里的运行期变量
///
/// 只在建立会话时读取一次；池刷新地址里的 `{{base_url}}` 等占位符依赖它。
fn master_refresh_vars(
    pool: &DbPool,
    config: &StressConfig,
    env_name: Option<&str>,
) -> HashMap<String, String> {
    let mut vars = HashMap::new();
    if let Ok(conn) = pool.get() {
        let env_id = config.env_id.or_else(|| {
            let name = env_name?.trim();
            if name.is_empty() {
                return None;
            }
            conn.query_row(
                "SELECT id FROM environments WHERE name = ?1
                 ORDER BY is_active DESC, id ASC LIMIT 1",
                params![name],
                |row| row.get::<_, i64>(0),
            )
            .optional()
            .ok()
            .flatten()
        });
        if let Some(id) = env_id {
            if let Ok((loaded, _secrets)) = crate::commands::http::load_vars_with_secrets(&conn, id)
            {
                vars = loaded;
            }
        }
    }
    for (key, value) in &config.vars {
        if !key.trim().is_empty() {
            vars.insert(key.clone(), value.clone());
        }
    }
    vars
}

/// 从配置提取变量名并校验一期边界
pub(crate) fn shared_config(
    config: &StressConfig,
) -> Result<(serde_json::Value, Vec<String>), String> {
    if config.concurrent == 0 {
        return Err("并发必须大于 0".to_string());
    }
    if !config.stages.is_empty() {
        return Err("分布式压测一期仅支持单场景，请先取消阶梯加压".to_string());
    }
    if !config.vars.is_empty() {
        return Err("分布式压测不携带运行期变量，请把变量放进环境后重试".to_string());
    }
    if let Some(auth) = &config.auth {
        for (label, value) in [
            ("token", auth.token.as_str()),
            ("password", auth.password.as_str()),
            ("clientSecret", auth.client_secret.as_str()),
            ("value", auth.value.as_str()),
        ] {
            let trimmed = value.trim();
            if !trimmed.is_empty() && !trimmed.contains("{{") {
                return Err(format!(
                    "分布式压测不发送明文认证信息（{label}），请改用环境变量引用后重试"
                ));
            }
        }
    }
    let required_vars = collect_required_vars(config);
    let mut clean = config.clone();
    clean.env_id = None;
    clean.vars.clear();
    let value = serde_json::to_value(&clean).map_err(err)?;
    Ok((value, required_vars))
}

/// 变量体检中的单节点状态：优先看 PrepareAck 的缺失清单，其次看准备结果
fn var_node_state(node: &NodeState, names: &[String]) -> super::protocol::VarAuditNodeState {
    use super::protocol::VarAuditNodeState;
    if node
        .missing_vars
        .iter()
        .any(|missing| names.iter().any(|name| name == missing))
    {
        return VarAuditNodeState {
            state: "missing".to_string(),
            detail: Some("从节点回执缺少该变量".to_string()),
        };
    }
    match node.prepare_ok {
        Some(true) => VarAuditNodeState {
            state: "ok".to_string(),
            detail: None,
        },
        Some(false) => VarAuditNodeState {
            state: "missing".to_string(),
            detail: node.error.clone(),
        },
        None if node.status == "rejected" => VarAuditNodeState {
            state: "unknown".to_string(),
            detail: node
                .error
                .clone()
                .or_else(|| Some("节点准备失败，原因未注明".to_string())),
        },
        None => VarAuditNodeState {
            state: "pending".to_string(),
            detail: Some("尚未下发配置".to_string()),
        },
    }
}

/// 收集需要从节点实际提供的变量名
///
/// v1.1.2 起必须走统一分类：`{{$timestamp}}`、`{{uuid}}`、`{{randomInt(...)}}`、
/// `{{faker:...}}` 由各节点本地生成，`{{os:NAME}}` 由各节点读本机环境，
/// `{{pool:x}}` 由池注入——这些都不再误判为「用户变量缺失」。
/// `{{secret:TOKEN}}` 归一化为 `TOKEN`，仍需主节点提供（值通过 EnvBundle 下发）。
fn collect_required_vars(config: &StressConfig) -> Vec<String> {
    let texts = config_texts(config);
    let classified = crate::util::required_vars_in_texts(texts.iter().map(String::as_str));
    let mut names = classified.user;
    for name in classified.secret {
        if !names.contains(&name) {
            names.push(name);
        }
    }
    names
}

/// 配置中所有可能出现占位符的文本字段
fn config_texts(config: &StressConfig) -> Vec<String> {
    let mut texts = vec![config.url.trim().to_string()];
    for item in config.params.iter().chain(config.headers.iter()) {
        if !item.enabled {
            continue;
        }
        texts.push(item.key.trim().to_string());
        texts.push(item.value.clone());
    }
    if let Some(body) = &config.body {
        texts.push(body.clone());
    }
    if let Some(auth) = &config.auth {
        for value in [
            &auth.token,
            &auth.username,
            &auth.password,
            &auth.key,
            &auth.value,
            &auth.token_url,
            &auth.client_id,
            &auth.client_secret,
            &auth.scope,
        ] {
            texts.push(value.clone());
        }
    }
    texts
}

/// 归一化分配模式：auto / even / custom；未传时按是否携带权重推断
pub(crate) fn normalize_allocation_mode(
    value: Option<&str>,
    has_weights: bool,
) -> Result<&'static str, String> {
    let normalized = value
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty());
    match normalized.as_deref() {
        None => Ok(if has_weights { "custom" } else { "auto" }),
        Some("auto") => Ok("auto"),
        Some("even") => Ok("even"),
        Some("custom") => Ok("custom"),
        Some(other) => Err(format!(
            "分配模式「{other}」不支持，可选 auto（能力分配）/ even（平均分配）/ custom（自定义权重）"
        )),
    }
}

/// 把前端可选权重落到所有参与节点（主节点恒参与）
fn resolve_weights(
    participants: &[(String, f64)],
    requested: &[ClusterNodeWeight],
    mode: &str,
) -> Result<Vec<(String, f64)>, String> {
    let unknown: Vec<&str> = requested
        .iter()
        .map(|item| item.node_id.as_str())
        .filter(|id| !participants.iter().any(|(node, _)| node == id))
        .collect();
    if !unknown.is_empty() {
        return Err(format!("权重包含未知节点：{}", unknown.join("、")));
    }
    // even：忽略自定义权重，按节点平均；auto：按能力预算作为权重
    if mode == "even" {
        return Ok(participants
            .iter()
            .map(|(id, _)| (id.clone(), 1.0))
            .collect());
    }
    if mode == "auto" {
        return Ok(participants.to_vec());
    }
    let mut weights = Vec::with_capacity(participants.len());
    for (id, default) in participants {
        let weight = requested
            .iter()
            .find(|item| &item.node_id == id)
            .map(|item| item.weight)
            .unwrap_or(*default);
        if !weight.is_finite() || weight <= 0.0 {
            return Err(format!("节点「{id}」的权重必须大于 0"));
        }
        weights.push((id.clone(), weight));
    }
    Ok(weights)
}

/// 绑定集群端口：指定端口优先；未指定时 17801 → 17811..17860 → 临时端口
async fn bind_cluster_listener(requested: Option<u16>) -> Result<TcpListener, String> {
    if let Some(port) = requested.filter(|port| *port != 0) {
        return TcpListener::bind(("0.0.0.0", port))
            .await
            .map_err(|error| format!("集群端口 {port} 监听失败：{error}"));
    }
    let mut candidates = vec![DEFAULT_PORT];
    candidates.extend((DEFAULT_PORT + 10)..=super::protocol::AUTO_PORT_END);
    for port in candidates {
        if let Ok(listener) = TcpListener::bind(("0.0.0.0", port)).await {
            return Ok(listener);
        }
    }
    TcpListener::bind(("0.0.0.0", 0))
        .await
        .map_err(|error| format!("无法分配集群端口：{error}"))
}

/// 带每节点 min/max 的分配：
/// 先为每个节点保留最小并发，再按权重分配剩余额度；超出上限时按权重回补给未满节点。
/// 如果所有节点都达到上限仍有剩余，直接报错，避免静默少分配。
fn allocate_concurrency_capped(
    total: u32,
    participants: &[(String, f64)],
    weights: &[(String, f64)],
    caps: &[(String, u32)],
    limits: &[(String, u32, u32)],
) -> Result<Vec<(String, u32)>, String> {
    if participants.is_empty() {
        return Err("没有可分配的节点".to_string());
    }
    let cap_of = |id: &str| {
        caps.iter()
            .find(|(node, _)| node == id)
            .map(|(_, cap)| *cap)
            .unwrap_or(u32::MAX)
    };
    let weight_of = |id: &str| {
        weights
            .iter()
            .find(|(node, _)| node == id)
            .map(|(_, weight)| *weight)
            .unwrap_or(1.0)
    };
    let limit_of = |id: &str| {
        limits
            .iter()
            .find(|(node, _, _)| node == id)
            .map(|(_, min, max)| (*min, *max))
            .unwrap_or((1, cap_of(id)))
    };

    let mut min_sum = 0u32;
    for (id, _) in participants {
        let (min, max) = limit_of(id);
        if min < 1 {
            return Err(format!("节点「{id}」的最小并发必须大于等于 1"));
        }
        if max < min {
            return Err(format!(
                "节点「{id}」的并发上限 {max} 小于下限 {min}，请调整范围"
            ));
        }
        if min > cap_of(id) {
            return Err(format!(
                "节点「{id}」的并发下限 {min} 超过本机预算 {}，请下调下限",
                cap_of(id)
            ));
        }
        min_sum = min_sum.saturating_add(min);
    }
    if min_sum > total {
        return Err(format!(
            "各节点最小并发之和 {min_sum} 超过总并发 {total}，请下调下限或提高总并发"
        ));
    }

    let mut result: Vec<(String, u32)> = participants
        .iter()
        .map(|(id, _)| (id.clone(), limit_of(id).0))
        .collect();
    let mut remaining = total - min_sum;
    if remaining > 0 {
        let sum: f64 = participants.iter().map(|(id, _)| weight_of(id)).sum();
        if sum <= 0.0 {
            return Err("权重之和必须大于 0".to_string());
        }
        // 先按权重向下取整分配，并限制在 max 以内
        let pool = remaining;
        let mut fractions: Vec<(usize, f64, u32)> = Vec::with_capacity(participants.len());
        for (index, (id, _)) in participants.iter().enumerate() {
            let (min, max) = limit_of(id);
            let room = max.saturating_sub(min);
            let share = pool as f64 * weight_of(id) / sum;
            let add = (share.floor() as u32).min(room);
            result[index].1 += add;
            remaining -= add;
            fractions.push((index, share.fract(), room - add));
        }
        // 余数从大到小逐轮补 1，仍然受 max 约束
        fractions.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        while remaining > 0 {
            let mut progressed = false;
            for (index, _, room) in fractions.iter_mut() {
                if remaining == 0 {
                    break;
                }
                if *room > 0 {
                    result[*index].1 += 1;
                    *room -= 1;
                    remaining -= 1;
                    progressed = true;
                }
            }
            if !progressed {
                break;
            }
        }
        if remaining > 0 {
            let cap_sum: u32 = participants
                .iter()
                .map(|(id, _)| limit_of(id).1)
                .fold(0u32, |acc, value| acc.saturating_add(value));
            return Err(format!(
                "各节点并发上限之和 {cap_sum} 小于总并发 {total}，请提高节点上限或减少总并发"
            ));
        }
    }
    Ok(result)
}

/// NTP 单样本：偏移 = t1 - (t0 + t2) / 2，误差取单程 RTT / 2
fn setup_ready(state: &SessionState) -> bool {
    let slaves: Vec<&NodeState> = state
        .nodes
        .iter()
        .filter(|node| node.role == "slave")
        .collect();
    !slaves.is_empty() && slaves.iter().all(|node| node.status == "ready")
}

/// 所有节点都已结算：done / lost / left / rejected；主节点本地引擎也在内
fn nodes_settled(state: &SessionState) -> bool {
    !state.nodes.is_empty()
        && state
            .nodes
            .iter()
            .all(|node| matches!(node.status.as_str(), "done" | "lost" | "left" | "rejected"))
}

/// 本机计算机名；取不到时用调用方给的兜底名（主节点 / 从节点展示不同）
pub(crate) fn local_node_name(fallback: &str) -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .ok()
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| fallback.to_string())
}

fn validate_vu_ranges(nodes: &[NodeState], total_vus: u32) -> Result<(), String> {
    use crate::services::pool::E_POOL_SLICE_GAP;
    let mut ranges: Vec<(u32, u32)> = nodes
        .iter()
        .filter(|node| node.concurrent > 0)
        .map(|node| (node.vu_start, node.vu_start.saturating_add(node.vu_count)))
        .collect();
    ranges.sort_unstable();
    let mut cursor = 0u32;
    for (start, end) in ranges {
        if start != cursor {
            return Err(format!(
                "{E_POOL_SLICE_GAP}: 节点 VU 区间在 {cursor} 处不连续（下一段从 {start} 开始）"
            ));
        }
        if end < start {
            return Err(format!("{E_POOL_SLICE_GAP}: 节点 VU 区间非法"));
        }
        cursor = end;
    }
    if cursor != total_vus {
        return Err(format!(
            "{E_POOL_SLICE_GAP}: 节点 VU 区间只覆盖 {cursor}/{total_vus}"
        ));
    }
    Ok(())
}

/// 按池条目估算下发体量：创建任务时算一次，供前端在下发前提示
///
/// 只读 `COUNT` 与 `LENGTH`，不加载条目；行开销按分片字段与 JSON 键名粗估。
fn estimate_pool_bytes(
    db: &DbPool,
    bindings: &[crate::models::PoolBinding],
) -> Option<PoolSizeEstimate> {
    let mut names: Vec<String> = Vec::new();
    for binding in bindings {
        let name = binding.pool.trim();
        if name.is_empty() || names.iter().any(|item| item == name) {
            continue;
        }
        names.push(name.to_string());
    }
    if names.is_empty() {
        return None;
    }
    let conn = db.get().ok()?;
    let mut estimate = PoolSizeEstimate::default();
    for name in names {
        let Ok(Some(pool_id)) = crate::services::pool::pool_id_by_name(&conn, &name) else {
            continue;
        };
        let columns = crate::services::pool::pool_columns(&conn, &name)
            .map(|items| items.len() as u64)
            .unwrap_or(0);
        let Ok((count, raw_bytes)) = conn.query_row(
            "SELECT COUNT(*), COALESCE(SUM(LENGTH(value) + LENGTH(COALESCE(account, '')) + LENGTH(data_json)), 0)
             FROM data_pool_items WHERE pool_id = ?1 AND enabled = 1",
            params![pool_id],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
        ) else {
            continue;
        };
        if count <= 0 {
            continue;
        }
        let count = count as u64;
        estimate.pools += 1;
        estimate.items += count;
        // 60 字节覆盖 id/权重/过期时间与 JSON 字段名，列名再按每个 16 字节估算
        estimate.bytes += raw_bytes.max(0) as u64 + count * (60 + columns * 16);
    }
    (estimate.pools > 0).then_some(estimate)
}

/// 单节点池切片体积校验（12.5）：序列化后超过 64 MB 直接拒绝
///
/// 返回该节点的实际字节数，供前端展示占用；超过 80% 时记告警日志。
fn ensure_pool_slice_size(slices: &[PoolSlice], node_name: &str) -> Result<u64, String> {
    let bytes = serde_json::to_vec(slices)
        .map_err(|error| format!("池切片序列化失败：{error}"))?
        .len();
    if bytes > MAX_POOL_SLICE_BYTES {
        // 找出占比最大的池，让用户知道该缩减哪个池
        let largest = slices
            .iter()
            .filter_map(|slice| serde_json::to_vec(slice).ok().map(|raw| (slice, raw.len())))
            .max_by_key(|(_, size)| *size);
        let detail = match largest {
            Some((slice, size)) => format!(
                "；最大来源是池「{}」（{} 条 × {} 列，约 {:.1} MB）",
                slice.name,
                slice.items.len(),
                slice.columns.len().max(1),
                size as f64 / 1024.0 / 1024.0
            ),
            None => String::new(),
        };
        let limit_mb = MAX_POOL_SLICE_BYTES / (1024 * 1024);
        return Err(format!(
            "{}: 节点「{}」的池切片 {:.1} MB 超过 {} MB 上限{}。请减少池条目、精简列，或增加从节点分摊后重试",
            crate::services::pool::E_POOL_SLICE_TOO_LARGE,
            node_name,
            bytes as f64 / 1024.0 / 1024.0,
            limit_mb,
            detail
        ));
    }
    if bytes * 5 > MAX_POOL_SLICE_BYTES * 4 {
        log::warn!(
            "节点「{}」的池切片已用 {:.1}/{:.0} MB，接近上限",
            node_name,
            bytes as f64 / 1024.0 / 1024.0,
            MAX_POOL_SLICE_BYTES as f64 / 1024.0 / 1024.0
        );
    }
    Ok(bytes as u64)
}

#[cfg(test)]
fn ensure_pool_slice_bytes(bytes: usize, node_name: &str) -> Result<(), String> {
    if bytes > MAX_POOL_SLICE_BYTES {
        let limit_mb = MAX_POOL_SLICE_BYTES / (1024 * 1024);
        return Err(format!(
            "{}: 节点「{}」的池切片 {:.1} MB 超过 {} MB 上限，请缩小池条目或增加节点",
            crate::services::pool::E_POOL_SLICE_TOO_LARGE,
            node_name,
            bytes as f64 / 1024.0 / 1024.0,
            limit_mb
        ));
    }
    Ok(())
}
