use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tauri::Manager;

pub type DbPool = Pool<SqliteConnectionManager>;

/// 数据库文件名
pub const DB_FILE: &str = "astroman.db";
/// 旧数据库文件名（FbinHttp 时期与更早的 FinkMeter 时期），按新→旧顺序尝试
const LEGACY_DB_FILES: [&str; 2] = ["fbinhttp.db", "finkmeter.db"];
/// 旧默认数据目录名，按新→旧顺序尝试
const LEGACY_DIR_NAMES: [&str; 2] = ["FbinHttp", "FinkMeter"];
/// 旧 Tauri identifier，对应旧的配置文件目录，按新→旧顺序尝试
const LEGACY_IDENTIFIERS: [&str; 2] = ["com.fbinhttp.app", "com.finkmeter.app"];

/// 数据目录指向文件（保存在固定的系统配置目录，不随数据目录移动）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageConfig {
    pub data_dir: String,
}

/// 全局网络与安全设置（保存在系统配置目录，不随数据目录移动）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppSettings {
    pub proxy_enabled: bool,
    pub proxy_url: String,
    /// 客户端证书（PKCS#12 格式，.pfx / .p12）
    pub cert_path: String,
    pub cert_password: String,
    /// Python 解释器路径，空表示自动探测
    pub python_path: String,
    /// 是否允许执行本地 Python 脚本（首次使用需显式授权）
    pub python_enabled: bool,
    /// 性能模式：auto / low / standard（P-05），空串按 auto 处理
    pub performance_mode: String,
    /// AI 权限模式：readonly / confirm_write / trusted（AI-01），空串按 confirm_write 处理
    pub ai_permission_mode: String,
    /// 摘要专用 provider id；0 = 沿用主 provider（AI-07）
    pub summary_provider_id: i64,
    /// 月度预算（美元）；0 = 不限制（AI-06）
    pub monthly_budget: f64,
    /// 预算达到 80% 时提醒；默认开启
    pub budget_alert: bool,
    /// 预算用尽后硬停止（默认只提醒，不阻断）
    pub budget_hard_stop: bool,
    /// 失败回退链：逗号分隔的 provider id，按顺序尝试；空 = 关闭（AI-12）
    pub ai_fallback_ids: String,
    /// 压测跨任务并发之和上限（v1.1.2 起仅手动模式使用；自动模式由预算计算）
    pub stress_max_concurrent: i64,
    /// 并发预算模式：auto（按 CPU/内存/fd 自动计算）/ manual（用户指定）
    pub stress_budget_mode: String,
    /// 同时运行的压测任务数上限；0 = 不限制
    pub stress_max_jobs: i64,
    /// OS 环境变量白名单（逗号分隔），供 `{{os:NAME}}` 使用；空 = 全部禁止
    pub stress_os_env_allow: String,
    /// 是否允许局域网自动发现（UDP 17802）
    pub cluster_discovery_enabled: bool,
    /// 本机在局域网中的显示名（空 = 计算机名）
    pub cluster_device_name: String,
    /// 是否广播在场信息并接受设备邀请（UDP/TCP 17803）
    pub cluster_presence_enabled: bool,
    /// 收到邀请时的处理策略：ask（弹窗确认）/ auto（自动接受）/ off（拒绝邀请）
    pub cluster_invite_policy: String,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            proxy_enabled: false,
            proxy_url: String::new(),
            cert_path: String::new(),
            cert_password: String::new(),
            python_path: String::new(),
            python_enabled: false,
            performance_mode: String::new(),
            ai_permission_mode: String::new(),
            summary_provider_id: 0,
            monthly_budget: 0.0,
            // 预算提醒默认开启：旧版设置文件没有该字段时也按开启处理
            budget_alert: true,
            budget_hard_stop: false,
            ai_fallback_ids: String::new(),
            stress_max_concurrent: 1000,
            stress_budget_mode: "auto".to_string(),
            stress_max_jobs: 0,
            stress_os_env_allow: String::new(),
            cluster_discovery_enabled: true,
            cluster_device_name: String::new(),
            cluster_presence_enabled: true,
            cluster_invite_policy: "ask".to_string(),
        }
    }
}

/* ---------- v1.1.2 并发预算 ---------- */

/// 并发预算模式
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BudgetMode {
    /// 按 CPU / 内存 / 文件句柄 / 最近能力检测自动计算
    Auto,
    /// 用户在设置页手动指定
    Manual,
}

impl BudgetMode {
    pub fn normalize(raw: &str) -> Self {
        if raw.eq_ignore_ascii_case("manual") {
            BudgetMode::Manual
        } else {
            BudgetMode::Auto
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            BudgetMode::Auto => "auto",
            BudgetMode::Manual => "manual",
        }
    }
}

/// 预算来源：把「为什么是这个数」完整暴露给设置页
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BudgetSources {
    pub cores: u32,
    pub mem_gb: f64,
    pub fd_limit: u64,
    pub probe_saturation: Option<u32>,
    pub by_cpu: u32,
    pub by_fd: u32,
    pub by_mem: u32,
    /// 本次预算实际被哪一项压住：cpu / fd / mem / probe / manual
    pub limited_by: String,
    pub computed_at: i64,
}

/// 本机并发预算快照
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConcurrencyBudget {
    /// auto / manual
    pub mode: String,
    /// 本机所有压测任务并发之和的上限
    pub machine_limit: u32,
    /// 单任务上限（正常等于 machine_limit）
    pub per_job_limit: u32,
    /// 当前活跃任务数
    pub active_jobs: usize,
    /// 任务数上限；0 = 不限
    pub max_jobs: usize,
    pub sources: BudgetSources,
}

/// 任务登记失败的类别
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReserveErrorKind {
    JobsFull,
    BudgetExceeded,
    SameTarget,
}

impl ReserveErrorKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ReserveErrorKind::JobsFull => "jobsFull",
            ReserveErrorKind::BudgetExceeded => "budgetExceeded",
            ReserveErrorKind::SameTarget => "sameTarget",
        }
    }
}

