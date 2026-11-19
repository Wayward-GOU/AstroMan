//! 从节点客户端（一期）
//!
//! 反向连接主节点：配对码换节点身份 → 5 轮时钟探测 → 按名字在本地解析环境变量
//! → 用本机引擎预检目标并等待统一起跑时刻 → 每秒外送采样、结束时回传 `done`。
//!
//! 与主节点一致，传输是内网明文 WebSocket + JSON；变量值只在本机解析，
//! 主节点下发的配置里只有变量名与引用。断线重连、跨网段不在本期。

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use parking_lot::Mutex;
use rusqlite::{params, OptionalExtension};
use tauri::AppHandle;
use tokio::net::TcpStream;
use tokio::sync::{mpsc, Notify};
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::tungstenite::Message;
use uuid::Uuid;

use super::host::ClusterApp;
use super::protocol::{
    AbortPayload, ClusterMessage, ClusterSlaveSnapshot, DeviceCapability, DonePayload,
    ErrorPayload, HelloAckPayload, HelloPayload, PhaseStartPayload, PingPayload, PongPayload,
    PreflightPayload, PrepareAckPayload, PreparePayload, StartPayload, TimeAckPayload,
    TimeProbePayload, DEFAULT_PORT, HEARTBEAT_MISSES, HEARTBEAT_SEC, MAX_MESSAGE_BYTES,
    MIN_PROTOCOL_VERSION, PROTOCOL_VERSION, RECONNECT_BACKOFF_MAX_SEC, RECONNECT_GRACE_SEC,
};
use super::server::local_node_name;
use crate::commands::stress::{
    prepare_stress, run_engine, EngineOptions, EngineOutcome, EngineSampleSink, PreparedStress,
};
use crate::models::StressConfig;
use crate::state::{AppState, DbPool, StressJobDraft};
use crate::util::now_ms;

/// 从节点会话快照事件：前端订阅后不用轮询
pub const EVENT_SLAVE: &str = "cluster:slave";
/// 主节点连续无消息的判定阈值：与主节点看从节点一致（2s × 3）
const MASTER_SILENCE: Duration = Duration::from_secs(HEARTBEAT_SEC * HEARTBEAT_MISSES as u64);
/// 连接握手与 hello 回执的超时
const CONNECT_TIMEOUT: Duration = Duration::from_secs(8);
const HELLO_TIMEOUT: Duration = Duration::from_secs(10);
/// 目标预检的总预算：必须小于主节点的失联阈值，避免误判
const PREFLIGHT_TIMEOUT: Duration = Duration::from_secs(5);
/// 断线期间缓存的消息上限：逐秒采样约 1 条/秒，保留最近 8 分钟足够覆盖重连
const MAX_PENDING_MESSAGES: usize = 512;

type WsStream = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<TcpStream>>;

/// 稳定设备标识：跨任务复用，存在 app_meta 里
const DEVICE_ID_KEY: &str = "cluster_device_id";

/// 从节点连接参数
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClusterSlaveConnectPayload {
    /// 主节点地址：`192.168.1.10:17801`，端口可省略
    pub master_addr: String,
    /// 6 位单次配对码
    pub pair_code: String,
    /// 节点名：缺省取本机计算机名
    #[serde(default)]
    pub node_name: Option<String>,
}

/// 解析后的主节点地址
#[derive(Debug, Clone)]
pub struct MasterAddr {
    pub host: String,
    /// 展示与快照用：`host:port`
    pub addr: String,
    pub ws_url: String,
}

/// 解析 `IP[:端口]`，拒绝公网地址与 wss
pub fn parse_master_addr(input: &str) -> Result<MasterAddr, String> {
    let raw = input.trim();
    if raw.is_empty() {
        return Err("请输入主节点地址（IP:端口）".to_string());
    }
    let lower = raw.to_ascii_lowercase();
    if lower.starts_with("wss://") {
        return Err("一期只支持内网明文 ws://，请填写主节点的 IP:端口".to_string());
    }
    let rest = if lower.starts_with("ws://") {
        raw[5..].trim()
    } else {
        raw
    };
    let rest = rest.trim_end_matches('/');
    if rest.is_empty() {
        return Err("主节点地址为空".to_string());
    }
    if rest.contains('/') {
        return Err("地址只需 IP:端口，不要包含路径".to_string());
    }
    let (host, port) = split_host_port(rest)?;
    if host.is_empty() {
        return Err("主节点地址缺少主机".to_string());
    }
    if host
        .parse::<std::net::IpAddr>()
        .ok()
        .is_some_and(|ip| !super::is_private(ip))
    {
        return Err("集群只允许局域网地址，请填写主节点的内网 IP".to_string());
    }
    let addr = format!("{host}:{port}");
    Ok(MasterAddr {
        host,
        ws_url: format!("ws://{addr}/"),
        addr,
    })
}

fn split_host_port(input: &str) -> Result<(String, u16), String> {
    if let Some(stripped) = input.strip_prefix('[') {
        let Some(end) = stripped.find(']') else {
            return Err("IPv6 地址缺少右括号".to_string());
        };
        let host = &stripped[..end];
        let tail = &stripped[end + 1..];
        let port = if tail.is_empty() {
            DEFAULT_PORT
        } else if let Some(port) = tail.strip_prefix(':') {
            parse_port(port)?
        } else {
            return Err("地址格式不正确".to_string());
        };
        return Ok((host.to_string(), port));
    }
    if let Some((host, port)) = input.rsplit_once(':') {
        if !port.is_empty() && port.chars().all(|ch| ch.is_ascii_digit()) {
            return Ok((host.to_string(), parse_port(port)?));
        }
        return Err("地址格式不正确；IPv6 请使用 [地址]:端口".to_string());
    }
    Ok((input.to_string(), DEFAULT_PORT))
}

fn parse_port(text: &str) -> Result<u16, String> {
    text.parse::<u16>()
        .ok()
        .filter(|port| *port > 0)
        .ok_or_else(|| format!("端口不合法：{text}"))
}

/* ---------- 会话状态 ---------- */

struct SlaveState {
    status: String,
    master_addr: String,
    node_name: String,
    node_id: Option<String>,
    plan_name: Option<String>,
    target: Option<String>,
    method: Option<String>,
    concurrent: u32,
    start_at_ms: Option<i64>,
    elapsed_sec: u64,
    qps: f64,
    /// 本机时钟相对主节点的偏移：done 时间戳回传前按它换算到主节点时钟
    clock_offset_ms: f64,
    error: Option<String>,
    missing_vars: Vec<String>,
    joined_at: Option<i64>,
    device_id: Option<String>,
    protocol_version: u32,
    reconnecting: bool,
    phase_index: Option<u32>,
    phase_concurrent: u32,
    reconnect_attempts: u32,
}

