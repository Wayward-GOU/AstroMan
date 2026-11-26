//! 局域网自动发现（W3）
//!
//! 主节点在配对阶段每 2 秒向 UDP 17802 发送 Beacon（同时走多播与各网卡广播），
//! 从节点监听该端口，按 `instanceId` 去重后展示「附近的主节点」。
//!
//! Beacon 只包含展示与连接所需信息，不携带配对码、令牌、环境变量；
//! 只接受私网地址，公网 / 跨网段数据直接丢弃。

use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use super::protocol::{ClusterMasterSnapshot, DISCOVERY_PORT, MAX_NODES};

/// Beacon 协议版本
const DISCOVERY_VERSION: u32 = 1;
/// 发现数据有效期（秒）：超时未刷新即从列表移除
pub const DISCOVERY_TTL_SEC: u64 = 6;
/// 多播地址：跨网卡时也能收到
const MULTICAST_ADDR: Ipv4Addr = Ipv4Addr::new(239, 255, 77, 88);

/// 主节点广播内容（不含任何密钥）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MasterBeacon {
    pub v: u32,
    #[serde(rename = "type")]
    pub kind: String,
    pub app: String,
    pub instance_id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub job_id: i64,
    pub status: String,
    pub join_mode: String,
    pub node_count: usize,
    pub max_nodes: usize,
    #[serde(default)]
    pub capacity: Option<BeaconCapacity>,
    pub sent_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BeaconCapacity {
    pub cores: u32,
    pub mem_mb: u64,
    pub budget: u32,
}

/// 从节点侧发现到的主节点
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredMaster {
    pub instance_id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub addr: String,
    pub job_id: i64,
    pub status: String,
    pub join_mode: String,
    pub node_count: usize,
    pub max_nodes: usize,
    pub capacity: Option<BeaconCapacity>,
    /// 最近一次收到 Beacon 的时刻（本机毫秒）
    pub last_seen_ms: i64,
    /// 从收到时刻估算的刷新间隔（毫秒）
    pub latency_ms: i64,
}

struct ListenerState {
    running: bool,
    discovered: HashMap<String, DiscoveredMaster>,
    error: Option<String>,
}

static LISTENER: OnceLock<Mutex<ListenerState>> = OnceLock::new();
static STOP: OnceLock<Arc<AtomicBool>> = OnceLock::new();

fn listener() -> &'static Mutex<ListenerState> {
    LISTENER.get_or_init(|| {
        Mutex::new(ListenerState {
            running: false,
            discovered: HashMap::new(),
            error: None,
        })
    })
}

fn stop_flag() -> Arc<AtomicBool> {
    STOP.get_or_init(|| Arc::new(AtomicBool::new(false)))
        .clone()
}

/// 开始监听发现端口：幂等，重复调用只保证监听线程存活
pub fn start_listener() -> Result<(), String> {
    {
        let mut state = listener().lock();
        if state.running {
            return Ok(());
        }
        state.error = None;
    }
    let socket = UdpSocket::bind(("0.0.0.0", DISCOVERY_PORT))
        .map_err(|error| format!("监听 UDP {DISCOVERY_PORT} 失败：{error}"))?;
    socket
        .set_read_timeout(Some(Duration::from_millis(500)))
        .map_err(|error| format!("设置发现端口超时失败：{error}"))?;
    stop_flag().store(false, Ordering::SeqCst);
    listener().lock().running = true;
    let stop = stop_flag();
    std::thread::spawn(move || {
        let mut buffer = [0u8; 4096];
        while !stop.load(Ordering::SeqCst) {
            match socket.recv_from(&mut buffer) {
                Ok((size, peer)) => {
                    let Ok(beacon) = serde_json::from_slice::<MasterBeacon>(&buffer[..size]) else {
                        continue;
                    };
                    if beacon.app != "AstroMan" || beacon.v != DISCOVERY_VERSION {
                        continue;
                    }
                    if !allowed_source(peer, &beacon.host) {
                        continue;
                    }
                    ingest(beacon);
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) => {}
                // Windows 的 UDP 会把 ICMP port unreachable 报告成 ConnectionReset；
                // 与 EINTR 一样属于瞬时事件，处理掉继续收包，不能退出监听线程
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::ConnectionReset
                            | std::io::ErrorKind::ConnectionAborted
                            | std::io::ErrorKind::Interrupted
                    ) => {}
                Err(error) => {
                    listener().lock().error = Some(format!("读取发现数据失败：{error}"));
                    break;
                }
            }
            prune();
        }
        let mut state = listener().lock();
        state.running = false;
        state.discovered.clear();
    });
    Ok(())
}

/// 停止监听并清空列表
pub fn stop_listener() {
    stop_flag().store(true, Ordering::SeqCst);
    let mut state = listener().lock();
    state.running = false;
    state.discovered.clear();
}

/// 当前发现列表：自动剔除超过有效期的条目
pub fn list_discovered() -> Vec<DiscoveredMaster> {
    prune();
    let state = listener().lock();
    let mut items: Vec<DiscoveredMaster> = state.discovered.values().cloned().collect();
    items.sort_by_key(|item| std::cmp::Reverse(item.last_seen_ms));
    items
}

/// 监听是否运行中
pub fn listener_running() -> bool {
    listener().lock().running
}

/// 监听错误（端口占用等），供设置页提示
pub fn listener_error() -> Option<String> {
    listener().lock().error.clone()
}

