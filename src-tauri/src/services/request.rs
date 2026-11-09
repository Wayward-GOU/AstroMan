//! 请求定义的读写与保存钩子
//!
//! 从 `commands/request.rs` 下沉：Tauri 命令只做参数适配，保存时统一追加快照版本。

use rusqlite::{params, Connection, Row};

use crate::models::{HttpRequestDetail, KeyValueItem, RequestSearchHit, RequestSummary};
use crate::util::{err, now_ms};

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

pub(crate) fn map_detail(row: &Row<'_>) -> rusqlite::Result<HttpRequestDetail> {
    let headers: Option<String> = row.get(5)?;
    let query: Option<String> = row.get(6)?;
    let auth: Option<String> = row.get(12)?;
    Ok(HttpRequestDetail {
        id: Some(row.get(0)?),
        collection_id: row.get(1)?,
        name: row.get(2)?,
        method: row.get(3)?,
        url: row.get(4)?,
        headers: decode_list(headers),
        params: decode_list(query),
        body_type: row
            .get::<_, Option<String>>(7)?
            .unwrap_or_else(|| "none".into()),
        body: row.get::<_, Option<String>>(8)?.unwrap_or_default(),
        body_meta: row.get::<_, Option<String>>(13)?.unwrap_or_default(),
        pre_script: row.get::<_, Option<String>>(9)?.unwrap_or_default(),
        post_script: row.get::<_, Option<String>>(10)?.unwrap_or_default(),
        assertions: row.get::<_, Option<String>>(11)?.unwrap_or_default(),
        request_entity: row.get::<_, Option<String>>(14)?,
        response_entity: row.get::<_, Option<String>>(15)?,
        protocol: row
            .get::<_, Option<String>>(16)?
            .filter(|value| matches!(value.as_str(), "graphql" | "sse" | "websocket"))
            .unwrap_or_else(|| "http".to_string()),
        auth: auth
            .map(|text| crate::secret::decrypt_auth_json(&text))
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default(),
    })
}

pub(crate) fn find_request(conn: &Connection, id: i64) -> Result<HttpRequestDetail, String> {
    conn.query_row(
        "SELECT id, collection_id, name, method, url, headers, params,
                body_type, body, pre_script, post_script, assertions, auth, body_meta,
                request_entity, response_entity, protocol
         FROM requests WHERE id = ?1",
        params![id],
        map_detail,
    )
    .map_err(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => "请求不存在".to_string(),
        other => err(other),
    })
}

