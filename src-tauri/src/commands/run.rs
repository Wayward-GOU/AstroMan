use std::collections::HashMap;

use crate::models::{HttpResult, KeyValueItem, RequestRun, RequestRunSummary, SendRequestPayload};
use crate::state::{AppState, DbPool};
use crate::util::{err, now_ms};
use rusqlite::params;
use tauri::State;

/// 单个接口最多保留的运行记录条数
const MAX_RUNS: i64 = 200;
/// 请求 / 响应体入库上限，避免大 Body 撑大数据库
const MAX_STORED_BODY: usize = 256 * 1024;
/// 列表预览长度（字符），只够用户判断是不是自己要看的那次请求
const PREVIEW_CHARS: i64 = 2048;

/// 发送请求后写入一条运行快照（由 http 命令内部调用）
///
/// 参数就是一次请求的完整快照，拆开传反而更容易漏字段。
#[allow(clippy::too_many_arguments)]
pub fn record_run(
    pool: &DbPool,
    payload: &SendRequestPayload,
    request_id: i64,
    url: &str,
    headers: &[(String, String)],
    body: &str,
    result: &HttpResult,
    origin: &str,
) -> Result<(), String> {
    let conn = pool.get().map_err(err)?;

    let headers_json = serde_json::to_string(
        &headers
            .iter()
            .map(|(key, value)| KeyValueItem {
                key: key.clone(),
                value: value.clone(),
                enabled: true,
            })
            .collect::<Vec<_>>(),
    )
    .map_err(err)?;
    let params_json = serde_json::to_string(&payload.params).map_err(err)?;
    let response_headers_json = serde_json::to_string(&result.headers).map_err(err)?;

    let request_bytes = body.as_bytes();
    let request_truncated = request_bytes.len() > MAX_STORED_BODY;
    let stored_request_body = if request_truncated {
        String::from_utf8_lossy(&request_bytes[..MAX_STORED_BODY]).to_string()
    } else {
        body.to_string()
    };

    let response_bytes = result.body.as_bytes();
    let response_truncated = response_bytes.len() > MAX_STORED_BODY;
    let response_body = if response_truncated {
        String::from_utf8_lossy(&response_bytes[..MAX_STORED_BODY]).to_string()
    } else {
        result.body.clone()
    };

    conn.execute(
        "INSERT INTO request_runs
            (request_id, method, url, headers, params, body_type, body,
             response_status, response_status_text, response_headers, response_body,
             duration_ms, size_bytes, error, created_at, origin,
             request_body_size, response_body_size,
             request_body_truncated, response_body_truncated)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16,
                 ?17, ?18, ?19, ?20)",
        params![
            request_id,
            payload.method.to_uppercase(),
            url,
            headers_json,
            params_json,
            payload.body_type,
            stored_request_body,
            result.status as i64,
            result.status_text,
            response_headers_json,
            response_body,
            result.duration_ms as i64,
            result.size_bytes as i64,
            result.error,
            now_ms(),
            origin,
            request_bytes.len() as i64,
            response_bytes.len() as i64,
            request_truncated as i64,
            response_truncated as i64,
        ],
    )
    .map_err(err)?;

    // 只保留最近 MAX_RUNS 条
    conn.execute(
        "DELETE FROM request_runs
         WHERE request_id = ?1 AND id NOT IN (
             SELECT id FROM request_runs WHERE request_id = ?1
             ORDER BY id DESC LIMIT ?2
         )",
        params![request_id, MAX_RUNS],
    )
    .map_err(err)?;

    Ok(())
}

fn parse_items(raw: Option<String>) -> Vec<KeyValueItem> {
    raw.and_then(|text| serde_json::from_str::<Vec<KeyValueItem>>(&text).ok())
        .unwrap_or_default()
}

fn parse_headers(raw: Option<String>) -> HashMap<String, String> {
    raw.and_then(|text| serde_json::from_str::<HashMap<String, String>>(&text).ok())
        .unwrap_or_default()
}