/// 结构化登记错误：前端可原样展示「当前总并发 + 本次 > 预算」
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReserveError {
    pub kind: String,
    pub requested: u32,
    pub current_total: u32,
    pub budget: u32,
    pub active_jobs: Vec<JobBrief>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobBrief {
    pub id: u64,
    pub name: String,
    pub concurrent: u32,
}

impl std::fmt::Display for ReserveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl From<ReserveError> for String {
    fn from(value: ReserveError) -> Self {
        value.message
    }
}

struct HttpClients {
    normal: reqwest::Client,
    insecure: reqwest::Client,
    /// 手工请求专用：不自动跟随重定向，由 send_chain 逐跳处理
    manual: reqwest::Client,
    manual_insecure: reqwest::Client,
    /// 建连探针：用来判断本次请求是「新建连接」还是「复用连接」
    probe: std::sync::Arc<crate::net_probe::ConnectorProbe>,
}

/// 企业策略运行态：解析失败时保留错误信息，设置页 / 诊断包可展示
#[derive(Debug, Clone)]
pub struct PolicyState {
    pub policy: crate::services::policy::Policy,
    pub error: Option<String>,
}

/// 运行中的一个压测任务
///
/// 任务归注册表所有：窗口关闭不影响任务，重新打开窗口按 `jobId` 恢复展示。
pub struct StressJob {
    pub id: u64,
    pub name: String,
    /// 展示用目标（渲染后的 URL）
    pub target: String,
    /// 归一化目标键 method + scheme://host:port/path，用于同目标判定
    pub target_key: String,
    pub plan_id: Option<i64>,
    pub request_id: Option<i64>,
    /// 本次运行峰值并发（跨任务求和用）
    pub concurrent: u32,
    pub started_at: i64,
    /// running / stopping
    pub status: String,
    /// 最新一次进度事件里的 QPS，主窗口面板展示用
    pub qps: AtomicU32,
    pub window_label: Option<String>,
    pub cancel: Arc<AtomicBool>,
}

/// 注册表对外快照：主窗口面板与窗口恢复共用
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StressJobSummary {
    pub id: u64,
    pub name: String,
    pub target: String,
    pub plan_id: Option<i64>,
    pub request_id: Option<i64>,
    pub concurrent: u32,
    pub started_at: i64,
    pub elapsed_sec: u64,
    pub status: String,
    pub qps: u32,
    pub window_label: Option<String>,
}

/// 新任务登记参数
pub struct StressJobDraft {
    pub name: String,
    pub target: String,
    pub target_key: String,
    pub plan_id: Option<i64>,
    pub request_id: Option<i64>,
    pub concurrent: u32,
}

/// 压测任务注册表：取代原先全局唯一的 `stress_running` / `stress_cancel`
pub struct StressJobRegistry {
    next_id: AtomicU64,
    jobs: parking_lot::Mutex<HashMap<u64, StressJob>>,
}

impl StressJobRegistry {
    pub fn new() -> Self {
        Self {
            next_id: AtomicU64::new(1),
            jobs: parking_lot::Mutex::new(HashMap::new()),
        }
    }

    /// 登记新任务：任务数、跨任务并发之和、同目标三项校验在同一把锁内完成，
    /// 避免多个窗口同时通过校验后一起启动。
    ///
    /// `max_jobs = 0` 表示不限制任务数；`max_total` 为本机并发预算。
    /// 返回 (jobId, 取消位, 被忽略的冲突任务 id)；同目标冲突且未声明忽略时返回错误。
    pub fn try_reserve(
        &self,
        draft: StressJobDraft,
        max_jobs: usize,
        max_total: u32,
        ignore_overlap: bool,
    ) -> Result<(u64, Arc<AtomicBool>, Vec<u64>), ReserveError> {
        let mut jobs = self.jobs.lock();
        let active: Vec<JobBrief> = {
            let mut items: Vec<JobBrief> = jobs
                .values()
                .map(|job| JobBrief {
                    id: job.id,
                    name: job.name.clone(),
                    concurrent: job.concurrent,
                })
                .collect();
            items.sort_by_key(|item| item.id);
            items
        };
        if max_jobs > 0 && active.len() >= max_jobs {
            return Err(ReserveError {
                kind: ReserveErrorKind::JobsFull.as_str().to_string(),
                requested: draft.concurrent,
                current_total: active.iter().map(|job| job.concurrent).sum(),
                budget: max_total,
                active_jobs: active,
                message: format!("最多同时运行 {max_jobs} 个压测任务，请先停止其中一个"),
            });
        }
        let total: u32 = active.iter().map(|job| job.concurrent).sum();
        if total.saturating_add(draft.concurrent) > max_total {
            return Err(ReserveError {
                kind: ReserveErrorKind::BudgetExceeded.as_str().to_string(),
                requested: draft.concurrent,
                current_total: total,
                budget: max_total,
                active_jobs: active,
                message: format!(
                    "当前总并发 {total} + 本次 {} 超过本机预算 {max_total}，请调低并发或先停止其他任务",
                    draft.concurrent
                ),
            });
        }
        let overlaps: Vec<u64> = jobs
            .values()
            .filter(|job| job.target_key == draft.target_key)
            .map(|job| job.id)
            .collect();
        if !overlaps.is_empty() && !ignore_overlap {
            return Err(ReserveError {
                kind: ReserveErrorKind::SameTarget.as_str().to_string(),
                requested: draft.concurrent,
                current_total: total,
                budget: max_total,
                active_jobs: active,
                message: format!(
                    "目标接口正在被任务 #{} 压测，同时施压会互相干扰；如确认继续请在配置中勾选「忽略同目标检查」",
                    overlaps[0]
                ),
            });
        }
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let cancel = Arc::new(AtomicBool::new(false));
        jobs.insert(
            id,
            StressJob {
                id,
                name: draft.name,
                target: draft.target,
                target_key: draft.target_key,
                plan_id: draft.plan_id,
                request_id: draft.request_id,
                concurrent: draft.concurrent,
                started_at: crate::util::now_ms(),
                status: "running".to_string(),
                qps: AtomicU32::new(0),
                window_label: None,
                cancel: cancel.clone(),
            },
        );
        Ok((id, cancel, overlaps))
    }

    /// 任务结束（完成 / 取消 / 失败）后释放登记
    pub fn finish(&self, id: u64) {
        self.jobs.lock().remove(&id);
    }

    /// 按任务停止，返回是否命中
    pub fn cancel(&self, id: u64) -> bool {
        let mut jobs = self.jobs.lock();
        match jobs.get_mut(&id) {
            Some(job) => {
                job.status = "stopping".to_string();
                job.cancel.store(true, Ordering::SeqCst);
                true
            }
            None => false,
        }
    }

    /// 全部停止（退出确认后使用），返回被取消的任务数
    pub fn cancel_all(&self) -> usize {
        let jobs = self.jobs.lock();
        for job in jobs.values() {
            job.cancel.store(true, Ordering::SeqCst);
        }
        jobs.len()
    }

    /// 绑定窗口 label（重复打开时聚焦已有窗口）
    pub fn set_window_label(&self, id: u64, label: &str) {
        if let Some(job) = self.jobs.lock().get_mut(&id) {
            job.window_label = Some(label.to_string());
        }
    }

    /// 把每秒进度里的 QPS 写回注册表：主窗口面板不订阅事件也能看到实时速率
    pub fn set_qps(&self, id: u64, qps: f64) {
        if let Some(job) = self.jobs.lock().get_mut(&id) {
            job.qps.store(qps.max(0.0).round() as u32, Ordering::SeqCst);
        }
    }

    pub fn window_label(&self, id: u64) -> Option<String> {
        self.jobs
            .lock()
            .get(&id)
            .and_then(|job| job.window_label.clone())
    }

    /// 同主机不同目标的运行中任务：只警告不拦截（1.3.2）
    ///
    /// 判定口径为 `scheme://host:port` 相同而完整目标键不同；同机同时施压会互相争抢
    /// 连接与带宽，报告里需要留痕，但用户确认后允许继续。
    pub fn same_host_others(&self, target_key: &str) -> Vec<(u64, String)> {
        let host = host_prefix_of(target_key);
        let mut items: Vec<(u64, String)> = self
            .jobs
            .lock()
            .values()
            .filter(|job| job.target_key != target_key && host_prefix_of(&job.target_key) == host)
            .map(|job| (job.id, job.name.clone()))
            .collect();
        items.sort_by_key(|(id, _)| *id);
        items
    }

    pub fn is_empty(&self) -> bool {
        self.jobs.lock().is_empty()
    }

    pub fn len(&self) -> usize {
        self.jobs.lock().len()
    }

    /// 运行中任务快照，按任务号升序
    pub fn snapshot(&self) -> Vec<StressJobSummary> {
        let now = crate::util::now_ms();
        let jobs = self.jobs.lock();
        let mut items: Vec<StressJobSummary> = jobs
            .values()
            .map(|job| StressJobSummary {
                id: job.id,
                name: job.name.clone(),
                target: job.target.clone(),
                plan_id: job.plan_id,
                request_id: job.request_id,
                concurrent: job.concurrent,
                started_at: job.started_at,
                elapsed_sec: (now - job.started_at).max(0) as u64 / 1000,
                status: job.status.clone(),
                qps: job.qps.load(Ordering::SeqCst),
                window_label: job.window_label.clone(),
            })
            .collect();
        items.sort_by_key(|item| item.id);
        items
    }
}

/// 从 `METHOD scheme://host:port/path` 中取出 `scheme://host:port`
///
/// 同主机判定忽略方法与路径：任何方法、任何路径落到同一主机端口都算同机施压。
fn host_prefix_of(target_key: &str) -> String {
    let rest = target_key
        .split_once(' ')
        .map(|(_, tail)| tail)
        .unwrap_or(target_key);
    let scheme_end = rest.find("://").map(|idx| idx + 3).unwrap_or(0);
    match rest[scheme_end..].find('/') {
        Some(idx) => rest[..scheme_end + idx].to_string(),
        None => rest.to_string(),
    }
}

pub struct AppState {
    pub pool: DbPool,
    pub data_dir: PathBuf,
    pub db_path: PathBuf,
    /// 未自定义时的默认数据目录
    pub default_data_dir: PathBuf,
    /// 自定义数据目录的配置文件
    pub config_file: PathBuf,
    /// 当前是否使用了自定义目录
    pub is_custom: bool,
    settings_file: PathBuf,
    settings: parking_lot::RwLock<AppSettings>,
    clients: parking_lot::RwLock<HttpClients>,
    /// 压测任务注册表（多任务并行；空即空闲）
    pub stress_jobs: Arc<StressJobRegistry>,
    /// 分布式压测主节点会话（多任务；空闲时不占资源）
    pub cluster: crate::services::cluster::server::ClusterMaster,
    /// 分布式压测从节点会话（同一进程只允许一个）
    pub cluster_slave: crate::services::cluster::client::ClusterSlave,
    /// 本机施压能力检测（全局单例，与压测互斥）
    pub capacity: crate::commands::capacity::CapacityState,
    /// AI 在途请求的取消标记：requestId → 取消位
    pub ai_cancels: Arc<parking_lot::Mutex<HashMap<String, Arc<AtomicBool>>>>,
    /// 套件运行的取消标记：runId → 取消位（按运行隔离，不用全局单例）
    pub test_cancels: Arc<parking_lot::Mutex<HashMap<i64, Arc<AtomicBool>>>>,
    /// 在途 HTTP 请求的取消标记：requestId → 取消位（4.6 取消能力）
    pub http_cancels: Arc<parking_lot::Mutex<HashMap<String, Arc<AtomicBool>>>>,
    /// 重任务互斥锁：备份 / VACUUM / integrity_check 与压测互斥（4.0.2）
    pub heavy_ops: Arc<parking_lot::Mutex<()>>,
    /// 抓包代理管理器：独立 runtime，空闲时不占资源
    pub capture: crate::capture::CaptureManager,
    /// 接口文档草稿缓存：draft_id → IR（TTL / LRU）
    pub docs_cache: crate::docs::cache::DocsCache,
    /// 只读文档站：进程内最多一个实例，空闲时不占资源
    pub docs_site: crate::services::docs_site::DocsSite,
    /// 本地 Mock Server：最多 3 个项目实例，空闲时不占资源
    pub mock: crate::services::mock::server::MockServer,
    /// SSE / WebSocket 实时会话（G-09）
    pub realtime: crate::services::realtime::RealtimeManager,
    /// 启动时固定的性能档位（P-05），运行中不随内存变化
    pub profile: parking_lot::RwLock<crate::services::profile::ActiveProfile>,
    /// 企业策略（policy.json），管理员改文件后可热加载
    pub policy: parking_lot::RwLock<PolicyState>,
}

impl AppState {
    pub fn new(app: &tauri::AppHandle) -> Result<Self, String> {
        // 产品已更名为 AstroMan，默认数据目录改为 AstroMan；
        // 依次尝试 FbinHttp / FinkMeter 旧目录，老数据继续可用
        let base_data_dir = app.path().data_dir().map_err(crate::util::err)?;
        let default_data_dir = base_data_dir.join("AstroMan");
        for (index, legacy_name) in LEGACY_DIR_NAMES.iter().enumerate() {
            let legacy_dir = base_data_dir.join(legacy_name);
            migrate_legacy_dir(&legacy_dir, &default_data_dir, LEGACY_DB_FILES[index]);
        }
        let config_dir = app.path().app_config_dir().map_err(crate::util::err)?;
        // identifier 已改为 com.astroman.app，先把旧配置目录里的 json
        //（storage.json / settings.json / 窗口状态）复制过来，升级后配置不丢
        if let Ok(base_config_dir) = app.path().config_dir() {
            for legacy_id in LEGACY_IDENTIFIERS {
                migrate_legacy_config(&base_config_dir.join(legacy_id), &config_dir);
            }
        }
        std::fs::create_dir_all(&config_dir).map_err(|e| format!("创建配置目录失败：{e}"))?;
        let config_file = config_dir.join("storage.json");

        let custom_dir = read_storage_config(&config_file);
        // 上次排队中的恢复：必须在任何连接建立之前完成文件替换
        let restore_target = custom_dir
            .clone()
            .unwrap_or_else(|| default_data_dir.clone());
        if crate::backup::has_pending_restore(&config_file) {
            match crate::backup::apply_pending_restore(&config_file, &restore_target) {
                Ok(Some(path)) => log::info!("启动时完成备份恢复：{path}"),
                Ok(None) => {}
                Err(error) => log::error!("恢复备份失败：{error}"),
            }
        }
        let (data_dir, pool, is_custom) = match custom_dir {
            Some(dir) if dir != default_data_dir => match open_pool(&dir) {
                Ok(pool) => (dir, pool, true),
                Err(error) => {
                    // 自定义目录不可用时回退到默认目录，避免应用无法启动；
                    // 保留 storage.json，目录恢复后下次启动仍会使用原位置
                    log::warn!(
                        "自定义数据目录不可用（{error}），本次回退到默认目录：{}",
                        default_data_dir.display()
                    );
                    (
                        default_data_dir.clone(),
                        open_pool(&default_data_dir)?,
                        false,
                    )
                }
            },
            _ => (
                default_data_dir.clone(),
                open_pool(&default_data_dir)?,
                false,
            ),
        };

        // 清理上次崩溃残留的无主报告 / 工作目录（文档 7.6）
        if let Ok(conn) = pool.get() {
            crate::report_builder::cleanup_orphans(&conn, &data_dir);
        }

        // 主密钥：钥匙串 / 口令 / 文件，并顺手把历史明文加密（4.2）
        let secrets = crate::secret::SecretStore::bootstrap(&data_dir, &pool)?;
        crate::secret::install(secrets);

        let db_path = data_dir.join(DB_FILE);
        let settings_file = config_dir.join("settings.json");
        let settings = read_settings(&settings_file);
        let clients = build_clients(&settings)?;
        let profile = crate::services::profile::active(&settings.performance_mode);
        let policy = crate::services::policy::load(&data_dir);
        if let Err(error) = crate::services::cluster::server::recover_stale_jobs(&pool) {
            log::warn!("清理上次遗留的集群任务失败：{error}");
        }

        let capture =
            crate::capture::CaptureManager::new_with_limits(data_dir.clone(), profile.limits);
        Ok(Self {
            pool,
            data_dir,
            db_path,
            default_data_dir,
            config_file,
            is_custom,
            settings_file,
            settings: parking_lot::RwLock::new(settings),
            clients: parking_lot::RwLock::new(clients),
            stress_jobs: Arc::new(StressJobRegistry::new()),
            cluster: crate::services::cluster::server::ClusterMaster::default(),
            cluster_slave: crate::services::cluster::client::ClusterSlave::default(),
            capacity: crate::commands::capacity::CapacityState::default(),
            ai_cancels: Arc::new(parking_lot::Mutex::new(HashMap::new())),
            test_cancels: Arc::new(parking_lot::Mutex::new(HashMap::new())),
            http_cancels: Arc::new(parking_lot::Mutex::new(HashMap::new())),
            heavy_ops: Arc::new(parking_lot::Mutex::new(())),
            capture,
            docs_cache: crate::docs::cache::DocsCache::new(),
            docs_site: crate::services::docs_site::DocsSite::default(),
            mock: crate::services::mock::server::MockServer::default(),
            realtime: crate::services::realtime::RealtimeManager::default(),
            profile: parking_lot::RwLock::new(profile),
            policy: parking_lot::RwLock::new(PolicyState {
                policy: policy.policy,
                error: policy.error,
            }),
        })
    }

    /// 无窗口初始化：CLI 与测试用，数据目录 / 配置目录由调用方给出
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn headless(
        data_dir: std::path::PathBuf,
        config_dir: std::path::PathBuf,
    ) -> Result<Self, String> {
        let db_path = data_dir.join(DB_FILE);
        Self::headless_at(data_dir, config_dir, db_path)
    }

    /// 无窗口初始化并指定数据库文件路径（CLI `--db`）
    pub fn headless_at(
        data_dir: std::path::PathBuf,
        config_dir: std::path::PathBuf,
        db_path: std::path::PathBuf,
    ) -> Result<Self, String> {
        std::fs::create_dir_all(&config_dir).map_err(|e| format!("创建配置目录失败：{e}"))?;
        let pool = open_pool_path(&db_path)?;
        let settings_file = config_dir.join("settings.json");
        let settings = read_settings(&settings_file);
        let clients = build_clients(&settings)?;
        let profile = crate::services::profile::active(&settings.performance_mode);
        let secrets = crate::secret::SecretStore::bootstrap(&data_dir, &pool)?;
        crate::secret::install(secrets);
        let policy = crate::services::policy::load(&data_dir);
        if let Err(error) = crate::services::cluster::server::recover_stale_jobs(&pool) {
            log::warn!("清理上次遗留的集群任务失败：{error}");
        }
        let capture =
            crate::capture::CaptureManager::new_with_limits(data_dir.clone(), profile.limits);
        Ok(Self {
            pool,
            db_path,
            default_data_dir: data_dir.clone(),
            data_dir,
            config_file: config_dir.join("storage.json"),
            is_custom: false,
            settings_file,
            settings: parking_lot::RwLock::new(settings),
            clients: parking_lot::RwLock::new(clients),
            stress_jobs: Arc::new(StressJobRegistry::new()),
            cluster: crate::services::cluster::server::ClusterMaster::default(),
            cluster_slave: crate::services::cluster::client::ClusterSlave::default(),
            capacity: crate::commands::capacity::CapacityState::default(),
            ai_cancels: Arc::new(parking_lot::Mutex::new(HashMap::new())),
            test_cancels: Arc::new(parking_lot::Mutex::new(HashMap::new())),
            http_cancels: Arc::new(parking_lot::Mutex::new(HashMap::new())),
            heavy_ops: Arc::new(parking_lot::Mutex::new(())),
            capture,
            docs_cache: crate::docs::cache::DocsCache::new(),
            docs_site: crate::services::docs_site::DocsSite::default(),
            mock: crate::services::mock::server::MockServer::default(),
            realtime: crate::services::realtime::RealtimeManager::default(),
            profile: parking_lot::RwLock::new(profile),
            policy: parking_lot::RwLock::new(PolicyState {
                policy: policy.policy,
                error: policy.error,
            }),
        })
    }

    /// 返回客户端句柄（内部为 Arc，克隆开销很小）
    pub fn client(&self, insecure: bool) -> reqwest::Client {
        let clients = self.clients.read();
        if insecure {
            clients.insecure.clone()
        } else {
            clients.normal.clone()
        }
    }

    /// 手工发送用客户端：不自动跟随重定向，跟随与否完全由 send_chain 决定
    pub fn manual_client(&self, insecure: bool) -> reqwest::Client {
        let clients = self.clients.read();
        if insecure {
            clients.manual_insecure.clone()
        } else {
            clients.manual.clone()
        }
    }

    /// 建连探针（普通请求计时用）
    pub fn connector_probe(&self) -> std::sync::Arc<crate::net_probe::ConnectorProbe> {
        self.clients.read().probe.clone()
    }

    /// 按请求覆盖生成一次性客户端：协议偏好、连接超时、是否跟随重定向
    ///
    /// 只在用户显式改了协议或超时时调用——一次性客户端会丢掉连接池，
    /// 换来的好处是「改动只影响这次请求」，符合 4.6「按请求网络覆盖」的语义。
    pub fn client_for_request(
        &self,
        insecure: bool,
        http1_only: bool,
        connect_timeout_ms: Option<u64>,
        follow_redirects: bool,
    ) -> Result<
        (
            reqwest::Client,
            std::sync::Arc<crate::net_probe::ConnectorProbe>,
        ),
        String,
    > {
        let probe = crate::net_probe::ConnectorProbe::new();
        let client = build_client_full(
            &self.settings.read(),
            insecure,
            connect_timeout_ms.unwrap_or(10_000),
            follow_redirects,
            None,
            Some(probe.clone()),
            http1_only,
        )?;
        Ok((client, probe))
    }

    pub fn settings(&self) -> AppSettings {
        self.settings.read().clone()
    }

    /// 当前性能档位（启动时固定，切换设置后立即重算）
    pub fn profile(&self) -> crate::services::profile::ActiveProfile {
        *self.profile.read()
    }

    /// 当前生效的企业策略（克隆，避免长期持锁）
    pub fn policy(&self) -> crate::services::policy::Policy {
        self.policy.read().policy.clone()
    }

    /// 压测并行限制：(最多任务数, 本机并发预算)
    ///
    /// v1.1.2 起不再有固定 3 个任务、2000 并发的硬编码：任务数 0 = 不限，
    /// 并发按 CPU / 内存 / 文件句柄 / 能力检测自动计算，或由用户手动指定。
    pub fn stress_limits(&self) -> (usize, u32) {
        let budget = self.budget();
        (budget.max_jobs, budget.machine_limit)
    }

    /// 计算当前生效的并发预算（自动或手动）
    pub fn budget(&self) -> ConcurrencyBudget {
        let settings = self.settings.read();
        let cores = std::thread::available_parallelism()
            .map(|value| value.get() as u32)
            .unwrap_or(4);
        let mem_gb = total_memory_bytes() as f64 / (1024.0 * 1024.0 * 1024.0);
        let fd_limit = fd_limit();
        let probe_saturation = latest_probe_saturation(&self.pool);
        let auto = auto_budget(cores, mem_gb, fd_limit, probe_saturation);
        let mode = BudgetMode::normalize(&settings.stress_budget_mode);
        let (machine_limit, limited_by) = match mode {
            BudgetMode::Manual => {
                let manual = settings.stress_max_concurrent.clamp(1, 100_000) as u32;
                (manual, "manual".to_string())
            }
            BudgetMode::Auto => (auto.limit, auto.limited_by.to_string()),
        };
        ConcurrencyBudget {
            mode: mode.as_str().to_string(),
            machine_limit,
            per_job_limit: machine_limit,
            active_jobs: self.stress_jobs.len(),
            max_jobs: settings.stress_max_jobs.max(0) as usize,
            sources: BudgetSources {
                cores,
                mem_gb,
                fd_limit,
                probe_saturation,
                by_cpu: auto.by_cpu,
                by_fd: auto.by_fd,
                by_mem: auto.by_mem,
                limited_by,
                computed_at: crate::util::now_ms(),
            },
        }
    }

    /// 更新预算模式并落盘；返回更新后的预算
    pub fn set_budget(
        &self,
        mode: &str,
        manual_limit: Option<i64>,
    ) -> Result<ConcurrencyBudget, String> {
        let mut next = self.settings();
        next.stress_budget_mode = BudgetMode::normalize(mode).as_str().to_string();
        if let Some(limit) = manual_limit {
            next.stress_max_concurrent = limit.clamp(1, 100_000);
        }
        self.update_settings(next)?;
        Ok(self.budget())
    }

    /// 策略文件解析错误（没有错误时为 None）
    pub fn policy_error(&self) -> Option<String> {
        self.policy.read().error.clone()
    }

    /// 策略文件位置
    pub fn policy_path(&self) -> PathBuf {
        crate::services::policy::policy_path(&self.data_dir)
    }

    /// 重新读取 policy.json（管理员改完文件后无需重启）
    pub fn reload_policy(&self) {
        let loaded = crate::services::policy::load(&self.data_dir);
        *self.policy.write() = PolicyState {
            policy: loaded.policy,
            error: loaded.error,
        };
    }

    /// 构建带自定义连接参数的客户端（压测等场景），代理与证书配置沿用应用设置
    ///
    /// `pool_max_idle` 为空时沿用默认的空闲连接上限；压测需按峰值并发放开，
    /// 普通请求保持默认值，避免无谓的 fd 与内存占用。
    pub fn client_with_options(
        &self,
        insecure: bool,
        connect_timeout_ms: u64,
        follow_redirects: bool,
        pool_max_idle: Option<usize>,
    ) -> Result<reqwest::Client, String> {
        build_client_full(
            &self.settings.read(),
            insecure,
            connect_timeout_ms,
            follow_redirects,
            pool_max_idle,
            None,
            false,
        )
    }

    /// 应用新的网络设置：先重建客户端，成功后再落盘，避免配置与运行态不一致
    pub fn update_settings(&self, next: AppSettings) -> Result<AppSettings, String> {
        let mut next = next;
        next.performance_mode =
            crate::services::profile::normalize_mode(&next.performance_mode).to_string();
        next.ai_permission_mode =
            crate::services::ai_policy::normalize_mode(&next.ai_permission_mode).to_string();
        // 预算与回退链归一化：负数 / NaN 归零，回退链只保留数字 id 并去重
        if !next.monthly_budget.is_finite() || next.monthly_budget < 0.0 {
            next.monthly_budget = 0.0;
        }
        next.summary_provider_id = next.summary_provider_id.max(0);
        // 并发预算：模式归一化，手动值允许 1..=100000，不再有 2000 硬上限
        next.stress_budget_mode = BudgetMode::normalize(&next.stress_budget_mode)
            .as_str()
            .to_string();
        if next.stress_max_concurrent <= 0 {
            next.stress_max_concurrent = 1000;
        }
        next.stress_max_concurrent = next.stress_max_concurrent.clamp(1, 100_000);
        next.stress_max_jobs = next.stress_max_jobs.max(0);
        next.cluster_invite_policy =
            crate::services::cluster::presence::normalize_policy(&next.cluster_invite_policy);
        let mut seen = std::collections::HashSet::new();
        next.ai_fallback_ids = next
            .ai_fallback_ids
            .split(',')
            .filter_map(|item| item.trim().parse::<i64>().ok())
            .filter(|id| *id > 0 && seen.insert(*id))
            .map(|id| id.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let clients = build_clients(&next)?;
        let profile = crate::services::profile::active(&next.performance_mode);
        let text = serde_json::to_string_pretty(&next).map_err(crate::util::err)?;
        std::fs::write(&self.settings_file, text).map_err(|e| format!("保存网络设置失败：{e}"))?;
        *self.clients.write() = clients;
        // 档位切换立即生效：抓包预算与单响应上限同步更新
        self.capture.set_limits(profile.limits);
        *self.profile.write() = profile;
        *self.settings.write() = next.clone();
        Ok(next)
    }

    /// AI-12：失败回退链（已归一化，空 = 关闭）
    pub fn ai_fallback_ids(&self) -> Vec<i64> {
        self.settings
            .read()
            .ai_fallback_ids
            .split(',')
            .filter_map(|item| item.trim().parse::<i64>().ok())
            .filter(|id| *id > 0)
            .collect()
    }

    /// 登记一个在途 AI 请求，返回它的取消标记
    pub fn ai_register(&self, id: &str) -> Arc<AtomicBool> {
        let flag = Arc::new(AtomicBool::new(false));
        self.ai_cancels.lock().insert(id.to_string(), flag.clone());
        flag
    }

    /// 请求停止：找到在途请求并置位，返回是否命中
    pub fn ai_request_cancel(&self, id: &str) -> bool {
        if let Some(flag) = self.ai_cancels.lock().get(id) {
            flag.store(true, Ordering::SeqCst);
            true
        } else {
            false
        }
    }

    /// 请求结束后释放登记，避免长期占用
    pub fn ai_release(&self, id: &str) {
        self.ai_cancels.lock().remove(id);
    }

    /// 登记一个在途 HTTP 请求，返回取消位（4.6）
    pub fn http_register(&self, id: &str) -> Arc<AtomicBool> {
        let flag = Arc::new(AtomicBool::new(false));
        self.http_cancels
            .lock()
            .insert(id.to_string(), flag.clone());
        flag
    }

    /// 请求停止某个在途 HTTP 请求，返回是否命中
    pub fn http_cancel(&self, id: &str) -> bool {
        match self.http_cancels.lock().get(id) {
            Some(flag) => {
                flag.store(true, Ordering::SeqCst);
                true
            }
            None => false,
        }
    }

    pub fn http_release(&self, id: &str) {
        self.http_cancels.lock().remove(id);
    }

    /// 登记一次套件运行的取消标记
    pub fn test_register(&self, run_id: i64) -> Arc<AtomicBool> {
        let flag = Arc::new(AtomicBool::new(false));
        self.test_cancels.lock().insert(run_id, flag.clone());
        flag
    }

    /// 请求停止某次套件运行，返回是否命中
    pub fn test_cancel_run(&self, run_id: i64) -> bool {
        match self.test_cancels.lock().get(&run_id) {
            Some(flag) => {
                flag.store(true, Ordering::SeqCst);
                true
            }
            None => false,
        }
    }

    pub fn test_release(&self, run_id: i64) {
        self.test_cancels.lock().remove(&run_id);
    }
}

/// auto 模式并发预算的推导结果（三项上界一并返回，设置页要逐项展示）
#[derive(Debug, Clone, Copy)]
struct AutoBudget {
    by_cpu: u32,
    by_fd: u32,
    by_mem: u32,
    limit: u32,
    limited_by: &'static str,
}

/// 每个 CPU 核折算的并发预算
const BUDGET_PER_CORE: u32 = 512;
/// 每 GB 内存折算的并发预算
const BUDGET_PER_GB: f64 = 384.0;
/// 文件句柄上限折算成并发预算的比例
const BUDGET_FD_RATIO: f64 = 0.75;
/// Windows 没有 rlimit：按每个进程可用的句柄/端口资源给一个保守上限
#[cfg(not(unix))]
const WINDOWS_FD_BUDGET: u64 = 16_384;

/// 按 CPU / 句柄 / 内存推导本机并发预算
///
/// 抽成纯函数是为了能覆盖「句柄软上限只有 256」这类本机复现不了的组合。
fn auto_budget(
    cores: u32,
    mem_gb: f64,
    fd_limit: u64,
    probe_saturation: Option<u32>,
) -> AutoBudget {
    let by_cpu = cores.saturating_mul(BUDGET_PER_CORE);
    // 每个在途连接占 1 个 fd，另要给 SQLite / 日志 / WebView / 抓包留余量，
    // 所以只把句柄上限的 75% 折算成并发预算（100% 会让并发几乎吃满句柄，
    // 60% 则把 macOS 抬高上限后的收益又还了回去）
    let by_fd = (fd_limit as f64 * BUDGET_FD_RATIO) as u32;
    // 每个并发连接约 1MB 内存（TLS 缓冲 + 请求/响应），另给应用本体留足空间；
    // 32GB 机器约 12.3k 并发，与 12 线程 CPU 项 6.1k 相比不会成为瓶颈
    let by_mem = (mem_gb * BUDGET_PER_GB) as u32;
    let candidate = by_cpu.max(probe_saturation.unwrap_or(0));
    let upper = by_fd.min(by_mem).min(100_000);
    let limit = candidate.max(1000).min(upper.max(1));
    let limited_by = if limit == by_fd {
        "fd"
    } else if limit == by_mem {
        "mem"
    } else if probe_saturation.is_some_and(|value| value >= limit && value > by_cpu) {
        "probe"
    } else {
        "cpu"
    };
    AutoBudget {
        by_cpu,
        by_fd,
        by_mem,
        limit,
        limited_by,
    }
}

/// 本机物理内存（字节）
pub fn total_memory_bytes() -> u64 {
    let mut system = sysinfo::System::new();
    system.refresh_memory();
    system.total_memory()
}

/// 压测并发需要的句柄目标值
///
/// 与 `budget()` 里 10 万的预算上限同口径，同时避免把 `rlim_cur` 顶成
/// `RLIM_INFINITY`（部分老接口遇到无穷值会直接失败）。
#[cfg(unix)]
const FD_TARGET: u64 = 100_000;

/// 启动时把 `RLIMIT_NOFILE` 软上限抬到硬上限（不超过 `FD_TARGET`）
///
/// 压测每个 keep-alive 连接占 1 个 fd，而 macOS 上从 Finder / launchd 启动的
/// 进程软上限可能只有 256：`budget()` 的 `by_fd` 直接由它算出，不抬就会把整机
/// 并发预算压到 204，用户只会在设置页看到「受 fd 限制」却查不出原因。
///
/// 只升不降；失败（沙箱 / 容器禁止 setrlimit）时保留原值并记一条 warn。
#[cfg(unix)]
pub fn raise_fd_limit() {
    // SAFETY: 只读写本进程的 rlimit，结构体由本函数初始化后传给内核
    unsafe {
        let mut limit: libc::rlimit = std::mem::zeroed();
        if libc::getrlimit(libc::RLIMIT_NOFILE, &mut limit) != 0 {
            log::warn!("读取 RLIMIT_NOFILE 失败，沿用当前软上限");
            return;
        }
        let hard = limit.rlim_max;
        let soft = limit.rlim_cur;
        let target = FD_TARGET as libc::rlim_t;
        let want = if hard == libc::RLIM_INFINITY {
            target
        } else {
            hard.min(target)
        };
        if soft >= want {
            // 硬上限本身就很低（部分沙箱/容器）时无解，但必须留下可查的线索：
            // 否则用户只会看到「受 fd 限制 153 并发」却不知道为什么
            if soft < FD_TARGET {
                log::warn!(
                    "RLIMIT_NOFILE 软上限 {soft} 已达硬上限（{hard}），无法提升；\
                     压测并发预算将被限制在约 {} 并发",
                    (soft as f64 * BUDGET_FD_RATIO) as u32
                );
            }
            return;
        }
        let next = libc::rlimit {
            rlim_cur: want,
            rlim_max: hard,
        };
        if libc::setrlimit(libc::RLIMIT_NOFILE, &next) != 0 {
            let error = std::io::Error::last_os_error();
            log::warn!(
                "提升 RLIMIT_NOFILE 失败（{error}），压测并发预算将被 fd 软上限 {soft} 限制"
            );
            return;
        }
        log::info!("RLIMIT_NOFILE 软上限已从 {soft} 提升到 {want}");
    }
}

/// Windows 没有 fd 概念，无需处理
#[cfg(not(unix))]
pub fn raise_fd_limit() {}

/// 进程可打开文件数上限；Windows 没有 fd 概念，按 16384 兜底
fn fd_limit() -> u64 {
    #[cfg(unix)]
    {
        // SAFETY: getrlimit 只写入调用方提供的结构体，参数合法
        unsafe {
            let mut limit: libc::rlimit = std::mem::zeroed();
            if libc::getrlimit(libc::RLIMIT_NOFILE, &mut limit) == 0 {
                let value = limit.rlim_cur as u64;
                // RLIM_INFINITY 在部分平台是极大值，按 100 万兜底，避免预算失真
                if value > 0 && value < 1_000_000 {
                    return value;
                }
                if value >= 1_000_000 {
                    return 1_000_000;
                }
            }
        }
        2560
    }
    #[cfg(not(unix))]
    {
        // Windows 没有 RLIMIT_NOFILE：按每个进程可用的句柄预算兜底。
        // 之前用 8192 会让 12 线程机器只剩 4915 并发，白白浪费 CPU 预算。
        WINDOWS_FD_BUDGET
    }
}

/// 最近一次能力检测的饱和并发（没有结果时为 None）
fn latest_probe_saturation(pool: &DbPool) -> Option<u32> {
    let conn = pool.get().ok()?;
    crate::commands::capacity::latest_result(&conn)
        .ok()
        .flatten()
        .map(|result| result.saturation_concurrent)
        .filter(|value| *value > 0)
}

/// 把旧版默认目录的数据库文件复制到新目录（只在目标还没有数据库时执行，不覆盖任何数据）
fn migrate_legacy_dir(legacy_dir: &Path, target_dir: &Path, legacy_db_file: &str) {
    let target_db = target_dir.join(DB_FILE);
    if target_db.exists() {
        return;
    }
    let legacy_db = legacy_dir.join(legacy_db_file);
    if !legacy_db.exists() {
        return;
    }
    if let Err(error) = std::fs::create_dir_all(target_dir) {
        log::warn!("创建新的数据目录失败：{error}");
        return;
    }
    if let Err(error) = std::fs::copy(&legacy_db, &target_db) {
        log::warn!("迁移旧数据库失败：{error}");
        return;
    }
    // WAL / SHM 一起带上，避免丢失尚未合并的事务
    for suffix in ["-wal", "-shm"] {
        let from = legacy_dir.join(format!("{legacy_db_file}{suffix}"));
        if from.exists() {
            let _ = std::fs::copy(&from, target_dir.join(format!("{DB_FILE}{suffix}")));
        }
    }
    log::info!(
        "已把旧数据目录 {:?} 中的 {legacy_db_file} 迁移到 {:?}，原目录保留可自行删除",
        legacy_dir,
        target_dir
    );
}

/// 把旧 identifier 配置目录里的 json（storage / settings / 窗口状态）复制到新目录
/// 只在目标文件不存在时复制，不覆盖任何已有配置
fn migrate_legacy_config(legacy_dir: &Path, target_dir: &Path) {
    if !legacy_dir.is_dir() {
        return;
    }
    if let Err(error) = std::fs::create_dir_all(target_dir) {
        log::warn!("创建新的配置目录失败：{error}");
        return;
    }
    let entries = match std::fs::read_dir(legacy_dir) {
        Ok(entries) => entries,
        Err(error) => {
            log::warn!("读取旧配置目录失败：{error}");
            return;
        }
    };
    let mut copied = 0;
    for entry in entries.flatten() {
        let from = entry.path();
        if !from.is_file() || from.extension().and_then(|v| v.to_str()) != Some("json") {
            continue;
        }
        let Some(name) = from.file_name() else {
            continue;
        };
        let to = target_dir.join(name);
        if to.exists() {
            continue;
        }
        match std::fs::copy(&from, &to) {
            Ok(_) => copied += 1,
            Err(error) => log::warn!("迁移旧配置 {:?} 失败：{error}", from),
        }
    }
    if copied > 0 {
        log::info!(
            "已从旧配置目录 {:?} 迁移 {copied} 个配置文件到 {:?}",
            legacy_dir,
            target_dir
        );
    }
}

fn open_pool(data_dir: &Path) -> Result<DbPool, String> {
    std::fs::create_dir_all(data_dir).map_err(crate::util::err)?;
    let db_path = data_dir.join(DB_FILE);
    // 若该目录下只有旧数据库（fbinhttp.db / finkmeter.db），重命名为新文件名
    if !db_path.exists() {
        for legacy_name in LEGACY_DB_FILES {
            let legacy = data_dir.join(legacy_name);
            if legacy.exists() {
                log::info!("检测到旧数据库 {legacy_name}，重命名为 {DB_FILE}");
                match std::fs::rename(&legacy, &db_path) {
                    Ok(()) => {
                        // WAL / SHM 跟着主库一起改名，否则最近未 checkpoint 的事务会丢
                        for suffix in ["-wal", "-shm"] {
                            let from = data_dir.join(format!("{legacy_name}{suffix}"));
                            if from.exists() {
                                let to = data_dir.join(format!("{DB_FILE}{suffix}"));
                                if to.exists() {
                                    let _ = std::fs::remove_file(&to);
                                }
                                if let Err(error) = std::fs::rename(&from, &to) {
                                    log::warn!("旧数据库附属文件 {suffix} 改名失败：{error}");
                                }
                            }
                        }
                    }
                    Err(error) => log::warn!("旧数据库重命名失败：{error}"),
                }
                break;
            }
        }
    }
    let manager = SqliteConnectionManager::file(&db_path)
        .with_init(|conn| conn.execute_batch(&crate::db::connection_pragmas()));
    let pool = Pool::builder()
        .max_size(8)
        .build(manager)
        .map_err(crate::util::err)?;
    // 结构升级前先留一份快照（失败即中止，避免升级到一半无法回退）
    let db_dir = db_path
        .parent()
        .map(|dir| dir.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));
    crate::backup::snapshot_before_migrate(&pool, &db_dir)?;
    crate::db::init(&pool)?;
    Ok(pool)
}

/// 直接按数据库文件路径建池（CLI `--db` 可能与数据目录内的默认文件名不同）
fn open_pool_path(db_path: &Path) -> Result<DbPool, String> {
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent).map_err(crate::util::err)?;
    }
    let manager = SqliteConnectionManager::file(db_path)
        .with_init(|conn| conn.execute_batch(&crate::db::connection_pragmas()));
    let pool = Pool::builder()
        .max_size(8)
        .build(manager)
        .map_err(crate::util::err)?;
    let db_dir = db_path
        .parent()
        .map(|dir| dir.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));
    crate::backup::snapshot_before_migrate(&pool, &db_dir)?;
    crate::db::init(&pool)?;
    Ok(pool)
}