pub(crate) fn list_requests(
    conn: &Connection,
    collection_id: i64,
) -> Result<Vec<RequestSummary>, String> {
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

pub(crate) fn search_requests(
    conn: &Connection,
    project_id: i64,
    keyword: &str,
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

pub(crate) fn request_options(
    conn: &Connection,
    project_id: i64,
) -> Result<Vec<RequestSearchHit>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT r.id, r.collection_id, c.name, r.name, r.method, r.url, r.updated_at
             FROM requests r
             JOIN collections c ON c.id = r.collection_id
             WHERE c.project_id = ?1
             ORDER BY c.sort_order ASC, c.id ASC, r.sort_order ASC, r.id ASC",
        )
        .map_err(err)?;
    let rows = stmt
        .query_map(params![project_id], |row| {
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

/// 保存前统一整理字段：名称兜底、列表序列化、凭据加密
struct SaveFields {
    name: String,
    protocol: String,
    headers: String,
    params: String,
    auth: String,
}

fn save_fields(request: &HttpRequestDetail) -> Result<SaveFields, String> {
    let name = request.name.trim();
    let name = if name.is_empty() {
        "未命名请求"
    } else {
        name
    };
    Ok(SaveFields {
        name: name.to_string(),
        protocol: match request.protocol.as_str() {
            "graphql" | "sse" | "websocket" => request.protocol.clone(),
            _ => "http".to_string(),
        },
        headers: serde_json::to_string(&request.headers).map_err(err)?,
        params: serde_json::to_string(&request.params).map_err(err)?,
        auth: crate::secret::encrypt_auth_json(
            &serde_json::to_string(&request.auth).map_err(err)?,
        )?,
    })
}

fn update_existing(
    conn: &Connection,
    id: i64,
    request: &HttpRequestDetail,
    fields: &SaveFields,
    now: i64,
    force_entities: bool,
) -> Result<(), String> {
    let affected = conn
        .execute(
            "UPDATE requests SET name = ?1, method = ?2, protocol = ?3, url = ?4, headers = ?5,
                params = ?6, body_type = ?7, body = ?8, pre_script = ?9,
                post_script = ?10, assertions = ?11, auth = ?12, body_meta = ?13,
                request_entity = CASE WHEN ?14 THEN ?15 ELSE COALESCE(?15, request_entity) END,
                response_entity = CASE WHEN ?14 THEN ?16 ELSE COALESCE(?16, response_entity) END,
                updated_at = ?17
             WHERE id = ?18",
            params![
                fields.name,
                request.method,
                fields.protocol,
                request.url,
                fields.headers,
                fields.params,
                request.body_type,
                request.body,
                request.pre_script,
                request.post_script,
                request.assertions,
                fields.auth,
                request.body_meta,
                force_entities,
                request.request_entity.as_deref(),
                request.response_entity.as_deref(),
                now,
                id
            ],
        )
        .map_err(err)?;
    if affected == 0 {
        return Err("请求不存在".to_string());
    }
    Ok(())
}

/// 保存请求定义并追加版本快照。
///
/// `force_entities = true` 仅用于版本回滚：此时实体列以快照为准（含清空），
/// 普通保存仍保持「None = 不修改实体列」的约定。
pub(crate) fn save_detail(
    conn: &Connection,
    request: &HttpRequestDetail,
    source: &str,
    force_entities: bool,
) -> Result<HttpRequestDetail, String> {
    let fields = save_fields(request)?;
    let now = now_ms();
    let saved = match request.id {
        Some(id) => {
            update_existing(conn, id, request, &fields, now, force_entities)?;
            find_request(conn, id)?
        }
        None => {
            conn.execute(
                "INSERT INTO requests
                    (collection_id, name, method, protocol, url, headers, params, body_type, body,
                     body_meta, pre_script, post_script, assertions, auth, request_entity,
                     response_entity, sort_order, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, 0, ?17, ?17)",
                params![
                    request.collection_id,
                    fields.name,
                    request.method,
                    fields.protocol,
                    request.url,
                    fields.headers,
                    fields.params,
                    request.body_type,
                    request.body,
                    request.body_meta,
                    request.pre_script,
                    request.post_script,
                    request.assertions,
                    fields.auth,
                    request.request_entity.as_deref(),
                    request.response_entity.as_deref(),
                    now
                ],
            )
            .map_err(err)?;
            find_request(conn, conn.last_insert_rowid())?
        }
    };
    if let Some(id) = saved.id {
        crate::services::revision::record(conn, id, source)?;
    }
    Ok(saved)
}

pub(crate) fn delete_request(conn: &Connection, id: i64) -> Result<(), String> {
    conn.execute("DELETE FROM requests WHERE id = ?1", params![id])
        .map_err(err)?;
    Ok(())
}

/// 按给定顺序重排某集合下的请求（拖拽排序）。
pub(crate) fn reorder_requests(
    conn: &mut Connection,
    collection_id: i64,
    ids: &[i64],
) -> Result<(), String> {
    if ids.is_empty() {
        return Err("排序列表为空".to_string());
    }
    let tx = conn.transaction().map_err(err)?;
    for (index, id) in ids.iter().enumerate() {
        tx.execute(
            "UPDATE requests SET sort_order = ?1, updated_at = ?2
             WHERE id = ?3 AND collection_id = ?4",
            params![index as i64, now_ms(), id, collection_id],
        )
        .map_err(err)?;
    }
    tx.commit().map_err(err)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use r2d2_sqlite::SqliteConnectionManager;

    fn pool(tag: &str) -> crate::state::DbPool {
        crate::secret::install_for_test();
        let dir = crate::util::test_dir(tag);
        let manager = SqliteConnectionManager::file(dir.join("astroman.db"));
        let pool = r2d2::Pool::builder().max_size(2).build(manager).unwrap();
        db::init(&pool).unwrap();
        pool
    }

    fn seed(conn: &Connection) -> i64 {
        conn.execute(
            "INSERT INTO projects (name, description, color, sort_order, created_at, updated_at) VALUES ('P','',NULL,0,0,0)",
            [],
        )
        .unwrap();
        let project = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO collections (project_id, parent_id, name, sort_order, created_at, updated_at) VALUES (?1,NULL,'C',0,0,0)",
            params![project],
        )
        .unwrap();
        let collection = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO requests (collection_id,name,method,url,headers,params,body_type,body,body_meta,pre_script,post_script,assertions,auth,sort_order,created_at,updated_at)
             VALUES (?1,'R','GET','https://example.com','[]','[]','none','','','','','','{\"kind\":\"none\"}',0,0,0)",
            params![collection],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    #[test]
    fn entity_columns_round_trip_and_none_keeps_existing() {
        let pool = pool("request-entity");
        let conn = pool.get().unwrap();
        let id = seed(&conn);

        let loaded = find_request(&conn, id).unwrap();
        assert!(loaded.request_entity.is_none());
        assert!(loaded.response_entity.is_none());

        let entity = r#"{"version":1,"updatedAt":0,"hiddenPaths":[],"root":{"name":"$","type":"object","itemType":"","required":false,"description":"","manual":false,"stale":false,"children":[]}}"#;
        let mut edited = loaded.clone();
        edited.request_entity = Some(entity.to_string());
        let fields = save_fields(&edited).unwrap();
        update_existing(&conn, id, &edited, &fields, 1, false).unwrap();
        assert_eq!(
            find_request(&conn, id).unwrap().request_entity.as_deref(),
            Some(entity)
        );

        // 保存时实体为 None 表示「不修改该列」，不能被清空
        let mut plain = find_request(&conn, id).unwrap();
        plain.request_entity = None;
        plain.response_entity = None;
        let fields = save_fields(&plain).unwrap();
        update_existing(&conn, id, &plain, &fields, 2, false).unwrap();
        assert_eq!(
            find_request(&conn, id).unwrap().request_entity.as_deref(),
            Some(entity)
        );
    }
}
