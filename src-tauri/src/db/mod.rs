use crate::state::DbPool;
use crate::util::err;

mod seed;

/// 数据库结构版本（PRAGMA user_version）
///
/// 1 = v1.0.7；2 = v1.0.8（耗时分解、响应体类型、按请求网络配置、app_meta）
/// 3 = 按 Body 类型保留草稿（requests.body_meta）
pub const DB_VERSION: i64 = 3;

/// 打开库时发现库版本高于本程序：说明用户降级了应用。
/// 直接返回错误而不是继续写，避免旧代码用旧结构写坏新数据。
const E_SCHEMA_FUTURE: &str = "E_SCHEMA: 数据库由更高版本的 AstroMan 创建，请升级应用；\
如已降级，请从「设置 → 数据备份」恢复对应版本的备份";

pub fn init(pool: &DbPool) -> Result<(), String> {
    {
        let conn = pool.get().map_err(err)?;
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .map_err(err)?;
        if version > DB_VERSION {
            return Err(E_SCHEMA_FUTURE.to_string());
        }
    }

    let conn = pool.get().map_err(err)?;
    conn.execute_batch(include_str!("schema.sql"))
        .map_err(err)?;
    drop(conn);

    migrate(pool)?;
    verify_core_tables(pool)?;
    recover_interrupted_runs(pool)?;
    seed_database(pool)
}

/// 关键表存在性校验：缺表说明库损坏或结构被外力改过，交给恢复流程处理
fn verify_core_tables(pool: &DbPool) -> Result<(), String> {
    let conn = pool.get().map_err(err)?;
    let mut missing = Vec::new();
    for table in ["projects", "collections", "requests", "environments"] {
        let exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                [table],
                |row| row.get(0),
            )
            .map_err(err)?;
        if exists == 0 {
            missing.push(table);
        }
    }
    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "E_CORRUPT: 数据库缺少核心表（{}），可能已损坏，请在启动提示中从备份恢复",
            missing.join("、")
        ))
    }
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

/// 存量表补列清单：(表, 列, 列定义)
///
/// 改成结构化清单而不是整条 SQL，是为了在执行前先 `PRAGMA table_info` 判存：
/// 老代码用 `let _ = conn.execute(...)` 吞掉所有错误，真实的「表不存在 / 库只读 /
/// 磁盘满」也被一并吞掉，用户只会看到功能莫名失效（文档 4.0.3 / 7.2）。
const COLUMN_MIGRATIONS: &[(&str, &str, &str)] = &[
    ("test_reports", "p75_rt", "REAL DEFAULT 0"),
    ("test_reports", "p90_rt", "REAL DEFAULT 0"),
    ("test_reports", "p999_rt", "REAL DEFAULT 0"),
    ("test_reports", "stddev_rt", "REAL DEFAULT 0"),
    ("test_reports", "ttfb_avg", "REAL DEFAULT 0"),
    ("test_reports", "ttfb_p95", "REAL DEFAULT 0"),
    ("test_reports", "apdex", "REAL DEFAULT 0"),
    ("test_reports", "bytes_total", "INTEGER DEFAULT 0"),
    ("test_reports", "target_rps", "REAL DEFAULT 0"),
    ("test_reports", "config_json", "TEXT"),
    ("test_reports", "stages_json", "TEXT"),
    ("test_reports", "codes_json", "TEXT"),
    ("test_reports", "histogram_json", "TEXT"),
    ("test_reports", "error_types_json", "TEXT"),
    // v1.0.8 压测报告：可编辑的场景描述与总结
    ("test_reports", "scenario_text", "TEXT"),
    ("test_reports", "summary_text", "TEXT"),
    ("report_samples", "p50_rt", "REAL DEFAULT 0"),
    ("report_samples", "ttfb_avg", "REAL DEFAULT 0"),
    ("report_samples", "bytes", "INTEGER DEFAULT 0"),
    ("report_requests", "body", "TEXT"),
    ("report_requests", "params", "TEXT"),
    ("report_requests", "req_headers", "TEXT"),
    ("report_requests", "res_headers", "TEXT"),
    ("report_requests", "res_body", "TEXT"),
    ("environments", "script", "TEXT"),
    ("requests", "auth", "TEXT"),
    ("test_plans", "config_json", "TEXT"),
    ("test_plans", "updated_at", "INTEGER DEFAULT 0"),
    ("ai_providers", "kind", "TEXT NOT NULL DEFAULT 'openai'"),
    ("ai_messages", "reasoning", "TEXT"),
    ("ai_usage_logs", "request_preview", "TEXT"),
    ("ai_providers", "max_tokens", "INTEGER DEFAULT 0"),
    ("ai_providers", "timeout_secs", "INTEGER DEFAULT 300"),
    ("ai_sessions", "pinned", "INTEGER NOT NULL DEFAULT 0"),
    ("ai_sessions", "summary", "TEXT"),
    ("ai_sessions", "summary_until", "INTEGER DEFAULT 0"),
    ("ai_messages", "token_count", "INTEGER DEFAULT 0"),
    ("ai_tool_logs", "round", "INTEGER DEFAULT 0"),
    ("ai_tool_logs", "trace_id", "TEXT"),
    ("ai_tool_logs", "timeout", "INTEGER DEFAULT 0"),
    // v1.0.8：执行记录来源（manual | batch | suite | cli），老数据默认手工
    (
        "request_history",
        "origin",
        "TEXT NOT NULL DEFAULT 'manual'",
    ),
    ("request_runs", "origin", "TEXT NOT NULL DEFAULT 'manual'"),
    // v1.0.8：耗时分解、响应体类型、按请求网络配置
    ("request_history", "timings_json", "TEXT"),
    ("request_history", "body_kind", "TEXT"),
    ("request_history", "body_file", "TEXT"),
    ("requests", "net_json", "TEXT"),
    // 各 Body 类型的草稿，切换类型时不丢失内容
    ("requests", "body_meta", "TEXT"),
];

