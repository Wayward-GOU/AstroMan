use crate::models::Project;
use crate::state::AppState;
use crate::util::{err, now_ms};
use rusqlite::{params, Connection, Row};
use tauri::State;

fn map_project(row: &Row<'_>) -> rusqlite::Result<Project> {
    Ok(Project {
        id: row.get(0)?,
        name: row.get(1)?,
        description: row.get(2)?,
        color: row.get(3)?,
        sort_order: row.get(4)?,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
    })
}

fn find_project(conn: &Connection, id: i64) -> Result<Project, String> {
    conn.query_row(
        "SELECT id, name, description, color, sort_order, created_at, updated_at
         FROM projects WHERE id = ?1",
        params![id],
        map_project,
    )
    .map_err(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => "项目不存在".to_string(),
        other => err(other),
    })
}

#[tauri::command]
pub fn project_list(state: State<'_, AppState>) -> Result<Vec<Project>, String> {
    let conn = state.pool.get().map_err(err)?;
    let mut stmt = conn
        .prepare(
            "SELECT id, name, description, color, sort_order, created_at, updated_at
             FROM projects ORDER BY sort_order ASC, created_at ASC",
        )
        .map_err(err)?;
    let rows = stmt.query_map([], map_project).map_err(err)?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(err)
}

#[tauri::command]
pub fn project_create(
    state: State<'_, AppState>,
    name: String,
    description: Option<String>,
) -> Result<Project, String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("项目名称不能为空".to_string());
    }

    let conn = state.pool.get().map_err(err)?;
    let now = now_ms();
    conn.execute(
        "INSERT INTO projects (name, description, color, sort_order, created_at, updated_at)
         VALUES (?1, ?2, NULL, 0, ?3, ?3)",
        params![name, description, now],
    )
    .map_err(err)?;

    find_project(&conn, conn.last_insert_rowid())
}

#[tauri::command]
pub fn project_update(
    state: State<'_, AppState>,
    id: i64,
    name: String,
    description: Option<String>,
) -> Result<(), String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("项目名称不能为空".to_string());
    }

    let conn = state.pool.get().map_err(err)?;
    conn.execute(
        "UPDATE projects SET name = ?1, description = ?2, updated_at = ?3 WHERE id = ?4",
        params![name, description, now_ms(), id],
    )
    .map_err(err)?;
    Ok(())
}

#[tauri::command]
pub fn project_delete(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    let conn = state.pool.get().map_err(err)?;
    conn.execute("DELETE FROM projects WHERE id = ?1", params![id])
        .map_err(err)?;
    Ok(())
}
