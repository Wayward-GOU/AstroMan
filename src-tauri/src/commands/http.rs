//! HTTP 请求执行：响应体判定与落盘、耗时分解、重定向链、取消与重试
//!
//! 与 v1.0.8 的差别（技术方案 4.4 / 4.5 / 4.6）：
//! - 正文先解压再判定再截取：`Content-Encoding` 由 reqwest 处理，判定看到的是解压后字节；
//! - 二进制与超大响应落盘，内存里只留预览，完整响应可另存；
//! - 手工跟随重定向并记录每一跳，总耗时 = 各跳之和；
//! - 建连耗时来自连接器探针（`net_probe`），复用连接时为 0；
//! - 支持取消与幂等重试；
//! - 请求头在 HTTP/2 下自动剔除 connection-specific 项，并回报被忽略的头。

use std::collections::HashMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use rusqlite::{params, Connection};
use tauri::State;

use crate::commands::cookie;
use crate::models::{
    FormField, HttpResult, RedirectHop, RequestTimings, RetryAttempt, SendRequestPayload,
};
use crate::secret::{self, AAD_ENV};
use crate::state::AppState;
use crate::util::{err, now_ms, render, strip_json_comments};

/// 正文入内存上限：超出即截断（4.0.4）
const MAX_BODY: usize = 5 * 1024 * 1024;
/// 超过该阈值同时流式落盘，供「保存完整响应」使用
const SPILL_THRESHOLD: u64 = 2 * 1024 * 1024;
/// 落盘时保留的预览片段
const PREVIEW_BYTES: usize = 64 * 1024;
/// 判定正文类型前先取这么多字节做嗅探
const SNIFF_BYTES: usize = 1024;
/// 请求历史最多保留最近 10 万条
const MAX_HISTORY: i64 = 100_000;
/// 手工跟随重定向的最大跳数
const MAX_REDIRECTS: usize = 10;
/// 中间跳响应体的排空上限：超过即放弃排空（连接会被关闭，但不会拖慢整体）
const DRAIN_LIMIT: usize = 256 * 1024;
/// 重试退避基数
const RETRY_BASE_MS: u64 = 200;

/// 落库策略：套件 / CLI 运行默认不写单接口历史与快照，避免冲掉手工调试记录
#[derive(Debug, Clone)]
pub struct RecordOptions {
    pub origin: String,
    pub history: bool,
    pub run_snapshot: bool,
}

impl RecordOptions {
    pub fn manual() -> Self {
        Self {
            origin: "manual".to_string(),
            history: true,
            run_snapshot: true,
        }
    }

    /// 套件运行：默认不写历史与快照，避免冲掉手工调试记录
    pub fn suite(origin: &str) -> Self {
        Self {
            origin: origin.to_string(),
            history: false,
            run_snapshot: false,
        }
    }

    pub fn from_payload(payload: &SendRequestPayload) -> Self {
        let mut options = Self::manual();
        if let Some(origin) = payload.origin.as_deref().map(str::trim) {
            if !origin.is_empty() {
                options.origin = origin.to_string();
            }
        }
        options
    }
}

#[tauri::command]
pub async fn send_http_request(
    state: State<'_, AppState>,
    payload: SendRequestPayload,
) -> Result<HttpResult, String> {
    let options = RecordOptions::from_payload(&payload);
    execute_with(state.inner(), payload, options, None).await
}

/// 停止在途请求（4.6）
#[tauri::command]
pub fn http_cancel(state: State<'_, AppState>, request_id: String) -> bool {
    state.http_cancel(&request_id)
}

/// 真正发请求的实现：手动发送、批量运行、套件与 AI 执行共用，Cookie / 鉴权行为一致
pub(crate) async fn execute(
    state: &AppState,
    payload: SendRequestPayload,
) -> Result<HttpResult, String> {
    execute_with(state, payload, RecordOptions::manual(), None).await
}

/// 请求体的可重建形态：重试与重定向需要用同一份内容重新构造请求
enum PlannedBody {
    None,
    Bytes(Vec<u8>),
    Multipart(Vec<MultipartPart>),
}

struct MultipartPart {
    key: String,
    filename: Option<String>,
    bytes: Vec<u8>,
}

struct RequestPlan {
    method: reqwest::Method,
    url: reqwest::Url,
    headers: Vec<(String, String)>,
    ignored_headers: Vec<String>,
    body: PlannedBody,
    timeout: Duration,
    cookie_enabled: bool,
    has_cookie: bool,
}

