use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;
use tauri::Manager;

pub type DbPool = Pool<SqliteConnectionManager>;

/// 数据目录指向文件（保存在固定的系统配置目录，不随数据目录移动）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageConfig {
    pub data_dir: String,
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
    pub http: reqwest::Client,
    pub http_insecure: reqwest::Client,
    /// 压测运行中标记（同一时间只允许一个压测任务）
    pub stress_running: Arc<AtomicBool>,
    /// 压测停止信号
    pub stress_cancel: Arc<AtomicBool>,
}

impl AppState {
    pub fn new(app: &tauri::AppHandle) -> Result<Self, String> {
        let default_data_dir = app
            .path()
            .data_dir()
            .map_err(crate::util::err)?
            .join("FinkMeter");
        let config_dir = app.path().app_config_dir().map_err(crate::util::err)?;
        std::fs::create_dir_all(&config_dir).map_err(crate::util::err)?;
        let config_file = config_dir.join("storage.json");

        let custom_dir = read_storage_config(&config_file);
        let (data_dir, pool, is_custom) = match custom_dir {
            Some(dir) if dir != default_data_dir => match open_pool(&dir) {
                Ok(pool) => (dir, pool, true),
                Err(error) => {
                    // 自定义目录不可用时回退到默认目录，避免应用无法启动
                    log::warn!("自定义数据目录不可用（{error}），回退到默认目录");
                    let _ = std::fs::remove_file(&config_file);
                    (default_data_dir.clone(), open_pool(&default_data_dir)?, false)
                }
            },
            _ => (default_data_dir.clone(), open_pool(&default_data_dir)?, false),
        };

        let db_path = data_dir.join("finkmeter.db");
        let http = build_client(false)?;
        let http_insecure = build_client(true)?;

        Ok(Self {
            pool,
            data_dir,
            db_path,
            default_data_dir,
            config_file,
            is_custom,
            http,
            http_insecure,
            stress_running: Arc::new(AtomicBool::new(false)),
            stress_cancel: Arc::new(AtomicBool::new(false)),
        })
    }

    pub fn client(&self, insecure: bool) -> &reqwest::Client {
        if insecure {
            &self.http_insecure
        } else {
            &self.http
        }
    }
}

fn open_pool(data_dir: &Path) -> Result<DbPool, String> {
    std::fs::create_dir_all(data_dir).map_err(crate::util::err)?;
    let db_path = data_dir.join("finkmeter.db");
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

fn build_client(insecure: bool) -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent(concat!("FinkMeter/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(10))
        .pool_max_idle_per_host(32)
        .danger_accept_invalid_certs(insecure)
        .build()
        .map_err(crate::util::err)
}
