//! 分布式压测命令入口
//!
//! 主节点会话的协议、状态机与持久化都在 `services::cluster::server`；
//! 这里只做 Tauri 参数/返回值适配，方便后续 CLI 与 E2E 复用。

use tauri::{AppHandle, State};

use crate::services::cluster::client::{parse_master_addr, ClusterSlaveConnectPayload};
use crate::services::cluster::discovery::{self, DiscoveredMaster};
use crate::services::cluster::protocol::{
    ClusterDevice, ClusterMasterSnapshot, ClusterProtocolInfo, ClusterSlaveSnapshot,
};
use crate::services::cluster::server::{ClusterMasterStartPayload, ClusterNodeWeight};
use crate::state::AppState;

/// 协议参数：端口、配对码有效期、心跳与时钟对齐口径
#[tauri::command]
pub fn cluster_protocol_info() -> ClusterProtocolInfo {
    ClusterProtocolInfo::default()
}

/// 本机可用于配对的地址：优先默认出口网卡的私网地址，再补 127.0.0.1
#[tauri::command]
pub fn cluster_local_addresses() -> Vec<String> {
    crate::services::cluster::local_addresses()
}

/// 创建主节点任务：绑定端口、生成配对码、等待从节点反向连接
#[tauri::command]
pub async fn cluster_master_create(
    state: State<'_, AppState>,
    app: AppHandle,
    payload: ClusterMasterStartPayload,
) -> Result<ClusterMasterSnapshot, String> {
    state.policy().ensure_lan(Some("0.0.0.0"))?;
    state.cluster.create(state.pool.clone(), app, payload).await
}

/// 任务快照：前端每秒轮询或按 `cluster:master` 事件刷新
#[tauri::command]
pub fn cluster_master_snapshot(
    state: State<'_, AppState>,
    job_id: i64,
) -> Result<ClusterMasterSnapshot, String> {
    state.cluster.snapshot(job_id)
}

/// 当前进程内所有主节点任务
#[tauri::command]
pub fn cluster_master_sessions(state: State<'_, AppState>) -> Vec<ClusterMasterSnapshot> {
    state.cluster.snapshots()
}

/// 下发配置与并发分配，等待从节点回执与目标预检结果
#[tauri::command]
pub async fn cluster_master_prepare(
    state: State<'_, AppState>,
    job_id: i64,
    weights: Option<Vec<ClusterNodeWeight>>,
) -> Result<ClusterMasterSnapshot, String> {
    state
        .cluster
        .prepare(job_id, weights.unwrap_or_default())
        .await
}

/// 全部节点就绪后同步开跑
#[tauri::command]
pub fn cluster_master_launch(
    state: State<'_, AppState>,
    job_id: i64,
) -> Result<ClusterMasterSnapshot, String> {
    state.cluster.launch(job_id)
}

/// 中止任务：准备阶段直接失败，运行阶段保留已收到数据并标记不完整
#[tauri::command]
pub fn cluster_master_abort(
    state: State<'_, AppState>,
    job_id: i64,
    reason: Option<String>,
) -> Result<ClusterMasterSnapshot, String> {
    state.cluster.abort(job_id, reason)
}

/// 关闭并移除会话（运行中会先中止）
#[tauri::command]
pub fn cluster_master_close(state: State<'_, AppState>, job_id: i64) -> Result<(), String> {
    state.cluster.close(job_id)
}

/// 从节点加入主节点：反向连接、配对码换节点身份、本地解析环境后等待下发配置
#[tauri::command]
pub fn cluster_slave_connect(
    state: State<'_, AppState>,
    app: AppHandle,
    payload: ClusterSlaveConnectPayload,
) -> Result<ClusterSlaveSnapshot, String> {
    let master = parse_master_addr(&payload.master_addr)?;
    state.policy().ensure_lan(Some(&master.host))?;
    state
        .cluster_slave
        .connect(state.pool.clone(), app, payload)
}

/// 从节点会话快照：前端可按 `cluster:slave` 事件刷新
#[tauri::command]
pub fn cluster_slave_snapshot(state: State<'_, AppState>) -> ClusterSlaveSnapshot {
    state.cluster_slave.snapshot()
}

/// 主动断开从节点会话：运行中会同时中止本机引擎
#[tauri::command]
pub fn cluster_slave_disconnect(state: State<'_, AppState>) -> Result<(), String> {
    state.cluster_slave.disconnect()
}

/// 历史设备缓存：能力检测结果按稳定 deviceId 跨任务复用
#[tauri::command]
pub fn cluster_devices_list(
    state: State<'_, AppState>,
    limit: Option<usize>,
) -> Result<Vec<ClusterDevice>, String> {
    let mut items =
        crate::services::cluster::devices::list_devices(&state.pool, limit.unwrap_or(50))?;
    let online: std::collections::HashSet<String> = state
        .cluster
        .snapshots()
        .into_iter()
        .flat_map(|session| session.nodes)
        .filter(|node| {
            node.role == "slave"
                && !matches!(
                    node.status.as_str(),
                    "lost" | "left" | "rejected" | "done" | "disconnected"
                )
        })
        .filter_map(|node| node.device_id)
        .collect();
    for item in &mut items {
        item.online = online.contains(&item.node_id);
    }
    Ok(items)
}

/// 浏览局域网主节点：mDNS 只做便利发现，失败时手动填 IP
#[tauri::command]
pub async fn cluster_mdns_browse(timeout_ms: Option<u64>) -> Result<Vec<DiscoveredMaster>, String> {
    discovery::browse_masters(timeout_ms.unwrap_or(2_500)).await
}
