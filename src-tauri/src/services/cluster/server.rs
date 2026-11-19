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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use parking_lot::Mutex;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::tungstenite::Message;
use uuid::Uuid;

use super::devices;
use super::discovery;
use super::host::ClusterApp;
use super::merge::{self, MergeNode, MergeRequest};
use super::protocol::{
    AbortPayload, ClusterDevice, ClusterMasterSnapshot, ClusterMessage, ClusterNodeInfo,
    DeviceCapability, DeviceListPayload, DonePayload, ErrorPayload, HelloAckPayload, HelloPayload,
    Orchestration, PhaseStartPayload, PingPayload, PongPayload, PreflightPayload,
    PrepareAckPayload, PreparePayload, SamplePayload, StartPayload, TimeAckPayload,
    TimeProbePayload, CLOCK_SAMPLE_ROUNDS, DEFAULT_PORT, HEARTBEAT_MISSES, HEARTBEAT_SEC,
    MAX_MESSAGE_BYTES, MAX_NODES, MAX_PAIR_CODE_TTL_SEC, MIN_PAIR_CODE_TTL_SEC,
    MIN_PROTOCOL_VERSION, PAIR_CODE_DIGITS, PAIR_CODE_TTL_SEC, PROTOCOL_VERSION,
    RECONNECT_GRACE_SEC,
};
use crate::commands::stress::{
    prepare_stress, run_engine, save_report, EngineOptions, EngineOutcome, EngineSampleSink,
    PreparedStress, ReportPayload,
};
use crate::models::{
    CapacityResult, ReportError, ReportRequestRecord, SlowRequest, StressConfig, StressStage,
};
use crate::state::{DbPool, StressJobDraft};
use crate::util::{err, now_ms};

/// 任务开跑前的统一提前量：留出网络单程与从节点建引擎的时间
pub const START_LEAD_MS: i64 = 2_000;
/// 准备阶段等待从节点回执的上限
pub const SETUP_TIMEOUT: Duration = Duration::from_secs(15);
/// 同时存在的集群任务上限（主窗口列表用）
pub const MAX_MASTER_SESSIONS: usize = 3;

pub const EVENT_MASTER: &str = "cluster:master";
pub const EVENT_SAMPLE: &str = "cluster:sample";

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
    /// 监听网卡：缺省 0.0.0.0；跨网段时可绑定指定内网地址
    #[serde(default)]
    pub listen_addr: Option<String>,
    /// 计划级编排：ramp / hold / step，按权重拆分每阶段并发
    #[serde(default)]
    pub orchestration: Option<Orchestration>,
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

/// 单节点权重：缺省为 1，主节点与从节点一视同仁
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClusterNodeWeight {
    pub node_id: String,
    pub weight: f64,
}

/// 每秒采样事件：前端画各节点实时曲线
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClusterSampleEvent {
    pub job_id: i64,
    pub node_id: String,
    pub sample: SamplePayload,
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
    clock_offset_ms: Option<f64>,
    clock_uncertainty_ms: Option<f64>,
    /// 时钟对齐时 RTT 最小的那一轮的误差（毫秒）
    clock_best_error_ms: f64,
    clock_probes: HashMap<u32, i64>,
    clock_samples: u32,
    dropped_samples: u64,
    joined_at: i64,
    last_seen_at: Option<i64>,
    error: Option<String>,
    tx: Option<mpsc::UnboundedSender<ClusterMessage>>,
    prepare_ok: Option<bool>,
    preflight_ok: Option<bool>,
    samples: Vec<SamplePayload>,
    done: Option<DonePayload>,
    protocol_version: u32,
    device_id: Option<String>,
    token: Option<String>,
    capacity_concurrent: Option<u32>,
    capacity_qps: Option<f64>,
    cores: Option<u32>,
    os: Option<String>,
    suggested_weight: Option<f64>,
    disconnected_at: Option<i64>,
    gap_seconds: u64,
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
            clock_offset_ms: None,
            clock_uncertainty_ms: None,
            clock_best_error_ms: f64::INFINITY,
            clock_probes: HashMap::new(),
            clock_samples: 0,
            dropped_samples: 0,
            joined_at,
            last_seen_at: Some(joined_at),
            error: None,
            tx: None,
            prepare_ok: None,
            preflight_ok: None,
            samples: Vec::new(),
            done: None,
            protocol_version: PROTOCOL_VERSION,
            device_id: None,
            token: None,
            capacity_concurrent: None,
            capacity_qps: None,
            cores: None,
            os: None,
            suggested_weight: None,
            disconnected_at: None,
            gap_seconds: 0,
        }
    }

    fn apply_capability(&mut self, capability: &DeviceCapability) {
        if capability.capacity_concurrent > 0 {
            self.capacity_concurrent = Some(capability.capacity_concurrent);
        }
        if capability.capacity_qps > 0.0 {
            self.capacity_qps = Some(capability.capacity_qps);
        }
        if capability.cores > 0 {
            self.cores = Some(capability.cores);
        }
        if capability
            .os
            .as_deref()
            .is_some_and(|os| !os.trim().is_empty())
        {
            self.os = capability.os.clone();
        }
    }

    fn info(&self) -> ClusterNodeInfo {
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
            clock_offset_ms: self.clock_offset_ms,
            clock_uncertainty_ms: self.clock_uncertainty_ms,
            dropped_samples: self.dropped_samples,
            joined_at: self.joined_at,
            last_seen_at: self.last_seen_at,
            error: self.error.clone(),
            protocol_version: self.protocol_version,
            device_id: self.device_id.clone(),
            capacity_concurrent: self.capacity_concurrent,
            capacity_qps: self.capacity_qps,
            cores: self.cores,
            os: self.os.clone(),
            suggested_weight: self.suggested_weight,
            disconnected_at: self.disconnected_at,
            gap_seconds: self.gap_seconds,
        }
    }

    fn is_active(&self) -> bool {
        !matches!(
            self.status.as_str(),
            "lost" | "left" | "rejected" | "done" | "disconnected"
        )
    }
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
    current_phase: Option<u32>,
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
    /// 手动中止：引擎仍会产出 stopped 结果，报告要标注不完整
    aborting: AtomicBool,
    /// 合并落库只允许执行一次
    finalizing: AtomicBool,
    orchestration: Option<Orchestration>,
}