#[tauri::command]
pub fn run_list(
    state: State<'_, AppState>,
    request_id: i64,
    limit: Option<u32>,
    origin: Option<String>,
) -> Result<Vec<RequestRunSummary>, String> {
    let limit = limit.unwrap_or(MAX_RUNS as u32).clamp(1, MAX_RUNS as u32) as i64;
    let origin = origin
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let conn = state.pool.get().map_err(err)?;
    let mut stmt = conn
        .prepare(
            "SELECT id, request_id, method, url, body_type,
                    response_status, COALESCE(response_status_text, ''),
                    duration_ms, size_bytes, error, COALESCE(origin, 'manual'), created_at,
                    CASE WHEN request_body_size > 0 THEN request_body_size
                         ELSE COALESCE(length(CAST(body AS BLOB)), 0) END,
                    CASE WHEN response_body_size > 0 THEN response_body_size
                         ELSE COALESCE(length(CAST(response_body AS BLOB)), 0) END,
                    CASE WHEN request_body_truncated <> 0 THEN 1 ELSE 0 END,
                    CASE WHEN response_body_truncated <> 0 THEN 1 ELSE 0 END,
                    substr(COALESCE(body, ''), 1, ?4),
                    substr(COALESCE(response_body, ''), 1, ?4)
             FROM request_runs WHERE request_id = ?1
               AND (?3 IS NULL OR origin = ?3)
             ORDER BY id DESC LIMIT ?2",
        )
        .map_err(err)?;
    let rows = stmt
        .query_map(params![request_id, limit, origin, PREVIEW_CHARS], |row| {
            Ok(RequestRunSummary {
                id: row.get(0)?,
                request_id: row.get(1)?,
                method: row.get(2)?,
                url: row.get(3)?,
                body_type: row
                    .get::<_, Option<String>>(4)?
                    .unwrap_or_else(|| "none".into()),
                response_status: row.get(5)?,
                response_status_text: row.get(6)?,
                duration_ms: row.get(7)?,
                size_bytes: row.get(8)?,
                error: row.get(9)?,
                origin: row.get(10)?,
                created_at: row.get(11)?,
                request_body_size: row.get(12)?,
                response_body_size: row.get(13)?,
                request_body_truncated: row.get(14)?,
                response_body_truncated: row.get(15)?,
                request_body_preview: row.get(16)?,
                response_body_preview: row.get(17)?,
            })
        })
        .map_err(err)?;

    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(err)
}

/// 展开某条运行记录时读取完整快照（请求 / 响应正文最多各 256KB）
#[tauri::command]
pub fn run_get(state: State<'_, AppState>, id: i64) -> Result<RequestRun, String> {
    let conn = state.pool.get().map_err(err)?;
    let mut stmt = conn
        .prepare(
            "SELECT id, request_id, method, url, headers, params, body_type, body,
                    response_status, response_status_text, response_headers, response_body,
                    duration_ms, size_bytes, error, origin, created_at,
                    request_body_size, response_body_size,
                    request_body_truncated, response_body_truncated
             FROM request_runs WHERE id = ?1",
        )
        .map_err(err)?;
    let mut rows = stmt
        .query_map(params![id], |row| {
            Ok(RequestRun {
                id: row.get(0)?,
                request_id: row.get(1)?,
                method: row.get(2)?,
                url: row.get(3)?,
                headers: parse_items(row.get::<_, Option<String>>(4)?),
                params: parse_items(row.get::<_, Option<String>>(5)?),
                body_type: row.get(6)?,
                body: row.get::<_, Option<String>>(7)?.unwrap_or_default(),
                response_status: row.get(8)?,
                response_status_text: row.get::<_, Option<String>>(9)?.unwrap_or_default(),
                response_headers: parse_headers(row.get::<_, Option<String>>(10)?),
                response_body: row.get::<_, Option<String>>(11)?.unwrap_or_default(),
                duration_ms: row.get(12)?,
                size_bytes: row.get(13)?,
                error: row.get(14)?,
                origin: row.get(15).unwrap_or_default(),
                created_at: row.get(16)?,
                request_body_size: row.get(17)?,
                response_body_size: row.get(18)?,
                request_body_truncated: row.get(19)?,
                response_body_truncated: row.get(20)?,
            })
        })
        .map_err(err)?;

    rows.next()
        .transpose()
        .map_err(err)?
        .ok_or_else(|| "运行记录不存在".to_string())
}

#[tauri::command]
pub fn run_clear(state: State<'_, AppState>, request_id: i64) -> Result<(), String> {
    let conn = state.pool.get().map_err(err)?;
    conn.execute(
        "DELETE FROM request_runs WHERE request_id = ?1",
        params![request_id],
    )
    .map_err(err)?;
    Ok(())
}
