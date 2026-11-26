//! 局域网设备在场发现与邀请加入（UDP 17803 + TCP 17803）
//!
//! 与旧的「主节点配对阶段 Beacon（UDP 17802）」不同，这里发现的是**任意一个
//! 装好 AstroMan 的实例**：谁在线、是否空闲、是否允许被邀请。主节点在配对阶段
//! 可以选中一台在线设备发起邀请，对方必须在本机点「接受」才会加入。
//!
//! 安全边界：
//! - 只接受私网 / 回环来源；
//! - Beacon 不含任何令牌、配对码或环境变量；
//! - 邀请令牌 32 字节随机、一次性、绑定主节点任务，120 秒过期；
//! - 每 IP 5 分钟最多 3 次邀请，同设备同一时间只保留 1 条待处理邀请。

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4, TcpListener, TcpStream, UdpSocket};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
use uuid::Uuid;

use crate::services::cluster::client::ClusterSlaveConnectPayload;
use crate::state::AppState;
use crate::util::now_ms;

/// 在场发现 / 邀请端口（UDP 与 TCP 共用端口号，协议不同）
pub const PRESENCE_PORT: u16 = 17_803;
/// Beacon 协议版本
const PRESENCE_VERSION: u32 = 1;
/// Beacon 发送间隔
const BEACON_SEC: u64 = 3;
/// 在场数据有效期：超过该时间未刷新即从列表移除
const PRESENCE_TTL_MS: i64 = 10_000;
/// 邀请令牌有效期
const INVITE_TTL_MS: i64 = 120_000;
/// 单条邀请报文上限
const MAX_INVITE_BYTES: usize = 16 * 1024;
/// 每 IP 邀请限速窗口
const INVITE_WINDOW_MS: i64 = 5 * 60_000;
/// 每 IP 窗口内允许的邀请次数
const INVITE_PER_WINDOW: usize = 3;
/// 多播地址：跨网卡时也能收到（与 17802 区分，避免两套协议互相污染）
const MULTICAST_ADDR: Ipv4Addr = Ipv4Addr::new(239, 255, 77, 89);
/// 事件：收到邀请，前端弹窗让用户选择
pub const EVENT_INVITE: &str = "cluster:invite";
/// 事件：邀请处理结果（接受 / 拒绝）
pub const EVENT_INVITE_RESULT: &str = "cluster:invite-result";

/* ---------- 协议结构 ---------- */

/// 在场广播（无私密信息）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerBeacon {
    pub v: u32,
    #[serde(rename = "type")]
    pub kind: String,
    pub app: String,
    pub device_id: String,
    pub name: String,
    pub host: String,
    /// 邀请监听端口（TCP）
    pub invite_port: u16,
    pub version: String,
    pub os: String,
    pub arch: String,
    pub cores: u32,
    pub mem_gb: f64,
    pub budget: u32,
    /// idle / busy
    pub state: String,
    /// 当前是否还能接受邀请
    pub joinable: bool,
    pub allow_invite: bool,
    pub sent_at: i64,
}

/// 前端展示的在线设备
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredPeer {
    pub device_id: String,
    pub name: String,
    pub host: String,
    pub invite_port: u16,
    pub addr: String,
    pub version: String,
    pub os: String,
    pub arch: String,
    pub cores: u32,
    pub mem_gb: f64,
    pub budget: u32,
    pub state: String,
    pub joinable: bool,
    pub allow_invite: bool,
    pub last_seen_ms: i64,
    pub latency_ms: i64,
    pub is_self: bool,
}

/// 主节点发出的邀请（含在任务快照里，前端直接渲染）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MasterInvite {
    pub request_id: String,
    pub job_id: i64,
    pub peer_device_id: Option<String>,
    pub peer_name: String,
    pub peer_addr: String,
    /// pending / accepted / expired / failed
    pub status: String,
    pub message: Option<String>,
    pub created_at: i64,
    pub expires_at: i64,
}

/// 线缆上的邀请请求
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InviteRequest {
    v: u32,
    #[serde(rename = "type")]
    kind: String,
    request_id: String,
    master_device_id: String,
    master_name: String,
    master_addr: String,
    job_name: String,
    expected_concurrency: u32,
    expires_at: i64,
    token: String,
}

/// 线缆上的邀请应答
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InviteReply {
    ok: bool,
    status: String,
    #[serde(default)]
    request_id: Option<String>,
    #[serde(default)]
    message: Option<String>,
    /// 被邀请方设备名：主节点用它展示「已通知哪台机器」
    #[serde(default)]
    peer_name: Option<String>,
}

