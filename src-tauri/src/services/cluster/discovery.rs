//! mDNS 发现（R9）
//!
//! 主节点在 `_astroman-cluster._tcp.local.` 注册服务，TXT 带版本 / 节点名 / 角色；
//! 从节点浏览同一服务类型列出可用主节点。mDNS 只是便利入口，失败时手动填 IP 仍然可用。

use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::OnceLock;
use std::time::Duration;

use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use parking_lot::Mutex;
use serde::Serialize;

use super::protocol::{MDNS_SERVICE_TYPE, PROTOCOL_VERSION};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredMaster {
    pub fullname: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub addresses: Vec<String>,
    pub version: u32,
    pub node_name: String,
    pub role: String,
}

static DAEMON: OnceLock<Option<ServiceDaemon>> = OnceLock::new();
static ADVERTISED: OnceLock<Mutex<HashMap<i64, String>>> = OnceLock::new();

fn daemon() -> Result<ServiceDaemon, String> {
    DAEMON
        .get_or_init(|| ServiceDaemon::new().ok())
        .clone()
        .ok_or_else(|| "mDNS 初始化失败：本机可能没有可用的多播网卡".to_string())
}

fn advertised() -> &'static Mutex<HashMap<i64, String>> {
    ADVERTISED.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 注册主节点服务；返回失败原因但不影响任务创建
pub fn advertise_master(job_id: i64, node_name: &str, port: u16) -> Result<(), String> {
    let daemon = daemon()?;
    let addresses = super::primary_lan_ip()
        .filter(|ip| super::is_private(*ip))
        .map(|ip| vec![ip.to_string()])
        .unwrap_or_default();
    if addresses.is_empty() {
        return Err("没有可用的内网地址，跳过 mDNS 注册".to_string());
    }
    let instance = format!("AstroMan-{node_name}-{job_id}");
    let host = format!("astroman-{job_id}.local.");
    let mut properties = HashMap::new();
    properties.insert("version".to_string(), PROTOCOL_VERSION.to_string());
    properties.insert("nodeName".to_string(), node_name.to_string());
    properties.insert("role".to_string(), "master".to_string());
    let info = ServiceInfo::new(
        MDNS_SERVICE_TYPE,
        &instance,
        &host,
        addresses.as_slice(),
        port,
        properties,
    )
    .map_err(|error| format!("构造 mDNS 服务失败：{error}"))?;
    let fullname = info.get_fullname().to_string();
    daemon
        .register(info)
        .map_err(|error| format!("注册 mDNS 服务失败：{error}"))?;
    advertised().lock().insert(job_id, fullname);
    Ok(())
}

/// 注销任务对应的 mDNS 服务；任务关闭或结束都调用，失败只记录日志
pub fn unadvertise_master(job_id: i64) {
    let fullname = advertised().lock().remove(&job_id);
    let Some(fullname) = fullname else { return };
    if let Ok(daemon) = daemon() {
        let _ = daemon.unregister(&fullname);
    }
}

/// 浏览局域网主节点：`timeout_ms` 内收集已解析的服务并去重
pub async fn browse_masters(timeout_ms: u64) -> Result<Vec<DiscoveredMaster>, String> {
    let timeout = Duration::from_millis(timeout_ms.clamp(500, 15_000));
    tokio::task::spawn_blocking(move || {
        let daemon = daemon()?;
        let receiver = daemon
            .browse(MDNS_SERVICE_TYPE)
            .map_err(|error| format!("启动 mDNS 浏览失败：{error}"))?;
        let deadline = std::time::Instant::now() + timeout;
        let mut seen: HashSet<String> = HashSet::new();
        let mut masters: Vec<DiscoveredMaster> = Vec::new();
        while let Some(left) = deadline.checked_duration_since(std::time::Instant::now()) {
            let Ok(event) = receiver.recv_timeout(left) else {
                break;
            };
            let ServiceEvent::ServiceResolved(info) = event else {
                continue;
            };
            if info
                .get_property_val_str("role")
                .map(str::trim)
                .is_some_and(|role| role != "master")
            {
                continue;
            }
            let fullname = info.get_fullname().to_string();
            if !seen.insert(fullname.clone()) {
                continue;
            }
            let version = info
                .get_property_val_str("version")
                .and_then(|text| text.parse::<u32>().ok())
                .unwrap_or(1);
            let node_name = info
                .get_property_val_str("nodeName")
                .unwrap_or_default()
                .to_string();
            let mut addresses: Vec<String> = info
                .get_addresses()
                .iter()
                .filter(|ip| super::is_private(ip.to_ip_addr()))
                .map(|ip| ip.to_ip_addr().to_string())
                .collect();
            addresses.sort();
            let host = addresses
                .first()
                .cloned()
                .unwrap_or_else(|| info.get_hostname().trim_end_matches('.').to_string());
            masters.push(DiscoveredMaster {
                name: info
                    .get_fullname()
                    .split('.')
                    .next()
                    .unwrap_or("AstroMan")
                    .to_string(),
                fullname,
                host,
                port: info.get_port(),
                addresses,
                version,
                node_name,
                role: "master".to_string(),
            });
        }
        let _ = daemon.stop_browse(MDNS_SERVICE_TYPE);
        Ok(masters)
    })
    .await
    .map_err(|error| format!("mDNS 浏览任务失败：{error}"))?
}
