use crate::state::DbPool;
use crate::util::{err, now_ms};

pub fn init(pool: &DbPool) -> Result<(), String> {
    let conn = pool.get().map_err(err)?;
    conn.execute_batch(include_str!("schema.sql")).map_err(err)?;
    drop(conn);

    migrate(pool)?;
    seed_if_empty(pool)
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
    ];
    for sql in statements {
        let _ = conn.execute(sql, []);
    }
    Ok(())
}

/// 首次启动时写入一份可直接上手试用的示例数据
fn seed_if_empty(pool: &DbPool) -> Result<(), String> {
    let conn = pool.get().map_err(err)?;

    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM projects", [], |row| row.get(0))
        .map_err(err)?;
    if count > 0 {
        return Ok(());
    }

    let now = now_ms();

    conn.execute(
        "INSERT INTO projects (name, description, color, sort_order, created_at, updated_at)
         VALUES (?1, ?2, ?3, 0, ?4, ?4)",
        rusqlite::params!["示例项目", "本地创建的示例，可直接改名或删除", "#3b7ddd", now],
    )
    .map_err(err)?;
    let project_id = conn.last_insert_rowid();

    conn.execute(
        "INSERT INTO collections (project_id, parent_id, name, sort_order, created_at, updated_at)
         VALUES (?1, NULL, ?2, 0, ?3, ?3)",
        rusqlite::params![project_id, "默认集合", now],
    )
    .map_err(err)?;
    let collection_id = conn.last_insert_rowid();

    conn.execute(
        "INSERT INTO requests
            (collection_id, name, method, url, headers, params, body_type, body,
             pre_script, post_script, assertions, sort_order, created_at, updated_at)
         VALUES (?1, ?2, 'GET', ?3, '[]', '[]', 'none', '', '', '', '[]', 0, ?4, ?4)",
        rusqlite::params![collection_id, "示例请求", "{{base_url}}/get", now],
    )
    .map_err(err)?;

    conn.execute(
        "INSERT INTO environments (project_id, name, is_active, created_at)
         VALUES (?1, ?2, 1, ?3)",
        rusqlite::params![project_id, "本地环境", now],
    )
    .map_err(err)?;
    let env_id = conn.last_insert_rowid();

    conn.execute(
        "INSERT INTO env_variables (env_id, \"key\", value, is_secret, sort_order)
         VALUES (?1, 'base_url', 'https://httpbin.org', 0, 0)",
        rusqlite::params![env_id],
    )
    .map_err(err)?;

    Ok(())
}