impl SlaveState {
    fn new(master_addr: String, node_name: String) -> Self {
        Self {
            status: "connecting".to_string(),
            master_addr,
            node_name,
            node_id: None,
            plan_name: None,
            target: None,
            method: None,
            concurrent: 0,
            start_at_ms: None,
            elapsed_sec: 0,
            qps: 0.0,
            clock_offset_ms: 0.0,
            error: None,
            missing_vars: Vec::new(),
            joined_at: None,
            device_id: None,
            protocol_version: PROTOCOL_VERSION,
            reconnecting: false,
            phase_index: None,
            phase_concurrent: 0,
            reconnect_attempts: 0,
        }
    }

    fn idle() -> Self {
        let mut state = Self::new(String::new(), String::new());
        state.status = "idle".to_string();
        state
    }

    fn snapshot(&self) -> ClusterSlaveSnapshot {
        let elapsed_sec = if self.status == "running" {
            self.start_at_ms
                .map(|at| (now_ms() - at).max(0) as u64 / 1000)
                .unwrap_or(self.elapsed_sec)
        } else {
            self.elapsed_sec
        };
        ClusterSlaveSnapshot {
            status: self.status.clone(),
            master_addr: self.master_addr.clone(),
            node_name: self.node_name.clone(),
            node_id: self.node_id.clone(),
            plan_name: self.plan_name.clone(),
            target: self.target.clone(),
            method: self.method.clone(),
            concurrent: self.concurrent,
            start_at_ms: self.start_at_ms,
            elapsed_sec,
            qps: self.qps,
            error: self.error.clone(),
            missing_vars: self.missing_vars.clone(),
            joined_at: self.joined_at,
            device_id: self.device_id.clone(),
            protocol_version: self.protocol_version,
            reconnecting: self.reconnecting,
            phase_index: self.phase_index,
            phase_concurrent: self.phase_concurrent,
            reconnect_attempts: self.reconnect_attempts,
        }
    }
}

struct RunningSlave {
    abort: Arc<AtomicBool>,
    notify: Arc<Notify>,
    state: Arc<Mutex<SlaveState>>,
    /// 后台会话任务：测试里用 `kill` 模拟进程被杀
    #[cfg_attr(not(test), allow(dead_code))]
    task: tauri::async_runtime::JoinHandle<()>,
}

/// 从节点会话管理器：同一进程只允许一个从节点会话
#[derive(Default)]
pub struct ClusterSlave {
    inner: Mutex<Option<RunningSlave>>,
}

impl ClusterSlave {
    /// 反向连接主节点；连接与配对在后台进行，返回值只反映初始快照
    pub fn connect(
        &self,
        pool: DbPool,
        app: AppHandle,
        payload: ClusterSlaveConnectPayload,
    ) -> Result<ClusterSlaveSnapshot, String> {
        self.connect_with(pool, Arc::new(app), payload)
    }

    /// 与 `connect` 相同，但接受解耦后的宿主接口：验收测试可注入 mock 应用句柄
    pub(crate) fn connect_with(
        &self,
        pool: DbPool,
        app: Arc<dyn ClusterApp>,
        payload: ClusterSlaveConnectPayload,
    ) -> Result<ClusterSlaveSnapshot, String> {
        let master = parse_master_addr(&payload.master_addr)?;
        let pair_code = payload.pair_code.trim().to_string();
        if pair_code.is_empty() {
            return Err("请输入主节点显示的配对码".to_string());
        }
        let node_name = payload
            .node_name
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| local_node_name("node"));
        if node_name.chars().count() > 64 {
            return Err("节点名不能超过 64 个字符".to_string());
        }

        let state = Arc::new(Mutex::new(SlaveState::new(
            master.addr.clone(),
            node_name.clone(),
        )));
        let abort = Arc::new(AtomicBool::new(false));
        let notify = Arc::new(Notify::new());
        let snapshot = state.lock().snapshot();
        {
            let mut guard = self.inner.lock();
            if let Some(current) = guard.as_ref() {
                let status = current.state.lock().status.clone();
                if !matches!(status.as_str(), "done" | "lost" | "left" | "rejected") {
                    return Err("已有从节点会话在进行中，请先断开".to_string());
                }
            }
            let app_for_task = Arc::clone(&app);
            let state_for_task = state.clone();
            let task_abort = abort.clone();
            let task_notify = notify.clone();
            let task = tauri::async_runtime::spawn(async move {
                run_slave(
                    app_for_task,
                    pool,
                    master,
                    node_name,
                    pair_code,
                    task_abort,
                    task_notify,
                    state_for_task,
                )
                .await;
            });
            *guard = Some(RunningSlave {
                abort,
                notify,
                state,
                task,
            });
        }
        Ok(snapshot)
    }

    /// 模拟进程被杀：直接中止后台任务，不发送 abort（验收测试用）
    #[cfg(test)]
    pub(crate) fn kill(&self) -> bool {
        let guard = self.inner.lock();
        let Some(running) = guard.as_ref() else {
            return false;
        };
        running.task.abort();
        true
    }

    pub fn snapshot(&self) -> ClusterSlaveSnapshot {
        match self.inner.lock().as_ref() {
            Some(running) => running.state.lock().snapshot(),
            None => SlaveState::idle().snapshot(),
        }
    }

    /// 主动断开：通知后台任务中止引擎并向主节点发 abort
    pub fn disconnect(&self) -> Result<(), String> {
        let guard = self.inner.lock();
        let Some(running) = guard.as_ref() else {
            return Err("当前没有从节点会话".to_string());
        };
        running.abort.store(true, Ordering::SeqCst);
        {
            let mut state = running.state.lock();
            if !matches!(state.status.as_str(), "done" | "lost" | "rejected" | "left") {
                state.status = "left".to_string();
            }
            state.qps = 0.0;
        }
        running.notify.notify_one();
        Ok(())
    }
}

/* ---------- 后台会话 ---------- */

struct LocalPrepare {
    prepared: PreparedStress,
    job_id: u64,
    watch: Arc<AtomicBool>,
    accepted: u32,
    target: String,
    method: String,
}

struct PrepareFailure {
    error: String,
    missing: Vec<String>,
}

fn prepare_failure(error: impl Into<String>) -> PrepareFailure {
    PrepareFailure {
        error: error.into(),
        missing: Vec::new(),
    }
}

