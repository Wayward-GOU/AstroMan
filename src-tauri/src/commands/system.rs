use crate::models::{AppInfo, WorkspaceStats};
use crate::state::AppState;
use crate::util::{err, now_ms};
use rusqlite::params;
use tauri::State;

#[tauri::command]
pub fn app_info(state: State<'_, AppState>) -> AppInfo {
    AppInfo {
        version: env!("CARGO_PKG_VERSION").to_string(),
        data_dir: state.data_dir.to_string_lossy().to_string(),
        db_path: state.db_path.to_string_lossy().to_string(),
    }
}

/// 工作台首屏所需的聚合统计
#[tauri::command]
pub fn workspace_stats(state: State<'_, AppState>) -> Result<WorkspaceStats, String> {
    let conn = state.pool.get().map_err(err)?;
    let count = |sql: &str| -> Result<i64, String> {
        conn.query_row(sql, [], |row| row.get(0)).map_err(err)
    };
    let week_ago = now_ms() - 7 * 24 * 3600 * 1000;

    Ok(WorkspaceStats {
        project_count: count("SELECT COUNT(*) FROM projects")?,
        collection_count: count("SELECT COUNT(*) FROM collections")?,
        request_count: count("SELECT COUNT(*) FROM requests")?,
        environment_count: count("SELECT COUNT(*) FROM environments")?,
        history_count: count("SELECT COUNT(*) FROM request_history")?,
        report_count: count("SELECT COUNT(*) FROM test_reports")?,
        report_request_total: count("SELECT COALESCE(SUM(total_requests), 0) FROM test_reports")?,
        report_success_total: count("SELECT COALESCE(SUM(success_count), 0) FROM test_reports")?,
        recent_history: conn
            .query_row(
                "SELECT COUNT(*) FROM request_history WHERE created_at >= ?1",
                params![week_ago],
                |row| row.get(0),
            )
            .map_err(err)?,
        recent_reports: conn
            .query_row(
                "SELECT COUNT(*) FROM test_reports WHERE created_at >= ?1",
                params![week_ago],
                |row| row.get(0),
            )
            .map_err(err)?,
    })
}
