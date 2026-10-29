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
}

struct HttpClients {
    normal: reqwest::Client,
    insecure: reqwest::Client,
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
                    (default_data_dir.clone(), open_pool(&default_data_dir)?, false)
                }
            },
            _ => (default_data_dir.clone(), open_pool(&default_data_dir)?, false),
        };

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

    pub fn settings(&self) -> AppSettings {
        self.settings.read().clone()
    }

    /// 应用新的网络设置：先重建客户端，成功后再落盘，避免配置与运行态不一致
    pub fn update_settings(&self, next: AppSettings) -> Result<AppSettings, String> {
        let clients = build_clients(&next)?;
        let text = serde_json::to_string_pretty(&next).map_err(crate::util::err)?;
        std::fs::write(&self.settings_file, text)
            .map_err(|e| format!("保存网络设置失败：{e}"))?;
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
        let Some(name) = from.file_name() else { continue };
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
    crate::db::init(&pool)?;
    Ok(pool)
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
    Ok(HttpClients {
        normal: build_client_with(settings, false)?,
        insecure: build_client_with(settings, true)?,
    })
}

fn build_client_with(settings: &AppSettings, insecure: bool) -> Result<reqwest::Client, String> {
    let mut builder = reqwest::Client::builder()
        .user_agent(concat!("AstroMan/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(10))
        .pool_max_idle_per_host(32)
        .danger_accept_invalid_certs(insecure);

    if settings.proxy_enabled {
        let proxy_url = settings.proxy_url.trim();
        if proxy_url.is_empty() {
            return Err("已开启代理，但代理地址为空".to_string());
        }
        let proxy = reqwest::Proxy::all(proxy_url)
            .map_err(|e| format!("代理地址无法解析（示例：http://127.0.0.1:7890 或 socks5h://127.0.0.1:1080）：{e}"))?;
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
        assert_eq!(std::fs::read(target.join("astroman.db-wal")).unwrap(), b"wal");
        assert_eq!(std::fs::read(target.join("astroman.db-shm")).unwrap(), b"shm");
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
        assert_eq!(std::fs::read_to_string(target.join("storage.json")).unwrap(), "old");
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
