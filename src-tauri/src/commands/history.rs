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
pub fn history_clear(state: State<'_, AppState>) -> Result<(), String> {
    let conn = state.pool.get().map_err(err)?;
    conn.execute("DELETE FROM request_history", []).map_err(err)?;
    Ok(())
}
