//! 抓包内核：MITM 代理、CA、录制管道。
//!
//! 本模块只负责「代理生命周期」，请求处理逻辑在 `handler.rs`（C-05 起接入）。
//! 代理跑在独立线程 + 独立 tokio runtime 上：`Runtime` 不能在 async 上下文里
//! drop，也不该和 Tauri 主 runtime 抢线程。代理 runtime 使用 2 个 worker：
//! 响应解压（gzip / br / zstd）和 body 收集是 CPU 活，单线程时一个大响应就会
//! 把所有并发请求堵在身后。

pub mod archive;
pub mod ca;
pub mod handler;
pub mod har;
pub mod intercept;
pub mod persist;
pub mod proxy;
pub mod store;
pub mod sysproxy;
pub mod upstream;

#[cfg(test)]
mod tests;

use intercept::{
    BreakpointRule, Decision, InterceptState, InterceptView, MapLocalRule, MapRemoteRule,
    WeakNetwork,
};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::net::{IpAddr, SocketAddr, TcpListener};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use store::{CaptureStore, FlowBrief, FlowDetail};
use tokio::sync::oneshot;

const DEFAULT_PORT: u16 = 8888;
const PORT_SCAN: u16 = 10;
/// 单次 HAR 导出允许携带的正文总量。超过后只允许「仅元数据」导出，
/// 避免满载（128MB 预算）时序列化把内存推到两倍以上。
pub const HAR_BODY_LIMIT: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CaptureState {
    Idle,
    Starting,
    Running,
    Stopping,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureConfig {
    pub port: u16,
    pub bind_addr: String,
    pub ssl_hosts: Vec<String>,
    pub ignore_patterns: Vec<String>,
    pub capture_static: bool,
    pub upstream_proxy: Option<String>,
    /// `memory` / `archive-meta` / `archive-full`，按会话生效（R7）
    #[serde(default)]
    pub archive_mode: persist::ArchiveMode,
    #[serde(default)]
    pub archive_name: Option<String>,
    /// 生产登录态等域名默认排除，支持 `*.example.com`
    #[serde(default)]
    pub archive_exclude: Vec<String>,
}

impl Default for CaptureConfig {
    fn default() -> Self {
        Self {
            port: DEFAULT_PORT,
            bind_addr: "127.0.0.1".into(),
            ssl_hosts: Vec::new(),
            ignore_patterns: Vec::new(),
            capture_static: true,
            upstream_proxy: None,
            archive_mode: persist::ArchiveMode::Memory,
            archive_name: None,
            archive_exclude: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureStatus {
    pub state: CaptureState,
    pub port: u16,
    pub bind_addr: String,
    pub session_id: Option<i64>,
    pub started_at: i64,
    pub flow_count: u64,
    pub dropped: u64,
    pub ca_installed: bool,
    pub sysproxy_on: bool,
    pub upstream_proxy: Option<String>,
    /// 本次录制是否真的走了上游代理（R6）
    #[serde(default)]
    pub upstream_active: bool,
    /// 命中 SSL 直通白名单的连接数（R6）
    #[serde(default)]
    pub passthrough_count: u64,
    pub last_error: Option<String>,
}

impl Default for CaptureStatus {
    fn default() -> Self {
        Self {
            state: CaptureState::Idle,
            port: 0,
            bind_addr: "127.0.0.1".into(),
            session_id: None,
            started_at: 0,
            flow_count: 0,
            dropped: 0,
            ca_installed: false,
            sysproxy_on: false,
            upstream_proxy: None,
            upstream_active: false,
            passthrough_count: 0,
            last_error: None,
        }
    }
}

struct Control {
    shutdown: Option<oneshot::Sender<()>>,
    join: Option<std::thread::JoinHandle<()>>,
    config: CaptureConfig,
    archive: Option<persist::ArchiveWriter>,
}

pub struct CaptureManager {
    data_dir: PathBuf,
    status: Arc<Mutex<CaptureStatus>>,
    store: Arc<CaptureStore>,
    intercept: Arc<InterceptState>,
    control: Mutex<Control>,
    pool: Option<crate::state::DbPool>,
}

impl CaptureManager {
    /// 仅测试用：以默认预算构造，生产路径走 `new_with_pool`（按档位）
    #[cfg(test)]
    pub fn new(data_dir: PathBuf) -> Self {
        Self::new_with_limits_and_pool(
            data_dir,
            crate::services::profile::CaptureLimits::default(),
            None,
        )
    }

    /// 生产路径：带数据库池，用于会话归档；启动时顺带做一次保留清理。
    pub fn new_with_pool(
        data_dir: PathBuf,
        limits: crate::services::profile::CaptureLimits,
        pool: crate::state::DbPool,
    ) -> Self {
        Self::new_with_limits_and_pool(data_dir, limits, Some(pool))
    }

    fn new_with_limits_and_pool(
        data_dir: PathBuf,
        limits: crate::services::profile::CaptureLimits,
        pool: Option<crate::state::DbPool>,
    ) -> Self {
        // 上次异常退出可能没来得及恢复系统代理，启动时先清理
        sysproxy::restore_stale(&data_dir);
        if let Some(pool) = &pool {
            if let Err(error) = persist::cleanup(pool, &data_dir) {
                log::warn!("启动时清理抓包归档失败：{error}");
            }
        }
        Self {
            data_dir: data_dir.clone(),
            status: Arc::new(Mutex::new(CaptureStatus::default())),
            store: Arc::new(CaptureStore::new_with_limits(limits)),
            // 断点/映射/弱网规则落盘，重启后仍然生效
            intercept: Arc::new(InterceptState::load(&intercept_path(&data_dir))),
            control: Mutex::new(Control {
                shutdown: None,
                join: None,
                config: CaptureConfig::default(),
                archive: None,
            }),
            pool,
        }
    }

    /// 设置页切换档位后立即生效：裁剪已有流量，不重启代理
    pub fn set_limits(&self, limits: crate::services::profile::CaptureLimits) {
        self.store.set_limits(limits);
    }

    pub fn data_dir(&self) -> &PathBuf {
        &self.data_dir
    }

    /// 拦截规则总览：规则、开关、挂起中的流量。
    pub fn intercept_view(&self) -> InterceptView {
        self.intercept.snapshot()
    }

    /// 保存断点开关与规则；传 None 表示只改开关或只改规则。
    pub fn set_breakpoints(
        &self,
        enabled: Option<bool>,
        rules: Option<Vec<BreakpointRule>>,
    ) -> Result<InterceptView, String> {
        self.intercept.set_breakpoints(enabled, rules);
        self.persist()?;
        Ok(self.intercept.snapshot())
    }

    pub fn set_map(
        &self,
        enabled: Option<bool>,
        rules: Option<Vec<MapLocalRule>>,
    ) -> Result<InterceptView, String> {
        self.intercept.set_map(enabled, rules);
        self.persist()?;
        Ok(self.intercept.snapshot())
    }

    /// 保存远程映射开关与规则。
    pub fn set_remote(
        &self,
        enabled: Option<bool>,
        rules: Option<Vec<MapRemoteRule>>,
    ) -> Result<InterceptView, String> {
        self.intercept.set_remote(enabled, rules);
        self.persist()?;
        Ok(self.intercept.snapshot())
    }

    pub fn set_weak(&self, weak: WeakNetwork) -> Result<InterceptView, String> {
        self.intercept.set_weak(weak);
        self.persist()?;
        Ok(self.intercept.snapshot())
    }

    /// 处理一条挂起的断点；暂停已超时时返回 E_PAUSE_GONE。
    pub fn resolve_pause(
        &self,
        pause_id: u64,
        decision: Decision,
    ) -> Result<InterceptView, String> {
        self.intercept.resolve(pause_id, decision)?;
        Ok(self.intercept.snapshot())
    }

    /// 全部放行，返回被释放的条数。
    pub fn release_pauses(&self) -> usize {
        self.intercept.release_all()
    }

    fn persist(&self) -> Result<(), String> {
        self.intercept.save(&intercept_path(&self.data_dir))
    }

    pub fn flows(&self, limit: usize) -> Vec<FlowBrief> {
        self.store.list(limit.clamp(1, 5_000))
    }

    pub fn flow_detail(&self, id: u64) -> Option<FlowDetail> {
        self.store.detail(id)
    }

    pub fn clear_flows(&self) {
        self.store.clear();
    }

    /// 导出当前全部流量为 HAR 1.2 文件。
    pub fn export_har(&self, path: &std::path::Path, metadata_only: bool) -> Result<(), String> {
        self.export_har_inner(path, None, metadata_only)
    }

    /// 导出指定流量（如某个域名分组）为 HAR 1.2 文件。
    pub fn export_har_ids(
        &self,
        path: &std::path::Path,
        ids: &[u64],
        metadata_only: bool,
    ) -> Result<(), String> {
        if ids.is_empty() {
            return Err("E_CAPTURE_EMPTY: 没有可导出的流量".into());
        }
        self.export_har_inner(path, Some(ids), metadata_only)
    }

    /// 流式写 HAR：分批从 store 取明细逐条落盘，内存不再随流量翻倍。
    ///
    /// 正文总量超过 [`HAR_BODY_LIMIT`] 时拒绝，由前端引导改用仅元数据模式；
    /// 中途失败或一条都没写出去时删掉半成品文件，避免留下损坏的 .har。
    fn export_har_inner(
        &self,
        path: &std::path::Path,
        ids: Option<&[u64]>,
        metadata_only: bool,
    ) -> Result<(), String> {
        let body_bytes = self.store.body_bytes(ids);
        if !metadata_only && body_bytes > HAR_BODY_LIMIT {
            return Err(format!(
                "E_CAPTURE_HAR_LARGE: 请求与响应正文共 {}，超过 {} 上限，请改用「仅元数据」导出",
                pretty_bytes(body_bytes),
                pretty_bytes(HAR_BODY_LIMIT)
            ));
        }

        let mut written = 0usize;
        let outcome = har::export_stream(path, |writer| {
            let mut emit = |flow: &FlowDetail| -> Result<(), String> {
                written += 1;
                writer.write_flow(flow, metadata_only)
            };
            match ids {
                Some(ids) => self
                    .store
                    .for_each_detail_ids(ids, store::EXPORT_BATCH, &mut emit),
                None => self.store.for_each_detail(store::EXPORT_BATCH, &mut emit),
            }
        });

        if let Err(error) = outcome {
            let _ = std::fs::remove_file(path);
            return Err(error);
        }
        if written == 0 {
            let _ = std::fs::remove_file(path);
            return Err(if ids.is_some() {
                "E_CAPTURE_EMPTY: 选中的流量已被淘汰".into()
            } else {
                "E_CAPTURE_EMPTY: 当前没有可导出的流量".into()
            });
        }
        Ok(())
    }

    /// 导出当前全部流量为 ZIP：索引页 + HAR + 每条流量的可读明细。
    pub fn export_zip(&self, path: &std::path::Path) -> Result<(), String> {
        let flows = self.store.details();
        if flows.is_empty() {
            return Err("E_CAPTURE_EMPTY: 当前没有可导出的流量".into());
        }
        archive::export(path, &flows)
    }

    /// 导出指定流量（如某个域名分组）为 ZIP。
    pub fn export_zip_ids(&self, path: &std::path::Path, ids: &[u64]) -> Result<(), String> {
        if ids.is_empty() {
            return Err("E_CAPTURE_EMPTY: 没有可导出的流量".into());
        }
        let flows = self.store.details_by_ids(ids);
        if flows.is_empty() {
            return Err("E_CAPTURE_EMPTY: 选中的流量已被淘汰".into());
        }
        archive::export(path, &flows)
    }

    pub fn status(&self) -> CaptureStatus {
        let mut status = self.status.lock().clone();
        status.flow_count = self.store.total();
        status.dropped = self.store.dropped();
        status.passthrough_count = self.store.passthrough_count();
        status
    }

    pub fn sysproxy_enable(&self) -> Result<CaptureStatus, String> {
        let status = self.status.lock().clone();
        if status.state != CaptureState::Running || status.port == 0 {
            return Err("E_SYSPROXY: 请先开始录制".into());
        }
        sysproxy::enable(&self.data_dir, status.port)?;
        let mut current = self.status.lock();
        current.sysproxy_on = true;
        Ok(current.clone())
    }

    pub fn sysproxy_disable(&self) -> Result<CaptureStatus, String> {
        sysproxy::disable(&self.data_dir)?;
        let mut current = self.status.lock();
        current.sysproxy_on = false;
        Ok(current.clone())
    }

    /// 启动代理。`config` 为 None 时沿用上次配置（首次为默认值）。
    pub fn start(&self, config: Option<CaptureConfig>) -> Result<CaptureStatus, String> {
        let mut control = self.control.lock();
        let mut status = self.status.lock();
        if matches!(
            status.state,
            CaptureState::Starting | CaptureState::Running | CaptureState::Stopping
        ) {
            return Err("E_CAPTURE_RUNNING: 抓包已在运行".into());
        }

        let config = match config {
            Some(config) => config,
            None => control.config.clone(),
        };
        // 上次异常退出未收尾的归档先落盘，避免 writer 线程悬空
        if let Some(writer) = control.archive.take() {
            self.store.set_archive(None);
            writer.finish();
        }
        // 只支持 http://；socks5 / https 代理在启动前就明确失败
        let upstream = config
            .upstream_proxy
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(upstream::ProxySpec::parse)
            .transpose()?;
        let ip: IpAddr = config
            .bind_addr
            .parse()
            .map_err(|_| format!("E_BIND_ADDR: 无效的绑定地址 {}", config.bind_addr))?;

        status.state = CaptureState::Starting;
        status.last_error = None;
        status.port = 0;
        status.bind_addr = ip.to_string();
        // 状态里只回显脱敏地址，避免把代理密码带到前端
        status.upstream_proxy = upstream.as_ref().map(upstream::ProxySpec::label);
        status.upstream_active = upstream.is_some();
        status.passthrough_count = 0;
        status.started_at = crate::util::now_ms();
        status.flow_count = 0;
        status.dropped = 0;
        status.sysproxy_on = false;
        status.session_id = None;
        self.store.clear();

        let material = match ca::ensure(&self.data_dir) {
            Ok(material) => material,
            Err(error) => {
                status.state = CaptureState::Error;
                status.last_error = Some(error.clone());
                return Err(error);
            }
        };
        let authority = match ca::authority(&material) {
            Ok(authority) => authority,
            Err(error) => {
                status.state = CaptureState::Error;
                status.last_error = Some(error.clone());
                return Err(error);
            }
        };

        let listener = match bind_listener(ip, config.port) {
            Ok(listener) => listener,
            Err(error) => {
                status.state = CaptureState::Error;
                status.last_error = Some(error.clone());
                return Err(error);
            }
        };
        let prepared = listener
            .local_addr()
            .map_err(|e| format!("E_BIND: 读取端口失败 {e}"))
            .and_then(|addr| {
                listener
                    .set_nonblocking(true)
                    .map_err(|e| format!("E_BIND: 设置非阻塞失败 {e}"))
                    .map(|_| addr.port())
            });
        let port = match prepared {
            Ok(port) => port,
            Err(error) => {
                status.state = CaptureState::Error;
                status.last_error = Some(error.clone());
                return Err(error);
            }
        };

        let mut archive_writer = None;
        if config.archive_mode.is_archive() {
            let Some(pool) = self.pool.as_ref() else {
                status.state = CaptureState::Error;
                status.last_error = Some("E_ARCHIVE: 当前环境未启用数据库归档".into());
                return Err("E_ARCHIVE: 当前环境未启用数据库归档".into());
            };
            if let Err(error) = persist::cleanup(pool, &self.data_dir) {
                log::warn!("新建抓包会话前清理归档失败：{error}");
            }
            let session_id = match persist::create_session(pool, &config, &status.bind_addr, port) {
                Ok(id) => id,
                Err(error) => {
                    status.state = CaptureState::Error;
                    status.last_error = Some(error.clone());
                    return Err(error);
                }
            };
            let writer = match persist::ArchiveWriter::start(
                pool.clone(),
                self.data_dir.clone(),
                session_id,
                config.archive_mode,
                config.archive_exclude.clone(),
            ) {
                Ok(writer) => writer,
                Err(error) => {
                    let _ = persist::delete_session(pool, &self.data_dir, session_id);
                    status.state = CaptureState::Error;
                    status.last_error = Some(error.clone());
                    return Err(error);
                }
            };
            self.store.set_archive(Some(writer.sender()));
            status.session_id = Some(session_id);
            archive_writer = Some(writer);
        }

        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let shared = self.status.clone();
        let thread_config = config.clone();
        let thread_store = self.store.clone();
        let thread_intercept = self.intercept.clone();
        let thread_data_dir = self.data_dir.clone();
        let spawned = std::thread::Builder::new()
            .name("capture-proxy".into())
            .spawn(move || {
                let runtime = match tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(2)
                    .enable_all()
                    .build()
                {
                    Ok(runtime) => runtime,
                    Err(error) => {
                        mark_error(&shared, format!("E_RUNTIME: {error}"));
                        return;
                    }
                };
                let result = runtime.block_on(async move {
                    let listener = tokio::net::TcpListener::from_std(listener)
                        .map_err(|e| format!("E_BIND: {e}"))?;
                    proxy::run(
                        listener,
                        authority,
                        thread_config,
                        thread_store,
                        thread_intercept,
                        async move {
                            let _ = shutdown_rx.await;
                        },
                    )
                    .await
                });
                // 代理线程结束时确保系统代理不会残留；停止流程里已经恢复过时这里是空操作
                if let Err(error) = sysproxy::disable(&thread_data_dir) {
                    log::warn!("代理退出时恢复系统代理失败：{error}");
                }
                if let Err(error) = result {
                    log::error!("抓包代理退出：{error}");
                    mark_error(&shared, error);
                } else {
                    let mut status = shared.lock();
                    if status.state != CaptureState::Error {
                        status.state = CaptureState::Idle;
                    }
                }
            });
        let join = match spawned {
            Ok(join) => join,
            Err(error) => {
                self.store.set_archive(None);
                if let Some(writer) = archive_writer {
                    writer.finish();
                }
                if let (Some(pool), Some(session_id)) = (self.pool.as_ref(), status.session_id) {
                    let _ = persist::delete_session(pool, &self.data_dir, session_id);
                }
                status.session_id = None;
                let message = format!("E_THREAD: 启动代理线程失败 {error}");
                status.state = CaptureState::Error;
                status.last_error = Some(message.clone());
                return Err(message);
            }
        };

        status.state = CaptureState::Running;
        status.port = port;
        control.config = config;
        control.shutdown = Some(shutdown_tx);
        control.join = Some(join);
        control.archive = archive_writer;
        Ok(status.clone())
    }

    pub fn stop(&self) -> Result<CaptureStatus, String> {
        let (shutdown, join, archive) = {
            let mut control = self.control.lock();
            let mut status = self.status.lock();
            if status.state == CaptureState::Idle {
                return Ok(status.clone());
            }
            status.state = CaptureState::Stopping;
            (
                control.shutdown.take(),
                control.join.take(),
                control.archive.take(),
            )
        };

        // 停止时把挂起的断点全部放行，否则代理线程会一直等到超时
        let released = self.intercept.release_all();
        if released > 0 {
            log::info!("停止录制：放行 {released} 条挂起断点");
        }

        if let Some(shutdown) = shutdown {
            let _ = shutdown.send(());
        }
        if let Err(error) = sysproxy::disable(&self.data_dir) {
            log::warn!("停止录制时恢复系统代理失败：{error}");
        }
        let join_result = if let Some(join) = join {
            join.join()
                .map_err(|_| "E_THREAD: 代理线程异常退出".to_string())
        } else {
            Ok(())
        };

        // 代理线程已退出，才能确定不会再有新的归档消息
        self.store.set_archive(None);
        if let Some(writer) = archive {
            let stats = writer.finish();
            log::info!(
                "抓包会话归档完成：{} 条，队列丢弃 {} 条，body {} 字节",
                stats.persisted,
                stats.dropped,
                stats.body_bytes
            );
        }
        join_result?;

        let mut status = self.status.lock();
        status.state = CaptureState::Idle;
        status.port = 0;
        status.session_id = None;
        status.sysproxy_on = false;
        Ok(status.clone())
    }
}

/// 断点 / 本地映射 / 弱网的规则文件。
fn intercept_path(data_dir: &Path) -> PathBuf {
    data_dir.join("capture").join("intercept.json")
}

fn mark_error(shared: &Arc<Mutex<CaptureStatus>>, error: String) {
    let mut status = shared.lock();
    status.state = CaptureState::Error;
    status.last_error = Some(error);
}

/// 优先使用用户端口，被占用时向上扫描 10 个；用户显式给 0 时由系统分配。
fn bind_listener(ip: IpAddr, preferred: u16) -> Result<TcpListener, String> {
    if preferred == 0 {
        return TcpListener::bind(SocketAddr::new(ip, 0))
            .map_err(|e| format!("E_BIND: 绑定 {ip}:0 失败 {e}"));
    }
    let mut last_error = None;
    for offset in 0..PORT_SCAN {
        let port = preferred.saturating_add(offset);
        if port == 0 {
            break;
        }
        match TcpListener::bind(SocketAddr::new(ip, port)) {
            Ok(listener) => return Ok(listener),
            Err(error) => last_error = Some((port, error)),
        }
    }
    match last_error {
        Some((port, error)) => Err(format!(
            "E_BIND: {}~{} 端口均不可用（最后尝试 {port}：{error}）",
            preferred,
            preferred.saturating_add(PORT_SCAN - 1)
        )),
        None => Err(format!("E_BIND: 没有可用端口（起始 {preferred}）")),
    }
}

/// 给导出上限提示用的粗略字节数（1024 进制，保留一位小数）。
fn pretty_bytes(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}