/// `cookie_jar` 为空时读写本机 Cookie 库；套件 isolated 模式传入运行级内存罐
pub(crate) async fn execute_with(
    state: &AppState,
    payload: SendRequestPayload,
    record: RecordOptions,
    cookie_jar: Option<&cookie::CookieJar>,
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
    // h2c 需要 prior-knowledge，reqwest 不支持；明文目标显式选 HTTP/2 直接说明原因
    if payload.protocol.as_deref() == Some("http2") && url.starts_with("http://") {
        return Err(
            "E_PROTOCOL: 明文 http:// 目标不支持 HTTP/2（h2c 需 prior-knowledge，暂未开放）；\
请改用 https:// 或选择「自动」"
                .to_string(),
        );
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

    let mut has_content_type = false;
    let mut has_authorization = false;
    let mut has_cookie = false;
    let mut headers: Vec<(String, String)> = Vec::new();
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
        headers.push((key, render(&item.value, &vars)));
    }

    // 认证头：用户手动配置的 Authorization 优先
    if !has_authorization {
        match auth.kind.as_str() {
            "bearer" | "oauth2" if !auth.token.trim().is_empty() => {
                headers.push((
                    "Authorization".to_string(),
                    format!("Bearer {}", render(auth.token.trim(), &vars)),
                ));
            }
            "basic" => {
                let raw = format!(
                    "{}:{}",
                    render(&auth.username, &vars),
                    render(&auth.password, &vars)
                );
                headers.push((
                    "Authorization".to_string(),
                    format!("Basic {}", base64_encode(raw.as_bytes())),
                ));
            }
            "apikey" if auth.add_to != "query" && !auth.key.trim().is_empty() => {
                headers.push((render(auth.key.trim(), &vars), render(&auth.value, &vars)));
            }
            _ => {}
        }
    }

    let mut planned = PlannedBody::None;
    if !body_type.is_empty() && body_type != "none" {
        planned = build_body_plan(
            &payload,
            body_type,
            &vars,
            &mut headers,
            &mut has_content_type,
        )?;
    }

    // HTTP/2 会直接拒绝 connection-specific 头：https 且未强制 HTTP/1.1 时静默剔除
    let mut ignored_headers = Vec::new();
    if parsed.scheme() == "https" && payload.protocol.as_deref() != Some("http1") {
        headers.retain(|(key, _)| {
            let lower = key.to_ascii_lowercase();
            let blocked = matches!(
                lower.as_str(),
                "connection"
                    | "keep-alive"
                    | "proxy-connection"
                    | "transfer-encoding"
                    | "upgrade"
                    | "http2-settings"
                    | "te"
                    | "trailer"
            );
            if blocked {
                ignored_headers.push(key.clone());
            }
            !blocked
        });
    }

    let timeout_ms = payload.timeout_ms.max(500);
    let follow_redirects = payload.follow_redirects.unwrap_or(true);
    let plan = RequestPlan {
        method: method.clone(),
        url: parsed,
        headers,
        ignored_headers,
        body: planned,
        timeout: Duration::from_millis(timeout_ms),
        cookie_enabled: payload.cookie_enabled.unwrap_or(true),
        has_cookie,
    };

    // 显式指定协议或连接超时时用一次性客户端，否则复用共享连接池
    let overrides = payload.protocol.is_some() || payload.connect_timeout_ms.is_some();
    let (client, probe) = if overrides {
        state.client_for_request(
            payload.insecure,
            payload.protocol.as_deref() == Some("http1"),
            payload.connect_timeout_ms,
            false,
        )?
    } else {
        (state.client(payload.insecure), state.connector_probe())
    };

    // 取消登记
    let cancel_flag = payload
        .request_key
        .as_deref()
        .filter(|key| !key.trim().is_empty())
        .map(|key| (key.to_string(), state.http_register(key)));

    let retry = payload.retry.clone();
    let max_attempts = if retry.as_ref().map(|r| r.enabled).unwrap_or(false) {
        retry
            .as_ref()
            .map(|r| r.max_attempts.clamp(1, 4))
            .unwrap_or(1)
    } else {
        1
    };
    let allow_non_idempotent = retry
        .as_ref()
        .map(|r| r.allow_non_idempotent)
        .unwrap_or(false);
    let idempotent = matches!(
        method,
        reqwest::Method::GET | reqwest::Method::HEAD | reqwest::Method::OPTIONS
    ) || allow_non_idempotent;

    let started = Instant::now();
    let connects_before = probe.snapshot();
    let mut attempts: Vec<RetryAttempt> = Vec::new();
    let mut last_error: Option<String> = None;
    let mut outcome: Option<ChainOutcome> = None;
    let mut attempt_index = 0u32;

    while attempt_index < max_attempts {
        attempt_index += 1;
        match send_chain(
            &client,
            &plan,
            follow_redirects,
            state,
            cookie_jar,
            cancel_flag.as_ref().map(|(_, flag)| flag.as_ref()),
        )
        .await
        {
            Ok(chain) => {
                let retryable = is_retryable_status(chain.status);
                if retryable && attempt_index < max_attempts && idempotent {
                    let delay = backoff_ms(attempt_index, chain.retry_after.as_deref());
                    attempts.push(RetryAttempt {
                        attempt: attempt_index,
                        status: chain.status,
                        delay_ms: delay,
                        error: None,
                    });
                    tokio::time::sleep(Duration::from_millis(delay)).await;
                    continue;
                }
                outcome = Some(chain);
                break;
            }
            Err(error) => {
                if error.cancelled {
                    let result = cancelled_result(started.elapsed());
                    finish_record(state, &payload, &url_for_history, &plan, &result, &record);
                    release_cancel(state, &cancel_flag);
                    return Ok(result);
                }
                last_error = Some(error.message.clone());
                if attempt_index < max_attempts && idempotent {
                    let delay = backoff_ms(attempt_index, None);
                    attempts.push(RetryAttempt {
                        attempt: attempt_index,
                        status: 0,
                        delay_ms: delay,
                        error: Some(error.message),
                    });
                    tokio::time::sleep(Duration::from_millis(delay)).await;
                    continue;
                }
                break;
            }
        }
    }

    release_cancel(state, &cancel_flag);
    let total = started.elapsed();
    let connect_us = {
        // 只有建连次数增加，才说明本次真的新建了连接
        if probe.snapshot() > connects_before {
            probe.last_connect_us() as f64 / 1000.0
        } else {
            0.0
        }
    };
    let reused = probe.snapshot() == connects_before;

    let mut result = match outcome {
        Some(chain) => chain.into_result(total, connect_us, reused, state),
        None => HttpResult {
            status: 0,
            status_text: String::new(),
            duration_ms: total.as_millis() as u64,
            size_bytes: 0,
            headers: HashMap::new(),
            body: String::new(),
            truncated: false,
            error: last_error,
            body_kind: "text".to_string(),
            body_file: None,
            body_bytes: 0,
            raw_bytes: 0,
            preview_base64: None,
            timings: None,
            http_version: String::new(),
            redirects: Vec::new(),
            retries: attempts.clone(),
            ignored_headers: plan.ignored_headers.clone(),
            cancelled: false,
        },
    };
    if result.retries.is_empty() {
        result.retries = attempts;
    }
    if result.ignored_headers.is_empty() {
        result.ignored_headers = plan.ignored_headers.clone();
    }

    // Cookie 回写（最终响应已在 send_chain 内处理，这里处理错误分支的兜底）
    finish_record(state, &payload, &url_for_history, &plan, &result, &record);
    Ok(result)
}