/// 前端弹窗内容
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InviteNotice {
    pub request_id: String,
    pub master_name: String,
    pub master_addr: String,
    pub job_name: String,
    pub expected_concurrency: u32,
    pub expires_at: i64,
    pub received_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct InviteResult {
    request_id: String,
    accepted: bool,
    message: String,
}

/// 在场状态（前端轮询）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PresenceStatus {
    pub running: bool,
    pub device_id: String,
    pub device_name: String,
    pub discoverable: bool,
    pub invite_policy: String,
    pub udp_port: u16,
    pub invite_port: u16,
    pub peer_count: usize,
    pub pending_count: usize,
    pub state: String,
    pub error: Option<String>,
}

struct PendingInvite {
    notice: InviteNotice,
    token: String,
    master_device_id: String,
    master_addr: String,
}

struct PresenceState {
    running: bool,
    device_id: String,
    device_name: String,
    discoverable: bool,
    /// ask / auto / off
    invite_policy: String,
    peers: HashMap<String, DiscoveredPeer>,
    pending: HashMap<String, PendingInvite>,
    invite_hits: HashMap<String, Vec<i64>>,
    error: Option<String>,
    started_at: i64,
    stop: Option<Arc<AtomicBool>>,
}

impl Default for PresenceState {
    fn default() -> Self {
        Self {
            running: false,
            device_id: String::new(),
            device_name: String::new(),
            discoverable: true,
            invite_policy: "ask".to_string(),
            peers: HashMap::new(),
            pending: HashMap::new(),
            invite_hits: HashMap::new(),
            error: None,
            started_at: 0,
            stop: None,
        }
    }
}

static STATE: OnceLock<Mutex<PresenceState>> = OnceLock::new();
static APP: OnceLock<AppHandle> = OnceLock::new();
static TOKENS: OnceLock<Mutex<HashMap<String, InviteToken>>> = OnceLock::new();
static MASTER_INVITES: OnceLock<Mutex<HashMap<String, MasterInvite>>> = OnceLock::new();

struct InviteToken {
    job_id: i64,
    request_id: String,
    expires_at: i64,
}

fn presence_state() -> &'static Mutex<PresenceState> {
    STATE.get_or_init(|| Mutex::new(PresenceState::default()))
}

fn tokens() -> &'static Mutex<HashMap<String, InviteToken>> {
    TOKENS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn master_invites() -> &'static Mutex<HashMap<String, MasterInvite>> {
    MASTER_INVITES.get_or_init(|| Mutex::new(HashMap::new()))
}

/* ---------- 身份与启动 ---------- */

/// 设备 ID：存在数据目录，保证重启后同机身份不变
fn device_id_for(data_dir: &Path) -> String {
    let file = data_dir.join("cluster_device_id");
    if let Ok(text) = std::fs::read_to_string(&file) {
        let value = text.trim();
        if !value.is_empty() {
            return value.to_string();
        }
    }
    let id = Uuid::new_v4().simple().to_string();
    let _ = std::fs::create_dir_all(data_dir);
    if let Err(error) = std::fs::write(&file, &id) {
        log::warn!("写入设备标识失败：{error}");
    }
    id
}

fn default_device_name() -> String {
    sysinfo::System::host_name()
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "AstroMan 节点".to_string())
}

/// 归一化邀请策略：ask（默认）/ auto / off
pub fn normalize_policy(raw: &str) -> String {
    match raw.trim().to_ascii_lowercase().as_str() {
        "auto" => "auto".to_string(),
        "off" => "off".to_string(),
        _ => "ask".to_string(),
    }
}

/// 启动在场发现与邀请监听；重复调用只更新身份与开关
pub fn start(
    app: &AppHandle,
    data_dir: &Path,
    device_name: &str,
    discoverable: bool,
    invite_policy: &str,
) -> Result<(), String> {
    let _ = APP.set(app.clone());
    let device_id = device_id_for(data_dir);
    let name = device_name.trim();
    let name = if name.is_empty() {
        default_device_name()
    } else {
        name.to_string()
    };
    let policy = normalize_policy(invite_policy);

    {
        let mut current = presence_state().lock();
        if current.running {
            current.device_name = name;
            current.discoverable = discoverable;
            current.invite_policy = policy;
            return Ok(());
        }
    }

    // 先绑定端口：失败时不要把 running 置 true，设置页才能明确报错
    let udp = match UdpSocket::bind(("0.0.0.0", PRESENCE_PORT)) {
        Ok(socket) => socket,
        Err(error) => {
            let message = format!("监听 UDP {PRESENCE_PORT} 失败：{error}");
            presence_state().lock().error = Some(message.clone());
            return Err(message);
        }
    };
    udp.set_read_timeout(Some(Duration::from_millis(500)))
        .map_err(|error| format!("设置在场端口超时失败：{error}"))?;
    // 多网卡场景：加入多播组 + 同时发受限广播，尽量覆盖非主网卡
    let _ = udp.join_multicast_v4(&MULTICAST_ADDR, &Ipv4Addr::UNSPECIFIED);
    let tcp = match TcpListener::bind(("0.0.0.0", PRESENCE_PORT)) {
        Ok(listener) => listener,
        Err(error) => {
            let message = format!("监听 TCP {PRESENCE_PORT} 失败：{error}");
            presence_state().lock().error = Some(message.clone());
            return Err(message);
        }
    };
    tcp.set_nonblocking(true)
        .map_err(|error| format!("设置邀请端口非阻塞失败：{error}"))?;

    let stop = Arc::new(AtomicBool::new(false));
    {
        let mut current = presence_state().lock();
        current.running = true;
        current.device_id = device_id.clone();
        current.device_name = name;
        current.discoverable = discoverable;
        current.invite_policy = policy;
        current.error = None;
        current.peers.clear();
        current.pending.clear();
        current.invite_hits.clear();
        current.started_at = now_ms();
        current.stop = Some(stop.clone());
    }

    spawn_udp_listener(app.clone(), udp, stop.clone());
    spawn_beacon(app.clone(), stop.clone());
    spawn_invite_listener(app.clone(), tcp, stop.clone());
    Ok(())
}

