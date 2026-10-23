use crate::models::{HttpRequestDetail, KeyValueItem, RequestSearchHit, RequestSummary};
use crate::state::AppState;
use crate::util::{err, now_ms};
use rusqlite::{params, Connection, Row};
use tauri::State;

fn decode_list(raw: Option<String>) -> Vec<KeyValueItem> {
    raw.and_then(|s| serde_json::from_str::<Vec<KeyValueItem>>(&s).ok())
        .unwrap_or_default()
}

fn map_summary(row: &Row<'_>) -> rusqlite::Result<RequestSummary> {
    Ok(RequestSummary {
        id: row.get(0)?,
        collection_id: row.get(1)?,
        name: row.get(2)?,
        method: row.get(3)?,
        url: row.get(4)?,
        sort_order: row.get(5)?,
        updated_at: row.get(6)?,
    })
}

fn map_detail(row: &Row<'_>) -> rusqlite::Result<HttpRequestDetail> {
    let headers: Option<String> = row.get(5)?;
    let query: Option<String> = row.get(6)?;
    Ok(HttpRequestDetail {
        id: Some(row.get(0)?),
        collection_id: row.get(1)?,
        name: row.get(2)?,
        method: row.get(3)?,
        url: row.get(4)?,
        headers: decode_list(headers),
        params: decode_list(query),
        body_type: row.get::<_, Option<String>>(7)?.unwrap_or_else(|| "none".into()),
        body: row.get::<_, Option<String>>(8)?.unwrap_or_default(),
        pre_script: row.get::<_, Option<String>>(9)?.unwrap_or_default(),
        post_script: row.get::<_, Option<String>>(10)?.unwrap_or_default(),
        assertions: row.get::<_, Option<String>>(11)?.unwrap_or_default(),
    })
}

fn find_request(conn: &Connection, id: i64) -> Result<HttpRequestDetail, String> {
    conn.query_row(
        "SELECT id, collection_id, name, method, url, headers, params,
                body_type, body, pre_script, post_script, assertions
         FROM requests WHERE id = ?1",
        params![id],
        map_detail,
    )
    .map_err(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => "请求不存在".to_string(),
        other => err(other),
    })
}

#[tauri::command]
pub fn request_list(
    state: State<'_, AppState>,
    collection_id: i64,
) -> Result<Vec<RequestSummary>, String> {
    let conn = state.pool.get().map_err(err)?;
    let mut stmt = conn
        .prepare(
            "SELECT id, collection_id, name, method, url, sort_order, updated_at
             FROM requests WHERE collection_id = ?1 ORDER BY sort_order ASC, id ASC",
        )
        .map_err(err)?;
    let rows = stmt
        .query_map(params![collection_id], map_summary)
        .map_err(err)?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(err)
}

#[tauri::command]
pub fn request_search(
    state: State<'_, AppState>,
    project_id: i64,
    keyword: String,
) -> Result<Vec<RequestSearchHit>, String> {
    let keyword = keyword.trim();
    if keyword.is_empty() {
        return Ok(Vec::new());
    }
    let escaped = keyword
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    let pattern = format!("%{escaped}%");
    let conn = state.pool.get().map_err(err)?;
    let mut stmt = conn
        .prepare(
            "SELECT r.id, r.collection_id, c.name, r.name, r.method, r.url, r.updated_at
             FROM requests r
             JOIN collections c ON c.id = r.collection_id
             WHERE c.project_id = ?1
               AND (r.name LIKE ?2 ESCAPE '\\' OR r.url LIKE ?2 ESCAPE '\\')
             ORDER BY r.updated_at DESC, r.id DESC
             LIMIT 100",
        )
        .map_err(err)?;
    let rows = stmt
        .query_map(params![project_id, pattern], |row| {
            Ok(RequestSearchHit {
                id: row.get(0)?,
                collection_id: row.get(1)?,
                collection_name: row.get(2)?,
                name: row.get(3)?,
                method: row.get(4)?,
                url: row.get(5)?,
                updated_at: row.get(6)?,
            })
        })
        .map_err(err)?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(err)
}

#[tauri::command]
pub fn request_get(state: State<'_, AppState>, id: i64) -> Result<HttpRequestDetail, String> {
    let conn = state.pool.get().map_err(err)?;
    find_request(&conn, id)
}

#[tauri::command]
pub fn request_save(
    state: State<'_, AppState>,
    request: HttpRequestDetail,
) -> Result<HttpRequestDetail, String> {
    let name = request.name.trim();
    let name = if name.is_empty() {
        "未命名请求"
    } else {
        name
    };
    let headers = serde_json::to_string(&request.headers).map_err(err)?;
    let query = serde_json::to_string(&request.params).map_err(err)?;
    let now = now_ms();

    let conn = state.pool.get().map_err(err)?;

    match request.id {
        Some(id) => {
            let affected = conn
                .execute(
                    "UPDATE requests SET name = ?1, method = ?2, url = ?3, headers = ?4,
                        params = ?5, body_type = ?6, body = ?7, pre_script = ?8,
                        post_script = ?9, assertions = ?10, updated_at = ?11
                     WHERE id = ?12",
                    params![
                        name,
                        request.method,
                        request.url,
                        headers,
                        query,
                        request.body_type,
                        request.body,
                        request.pre_script,
                        request.post_script,
                        request.assertions,
                        now,
                        id
                    ],
                )
                .map_err(err)?;
            if affected == 0 {
                return Err("请求不存在".to_string());
            }
            find_request(&conn, id)
        }
        None => {
            conn.execute(
                "INSERT INTO requests
                    (collection_id, name, method, url, headers, params, body_type, body,
                     pre_script, post_script, assertions, sort_order, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 0, ?12, ?12)",
                params![
                    request.collection_id,
                    name,
                    request.method,
                    request.url,
                    headers,
                    query,
                    request.body_type,
                    request.body,
                    request.pre_script,
                    request.post_script,
                    request.assertions,
                    now
                ],
            )
            .map_err(err)?;
            find_request(&conn, conn.last_insert_rowid())
        }
    }
}

#[tauri::command]
pub fn request_delete(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    let conn = state.pool.get().map_err(err)?;
    conn.execute("DELETE FROM requests WHERE id = ?1", params![id])
        .map_err(err)?;
    Ok(())
}