fn release_cancel(state: &AppState, canceled: &Option<(String, Arc<AtomicBool>)>) {
    if let Some((key, _)) = canceled {
        state.http_release(key);
    }
}

fn finish_record(
    state: &AppState,
    payload: &SendRequestPayload,
    url: &str,
    plan: &RequestPlan,
    result: &HttpResult,
    record: &RecordOptions,
) {
    if record.history {
        record_history(state, payload, url, result);
    }
    if record.run_snapshot {
        let body_text = match &plan.body {
            PlannedBody::None => String::new(),
            PlannedBody::Bytes(bytes) => String::from_utf8_lossy(bytes).into_owned(),
            PlannedBody::Multipart(parts) => parts
                .iter()
                .map(|part| match &part.filename {
                    Some(name) => format!("{}={name}（{} 字节）", part.key, part.bytes.len()),
                    None => format!("{}={}", part.key, String::from_utf8_lossy(&part.bytes)),
                })
                .collect::<Vec<_>>()
                .join("&"),
        };
        record_run_snapshot(
            state,
            payload,
            url,
            &plan.headers,
            &body_text,
            result,
            &record.origin,
        );
    }
}

fn cancelled_result(elapsed: Duration) -> HttpResult {
    HttpResult {
        status: 0,
        status_text: String::new(),
        duration_ms: elapsed.as_millis() as u64,
        size_bytes: 0,
        headers: HashMap::new(),
        body: String::new(),
        truncated: false,
        error: Some("已取消".to_string()),
        body_kind: "text".to_string(),
        body_file: None,
        body_bytes: 0,
        raw_bytes: 0,
        preview_base64: None,
        timings: None,
        http_version: String::new(),
        redirects: Vec::new(),
        retries: Vec::new(),
        ignored_headers: Vec::new(),
        cancelled: true,
    }
}

