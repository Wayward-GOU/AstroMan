use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, Instant};

use crate::commands::cookie;
use crate::models::{FormField, HttpResult, SendRequestPayload};
use crate::state::AppState;
use crate::util::{err, now_ms, render, strip_json_comments};
use rusqlite::{params, Connection};
use tauri::State;

const MAX_BODY: usize = 5 * 1024 * 1024;
/// 请求历史最多保留最近 10 万条
const MAX_HISTORY: i64 = 100_000;

#[tauri::command]
pub async fn send_http_request(
    state: State<'_, AppState>,
    payload: SendRequestPayload,
) -> Result<HttpResult, String> {
    execute(state.inner(), payload).await
}

/// 真正发请求的实现：手动发送与 AI 执行共用，保证历史、Cookie、快照逻辑完全一致
pub(crate) async fn execute(
    state: &AppState,
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

    // API Key 认证可以放在查询参数上
    let auth = payload.auth.clone().unwrap_or_default();
    if auth.kind == "apikey" && auth.add_to == "query" && !auth.key.trim().is_empty() {
        let mut pairs = parsed.query_pairs_mut();
        pairs.append_pair(&render(auth.key.trim(), &vars), &render(&auth.value, &vars));
    }
    let url_for_history = parsed.as_str().to_string();

    let method = reqwest::Method::from_bytes(payload.method.trim().to_uppercase().as_bytes())
        .map_err(|_| format!("不支持的请求方法：{}", payload.method))?;

    let body_type = payload.body_type.as_str();

    let mut builder = state
        .client(payload.insecure)
        .request(method, parsed.clone())
        .timeout(Duration::from_millis(payload.timeout_ms.max(500)));

    let mut has_content_type = false;
    let mut has_authorization = false;
    let mut has_cookie = false;
    let mut applied_headers: Vec<(String, String)> = Vec::new();
    for item in &payload.headers {
        if !item.enabled {
            continue;
        }
        let key = render(item.key.trim(), &vars);
        if key.is_empty() {
            continue;
        }
        let lower = key.to_ascii_lowercase();
        if lower == "content-type" {
            // multipart 的 Content-Type 必须由 reqwest 自动生成（带 boundary）
            if body_type == "multipart" {
                continue;
            }
            has_content_type = true;
        }
        if lower == "authorization" {
            has_authorization = true;
        }
        if lower == "cookie" {
            has_cookie = true;
        }
        let value = render(&item.value, &vars);
        applied_headers.push((key.clone(), value.clone()));
        builder = builder.header(key, value);
    }

    // 认证头：用户手动配置的 Authorization 优先
    if !has_authorization {
        match auth.kind.as_str() {
            "bearer" | "oauth2" if !auth.token.trim().is_empty() => {
                let value = format!("Bearer {}", render(auth.token.trim(), &vars));
                applied_headers.push(("Authorization".to_string(), value.clone()));
                builder = builder.header(reqwest::header::AUTHORIZATION, value);
            }
            "basic" => {
                let raw = format!(
                    "{}:{}",
                    render(&auth.username, &vars),
                    render(&auth.password, &vars)
                );
                let value = format!("Basic {}", base64_encode(raw.as_bytes()));
                applied_headers.push(("Authorization".to_string(), value.clone()));
                builder = builder.header(reqwest::header::AUTHORIZATION, value);
            }
            "apikey" if auth.add_to != "query" && !auth.key.trim().is_empty() => {
                let key = render(auth.key.trim(), &vars);
                let value = render(&auth.value, &vars);
                applied_headers.push((key.clone(), value.clone()));
                builder = builder.header(key, value);
            }
            _ => {}
        }
    }

    // 自动携带本机 Cookie
    if payload.cookie_enabled.unwrap_or(true) && !has_cookie {
        if let Ok(conn) = state.pool.get() {
            let cookies = cookie::cookies_for_url(&conn, &parsed);
            if !cookies.is_empty() {
                let value = cookies.join("; ");
                applied_headers.push(("Cookie".to_string(), value.clone()));
                builder = builder.header(reqwest::header::COOKIE, value);
            }
        }
    }

    let mut sent_body = String::new();
    if !body_type.is_empty() && body_type != "none" {
        match body_type {
            "multipart" => {
                let fields: Vec<FormField> =
                    serde_json::from_str(payload.body.as_deref().unwrap_or("[]")).unwrap_or_default();
                let mut form = reqwest::multipart::Form::new();
                for field in fields.iter().filter(|f| f.enabled) {
                    let key = render(field.key.trim(), &vars);
                    if key.is_empty() {
                        continue;
                    }
                    if field.kind == "file" {
                        let path = render(field.value.trim(), &vars);
                        if path.is_empty() {
                            continue;
                        }
                        let bytes = std::fs::read(&path)
                            .map_err(|e| format!("读取上传文件失败（{path}）：{e}"))?;
                        let filename = Path::new(&path)
                            .file_name()
                            .map(|name| name.to_string_lossy().to_string())
                            .unwrap_or_else(|| "file".to_string());
                        form = form.part(
                            key,
                            reqwest::multipart::Part::bytes(bytes).file_name(filename),
                        );
                    } else {
                        form = form.text(key, render(&field.value, &vars));
                    }
                }
                sent_body = serde_json::to_string(&fields).unwrap_or_default();
                builder = builder.multipart(form);
            }
            "binary" => {
                let path = render(payload.body.as_deref().unwrap_or("").trim(), &vars);
                if path.is_empty() {
                    return Err("请选择要上传的二进制文件".to_string());
                }
                let bytes = std::fs::read(&path)
                    .map_err(|e| format!("读取上传文件失败（{path}）：{e}"))?;
                if !has_content_type {
                    applied_headers
                        .push(("Content-Type".to_string(), "application/octet-stream".to_string()));
                    builder = builder.header(reqwest::header::CONTENT_TYPE, "application/octet-stream");
                }
                sent_body = format!("[二进制文件] {path}（{} 字节）", bytes.len());
                builder = builder.body(bytes);
            }
            _ => {
                if !has_content_type {
                    let content_type = match body_type {
                        "json" => "application/json",
                        "graphql" => "application/json",
                        "form" => "application/x-www-form-urlencoded",
                        _ => "text/plain; charset=utf-8",
                    };
                    applied_headers.push(("Content-Type".to_string(), content_type.to_string()));
                    builder = builder.header(reqwest::header::CONTENT_TYPE, content_type);
                }
                let raw_body = payload.body.as_deref().unwrap_or("");
                // JSON 允许写注释（Postman 的 raw/JSON 编辑器同样如此），发送前去掉，避免服务端解析失败
                let source = if body_type == "json" {
                    strip_json_comments(raw_body)
                } else {
                    raw_body.to_string()
                };
                sent_body = render(&source, &vars);
                builder = builder.body(sent_body.clone());
            }
        }
    }

    let started = Instant::now();
    match builder.send().await {
        Ok(response) => {
            let (result, set_cookies) = read_response(response, started).await;
            if payload.cookie_enabled.unwrap_or(true) && !set_cookies.is_empty() {
                if let Ok(conn) = state.pool.get() {
                    cookie::store_response_cookies(&conn, &parsed, &set_cookies);
                }
            }
            record_history(
                state,
                &payload,
                &url_for_history,
                Some(result.status as i64),
                result.duration_ms as i64,
            );
            record_run_snapshot(
                state,
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
            record_history(state, &payload, &url_for_history, None, duration);
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
                state,
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

async fn read_response(response: reqwest::Response, started: Instant) -> (HttpResult, Vec<String>) {
    let status = response.status();
    let status_text = status.canonical_reason().unwrap_or_default().to_string();

    let mut headers = HashMap::new();
    let mut set_cookies = Vec::new();
    for (name, value) in response.headers().iter() {
        if name.as_str().eq_ignore_ascii_case("set-cookie") {
            if let Ok(text) = value.to_str() {
                set_cookies.push(text.to_string());
            }
        }
        headers
            .entry(name.as_str().to_string())
            .or_insert_with(|| value.to_str().unwrap_or_default().to_string());
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

    (
        HttpResult {
            status: status.as_u16(),
            status_text,
            duration_ms: started.elapsed().as_millis() as u64,
            size_bytes: buffer.len(),
            headers,
            body: String::from_utf8_lossy(&buffer).into_owned(),
            truncated,
            error: read_error,
        },
        set_cookies,
    )
}

/// 标准 Base64 编码（Basic 认证使用）
fn base64_encode(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((input.len() + 2) / 3 * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let value = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[((value >> 18) & 63) as usize] as char);
        out.push(TABLE[((value >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            TABLE[((value >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[(value & 63) as usize] as char
        } else {
            '='
        });
    }
    out
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
    if count > MAX_HISTORY {
        let _ = conn.execute(
            "DELETE FROM request_history
             WHERE id IN (SELECT id FROM request_history ORDER BY id ASC LIMIT ?1)",
            params![count - MAX_HISTORY],
        );
    }
}