/* ---------- 连接与重连 ---------- */

/// 断线期间缓存出站消息：逐秒采样最多保留最近 8 分钟
#[derive(Default)]
struct Link {
    tx: Mutex<Option<mpsc::UnboundedSender<ClusterMessage>>>,
    pending: Mutex<VecDeque<ClusterMessage>>,
}

impl Link {
    fn attach(&self, tx: &mpsc::UnboundedSender<ClusterMessage>) {
        {
            let mut pending = self.pending.lock();
            for message in pending.drain(..) {
                let _ = tx.send(message);
            }
        }
        *self.tx.lock() = Some(tx.clone());
    }

    fn detach(&self) {
        *self.tx.lock() = None;
    }

    /// 已连接就直接发送；断线期间入队等待重连后补发
    fn send(&self, message: ClusterMessage) -> bool {
        let tx = self.tx.lock().clone();
        if let Some(tx) = tx {
            if tx.send(message.clone()).is_ok() {
                return true;
            }
        }
        let mut pending = self.pending.lock();
        if pending.len() >= MAX_PENDING_MESSAGES {
            pending.pop_front();
        }
        pending.push_back(message);
        false
    }
}

/// 跨连接保留的执行上下文：断线只换链路，引擎与已准备的配置不重建
struct SlaveRuntime {
    prepared: Option<PreparedStress>,
    local_job: Option<u64>,
    watch: Option<Arc<AtomicBool>>,
    running_job: Option<u64>,
    engine_job: Option<tauri::async_runtime::JoinHandle<()>>,
    outcome: Option<EngineOutcome>,
    started: bool,
    token: Option<String>,
    protocol_version: u32,
}

impl SlaveRuntime {
    fn new() -> Self {
        Self {
            prepared: None,
            local_job: None,
            watch: None,
            running_job: None,
            engine_job: None,
            outcome: None,
            started: false,
            token: None,
            protocol_version: PROTOCOL_VERSION,
        }
    }
}

enum ConnEnd {
    /// 连接中断，按令牌重连
    Reconnect,
    Terminate {
        status: &'static str,
        error: Option<String>,
    },
}

enum HelloFailure {
    /// 主节点明确拒绝（配对码 / 版本 / 令牌问题），重试无意义
    Rejected(String),
    /// 网络层断开或超时，可重连
    Closed(String),
}

async fn connect_ws(master: &MasterAddr) -> Result<WsStream, String> {
    let mut config = WebSocketConfig::default();
    config.max_message_size = Some(MAX_MESSAGE_BYTES);
    config.max_frame_size = Some(MAX_MESSAGE_BYTES);
    match tokio::time::timeout(
        CONNECT_TIMEOUT,
        tokio_tungstenite::connect_async_with_config(master.ws_url.as_str(), Some(config), false),
    )
    .await
    {
        Ok(Ok(pair)) => Ok(pair.0),
        Ok(Err(error)) => Err(format!("连接主节点失败：{error}")),
        Err(_) => Err(format!(
            "连接主节点超时（{} 秒）",
            CONNECT_TIMEOUT.as_secs()
        )),
    }
}

type WsRead = futures_util::stream::SplitStream<WsStream>;

fn spawn_connection(
    ws: WsStream,
) -> (
    mpsc::UnboundedSender<ClusterMessage>,
    WsRead,
    tauri::async_runtime::JoinHandle<()>,
) {
    let (mut sink, read) = ws.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<ClusterMessage>();
    let writer = tauri::async_runtime::spawn(async move {
        while let Some(message) = rx.recv().await {
            let text = match serde_json::to_string(&message) {
                Ok(text) => text,
                Err(error) => {
                    log::warn!("集群从节点消息序列化失败：{error}");
                    continue;
                }
            };
            if sink.send(Message::Text(text.into())).await.is_err() {
                break;
            }
            // 立即刷出：拒绝 / 失联前最后一条消息不能输给随后的 close 帧
            if sink.flush().await.is_err() {
                break;
            }
        }
        let _ = sink.close().await;
    });
    (tx, read, writer)
}

/// 等待 hello 回执：期间主节点可能先发版本拒绝
async fn read_hello_ack(read: &mut WsRead) -> Result<HelloAckPayload, HelloFailure> {
    let deadline = tokio::time::sleep(HELLO_TIMEOUT);
    tokio::pin!(deadline);
    loop {
        tokio::select! {
            _ = &mut deadline => {
                return Err(HelloFailure::Closed(format!(
                    "主节点未回执 hello（超时 {} 秒）",
                    HELLO_TIMEOUT.as_secs()
                )));
            }
            frame = read.next() => {
                let Some(frame) = frame else {
                    return Err(HelloFailure::Closed("主节点关闭了连接".to_string()));
                };
                match frame {
                    Ok(Message::Text(text)) => {
                        let message: ClusterMessage = match serde_json::from_str(&text) {
                            Ok(message) => message,
                            Err(error) => {
                                return Err(HelloFailure::Rejected(format!(
                                    "主节点回执无法解析：{error}"
                                )));
                            }
                        };
                        match message {
                            ClusterMessage::HelloAck(ack) => return Ok(ack),
                            ClusterMessage::Error(ErrorPayload { message }) => {
                                return Err(HelloFailure::Rejected(message));
                            }
                            _ => {}
                        }
                    }
                    Ok(Message::Close(_)) => {
                        return Err(HelloFailure::Closed("主节点关闭了连接".to_string()));
                    }
                    Ok(Message::Binary(_)) => {
                        return Err(HelloFailure::Rejected(
                            "集群协议只接受 JSON 文本消息".to_string(),
                        ));
                    }
                    Ok(Message::Ping(_)) | Ok(Message::Pong(_)) | Ok(Message::Frame(_)) => {}
                    Err(error) => {
                        return Err(HelloFailure::Closed(format!(
                            "读取主节点消息失败：{error}"
                        )));
                    }
                }
            }
        }
    }
}

fn local_device_id(pool: &DbPool) -> String {
    if let Ok(conn) = pool.get() {
        if let Ok(Some(value)) = conn
            .query_row(
                "SELECT value FROM app_meta WHERE key = ?1",
                params![DEVICE_ID_KEY],
                |row| row.get::<_, String>(0),
            )
            .optional()
        {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return trimmed.to_string();
            }
        }
        let id = format!("device-{}", Uuid::new_v4().simple());
        let _ = conn.execute(
            "INSERT INTO app_meta (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![DEVICE_ID_KEY, id],
        );
        return id;
    }
    format!("device-{}", Uuid::new_v4().simple())
}

