//! 接口实体：字段树推断、合并与发送后自动同步
//!
//! 与 `docs` 解耦：本模块只负责「字段结构 + 说明」，不感知任何文档格式；
//! `docs::builder` 读取实体列后自行转换为 `DocEntity`。

pub mod infer;
pub mod merge;
pub mod model;
pub mod schema;

use crate::models::HttpResult;
use crate::state::DbPool;
use crate::util::{err, now_ms};
use rusqlite::{params, OptionalExtension};

use model::EntityDoc;

/// 手工发送成功后旁路同步实体。
///
/// best-effort：请求体 / 响应体任一可推断就写对应列；不可推断时保留原实体。
/// 只处理 `origin = manual`，避免批量 / 套件 / 压测的脚本化请求把实体冲成噪声。
pub fn sync_after_send(
    pool: &DbPool,
    request_id: i64,
    origin: &str,
    body: &str,
    body_type: &str,
    result: &HttpResult,
) -> Result<(), String> {
    if origin != "manual" {
        return Ok(());
    }
    let conn = pool.get().map_err(err)?;
    let row = conn
        .query_row(
            "SELECT request_entity, response_entity FROM requests WHERE id = ?1",
            params![request_id],
            |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, Option<String>>(1)?,
                ))
            },
        )
        .optional()
        .map_err(err)?;
    let Some((old_request, old_response)) = row else {
        return Ok(());
    };

    let now = now_ms();
    let mut next_request: Option<String> = None;
    if let Ok(inferred) = infer::infer_body(body_type, body) {
        let existing = decode_lenient(old_request.as_deref());
        let merged = merge::merge(existing, inferred.root, false, now);
        log_warnings("请求实体", &merged.warnings, &inferred.warnings);
        next_request = Some(merged.doc.encode()?);
    }

    let mut next_response: Option<String> = None;
    if (200..300).contains(&result.status) && !result.truncated && !result.body.trim().is_empty() {
        if let Ok(inferred) = infer::infer_json(&result.body) {
            let existing = decode_lenient(old_response.as_deref());
            let merged = merge::merge(existing, inferred.root, false, now);
            log_warnings("返回实体", &merged.warnings, &inferred.warnings);
            next_response = Some(merged.doc.encode()?);
        }
    }

    if next_request.is_none() && next_response.is_none() {
        return Ok(());
    }
    conn.execute(
        "UPDATE requests
            SET request_entity = COALESCE(?2, request_entity),
                response_entity = COALESCE(?3, response_entity)
          WHERE id = ?1",
        params![request_id, next_request, next_response],
    )
    .map_err(err)?;
    Ok(())
}

fn log_warnings(label: &str, merge_warnings: &[String], infer_warnings: &[String]) {
    for warning in merge_warnings.iter().chain(infer_warnings.iter()) {
        log::debug!("{label}自动同步：{warning}");
    }
}

/// 读库时容忍损坏 / 旧版本：按「无实体」处理，不阻断发送与导出
pub fn decode_lenient(raw: Option<&str>) -> Option<EntityDoc> {
    let raw = raw?.trim();
    if raw.is_empty() {
        return None;
    }
    EntityDoc::decode(raw).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::models::HttpResult;
    use r2d2_sqlite::SqliteConnectionManager;
    use std::collections::HashMap;

    fn pool(tag: &str) -> DbPool {
        let dir = crate::util::test_dir(tag);
        let manager = SqliteConnectionManager::file(dir.join("astroman.db"));
        let pool = r2d2::Pool::builder().max_size(2).build(manager).unwrap();
        db::init(&pool).unwrap();
        pool
    }

    fn seed(conn: &rusqlite::Connection) -> i64 {
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
             VALUES (?1,'R','POST','https://example.com','[]','[]','none','','','','','','{\"kind\":\"none\"}',0,0,0)",
            params![collection],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    fn http_result(status: u16, body: &str) -> HttpResult {
        HttpResult {
            status,
            status_text: String::new(),
            duration_ms: 1,
            size_bytes: body.len(),
            headers: HashMap::new(),
            body: body.to_string(),
            truncated: false,
            error: None,
            body_kind: "json".to_string(),
            body_file: None,
            body_bytes: body.len() as u64,
            raw_bytes: body.len() as u64,
            preview_base64: None,
            timings: None,
            http_version: "HTTP/1.1".to_string(),
            redirects: Vec::new(),
            retries: Vec::new(),
            ignored_headers: Vec::new(),
            cancelled: false,
        }
    }

    fn columns(conn: &rusqlite::Connection, id: i64) -> (Option<String>, Option<String>) {
        conn.query_row(
            "SELECT request_entity, response_entity FROM requests WHERE id = ?1",
            params![id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap()
    }

    #[test]
    fn lenient_decode_ignores_broken_or_unknown_versions() {
        assert!(decode_lenient(None).is_none());
        assert!(decode_lenient(Some("   ")).is_none());
        assert!(decode_lenient(Some("{")).is_none());
        let raw = r#"{"version":9,"updatedAt":0,"hiddenPaths":[],"root":{"name":"$","type":"object","itemType":"","required":false,"description":"","manual":false,"stale":false,"children":[]}}"#;
        assert!(decode_lenient(Some(raw)).is_none());
    }

    #[test]
    fn manual_send_syncs_entities_and_skips_other_origins() {
        let pool = pool("entity-sync");
        let conn = pool.get().unwrap();
        let id = seed(&conn);

        // 批量 / 套件 / 压测来源不写实体
        sync_after_send(
            &pool,
            id,
            "batch",
            r#"{"secret":"tok-123456"}"#,
            "json",
            &http_result(200, r#"{"ok":true}"#),
        )
        .unwrap();
        assert_eq!(columns(&conn, id), (None, None));

        let body = r#"{"password":"tok-123456","data":{"roles":["admin"]}}"#;
        sync_after_send(
            &pool,
            id,
            "manual",
            body,
            "json",
            &http_result(200, r#"{"items":[{"id":1}]}"#),
        )
        .unwrap();
        let (request_raw, response_raw) = columns(&conn, id);
        let request = EntityDoc::decode(&request_raw.unwrap()).unwrap();
        let response = EntityDoc::decode(&response_raw.unwrap()).unwrap();
        let request_fields = request.flatten();
        let names: Vec<&str> = request_fields
            .iter()
            .map(|field| field.path.as_str())
            .collect();
        assert_eq!(names, vec!["password", "data", "data.roles[]"]);
        // 实体只存结构，不保存字段取值
        assert!(!request.encode().unwrap().contains("tok-123456"));
        let response_fields = response.flatten();
        let response_names: Vec<&str> = response_fields
            .iter()
            .map(|field| field.path.as_str())
            .collect();
        assert_eq!(response_names, vec!["items[]", "items[].id"]);

        // 失败响应不覆盖返回实体
        let before = columns(&conn, id).1;
        sync_after_send(
            &pool,
            id,
            "manual",
            body,
            "json",
            &http_result(500, r#"{"error":"boom"}"#),
        )
        .unwrap();
        assert_eq!(columns(&conn, id).1, before);
    }
}
