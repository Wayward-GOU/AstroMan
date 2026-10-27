use crate::models::CollectionNode;
use crate::state::AppState;
use crate::util::{err, now_ms};
use rusqlite::{params, Connection, Row};
use std::collections::HashMap;
use tauri::State;

fn map_node(row: &Row<'_>) -> rusqlite::Result<CollectionNode> {
    Ok(CollectionNode {
        id: row.get(0)?,
        project_id: row.get(1)?,
        parent_id: row.get(2)?,
        name: row.get(3)?,
        sort_order: row.get(4)?,
        children: Vec::new(),
    })
}

fn find_node(conn: &Connection, id: i64) -> Result<CollectionNode, String> {
    conn.query_row(
        "SELECT id, project_id, parent_id, name, sort_order FROM collections WHERE id = ?1",
        params![id],
        map_node,
    )
    .map_err(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => "集合不存在".to_string(),
        other => err(other),
    })
}

fn build_children(
    map: &mut HashMap<Option<i64>, Vec<CollectionNode>>,
    parent: Option<i64>,
) -> Vec<CollectionNode> {
    let Some(mut nodes) = map.remove(&parent) else {
        return Vec::new();
    };
    for node in &mut nodes {
        node.children = build_children(map, Some(node.id));
    }
    nodes
}

#[tauri::command]
pub fn collection_tree(
    state: State<'_, AppState>,
    project_id: i64,
) -> Result<Vec<CollectionNode>, String> {
    let conn = state.pool.get().map_err(err)?;
    let mut stmt = conn
        .prepare(
            "SELECT id, project_id, parent_id, name, sort_order
             FROM collections WHERE project_id = ?1 ORDER BY sort_order ASC, id ASC",
        )
        .map_err(err)?;
    let rows = stmt.query_map(params![project_id], map_node).map_err(err)?;
    let nodes = rows
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(err)?;

    let mut map: HashMap<Option<i64>, Vec<CollectionNode>> = HashMap::new();
    for node in nodes {
        map.entry(node.parent_id).or_default().push(node);
    }

    Ok(build_children(&mut map, None))
}

#[tauri::command]
pub fn collection_create(
    state: State<'_, AppState>,
    project_id: i64,
    parent_id: Option<i64>,
    name: String,
) -> Result<CollectionNode, String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("集合名称不能为空".to_string());
    }

    let conn = state.pool.get().map_err(err)?;

    if let Some(parent) = parent_id {
        let parent_project: i64 = conn
            .query_row(
                "SELECT project_id FROM collections WHERE id = ?1",
                params![parent],
                |row| row.get(0),
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => "父集合不存在".to_string(),
                other => err(other),
            })?;
        if parent_project != project_id {
            return Err("父集合不属于当前项目".to_string());
        }
    }

    let now = now_ms();
    conn.execute(
        "INSERT INTO collections (project_id, parent_id, name, sort_order, created_at, updated_at)
         VALUES (?1, ?2, ?3, 0, ?4, ?4)",
        params![project_id, parent_id, name, now],
    )
    .map_err(err)?;

    find_node(&conn, conn.last_insert_rowid())
}

#[tauri::command]
pub fn collection_rename(
    state: State<'_, AppState>,
    id: i64,
    name: String,
) -> Result<(), String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("集合名称不能为空".to_string());
    }

    let conn = state.pool.get().map_err(err)?;
    conn.execute(
        "UPDATE collections SET name = ?1, updated_at = ?2 WHERE id = ?3",
        params![name, now_ms(), id],
    )
    .map_err(err)?;
    Ok(())
}

#[tauri::command]
pub fn collection_delete(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    let conn = state.pool.get().map_err(err)?;
    conn.execute("DELETE FROM collections WHERE id = ?1", params![id])
        .map_err(err)?;
    Ok(())
}

/// 按给定顺序重排同级集合（拖拽排序）。ids 为同一父集合下集合 id 的新顺序。
#[tauri::command]
pub fn collection_reorder(
    state: State<'_, AppState>,
    project_id: i64,
    parent_id: Option<i64>,
    ids: Vec<i64>,
) -> Result<(), String> {
    if ids.is_empty() {
        return Err("排序列表为空".to_string());
    }
    let mut conn = state.pool.get().map_err(err)?;
    let tx = conn.transaction().map_err(err)?;
    for (index, id) in ids.iter().enumerate() {
        tx.execute(
            "UPDATE collections SET sort_order = ?1, updated_at = ?2
             WHERE id = ?3 AND project_id = ?4 AND parent_id IS ?5",
            params![index as i64, now_ms(), id, project_id, parent_id],
        )
        .map_err(err)?;
    }
    tx.commit().map_err(err)?;
    Ok(())
}
