use crate::models::{CleanupResult, HistoryItem, HistoryStats};
use crate::state::AppState;
use crate::util::err;
use rusqlite::{params, Connection};
use tauri::State;

/// 未指定时间范围时的时间边界，覆盖全部记录
const MIN_TS: i64 = 0;
const MAX_TS: i64 = i64::MAX;
/// 单次最多查询的历史条数
const MAX_QUERY_LIMIT: u32 = 20_000;

#[tauri::command]
pub fn history_list(
    state: State<'_, AppState>,
    limit: Option<u32>,
) -> Result<Vec<HistoryItem>, String> {
    let limit = limit.unwrap_or(100).clamp(1, MAX_QUERY_LIMIT) as i64;

    let conn = state.pool.get().map_err(err)?;
    let mut stmt = conn
        .prepare(
            "SELECT h.id, h.request_id, c.project_id, h.request_name, h.method, h.url,
                    h.status_code, h.duration_ms, h.created_at
             FROM request_history h
             LEFT JOIN requests r ON r.id = h.request_id
             LEFT JOIN collections c ON c.id = r.collection_id
             ORDER BY h.id DESC LIMIT ?1",
        )
        .map_err(err)?;
    let rows = stmt
        .query_map(params![limit], |row| {
            Ok(HistoryItem {
                id: row.get(0)?,
                request_id: row.get(1)?,
                project_id: row.get(2)?,
                request_name: row.get(3)?,
                method: row.get(4).unwrap_or_default(),
                url: row.get(5).unwrap_or_default(),
                status_code: row.get(6)?,
                duration_ms: row.get(7)?,
                created_at: row.get(8)?,
            })
        })
        .map_err(err)?;

    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(err)
}

#[tauri::command]
pub fn history_stats(state: State<'_, AppState>) -> Result<HistoryStats, String> {
    let conn = state.pool.get().map_err(err)?;
    let (total, oldest_at, newest_at): (i64, Option<i64>, Option<i64>) = conn
        .query_row(
            "SELECT COUNT(*), MIN(created_at), MAX(created_at) FROM request_history",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .map_err(err)?;
    let runs_total: i64 = conn
        .query_row("SELECT COUNT(*) FROM request_runs", [], |row| row.get(0))
        .map_err(err)?;

    Ok(HistoryStats {
        total,
        oldest_at,
        newest_at,
        runs_total,
    })
}

/// 按时间范围统计条数
fn count_range(conn: &Connection, table: &str, from: i64, to: i64) -> rusqlite::Result<u32> {
    let sql = format!("SELECT COUNT(*) FROM {table} WHERE created_at >= ?1 AND created_at <= ?2");
    conn.query_row(&sql, params![from, to], |row| row.get(0))
}

/// 按时间范围删除，返回删除条数
fn delete_range(conn: &Connection, table: &str, from: i64, to: i64) -> rusqlite::Result<u32> {
    let sql = format!("DELETE FROM {table} WHERE created_at >= ?1 AND created_at <= ?2");
    conn.execute(&sql, params![from, to]).map(|count| count as u32)
}

/// 清理历史数据：from / to 为空表示不限制边界，dry_run 为 true 时只统计不删除
#[tauri::command]
pub fn data_cleanup(
    state: State<'_, AppState>,
    from: Option<i64>,
    to: Option<i64>,
    history: bool,
    runs: bool,
    vacuum: Option<bool>,
    dry_run: Option<bool>,
) -> Result<CleanupResult, String> {
    let from = from.unwrap_or(MIN_TS);
    let to = to.unwrap_or(MAX_TS);
    let dry_run = dry_run.unwrap_or(false);
    let conn = state.pool.get().map_err(err)?;

    if dry_run {
        let history_count = if history {
            count_range(&conn, "request_history", from, to).map_err(err)?
        } else {
            0
        };
        let runs_count = if runs {
            count_range(&conn, "request_runs", from, to).map_err(err)?
        } else {
            0
        };
        return Ok(CleanupResult {
            history: history_count,
            runs: runs_count,
        });
    }

    let history_count = if history {
        delete_range(&conn, "request_history", from, to).map_err(err)?
    } else {
        0
    };
    let runs_count = if runs {
        delete_range(&conn, "request_runs", from, to).map_err(err)?
    } else {
        0
    };

    // 删除后 SQLite 文件不会自动缩小，按需压缩一次回收空间
    if vacuum.unwrap_or(false) && (history_count > 0 || runs_count > 0) {
        if let Err(error) = conn.execute_batch("VACUUM;") {
            log::warn!("压缩数据库失败（已忽略）：{error}");
        }
    }

    Ok(CleanupResult {
        history: history_count,
        runs: runs_count,
    })
}
