use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
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
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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
}

struct HttpClients {
    normal: reqwest::Client,
    insecure: reqwest::Client,
    /// 建连探针：用来判断本次请求是「新建连接」还是「复用连接」
    probe: std::sync::Arc<crate::net_probe::ConnectorProbe>,
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
    /// 压测运行中标记（同一时间只允许一个压测任务）
    pub stress_running: Arc<AtomicBool>,
    /// 压测停止信号
    pub stress_cancel: Arc<AtomicBool>,
    /// AI 在途请求的取消标记：requestId → 取消位
    pub ai_cancels: Arc<parking_lot::Mutex<HashMap<String, Arc<AtomicBool>>>>,
    /// 套件运行的取消标记：runId → 取消位（按运行隔离，不用全局单例）
    pub test_cancels: Arc<parking_lot::Mutex<HashMap<i64, Arc<AtomicBool>>>>,
    /// 在途 HTTP 请求的取消标记：requestId → 取消位（4.6 取消能力）
    pub http_cancels: Arc<parking_lot::Mutex<HashMap<String, Arc<AtomicBool>>>>,
    /// 重任务互斥锁：备份 / VACUUM / integrity_check 与压测互斥（4.0.2）
    pub heavy_ops: Arc<parking_lot::Mutex<()>>,
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
            stress_running: Arc::new(AtomicBool::new(false)),
            stress_cancel: Arc::new(AtomicBool::new(false)),
            ai_cancels: Arc::new(parking_lot::Mutex::new(HashMap::new())),
            test_cancels: Arc::new(parking_lot::Mutex::new(HashMap::new())),
            http_cancels: Arc::new(parking_lot::Mutex::new(HashMap::new())),
            heavy_ops: Arc::new(parking_lot::Mutex::new(())),
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
        let secrets = crate::secret::SecretStore::bootstrap(&data_dir, &pool)?;
        crate::secret::install(secrets);
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
            stress_running: Arc::new(AtomicBool::new(false)),
            stress_cancel: Arc::new(AtomicBool::new(false)),
            ai_cancels: Arc::new(parking_lot::Mutex::new(HashMap::new())),
            test_cancels: Arc::new(parking_lot::Mutex::new(HashMap::new())),
            http_cancels: Arc::new(parking_lot::Mutex::new(HashMap::new())),
            heavy_ops: Arc::new(parking_lot::Mutex::new(())),
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
        let clients = build_clients(&next)?;
        let text = serde_json::to_string_pretty(&next).map_err(crate::util::err)?;
        std::fs::write(&self.settings_file, text).map_err(|e| format!("保存网络设置失败：{e}"))?;
        *self.clients.write() = clients;
        *self.settings.write() = next.clone();
        Ok(next)
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
    let manager = SqliteConnectionManager::file(&db_path).with_init(|conn| {
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA foreign_keys = ON;
             PRAGMA busy_timeout = 5000;",
        )
    });
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
    let manager = SqliteConnectionManager::file(db_path).with_init(|conn| {
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA foreign_keys = ON;
             PRAGMA busy_timeout = 5000;",
        )
    });
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
    let mut builder = reqwest::Client::builder()
        .user_agent(concat!("AstroMan/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_millis(
            connect_timeout_ms.clamp(500, 120_000),
        ))
        .pool_max_idle_per_host(
            pool_max_idle
                .unwrap_or(DEFAULT_POOL_MAX_IDLE)
                .clamp(1, 4096),
        )
        .danger_accept_invalid_certs(insecure)
        .redirect(if follow_redirects {
            reqwest::redirect::Policy::limited(10)
        } else {
            reqwest::redirect::Policy::none()
        });

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
}