/// 停止在场发现：关闭 UDP/TCP 监听并清空在线设备
pub fn stop() {
    let stop = {
        let mut current = presence_state().lock();
        current.running = false;
        current.peers.clear();
        current.pending.clear();
        current.invite_hits.clear();
        current.stop.take()
    };
    if let Some(flag) = stop {
        flag.store(true, Ordering::SeqCst);
    }
}

/// 更新身份与可见性（设置页保存后调用）
pub fn update_identity(device_name: &str, discoverable: bool, invite_policy: &str) {
    let mut current = presence_state().lock();
    if !device_name.trim().is_empty() {
        current.device_name = device_name.trim().to_string();
    }
    current.discoverable = discoverable;
    current.invite_policy = normalize_policy(invite_policy);
}

pub fn status() -> PresenceStatus {
    prune_peers();
    let current = presence_state().lock();
    let busy = APP.get().map(|app| busy_state(app).1).unwrap_or(false);
    PresenceStatus {
        running: current.running,
        device_id: current.device_id.clone(),
        device_name: current.device_name.clone(),
        discoverable: current.discoverable,
        invite_policy: current.invite_policy.clone(),
        udp_port: PRESENCE_PORT,
        invite_port: PRESENCE_PORT,
        peer_count: current.peers.len(),
        pending_count: current.pending.len(),
        state: if busy {
            "busy".to_string()
        } else {
            "idle".to_string()
        },
        error: current.error.clone(),
    }
}

pub fn list_peers() -> Vec<DiscoveredPeer> {
    prune_peers();
    let current = presence_state().lock();
    let mut items: Vec<DiscoveredPeer> = current.peers.values().cloned().collect();
    items.sort_by(|left, right| {
        right
            .joinable
            .cmp(&left.joinable)
            .then_with(|| right.last_seen_ms.cmp(&left.last_seen_ms))
    });
    items
}

/* ---------- UDP 在场发现 ---------- */

fn spawn_udp_listener(app: AppHandle, socket: UdpSocket, stop: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        let mut buffer = [0u8; 4096];
        let _ = app;
        while !stop.load(Ordering::SeqCst) {
            match socket.recv_from(&mut buffer) {
                Ok((size, peer)) => {
                    let Ok(beacon) = serde_json::from_slice::<PeerBeacon>(&buffer[..size]) else {
                        continue;
                    };
                    if beacon.app != "AstroMan"
                        || beacon.v != PRESENCE_VERSION
                        || beacon.kind != "peer"
                    {
                        continue;
                    }
                    if !peer.ip().is_private_or_local() {
                        continue;
                    }
                    ingest(beacon, peer);
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) => {}
                // Windows 会把 ICMP port unreachable 报告成下一次 recv 的 ConnectionReset，
                // macOS/Linux 也可能返回 EINTR；这些都是瞬时事件，不能当成致命错误退出，
                // 否则设备发现线程会永久停摆，用户只能重启应用
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::ConnectionReset
                            | std::io::ErrorKind::ConnectionAborted
                            | std::io::ErrorKind::Interrupted
                    ) => {}
                Err(error) => {
                    presence_state().lock().error = Some(format!("读取在场数据失败：{error}"));
                    break;
                }
            }
            prune_peers();
        }
    });
}

trait PrivateIp {
    fn is_private_or_local(&self) -> bool;
}

impl PrivateIp for IpAddr {
    fn is_private_or_local(&self) -> bool {
        if let IpAddr::V4(v4) = self {
            v4.is_private() || v4.is_loopback() || v4.is_link_local()
        } else {
            matches!(self, IpAddr::V6(v6) if v6.is_loopback())
        }
    }
}

