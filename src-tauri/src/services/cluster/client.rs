//! 从节点客户端（一期）
//!
//! 反向连接主节点：配对码换节点身份 → 5 轮时钟探测 → 按名字在本地解析环境变量
//! → 用本机引擎预检目标并等待统一起跑时刻 → 每秒外送采样、结束时回传 `done`。
//!
//! 与主节点一致，传输是内网明文 WebSocket + JSON；变量值只在本机解析，
//! 主节点下发的配置里只有变量名与引用。断线重连、跨网段不在本期。

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
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

use super::host::ClusterApp;
use super::protocol::{
    AbortPayload, ClusterMessage, ClusterSlaveSnapshot, DonePayload, EngineStats, ErrorPayload,
    EnvBundle, HelloAckPayload, HelloPayload, HelloPendingPayload, HelloRejectedPayload,
    PingPayload, PongPayload, PoolRefreshRequestPayload, PoolRefreshResultPayload,
    PreflightPayload, PrepareAckPayload,
    PreparePayload, StartPayload, TimeAckPayload, TimeProbePayload, DEFAULT_PORT,
    ENV_BUNDLE_VERSION, HEARTBEAT_MISSES,
    HEARTBEAT_SEC, MAX_PREPARE_BYTES, PROTOCOL_VERSION,
};
use super::server::local_node_name;
use crate::commands::stress::{
    prepare_stress_with_relay, run_engine, EngineOptions, EngineOutcome, EngineSampleSink,
    PoolRefreshRelay, PreparedStress,
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

/// 从节点连接参数
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClusterSlaveConnectPayload {
    /// 主节点地址：`192.168.1.10:17801`，端口可省略
    pub master_addr: String,
    /// 6 位单次配对码：审批模式可留空
    #[serde(default)]
    pub pair_code: String,
    /// 加入方式：code（配对码）/ request（发现后申请，需主节点审批）
    #[serde(default)]
    pub join_mode: Option<String>,
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
    /// 最近一次回传主节点的引擎累计计数
    engine_stats: Option<EngineStats>,
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
            engine_stats: None,
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
            engine_stats: self.engine_stats.clone(),
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

/* ---------- Token 刷新中继（8.10.5） ---------- */

/// 待返回的刷新结果：oneshot 派发器
type RefreshWaiter = tokio::sync::oneshot::Sender<Result<(String, i64, Option<String>), String>>;

/// 从节点的刷新中继：把请求发给主节点并等待加密结果
///
/// 主节点按条目单飞，节点的 401/过期刷新因此不会互相踢掉 `refreshToken`。
struct WsPoolRefreshRelay {
    tx: mpsc::UnboundedSender<ClusterMessage>,
    pending: Mutex<HashMap<String, RefreshWaiter>>,
    secret: x25519_dalek::StaticSecret,
    master_public: [u8; 32],
    counter: AtomicU64,
}

impl WsPoolRefreshRelay {
    fn new(
        tx: mpsc::UnboundedSender<ClusterMessage>,
        secret: x25519_dalek::StaticSecret,
        master_public: [u8; 32],
    ) -> Self {
        Self {
            tx,
            pending: Mutex::new(HashMap::new()),
            secret,
            master_public,
            counter: AtomicU64::new(0),
        }
    }

    /// 主节点回传结果：解密后派发给等待的 VU
    fn complete(&self, payload: PoolRefreshResultPayload) {
        let Some(sender) = self.pending.lock().remove(&payload.request_id) else {
            return;
        };
        let result = if payload.ok {
            self.decrypt(&payload)
                .map_err(|error| format!("刷新结果解密失败：{error}"))
        } else {
            Err(payload
                .error
                .unwrap_or_else(|| "主节点刷新失败".to_string()))
        };
        let _ = sender.send(result);
    }

    /// 解密 `{"value":..,"expiresAt":..}`，并返回本次写入的列名
    fn decrypt(
        &self,
        payload: &PoolRefreshResultPayload,
    ) -> Result<(String, i64, Option<String>), String> {
        let bundle = EnvBundle {
            version: ENV_BUNDLE_VERSION,
            env_name: None,
            nonce: payload.nonce.clone(),
            payload: payload.payload.clone(),
            digest: payload.digest.clone(),
            secret_names: Vec::new(),
        };
        let plaintext =
            super::env_bundle::decrypt_bundle(&self.secret, &self.master_public, &bundle)?;
        let value: serde_json::Value = serde_json::from_slice(&plaintext)
            .map_err(|error| format!("刷新结果格式错误：{error}"))?;
        let token = value
            .get("value")
            .and_then(|item| item.as_str())
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .ok_or_else(|| "刷新结果缺少 token".to_string())?
            .to_string();
        let expires_at = value
            .get("expiresAt")
            .and_then(|item| item.as_i64())
            .unwrap_or(0);
        // 列名优先取明文里的 column，其次取协议字段（兼容旧主节点）
        let column = value
            .get("column")
            .and_then(|item| item.as_str())
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(str::to_string)
            .or_else(|| {
                payload
                    .column
                    .as_deref()
                    .map(str::trim)
                    .filter(|item| !item.is_empty())
                    .map(str::to_string)
            });
        Ok((token, expires_at, column))
    }

    /// 会话结束时清空等待队列：drop sender 让等待方立即失败
    fn fail_all(&self) {
        self.pending.lock().clear();
    }
}

impl PoolRefreshRelay for WsPoolRefreshRelay {
    fn refresh<'a>(
        &'a self,
        pool: &'a str,
        item_id: i64,
        item_index: usize,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<(String, i64, Option<String>), String>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async move {
            let request_id = format!(
                "pr-{}",
                self.counter.fetch_add(1, Ordering::Relaxed)
            );
            let (sender, receiver) = tokio::sync::oneshot::channel();
            self.pending.lock().insert(request_id.clone(), sender);
            let request = ClusterMessage::PoolRefreshRequest(PoolRefreshRequestPayload {
                request_id: request_id.clone(),
                pool: pool.to_string(),
                item_id,
                item_index,
            });
            if self.tx.send(request).is_err() {
                self.pending.lock().remove(&request_id);
                return Err("与主节点的连接已断开".to_string());
            }
            match tokio::time::timeout(Duration::from_secs(5), receiver).await {
                Ok(Ok(result)) => result,
                Ok(Err(_)) => Err("主节点未回传刷新结果".to_string()),
                Err(_) => {
                    self.pending.lock().remove(&request_id);
                    Err("主节点刷新超时（5 秒）".to_string())
                }
            }
        })
    }
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
        let join_mode = payload
            .join_mode
            .as_deref()
            .map(|value| value.trim().to_ascii_lowercase())
            .filter(|value| value == "request")
            .unwrap_or_else(|| "code".to_string());
        let pair_code = payload.pair_code.trim().to_string();
        if join_mode == "code" && pair_code.is_empty() {
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
                    join_mode,
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

enum RunEnd {
    Done,
    Aborted(String),
    Lost(String),
}

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

#[allow(clippy::too_many_arguments)]
async fn run_slave(
    app: Arc<dyn ClusterApp>,
    pool: DbPool,
    master: MasterAddr,
    node_name: String,
    pair_code: String,
    join_mode: String,
    abort: Arc<AtomicBool>,
    notify: Arc<Notify>,
    state: Arc<Mutex<SlaveState>>,
) {
    let mut config = WebSocketConfig::default();
    config.max_message_size = Some(MAX_PREPARE_BYTES);
    config.max_frame_size = Some(MAX_PREPARE_BYTES);
    let ws = match tokio::time::timeout(
        CONNECT_TIMEOUT,
        tokio_tungstenite::connect_async_with_config(master.ws_url.as_str(), Some(config), false),
    )
    .await
    {
        Ok(Ok(pair)) => pair.0,
        Ok(Err(error)) => {
            finish_with_error(
                app.as_ref(),
                &state,
                "lost",
                format!("连接主节点失败：{error}"),
            );
            return;
        }
        Err(_) => {
            finish_with_error(
                app.as_ref(),
                &state,
                "lost",
                format!("连接主节点超时（{} 秒）", CONNECT_TIMEOUT.as_secs()),
            );
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

    let capacity = pool
        .get()
        .ok()
        .and_then(|conn| {
            crate::commands::capacity::latest_result(&conn)
                .ok()
                .flatten()
        })
        .map(|item| item.saturation_concurrent)
        .filter(|value| *value > 0);
    // EnvBundle 会话密钥：私钥只存在本次会话内存
    let (slave_secret, slave_public) = super::env_bundle::generate_keypair();
    let mut master_public: Option<[u8; 32]> = None;
    let node_capacity = {
        let budget = app.try_state().map(|state| state.budget());
        match budget {
            Some(budget) => Some(super::protocol::NodeCapacity {
                cores: budget.sources.cores,
                mem_gb: budget.sources.mem_gb,
                machine_limit: budget.machine_limit,
            }),
            None => Some(super::protocol::NodeCapacity {
                cores: std::thread::available_parallelism()
                    .map(|value| value.get() as u32)
                    .unwrap_or(4),
                mem_gb: 8.0,
                machine_limit: 1000,
            }),
        }
    };
    let _ = tx.send(ClusterMessage::Hello(HelloPayload {
        version: PROTOCOL_VERSION,
        pair_code,
        node_name: node_name.clone(),
        join_mode: Some(join_mode.clone()),
        device_id: Some(node_name.clone()),
        capacity_concurrent: capacity,
        node_capacity,
        public_key: Some(slave_public),
        pool_columns: true,
    }));

    let mut prepared: Option<PreparedStress> = None;
    let mut local_job: Option<u64> = None;
    let mut watch: Option<Arc<AtomicBool>> = None;
    let mut greeted = false;
    let mut last_rx = Instant::now();
    // 主节点是否支持统一刷新 token：旧主节点为 false，退回本地单飞
    let mut relay_supported = false;
    let mut relay_handle: Option<Arc<WsPoolRefreshRelay>> = None;
    let request_mode = join_mode == "request";
    // 审批模式要等主节点用户操作，放宽到 120 秒；配对码模式仍为 10 秒
    let mut hello_deadline = Instant::now()
        + if request_mode {
            Duration::from_secs(super::server::JOIN_WAIT_SEC)
        } else {
            HELLO_TIMEOUT
        };

    let mut ticker = tokio::time::interval(Duration::from_secs(1));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    ticker.tick().await;

    loop {
        tokio::select! {
            _ = notify.notified() => {
                abort.store(true, Ordering::SeqCst);
                let _ = tx.send(ClusterMessage::Abort(AbortPayload {
                    reason: Some("从节点主动断开".to_string()),
                }));
                set_status(&state, "left", Some("已断开与主节点的会话".to_string()));
                break;
            }
            _ = ticker.tick() => {
                // 审批等待期主节点不会发消息，此时不能按心跳判失联
                if greeted && last_rx.elapsed() >= MASTER_SILENCE {
                    abort.store(true, Ordering::SeqCst);
                    set_status(&state, "lost", Some("与主节点失联超过 6 秒".to_string()));
                    break;
                }
                if !greeted && Instant::now() >= hello_deadline {
                    abort.store(true, Ordering::SeqCst);
                    let message = if request_mode {
                        "等待主机接受超时（120 秒），请重新申请".to_string()
                    } else {
                        "主节点未回执 hello（超时 10 秒）".to_string()
                    };
                    set_status(&state, "rejected", Some(message));
                    break;
                }
            }
            frame = read.next() => {
                let Some(frame) = frame else {
                    if !matches!(state.lock().status.as_str(), "done" | "left" | "rejected") {
                        abort.store(true, Ordering::SeqCst);
                        set_status(&state, "lost", Some("主节点关闭了连接".to_string()));
                    }
                    break;
                };
                last_rx = Instant::now();
                let text = match frame {
                    Ok(Message::Text(text)) => text,
                    Ok(Message::Close(_)) => {
                        abort.store(true, Ordering::SeqCst);
                        set_status(&state, "lost", Some("主节点关闭了连接".to_string()));
                        break;
                    }
                    Ok(Message::Binary(_)) => {
                        let _ = tx.send(ClusterMessage::Error(ErrorPayload {
                            message: "集群协议只接受 JSON 文本消息".to_string(),
                        }));
                        continue;
                    }
                    Ok(Message::Ping(_)) | Ok(Message::Pong(_)) | Ok(Message::Frame(_)) => continue,
                    Err(error) => {
                        abort.store(true, Ordering::SeqCst);
                        set_status(&state, "lost", Some(format!("读取主节点消息失败：{error}")));
                        break;
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
                    ClusterMessage::HelloAck(HelloAckPayload {
                        version,
                        node_id,
                        public_key,
                        pool_refresh_relay,
                        ..
                    }) => {
                        if version != PROTOCOL_VERSION {
                            abort.store(true, Ordering::SeqCst);
                            set_status(
                                &state,
                                "rejected",
                                Some(format!(
                                    "协议版本不一致：主节点 v{version}，本机 v{PROTOCOL_VERSION}，请升级后重试"
                                )),
                            );
                            break;
                        }
                        greeted = true;
                        relay_supported = pool_refresh_relay;
                        if let Some(raw) = public_key {
                            match super::env_bundle::decode_public(&raw) {
                                Ok(key) => master_public = Some(key),
                                Err(error) => {
                                    log::warn!("主节点公钥解析失败：{error}");
                                }
                            }
                        }
                        {
                            let mut guard = state.lock();
                            guard.node_id = Some(node_id);
                            guard.joined_at = Some(now_ms());
                        }
                        emit_state(app.as_ref(), &state);
                    }
                    ClusterMessage::HelloPending(HelloPendingPayload {
                        message,
                        timeout_sec,
                        ..
                    }) => {
                        let mut guard = state.lock();
                        guard.status = "pending".to_string();
                        guard.error = message.clone().or(Some("等待主机接受".to_string()));
                        drop(guard);
                        let wait = if timeout_sec > 0 {
                            Duration::from_secs(timeout_sec)
                        } else {
                            Duration::from_secs(super::server::JOIN_WAIT_SEC)
                        };
                        hello_deadline = Instant::now() + wait;
                        emit_state(app.as_ref(), &state);
                    }
                    ClusterMessage::HelloRejected(HelloRejectedPayload { reason }) => {
                        abort.store(true, Ordering::SeqCst);
                        set_status(&state, "rejected", Some(reason));
                        break;
                    }
                    ClusterMessage::TimeProbe(TimeProbePayload { seq, t0 }) => {
                        let _ = tx.send(ClusterMessage::TimeAck(TimeAckPayload {
                            seq,
                            t0,
                            t1: now_ms(),
                        }));
                    }
                    ClusterMessage::Prepare(payload) => {
                        {
                            let mut guard = state.lock();
                            guard.status = "preparing".to_string();
                            guard.plan_name = Some(payload.plan_name.clone());
                            guard.concurrent = payload.concurrent;
                            guard.error = None;
                            guard.missing_vars.clear();
                        }
                        emit_state(app.as_ref(), &state);
                        // 主节点支持时建立刷新中继：token 刷新统一走主节点单飞
                        if relay_supported {
                            if let Some(peer) = master_public {
                                relay_handle = Some(Arc::new(WsPoolRefreshRelay::new(
                                    tx.clone(),
                                    slave_secret.clone(),
                                    peer,
                                )));
                            }
                        }
                        let relay: Option<Arc<dyn PoolRefreshRelay>> = relay_handle
                            .clone()
                            .map(|relay| relay as Arc<dyn PoolRefreshRelay>);
                        let result = match app.try_state() {
                            Some(app_state) => prepare_local(
                                app_state.inner(),
                                &pool,
                                payload,
                                &abort,
                                master_public,
                                &slave_secret,
                                relay,
                            ),
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
                                prepared = Some(local.prepared);
                                local_job = Some(local.job_id);
                                watch = Some(local.watch);
                                let _ = tx.send(ClusterMessage::PrepareAck(PrepareAckPayload {
                                    ok: true,
                                    error: None,
                                    missing_vars: Vec::new(),
                                    accepted_concurrent: local.accepted,
                                }));
                                // 预检用渲染后的真实目标：变量缺失在前一步已拦截
                                let target = prepared
                                    .as_ref()
                                    .map(|item| item.url_text.clone())
                                    .unwrap_or_default();
                                let preflight = preflight_target(&target).await;
                                // 预检期间没有读连接，缓冲的 Ping 尚未消费；
                                // 以预检结束时刻为准，避免紧接着的 tick 误判失联
                                last_rx = Instant::now();
                                if abort.load(Ordering::SeqCst) {
                                    // 预检期间被断开：主动告诉主节点，避免它等到超时
                                    let _ = tx.send(ClusterMessage::Abort(AbortPayload {
                                        reason: Some("从节点在准备阶段中止".to_string()),
                                    }));
                                    break;
                                }
                                if preflight.ok {
                                    set_status(&state, "ready", None);
                                    let _ = tx.send(ClusterMessage::Preflight(PreflightPayload {
                                        ok: true,
                                        error: None,
                                    }));
                                } else {
                                    if let Some(job) = local_job.take() {
                                        finish_local(app.as_ref(), job);
                                    }
                                    set_status(
                                        &state,
                                        "rejected",
                                        preflight.error.clone(),
                                    );
                                    let _ = tx.send(ClusterMessage::Preflight(PreflightPayload {
                                        ok: false,
                                        error: preflight.error,
                                    }));
                                    break;
                                }
                            }
                            Err(failure) => {
                                {
                                    let mut guard = state.lock();
                                    guard.missing_vars = failure.missing.clone();
                                }
                                set_status(&state, "rejected", Some(failure.error.clone()));
                                let _ = tx.send(ClusterMessage::PrepareAck(PrepareAckPayload {
                                    ok: false,
                                    error: Some(failure.error),
                                    missing_vars: failure.missing,
                                    accepted_concurrent: 0,
                                }));
                                break;
                            }
                        }
                    }
                    ClusterMessage::Start(StartPayload { at_ms, offset_ms }) => {
                        let (Some(item), Some(job), Some(run_watch)) =
                            (prepared.take(), local_job.take(), watch.take())
                        else {
                            let _ = tx.send(ClusterMessage::Error(ErrorPayload {
                                message: "尚未收到可执行的配置".to_string(),
                            }));
                            break;
                        };
                        // 主节点时钟 → 本机时钟：偏移由时钟探测得到，正值表示本机更快
                        let local_at_ms = at_ms.saturating_add(offset_ms.round() as i64);
                        {
                            let mut guard = state.lock();
                            guard.status = "running".to_string();
                            guard.start_at_ms = Some(local_at_ms);
                            guard.elapsed_sec = 0;
                            guard.qps = 0.0;
                            guard.clock_offset_ms = offset_ms;
                        }
                        emit_state(app.as_ref(), &state);
                        let end = run_session(
                            app.as_ref(),
                            &tx,
                            &state,
                            &abort,
                            &notify,
                            &mut read,
                            relay_handle.clone(),
                            item,
                            job,
                            run_watch,
                            local_at_ms,
                        )
                        .await;
                        match end {
                            RunEnd::Done => set_status(&state, "done", None),
                            RunEnd::Aborted(reason) => set_status(&state, "left", Some(reason)),
                            RunEnd::Lost(reason) => set_status(&state, "lost", Some(reason)),
                        }
                        emit_state(app.as_ref(), &state);
                        break;
                    }
                    ClusterMessage::Abort(AbortPayload { reason }) => {
                        abort.store(true, Ordering::SeqCst);
                        set_status(
                            &state,
                            "left",
                            Some(
                                reason
                                    .filter(|text| !text.trim().is_empty())
                                    .unwrap_or_else(|| "主节点中止了任务".to_string()),
                            ),
                        );
                        break;
                    }
                    ClusterMessage::Ping(PingPayload { at_ms, .. }) => {
                        let _ = tx.send(ClusterMessage::Pong(PongPayload {
                            at_ms,
                            stats: None,
                        }));
                    }
                    ClusterMessage::Pong(payload) => {
                        if payload.stats.is_some() {
                            state.lock().engine_stats = payload.stats;
                        }
                    }
                    ClusterMessage::Error(ErrorPayload { message }) => {
                        abort.store(true, Ordering::SeqCst);
                        set_status(&state, "rejected", Some(message));
                        break;
                    }
                    ClusterMessage::PoolRefreshResult(payload) => {
                        if let Some(relay) = &relay_handle {
                            relay.complete(payload);
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    if let Some(job) = local_job {
        finish_local(app.as_ref(), job);
    }
    // 会话结束：让仍在等待刷新结果的 VU 立即失败，而不是等超时
    if let Some(relay) = &relay_handle {
        relay.fail_all();
    }
    // 拒绝 / 失联前的最后一条消息不能被随后的 close 抢掉：留出发送窗口
    tokio::time::sleep(Duration::from_millis(200)).await;
    drop(tx);
    if tokio::time::timeout(Duration::from_secs(2), &mut writer)
        .await
        .is_err()
    {
        writer.abort();
    }
}

/// 合并主节点下发的环境变量包
///
/// - `master_wins`（默认）：主节点值覆盖本机同名变量；
/// - `local_wins`：本机已有值保持不变，只用主节点值补缺失项。
fn merge_bundle_vars(
    vars: &mut HashMap<String, String>,
    bundle: &serde_json::Value,
    policy: &str,
) {
    let local_wins = policy.eq_ignore_ascii_case("local_wins");
    for field in ["vars", "secrets"] {
        if let Some(map) = bundle.get(field).and_then(|item| item.as_object()) {
            for (key, item) in map {
                if let Some(text) = item.as_str() {
                    if local_wins && vars.contains_key(key) {
                        continue;
                    }
                    vars.insert(key.clone(), text.to_string());
                }
            }
        }
    }
}

/// 本地准备：解密主节点环境包 → 核对变量 → 用本机引擎准备 → 登记本地任务
fn prepare_local(
    state: &AppState,
    pool: &DbPool,
    payload: PreparePayload,
    abort: &Arc<AtomicBool>,
    master_public: Option<[u8; 32]>,
    slave_secret: &x25519_dalek::StaticSecret,
    relay: Option<Arc<dyn PoolRefreshRelay>>,
) -> Result<LocalPrepare, PrepareFailure> {
    if state.capacity.is_running() {
        return Err(prepare_failure(
            "本机能力检测正在运行，检测期间不能加入分布式压测",
        ));
    }
    let raw_config = payload.config.clone();
    let classified = crate::util::required_vars_in_json(&raw_config);
    let config: StressConfig = serde_json::from_value(payload.config)
        .map_err(|error| prepare_failure(format!("节点无法解析下发的配置：{error}")))?;
    let env_name = payload
        .env_name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty());
    // 本地同名环境只作为兜底：主节点 EnvBundle 才是权威来源
    let env_id = match env_name {
        Some(name) => {
            let conn = pool
                .get()
                .map_err(|error| prepare_failure(format!("节点本地数据库不可用：{error}")))?;
            Some(query_env_id(&conn, name)?)
        }
        None => None,
    };
    let mut vars = match env_id {
        Some(id) => {
            let conn = pool
                .get()
                .map_err(|error| prepare_failure(format!("节点本地数据库不可用：{error}")))?;
            crate::commands::http::load_vars(&conn, id)
                .map_err(|error| prepare_failure(format!("读取环境变量失败：{error}")))?
        }
        None => HashMap::new(),
    };

    // 1) 解密主节点环境包（含主节点独有变量与 secret）
    if let Some(bundle) = &payload.env_bundle {
        let peer = master_public.ok_or_else(|| {
            prepare_failure("缺少主节点公钥，无法解密环境变量包；请升级主从版本后重试")
        })?;
        let plaintext = super::env_bundle::decrypt_bundle(slave_secret, &peer, bundle)
            .map_err(prepare_failure)?;
        let value: serde_json::Value = serde_json::from_slice(&plaintext)
            .map_err(|error| prepare_failure(format!("环境变量包格式错误：{error}")))?;
        merge_bundle_vars(&mut vars, &value, payload.var_policy.as_str());
    }

    // 2) OS 环境变量：只注入白名单内的变量
    let allow = state.settings().stress_os_env_allow;
    let mut os_missing: Vec<String> = Vec::new();
    for name in &classified.os {
        if !crate::util::os_var_allowed(&allow, name) {
            os_missing.push(name.clone());
            continue;
        }
        match std::env::var(name) {
            Ok(value) => {
                vars.insert(name.clone(), value);
            }
            Err(_) => os_missing.push(name.clone()),
        }
    }

    // 3) 只有真正需要用户提供且仍缺失的变量才拦截
    let mut missing: Vec<String> = payload
        .required_vars
        .iter()
        .filter(|name| !vars.contains_key(*name))
        .cloned()
        .collect();
    for name in os_missing {
        if !missing.contains(&name) {
            missing.push(format!("os:{name}"));
        }
    }
    if !missing.is_empty() {
        let detail = match env_name {
            Some(name) => format!("本机环境「{name}」缺少变量"),
            None => "主节点下发的环境变量不完整".to_string(),
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
    // 解密后的变量直接作为运行期变量，后续渲染不再依赖本机库
    config.vars = vars;
    // 参数池由主节点切片下发，从节点按池名取用
    if !payload.pool_slices.is_empty() {
        config.pool_slices = payload.pool_slices;
    }
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

    let prepared = prepare_stress_with_relay(state, config, abort.clone(), relay)
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

/// 执行引擎并同时处理主节点消息：只在中止 / 失联时提前返回
#[allow(clippy::too_many_arguments)]
async fn run_session<S>(
    app: &dyn ClusterApp,
    tx: &mpsc::UnboundedSender<ClusterMessage>,
    state: &Arc<Mutex<SlaveState>>,
    abort: &Arc<AtomicBool>,
    notify: &Notify,
    read: &mut S,
    relay: Option<Arc<WsPoolRefreshRelay>>,
    prepared: PreparedStress,
    job_id: u64,
    watch: Arc<AtomicBool>,
    at_ms: i64,
) -> RunEnd
where
    S: futures_util::Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    let state_for_sink = state.clone();
    let tx_for_sink = tx.clone();
    let sink: EngineSampleSink = Arc::new(move |sample| {
        {
            let mut guard = state_for_sink.lock();
            guard.qps = sample.qps;
            let stats = guard.engine_stats.get_or_insert_with(EngineStats::default);
            stats.elapsed_sec = sample.elapsed_sec;
            stats.concurrent = sample.concurrent;
            stats.qps = sample.qps;
            stats.total = stats
                .total
                .saturating_add(sample.success_count)
                .saturating_add(sample.error_count);
            stats.success = stats.success.saturating_add(sample.success_count);
            stats.errors = stats.errors.saturating_add(sample.error_count);
            stats.bytes = stats.bytes.saturating_add(sample.bytes);
        }
        let _ = tx_for_sink.send(ClusterMessage::Sample(sample));
    });
    let engine = run_engine(
        prepared,
        job_id,
        None,
        Some(&watch),
        EngineOptions {
            start_at_ms: Some(at_ms),
            sink: Some(sink),
        },
    );
    tokio::pin!(engine);

    let mut ticker = tokio::time::interval(Duration::from_secs(1));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    ticker.tick().await;
    let mut last_rx = Instant::now();

    let (end, outcome) = loop {
        tokio::select! {
            outcome = &mut engine => break (RunEnd::Done, Some(outcome)),
            _ = notify.notified() => {
                abort.store(true, Ordering::SeqCst);
                watch.store(true, Ordering::SeqCst);
                let _ = tx.send(ClusterMessage::Abort(AbortPayload {
                    reason: Some("从节点主动断开".to_string()),
                }));
                break (RunEnd::Aborted("已断开与主节点的会话".to_string()), None);
            }
            _ = ticker.tick() => {
                if last_rx.elapsed() >= MASTER_SILENCE {
                    abort.store(true, Ordering::SeqCst);
                    watch.store(true, Ordering::SeqCst);
                    break (RunEnd::Lost("与主节点失联超过 6 秒".to_string()), None);
                }
            }
            frame = read.next() => {
                let Some(frame) = frame else {
                    abort.store(true, Ordering::SeqCst);
                    watch.store(true, Ordering::SeqCst);
                    break (RunEnd::Lost("主节点关闭了连接".to_string()), None);
                };
                last_rx = Instant::now();
                match frame {
                    Ok(Message::Text(text)) => {
                        let message: ClusterMessage = match serde_json::from_str(&text) {
                            Ok(message) => message,
                            Err(_) => continue,
                        };
                        match message {
                            ClusterMessage::Ping(PingPayload { at_ms, .. }) => {
                                let stats = state.lock().engine_stats.clone();
                                let _ = tx.send(ClusterMessage::Pong(PongPayload {
                                    at_ms,
                                    stats,
                                }));
                            }
                            ClusterMessage::Abort(AbortPayload { reason }) => {
                                abort.store(true, Ordering::SeqCst);
                                watch.store(true, Ordering::SeqCst);
                                let detail = reason
                                    .filter(|text| !text.trim().is_empty())
                                    .unwrap_or_else(|| "主节点中止了任务".to_string());
                                break (RunEnd::Aborted(detail), None);
                            }
                            ClusterMessage::Error(ErrorPayload { message }) => {
                                abort.store(true, Ordering::SeqCst);
                                watch.store(true, Ordering::SeqCst);
                                break (RunEnd::Lost(format!("主节点报错：{message}")), None);
                            }
                            ClusterMessage::PoolRefreshResult(payload) => {
                                if let Some(relay) = &relay {
                                    relay.complete(payload);
                                }
                            }
                            _ => {}
                        }
                    }
                    Ok(Message::Close(_)) => {
                        abort.store(true, Ordering::SeqCst);
                        watch.store(true, Ordering::SeqCst);
                        break (RunEnd::Lost("主节点关闭了连接".to_string()), None);
                    }
                    Ok(_) => {}
                    Err(error) => {
                        abort.store(true, Ordering::SeqCst);
                        watch.store(true, Ordering::SeqCst);
                        break (RunEnd::Lost(format!("读取主节点消息失败：{error}")), None);
                    }
                }
            }
        }
    };

    finish_local(app, job_id);

    if let Some(outcome) = outcome {
        let elapsed_sec = outcome
            .report
            .end_time
            .unwrap_or(0)
            .saturating_sub(outcome.report.start_time.unwrap_or(0))
            as u64
            / 1000;
        let offset_ms = state.lock().clock_offset_ms;
        {
            let mut guard = state.lock();
            guard.elapsed_sec = elapsed_sec;
            guard.qps = 0.0;
        }
        let payload = done_payload(&outcome, offset_ms);
        let _ = tx.send(ClusterMessage::Done(payload));
    } else {
        state.lock().qps = 0.0;
    }
    end
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
        pools: snapshot
            .get("pools")
            .and_then(|value| value.as_array())
            .cloned()
            .unwrap_or_default(),
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

    #[test]
    fn bundle_vars_follow_master_or_local_policy() {
        let bundle = serde_json::json!({
            "vars": {"token": "master", "only_master": "m"},
            "secrets": {"secret:db": "master-db"}
        });

        let mut vars = HashMap::new();
        vars.insert("token".to_string(), "local".to_string());
        merge_bundle_vars(&mut vars, &bundle, "master_wins");
        assert_eq!(vars.get("token").map(String::as_str), Some("master"));
        assert_eq!(vars.get("only_master").map(String::as_str), Some("m"));
        assert_eq!(vars.get("secret:db").map(String::as_str), Some("master-db"));

        let mut vars = HashMap::new();
        vars.insert("token".to_string(), "local".to_string());
        merge_bundle_vars(&mut vars, &bundle, "LOCAL_WINS");
        assert_eq!(vars.get("token").map(String::as_str), Some("local"));
        // 本机没有的变量仍然从主节点补齐
        assert_eq!(vars.get("only_master").map(String::as_str), Some("m"));
        assert_eq!(vars.get("secret:db").map(String::as_str), Some("master-db"));
    }
}
