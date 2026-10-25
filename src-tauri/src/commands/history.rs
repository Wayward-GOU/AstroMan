use crate::models::HistoryItem;
use crate::state::AppState;
use crate::util::err;
use rusqlite::params;
use tauri::State;

#[tauri::command]
pub fn history_list(
    state: State<'_, AppState>,
    limit: Option<u32>,
) -> Result<Vec<HistoryItem>, String> {
    let limit = limit.unwrap_or(100).clamp(1, 500) as i64;

    let conn = state.pool.get().map_err(err)?;
    let mut stmt = conn
        .prepare(
            "SELECT id, request_id, request_name, method, url, status_code, duration_ms, created_at
             FROM request_history ORDER BY id DESC LIMIT ?1",
        )
        .map_err(err)?;
    let rows = stmt
        .query_map(params![limit], |row| {
            Ok(HistoryItem {
                id: row.get(0)?,
                request_id: row.get(1)?,
                request_name: row.get(2)?,
                method: row.get(3).unwrap_or_default(),
                url: row.get(4).unwrap_or_default(),
                status_code: row.get(5)?,
                duration_ms: row.get(6)?,
                created_at: row.get(7)?,
            })
        })
        .map_err(err)?;

    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(err)
}

#[tauri::command]
pub fn history_clear(state: State<'_, AppState>) -> Result<(), String> {
    let conn = state.pool.get().map_err(err)?;
    conn.execute("DELETE FROM request_history", []).map_err(err)?;
    Ok(())
}
