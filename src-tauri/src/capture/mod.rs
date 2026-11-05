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
pub mod proxy;
pub mod store;
pub mod sysproxy;

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
            last_error: None,
        }
    }
}

struct Control {
    shutdown: Option<oneshot::Sender<()>>,
    join: Option<std::thread::JoinHandle<()>>,
    config: CaptureConfig,
}

pub struct CaptureManager {
    data_dir: PathBuf,
    status: Arc<Mutex<CaptureStatus>>,
    store: Arc<CaptureStore>,
    intercept: Arc<InterceptState>,
    control: Mutex<Control>,
}

impl CaptureManager {
    pub fn new(data_dir: PathBuf) -> Self {
        // 上次异常退出可能没来得及恢复系统代理，启动时先清理
        sysproxy::restore_stale(&data_dir);
        Self {
            data_dir: data_dir.clone(),
            status: Arc::new(Mutex::new(CaptureStatus::default())),
            store: Arc::new(CaptureStore::new()),
            // 断点/映射/弱网规则落盘，重启后仍然生效
            intercept: Arc::new(InterceptState::load(&intercept_path(&data_dir))),
            control: Mutex::new(Control {
                shutdown: None,
                join: None,
                config: CaptureConfig::default(),
            }),
        }
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
    pub fn export_har(&self, path: &std::path::Path) -> Result<(), String> {
        let flows = self.store.details();
        if flows.is_empty() {
            return Err("E_CAPTURE_EMPTY: 当前没有可导出的流量".into());
        }
        har::export(path, &flows)
    }

    /// 导出指定流量（如某个域名分组）为 HAR 1.2 文件。
    pub fn export_har_ids(&self, path: &std::path::Path, ids: &[u64]) -> Result<(), String> {
        if ids.is_empty() {
            return Err("E_CAPTURE_EMPTY: 没有可导出的流量".into());
        }
        let flows = self.store.details_by_ids(ids);
        if flows.is_empty() {
            return Err("E_CAPTURE_EMPTY: 选中的流量已被淘汰".into());
        }
        har::export(path, &flows)
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
        let ip: IpAddr = config
            .bind_addr
            .parse()
            .map_err(|_| format!("E_BIND_ADDR: 无效的绑定地址 {}", config.bind_addr))?;

        status.state = CaptureState::Starting;
        status.last_error = None;
        status.port = 0;
        status.bind_addr = ip.to_string();
        status.upstream_proxy = config.upstream_proxy.clone();
        status.started_at = crate::util::now_ms();
        status.flow_count = 0;
        status.dropped = 0;
        status.sysproxy_on = false;
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
        Ok(status.clone())
    }

    pub fn stop(&self) -> Result<CaptureStatus, String> {
        let (shutdown, join) = {
            let mut control = self.control.lock();
            let mut status = self.status.lock();
            if status.state == CaptureState::Idle {
                return Ok(status.clone());
            }
            status.state = CaptureState::Stopping;
            (control.shutdown.take(), control.join.take())
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
        if let Some(join) = join {
            join.join()
                .map_err(|_| "E_THREAD: 代理线程异常退出".to_string())?;
        }

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
