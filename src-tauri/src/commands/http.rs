use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::models::{HttpResult, SendRequestPayload};
use crate::state::AppState;
use crate::util::{err, now_ms, render};
use rusqlite::{params, Connection};
use tauri::State;

const MAX_BODY: usize = 5 * 1024 * 1024;

#[tauri::command]
pub async fn send_http_request(
    state: State<'_, AppState>,
    payload: SendRequestPayload,
) -> Result<HttpResult, String> {
    let vars = match payload.env_id {
        Some(env_id) => {
            let conn = state.pool.get().map_err(err)?;
            load_vars(&conn, env_id)?
        }
        None => HashMap::new(),
    };

    let mut url = render(payload.url.trim(), &vars).trim().to_string();
    if url.is_empty() {
        return Err("URL 不能为空".to_string());
    }
    if !url.contains("://") {
        url = format!("http://{url}");
    }

    let mut parsed = reqwest::Url::parse(&url).map_err(|e| format!("URL 无法解析：{e}"))?;
    if !payload.params.is_empty() {
        let mut pairs = parsed.query_pairs_mut();
        for item in &payload.params {
            if !item.enabled {
                continue;
            }
            let key = render(item.key.trim(), &vars);
            if key.is_empty() {
                continue;
            }
            pairs.append_pair(&key, &render(&item.value, &vars));
        }
    }
    let url_for_history = parsed.as_str().to_string();

    let method = reqwest::Method::from_bytes(payload.method.trim().to_uppercase().as_bytes())
        .map_err(|_| format!("不支持的请求方法：{}", payload.method))?;

    let mut builder = state
        .client(payload.insecure)
        .request(method, parsed)
        .timeout(Duration::from_millis(payload.timeout_ms.max(500)));

    let mut has_content_type = false;
    let mut applied_headers: Vec<(String, String)> = Vec::new();
    for item in &payload.headers {
        if !item.enabled {
            continue;
        }
        let key = render(item.key.trim(), &vars);
        if key.is_empty() {
            continue;
        }
        if key.eq_ignore_ascii_case("content-type") {
            has_content_type = true;
        }
        let value = render(&item.value, &vars);
        applied_headers.push((key.clone(), value.clone()));
        builder = builder.header(key, value);
    }

    let body_type = payload.body_type.as_str();
    let mut sent_body = String::new();
    if !body_type.is_empty() && body_type != "none" {
        if !has_content_type {
            let content_type = match body_type {
                "json" => "application/json",
                "form" => "application/x-www-form-urlencoded",
                _ => "text/plain; charset=utf-8",
            };
            applied_headers.push(("Content-Type".to_string(), content_type.to_string()));
            builder = builder.header(reqwest::header::CONTENT_TYPE, content_type);
        }
        sent_body = render(payload.body.as_deref().unwrap_or(""), &vars);
        builder = builder.body(sent_body.clone());
    }

    let started = Instant::now();
    match builder.send().await {
        Ok(response) => {
            let result = read_response(response, started).await;
            record_history(
                &state,
                &payload,
                &url_for_history,
                Some(result.status as i64),
                result.duration_ms as i64,
            );
            record_run_snapshot(
                &state,
                &payload,
                &url_for_history,
                &applied_headers,
                &sent_body,
                &result,
            );
            Ok(result)
        }
        Err(error) => {
            let duration = started.elapsed().as_millis() as i64;
            record_history(&state, &payload, &url_for_history, None, duration);
            let result = HttpResult {
                status: 0,
                status_text: String::new(),
                duration_ms: duration as u64,
                size_bytes: 0,
                headers: HashMap::new(),
                body: String::new(),
                truncated: false,
                error: Some(error.to_string()),
            };
            record_run_snapshot(
                &state,
                &payload,
                &url_for_history,
                &applied_headers,
                &sent_body,
                &result,
            );
            Ok(result)
        }
    }
}

/// 写入单接口的运行快照，失败不影响本次请求结果
fn record_run_snapshot(
    state: &AppState,
    payload: &SendRequestPayload,
    url: &str,
    headers: &[(String, String)],
    body: &str,
    result: &HttpResult,
) {
    let Some(request_id) = payload.request_id else {
        return;
    };
    let _ = crate::commands::run::record_run(
        &state.pool,
        payload,
        request_id,
        url,
        headers,
        body,
        result,
    );
}

async fn read_response(response: reqwest::Response, started: Instant) -> HttpResult {
    let status = response.status();
    let status_text = status.canonical_reason().unwrap_or_default().to_string();

    let mut headers = HashMap::new();
    for (name, value) in response.headers().iter() {
        headers.insert(
            name.as_str().to_string(),
            value.to_str().unwrap_or_default().to_string(),
        );
    }

    let mut response = response;
    let mut buffer: Vec<u8> = Vec::new();
    let mut truncated = false;
    let mut read_error: Option<String> = None;

    loop {
        match response.chunk().await {
            Ok(Some(chunk)) => {
                if buffer.len() + chunk.len() > MAX_BODY {
                    let remain = MAX_BODY - buffer.len();
                    buffer.extend_from_slice(&chunk[..remain]);
                    truncated = true;
                    break;
                }
                buffer.extend_from_slice(&chunk);
            }
            Ok(None) => break,
            Err(e) => {
                read_error = Some(e.to_string());
                break;
            }
        }
    }

    HttpResult {
        status: status.as_u16(),
        status_text,
        duration_ms: started.elapsed().as_millis() as u64,
        size_bytes: buffer.len(),
        headers,
        body: String::from_utf8_lossy(&buffer).into_owned(),
        truncated,
        error: read_error,
    }
}

pub(crate) fn load_vars(conn: &Connection, env_id: i64) -> Result<HashMap<String, String>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT \"key\", value FROM env_variables
             WHERE env_id = ?1 ORDER BY sort_order ASC, id ASC",
        )
        .map_err(err)?;
    let rows = stmt
        .query_map(params![env_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?.unwrap_or_default(),
            ))
        })
        .map_err(err)?;

    let mut vars = HashMap::new();
    for item in rows {
        let (key, value) = item.map_err(err)?;
        if !key.is_empty() {
            vars.insert(key, value);
        }
    }
    Ok(vars)
}

fn record_history(
    state: &AppState,
    payload: &SendRequestPayload,
    url: &str,
    status: Option<i64>,
    duration_ms: i64,
) {
    let Ok(conn) = state.pool.get() else {
        return;
    };

    let name = payload
        .name
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| url.to_string());

    let _ = conn.execute(
        "INSERT INTO request_history
            (request_id, request_name, method, url, status_code, duration_ms, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            payload.request_id,
            name,
            payload.method,
            url,
            status,
            duration_ms,
            now_ms()
        ],
    );

    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM request_history", [], |row| row.get(0))
        .unwrap_or(0);
    if count > 5000 {
        let _ = conn.execute(
            "DELETE FROM request_history
             WHERE id IN (SELECT id FROM request_history ORDER BY id ASC LIMIT ?1)",
            params![count - 5000],
        );
    }
}
