use std::collections::HashMap;

use crate::models::{HttpResult, KeyValueItem, RequestRun, SendRequestPayload};
use crate::state::{AppState, DbPool};
use crate::util::{err, now_ms};
use rusqlite::params;
use tauri::State;

/// 单个接口最多保留的运行记录条数
const MAX_RUNS: i64 = 50;
/// 响应体入库上限，避免大响应撑大数据库
const MAX_STORED_BODY: usize = 256 * 1024;

/// 发送请求后写入一条运行快照（由 http 命令内部调用）
pub fn record_run(
    pool: &DbPool,
    payload: &SendRequestPayload,
    request_id: i64,
    url: &str,
    headers: &[(String, String)],
    body: &str,
    result: &HttpResult,
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

    let body_bytes = result.body.as_bytes();
    let response_body = if body_bytes.len() > MAX_STORED_BODY {
        String::from_utf8_lossy(&body_bytes[..MAX_STORED_BODY]).to_string()
    } else {
        result.body.clone()
    };

    conn.execute(
        "INSERT INTO request_runs
            (request_id, method, url, headers, params, body_type, body,
             response_status, response_status_text, response_headers, response_body,
             duration_ms, size_bytes, error, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
        params![
            request_id,
            payload.method.to_uppercase(),
            url,
            headers_json,
            params_json,
            payload.body_type,
            body,
            result.status as i64,
            result.status_text,
            response_headers_json,
            response_body,
            result.duration_ms as i64,
            result.size_bytes as i64,
            result.error,
            now_ms(),
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
) -> Result<Vec<RequestRun>, String> {
    let limit = limit.unwrap_or(MAX_RUNS as u32).clamp(1, MAX_RUNS as u32) as i64;
    let conn = state.pool.get().map_err(err)?;
    let mut stmt = conn
        .prepare(
            "SELECT id, request_id, method, url, headers, params, body_type, body,
                    response_status, response_status_text, response_headers, response_body,
                    duration_ms, size_bytes, error, created_at
             FROM request_runs WHERE request_id = ?1
             ORDER BY id DESC LIMIT ?2",
        )
        .map_err(err)?;
    let rows = stmt
        .query_map(params![request_id, limit], |row| {
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
                created_at: row.get(15)?,
            })
        })
        .map_err(err)?;

    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(err)
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