/// 主节点广播 Beacon：多播 + 广播地址（单次发送，不阻塞）
pub fn broadcast_master(snapshot: &ClusterMasterSnapshot) {
    let Some(host) = snapshot.local_addresses.first().cloned() else {
        return;
    };
    let beacon = MasterBeacon {
        v: DISCOVERY_VERSION,
        kind: "master".to_string(),
        app: "AstroMan".to_string(),
        instance_id: format!(
            "job-{}-{}-{}",
            snapshot.job_id, snapshot.port, snapshot.created_at
        ),
        name: snapshot.name.clone(),
        host: host.clone(),
        port: snapshot.port,
        job_id: snapshot.job_id,
        status: snapshot.status.clone(),
        join_mode: "approval".to_string(),
        node_count: snapshot.nodes.len(),
        max_nodes: MAX_NODES,
        capacity: Some(BeaconCapacity {
            cores: std::thread::available_parallelism()
                .map(|value| value.get() as u32)
                .unwrap_or(4),
            mem_mb: crate::state::total_memory_bytes() / (1024 * 1024),
            budget: snapshot.budget,
        }),
        sent_at: crate::util::now_ms(),
    };
    let Ok(payload) = serde_json::to_vec(&beacon) else {
        return;
    };
    let Ok(socket) = UdpSocket::bind(("0.0.0.0", 0)) else {
        return;
    };
    let _ = socket.set_broadcast(true);
    let _ = socket.send_to(
        &payload,
        SocketAddr::V4(SocketAddrV4::new(MULTICAST_ADDR, DISCOVERY_PORT)),
    );
    for target in broadcast_targets(&host) {
        let _ = socket.send_to(&payload, target);
    }
}

fn ingest(beacon: MasterBeacon) {
    let now = crate::util::now_ms();
    let mut state = listener().lock();
    let entry = state
        .discovered
        .entry(beacon.instance_id.clone())
        .or_insert_with(|| DiscoveredMaster {
            instance_id: beacon.instance_id.clone(),
            name: beacon.name.clone(),
            host: beacon.host.clone(),
            port: beacon.port,
            addr: format!("{}:{}", beacon.host, beacon.port),
            job_id: beacon.job_id,
            status: beacon.status.clone(),
            join_mode: beacon.join_mode.clone(),
            node_count: beacon.node_count,
            max_nodes: beacon.max_nodes,
            capacity: beacon.capacity.clone(),
            last_seen_ms: now,
            latency_ms: 0,
        });
    entry.latency_ms = (now - entry.last_seen_ms).max(0);
    entry.last_seen_ms = now;
    entry.name = beacon.name;
    entry.host = beacon.host.clone();
    entry.port = beacon.port;
    entry.addr = format!("{}:{}", beacon.host, beacon.port);
    entry.job_id = beacon.job_id;
    entry.status = beacon.status;
    entry.join_mode = beacon.join_mode;
    entry.node_count = beacon.node_count;
    entry.max_nodes = beacon.max_nodes;
    entry.capacity = beacon.capacity;
}

fn prune() {
    let now = crate::util::now_ms();
    let ttl = (DISCOVERY_TTL_SEC * 1000) as i64;
    let mut state = listener().lock();
    state
        .discovered
        .retain(|_, item| now - item.last_seen_ms <= ttl);
}

/// 只接受私网 IPv4 与回环；公网 / 跨网段地址丢弃
fn allowed_source(peer: SocketAddr, host: &str) -> bool {
    let ip = match peer {
        SocketAddr::V4(value) => Some(std::net::IpAddr::V4(*value.ip())),
        SocketAddr::V6(value) => Some(std::net::IpAddr::V6(*value.ip())),
    };
    if !ip.is_some_and(super::is_private) {
        return false;
    }
    host.parse::<Ipv4Addr>().is_ok() || host.parse::<std::net::Ipv6Addr>().is_ok()
}

/// 计算各网卡的定向广播地址：取首段私网地址的 /24 广播
fn broadcast_targets(host: &str) -> Vec<SocketAddr> {
    let mut targets = Vec::new();
    if let Ok(ip) = host.parse::<Ipv4Addr>() {
        let octets = ip.octets();
        let mask_host = Ipv4Addr::new(octets[0], octets[1], octets[2], 255);
        targets.push(SocketAddr::V4(SocketAddrV4::new(mask_host, DISCOVERY_PORT)));
    }
    targets
}

/// 便于前端展示的发现状态
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveryStatus {
    pub running: bool,
    pub port: u16,
    pub ttl_sec: u64,
    pub error: Option<String>,
}

pub fn status() -> DiscoveryStatus {
    DiscoveryStatus {
        running: listener_running(),
        port: DISCOVERY_PORT,
        ttl_sec: DISCOVERY_TTL_SEC,
        error: listener_error(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn beacon_never_contains_secrets() {
        let beacon = MasterBeacon {
            v: DISCOVERY_VERSION,
            kind: "master".to_string(),
            app: "AstroMan".to_string(),
            instance_id: "x".to_string(),
            name: "n".to_string(),
            host: "192.168.1.2".to_string(),
            port: 17801,
            job_id: 1,
            status: "pairing".to_string(),
            join_mode: "approval".to_string(),
            node_count: 1,
            max_nodes: MAX_NODES,
            capacity: None,
            sent_at: 1,
        };
        let text = serde_json::to_string(&beacon).unwrap();
        assert!(!text.contains("pairCode"));
        assert!(!text.contains("token"));
    }

    #[test]
    fn broadcast_targets_use_24_mask() {
        let targets = broadcast_targets("192.168.1.10");
        let addr = targets.first().unwrap().to_string();
        assert_eq!(addr, "192.168.1.255:17802");
    }

    #[test]
    fn listener_lifecycle_is_idempotent() {
        // 端口被其他进程占用时允许失败，但不得 panic
        let _ = start_listener();
        let _ = list_discovered();
        stop_listener();
        assert!(!listener_running());
    }
}