/// 组装请求体：文件在这里一次性读入，重试/重定向时可以直接重建
fn build_body_plan(
    payload: &SendRequestPayload,
    body_type: &str,
    vars: &HashMap<String, String>,
    headers: &mut Vec<(String, String)>,
    has_content_type: &mut bool,
) -> Result<PlannedBody, String> {
    match body_type {
        "multipart" => {
            let fields: Vec<FormField> =
                serde_json::from_str(payload.body.as_deref().unwrap_or("[]")).unwrap_or_default();
            let mut parts = Vec::new();
            for field in fields.iter().filter(|f| f.enabled) {
                let key = render(field.key.trim(), vars);
                if key.is_empty() {
                    continue;
                }
                if field.kind == "file" {
                    let path = render(field.value.trim(), vars);
                    if path.is_empty() {
                        continue;
                    }
                    let bytes = std::fs::read(&path)
                        .map_err(|e| format!("读取上传文件失败（{path}）：{e}"))?;
                    let filename = Path::new(&path)
                        .file_name()
                        .map(|name| name.to_string_lossy().to_string())
                        .unwrap_or_else(|| "file".to_string());
                    parts.push(MultipartPart {
                        key,
                        filename: Some(filename),
                        bytes,
                    });
                } else {
                    parts.push(MultipartPart {
                        key,
                        filename: None,
                        bytes: render(&field.value, vars).into_bytes(),
                    });
                }
            }
            Ok(PlannedBody::Multipart(parts))
        }
        "binary" => {
            let path = render(payload.body.as_deref().unwrap_or("").trim(), vars);
            if path.is_empty() {
                return Err("请选择要上传的二进制文件".to_string());
            }
            let bytes =
                std::fs::read(&path).map_err(|e| format!("读取上传文件失败（{path}）：{e}"))?;
            if !*has_content_type {
                headers.push((
                    "Content-Type".to_string(),
                    "application/octet-stream".to_string(),
                ));
                *has_content_type = true;
            }
            Ok(PlannedBody::Bytes(bytes))
        }
        _ => {
            if !*has_content_type {
                let content_type = match body_type {
                    "json" => "application/json",
                    "graphql" => "application/json",
                    "form" => "application/x-www-form-urlencoded",
                    _ => "text/plain; charset=utf-8",
                };
                headers.push(("Content-Type".to_string(), content_type.to_string()));
                *has_content_type = true;
            }
            let raw = payload.body.as_deref().unwrap_or("");
            // JSON 允许写注释（Postman 的 raw/JSON 编辑器同样如此），发送前去掉
            let source = if body_type == "json" {
                strip_json_comments(raw)
            } else {
                raw.to_string()
            };
            Ok(PlannedBody::Bytes(render(&source, vars).into_bytes()))
        }
    }
}

fn apply_body(
    builder: reqwest::RequestBuilder,
    plan: &RequestPlan,
    method: &reqwest::Method,
) -> reqwest::RequestBuilder {
    if method == reqwest::Method::GET || method == reqwest::Method::HEAD {
        return builder;
    }
    match &plan.body {
        PlannedBody::None => builder,
        PlannedBody::Bytes(bytes) => builder.body(bytes.clone()),
        PlannedBody::Multipart(parts) => {
            let mut form = reqwest::multipart::Form::new();
            for part in parts {
                let piece = reqwest::multipart::Part::bytes(part.bytes.clone());
                let piece = match &part.filename {
                    Some(name) => piece.file_name(name.clone()),
                    None => piece,
                };
                form = form.part(part.key.clone(), piece);
            }
            builder.multipart(form)
        }
    }
}

struct ChainOutcome {
    status: u16,
    status_text: String,
    headers: HashMap<String, String>,
    hops: Vec<RedirectHop>,
    body: BodyOutcome,
    ttfb: Duration,
    download: Duration,
    http_version: String,
    retry_after: Option<String>,
}

impl ChainOutcome {
    fn into_result(
        self,
        total: Duration,
        connect_ms: f64,
        reused: bool,
        state: &AppState,
    ) -> HttpResult {
        let proxy = state.settings().proxy_enabled;
        let timings = RequestTimings {
            dns_ms: None,
            connect_ms,
            ttfb_ms: self.ttfb.as_secs_f64() * 1000.0,
            download_ms: self.download.as_secs_f64() * 1000.0,
            total_ms: total.as_secs_f64() * 1000.0,
            reused_connection: reused,
            proxy,
        };
        HttpResult {
            status: self.status,
            status_text: self.status_text,
            duration_ms: total.as_millis() as u64,
            size_bytes: self.body.bytes as usize,
            headers: self.headers,
            body: self.body.text,
            truncated: self.body.truncated,
            error: self.body.error,
            body_kind: self.body.kind,
            body_file: self.body.file,
            body_bytes: self.body.bytes,
            raw_bytes: self.body.raw_bytes,
            preview_base64: self.body.preview_base64,
            timings: Some(timings),
            http_version: self.http_version,
            redirects: self.hops,
            retries: Vec::new(),
            ignored_headers: Vec::new(),
            cancelled: false,
        }
    }
}

struct SendError {
    cancelled: bool,
    message: String,
}

impl SendError {
    fn failed(message: impl Into<String>) -> Self {
        Self {
            cancelled: false,
            message: message.into(),
        }
    }
}