fn local_capability(pool: &DbPool, node_name: &str, device_id: &str) -> Option<DeviceCapability> {
    let conn = pool.get().ok()?;
    let result = crate::commands::capacity::latest_result(&conn)
        .ok()
        .flatten()?;
    Some(DeviceCapability {
        device_id: Some(device_id.to_string()),
        node_name: node_name.to_string(),
        os: Some(result.os.clone()),
        cores: result.cores,
        capacity_concurrent: result.saturation_concurrent,
        capacity_qps: result.max_qps,
        probed_at: Some(result.probed_at),
    })
}

fn drain_outcome(
    runtime: &mut SlaveRuntime,
    engine_rx: &mut Option<mpsc::UnboundedReceiver<EngineOutcome>>,
) {
    let Some(receiver) = engine_rx.as_mut() else {
        return;
    };
    match receiver.try_recv() {
        Ok(outcome) => {
            *engine_rx = None;
            runtime.outcome = Some(outcome);
        }
        Err(mpsc::error::TryRecvError::Disconnected) => *engine_rx = None,
        Err(mpsc::error::TryRecvError::Empty) => {}
    }
}

fn deliver_done(runtime: &mut SlaveRuntime, state: &Arc<Mutex<SlaveState>>, link: &Link) -> bool {
    let Some(outcome) = runtime.outcome.as_ref() else {
        return true;
    };
    let offset_ms = state.lock().clock_offset_ms;
    let payload = done_payload(outcome, offset_ms);
    let elapsed_sec = outcome
        .report
        .end_time
        .unwrap_or(0)
        .saturating_sub(outcome.report.start_time.unwrap_or(0)) as u64
        / 1000;
    {
        let mut guard = state.lock();
        guard.elapsed_sec = elapsed_sec;
        guard.qps = 0.0;
        guard.phase_index = None;
    }
    if link.send(ClusterMessage::Done(payload)) {
        runtime.outcome = None;
        true
    } else {
        false
    }
}

fn start_engine(
    app: Arc<dyn ClusterApp>,
    runtime: &mut SlaveRuntime,
    engine_rx: &mut Option<mpsc::UnboundedReceiver<EngineOutcome>>,
    state: &Arc<Mutex<SlaveState>>,
    link: &Arc<Link>,
    at_ms: i64,
) -> Result<(), String> {
    let (Some(prepared), Some(job), Some(watch)) = (
        runtime.prepared.take(),
        runtime.local_job.take(),
        runtime.watch.take(),
    ) else {
        return Err("尚未收到可执行的配置".to_string());
    };
    let state_for_sink = state.clone();
    let link_for_sink = link.clone();
    let sink: EngineSampleSink = Arc::new(move |sample| {
        state_for_sink.lock().qps = sample.qps;
        link_for_sink.send(ClusterMessage::Sample(sample));
    });
    let (outcome_tx, outcome_rx) = mpsc::unbounded_channel();
    let app_for_task = Arc::clone(&app);
    let handle = tauri::async_runtime::spawn(async move {
        let outcome = run_engine(
            prepared,
            job,
            None,
            Some(&watch),
            EngineOptions {
                start_at_ms: Some(at_ms),
                sink: Some(sink),
                persist: None,
            },
        )
        .await;
        let _ = outcome_tx.send(outcome);
        finish_local(app_for_task.as_ref(), job);
    });
    *engine_rx = Some(outcome_rx);
    runtime.running_job = Some(job);
    runtime.engine_job = Some(handle);
    runtime.started = true;
    Ok(())
}

fn stop_engine(
    app: &dyn ClusterApp,
    runtime: &mut SlaveRuntime,
    engine_rx: &mut Option<mpsc::UnboundedReceiver<EngineOutcome>>,
) {
    if let Some(watch) = runtime.watch.as_ref() {
        watch.store(true, Ordering::SeqCst);
    }
    if let Some(job) = runtime.running_job.take() {
        finish_local(app, job);
    }
    if let Some(handle) = runtime.engine_job.take() {
        handle.abort();
    }
    *engine_rx = None;
}

fn mark_reconnecting(state: &Arc<Mutex<SlaveState>>, attempts: u32, elapsed: Duration) {
    let mut guard = state.lock();
    guard.reconnecting = true;
    guard.reconnect_attempts = attempts;
    guard.error = Some(format!(
        "与主节点连接中断，正在重连（第 {attempts} 次，已等待 {}s）",
        elapsed.as_secs()
    ));
}