/// 解析本次启动会使用的数据目录（日志初始化需要它，早于 AppState 构建）
pub fn resolve_data_dir(app: &tauri::AppHandle) -> PathBuf {
    let base = app.path().data_dir().unwrap_or_else(|_| PathBuf::from("."));
    let default_dir = base.join("AstroMan");
    let config_dir = app
        .path()
        .app_config_dir()
        .unwrap_or_else(|_| default_dir.clone());
    match read_storage_config(&config_dir.join("storage.json")) {
        Some(dir) if dir != default_dir && dir.exists() => dir,
        _ => default_dir,
    }
}

/// 读取已保存的自定义数据目录
pub fn read_storage_config(config_file: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(config_file).ok()?;
    let config: StorageConfig = serde_json::from_str(&text).ok()?;
    if config.data_dir.trim().is_empty() {
        return None;
    }
    Some(PathBuf::from(config.data_dir))
}

fn read_settings(path: &Path) -> AppSettings {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn build_clients(settings: &AppSettings) -> Result<HttpClients, String> {
    let probe = crate::net_probe::ConnectorProbe::new();
    Ok(HttpClients {
        normal: build_client_full(
            settings,
            false,
            10_000,
            true,
            None,
            Some(probe.clone()),
            false,
        )?,
        insecure: build_client_full(
            settings,
            true,
            10_000,
            true,
            None,
            Some(probe.clone()),
            false,
        )?,
        manual: build_client_full(
            settings,
            false,
            10_000,
            false,
            None,
            Some(probe.clone()),
            false,
        )?,
        manual_insecure: build_client_full(
            settings,
            true,
            10_000,
            false,
            None,
            Some(probe.clone()),
            false,
        )?,
        probe,
    })
}

/// 按自定义连接参数构建客户端
///
/// 压测等场景需要单独覆盖连接超时与重定向策略，但仍沿用应用的代理与客户端
/// 证书配置，保证与手工请求走同一条网络链路。
#[allow(clippy::too_many_arguments)]
fn build_client_full(
    settings: &AppSettings,
    insecure: bool,
    connect_timeout_ms: u64,
    follow_redirects: bool,
    pool_max_idle: Option<usize>,
    probe: Option<std::sync::Arc<crate::net_probe::ConnectorProbe>>,
    http1_only: bool,
) -> Result<reqwest::Client, String> {
    /// 默认空闲连接上限，与历史行为保持一致
    const DEFAULT_POOL_MAX_IDLE: usize = 32;
    // 压测客户端（传了 pool_max_idle）按峰值并发放开空闲连接；普通客户端保持默认。
    // 上限从 4096 提到 10 万：12 线程机器预算 6144，4096 会把连接池变成新瓶颈。
    let stress_client = pool_max_idle.is_some();
    let idle_limit = pool_max_idle
        .unwrap_or(DEFAULT_POOL_MAX_IDLE)
        .clamp(1, 100_000);
    let mut builder = reqwest::Client::builder()
        .user_agent(concat!("AstroMan/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_millis(
            connect_timeout_ms.clamp(500, 120_000),
        ))
        .pool_max_idle_per_host(idle_limit)
        .danger_accept_invalid_certs(insecure)
        .redirect(if follow_redirects {
            reqwest::redirect::Policy::limited(10)
        } else {
            reqwest::redirect::Policy::none()
        });

    if stress_client {
        // 压测的并发请求都是小包，Nagle 会把多个请求攒成 40ms 的延迟：
        // 高并发下这直接表现为 QPS 上不去、P99 抖动。仅压测客户端开启。
        builder = builder.tcp_nodelay(true);
    }

    if http1_only {
        builder = builder.http1_only();
    }
    if let Some(probe) = probe {
        builder = builder.connector_layer(crate::net_probe::ProbeLayer::new(probe));
    }

    if settings.proxy_enabled {
        let proxy_url = settings.proxy_url.trim();
        if proxy_url.is_empty() {
            return Err("已开启代理，但代理地址为空".to_string());
        }
        let proxy = reqwest::Proxy::all(proxy_url).map_err(|e| {
            format!(
                "代理地址无法解析（示例：http://127.0.0.1:7890 或 socks5h://127.0.0.1:1080）：{e}"
            )
        })?;
        builder = builder.proxy(proxy);
    }

    let cert_path = settings.cert_path.trim();
    if !cert_path.is_empty() {
        let der = std::fs::read(cert_path)
            .map_err(|e| format!("读取客户端证书失败（{cert_path}）：{e}"))?;
        let identity = reqwest::Identity::from_pkcs12_der(&der, &settings.cert_password)
            .map_err(|e| format!("客户端证书解析失败，仅支持 PKCS#12 格式（.pfx / .p12）：{e}"))?;
        builder = builder.identity(identity);
    }

    builder.build().map_err(crate::util::err)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(tag: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!("astroman-{tag}-{}-{stamp}", std::process::id()))
    }

    #[test]
    fn migrates_legacy_db_with_wal_and_shm() {
        let root = temp_root("db");
        let legacy = root.join("FbinHttp");
        let target = root.join("AstroMan");
        std::fs::create_dir_all(&legacy).unwrap();
        std::fs::write(legacy.join("fbinhttp.db"), b"main").unwrap();
        std::fs::write(legacy.join("fbinhttp.db-wal"), b"wal").unwrap();
        std::fs::write(legacy.join("fbinhttp.db-shm"), b"shm").unwrap();

        migrate_legacy_dir(&legacy, &target, "fbinhttp.db");

        assert_eq!(std::fs::read(target.join("astroman.db")).unwrap(), b"main");
        assert_eq!(
            std::fs::read(target.join("astroman.db-wal")).unwrap(),
            b"wal"
        );
        assert_eq!(
            std::fs::read(target.join("astroman.db-shm")).unwrap(),
            b"shm"
        );
        // 目标已存在时不覆盖
        std::fs::write(legacy.join("fbinhttp.db"), b"new").unwrap();
        migrate_legacy_dir(&legacy, &target, "fbinhttp.db");
        assert_eq!(std::fs::read(target.join("astroman.db")).unwrap(), b"main");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn migrates_config_json_without_overwrite() {
        let root = temp_root("cfg");
        let legacy = root.join("com.fbinhttp.app");
        let target = root.join("com.astroman.app");
        std::fs::create_dir_all(&legacy).unwrap();
        std::fs::write(legacy.join("storage.json"), "old").unwrap();
        std::fs::write(legacy.join("settings.json"), "old-settings").unwrap();
        migrate_legacy_config(&legacy, &target);
        assert_eq!(
            std::fs::read_to_string(target.join("storage.json")).unwrap(),
            "old"
        );
        assert_eq!(
            std::fs::read_to_string(target.join("settings.json")).unwrap(),
            "old-settings"
        );
        // 已有值不被旧配置覆盖
        std::fs::write(legacy.join("storage.json"), "older").unwrap();
        std::fs::write(target.join("storage.json"), "newer").unwrap();
        migrate_legacy_config(&legacy, &target);
        assert_eq!(
            std::fs::read_to_string(target.join("storage.json")).unwrap(),
            "newer"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    fn draft(name: &str, target_key: &str, concurrent: u32) -> StressJobDraft {
        StressJobDraft {
            name: name.to_string(),
            target: "http://127.0.0.1/api".to_string(),
            target_key: target_key.to_string(),
            plan_id: None,
            request_id: None,
            concurrent,
        }
    }

    #[test]
    fn registry_enforces_job_limit_and_total_concurrency() {
        let registry = StressJobRegistry::new();
        let (first, _, _) = registry
            .try_reserve(draft("a", "GET http://h:80/a", 400), 2, 1000, false)
            .unwrap();
        let (second, _, _) = registry
            .try_reserve(draft("b", "GET http://h:80/b", 400), 2, 1000, false)
            .unwrap();
        // 任务数上限
        let error = registry
            .try_reserve(draft("c", "GET http://h:80/c", 100), 2, 1000, false)
            .unwrap_err();
        assert!(error.message.contains("最多同时运行"));
        // 跨任务并发之和上限
        registry.finish(first);
        let error = registry
            .try_reserve(draft("c", "GET http://h:80/c", 700), 2, 1000, false)
            .unwrap_err();
        assert!(error.message.contains("超过本机预算"));
        registry.finish(second);
        assert!(registry.is_empty());
    }

    #[test]
    fn registry_rejects_same_target_and_ignore_keeps_trace() {
        let registry = StressJobRegistry::new();
        let (_, _, overlaps) = registry
            .try_reserve(draft("a", "GET http://h:80/api", 100), 3, 1000, false)
            .unwrap();
        assert!(overlaps.is_empty());
        let error = registry
            .try_reserve(draft("b", "GET http://h:80/api", 100), 3, 1000, false)
            .unwrap_err();
        assert!(error.message.contains("目标接口正在被任务"));
        // 忽略同目标时允许启动，并把冲突任务 id 交给调用方留痕
        let (id, cancel, overlaps) = registry
            .try_reserve(draft("b", "GET http://h:80/api", 100), 3, 1000, true)
            .unwrap();
        assert_eq!(overlaps.len(), 1);
        assert!(!cancel.load(std::sync::atomic::Ordering::SeqCst));
        // 同 host 不同 path 不冲突
        let (other, _, overlaps) = registry
            .try_reserve(draft("c", "GET http://h:80/users", 100), 3, 1000, false)
            .unwrap();
        assert!(overlaps.is_empty());
        // 按任务取消不影响其他任务
        assert!(registry.cancel(id));
        registry.finish(id);
        assert_eq!(registry.snapshot().len(), 2);
        registry.finish(other);
    }

    #[test]
    fn registry_warns_same_host_without_blocking() {
        let registry = StressJobRegistry::new();
        let (a, _, _) = registry
            .try_reserve(
                draft("a", "GET https://api.example.com:8443/users", 100),
                3,
                1000,
                false,
            )
            .unwrap();
        let (b, _, _) = registry
            .try_reserve(
                draft("b", "POST https://api.example.com:8443/orders", 100),
                3,
                1000,
                false,
            )
            .unwrap();
        let (c, _, _) = registry
            .try_reserve(
                draft("c", "GET https://other.example.com/users", 100),
                3,
                1000,
                false,
            )
            .unwrap();
        // 同 scheme/host/port 不同 path 或方法只警告：列出对方任务
        let others = registry.same_host_others("GET https://api.example.com:8443/orders");
        assert_eq!(
            others.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
            vec![a, b]
        );
        // 完全相同的目标由 try_reserve 拦截，不重复出现在警告里
        assert!(registry
            .same_host_others("GET https://api.example.com:8443/users")
            .iter()
            .all(|(id, _)| *id != a));
        // 不同主机不告警
        assert!(registry
            .same_host_others("GET https://third.example.com/users")
            .is_empty());
        registry.finish(a);
        registry.finish(b);
        registry.finish(c);
    }

    #[test]
    fn registry_tracks_qps_and_window_label() {
        let registry = StressJobRegistry::new();
        let (id, _, _) = registry
            .try_reserve(draft("a", "GET http://h:80/a", 100), 3, 1000, false)
            .unwrap();
        registry.set_qps(id, 1234.6);
        registry.set_window_label(id, "stress-1");
        let snapshot = registry.snapshot();
        assert_eq!(snapshot[0].qps, 1235);
        assert_eq!(registry.window_label(id).as_deref(), Some("stress-1"));
        registry.finish(id);
    }

    #[test]
    fn auto_budget_is_capped_by_fd_ratio() {
        // 10 核 / 32GB / 句柄 10240：CPU 5120、句柄 7680、内存 12288 → CPU 先到顶，
        // 但句柄项必须算对，否则低配机器上会误判瓶颈归属
        let budget = auto_budget(10, 32.0, 10240, None);
        assert_eq!(budget.by_cpu, 5120);
        assert_eq!(budget.by_fd, 7680);
        assert_eq!(budget.by_mem, 12288);
        assert_eq!(budget.limit, 5120);
        assert_eq!(budget.limited_by, "cpu");
    }

    #[test]
    fn auto_budget_flags_small_fd_limit_instead_of_silently_shrinking() {
        // macOS GUI 进程若继承到 256 的软上限：预算只能是 192，且必须报 fd，
        // 用户才看得出「为什么这台机器只能跑一百多并发」
        let budget = auto_budget(10, 32.0, 256, None);
        assert_eq!(budget.by_fd, 192);
        assert_eq!(budget.limit, 192);
        assert_eq!(budget.limited_by, "fd");
    }

    #[test]
    fn auto_budget_prefers_memory_when_fd_is_plentiful() {
        // 句柄抬到 10 万后，64 核 / 32GB 机器的瓶颈回到内存（12288）
        let budget = auto_budget(64, 32.0, 100_000, None);
        assert_eq!(budget.by_mem, 12288);
        assert_eq!(budget.limit, 12288);
        assert_eq!(budget.limited_by, "mem");
    }

    #[test]
    fn auto_budget_can_be_lifted_by_probe_saturation() {
        // 能力检测测出更高饱和并发时，允许超过 CPU 估算，但不越过内存上限
        let budget = auto_budget(8, 64.0, 65_536, Some(9000));
        assert_eq!(budget.by_cpu, 4096);
        assert_eq!(budget.limit, 9000);
        assert_eq!(budget.limited_by, "probe");
    }

    #[test]
    fn auto_budget_never_exceeds_hard_cap() {
        // 256 核 / 512GB / 句柄 100 万：三项都远超 10 万，仍要卡在 10 万
        let budget = auto_budget(256, 512.0, 1_000_000, None);
        assert_eq!(budget.limit, 100_000);
    }

    #[cfg(unix)]
    #[test]
    fn raise_fd_limit_is_idempotent_and_never_lowers() {
        let before = fd_limit();
        raise_fd_limit();
        let raised = fd_limit();
        assert!(raised >= before, "软上限被调低了：{before} -> {raised}");
        raise_fd_limit();
        assert_eq!(fd_limit(), raised, "重复调用应保持幂等");
        // 抬高后预算的 fd 项必须跟着变大或持平，不能在 256 上原地不动
        let budget = auto_budget(10, 32.0, raised, None);
        assert!(budget.by_fd >= (before as f64 * BUDGET_FD_RATIO) as u32);
    }

    /// 复现 macOS GUI 的真实处境：软上限 256、硬上限更高
    ///
    /// 必须放到子进程里做：拉低本进程软上限会让并行跑的其他用例偶发 EMFILE。
    #[cfg(unix)]
    #[test]
    fn raise_fd_limit_lifts_a_lowered_soft_limit() {
        const CHILD_ENV: &str = "ASTROMAN_FD_CHILD";
        const CHILD_TEST: &str = "state::tests::raise_fd_limit_lifts_a_lowered_soft_limit";

        if std::env::var(CHILD_ENV).is_ok() {
            // SAFETY: 只改本进程 rlimit
            let hard = unsafe {
                let mut limit: libc::rlimit = std::mem::zeroed();
                libc::getrlimit(libc::RLIMIT_NOFILE, &mut limit);
                limit.rlim_max
            };
            if hard > 256 {
                // SAFETY: hard 已确认大于 256，压低软上限一定合法
                unsafe {
                    let next = libc::rlimit {
                        rlim_cur: 256,
                        rlim_max: hard,
                    };
                    assert_eq!(libc::setrlimit(libc::RLIMIT_NOFILE, &next), 0);
                }
            }
            raise_fd_limit();
            let soft = fd_limit();
            if hard > 256 {
                assert!(
                    soft >= 2560,
                    "软上限没有被抬起，仍是 {soft}；压测预算会被压到 {}",
                    auto_budget(10, 32.0, soft, None).limit
                );
            }
            return;
        }

        let exe = std::env::current_exe().expect("current_exe");
        let status = std::process::Command::new(exe)
            .args(["--exact", CHILD_TEST, "--nocapture"])
            .env(CHILD_ENV, "1")
            .status()
            .expect("spawn child");
        assert!(status.success(), "子进程未能把软上限抬起来");
    }
}
