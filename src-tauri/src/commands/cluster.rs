//! 分布式压测命令入口
//!
//! 主节点会话的协议、状态机与持久化都在 `services::cluster::server`；
//! 这里只做 Tauri 参数/返回值适配，方便后续 CLI 与 E2E 复用。

use tauri::{AppHandle, State};

use crate::services::cluster::client::{parse_master_addr, ClusterSlaveConnectPayload};
use crate::services::cluster::discovery::{self, DiscoveredMaster, DiscoveryStatus};
use crate::services::cluster::history::{
    self, ClusterJobDetail, ClusterJobPage, ClusterSlaveHistoryItem,
};
use crate::services::cluster::presence::{self, DiscoveredPeer, MasterInvite, PresenceStatus};
use crate::services::cluster::protocol::{
    ClusterMasterSnapshot, ClusterProtocolInfo, ClusterSlaveSnapshot,
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
    mode: Option<String>,
    weights: Option<Vec<ClusterNodeWeight>>,
) -> Result<ClusterMasterSnapshot, String> {
    state
        .cluster
        .prepare(job_id, mode, weights.unwrap_or_default())
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

/// 节点最近 N 个每秒采样（AI 诊断用，默认 30，最多 300）
#[tauri::command]
pub fn cluster_node_samples(
    state: State<'_, AppState>,
    job_id: i64,
    node_id: String,
    limit: Option<usize>,
) -> Result<Vec<crate::services::cluster::protocol::SamplePayload>, String> {
    state.cluster.node_samples(job_id, &node_id, limit)
}

/// 变量体检：列出每个变量的来源、主节点是否有值与各从节点状态（不含变量值）
#[tauri::command]
pub fn cluster_var_audit(
    state: State<'_, AppState>,
    job_id: i64,
) -> Result<crate::services::cluster::protocol::VarAuditReport, String> {
    state.cluster.var_audit(job_id)
}

/// 接受一个待审批节点（审批配对）
#[tauri::command]
pub fn cluster_master_approve(
    state: State<'_, AppState>,
    job_id: i64,
    node_id: String,
) -> Result<ClusterMasterSnapshot, String> {
    state.cluster.approve(job_id, &node_id)
}

/// 拒绝一个待审批节点
#[tauri::command]
pub fn cluster_master_reject(
    state: State<'_, AppState>,
    job_id: i64,
    node_id: String,
    reason: Option<String>,
) -> Result<ClusterMasterSnapshot, String> {
    state.cluster.reject(job_id, &node_id, reason)
}

/// 本次任务自动接受加入申请
#[tauri::command]
pub fn cluster_master_auto_accept(
    state: State<'_, AppState>,
    job_id: i64,
    enabled: bool,
) -> Result<ClusterMasterSnapshot, String> {
    state.cluster.set_auto_accept(job_id, enabled)
}

/// 启动局域网发现监听（从节点侧）
#[tauri::command]
pub fn cluster_discovery_start() -> Result<DiscoveryStatus, String> {
    discovery::start_listener()?;
    Ok(discovery::status())
}

/// 停止局域网发现监听
#[tauri::command]
pub fn cluster_discovery_stop() -> DiscoveryStatus {
    discovery::stop_listener();
    discovery::status()
}

/// 发现状态：是否监听、端口与错误
#[tauri::command]
pub fn cluster_discovery_status() -> DiscoveryStatus {
    discovery::status()
}

/// 附近的主节点列表（6 秒内有 Beacon）
#[tauri::command]
pub fn cluster_discovery_list() -> Vec<DiscoveredMaster> {
    discovery::list_discovered()
}

/// 发现后申请加入：审批模式，等待主机接受
#[tauri::command]
pub fn cluster_join_discovered(
    state: State<'_, AppState>,
    app: AppHandle,
    master_addr: String,
    node_name: Option<String>,
) -> Result<ClusterSlaveSnapshot, String> {
    let master = parse_master_addr(&master_addr)?;
    state.policy().ensure_lan(Some(&master.host))?;
    state.cluster_slave.connect(
        state.pool.clone(),
        app,
        ClusterSlaveConnectPayload {
            master_addr,
            pair_code: String::new(),
            join_mode: Some("request".to_string()),
            invite_token: None,
            node_name,
        },
    )
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

/* ---------- 设备在场发现与邀请 ---------- */

/// 启动设备发现与邀请监听（端口 17803；名称 / 策略取自设置）
#[tauri::command]
pub fn cluster_presence_start(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<PresenceStatus, String> {
    let settings = state.settings();
    presence::start(
        &app,
        &state.data_dir,
        &settings.cluster_device_name,
        settings.cluster_presence_enabled,
        &settings.cluster_invite_policy,
    )?;
    Ok(presence::status())
}

/// 停止设备发现：不再广播，也不再接受邀请
#[tauri::command]
pub fn cluster_presence_stop() -> PresenceStatus {
    presence::stop();
    presence::status()
}

/// 在场状态：是否运行、本机身份、在线设备数、错误
#[tauri::command]
pub fn cluster_presence_status() -> PresenceStatus {
    presence::status()
}

/// 当前在线设备（按可邀请与最近活跃排序）
#[tauri::command]
pub fn cluster_presence_list() -> Vec<DiscoveredPeer> {
    presence::list_peers()
}

/// 主节点向在线设备发起邀请，等待对方在本机确认
#[tauri::command]
pub async fn cluster_invite_send(
    state: State<'_, AppState>,
    job_id: i64,
    host: String,
    port: u16,
    device_id: Option<String>,
) -> Result<MasterInvite, String> {
    presence::send_invite(&state, job_id, &host, port, device_id).await
}

/// 被邀请方处理邀请：接受后立即反向连接主节点
#[tauri::command]
pub fn cluster_invite_respond(
    app: AppHandle,
    request_id: String,
    accept: bool,
) -> Result<crate::services::cluster::protocol::ClusterSlaveSnapshot, String> {
    presence::respond(&app, &request_id, accept)
}

/* ---------- 历史记录 ---------- */

/// 主节点任务历史（分页）；状态与关键字传空串表示不过滤
#[tauri::command]
pub fn cluster_jobs_list(
    state: State<'_, AppState>,
    page: Option<u32>,
    page_size: Option<u32>,
    status: Option<String>,
    keyword: Option<String>,
) -> Result<ClusterJobPage, String> {
    history::list_jobs(
        &state.pool,
        page.unwrap_or(1).max(1),
        page_size.unwrap_or(20).clamp(1, 100),
        status.as_deref().unwrap_or(""),
        keyword.as_deref().unwrap_or(""),
    )
}

/// 主节点任务详情：任务本身 + 节点明细
#[tauri::command]
pub fn cluster_job_detail(
    state: State<'_, AppState>,
    job_id: i64,
) -> Result<ClusterJobDetail, String> {
    history::job_detail(&state.pool, job_id)
}

/// 清理已结束的任务历史；`keep_days` 为 0 时清空全部已结束任务
#[tauri::command]
pub fn cluster_jobs_clear(
    state: State<'_, AppState>,
    keep_days: Option<i64>,
) -> Result<u32, String> {
    history::clear_jobs(&state.pool, keep_days.unwrap_or(0))
}

/// 从节点参与记录：本机连过的主节点与任务
#[tauri::command]
pub fn cluster_slave_history_list(
    state: State<'_, AppState>,
    limit: Option<u32>,
    offset: Option<u32>,
) -> Result<Vec<ClusterSlaveHistoryItem>, String> {
    history::list_slave_history(
        &state.pool,
        limit.unwrap_or(50).clamp(1, 200),
        offset.unwrap_or(0),
    )
}

/// 清空从节点参与记录
#[tauri::command]
pub fn cluster_slave_history_clear(state: State<'_, AppState>) -> Result<u32, String> {
    history::clear_slave_history(&state.pool)
}
