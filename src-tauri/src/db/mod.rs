use crate::state::DbPool;
use crate::util::err;

mod seed;

/// 数据库结构版本（PRAGMA user_version）。新增表走 schema.sql 不算版本，
/// 只有需要 ALTER 存量表的升级才递增，供 CLI 判断「版本不兼容」。
pub const DB_VERSION: i64 = 1;

pub fn init(pool: &DbPool) -> Result<(), String> {
    let conn = pool.get().map_err(err)?;
    conn.execute_batch(include_str!("schema.sql")).map_err(err)?;
    drop(conn);

    migrate(pool)?;
    recover_interrupted_runs(pool)?;
    seed_database(pool)
}

/// 上次异常退出（崩溃 / 强杀）遗留的 queued / running 运行标记为 error，
/// 避免界面永远显示「运行中」；用例级结果已落库的保持不变
fn recover_interrupted_runs(pool: &DbPool) -> Result<(), String> {
    let conn = pool.get().map_err(err)?;
    conn.execute(
        "UPDATE test_runs SET status = 'error', finished_at = ?1
         WHERE status IN ('queued', 'running')",
        rusqlite::params![crate::util::now_ms()],
    )
    .map_err(err)?;
    Ok(())
}

/// 老版本数据库补列：已存在的列会报错，直接忽略
fn migrate(pool: &DbPool) -> Result<(), String> {
    let conn = pool.get().map_err(err)?;
    let statements = [
        "ALTER TABLE test_reports ADD COLUMN p75_rt REAL DEFAULT 0",
        "ALTER TABLE test_reports ADD COLUMN p90_rt REAL DEFAULT 0",
        "ALTER TABLE test_reports ADD COLUMN p999_rt REAL DEFAULT 0",
        "ALTER TABLE test_reports ADD COLUMN stddev_rt REAL DEFAULT 0",
        "ALTER TABLE test_reports ADD COLUMN ttfb_avg REAL DEFAULT 0",
        "ALTER TABLE test_reports ADD COLUMN ttfb_p95 REAL DEFAULT 0",
        "ALTER TABLE test_reports ADD COLUMN apdex REAL DEFAULT 0",
        "ALTER TABLE test_reports ADD COLUMN bytes_total INTEGER DEFAULT 0",
        "ALTER TABLE test_reports ADD COLUMN target_rps REAL DEFAULT 0",
        "ALTER TABLE test_reports ADD COLUMN config_json TEXT",
        "ALTER TABLE test_reports ADD COLUMN stages_json TEXT",
        "ALTER TABLE test_reports ADD COLUMN codes_json TEXT",
        "ALTER TABLE test_reports ADD COLUMN histogram_json TEXT",
        "ALTER TABLE test_reports ADD COLUMN error_types_json TEXT",
        "ALTER TABLE report_samples ADD COLUMN p50_rt REAL DEFAULT 0",
        "ALTER TABLE report_samples ADD COLUMN ttfb_avg REAL DEFAULT 0",
        "ALTER TABLE report_samples ADD COLUMN bytes INTEGER DEFAULT 0",
        "ALTER TABLE report_requests ADD COLUMN body TEXT",
        "ALTER TABLE report_requests ADD COLUMN params TEXT",
        "ALTER TABLE report_requests ADD COLUMN req_headers TEXT",
        "ALTER TABLE report_requests ADD COLUMN res_headers TEXT",
        "ALTER TABLE report_requests ADD COLUMN res_body TEXT",
        "ALTER TABLE environments ADD COLUMN script TEXT",
        "ALTER TABLE requests ADD COLUMN auth TEXT",
        "ALTER TABLE test_plans ADD COLUMN config_json TEXT",
        "ALTER TABLE test_plans ADD COLUMN updated_at INTEGER DEFAULT 0",
        "ALTER TABLE ai_providers ADD COLUMN kind TEXT NOT NULL DEFAULT 'openai'",
        "ALTER TABLE ai_messages ADD COLUMN reasoning TEXT",
        "ALTER TABLE ai_usage_logs ADD COLUMN request_preview TEXT",
        "ALTER TABLE ai_providers ADD COLUMN max_tokens INTEGER DEFAULT 0",
        "ALTER TABLE ai_providers ADD COLUMN timeout_secs INTEGER DEFAULT 300",
        "ALTER TABLE ai_sessions ADD COLUMN pinned INTEGER NOT NULL DEFAULT 0",
        "ALTER TABLE ai_sessions ADD COLUMN summary TEXT",
        "ALTER TABLE ai_sessions ADD COLUMN summary_until INTEGER DEFAULT 0",
        "ALTER TABLE ai_messages ADD COLUMN token_count INTEGER DEFAULT 0",
        "ALTER TABLE ai_tool_logs ADD COLUMN round INTEGER DEFAULT 0",
        "ALTER TABLE ai_tool_logs ADD COLUMN trace_id TEXT",
        "ALTER TABLE ai_tool_logs ADD COLUMN timeout INTEGER DEFAULT 0",
        // v1.0.8：执行记录来源（manual | batch | suite | cli），老数据默认手工
        "ALTER TABLE request_history ADD COLUMN origin TEXT NOT NULL DEFAULT 'manual'",
        "ALTER TABLE request_runs ADD COLUMN origin TEXT NOT NULL DEFAULT 'manual'",
    ];
    for sql in statements {
        let _ = conn.execute(sql, []);
    }
    // 依赖 updated_at 的索引必须在补列之后创建，否则老库会在建索引时报「no such column」
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_plans_project ON test_plans(project_id, updated_at DESC)",
        [],
    )
    .map_err(err)?;
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_history_origin ON request_history(origin, created_at DESC);
         CREATE INDEX IF NOT EXISTS idx_runs_origin ON request_runs(origin, created_at DESC);",
    )
    .map_err(err)?;

    // 库版本号：CLI 与后续迁移据此判断兼容性，只升不降
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(err)?;
    if version < DB_VERSION {
        conn.execute_batch(&format!("PRAGMA user_version = {DB_VERSION}"))
            .map_err(err)?;
    }
    Ok(())
}