fn ingest(beacon: PeerBeacon, peer: SocketAddr) {
    let now = now_ms();
    let self_id = presence_state().lock().device_id.clone();
    let is_self = !self_id.is_empty() && beacon.device_id == self_id;
    let mut current = presence_state().lock();
    let entry = current
        .peers
        .entry(beacon.device_id.clone())
        .or_insert_with(|| DiscoveredPeer {
            device_id: beacon.device_id.clone(),
            name: beacon.name.clone(),
            host: beacon.host.clone(),
            invite_port: beacon.invite_port,
            addr: format!("{}:{}", beacon.host, beacon.invite_port),
            version: beacon.version.clone(),
            os: beacon.os.clone(),
            arch: beacon.arch.clone(),
            cores: beacon.cores,
            mem_gb: beacon.mem_gb,
            budget: beacon.budget,
            state: beacon.state.clone(),
            joinable: beacon.joinable,
            allow_invite: beacon.allow_invite,
            last_seen_ms: now,
            latency_ms: 0,
            is_self,
        });
    entry.latency_ms = (now - entry.last_seen_ms).max(0);
    entry.last_seen_ms = now;
    entry.name = beacon.name;
    entry.host = beacon.host.clone();
    entry.invite_port = beacon.invite_port;
    entry.addr = format!("{}:{}", beacon.host, beacon.invite_port);
    entry.version = beacon.version;
    entry.os = beacon.os;
    entry.arch = beacon.arch;
    entry.cores = beacon.cores;
    entry.mem_gb = beacon.mem_gb;
    entry.budget = beacon.budget;
    entry.state = beacon.state;
    entry.joinable = beacon.joinable;
    entry.allow_invite = beacon.allow_invite;
    entry.is_self = is_self;
    // 来源地址与广播声明不一致时以真实来源为准，避免伪造 host 把邀请引到别处
    if let IpAddr::V4(v4) = peer.ip() {
        let claimed = entry.host.parse::<Ipv4Addr>().ok();
        if claimed != Some(v4) {
            entry.host = v4.to_string();
            entry.addr = format!("{}:{}", v4, entry.invite_port);
        }
    }
}

fn prune_peers() {
    let now = now_ms();
    let mut current = presence_state().lock();
    current
        .peers
        .retain(|_, item| now - item.last_seen_ms <= PRESENCE_TTL_MS);
}

fn spawn_beacon(app: AppHandle, stop: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        while !stop.load(Ordering::SeqCst) {
            if let Some(beacon) = build_beacon(&app) {
                broadcast(&beacon);
            }
            for _ in 0..(BEACON_SEC * 10) {
                if stop.load(Ordering::SeqCst) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    });
}

fn build_beacon(app: &AppHandle) -> Option<PeerBeacon> {
    let current = presence_state().lock();
    if !current.discoverable {
        return None;
    }
    let (state_label, _) = busy_state(app);
    let budget = app.try_state::<AppState>().map(|value| value.budget());
    let host = primary_host()?;
    Some(PeerBeacon {
        v: PRESENCE_VERSION,
        kind: "peer".to_string(),
        app: "AstroMan".to_string(),
        device_id: current.device_id.clone(),
        name: current.device_name.clone(),
        host,
        invite_port: PRESENCE_PORT,
        version: env!("CARGO_PKG_VERSION").to_string(),
        os: sysinfo::System::name().unwrap_or_default(),
        arch: std::env::consts::ARCH.to_string(),
        cores: budget
            .as_ref()
            .map(|item| item.sources.cores)
            .unwrap_or_else(|| {
                std::thread::available_parallelism()
                    .map(|value| value.get() as u32)
                    .unwrap_or(4)
            }),
        mem_gb: budget
            .as_ref()
            .map(|item| item.sources.mem_gb)
            .unwrap_or_else(|| {
                crate::state::total_memory_bytes() as f64 / 1024.0 / 1024.0 / 1024.0
            }),
        budget: budget.as_ref().map(|item| item.machine_limit).unwrap_or(0),
        state: state_label.clone(),
        joinable: state_label == "idle" && current.invite_policy != "off",
        allow_invite: current.invite_policy != "off",
        sent_at: now_ms(),
    })
}

fn broadcast(beacon: &PeerBeacon) {
    let Ok(payload) = serde_json::to_vec(beacon) else {
        return;
    };
    let Ok(socket) = UdpSocket::bind(("0.0.0.0", 0)) else {
        return;
    };
    let _ = socket.set_broadcast(true);
    let _ = socket.send_to(
        &payload,
        SocketAddr::V4(SocketAddrV4::new(MULTICAST_ADDR, PRESENCE_PORT)),
    );
    // 受限广播：由系统选择出口网卡，多网卡时比定向广播更容易覆盖到对端
    let _ = socket.send_to(
        &payload,
        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::BROADCAST, PRESENCE_PORT)),
    );
    for target in broadcast_targets(&beacon.host) {
        let _ = socket.send_to(&payload, target);
    }
}

