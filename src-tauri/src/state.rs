use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;
use tauri::Manager;

pub type DbPool = Pool<SqliteConnectionManager>;

pub struct AppState {
    pub pool: DbPool,
    pub data_dir: PathBuf,
    pub db_path: PathBuf,
    pub http: reqwest::Client,
    pub http_insecure: reqwest::Client,
    /// 压测运行中标记（同一时间只允许一个压测任务）
    pub stress_running: Arc<AtomicBool>,
    /// 压测停止信号
    pub stress_cancel: Arc<AtomicBool>,
}

impl AppState {
    pub fn new(app: &tauri::AppHandle) -> Result<Self, String> {
        let data_dir = app
            .path()
            .data_dir()
            .map_err(crate::util::err)?
            .join("FinkMeter");
        std::fs::create_dir_all(&data_dir).map_err(crate::util::err)?;

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

        let http = build_client(false)?;
        let http_insecure = build_client(true)?;

        Ok(Self {
            pool,
            data_dir,
            db_path,
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

fn build_client(insecure: bool) -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent(concat!("FinkMeter/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(10))
        .pool_max_idle_per_host(32)
        .danger_accept_invalid_certs(insecure)
        .build()
        .map_err(crate::util::err)
}