/// 一次尝试：可能包含多跳重定向，最终返回最后一条响应
async fn send_chain(
    client: &reqwest::Client,
    plan: &RequestPlan,
    follow: bool,
    state: &AppState,
    cookie_jar: Option<&cookie::CookieJar>,
    cancel: Option<&AtomicBool>,
) -> Result<ChainOutcome, SendError> {
    let mut url = plan.url.clone();
    let mut method = plan.method.clone();
    let mut hops: Vec<RedirectHop> = Vec::new();
    let mut ttfb = Duration::ZERO;

    for hop in 0..=MAX_REDIRECTS {
        let mut builder = client
            .request(method.clone(), url.clone())
            .timeout(plan.timeout);

        for (key, value) in &plan.headers {
            builder = builder.header(key.as_str(), value.as_str());
        }

        // 每跳重新取一次 Cookie：上一跳的 Set-Cookie 已经写进罐里
        if plan.cookie_enabled && !plan.has_cookie {
            let cookies = match cookie_jar {
                Some(jar) => jar.cookies_for_url(&url),
                None => match state.pool.get() {
                    Ok(conn) => cookie::cookies_for_url(&conn, &url),
                    Err(_) => Vec::new(),
                },
            };
            if !cookies.is_empty() {
                builder = builder.header(reqwest::header::COOKIE, cookies.join("; "));
            }
        }

        builder = apply_body(builder, plan, &method);

        let started = Instant::now();
        let response = send_with_cancel(builder, cancel).await?;
        let hop_ms = started.elapsed().as_secs_f64() * 1000.0;
        if hop == 0 {
            ttfb = started.elapsed();
        }

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

        // Set-Cookie 落罐：下一跳要用
        if plan.cookie_enabled && !set_cookies.is_empty() {
            match cookie_jar {
                Some(jar) => jar.store_response_cookies(&url, &set_cookies),
                None => {
                    if let Ok(conn) = state.pool.get() {
                        cookie::store_response_cookies(&conn, &url, &set_cookies);
                    }
                }
            }
        }

        let location = headers.get("location").cloned();
        if follow && is_redirect(status) && location.is_some() && hop < MAX_REDIRECTS {
            let target = location.unwrap_or_default();
            let next = url
                .join(target.trim())
                .map_err(|e| SendError::failed(format!("重定向地址无法解析（{target}）：{e}")))?;
            hops.push(RedirectHop {
                status: status.as_u16(),
                location: target,
                host: next.host_str().unwrap_or_default().to_string(),
                duration_ms: hop_ms,
            });
            // 303 或 301/302 上的非 GET 请求按浏览器行为改为 GET 且不再带体
            if status == reqwest::StatusCode::SEE_OTHER
                || (matches!(status.as_u16(), 301 | 302)
                    && method != reqwest::Method::GET
                    && method != reqwest::Method::HEAD)
            {
                method = reqwest::Method::GET;
            }
            drain_response(response).await;
            url = next;
            continue;
        }

        let http_version = format!("{:?}", response.version());
        let retry_after = headers.get("retry-after").cloned();
        let mut response = response;
        let download_start = Instant::now();
        let body = read_body(&mut response, &headers, &state.data_dir).await;
        let download = download_start.elapsed();
        return Ok(ChainOutcome {
            status: status.as_u16(),
            status_text,
            headers,
            hops,
            body,
            ttfb,
            download,
            http_version,
            retry_after,
        });
    }

    Err(SendError::failed(format!(
        "重定向次数超过 {MAX_REDIRECTS} 次，已停止跟随"
    )))
}

/// 发送并响应取消：取消位每 50ms 轮询一次，实测 1s 内可中止
async fn send_with_cancel(
    builder: reqwest::RequestBuilder,
    cancel: Option<&AtomicBool>,
) -> Result<reqwest::Response, SendError> {
    let Some(flag) = cancel else {
        return builder
            .send()
            .await
            .map_err(|e| SendError::failed(e.to_string()));
    };
    if flag.load(Ordering::SeqCst) {
        return Err(SendError {
            cancelled: true,
            message: "已取消".to_string(),
        });
    }
    let (tx, mut rx) = tokio::sync::oneshot::channel();
    let future = builder.send();
    tokio::spawn(async move {
        let _ = tx.send(future.await);
    });
    loop {
        tokio::time::sleep(Duration::from_millis(50)).await;
        if flag.load(Ordering::SeqCst) {
            return Err(SendError {
                cancelled: true,
                message: "已取消".to_string(),
            });
        }
        match rx.try_recv() {
            Ok(Ok(response)) => return Ok(response),
            Ok(Err(error)) => return Err(SendError::failed(error.to_string())),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty) => continue,
            Err(tokio::sync::oneshot::error::TryRecvError::Closed) => {
                return Err(SendError::failed("请求通道已关闭"))
            }
        }
    }
}

/// 排空中间跳响应体，让连接可以复用；超过上限就放弃（连接会被关闭）
async fn drain_response(mut response: reqwest::Response) {
    let mut drained = 0usize;
    loop {
        match response.chunk().await {
            Ok(Some(chunk)) => {
                drained += chunk.len();
                if drained > DRAIN_LIMIT {
                    return;
                }
            }
            _ => return,
        }
    }
}

fn is_redirect(status: reqwest::StatusCode) -> bool {
    matches!(status.as_u16(), 301 | 302 | 303 | 307 | 308)
}

fn is_retryable_status(status: u16) -> bool {
    matches!(status, 408 | 429 | 500 | 502 | 503 | 504)
}