struct RunningMaster {
    session: Arc<MasterSession>,
    accept: tauri::async_runtime::JoinHandle<()>,
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
        if self.sessions.lock().len() >= MAX_MASTER_SESSIONS {
            return Err(format!(
                "最多同时保留 {MAX_MASTER_SESSIONS} 个分布式压测任务，请先结束已有任务"
            ));
        }
        let (config, required_vars) = shared_config(&payload.config)?;
        if let Some(orchestration) = &payload.orchestration {
            orchestration.expand_totals()?;
        }
        let total_concurrent = match &payload.orchestration {
            Some(orchestration) => orchestration.peak()?,
            None => payload.config.concurrent,
        };
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
        let port = payload.port.unwrap_or(DEFAULT_PORT);
        let listen_ip: std::net::IpAddr = payload
            .listen_addr
            .as_deref()
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .unwrap_or("0.0.0.0")
            .parse()
            .map_err(|_| "监听地址需填写本机内网 IP，例如 192.168.1.10".to_string())?;
        if !listen_ip.is_unspecified() && !super::is_private(listen_ip) {
            return Err("集群端口只允许绑定内网 / 回环地址".to_string());
        }
        let pair_code_ttl = resolve_pair_code_ttl(payload.pair_code_ttl_sec)?;
        let listener = TcpListener::bind((listen_ip, port))
            .await
            .map_err(|error| format!("集群端口 {port} 监听失败：{error}"))?;
        let port = listener.local_addr().map_err(err)?.port();
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
        let orchestration_json = payload
            .orchestration
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(err)?;
        let job_config = serde_json::json!({
            "config": config.clone(),
            "envName": payload.env_name.clone(),
            "requiredVars": required_vars.clone(),
            "weights": {},
            "orchestration": payload.orchestration,
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
            orchestration_json.as_deref(),
        )?;
        let mut master_node = NodeState::master(master_name, master_addr, created_at);
        if let Some(capacity) = local_capacity(&pool) {
            master_node.apply_capability(&DeviceCapability {
                device_id: None,
                node_name: master_node.name.clone(),
                os: Some(capacity.os.clone()),
                cores: capacity.cores,
                capacity_concurrent: capacity.saturation_concurrent,
                capacity_qps: capacity.max_qps,
                probed_at: Some(capacity.probed_at),
            });
            master_node.suggested_weight = Some(1.0);
        }
        upsert_worker(&pool, job_id, &master_node)?;
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
                current_phase: None,
            }),
            local_prepared: Mutex::new(None),
            local_job: Mutex::new(None),
            local_watch: Mutex::new(None),
            local_details: Mutex::new(None),
            aborting: AtomicBool::new(false),
            finalizing: AtomicBool::new(false),
            orchestration: payload.orchestration,
        });
        let mdns_name = session
            .state
            .lock()
            .nodes
            .first()
            .map(|node| node.name.clone())
            .unwrap_or_else(|| "master".to_string());
        if let Err(error) = discovery::advertise_master(job_id, &mdns_name, port) {
            log::warn!("mDNS 注册跳过：{error}");
        }
        let accept = tauri::async_runtime::spawn(accept_loop(session.clone(), listener));
        tauri::async_runtime::spawn(watchdog_loop(session.clone()));
        let snapshot = session.snapshot();
        self.sessions.lock().insert(
            job_id,
            RunningMaster {
                session: session.clone(),
                accept,
            },
        );
        session.emit_snapshot();
        Ok(snapshot)
    }

    pub fn snapshot(&self, job_id: i64) -> Result<ClusterMasterSnapshot, String> {
        Ok(self.session(job_id)?.snapshot())
    }

    pub fn snapshots(&self) -> Vec<ClusterMasterSnapshot> {
        let sessions: Vec<Arc<MasterSession>> = self
            .sessions
            .lock()
            .values()
            .map(|running| running.session.clone())
            .collect();
        sessions.iter().map(|session| session.snapshot()).collect()
    }

    fn session(&self, job_id: i64) -> Result<Arc<MasterSession>, String> {
        self.sessions
            .lock()
            .get(&job_id)
            .map(|running| running.session.clone())
            .ok_or_else(|| format!("分布式压测任务 {job_id} 不存在或已关闭"))
    }

    /// 按权重下发配置并等待从节点回执
    pub async fn prepare(
        &self,
        job_id: i64,
        weights: Vec<ClusterNodeWeight>,
    ) -> Result<ClusterMasterSnapshot, String> {
        let session = self.session(job_id)?;
        session.prepare(weights).await?;
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

    /// 关闭并移除会话（运行中会先中止）
    pub fn close(&self, job_id: i64) -> Result<(), String> {
        let running = self
            .sessions
            .lock()
            .remove(&job_id)
            .ok_or_else(|| format!("分布式压测任务 {job_id} 不存在或已关闭"))?;
        running.session.shutdown.store(true, Ordering::SeqCst);
        running.accept.abort();
        discovery::unadvertise_master(job_id);
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
            start_at_ms: state.start_at_ms,
            elapsed_sec,
            report_id: state.report_id,
            incomplete: state.incomplete,
            message: state.message.clone(),
            nodes: state.nodes.iter().map(NodeState::info).collect(),
            created_at: self.created_at,
            orchestration: self.orchestration.clone(),
            current_phase: state.current_phase,
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
        let now = now_ms();
        if !(MIN_PROTOCOL_VERSION..=PROTOCOL_VERSION).contains(&hello.version) {
            return Err(format!(
                "协议版本不兼容：主节点支持 v{}–v{}，从节点 v{}，请升级后重试",
                MIN_PROTOCOL_VERSION, PROTOCOL_VERSION, hello.version
            ));
        }
        let negotiated = hello.version.min(PROTOCOL_VERSION);
        if let Some(token) = hello
            .session_token
            .as_deref()
            .map(str::trim)
            .filter(|token| !token.is_empty())
        {
            return self.resume_node(peer, &hello, token, negotiated, tx);
        }
        let ttl_sec = self.pair_code_ttl_sec;
        let (node_id, token, probes, pair_expires, device_id) = {
            let mut state = self.state.lock();
            if state.status != "pairing" {
                return Err("任务已开始或已结束，不能再加入节点".to_string());
            }
            let code = state
                .pair_code
                .as_ref()
                .filter(|code| code.expires_at > now)
                .ok_or_else(|| "配对码已过期，请在主节点刷新配对码".to_string())?
                .code
                .clone();
            if hello.pair_code.trim() != code {
                return Err("配对码不正确".to_string());
            }
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
                return Err(format!("单任务最多 {MAX_NODES} 个从节点"));
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
            let device_id = hello
                .device_id
                .as_deref()
                .map(str::trim)
                .filter(|id| !id.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| format!("device-{}", Uuid::new_v4().simple()));
            let mut node = NodeState {
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
                done: None,
                protocol_version: negotiated,
                device_id: Some(device_id.clone()),
                token: Some(Uuid::new_v4().simple().to_string()),
                capacity_concurrent: hello.capacity_concurrent.filter(|value| *value > 0),
                capacity_qps: None,
                cores: None,
                os: None,
                suggested_weight: None,
                disconnected_at: None,
                gap_seconds: 0,
            };
            node.apply_capability(&DeviceCapability {
                device_id: Some(device_id.clone()),
                node_name: node.name.clone(),
                os: None,
                cores: 0,
                capacity_concurrent: hello.capacity_concurrent.unwrap_or(0),
                capacity_qps: 0.0,
                probed_at: None,
            });
            state.nodes.push(node);
            refresh_suggestions(&mut state.nodes);
            (
                id,
                state
                    .nodes
                    .last()
                    .and_then(|node| node.token.clone())
                    .unwrap_or_default(),
                probes,
                pair_expires,
                device_id,
            )
        };

        if let Err(error) = devices::touch_device(
            &self.db,
            &device_id,
            hello.node_name.trim(),
            &peer.to_string(),
        ) {
            log::warn!("缓存集群设备失败：{error}");
        }
        let ack = ClusterMessage::HelloAck(HelloAckPayload {
            version: negotiated,
            token,
            node_id: node_id.clone(),
            job_id: self.job_id,
            resumed: false,
        });
        if tx.send(ack).is_err() {
            return Err("连接已断开".to_string());
        }
        for probe in probes {
            if tx.send(ClusterMessage::TimeProbe(probe)).is_err() {
                return Err("连接已断开".to_string());
            }
        }
        if negotiated >= 2 {
            let _ = tx.send(ClusterMessage::DeviceList(DeviceListPayload {
                devices: self.device_list(),
            }));
        }
        if let Err(error) = update_pair_expiry(&self.db, self.job_id, pair_expires) {
            log::warn!("更新配对码有效期失败：{error}");
        }
        self.persist_node(&node_id);
        self.emit_snapshot();
        Ok(node_id)
    }

    /// 断线重连：令牌恢复原节点，不重新走配对码与时钟探测
    fn resume_node(
        &self,
        peer: SocketAddr,
        hello: &HelloPayload,
        token: &str,
        negotiated: u32,
        tx: mpsc::UnboundedSender<ClusterMessage>,
    ) -> Result<String, String> {
        let now = now_ms();
        let (node_id, ack_token, node_version, resumed_status, device_id) = {
            let mut state = self.state.lock();
            if matches!(state.status.as_str(), "finished" | "failed") {
                return Err("任务已结束，无法恢复会话；请重新创建任务并配对".to_string());
            }
            let session_status = match state.status.as_str() {
                "running" => "running",
                "ready" => "ready",
                "preparing" => "preparing",
                _ => "pairReady",
            };
            let Some(node) = state
                .nodes
                .iter_mut()
                .find(|node| node.role == "slave" && node.token.as_deref() == Some(token))
            else {
                return Err("会话令牌已失效，请用新配对码重新加入".to_string());
            };
            if matches!(node.status.as_str(), "left" | "rejected" | "done" | "lost") {
                return Err(format!("节点「{}」已结束会话，不能恢复", node.name));
            }
            if let Some(existing) = node.device_id.as_deref() {
                if let Some(incoming) = hello.device_id.as_deref().map(str::trim) {
                    if !incoming.is_empty() && incoming != existing {
                        return Err("重连令牌与设备标识不匹配".to_string());
                    }
                }
            }
            if node.disconnected_at.is_some() {
                let gap = (now - node.disconnected_at.unwrap_or(now)).max(0) as u64 / 1000;
                node.gap_seconds = node.gap_seconds.saturating_add(gap);
            }
            node.tx = Some(tx.clone());
            node.last_seen_at = Some(now);
            node.disconnected_at = None;
            node.error = None;
            node.protocol_version = node.protocol_version.min(negotiated);
            node.status = session_status.to_string();
            (
                node.id.clone(),
                node.token.clone().unwrap_or_default(),
                node.protocol_version,
                true,
                node.device_id.clone(),
            )
        };
        if let Err(error) = devices::touch_device(
            &self.db,
            device_id.as_deref().unwrap_or(""),
            hello.node_name.trim(),
            &peer.to_string(),
        ) {
            log::warn!("刷新集群设备失败：{error}");
        }
        let ack = ClusterMessage::HelloAck(HelloAckPayload {
            version: node_version,
            token: ack_token,
            node_id: node_id.clone(),
            job_id: self.job_id,
            resumed: resumed_status,
        });
        if tx.send(ack).is_err() {
            return Err("连接已断开".to_string());
        }
        if node_version >= 2 {
            let _ = tx.send(ClusterMessage::DeviceList(DeviceListPayload {
                devices: self.device_list(),
            }));
        }
        self.persist_node(&node_id);
        self.emit_snapshot();
        Ok(node_id)
    }

    fn device_list(&self) -> Vec<ClusterDevice> {
        let mut items = devices::list_devices(&self.db, 100).unwrap_or_default();
        let online: HashMap<String, bool> = {
            let state = self.state.lock();
            state
                .nodes
                .iter()
                .filter_map(|node| {
                    node.device_id
                        .clone()
                        .map(|id| (id, node.is_active() && node.status != "disconnected"))
                })
                .collect()
        };
        for item in &mut items {
            item.online = online.get(&item.node_id).copied().unwrap_or(false);
        }
        items
    }

    fn touch(&self, node_id: &str) {
        let mut state = self.state.lock();
        if let Some(node) = state.nodes.iter_mut().find(|node| node.id == node_id) {
            node.last_seen_at = Some(now_ms());
        }
    }

    fn on_capability(&self, node_id: &str, payload: DeviceCapability) {
        let device_id = {
            let mut state = self.state.lock();
            let device_id = {
                let Some(node) = state.nodes.iter_mut().find(|node| node.id == node_id) else {
                    return;
                };
                if node.protocol_version < 2 {
                    return;
                }
                node.apply_capability(&payload);
                if let Some(id) = payload
                    .device_id
                    .as_deref()
                    .map(str::trim)
                    .filter(|id| !id.is_empty())
                {
                    node.device_id = Some(id.to_string());
                }
                node.device_id.clone()
            };
            // 配对阶段才用能力建议覆盖默认权重，准备后尊重用户显式设置
            if state.status == "pairing" {
                refresh_suggestions(&mut state.nodes);
            }
            device_id
        };
        if let Some(device_id) = device_id {
            let addr = {
                let state = self.state.lock();
                state
                    .nodes
                    .iter()
                    .find(|node| node.id == node_id)
                    .map(|node| node.addr.clone())
                    .unwrap_or_default()
            };
            if let Err(error) = devices::upsert_device(&self.db, &device_id, &addr, &payload) {
                log::warn!("缓存节点能力失败：{error}");
            }
        }
        self.persist_node(node_id);
        self.emit_snapshot();
    }

    fn handle_message(&self, node_id: &str, message: ClusterMessage) {
        self.touch(node_id);
        match message {
            ClusterMessage::CapabilityReport(payload) => self.on_capability(node_id, payload),
            ClusterMessage::DeviceList(_) => {}
            ClusterMessage::PhaseStart(_) => {}
            ClusterMessage::TimeAck(payload) => self.on_time_ack(node_id, payload),
            ClusterMessage::PrepareAck(payload) => self.on_prepare_ack(node_id, payload),
            ClusterMessage::Preflight(payload) => self.on_preflight(node_id, payload),
            ClusterMessage::Sample(payload) => self.on_sample(node_id, payload),
            ClusterMessage::Done(payload) => self.on_done(node_id, payload),
            ClusterMessage::Ping(payload) => {
                let _ = self.send(
                    node_id,
                    ClusterMessage::Pong(PongPayload {
                        at_ms: payload.at_ms,
                    }),
                );
            }
            ClusterMessage::Pong(_) => {}
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

    fn on_time_ack(&self, node_id: &str, payload: TimeAckPayload) {
        let mut paired = false;
        {
            let mut state = self.state.lock();
            if state.status != "pairing" {
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
        if paired {
            self.persist_node(node_id);
            self.emit_snapshot();
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
                failure = Some(format!("节点「{node_name}」准备失败：{message}"));
            } else {
                node.prepare_ok = Some(true);
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
            node.samples.push(payload.clone());
        }
        self.persist_node(node_id);
        self.app.emit_sample(ClusterSampleEvent {
            job_id: self.job_id,
            node_id: node_id.to_string(),
            sample: payload,
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
            let running = state.status == "running";
            let Some(node) = state.nodes.iter_mut().find(|node| node.id == node_id) else {
                return;
            };
            if !node.is_active() && node.status != "disconnected" {
                return;
            }
            node.tx = None;
            if running {
                // 运行期断线保留 5 分钟重连窗口；期间引擎继续跑，采样形成缺口
                if node.status != "disconnected" {
                    node.disconnected_at = Some(now_ms());
                }
                node.status = "disconnected".to_string();
                node.error = Some("连接断开，等待从节点重连".to_string());
                state.incomplete = true;
            } else {
                node.status = "lost".to_string();
                node.error = Some("连接断开".to_string());
            }
        }
        self.persist_node(node_id);
        self.emit_snapshot();
        self.maybe_finalize();
    }

    async fn prepare(&self, weights: Vec<ClusterNodeWeight>) -> Result<(), String> {
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
            let participants: Vec<(String, f64)> = state
                .nodes
                .iter()
                .map(|node| (node.id.clone(), node.weight))
                .collect();
            let weights = resolve_weights(&participants, &weights)?;
            let allocations = allocate_concurrency(self.total_concurrent, &participants, &weights)?;
            for node in &mut state.nodes {
                if let Some((_, weight)) = weights.iter().find(|(id, _)| id == &node.id) {
                    node.weight = *weight;
                }
                if let Some((_, concurrent)) = allocations.iter().find(|(id, _)| id == &node.id) {
                    node.concurrent = *concurrent;
                }
                node.prepare_ok = None;
                node.preflight_ok = None;
                node.error = None;
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
        // 编排任务把每秒集群总并发按权重拆到各节点，再合并成阶梯阶段交给引擎
        let stage_map = match &self.orchestration {
            Some(orchestration) => {
                let state = self.state.lock();
                let participants: Vec<(String, f64)> = state
                    .nodes
                    .iter()
                    .map(|node| (node.id.clone(), node.weight))
                    .collect();
                let weights = participants.clone();
                Some(orchestration_stages(
                    orchestration,
                    &participants,
                    &weights,
                )?)
            }
            None => None,
        };
        let master_stages = stage_map
            .as_ref()
            .and_then(|map| map.get("master").cloned())
            .unwrap_or_default();
        let master_concurrent = master_stages
            .iter()
            .map(|stage| stage.concurrent)
            .max()
            .unwrap_or(master_concurrent);
        if let Err(message) = self.prepare_local_engine(master_concurrent, master_stages) {
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

        for (node_id, concurrent) in &allocations {
            let payload = {
                let state = self.state.lock();
                let Some(node) = state.nodes.iter().find(|node| node.id == *node_id) else {
                    continue;
                };
                if node.role == "master" {
                    continue;
                }
                let stages = stage_map
                    .as_ref()
                    .and_then(|map| map.get(node_id).cloned())
                    .unwrap_or_default();
                let mut config = self.config.clone();
                if !stages.is_empty() {
                    if let Some(object) = config.as_object_mut() {
                        object.insert(
                            "stages".to_string(),
                            serde_json::to_value(&stages).unwrap_or_default(),
                        );
                    }
                }
                PreparePayload {
                    job_id: self.job_id,
                    plan_name: self.plan_name.clone(),
                    config,
                    env_name: self.env_name.clone(),
                    required_vars: self.required_vars.clone(),
                    concurrent: stages
                        .iter()
                        .map(|stage| stage.concurrent)
                        .max()
                        .unwrap_or(*concurrent),
                    total_concurrent: self.total_concurrent,
                    orchestration: self.orchestration.clone(),
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
        self.spawn_phase_scheduler(at_ms);
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

    /// 编排阶段切换：按原始阶段边界通知各节点并刷新当前阶段
    ///
    /// 引擎已按每秒拆分后的阶梯表执行，这份通知只用于协议与界面同步。
    fn spawn_phase_scheduler(self: &Arc<Self>, at_ms: i64) {
        let Some(orchestration) = self.orchestration.clone() else {
            return;
        };
        let session = Arc::clone(self);
        tauri::async_runtime::spawn(async move {
            let mut offset_ms: i64 = 0;
            for (index, phase) in orchestration.phases.iter().enumerate() {
                let due = at_ms + offset_ms;
                offset_ms += phase.duration_sec as i64 * 1000;
                let wait = due - now_ms();
                if wait > 0 {
                    tokio::time::sleep(Duration::from_millis(wait as u64)).await;
                }
                if session.shutdown.load(Ordering::SeqCst) {
                    break;
                }
                if session.state.lock().status != "running" {
                    break;
                }
                let target = match phase.kind.as_str() {
                    "ramp" => phase.to_concurrent.unwrap_or(0),
                    _ => phase.concurrent.unwrap_or(0),
                };
                let (participants, node_ids) = {
                    let state = session.state.lock();
                    (
                        state
                            .nodes
                            .iter()
                            .map(|node| (node.id.clone(), node.weight))
                            .collect::<Vec<(String, f64)>>(),
                        state
                            .nodes
                            .iter()
                            .filter(|node| node.status == "running")
                            .map(|node| node.id.clone())
                            .collect::<Vec<String>>(),
                    )
                };
                if participants.is_empty() {
                    break;
                }
                let total = target.max(participants.len() as u32);
                let allocation = allocate_concurrency(total, &participants, &participants)
                    .unwrap_or_else(|_| {
                        participants.iter().map(|(id, _)| (id.clone(), 1)).collect()
                    });
                {
                    let mut state = session.state.lock();
                    state.current_phase = Some(index as u32);
                    for node in &mut state.nodes {
                        if let Some((_, concurrent)) =
                            allocation.iter().find(|(id, _)| id == &node.id)
                        {
                            node.concurrent = *concurrent;
                        }
                    }
                }
                for node_id in &node_ids {
                    let weight = participants
                        .iter()
                        .find(|(id, _)| id == node_id)
                        .map(|(_, weight)| *weight)
                        .unwrap_or(1.0);
                    let node_concurrent = allocation
                        .iter()
                        .find(|(id, _)| id == node_id)
                        .map(|(_, concurrent)| *concurrent)
                        .unwrap_or(0);
                    let _ = session.send(
                        node_id,
                        ClusterMessage::PhaseStart(PhaseStartPayload {
                            index: index as u32,
                            at_ms: now_ms(),
                            concurrent: total,
                            weight,
                            node_concurrent,
                            kind: phase.kind.clone(),
                            duration_sec: phase.duration_sec,
                        }),
                    );
                }
                session.persist_node("master");
                session.emit_snapshot();
            }
        });
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
                persist: None,
            },
        )
        .await;
        self.on_local_done(outcome);
    }

    /// 准备主节点本地引擎：与从节点一样受本机任务注册表约束
    fn prepare_local_engine(
        &self,
        concurrent: u32,
        stages: Vec<StressStage>,
    ) -> Result<(), String> {
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
        if !stages.is_empty() {
            config.stages = stages;
        }
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
        let mut prepared = prepare_stress(app_state.inner(), config.clone(), placeholder)?;
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
                .filter(|node| {
                    matches!(
                        node.status.as_str(),
                        "lost" | "left" | "rejected" | "disconnected"
                    )
                })
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
                orchestration: self.orchestration.clone(),
                config_snapshot: local
                    .as_ref()
                    .map(|(_, _, snapshot)| snapshot.clone())
                    .unwrap_or_else(|| self.config.to_string()),
                nodes,
            }
        };
        let details = self.local_details.lock().take();
        let merged = merge::merge(request);
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
                job_id: None,
                samples: &merged.samples,
                worker_states: &[],
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
        let mut changed_ids: Vec<String> = Vec::new();
        let mut setup_failure: Option<String> = None;
        {
            let mut state = self.state.lock();
            if matches!(state.status.as_str(), "finished" | "failed") {
                return;
            }
            let running = state.status == "running";
            for node in state.nodes.iter_mut() {
                if node.role != "slave" {
                    continue;
                }
                if node.status == "disconnected" {
                    let since = node.disconnected_at.unwrap_or(now);
                    if now - since >= RECONNECT_GRACE_SEC as i64 * 1000 {
                        node.gap_seconds = node
                            .gap_seconds
                            .saturating_add(((now - since).max(0) as u64) / 1000);
                        node.status = "lost".to_string();
                        node.error = Some(format!(
                            "重连超过 {} 分钟，节点已定格",
                            RECONNECT_GRACE_SEC / 60
                        ));
                        changed_ids.push(node.id.clone());
                        lost.push(node.name.clone());
                    }
                    continue;
                }
                if !node.is_active() {
                    continue;
                }
                let last_seen = node.last_seen_at.unwrap_or(node.joined_at);
                if now - last_seen <= threshold {
                    continue;
                }
                node.tx = None;
                if running {
                    node.status = "disconnected".to_string();
                    node.disconnected_at = Some(now);
                    node.error = Some(format!(
                        "心跳超时（{}s 未收到消息），等待重连",
                        threshold / 1000
                    ));
                } else {
                    node.status = "lost".to_string();
                    node.error = Some(format!("心跳超时（{}s 未收到消息）", threshold / 1000));
                    lost.push(node.name.clone());
                }
                changed_ids.push(node.id.clone());
            }
            if changed_ids.is_empty() {
                return;
            }
            let running = state.status == "running";
            if running {
                state.incomplete = true;
            } else if matches!(state.status.as_str(), "preparing" | "ready") {
                setup_failure = Some(format!("节点失联，启动已取消：{}", lost.join("、")));
            }
        }
        for node_id in &changed_ids {
            self.persist_node(node_id);
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
    loop {
        if session.shutdown.load(Ordering::SeqCst) {
            break;
        }
        if tx
            .send(ClusterMessage::Ping(PingPayload { at_ms: now_ms() }))
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
    }
}

/* ---------- 规则与算法 ---------- */

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

fn collect_required_vars(config: &StressConfig) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    let mut push = |text: &str| {
        for name in find_placeholders(text) {
            if !names.iter().any(|item| item == &name) {
                names.push(name);
            }
        }
    };
    push(config.url.trim());
    for item in config.params.iter().chain(config.headers.iter()) {
        if !item.enabled {
            continue;
        }
        push(item.key.trim());
        push(&item.value);
    }
    if let Some(body) = &config.body {
        push(body);
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
            push(value);
        }
    }
    names
}

fn find_placeholders(text: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("{{") {
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else {
            break;
        };
        let name = after[..end].trim();
        if !name.is_empty() && !names.iter().any(|item| item == name) {
            names.push(name.to_string());
        }
        rest = &after[end + 2..];
    }
    names
}

/// 把前端可选权重落到所有参与节点（主节点恒参与）
fn resolve_weights(
    participants: &[(String, f64)],
    requested: &[ClusterNodeWeight],
) -> Result<Vec<(String, f64)>, String> {
    let unknown: Vec<&str> = requested
        .iter()
        .map(|item| item.node_id.as_str())
        .filter(|id| !participants.iter().any(|(node, _)| node == id))
        .collect();
    if !unknown.is_empty() {
        return Err(format!("权重包含未知节点：{}", unknown.join("、")));
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

/// 先每个节点分 1，再按权重分配剩余并发；用最大余数法保证总和精确
pub(crate) fn allocate_concurrency(
    total: u32,
    participants: &[(String, f64)],
    weights: &[(String, f64)],
) -> Result<Vec<(String, u32)>, String> {
    if participants.is_empty() {
        return Err("没有可分配的节点".to_string());
    }
    if (total as usize) < participants.len() {
        return Err(format!(
            "总并发 {total} 少于节点数 {}，无法保证每个节点至少有 1 并发",
            participants.len()
        ));
    }
    let remaining = total - participants.len() as u32;
    let sum: f64 = weights.iter().map(|(_, weight)| weight).sum();
    if sum <= 0.0 {
        return Err("权重之和必须大于 0".to_string());
    }
    let mut exact: Vec<(String, u32, f64)> = participants
        .iter()
        .map(|(id, _)| {
            let weight = weights
                .iter()
                .find(|(node, _)| node == id)
                .map(|(_, weight)| *weight)
                .unwrap_or(1.0);
            let share = remaining as f64 * weight / sum;
            let base = share.floor() as u32;
            (id.clone(), 1 + base, share - base as f64)
        })
        .collect();
    let assigned: u32 = exact.iter().map(|(_, count, _)| *count).sum();
    let mut leftover = total.saturating_sub(assigned);
    exact.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
    let count = exact.len();
    let mut index = 0;
    while leftover > 0 && count > 0 {
        exact[index % count].1 += 1;
        leftover -= 1;
        index += 1;
    }
    exact.sort_by_key(|(id, _, _)| {
        participants
            .iter()
            .position(|(node, _)| node == id)
            .unwrap_or(usize::MAX)
    });
    Ok(exact
        .into_iter()
        .map(|(id, count, _)| (id, count))
        .collect())
}

/// 把编排展开成逐节点阶梯表：每秒集群总并发按权重拆分，连续相同并发合并
///
/// 集群总并发低于节点数时抬升到节点数，保证每节点至少有 1 并发。
pub(crate) fn orchestration_stages(
    orchestration: &Orchestration,
    participants: &[(String, f64)],
    weights: &[(String, f64)],
) -> Result<HashMap<String, Vec<StressStage>>, String> {
    let totals = orchestration.expand_totals()?;
    let minimum = participants.len() as u32;
    let mut stages: HashMap<String, Vec<StressStage>> = participants
        .iter()
        .map(|(id, _)| (id.clone(), Vec::new()))
        .collect();
    for total in totals {
        let allocation = allocate_concurrency(total.max(minimum), participants, weights)?;
        for (id, concurrent) in allocation {
            let Some(list) = stages.get_mut(&id) else {
                continue;
            };
            match list.last_mut() {
                Some(last) if last.concurrent == concurrent => {
                    last.duration_sec = last.duration_sec.saturating_add(1);
                }
                _ => list.push(StressStage {
                    concurrent,
                    duration_sec: 1,
                }),
            }
        }
    }
    Ok(stages)
}

/// 按能力归一化刷新建议权重；没有能力数据的节点保持 None
fn refresh_suggestions(nodes: &mut [NodeState]) {
    let inputs: Vec<(String, Option<u32>)> = nodes
        .iter()
        .map(|node| (node.id.clone(), node.capacity_concurrent))
        .collect();
    let weights = devices::suggest_weights(&inputs);
    for node in nodes {
        if let Some((_, weight)) = weights.iter().find(|(id, _)| id == &node.id) {
            node.suggested_weight = *weight;
            // 配对阶段默认权重跟随能力；用户显式改动会在 prepare 时覆盖
            if let Some(weight) = weight.filter(|value| *value > 0.0) {
                node.weight = weight;
            }
        }
    }
}

/// 读取最近一次本机能力检测：主节点自身也参与权重建议
fn local_capacity(pool: &DbPool) -> Option<CapacityResult> {
    let conn = pool.get().ok()?;
    crate::commands::capacity::latest_result(&conn)
        .ok()
        .flatten()
}

/// NTP 单样本：偏移 = t1 - (t0 + t2) / 2，误差取单程 RTT / 2
pub(crate) fn clock_metrics(t0: i64, t1: i64, t2: i64) -> (f64, f64) {
    let rtt = (t2 - t0).max(0) as f64;
    let offset = t1 as f64 - (t0 as f64 + t2 as f64) / 2.0;
    (offset, rtt / 2.0)
}

fn setup_ready(state: &SessionState) -> bool {
    let slaves: Vec<&NodeState> = state
        .nodes
        .iter()
        .filter(|node| node.role == "slave")
        .collect();
    !slaves.is_empty() && slaves.iter().all(|node| node.status == "ready")
}

/// 所有节点都已结算：done / lost / left / rejected / disconnected
///
/// 主节点本地引擎结束时不再等 5 分钟宽限：断线节点按缺口定格，
/// 报告里保留缺口与不完整标记；若主节点仍在跑，重连窗口内仍可恢复。
fn nodes_settled(state: &SessionState) -> bool {
    !state.nodes.is_empty()
        && state.nodes.iter().all(|node| {
            matches!(
                node.status.as_str(),
                "done" | "lost" | "left" | "rejected" | "disconnected"
            )
        })
}

fn new_pair_code() -> String {
    let value = Uuid::new_v4().as_u128() % 10u128.pow(PAIR_CODE_DIGITS as u32);
    format!("{value:0width$}", width = PAIR_CODE_DIGITS)
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

/* ---------- 持久化 ---------- */

struct WorkerRow {
    name: String,
    addr: String,
    role: String,
    weight: f64,
    concurrent: u32,
    status: String,
    qps: f64,
    cpu: Option<f64>,
    mem_mb: Option<u64>,
    clock_offset_ms: Option<f64>,
    clock_uncertainty_ms: Option<f64>,
    dropped_samples: u64,
    joined_at: i64,
    last_seen_at: Option<i64>,
    error: Option<String>,
}

#[allow(clippy::too_many_arguments)]
fn insert_job(
    pool: &DbPool,
    created_at: i64,
    master_node: &str,
    plan_id: Option<i64>,
    plan_name: &str,
    port: u16,
    config_json: &str,
    pair_expires_at: i64,
    total_concurrent: u32,
    orchestration_json: Option<&str>,
) -> Result<i64, String> {
    let conn = pool.get().map_err(err)?;
    conn.execute(
        "INSERT INTO cluster_jobs (created_at, status, master_node, plan_id, plan_name, port,
            config_json, pair_code_expires_at, total_concurrent, orchestration_json)
         VALUES (?1, 'pairing', ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            created_at,
            master_node,
            plan_id,
            plan_name,
            port,
            config_json,
            pair_expires_at,
            total_concurrent,
            orchestration_json
        ],
    )
    .map_err(err)?;
    Ok(conn.last_insert_rowid())
}

fn upsert_worker(pool: &DbPool, job_id: i64, node: &NodeState) -> Result<(), String> {
    update_worker(
        pool,
        job_id,
        &WorkerRow {
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
        },
    )
}

fn update_worker(pool: &DbPool, job_id: i64, node: &WorkerRow) -> Result<(), String> {
    let conn = pool.get().map_err(err)?;
    conn.execute(
        "INSERT INTO cluster_workers (job_id, node_name, addr, role, weight, concurrent, status,
            qps, cpu, mem, clock_offset_ms, clock_uncertainty_ms, dropped_samples, joined_at,
            last_seen_at, error)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
         ON CONFLICT(job_id, node_name) DO UPDATE SET
            addr = excluded.addr,
            role = excluded.role,
            weight = excluded.weight,
            concurrent = excluded.concurrent,
            status = excluded.status,
            qps = excluded.qps,
            cpu = excluded.cpu,
            mem = excluded.mem,
            clock_offset_ms = excluded.clock_offset_ms,
            clock_uncertainty_ms = excluded.clock_uncertainty_ms,
            dropped_samples = excluded.dropped_samples,
            last_seen_at = excluded.last_seen_at,
            error = excluded.error",
        params![
            job_id,
            node.name,
            node.addr,
            node.role,
            node.weight,
            node.concurrent,
            node.status,
            node.qps,
            // 表里这些列是 NOT NULL DEFAULT 0：未采集到就直接写 0，不写 NULL
            node.cpu.unwrap_or(0.0),
            node.mem_mb.map(|value| value as i64).unwrap_or(0),
            node.clock_offset_ms.unwrap_or(0.0),
            node.clock_uncertainty_ms.unwrap_or(0.0),
            node.dropped_samples as i64,
            node.joined_at,
            node.last_seen_at,
            node.error
        ],
    )
    .map_err(err)?;
    Ok(())
}

fn update_job(
    pool: &DbPool,
    job_id: i64,
    status: &str,
    message: Option<&str>,
    incomplete: bool,
    finished_at: Option<i64>,
) -> Result<(), String> {
    let conn = pool.get().map_err(err)?;
    conn.execute(
        "UPDATE cluster_jobs SET status = ?2, message = ?3, incomplete = ?4,
            finished_at = COALESCE(?5, finished_at) WHERE id = ?1",
        params![job_id, status, message, incomplete, finished_at],
    )
    .map_err(err)?;
    Ok(())
}

fn update_job_finished(
    pool: &DbPool,
    job_id: i64,
    message: &str,
    incomplete: bool,
    report_id: Option<i64>,
) -> Result<(), String> {
    let conn = pool.get().map_err(err)?;
    conn.execute(
        "UPDATE cluster_jobs SET status = 'finished', message = ?2, incomplete = ?3,
            report_id = ?4, finished_at = ?5 WHERE id = ?1",
        params![job_id, message, incomplete, report_id, now_ms()],
    )
    .map_err(err)?;
    Ok(())
}

fn update_job_start(pool: &DbPool, job_id: i64, at_ms: i64) -> Result<(), String> {
    let conn = pool.get().map_err(err)?;
    conn.execute(
        "UPDATE cluster_jobs SET status = 'running', start_at_ms = ?2 WHERE id = ?1",
        params![job_id, at_ms],
    )
    .map_err(err)?;
    Ok(())
}

fn update_pair_expiry(pool: &DbPool, job_id: i64, expires_at: i64) -> Result<(), String> {
    let conn = pool.get().map_err(err)?;
    conn.execute(
        "UPDATE cluster_jobs SET pair_code_expires_at = ?2 WHERE id = ?1",
        params![job_id, expires_at],
    )
    .map_err(err)?;
    Ok(())
}

fn update_weights(
    pool: &DbPool,
    job_id: i64,
    weights: &HashMap<String, f64>,
) -> Result<(), String> {
    let conn = pool.get().map_err(err)?;
    let current: Option<String> = conn
        .query_row(
            "SELECT config_json FROM cluster_jobs WHERE id = ?1",
            params![job_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(err)?;
    let text = match current {
        Some(text) => text,
        None => return Ok(()),
    };
    let mut value: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
    if !value.is_object() {
        value = serde_json::json!({});
    }
    value["weights"] = serde_json::to_value(weights).map_err(err)?;
    conn.execute(
        "UPDATE cluster_jobs SET config_json = ?2 WHERE id = ?1",
        params![job_id, value.to_string()],
    )
    .map_err(err)?;
    Ok(())
}

/// 启动时把上次异常退出遗留的任务标记为失败，避免列表长期显示运行中
pub fn recover_stale_jobs(pool: &DbPool) -> Result<usize, String> {
    let conn = pool.get().map_err(err)?;
    let now = now_ms();
    let jobs = conn
        .execute(
            "UPDATE cluster_jobs SET status = 'failed', message = '应用退出导致任务中断',
                finished_at = ?1, incomplete = 1
             WHERE status IN ('pairing', 'preparing', 'ready', 'running')",
            params![now],
        )
        .map_err(err)?;
    conn.execute(
        "UPDATE cluster_workers SET status = 'lost', error = '应用退出导致连接中断'
         WHERE status NOT IN ('done', 'lost', 'left', 'rejected')",
        [],
    )
    .map_err(err)?;
    Ok(jobs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::cluster::protocol::OrchestrationPhase;

    #[test]
    fn pair_code_is_fixed_width_digits() {
        for _ in 0..20 {
            let code = new_pair_code();
            assert_eq!(code.len(), PAIR_CODE_DIGITS);
            assert!(code.chars().all(|ch| ch.is_ascii_digit()));
        }
    }

    #[test]
    fn pair_code_ttl_defaults_and_bounds() {
        assert_eq!(resolve_pair_code_ttl(None).unwrap(), PAIR_CODE_TTL_SEC);
        assert_eq!(resolve_pair_code_ttl(Some(900)).unwrap(), 900);
        assert!(resolve_pair_code_ttl(Some(30)).is_err());
        assert!(resolve_pair_code_ttl(Some(7200)).is_err());
        assert_eq!(resolve_pair_code_ttl(Some(60)).unwrap(), 60);
        assert_eq!(resolve_pair_code_ttl(Some(3600)).unwrap(), 3600);
    }

    #[test]
    fn clock_metrics_follow_ntp_single_sample() {
        // 从节点比主节点快 5 分钟，RTT 40ms
        let (offset, uncertainty) = clock_metrics(1_000, 301_020, 1_040);
        assert!((offset - 300_000.0).abs() < 0.01);
        assert!((uncertainty - 20.0).abs() < 0.01);
    }

    #[test]
    fn allocation_sums_to_total_and_keeps_minimum_one() {
        let participants = vec![
            ("master".to_string(), 1.0),
            ("a".to_string(), 1.0),
            ("b".to_string(), 1.0),
        ];
        let weights = participants.clone();
        let allocation = allocate_concurrency(16, &participants, &weights).unwrap();
        assert_eq!(allocation.iter().map(|(_, count)| count).sum::<u32>(), 16);
        assert!(allocation.iter().all(|(_, count)| *count >= 1));
        assert_eq!(allocation.len(), 3);

        let two = vec![("master".to_string(), 1.0), ("a".to_string(), 1.0)];
        let allocation = allocate_concurrency(16, &two, &two).unwrap();
        assert_eq!(
            allocation,
            vec![("master".to_string(), 8), ("a".to_string(), 8)]
        );

        let skewed = vec![
            ("master".to_string(), 2.0),
            ("a".to_string(), 1.0),
            ("b".to_string(), 1.0),
        ];
        let allocation = allocate_concurrency(10, &skewed, &skewed).unwrap();
        assert_eq!(allocation.iter().map(|(_, count)| count).sum::<u32>(), 10);
        assert_eq!(allocation[0].1, 4);
    }

    #[test]
    fn allocation_rejects_more_nodes_than_concurrency() {
        let participants = vec![("a".to_string(), 1.0), ("b".to_string(), 1.0)];
        assert!(allocate_concurrency(1, &participants, &participants).is_err());
    }

    #[test]
    fn required_vars_cover_headers_body_auth_and_skip_disabled() {
        let config: StressConfig = serde_json::from_str(
            r#"{
                "url":"{{base_url}}/api/{{id}}",
                "headers":[
                    {"key":"X-Token","value":"{{token}}","enabled":true},
                    {"key":"X-Off","value":"{{hidden}}","enabled":false}
                ],
                "body":"{\"user\":\"{{user}}\"}",
                "auth":{"kind":"bearer","token":"{{token}}"}
            }"#,
        )
        .unwrap();
        let names = collect_required_vars(&config);
        assert!(names.contains(&"base_url".to_string()));
        assert!(names.contains(&"id".to_string()));
        assert!(names.contains(&"token".to_string()));
        assert!(names.contains(&"user".to_string()));
        assert!(!names.contains(&"hidden".to_string()));
        assert_eq!(names.iter().filter(|name| *name == "token").count(), 1);
    }

    #[test]
    fn shared_config_rejects_phase_one_gaps() {
        let mut config: StressConfig =
            serde_json::from_str(r#"{"url":"http://127.0.0.1/x"}"#).unwrap();
        config.vars.insert("token".to_string(), "abc".to_string());
        assert!(shared_config(&config).unwrap_err().contains("运行期变量"));

        let mut config: StressConfig =
            serde_json::from_str(r#"{"url":"http://127.0.0.1/x"}"#).unwrap();
        config.stages = vec![serde_json::from_str(r#"{"concurrent":1,"durationSec":5}"#).unwrap()];
        assert!(shared_config(&config).unwrap_err().contains("阶梯"));

        let mut config: StressConfig = serde_json::from_str(
            r#"{"url":"http://127.0.0.1/x","auth":{"kind":"bearer","token":"plain-secret"}}"#,
        )
        .unwrap();
        assert!(shared_config(&config).unwrap_err().contains("明文"));

        config.auth.as_mut().unwrap().token = "{{token}}".to_string();
        let (value, _) = shared_config(&config).unwrap();
        assert!(value.get("auth").is_some());
    }

    #[test]
    fn shared_config_clears_env_and_keeps_template() {
        let config: StressConfig = serde_json::from_str(
            r#"{"url":"{{base_url}}/x","envId":7,"headers":[{"key":"A","value":"{{token}}","enabled":true}]}"#,
        )
        .unwrap();
        let (value, names) = shared_config(&config).unwrap();
        assert!(value.get("envId").is_none() || value["envId"].is_null());
        assert_eq!(value["url"], "{{base_url}}/x");
        assert!(names.contains(&"base_url".to_string()));
        assert!(names.contains(&"token".to_string()));
    }

    #[test]
    fn finished_job_records_report_and_incomplete_flag() {
        let (dir, pool) = crate::notes::test_pool("cluster-job-finish");
        let job_id = insert_job(
            &pool,
            1_000,
            "master",
            Some(7),
            "计划",
            17801,
            "{}",
            2_000,
            16,
            None,
        )
        .unwrap();
        update_job_finished(&pool, job_id, "合并报告 #9 已生成", true, Some(9)).unwrap();
        let conn = pool.get().unwrap();
        let (status, report_id, incomplete, message, finished): (
            String,
            Option<i64>,
            i64,
            String,
            Option<i64>,
        ) = conn
            .query_row(
                "SELECT status, report_id, incomplete, message, finished_at
                 FROM cluster_jobs WHERE id = ?1",
                params![job_id],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(status, "finished");
        assert_eq!(report_id, Some(9));
        assert_eq!(incomplete, 1);
        assert_eq!(message, "合并报告 #9 已生成");
        assert!(finished.is_some());
        drop(conn);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn nodes_settled_requires_master_done() {
        let mut state = SessionState {
            status: "running".to_string(),
            start_at_ms: None,
            report_id: None,
            incomplete: false,
            message: None,
            pair_code: None,
            nodes: vec![NodeState::master("m".to_string(), String::new(), 1)],
            next_node: 1,
            current_phase: None,
        };
        assert!(!nodes_settled(&state));
        state.nodes[0].status = "done".to_string();
        assert!(nodes_settled(&state));
        state
            .nodes
            .push(NodeState::master("s".to_string(), String::new(), 1));
        state.nodes[1].status = "lost".to_string();
        assert!(nodes_settled(&state));
    }

    #[test]
    fn orchestration_expands_to_weighted_per_node_stages() {
        let orchestration = Orchestration {
            version: 1,
            assignment: "weighted".to_string(),
            phases: vec![
                OrchestrationPhase {
                    kind: "hold".to_string(),
                    duration_sec: 2,
                    from_concurrent: None,
                    to_concurrent: None,
                    concurrent: Some(10),
                },
                OrchestrationPhase {
                    kind: "ramp".to_string(),
                    duration_sec: 3,
                    from_concurrent: Some(10),
                    to_concurrent: Some(30),
                    concurrent: None,
                },
            ],
        };
        let participants = vec![("master".to_string(), 1.0), ("a".to_string(), 1.0)];
        let stages = orchestration_stages(&orchestration, &participants, &participants).unwrap();
        let master = &stages["master"];
        assert_eq!(
            master.iter().map(|stage| stage.duration_sec).sum::<u32>(),
            5
        );
        assert_eq!(master.iter().map(|stage| stage.concurrent).max(), Some(15));
        // 10 并发持续 2 秒 + ramp 首秒仍为 10，前三秒合并成一个 3 秒阶段
        assert_eq!(master.first().map(|stage| stage.duration_sec), Some(3));
    }

    #[test]
    fn resolve_weights_uses_default_and_rejects_unknown() {
        let participants = vec![("master".to_string(), 1.0), ("a".to_string(), 1.0)];
        let resolved = resolve_weights(
            &participants,
            &[ClusterNodeWeight {
                node_id: "a".to_string(),
                weight: 2.0,
            }],
        )
        .unwrap();
        assert_eq!(resolved[0].1, 1.0);
        assert_eq!(resolved[1].1, 2.0);

        assert!(resolve_weights(
            &participants,
            &[ClusterNodeWeight {
                node_id: "ghost".to_string(),
                weight: 1.0,
            }],
        )
        .is_err());
    }
}