async fn wait_backoff(
    runtime: &mut SlaveRuntime,
    engine_rx: &mut Option<mpsc::UnboundedReceiver<EngineOutcome>>,
    attempts: u32,
) {
    let seconds = (1u64 << attempts.saturating_sub(1).min(5)).min(RECONNECT_BACKOFF_MAX_SEC);
    let deadline = Instant::now() + Duration::from_secs(seconds);
    while Instant::now() < deadline {
        let left = deadline.saturating_duration_since(Instant::now());
        tokio::time::sleep(left.min(Duration::from_millis(250))).await;
        drain_outcome(runtime, engine_rx);
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_slave(
    app: Arc<dyn ClusterApp>,
    pool: DbPool,
    master: MasterAddr,
    node_name: String,
    pair_code: String,
    abort: Arc<AtomicBool>,
    notify: Arc<Notify>,
    state: Arc<Mutex<SlaveState>>,
) {
    let device_id = local_device_id(&pool);
    let capability = local_capability(&pool, &node_name, &device_id);
    let legacy_capacity = capability
        .as_ref()
        .map(|item| item.capacity_concurrent)
        .filter(|value| *value > 0);
    let mut pair_code = Some(pair_code);
    let link = Arc::new(Link::default());
    let mut runtime = SlaveRuntime::new();
    let mut engine_rx: Option<mpsc::UnboundedReceiver<EngineOutcome>> = None;
    let mut reconnect_since: Option<Instant> = None;
    let mut attempts = 0u32;
    {
        let mut guard = state.lock();
        guard.device_id = Some(device_id.clone());
        guard.protocol_version = PROTOCOL_VERSION;
    }

    loop {
        if abort.load(Ordering::SeqCst) {
            set_status(&state, "left", Some("已断开与主节点的会话".to_string()));
            emit_state(app.as_ref(), &state);
            return;
        }
        if let Some(since) = reconnect_since {
            if since.elapsed() >= Duration::from_secs(RECONNECT_GRACE_SEC) {
                stop_engine(app.as_ref(), &mut runtime, &mut engine_rx);
                set_status(
                    &state,
                    "lost",
                    Some(format!(
                        "与主节点重连超过 {} 分钟，已停止本机压测",
                        RECONNECT_GRACE_SEC / 60
                    )),
                );
                emit_state(app.as_ref(), &state);
                return;
            }
        }

        let ws = match connect_ws(&master).await {
            Ok(ws) => ws,
            Err(error) => {
                if !runtime.started {
                    finish_with_error(app.as_ref(), &state, "lost", error);
                    return;
                }
                let since = *reconnect_since.get_or_insert_with(Instant::now);
                attempts += 1;
                mark_reconnecting(&state, attempts, since.elapsed());
                emit_state(app.as_ref(), &state);
                wait_backoff(&mut runtime, &mut engine_rx, attempts).await;
                continue;
            }
        };
        let (tx, mut read, mut writer) = spawn_connection(ws);
        let hello = HelloPayload {
            version: PROTOCOL_VERSION,
            pair_code: pair_code.take().unwrap_or_default(),
            node_name: node_name.clone(),
            capacity_concurrent: legacy_capacity,
            device_id: Some(device_id.clone()),
            session_token: runtime.started.then(|| runtime.token.clone()).flatten(),
        };
        let _ = tx.send(ClusterMessage::Hello(hello));
        let ack = match read_hello_ack(&mut read).await {
            Ok(ack) => ack,
            Err(HelloFailure::Rejected(message)) => {
                drop(tx);
                let _ = tokio::time::timeout(Duration::from_secs(2), &mut writer).await;
                stop_engine(app.as_ref(), &mut runtime, &mut engine_rx);
                finish_with_error(app.as_ref(), &state, "rejected", message);
                return;
            }
            Err(HelloFailure::Closed(message)) => {
                drop(tx);
                let _ = tokio::time::timeout(Duration::from_secs(2), &mut writer).await;
                if !runtime.started {
                    finish_with_error(app.as_ref(), &state, "lost", message);
                    return;
                }
                let since = *reconnect_since.get_or_insert_with(Instant::now);
                attempts += 1;
                mark_reconnecting(&state, attempts, since.elapsed());
                emit_state(app.as_ref(), &state);
                wait_backoff(&mut runtime, &mut engine_rx, attempts).await;
                continue;
            }
        };
        if !(MIN_PROTOCOL_VERSION..=PROTOCOL_VERSION).contains(&ack.version) {
            drop(tx);
            let _ = tokio::time::timeout(Duration::from_secs(2), &mut writer).await;
            finish_with_error(
                app.as_ref(),
                &state,
                "rejected",
                format!(
                    "协议版本不兼容：主节点 v{}，本机支持 v{}–v{}",
                    ack.version, MIN_PROTOCOL_VERSION, PROTOCOL_VERSION
                ),
            );
            return;
        }
        runtime.token = Some(ack.token.clone());
        runtime.protocol_version = ack.version;
        {
            let mut guard = state.lock();
            guard.node_id = Some(ack.node_id.clone());
            guard.protocol_version = ack.version;
            if guard.joined_at.is_none() {
                guard.joined_at = Some(now_ms());
            }
            guard.reconnecting = false;
            guard.reconnect_attempts = 0;
            guard.error = None;
        }
        if ack.version >= 2 {
            if let Some(capability) = capability.clone() {
                let _ = tx.send(ClusterMessage::CapabilityReport(capability));
            }
        }
        link.attach(&tx);
        reconnect_since = None;
        attempts = 0;
        emit_state(app.as_ref(), &state);

        let end = run_link_loop(
            &app,
            &pool,
            &state,
            &abort,
            &notify,
            &link,
            &mut read,
            &mut runtime,
            &mut engine_rx,
        )
        .await;
        link.detach();
        drop(tx);
        if tokio::time::timeout(Duration::from_secs(2), &mut writer)
            .await
            .is_err()
        {
            writer.abort();
        }

        match end {
            ConnEnd::Terminate { status, error } => {
                if status != "done" {
                    if let Some(watch) = runtime.watch.as_ref() {
                        watch.store(true, Ordering::SeqCst);
                    }
                }
                if let Some(job) = runtime.running_job.take() {
                    finish_local(app.as_ref(), job);
                }
                set_status(&state, status, error);
                emit_state(app.as_ref(), &state);
                return;
            }
            ConnEnd::Reconnect => {
                let since = *reconnect_since.get_or_insert_with(Instant::now);
                attempts += 1;
                mark_reconnecting(&state, attempts, since.elapsed());
                emit_state(app.as_ref(), &state);
                wait_backoff(&mut runtime, &mut engine_rx, attempts).await;
            }
        }
    }
}

/// 单条连接的收发循环：准备 → 起跑 → 断线时保留引擎并返回重连
#[allow(clippy::too_many_arguments)]
async fn run_link_loop(
    app: &Arc<dyn ClusterApp>,
    pool: &DbPool,
    state: &Arc<Mutex<SlaveState>>,
    abort: &Arc<AtomicBool>,
    notify: &Notify,
    link: &Arc<Link>,
    read: &mut WsRead,
    runtime: &mut SlaveRuntime,
    engine_rx: &mut Option<mpsc::UnboundedReceiver<EngineOutcome>>,
) -> ConnEnd {
    let mut ticker = tokio::time::interval(Duration::from_secs(1));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    ticker.tick().await;
    let mut last_rx = Instant::now();

    loop {
        drain_outcome(runtime, engine_rx);
        if runtime.outcome.is_some() {
            if deliver_done(runtime, state, link) {
                return ConnEnd::Terminate {
                    status: "done",
                    error: None,
                };
            }
            return ConnEnd::Reconnect;
        }
        if abort.load(Ordering::SeqCst) {
            return ConnEnd::Terminate {
                status: "left",
                error: Some("已断开与主节点的会话".to_string()),
            };
        }

        let frame = tokio::select! {
            _ = notify.notified() => {
                abort.store(true, Ordering::SeqCst);
                if let Some(watch) = runtime.watch.as_ref() {
                    watch.store(true, Ordering::SeqCst);
                }
                let _ = link.send(ClusterMessage::Abort(AbortPayload {
                    reason: Some("从节点主动断开".to_string()),
                }));
                return ConnEnd::Terminate {
                    status: "left",
                    error: Some("已断开与主节点的会话".to_string()),
                };
            }
            _ = ticker.tick() => {
                if last_rx.elapsed() >= MASTER_SILENCE {
                    return if runtime.started {
                        ConnEnd::Reconnect
                    } else {
                        ConnEnd::Terminate {
                            status: "lost",
                            error: Some("与主节点失联超过 6 秒".to_string()),
                        }
                    };
                }
                continue;
            }
            frame = read.next() => frame,
        };
        let Some(frame) = frame else {
            return if runtime.started {
                ConnEnd::Reconnect
            } else {
                ConnEnd::Terminate {
                    status: "lost",
                    error: Some("主节点关闭了连接".to_string()),
                }
            };
        };
        last_rx = Instant::now();
        let text = match frame {
            Ok(Message::Text(text)) => text,
            Ok(Message::Close(_)) => {
                return if runtime.started {
                    ConnEnd::Reconnect
                } else {
                    ConnEnd::Terminate {
                        status: "lost",
                        error: Some("主节点关闭了连接".to_string()),
                    }
                };
            }
            Ok(Message::Binary(_)) => {
                let _ = link.send(ClusterMessage::Error(ErrorPayload {
                    message: "集群协议只接受 JSON 文本消息".to_string(),
                }));
                continue;
            }
            Ok(Message::Ping(_)) | Ok(Message::Pong(_)) | Ok(Message::Frame(_)) => continue,
            Err(error) => {
                return if runtime.started {
                    ConnEnd::Reconnect
                } else {
                    ConnEnd::Terminate {
                        status: "lost",
                        error: Some(format!("读取主节点消息失败：{error}")),
                    }
                };
            }
        };
        let message: ClusterMessage = match serde_json::from_str(&text) {
            Ok(message) => message,
            Err(error) => {
                log::warn!("集群从节点收到无法解析的消息：{error}");
                continue;
            }
        };

        match message {
            ClusterMessage::Ping(PingPayload { at_ms }) => {
                let _ = link.send(ClusterMessage::Pong(PongPayload { at_ms }));
            }
            ClusterMessage::Pong(_) => {}
            ClusterMessage::TimeProbe(TimeProbePayload { seq, t0 }) => {
                let _ = link.send(ClusterMessage::TimeAck(TimeAckPayload {
                    seq,
                    t0,
                    t1: now_ms(),
                }));
            }
            ClusterMessage::PhaseStart(PhaseStartPayload {
                index,
                node_concurrent,
                ..
            }) => {
                {
                    let mut guard = state.lock();
                    guard.phase_index = Some(index);
                    guard.phase_concurrent = node_concurrent;
                    guard.concurrent = node_concurrent;
                }
                emit_state(app.as_ref(), state);
            }
            ClusterMessage::Prepare(payload) => {
                if runtime.started {
                    continue;
                }
                let plan_name = payload.plan_name.clone();
                let requested = payload.concurrent;
                {
                    let mut guard = state.lock();
                    guard.status = "preparing".to_string();
                    guard.plan_name = Some(plan_name);
                    guard.concurrent = requested;
                    guard.error = None;
                    guard.missing_vars.clear();
                }
                emit_state(app.as_ref(), state);
                let result = match app.try_state() {
                    Some(app_state) => prepare_local(app_state.inner(), pool, payload, abort),
                    None => Err(prepare_failure("应用状态不可用")),
                };
                match result {
                    Ok(local) => {
                        {
                            let mut guard = state.lock();
                            guard.target = Some(local.target.clone());
                            guard.method = Some(local.method.clone());
                            guard.concurrent = local.accepted;
                        }
                        runtime.prepared = Some(local.prepared);
                        runtime.local_job = Some(local.job_id);
                        runtime.watch = Some(local.watch);
                        let _ = link.send(ClusterMessage::PrepareAck(PrepareAckPayload {
                            ok: true,
                            error: None,
                            missing_vars: Vec::new(),
                            accepted_concurrent: local.accepted,
                        }));
                        // 预检用渲染后的真实目标：变量缺失在前一步已拦截
                        let target = runtime
                            .prepared
                            .as_ref()
                            .map(|item| item.url_text.clone())
                            .unwrap_or_default();
                        let preflight = preflight_target(&target).await;
                        // 预检期间没有读连接，缓冲的 Ping 尚未消费；
                        // 以预检结束时刻为准，避免紧接着的 tick 误判失联
                        last_rx = Instant::now();
                        if abort.load(Ordering::SeqCst) {
                            let _ = link.send(ClusterMessage::Abort(AbortPayload {
                                reason: Some("从节点在准备阶段中止".to_string()),
                            }));
                            return ConnEnd::Terminate {
                                status: "left",
                                error: Some("从节点在准备阶段中止".to_string()),
                            };
                        }
                        if preflight.ok {
                            set_status(state, "ready", None);
                            let _ = link.send(ClusterMessage::Preflight(PreflightPayload {
                                ok: true,
                                error: None,
                            }));
                            emit_state(app.as_ref(), state);
                        } else {
                            if let Some(job) = runtime.local_job.take() {
                                finish_local(app.as_ref(), job);
                            }
                            let detail = preflight
                                .error
                                .clone()
                                .unwrap_or_else(|| "目标不可达".to_string());
                            set_status(state, "rejected", Some(detail.clone()));
                            let _ = link.send(ClusterMessage::Preflight(PreflightPayload {
                                ok: false,
                                error: preflight.error,
                            }));
                            return ConnEnd::Terminate {
                                status: "rejected",
                                error: Some(detail),
                            };
                        }
                    }
                    Err(failure) => {
                        {
                            let mut guard = state.lock();
                            guard.missing_vars = failure.missing.clone();
                        }
                        set_status(state, "rejected", Some(failure.error.clone()));
                        let _ = link.send(ClusterMessage::PrepareAck(PrepareAckPayload {
                            ok: false,
                            error: Some(failure.error.clone()),
                            missing_vars: failure.missing,
                            accepted_concurrent: 0,
                        }));
                        return ConnEnd::Terminate {
                            status: "rejected",
                            error: Some(failure.error),
                        };
                    }
                }
            }
            ClusterMessage::Start(StartPayload { at_ms, offset_ms }) => {
                // 主节点时钟 → 本机时钟：偏移由时钟探测得到，正值表示本机更快
                let local_at_ms = at_ms.saturating_add(offset_ms.round() as i64);
                {
                    let mut guard = state.lock();
                    guard.status = "running".to_string();
                    guard.start_at_ms = Some(local_at_ms);
                    guard.elapsed_sec = 0;
                    guard.qps = 0.0;
                    guard.clock_offset_ms = offset_ms;
                    guard.phase_index = None;
                    guard.phase_concurrent = 0;
                    guard.reconnecting = false;
                    guard.error = None;
                }
                emit_state(app.as_ref(), state);
                if let Err(error) = start_engine(
                    Arc::clone(app),
                    runtime,
                    engine_rx,
                    state,
                    link,
                    local_at_ms,
                ) {
                    let _ = link.send(ClusterMessage::Error(ErrorPayload {
                        message: error.clone(),
                    }));
                    return ConnEnd::Terminate {
                        status: "rejected",
                        error: Some(error),
                    };
                }
            }
            ClusterMessage::Abort(AbortPayload { reason }) => {
                abort.store(true, Ordering::SeqCst);
                if let Some(watch) = runtime.watch.as_ref() {
                    watch.store(true, Ordering::SeqCst);
                }
                let detail = reason
                    .filter(|text| !text.trim().is_empty())
                    .unwrap_or_else(|| "主节点中止了任务".to_string());
                return ConnEnd::Terminate {
                    status: "left",
                    error: Some(detail),
                };
            }
            ClusterMessage::Error(ErrorPayload { message }) => {
                if runtime.started {
                    // 运行期可能是主节点临时异常：保留引擎，按令牌重连
                    log::warn!("主节点运行期报错：{message}");
                    return ConnEnd::Reconnect;
                }
                return ConnEnd::Terminate {
                    status: "rejected",
                    error: Some(message),
                };
            }
            _ => {}
        }
    }
}

/// 本地准备：查同名环境 → 核对变量 → 用本机引擎准备 → 登记本地任务
fn prepare_local(
    state: &AppState,
    pool: &DbPool,
    payload: PreparePayload,
    abort: &Arc<AtomicBool>,
) -> Result<LocalPrepare, PrepareFailure> {
    if state.capacity.is_running() {
        return Err(prepare_failure(
            "本机能力检测正在运行，检测期间不能加入分布式压测",
        ));
    }
    let config: StressConfig = serde_json::from_value(payload.config)
        .map_err(|error| prepare_failure(format!("节点无法解析下发的配置：{error}")))?;
    let env_name = payload
        .env_name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty());
    let env_id = match env_name {
        Some(name) => {
            let conn = pool
                .get()
                .map_err(|error| prepare_failure(format!("节点本地数据库不可用：{error}")))?;
            Some(query_env_id(&conn, name)?)
        }
        None => None,
    };
    let missing: Vec<String> = {
        let vars = match env_id {
            Some(id) => {
                let conn = pool
                    .get()
                    .map_err(|error| prepare_failure(format!("节点本地数据库不可用：{error}")))?;
                Some(
                    crate::commands::http::load_vars(&conn, id)
                        .map_err(|error| prepare_failure(format!("读取环境变量失败：{error}")))?,
                )
            }
            None => None,
        };
        payload
            .required_vars
            .iter()
            .filter(|name| {
                vars.as_ref()
                    .map(|values| !values.contains_key(*name))
                    .unwrap_or(true)
            })
            .cloned()
            .collect()
    };
    if !missing.is_empty() {
        let detail = match env_name {
            Some(name) => format!("本机环境「{name}」缺少变量"),
            None => "主节点配置引用了变量，但没有指定环境".to_string(),
        };
        return Err(PrepareFailure {
            error: detail,
            missing,
        });
    }

    let ignore_overlap = config.ignore_overlap;
    let mut config = config;
    config.env_id = env_id;
    // 主节点的计划 / 请求 / 项目 ID 属于主节点数据库，不能用来解析本机环境
    config.project_id = None;
    config.plan_id = None;
    config.request_id = None;
    config.run_group_id = None;
    config.baseline_report_id = None;
    config.concurrent = payload.concurrent.max(1);
    // 长稳确认已在主节点完成，各节点不再重复拦截
    config.long_run_ack = true;
    if config
        .name
        .as_deref()
        .map(str::trim)
        .unwrap_or("")
        .is_empty()
    {
        config.name = Some(payload.plan_name.clone());
    }
    let target = config.url.clone();
    let method = config.method.clone();

    let prepared = prepare_stress(state, config, abort.clone())
        .map_err(|error| prepare_failure(format!("节点准备失败：{error}")))?;
    let (max_jobs, max_total) = state.stress_limits();
    let draft = StressJobDraft {
        name: prepared.name.clone(),
        target: prepared.url_text.clone(),
        target_key: prepared.target_key.clone(),
        plan_id: None,
        request_id: None,
        concurrent: prepared.peak,
    };
    let (job_id, watch, _overlaps) = state
        .stress_jobs
        .try_reserve(draft, max_jobs, max_total, ignore_overlap)
        .map_err(|error| prepare_failure(format!("节点无法登记本地任务：{error}")))?;
    let accepted = prepared.peak;
    Ok(LocalPrepare {
        prepared,
        job_id,
        watch,
        accepted,
        target,
        method,
    })
}

fn query_env_id(conn: &rusqlite::Connection, name: &str) -> Result<i64, PrepareFailure> {
    let found: Option<i64> = conn
        .query_row(
            "SELECT id FROM environments WHERE name = ?1
             ORDER BY is_active DESC, id ASC LIMIT 1",
            params![name],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| prepare_failure(format!("查询本地环境失败：{error}")))?;
    found.ok_or_else(|| {
        prepare_failure(format!(
            "本机未找到环境「{name}」，请在各节点创建同名环境并补全变量"
        ))
    })
}

/// 从引擎结果生成 `done`：合并只认原始计数与毫秒桶
///
/// 时间戳按 `offset_ms` 换算回主节点时钟，避免各节点时钟不同步影响合并口径。
fn done_payload(outcome: &EngineOutcome, offset_ms: f64) -> DonePayload {
    let report = &outcome.report;
    let (start_ms, end_ms) = align_window(
        report.start_time.unwrap_or(0),
        report.end_time.unwrap_or(0),
        offset_ms,
    );
    let snapshot: serde_json::Value =
        serde_json::from_str(&outcome.config_snapshot).unwrap_or(serde_json::Value::Null);
    DonePayload {
        start_ms,
        end_ms,
        total: outcome.sparse.total,
        success: outcome.sparse.success,
        errors: outcome.sparse.errors,
        bytes: outcome.sparse.bytes,
        bytes_sent: outcome.sparse.bytes_sent,
        sum_rt_ms: outcome.sparse.sum_rt_ms,
        sum_ttfb_ms: outcome.sparse.sum_ttfb_ms,
        min_rt: outcome.sparse.min_rt,
        max_rt: outcome.sparse.max_rt,
        latency_buckets: outcome.sparse.latency.clone(),
        ttfb_buckets: outcome.sparse.ttfb.clone(),
        body_buckets: outcome.sparse.body.clone(),
        status_codes: outcome.sparse.status_codes.clone(),
        error_types: outcome.sparse.error_types.clone(),
        cpu_peak: snapshot
            .get("cpuPeakCores")
            .and_then(|value| value.as_f64())
            .unwrap_or(0.0),
        mem_peak_mb: snapshot
            .get("memPeakMb")
            .and_then(|value| value.as_u64())
            .unwrap_or(0),
        stopped: report.status == "stopped",
        errors_dropped: outcome.sparse.errors_dropped,
        records_dropped: outcome.sparse.records_dropped,
        dropped_samples: 0,
    }
}

/// 本机时钟 → 主节点时钟：偏移为正表示本机更快，done 里的窗口要减回去
///
/// 只校正起止时间戳；逐秒采样的 `elapsed_sec` 是相对量，天然不受时钟差影响。
pub(crate) fn align_window(start_ms: i64, end_ms: i64, offset_ms: f64) -> (i64, i64) {
    let shift = offset_ms.round() as i64;
    (start_ms.saturating_sub(shift), end_ms.saturating_sub(shift))
}

/* ---------- 目标预检 ---------- */

struct Preflight {
    ok: bool,
    error: Option<String>,
}

/// DNS + TCP 可达性检查：只验证目标是否可连，不发送业务请求
async fn preflight_target(url_text: &str) -> Preflight {
    let url = match reqwest::Url::parse(url_text) {
        Ok(url) => url,
        Err(error) => {
            return Preflight {
                ok: false,
                error: Some(format!("目标 URL 不合法：{error}")),
            }
        }
    };
    if !matches!(url.scheme(), "http" | "https") {
        return Preflight {
            ok: false,
            error: Some(format!("不支持的目标协议：{}", url.scheme())),
        };
    }
    let Some(host) = url.host_str().map(str::to_string) else {
        return Preflight {
            ok: false,
            error: Some("目标 URL 缺少主机名".to_string()),
        };
    };
    let port = url.port_or_known_default().unwrap_or(80);
    let addrs = match tokio::time::timeout(
        Duration::from_secs(3),
        tokio::net::lookup_host((host.as_str(), port)),
    )
    .await
    {
        Ok(Ok(addrs)) => addrs.collect::<Vec<_>>(),
        Ok(Err(error)) => {
            return Preflight {
                ok: false,
                error: Some(format!("无法解析目标主机 {host}：{error}")),
            }
        }
        Err(_) => {
            return Preflight {
                ok: false,
                error: Some(format!("解析目标主机 {host} 超时")),
            }
        }
    };
    if addrs.is_empty() {
        return Preflight {
            ok: false,
            error: Some(format!("目标主机 {host} 没有可用地址")),
        };
    }
    let deadline = Instant::now() + PREFLIGHT_TIMEOUT;
    let mut last_error = String::new();
    for addr in addrs {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            break;
        }
        match tokio::time::timeout(left, TcpStream::connect(addr)).await {
            Ok(Ok(_)) => {
                return Preflight {
                    ok: true,
                    error: None,
                }
            }
            Ok(Err(error)) => last_error = format!("{addr}: {error}"),
            Err(_) => last_error = format!("{addr}: 连接超时"),
        }
    }
    Preflight {
        ok: false,
        error: Some(format!(
            "目标 {host}:{port} 不可达{}",
            if last_error.is_empty() {
                String::new()
            } else {
                format!("（{last_error}）")
            }
        )),
    }
}