/// 指数退避 200ms / 400ms / 800ms，尊重 Retry-After（秒）
fn backoff_ms(attempt: u32, retry_after: Option<&str>) -> u64 {
    if let Some(text) = retry_after {
        if let Ok(seconds) = text.trim().parse::<u64>() {
            return (seconds * 1000).min(10_000);
        }
    }
    RETRY_BASE_MS * 2u64.pow(attempt.saturating_sub(1).min(4))
}

/* ---------- 正文读取 ---------- */

struct BodyOutcome {
    text: String,
    kind: String,
    file: Option<String>,
    bytes: u64,
    raw_bytes: u64,
    truncated: bool,
    preview_base64: Option<String>,
    error: Option<String>,
}

impl BodyOutcome {
    fn empty() -> Self {
        Self {
            text: String::new(),
            kind: "text".to_string(),
            file: None,
            bytes: 0,
            raw_bytes: 0,
            truncated: false,
            preview_base64: None,
            error: None,
        }
    }

    fn failure(message: String) -> Self {
        let mut outcome = Self::empty();
        outcome.error = Some(message);
        outcome
    }
}

#[derive(Debug, PartialEq)]
enum BodyClass {
    Text,
    Binary(&'static str),
}

/// 先解压（reqwest 已做）→ 看 Content-Type → 嗅探魔数 → 校验 UTF-8
fn classify(content_type: &str, disposition: &str, head: &[u8]) -> BodyClass {
    let lower = content_type.to_ascii_lowercase();
    let base = lower.split(';').next().unwrap_or("").trim().to_string();
    if disposition.to_ascii_lowercase().contains("attachment") {
        return BodyClass::Binary(extension_for(&base, head));
    }
    let textual = base.starts_with("text/")
        || base.contains("json")
        || base.contains("xml")
        || base == "application/javascript"
        || base == "application/x-www-form-urlencoded"
        || base == "image/svg+xml";

    if textual {
        // 声明是文本但字节不是 UTF-8：按二进制处理，绝不输出乱码
        return if looks_utf8(head) {
            BodyClass::Text
        } else {
            BodyClass::Binary(extension_for(&base, head))
        };
    }

    if base.is_empty() || base == "application/octet-stream" {
        if sniff_magic(head).is_some() {
            return BodyClass::Binary(extension_for(&base, head));
        }
        return if looks_utf8(head) {
            BodyClass::Text
        } else {
            BodyClass::Binary("bin")
        };
    }

    BodyClass::Binary(extension_for(&base, head))
}

fn looks_utf8(bytes: &[u8]) -> bool {
    match std::str::from_utf8(bytes) {
        Ok(_) => true,
        // 结尾被截断的多字节字符不算失败
        Err(error) => error.error_len().is_none(),
    }
}

/// 魔数嗅探：只认最常见的几种，用于 octet-stream 与无 Content-Type 的场景
fn sniff_magic(head: &[u8]) -> Option<&'static str> {
    if head.starts_with(b"PK\x03\x04") {
        return Some("zip");
    }
    if head.starts_with(b"%PDF") {
        return Some("pdf");
    }
    if head.starts_with(b"\x89PNG") {
        return Some("png");
    }
    if head.starts_with(b"\xff\xd8\xff") {
        return Some("jpg");
    }
    if head.starts_with(b"GIF8") {
        return Some("gif");
    }
    if head.starts_with(b"\x1f\x8b") {
        return Some("gz");
    }
    None
}

fn extension_for(content_type: &str, head: &[u8]) -> &'static str {
    let base = content_type.split(';').next().unwrap_or("").trim();
    let mapped = match base {
        "image/png" => Some("png"),
        "image/jpeg" | "image/jpg" => Some("jpg"),
        "image/gif" => Some("gif"),
        "image/webp" => Some("webp"),
        "image/svg+xml" => Some("svg"),
        "application/pdf" => Some("pdf"),
        "application/zip" | "application/zip-compressed" => Some("zip"),
        "application/gzip" | "application/x-gzip" => Some("gz"),
        "application/msword" => Some("doc"),
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document" => Some("docx"),
        "application/vnd.ms-excel" => Some("xls"),
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" => Some("xlsx"),
        "application/octet-stream" => None,
        _ => None,
    };
    mapped.or_else(|| sniff_magic(head)).unwrap_or("bin")
}

fn is_image_or_pdf(content_type: &str) -> bool {
    let base = content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    base.starts_with("image/") || base == "application/pdf"
}

/// 创建响应文件，返回（相对数据目录的路径, 文件句柄）
fn create_response_file(data_dir: &Path, ext: &str) -> Result<(String, std::fs::File), String> {
    let day = chrono::Local::now().format("%Y%m%d").to_string();
    let dir: PathBuf = data_dir.join("responses").join(&day);
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建响应目录失败：{e}"))?;
    let name = format!("{}.{}", uuid::Uuid::new_v4(), ext);
    let path = dir.join(&name);
    let file = std::fs::File::create(&path).map_err(|e| format!("创建响应文件失败：{e}"))?;
    Ok((format!("responses/{day}/{name}"), file))
}

