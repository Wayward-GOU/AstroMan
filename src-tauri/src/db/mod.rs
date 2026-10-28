use crate::state::DbPool;
use crate::util::err;

mod seed;

pub fn init(pool: &DbPool) -> Result<(), String> {
    let conn = pool.get().map_err(err)?;
    conn.execute_batch(include_str!("schema.sql")).map_err(err)?;
    drop(conn);

    migrate(pool)?;
    seed_database(pool)
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
    Ok(())
}

/// 首次启动写入示例数据；已有数据库则按需补齐示例场景
fn seed_database(pool: &DbPool) -> Result<(), String> {
    let conn = pool.get().map_err(err)?;

    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM projects", [], |row| row.get(0))
        .map_err(err)?;

    if count == 0 {
        seed::seed(&conn)
    } else {
        seed::upgrade_sample(&conn)
    }
}