/// 各网卡的定向广播：当前取主地址所在 /24 + 多播（多网卡仍可从多播收到）
fn broadcast_targets(host: &str) -> Vec<SocketAddr> {
    let mut targets = Vec::new();
    if let Ok(ip) = host.parse::<Ipv4Addr>() {
        let octets = ip.octets();
        targets.push(SocketAddr::V4(SocketAddrV4::new(
            Ipv4Addr::new(octets[0], octets[1], octets[2], 255),
            PRESENCE_PORT,
        )));
    }
    targets
}

fn primary_host() -> Option<String> {
    super::local_addresses()
        .into_iter()
        .find(|item| item != "127.0.0.1")
}

/* ---------- 忙闲判断 ---------- */

/// 返回（对外状态, 是否忙）。从节点会话或任一主节点任务进行中都算忙。
fn busy_state(app: &AppHandle) -> (String, bool) {
    let Some(state) = app.try_state::<AppState>() else {
        return ("idle".to_string(), false);
    };
    let slave = state.cluster_slave.snapshot();
    if matches!(
        slave.status.as_str(),
        "connecting" | "pairing" | "pending" | "preparing" | "ready" | "running"
    ) {
        return ("running".to_string(), true);
    }
    if state.cluster.snapshots().iter().any(|item| {
        matches!(
            item.status.as_str(),
            "pairing" | "preparing" | "ready" | "running"
        )
    }) {
        return ("busy".to_string(), true);
    }
    ("idle".to_string(), false)
}

/* ---------- TCP 邀请监听 ---------- */

fn spawn_invite_listener(app: AppHandle, listener: TcpListener, stop: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        while !stop.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((stream, peer)) => {
                    let app = app.clone();
                    let stop = stop.clone();
                    std::thread::spawn(move || handle_invite(stream, peer, app, stop));
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(200));
                }
                // 对方在 accept 前主动断开（Windows 常见 ConnectionAborted）属于正常现象：
                // 只短暂退避后继续监听，既不影响已有连接也不该把错误常驻在状态里
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::ConnectionReset
                            | std::io::ErrorKind::ConnectionAborted
                            | std::io::ErrorKind::Interrupted
                    ) =>
                {
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(error) => {
                    presence_state().lock().error = Some(format!("接受邀请连接失败：{error}"));
                    std::thread::sleep(Duration::from_millis(500));
                }
            }
        }
    });
}