async fn read_body(
    response: &mut reqwest::Response,
    headers: &HashMap<String, String>,
    data_dir: &Path,
) -> BodyOutcome {
    let content_type = headers
        .get("content-type")
        .cloned()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let disposition = headers
        .get("content-disposition")
        .cloned()
        .unwrap_or_default();
    let raw_bytes = headers
        .get("content-length")
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0);

    let mut head: Vec<u8> = Vec::new();
    let mut ended = false;
    while head.len() < SNIFF_BYTES {
        match response.chunk().await {
            Ok(Some(chunk)) => head.extend_from_slice(&chunk),
            Ok(None) => {
                ended = true;
                break;
            }
            Err(error) => return BodyOutcome::failure(error.to_string()),
        }
    }

    match classify(&content_type, &disposition, &head) {
        BodyClass::Binary(ext) => {
            write_binary(
                response,
                head,
                ended,
                data_dir,
                ext,
                &content_type,
                raw_bytes,
            )
            .await
        }
        BodyClass::Text => {
            collect_text(response, head, ended, data_dir, &content_type, raw_bytes).await
        }
    }
}

async fn write_binary(
    response: &mut reqwest::Response,
    head: Vec<u8>,
    ended: bool,
    data_dir: &Path,
    ext: &str,
    content_type: &str,
    raw_bytes: u64,
) -> BodyOutcome {
    let (rel, mut file) = match create_response_file(data_dir, ext) {
        Ok(pair) => pair,
        Err(error) => {
            log::warn!("响应落盘失败：{error}");
            return BodyOutcome::failure(error);
        }
    };
    let mut bytes = head.len() as u64;
    let mut preview: Vec<u8> = head[..head.len().min(PREVIEW_BYTES)].to_vec();
    if let Err(error) = file.write_all(&head) {
        return BodyOutcome::failure(format!("写入响应文件失败：{error}"));
    }
    if !ended {
        loop {
            match response.chunk().await {
                Ok(Some(chunk)) => {
                    bytes += chunk.len() as u64;
                    if preview.len() < PREVIEW_BYTES {
                        let take = (PREVIEW_BYTES - preview.len()).min(chunk.len());
                        preview.extend_from_slice(&chunk[..take]);
                    }
                    if let Err(error) = file.write_all(&chunk) {
                        return BodyOutcome::failure(format!("写入响应文件失败：{error}"));
                    }
                }
                Ok(None) => break,
                Err(error) => {
                    let mut outcome = BodyOutcome::empty();
                    outcome.kind = "binary".to_string();
                    outcome.file = Some(rel);
                    outcome.bytes = bytes;
                    outcome.raw_bytes = raw_bytes;
                    outcome.error = Some(format!("下载中断：{error}"));
                    return outcome;
                }
            }
        }
    }
    let preview_base64 = if is_image_or_pdf(content_type) || ext == "pdf" {
        Some(B64.encode(&preview))
    } else {
        None
    };
    BodyOutcome {
        text: String::new(),
        kind: "binary".to_string(),
        file: Some(rel),
        bytes,
        raw_bytes,
        truncated: false,
        preview_base64,
        error: None,
    }
}

async fn collect_text(
    response: &mut reqwest::Response,
    head: Vec<u8>,
    ended: bool,
    data_dir: &Path,
    content_type: &str,
    raw_bytes: u64,
) -> BodyOutcome {
    let mut buffer = head;
    let mut bytes = buffer.len() as u64;
    let mut truncated = false;
    let mut read_error: Option<String> = None;
    let mut file: Option<(String, std::fs::File)> = None;

    loop {
        if ended {
            break;
        }
        match response.chunk().await {
            Ok(Some(chunk)) => {
                bytes += chunk.len() as u64;
                // 超过阈值：开始流式落盘（含已读入内存的部分），保证「保存完整响应」可用
                if file.is_none() && bytes > SPILL_THRESHOLD {
                    match create_response_file(data_dir, extension_for(content_type, &buffer)) {
                        Ok((rel, mut handle)) => {
                            if let Err(error) = handle.write_all(&buffer) {
                                log::warn!("写入响应文件失败：{error}");
                            }
                            file = Some((rel, handle));
                        }
                        Err(error) => log::warn!("响应落盘失败：{error}"),
                    }
                }
                if let Some((_, handle)) = file.as_mut() {
                    if let Err(error) = handle.write_all(&chunk) {
                        log::warn!("写入响应文件失败：{error}");
                    }
                }
                if buffer.len() < MAX_BODY {
                    let remain = MAX_BODY - buffer.len();
                    if chunk.len() <= remain {
                        buffer.extend_from_slice(&chunk);
                    } else {
                        buffer.extend_from_slice(&chunk[..remain]);
                        truncated = true;
                    }
                } else {
                    truncated = true;
                }
            }
            Ok(None) => break,
            Err(error) => {
                read_error = Some(error.to_string());
                break;
            }
        }
    }

    let text = String::from_utf8_lossy(&buffer).into_owned();
    let trimmed = text.trim_start();
    let kind =
        if content_type.contains("json") || trimmed.starts_with('{') || trimmed.starts_with('[') {
            "json"
        } else {
            "text"
        };

    BodyOutcome {
        text,
        kind: kind.to_string(),
        file: file.map(|(rel, _)| rel),
        bytes,
        raw_bytes,
        truncated,
        preview_base64: None,
        error: read_error,
    }
}