fn column_exists(conn: &rusqlite::Connection, table: &str, column: &str) -> Result<bool, String> {
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(err)?;
    let mut rows = stmt.query([]).map_err(err)?;
    while let Some(row) = rows.next().map_err(err)? {
        let name: String = row.get(1).map_err(err)?;
        if name.eq_ignore_ascii_case(column) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// 判存式补列：只有「列已存在」被忽略，其它错误一律上报
pub fn add_column_if_missing(
    conn: &rusqlite::Connection,
    table: &str,
    column: &str,
    ddl: &str,
) -> Result<bool, String> {
    if column_exists(conn, table, column)? {
        return Ok(false);
    }
    conn.execute(
        &format!("ALTER TABLE {table} ADD COLUMN {column} {ddl}"),
        [],
    )
    .map_err(|e| format!("E_SCHEMA: 为 {table} 添加列 {column} 失败：{e}"))?;
    Ok(true)
}

/// 老版本数据库补列 + 补索引 + 版本号推进，整体包在一个事务里
fn migrate(pool: &DbPool) -> Result<(), String> {
    let conn = pool.get().map_err(err)?;
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(err)?;

    {
        let tx = conn.unchecked_transaction().map_err(err)?;
        for (table, column, ddl) in COLUMN_MIGRATIONS {
            add_column_if_missing(&tx, table, column, ddl)?;
        }
        // 依赖 updated_at 的索引必须在补列之后创建，否则老库会在建索引时报「no such column」
        tx.execute(
            "CREATE INDEX IF NOT EXISTS idx_plans_project ON test_plans(project_id, updated_at DESC)",
            [],
        )
        .map_err(err)?;
        tx.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_history_origin ON request_history(origin, created_at DESC);
             CREATE INDEX IF NOT EXISTS idx_runs_origin ON request_runs(origin, created_at DESC);",
        )
        .map_err(err)?;
        if version < DB_VERSION {
            tx.execute_batch(&format!("PRAGMA user_version = {DB_VERSION}"))
                .map_err(err)?;
        }
        tx.commit().map_err(err)?;
    }

    if version > 0 && version < DB_VERSION {
        log::info!("数据库已从结构版本 {version} 升级到 {DB_VERSION}");
    }
    Ok(())
}

/// 读取 app_meta 中的一项（表不存在时返回 None，便于迁移早期调用）
pub fn meta_get(conn: &rusqlite::Connection, key: &str) -> Option<String> {
    conn.query_row("SELECT value FROM app_meta WHERE key = ?1", [key], |row| {
        row.get::<_, Option<String>>(0)
    })
    .ok()
    .flatten()
}

pub fn meta_set(conn: &rusqlite::Connection, key: &str, value: &str) -> Result<(), String> {
    conn.execute(
        "INSERT INTO app_meta (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        rusqlite::params![key, value],
    )
    .map_err(err)?;
    Ok(())
}

/// 诊断包与备份列表用：各表行数
pub fn table_counts(conn: &rusqlite::Connection) -> Vec<(String, i64)> {
    let mut names = Vec::new();
    if let Ok(mut stmt) = conn.prepare(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
    ) {
        if let Ok(rows) = stmt.query_map([], |row| row.get::<_, String>(0)) {
            names = rows.filter_map(Result::ok).collect();
        }
    }
    names
        .into_iter()
        .map(|name| {
            let count: i64 = conn
                .query_row(&format!("SELECT COUNT(*) FROM \"{name}\""), [], |row| {
                    row.get(0)
                })
                .unwrap_or(0);
            (name, count)
        })
        .collect()
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
        let dir =
            std::env::temp_dir().join(format!("astroman-db-{tag}-{}-{stamp}", std::process::id()));
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
        let mut stmt = conn
            .prepare(&format!("PRAGMA table_info({table})"))
            .unwrap();
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
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
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
            .query_row(
                "SELECT origin FROM request_history WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(origin, "manual");
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, DB_VERSION);
        drop(conn);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