fn handle_invite(stream: TcpStream, peer: SocketAddr, app: AppHandle, stop: Arc<AtomicBool>) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
    if !peer.ip().is_private_or_local() {
        let _ = reply(&stream, false, "rejected", None, Some("只允许局域网邀请"));
        return;
    }
    if stop.load(Ordering::SeqCst) {
        return;
    }
    let mut reader = BufReader::new(match stream.try_clone() {
        Ok(clone) => clone,
        Err(error) => {
            log::warn!("复制邀请连接失败：{error}");
            return;
        }
    });
    let mut line = String::new();
    if reader
        .by_ref()
        .take(MAX_INVITE_BYTES as u64)
        .read_line(&mut line)
        .is_err()
    {
        let _ = reply(&stream, false, "rejected", None, Some("读取邀请失败"));
        return;
    }
    let request: InviteRequest = match serde_json::from_str(line.trim()) {
        Ok(value) => value,
        Err(_) => {
            let _ = reply(&stream, false, "rejected", None, Some("邀请格式不正确"));
            return;
        }
    };
    if request.v != PRESENCE_VERSION || request.kind != "invite" {
        let _ = reply(&stream, false, "rejected", None, Some("邀请协议版本不一致"));
        return;
    }
    if request.expires_at <= now_ms() {
        let _ = reply(
            &stream,
            false,
            "expired",
            Some(request.request_id),
            Some("邀请已过期"),
        );
        return;
    }

    let ip = peer.ip().to_string();
    let now = now_ms();
    let policy = {
        let mut current = presence_state().lock();
        if !current.running {
            let _ = reply(&stream, false, "rejected", None, Some("本机未开启设备发现"));
            return;
        }
        if current.invite_policy == "off" || !current.discoverable {
            let _ = reply(&stream, false, "rejected", None, Some("本机未开启接收邀请"));
            return;
        }
        // 限速：同一 IP 5 分钟最多 3 次
        current.invite_hits.retain(|_, hits| {
            hits.retain(|at| now - *at <= INVITE_WINDOW_MS);
            !hits.is_empty()
        });
        let hits = current.invite_hits.get(&ip).map(Vec::len).unwrap_or(0);
        if hits >= INVITE_PER_WINDOW {
            let _ = reply(
                &stream,
                false,
                "rejected",
                None,
                Some("邀请过于频繁，请稍后再试"),
            );
            return;
        }
        if !current.pending.is_empty() {
            let _ = reply(&stream, false, "busy", None, Some("本机还有未处理的邀请"));
            return;
        }
        // 只接受「在线设备列表里确实见过」的主节点：防止局域网内任意程序
        // 直接发一条邀请就把弹窗推到用户面前（钓鱼 / 骚扰）
        if !current
            .peers
            .values()
            .any(|item| item.device_id == request.master_device_id)
        {
            let _ = reply(
                &stream,
                false,
                "rejected",
                None,
                Some("邀请来源不在在线设备列表中"),
            );
            return;
        }
        if current
            .pending
            .values()
            .any(|item| item.master_device_id == request.master_device_id)
        {
            let _ = reply(
                &stream,
                false,
                "busy",
                None,
                Some("该主节点的邀请已在等待处理"),
            );
            return;
        }
        current.invite_hits.entry(ip.clone()).or_default().push(now);
        current.invite_policy.clone()
    };

    let (state_label, busy) = busy_state(&app);
    if busy {
        let _ = reply(
            &stream,
            false,
            "busy",
            None,
            Some(&format!(
                "本机正在执行压测任务（{state_label}），暂不能加入"
            )),
        );
        return;
    }

    // 回连地址不信任报文里的 host：以 TCP 真实来源 IP 为准，避免被诱导连到第三台机器
    let master_port = request
        .master_addr
        .rsplit_once(':')
        .and_then(|(_, port)| port.parse::<u16>().ok())
        .unwrap_or(super::protocol::DEFAULT_PORT);
    let master_addr = SocketAddr::new(peer.ip(), master_port).to_string();
    let notice = InviteNotice {
        request_id: request.request_id.clone(),
        master_name: request.master_name.clone(),
        master_addr: master_addr.clone(),
        job_name: request.job_name.clone(),
        expected_concurrency: request.expected_concurrency,
        expires_at: request.expires_at,
        received_at: now,
    };
    {
        let mut current = presence_state().lock();
        current.pending.insert(
            request.request_id.clone(),
            PendingInvite {
                notice: notice.clone(),
                token: request.token.clone(),
                master_device_id: request.master_device_id.clone(),
                master_addr,
            },
        );
    }
    let _ = reply_with_name(
        &stream,
        true,
        "pending",
        Some(request.request_id.clone()),
        Some("已通知本机用户，等待确认"),
        Some(&current_device_name()),
    );
    let _ = app.emit(EVENT_INVITE, notice.clone());
    log::info!(
        "收到主节点「{}」({}) 的加入邀请，任务 {}",
        request.master_name,
        request.master_addr,
        request.job_name
    );

    // 自动接受：仍走同一套 connect 流程，只是不再等用户点按钮
    if policy == "auto" {
        let app = app.clone();
        let request_id = request.request_id.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(error) = accept_now(app, &request_id) {
                log::warn!("自动接受邀请失败：{error}");
            }
        });
    }
}

fn reply(
    stream: &TcpStream,
    ok: bool,
    status: &str,
    request_id: Option<String>,
    message: Option<&str>,
) -> std::io::Result<()> {
    reply_with_name(stream, ok, status, request_id, message, None)
}

fn reply_with_name(
    stream: &TcpStream,
    ok: bool,
    status: &str,
    request_id: Option<String>,
    message: Option<&str>,
    peer_name: Option<&str>,
) -> std::io::Result<()> {
    let payload = InviteReply {
        ok,
        status: status.to_string(),
        request_id,
        message: message.map(str::to_string),
        peer_name: peer_name.map(str::to_string),
    };
    let mut text = serde_json::to_string(&payload).unwrap_or_else(|_| "{\"ok\":false}".to_string());
    text.push('\n');
    let mut writer = stream;
    writer.write_all(text.as_bytes())?;
    writer.flush()
}

/* ---------- 被邀请方：接受 / 拒绝 ---------- */