/* ---------- 基础工具 ---------- */

/// 写入单接口的运行快照，失败不影响本次请求结果
fn record_run_snapshot(
    state: &AppState,
    payload: &SendRequestPayload,
    url: &str,
    headers: &[(String, String)],
    body: &str,
    result: &HttpResult,
    origin: &str,
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
        origin,
    );
}

/// 标准 Base64 编码（Basic 认证使用）
pub(crate) fn base64_encode(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
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
    Ok(load_vars_with_secrets(conn, env_id)?.0)
}

/// 同时返回变量与需要脱敏的密钥名（`is_secret = 1`）
///
/// 敏感值在库里是密文，这里是**唯一**的解密出口：前端永远拿不到明文。
pub(crate) fn load_vars_with_secrets(
    conn: &Connection,
    env_id: i64,
) -> Result<(HashMap<String, String>, std::collections::HashSet<String>), String> {
    let mut stmt = conn
        .prepare(
            "SELECT \"key\", value, is_secret FROM env_variables
             WHERE env_id = ?1 ORDER BY sort_order ASC, id ASC",
        )
        .map_err(err)?;
    let rows = stmt
        .query_map(params![env_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                row.get::<_, i64>(2).unwrap_or(0),
            ))
        })
        .map_err(err)?;

    let mut vars = HashMap::new();
    let mut secrets = std::collections::HashSet::new();
    for item in rows {
        let (key, value, is_secret) = item.map_err(err)?;
        if key.is_empty() {
            continue;
        }
        let value = if is_secret != 0 {
            secrets.insert(key.clone());
            let plain = secret::decrypt(AAD_ENV, &value);
            // 登记脱敏清单：日志 / 诊断包落盘前统一替换（4.0.5）
            crate::logging::register_secret(&plain);
            plain
        } else {
            value
        };
        vars.insert(key, value);
    }
    Ok((vars, secrets))
}

fn record_history(state: &AppState, payload: &SendRequestPayload, url: &str, result: &HttpResult) {
    let Ok(conn) = state.pool.get() else {
        return;
    };

    let name = payload
        .name
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| url.to_string());

    let timings_json = result
        .timings
        .as_ref()
        .and_then(|timings| serde_json::to_string(timings).ok());

    let _ = conn.execute(
        "INSERT INTO request_history
            (request_id, request_name, method, url, status_code, duration_ms, created_at, origin,
             timings_json, body_kind, body_file)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            payload.request_id,
            name,
            payload.method,
            url,
            if result.status > 0 {
                Some(result.status as i64)
            } else {
                None
            },
            result.duration_ms as i64,
            now_ms(),
            RecordOptions::from_payload(payload).origin,
            timings_json,
            result.body_kind,
            result.body_file,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_common_content_types() {
        assert_eq!(
            classify("application/json; charset=utf-8", "", b"{}"),
            BodyClass::Text
        );
        assert_eq!(
            classify("application/octet-stream", "", b"PK\x03\x04rest"),
            BodyClass::Binary("zip")
        );
        assert_eq!(
            classify("image/png", "", b"\x89PNG\r\n"),
            BodyClass::Binary("png")
        );
        // 声明 text/plain 但字节不是 UTF-8：按二进制处理，避免输出乱码
        assert_eq!(
            classify("text/plain", "", &[0xff, 0xfe, 0x00]),
            BodyClass::Binary("bin")
        );
        // 附件一律二进制，即使 Content-Type 是 text
        assert_eq!(
            classify("text/plain", "attachment; filename=a.csv", b"a,b"),
            BodyClass::Binary("bin")
        );
    }

    #[test]
    fn backoff_respects_retry_after_and_caps() {
        assert_eq!(backoff_ms(1, None), 200);
        assert_eq!(backoff_ms(3, None), 800);
        assert_eq!(backoff_ms(1, Some("2")), 2000);
        assert_eq!(backoff_ms(1, Some("120")), 10_000);
    }

    #[test]
    fn redirect_and_retry_status_ranges() {
        for status in [301, 302, 303, 307, 308] {
            assert!(is_redirect(reqwest::StatusCode::from_u16(status).unwrap()));
        }
        assert!(!is_redirect(reqwest::StatusCode::OK));
        for status in [408, 429, 500, 502, 503, 504] {
            assert!(is_retryable_status(status));
        }
        assert!(!is_retryable_status(200));
    }

    #[test]
    fn base64_matches_known_vectors() {
        assert_eq!(base64_encode(b"user:pass"), "dXNlcjpwYXNz");
        assert_eq!(base64_encode(b"a"), "YQ==");
        assert_eq!(base64_encode(b"ab"), "YWI=");
    }
}