/// 首次启动写入示例数据；已有数据库则按需补齐示例场景
fn seed_database(pool: &DbPool) -> Result<(), String> {
    let conn = pool.get().map_err(err)?;

    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM projects", [], |row| row.get(0))
        .map_err(err)?;

    if count == 0 {
        seed::seed(&conn)?;
    } else {
        seed::upgrade_sample(&conn)?;
    }
    // 示例测试套件幂等补齐：新库与老库升级都能看到可直接运行的回归例子
    seed::ensure_sample_suite(&conn)
}

#[cfg(test)]
mod tests {
    use super::*;
    use r2d2_sqlite::SqliteConnectionManager;
    use rusqlite::params;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_pool(tag: &str) -> (PathBuf, DbPool) {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!(
            "astroman-db-{tag}-{}-{stamp}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let manager = SqliteConnectionManager::file(dir.join("astroman.db")).with_init(|conn| {
            conn.execute_batch(
                "PRAGMA journal_mode = WAL;
                 PRAGMA foreign_keys = ON;
                 PRAGMA busy_timeout = 5000;",
            )
        });
        let pool = r2d2::Pool::builder().max_size(2).build(manager).unwrap();
        (dir, pool)
    }

    fn has_column(pool: &DbPool, table: &str, column: &str) -> bool {
        let conn = pool.get().unwrap();
        let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})")).unwrap();
        let found = stmt
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .filter_map(Result::ok)
            .any(|name| name == column);
        found
    }

    #[test]
    fn init_is_idempotent_and_creates_test_tables() {
        let (dir, pool) = temp_pool("init");
        init(&pool).unwrap();
        // 重复执行不应报错（新建表幂等 + ALTER 已存在则忽略）
        init(&pool).unwrap();

        let conn = pool.get().unwrap();
        for table in [
            "test_suites",
            "test_cases",
            "test_datasets",
            "test_runs",
            "test_run_cases",
            "test_run_assertions",
            "test_artifacts",
            "notes",
        ] {
            let count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                    [table],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(count, 1, "缺少表 {table}");
        }

        assert!(has_column(&pool, "request_history", "origin"));
        assert!(has_column(&pool, "request_runs", "origin"));
        let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0)).unwrap();
        assert_eq!(version, DB_VERSION);
        drop(conn);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn seeds_sample_suite_with_runnable_cases() {
        let (dir, pool) = temp_pool("sample-suite");
        init(&pool).unwrap();
        let conn = pool.get().unwrap();
        let suite_id: i64 = conn
            .query_row(
                "SELECT id FROM test_suites WHERE name = '示例回归（httpbin）'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let cases: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM test_cases WHERE suite_id = ?1 AND enabled = 1",
                params![suite_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(cases, 3, "示例套件应包含 3 个可直接运行的用例");
        let with_assertions: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM test_cases WHERE suite_id = ?1
                   AND assertions_json IS NOT NULL AND assertions_json != '[]'",
                params![suite_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(with_assertions, 3);
        drop(conn);

        // 重复初始化不应重复插入
        init(&pool).unwrap();
        let conn = pool.get().unwrap();
        let suites: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM test_suites WHERE name = '示例回归（httpbin）'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(suites, 1);
        drop(conn);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn upgrades_legacy_db_without_origin_columns() {
        let (dir, pool) = temp_pool("legacy");
        {
            // 模拟 v1.0.7 老库：存在历史表但没有 origin 列
            let conn = pool.get().unwrap();
            conn.execute_batch(
                "CREATE TABLE request_history (
                     id INTEGER PRIMARY KEY AUTOINCREMENT,
                     request_id INTEGER, request_name TEXT, method TEXT, url TEXT,
                     status_code INTEGER, duration_ms INTEGER, created_at INTEGER NOT NULL);
                 CREATE TABLE request_runs (
                     id INTEGER PRIMARY KEY AUTOINCREMENT,
                     request_id INTEGER NOT NULL, method TEXT NOT NULL, url TEXT NOT NULL,
                     headers TEXT, params TEXT, body_type TEXT DEFAULT 'none', body TEXT,
                     response_status INTEGER DEFAULT 0, response_status_text TEXT,
                     response_headers TEXT, response_body TEXT,
                     duration_ms INTEGER DEFAULT 0, size_bytes INTEGER DEFAULT 0,
                     error TEXT, created_at INTEGER NOT NULL);
                 INSERT INTO request_history (request_name, method, url, created_at)
                 VALUES ('旧记录', 'GET', 'http://localhost', 1);",
            )
            .unwrap();
        }
        init(&pool).unwrap();
        let conn = pool.get().unwrap();
        let origin: String = conn
            .query_row("SELECT origin FROM request_history WHERE id = 1", [], |row| row.get(0))
            .unwrap();
        assert_eq!(origin, "manual");
        let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0)).unwrap();
        assert_eq!(version, DB_VERSION);
        drop(conn);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