fn accept_now(app: AppHandle, request_id: &str) -> Result<(), String> {
    let pending = {
        let mut current = presence_state().lock();
        current.pending.remove(request_id)
    };
    let Some(pending) = pending else {
        return Err("邀请不存在或已处理".to_string());
    };
    if pending.notice.expires_at <= now_ms() {
        return Err("邀请已过期".to_string());
    }
    let state = app.state::<AppState>();
    state.policy().ensure_lan(Some(
        pending
            .master_addr
            .rsplit_once(':')
            .map(|(host, _)| host)
            .unwrap_or("0.0.0.0"),
    ))?;
    let snapshot = state.cluster_slave.connect(
        state.pool.clone(),
        app.clone(),
        ClusterSlaveConnectPayload {
            master_addr: pending.master_addr.clone(),
            pair_code: String::new(),
            join_mode: Some("invite".to_string()),
            node_name: None,
            invite_token: Some(pending.token.clone()),
        },
    )?;
    let _ = app.emit(
        EVENT_INVITE_RESULT,
        InviteResult {
            request_id: request_id.to_string(),
            accepted: true,
            message: format!("已加入主节点，节点状态：{}", snapshot.status),
        },
    );
    Ok(())
}

/// 前端点击「接受」/「拒绝」
pub fn respond(
    app: &AppHandle,
    request_id: &str,
    accept: bool,
) -> Result<crate::services::cluster::protocol::ClusterSlaveSnapshot, String> {
    let state = app.state::<AppState>();
    if accept {
        accept_now(app.clone(), request_id)?;
    } else {
        let removed = presence_state().lock().pending.remove(request_id);
        if removed.is_none() {
            return Err("邀请不存在或已处理".to_string());
        }
    }
    let _ = app.emit(
        EVENT_INVITE_RESULT,
        InviteResult {
            request_id: request_id.to_string(),
            accepted: accept,
            message: if accept {
                "已接受邀请".to_string()
            } else {
                "已拒绝邀请".to_string()
            },
        },
    );
    Ok(state.cluster_slave.snapshot())
}

/* ---------- 发起方（主节点） ---------- */

/// 向在线设备发起邀请；返回记录用于前端展示「等待对方确认」
pub async fn send_invite(
    state: &AppState,
    job_id: i64,
    host: &str,
    port: u16,
    peer_device_id: Option<String>,
) -> Result<MasterInvite, String> {
    let snapshot = state.cluster.snapshot(job_id)?;
    if snapshot.status != "pairing" {
        return Err("当前任务不在配对阶段，不能邀请新节点".to_string());
    }
    let ip = host.trim();
    if ip.is_empty() {
        return Err("设备地址为空".to_string());
    }
    if ip
        .parse::<IpAddr>()
        .ok()
        .is_some_and(|address| !address.is_private_or_local())
    {
        return Err("只允许邀请局域网设备".to_string());
    }
    state.policy().ensure_lan(Some(ip))?;

    let (device_id, device_name) = {
        let current = presence_state().lock();
        (current.device_id.clone(), current.device_name.clone())
    };
    let master_device_id = if device_id.is_empty() {
        device_id_for(&state.data_dir)
    } else {
        device_id
    };
    let master_name = if device_name.is_empty() {
        default_device_name()
    } else {
        device_name
    };
    let master_addr = snapshot
        .local_addresses
        .iter()
        .find(|item| item.as_str() != "127.0.0.1")
        .cloned()
        .unwrap_or_else(|| {
            snapshot
                .local_addresses
                .first()
                .cloned()
                .unwrap_or_else(|| format!("{ip}:{}", snapshot.port))
        });
    let master_addr = format!(
        "{}:{}",
        master_addr
            .rsplit_once(':')
            .map(|(v, _)| v)
            .unwrap_or(&master_addr),
        snapshot.port
    );

    let request_id = Uuid::new_v4().simple().to_string();
    let token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    let expires_at = now_ms() + INVITE_TTL_MS;
    prune_tokens();
    tokens().lock().insert(
        token.clone(),
        InviteToken {
            job_id,
            request_id: request_id.clone(),
            expires_at,
        },
    );

    let request = InviteRequest {
        v: PRESENCE_VERSION,
        kind: "invite".to_string(),
        request_id: request_id.clone(),
        master_device_id: master_device_id.clone(),
        master_name: master_name.clone(),
        master_addr: master_addr.clone(),
        job_name: snapshot.name.clone(),
        expected_concurrency: snapshot.total_concurrent,
        expires_at,
        token: token.clone(),
    };
    let mut text = serde_json::to_string(&request).map_err(crate::util::err)?;
    text.push('\n');

    let connect = tokio::time::timeout(
        Duration::from_secs(3),
        tokio::net::TcpStream::connect((ip, port)),
    )
    .await
    .map_err(|_| format!("连接设备 {ip}:{port} 超时"))?
    .map_err(|error| format!("连接设备 {ip}:{port} 失败：{error}"))?;
    // 邀请是一次性短连接：直接拆成读写半部，避免额外依赖
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
    let (read_half, mut write_half) = connect.into_split();
    write_half
        .write_all(text.as_bytes())
        .await
        .map_err(|error| format!("发送邀请失败：{error}"))?;
    let mut reader = tokio::io::BufReader::new(read_half);
    let mut line = String::new();
    tokio::time::timeout(Duration::from_secs(5), reader.read_line(&mut line))
        .await
        .map_err(|_| "等待对方确认超时".to_string())?
        .map_err(|error| format!("读取对方应答失败：{error}"))?;
    let reply: InviteReply =
        serde_json::from_str(line.trim()).map_err(|_| "对方应答格式不正确".to_string())?;
    if !reply.ok {
        tokens().lock().remove(&token);
        return Err(reply
            .message
            .unwrap_or_else(|| "对方拒绝了邀请".to_string()));
    }

    let record = MasterInvite {
        request_id: request_id.clone(),
        job_id,
        peer_device_id: peer_device_id.filter(|value| !value.trim().is_empty()),
        peer_name: reply
            .peer_name
            .clone()
            .unwrap_or_else(|| format!("{ip}:{port}")),
        peer_addr: format!("{ip}:{port}"),
        status: "pending".to_string(),
        message: Some("已通知对方，等待本机用户确认".to_string()),
        created_at: now_ms(),
        expires_at,
    };
    prune_master_invites();
    master_invites()
        .lock()
        .insert(request_id.clone(), record.clone());
    Ok(record)
}