/* ---------- 状态与清理 ---------- */

fn set_status(state: &Arc<Mutex<SlaveState>>, status: &str, error: Option<String>) {
    let mut guard = state.lock();
    guard.status = status.to_string();
    guard.error = error;
    if status != "running" {
        guard.qps = 0.0;
    }
}

fn finish_with_error(
    app: &dyn ClusterApp,
    state: &Arc<Mutex<SlaveState>>,
    status: &str,
    message: String,
) {
    set_status(state, status, Some(message));
    emit_state(app, state);
}

fn emit_state(app: &dyn ClusterApp, state: &Arc<Mutex<SlaveState>>) {
    // 先取快照再发事件：事件派发期间不占状态锁，UI 查询不会被 emit 拖住
    let snapshot = state.lock().snapshot();
    app.emit_slave(snapshot);
}

fn finish_local(app: &dyn ClusterApp, job_id: u64) {
    if let Some(state) = app.try_state() {
        state.stress_jobs.finish(job_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_addr_defaults_port_and_rejects_public_ip() {
        let addr = parse_master_addr("192.168.1.10").unwrap();
        assert_eq!(addr.addr, "192.168.1.10:17801");
        assert_eq!(addr.ws_url, "ws://192.168.1.10:17801/");

        let addr = parse_master_addr(" ws://10.0.0.8:18800/ ").unwrap();
        assert_eq!(addr.addr, "10.0.0.8:18800");

        assert!(parse_master_addr("8.8.8.8:17801").is_err());
        assert!(parse_master_addr("wss://192.168.1.10:17801").is_err());
        assert!(parse_master_addr("192.168.1.10:17801/abc").is_err());
        assert!(parse_master_addr("192.168.1.10:0").is_err());
    }

    #[test]
    fn snapshot_idle_and_running_elapsed() {
        let idle = SlaveState::idle().snapshot();
        assert_eq!(idle.status, "idle");
        assert_eq!(idle.concurrent, 0);

        let mut state = SlaveState::new("192.168.1.10:17801".to_string(), "node-a".to_string());
        state.status = "running".to_string();
        state.start_at_ms = Some(now_ms() - 3_000);
        let snapshot = state.snapshot();
        assert!(snapshot.elapsed_sec >= 2, "{}", snapshot.elapsed_sec);
    }

    #[test]
    fn five_minute_clock_skew_shifts_window_only() {
        // 从快机（+5 分钟）与从慢机（-5 分钟）都换算回主节点时间轴
        let start = 1_700_000_000_000;
        let (skew_start, skew_end) = align_window(start, start + 4_000, 300_000.0);
        assert_eq!(skew_start, start - 300_000);
        assert_eq!(skew_end - skew_start, 4_000);

        let (slow_start, slow_end) = align_window(start, start + 4_000, -300_000.0);
        assert_eq!(slow_start, start + 300_000);
        assert_eq!(slow_end - slow_start, 4_000);

        // 四舍五入：±0.4ms 不影响窗口，±0.6ms 进 1ms
        assert_eq!(align_window(start, start, 0.4), (start, start));
        assert_eq!(align_window(start, start, 0.6), (start - 1, start - 1));
    }
}