/// 主节点校验从节点回传的邀请令牌：一次性，校验通过即作废
pub fn consume_invite_token(token: &str, job_id: i64) -> Result<(), String> {
    let mut guard = tokens().lock();
    let Some(item) = guard.get(token) else {
        return Err("邀请令牌无效或已被使用，请重新邀请".to_string());
    };
    if item.job_id != job_id {
        return Err("邀请令牌不属于当前任务".to_string());
    }
    if item.expires_at <= now_ms() {
        guard.remove(token);
        return Err("邀请已过期（120 秒），请重新邀请".to_string());
    }
    let request_id = item.request_id.clone();
    guard.remove(token);
    drop(guard);
    if let Some(record) = master_invites().lock().get_mut(&request_id) {
        record.status = "accepted".to_string();
        record.message = Some("对方已接受邀请".to_string());
    }
    Ok(())
}

/// 某个任务的邀请记录（按时间倒序），过期记录自动标记
pub fn master_invites_for(job_id: i64) -> Vec<MasterInvite> {
    prune_master_invites();
    let guard = master_invites().lock();
    let mut items: Vec<MasterInvite> = guard
        .values()
        .filter(|item| item.job_id == job_id)
        .cloned()
        .collect();
    items.sort_by_key(|item| std::cmp::Reverse(item.created_at));
    items
}

fn prune_tokens() {
    let now = now_ms();
    tokens().lock().retain(|_, item| item.expires_at > now);
}

fn prune_master_invites() {
    let now = now_ms();
    let mut guard = master_invites().lock();
    for item in guard.values_mut() {
        if item.status == "pending" && item.expires_at <= now {
            item.status = "expired".to_string();
            item.message = Some("对方未在有效期内确认".to_string());
        }
    }
    // 只保留最近 1 小时，避免长期运行后无限增长
    guard.retain(|_, item| now - item.created_at <= 60 * 60_000);
}

/// 设备名称（未启动时也返回一个可展示值）
pub fn current_device_name() -> String {
    let name = presence_state().lock().device_name.clone();
    if name.is_empty() {
        default_device_name()
    } else {
        name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_normalizes_unknown_to_ask() {
        assert_eq!(normalize_policy("auto"), "auto");
        assert_eq!(normalize_policy("OFF"), "off");
        assert_eq!(normalize_policy("whatever"), "ask");
        assert_eq!(normalize_policy(""), "ask");
    }

    #[test]
    fn invite_token_is_single_use_and_job_bound() {
        let token = "test-token-single-use";
        tokens().lock().insert(
            token.to_string(),
            InviteToken {
                job_id: 7,
                request_id: "r1".to_string(),
                expires_at: now_ms() + 10_000,
            },
        );
        assert!(consume_invite_token(token, 8).is_err());
        assert!(consume_invite_token(token, 7).is_ok());
        assert!(consume_invite_token(token, 7).is_err());
    }

    #[test]
    fn expired_invite_token_is_rejected() {
        let token = "test-token-expired";
        tokens().lock().insert(
            token.to_string(),
            InviteToken {
                job_id: 1,
                request_id: "r2".to_string(),
                expires_at: now_ms() - 1,
            },
        );
        assert!(consume_invite_token(token, 1).is_err());
    }

    #[test]
    fn broadcast_target_uses_24_mask() {
        let items = broadcast_targets("192.168.1.20");
        assert_eq!(items[0].to_string(), "192.168.1.255:17803");
    }
}
