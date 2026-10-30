use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rusqlite::params;
use serde_json::json;
use tauri::{AppHandle, Emitter, State};
use tokio::sync::mpsc;
use tokio::task::JoinSet;

use crate::commands::http::base64_encode;
use crate::models::{
    ErrorTypeCount, HistogramBucket, ReportError, ReportRequestRecord, ReportSample, RequestAuth,
    StageSummary, StatusCount, StressConfig, StressDone, StressProgress, StressReport, StressStage,
};
use crate::state::{AppState, DbPool};
use crate::util::{err, now_ms, render, strip_json_comments};

/// 延迟直方图上限（毫秒），超出的请求计入最后一个桶
const MAX_RT_MS: usize = 60_000;
/// 首字节时间直方图上限
const MAX_TTFB_MS: usize = 10_000;
/// 报告中最多保留的错误明细条数
const MAX_ERRORS: usize = 500;
/// 报告中最多保留的逐请求明细条数
const MAX_REQUEST_RECORDS: usize = 50_000;
/// 请求明细中错误信息的最大长度
const MAX_RECORD_ERROR: usize = 500;
/// 请求明细中请求体的最大长度
const MAX_RECORD_BODY: usize = 1000;
/// 请求明细中响应体的最大字符数
const MAX_RECORD_RESPONSE: usize = 2000;
const SNIPPET_LIMIT: usize = 256;
/// APDEX 的 T 值（毫秒）
const APDEX_T: f64 = 500.0;

struct Metrics {
    total: AtomicU64,
    success: AtomicU64,
    errors: AtomicU64,
    sum_ms: AtomicU64,
    sum_ttfb: AtomicU64,
    bytes: AtomicU64,
    min_ms: AtomicU64,
    max_ms: AtomicU64,
    concurrent: AtomicU32,
    buckets: Vec<AtomicU64>,
    ttfb_buckets: Vec<AtomicU64>,
    codes: Mutex<HashMap<u16, u64>>,
    error_kinds: Mutex<HashMap<String, i64>>,
}

impl Metrics {
    fn new() -> Self {
        Self {
            total: AtomicU64::new(0),
            success: AtomicU64::new(0),
            errors: AtomicU64::new(0),
            sum_ms: AtomicU64::new(0),
            sum_ttfb: AtomicU64::new(0),
            bytes: AtomicU64::new(0),
            min_ms: AtomicU64::new(u64::MAX),
            max_ms: AtomicU64::new(0),
            concurrent: AtomicU32::new(0),
            buckets: (0..=MAX_RT_MS).map(|_| AtomicU64::new(0)).collect(),
            ttfb_buckets: (0..=MAX_TTFB_MS).map(|_| AtomicU64::new(0)).collect(),
            codes: Mutex::new(HashMap::new()),
            error_kinds: Mutex::new(HashMap::new()),
        }
    }

    fn record(&self, latency_ms: u64, ttfb_ms: u64, status: u16, bytes: u64, ok: bool) {
        self.total.fetch_add(1, Ordering::Relaxed);
        if ok {
            self.success.fetch_add(1, Ordering::Relaxed);
        } else {
            self.errors.fetch_add(1, Ordering::Relaxed);
        }
        self.sum_ms.fetch_add(latency_ms, Ordering::Relaxed);
        self.sum_ttfb.fetch_add(ttfb_ms, Ordering::Relaxed);
        self.bytes.fetch_add(bytes, Ordering::Relaxed);
        self.min_ms.fetch_min(latency_ms, Ordering::Relaxed);
        self.max_ms.fetch_max(latency_ms, Ordering::Relaxed);
        let index = latency_ms.min(MAX_RT_MS as u64) as usize;
        self.buckets[index].fetch_add(1, Ordering::Relaxed);
        let ttfb_index = ttfb_ms.min(MAX_TTFB_MS as u64) as usize;
        self.ttfb_buckets[ttfb_index].fetch_add(1, Ordering::Relaxed);
        if status > 0 {
            if let Ok(mut map) = self.codes.lock() {
                *map.entry(status).or_insert(0) += 1;
            }
        }
    }

    fn count(&self) -> u64 {
        self.total.load(Ordering::Relaxed)
    }

    /// 按错误类型累计，不受明细条数上限影响
    fn bump_error_type(&self, kind: &str) {
        if let Ok(mut map) = self.error_kinds.lock() {
            *map.entry(kind.to_string()).or_insert(0) += 1;
        }
    }

    fn error_types(&self) -> Vec<ErrorTypeCount> {
        let mut items: Vec<ErrorTypeCount> = self
            .error_kinds
            .lock()
            .map(|map| {
                map.iter()
                    .map(|(kind, count)| ErrorTypeCount {
                        error_type: kind.clone(),
                        count: *count,
                    })
                    .collect()
            })
            .unwrap_or_default();
        items.sort_by(|a, b| b.count.cmp(&a.count).then(a.error_type.cmp(&b.error_type)));
        items
    }

    fn avg(&self) -> f64 {
        let total = self.count();
        if total == 0 {
            0.0
        } else {
            self.sum_ms.load(Ordering::Relaxed) as f64 / total as f64
        }
    }

    fn ttfb_avg(&self) -> f64 {
        let total = self.count();
        if total == 0 {
            0.0
        } else {
            self.sum_ttfb.load(Ordering::Relaxed) as f64 / total as f64
        }
    }

    fn min(&self) -> f64 {
        if self.count() == 0 {
            0.0
        } else {
            self.min_ms.load(Ordering::Relaxed) as f64
        }
    }

    fn max(&self) -> f64 {
        self.max_ms.load(Ordering::Relaxed) as f64
    }

    fn percentile_in(buckets: &[AtomicU64], total: u64, p: f64) -> f64 {
        if total == 0 {
            return 0.0;
        }
        let target = ((total as f64) * p).ceil() as u64;
        let mut acc = 0u64;
        for (index, bucket) in buckets.iter().enumerate() {
            acc += bucket.load(Ordering::Relaxed);
            if acc >= target {
                return index as f64;
            }
        }
        buckets.len() as f64 - 1.0
    }

    fn percentile(&self, p: f64) -> f64 {
        Self::percentile_in(&self.buckets, self.count(), p)
    }

    fn ttfb_percentile(&self, p: f64) -> f64 {
        Self::percentile_in(&self.ttfb_buckets, self.count(), p)
    }

    /// 由直方图计算总体标准差（按毫秒桶中值）
    fn stddev(&self) -> f64 {
        let total = self.count();
        if total < 2 {
            return 0.0;
        }
        let mean = self.avg();
        let mut acc = 0.0;
        for (index, bucket) in self.buckets.iter().enumerate() {
            let count = bucket.load(Ordering::Relaxed) as f64;
            if count > 0.0 {
                let diff = index as f64 - mean;
                acc += count * diff * diff;
            }
        }
        (acc / (total - 1) as f64).sqrt()
    }

    /// APDEX：满意 (<T)、可容忍 (<4T)、失望 三档打分
    fn apdex(&self) -> f64 {
        let total = self.count();
        if total == 0 {
            return 0.0;
        }
        let mut satisfied = 0u64;
        let mut tolerating = 0u64;
        for (index, bucket) in self.buckets.iter().enumerate() {
            let value = index as f64;
            let count = bucket.load(Ordering::Relaxed);
            if value <= APDEX_T {
                satisfied += count;
            } else if value <= APDEX_T * 4.0 {
                tolerating += count;
            }
        }
        (satisfied as f64 + tolerating as f64 / 2.0) / total as f64
    }

    fn status_codes(&self) -> Vec<StatusCount> {
        let mut items: Vec<StatusCount> = self
            .codes
            .lock()
            .map(|map| {
                map.iter()
                    .map(|(code, count)| StatusCount {
                        code: *code as i64,
                        count: *count as i64,
                    })
                    .collect()
            })
            .unwrap_or_default();
        items.sort_by(|a, b| b.count.cmp(&a.count).then(a.code.cmp(&b.code)));
        items
    }

    /// 把逐毫秒直方图归并为便于阅读的区间
    fn histogram(&self) -> Vec<HistogramBucket> {
        const RANGES: [(f64, f64, &str); 17] = [
            (0.0, 10.0, "≤10ms"),
            (10.0, 25.0, "10-25ms"),
            (25.0, 50.0, "25-50ms"),
            (50.0, 100.0, "50-100ms"),
            (100.0, 200.0, "100-200ms"),
            (200.0, 350.0, "200-350ms"),
            (350.0, 500.0, "350-500ms"),
            (500.0, 800.0, "0.5-0.8s"),
            (800.0, 1000.0, "0.8-1s"),
            (1000.0, 1500.0, "1-1.5s"),
            (1500.0, 2000.0, "1.5-2s"),
            (2000.0, 3000.0, "2-3s"),
            (3000.0, 5000.0, "3-5s"),
            (5000.0, 10000.0, "5-10s"),
            (10000.0, 30000.0, "10-30s"),
            (30000.0, 60000.0, "30-60s"),
            (60000.0, f64::MAX, ">60s"),
        ];
        let mut result: Vec<HistogramBucket> = RANGES
            .iter()
            .map(|(min, max, label)| HistogramBucket {
                label: (*label).to_string(),
                min_ms: *min,
                max_ms: *max,
                count: 0,
            })
            .collect();
        for (index, bucket) in self.buckets.iter().enumerate() {
            let count = bucket.load(Ordering::Relaxed) as i64;
            if count == 0 {
                continue;
            }
            let value = index as f64;
            for item in result.iter_mut() {
                if value >= item.min_ms && value < item.max_ms {
                    item.count += count;
                    break;
                }
            }
        }
        result
    }
}

/// 运行时调度：阶梯加压时调整生效并发，限速时按秒发放令牌
struct Control {
    active: AtomicU32,
    rate: AtomicU32,
    tokens: AtomicU64,
    stage: std::sync::atomic::AtomicI64,
}

impl Control {
    fn new(active: u32, rate: u32) -> Self {
        Self {
            active: AtomicU32::new(active),
            rate: AtomicU32::new(rate),
            tokens: AtomicU64::new(rate as u64),
            stage: std::sync::atomic::AtomicI64::new(0),
        }
    }
}

struct RunCtx {
    client: reqwest::Client,
    method: reqwest::Method,
    config: StressConfig,
    vars: HashMap<String, String>,
    /// 启动时渲染好的认证配置（模板变量已替换）
    auth: RequestAuth,
    /// 启动时按目标 URL 预取的本机 Cookie（未启用或用户自建 Cookie 头时为空）
    cookie: Option<String>,
    metrics: Arc<Metrics>,
    control: Arc<Control>,
    start: Instant,
    deadline: Instant,
    cancel: Arc<AtomicBool>,
    errors: mpsc::Sender<ReportError>,
    records: mpsc::UnboundedSender<ReportRequestRecord>,
}

struct Attempt {
    status: u16,
    ok: bool,
    url: String,
    error: Option<String>,
    snippet: String,
    ttfb_ms: u64,
    bytes: u64,
    body: Option<String>,
    /// 查询参数（query string）
    params: String,
    /// 请求头 [[key, value], ...] 序列化后的 JSON
    headers: String,
    /// 响应头 JSON
    response_headers: Option<String>,
    /// 响应体（截断）
    response_body: Option<String>,
}

fn headers_json(pairs: &[(String, String)]) -> String {
    if pairs.is_empty() {
        return String::new();
    }
    serde_json::to_string(pairs).unwrap_or_default()
}

/// 认证配置在启动时渲染一次，避免每个请求重复解析模板
fn render_auth(auth: &RequestAuth, vars: &HashMap<String, String>) -> RequestAuth {
    RequestAuth {
        kind: auth.kind.clone(),
        token: render(&auth.token, vars),
        username: render(&auth.username, vars),
        password: render(&auth.password, vars),
        key: render(&auth.key, vars),
        value: render(&auth.value, vars),
        add_to: auth.add_to.clone(),
        token_url: render(&auth.token_url, vars),
        client_id: render(&auth.client_id, vars),
        client_secret: render(&auth.client_secret, vars),
        scope: render(&auth.scope, vars),
    }
}

fn build_url(
    config: &StressConfig,
    vars: &HashMap<String, String>,
    auth: &RequestAuth,
) -> Result<reqwest::Url, String> {
    let mut raw = render(config.url.trim(), vars).trim().to_string();
    if raw.is_empty() {
        return Err("URL 不能为空".to_string());
    }
    if !raw.contains("://") {
        raw = format!("http://{raw}");
    }
    let mut url = reqwest::Url::parse(&raw).map_err(|e| format!("URL 无法解析：{e}"))?;
    if !config.params.is_empty() {
        let mut pairs = url.query_pairs_mut();
        for item in &config.params {
            if !item.enabled {
                continue;
            }
            let key = render(item.key.trim(), vars);
            if key.is_empty() {
                continue;
            }
            pairs.append_pair(&key, &render(&item.value, vars));
        }
    }
    // API Key 认证可以放在查询参数上（auth 已渲染，无需再次替换变量）
    if auth.kind == "apikey" && auth.add_to == "query" && !auth.key.trim().is_empty() {
        let mut pairs = url.query_pairs_mut();
        pairs.append_pair(auth.key.trim(), &auth.value);
    }
    Ok(url)
}

async fn read_snippet(mut response: reqwest::Response, limit: usize) -> (String, u64) {
    let declared = response
        .headers()
        .get(reqwest::header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0);

    let mut buffer: Vec<u8> = Vec::with_capacity(limit);
    while buffer.len() < limit {
        match response.chunk().await {
            Ok(Some(chunk)) => {
                let take = (limit - buffer.len()).min(chunk.len());
                buffer.extend_from_slice(&chunk[..take]);
                if take < chunk.len() {
                    break;
                }
            }
            _ => break,
        }
    }
    let bytes = if declared > 0 {
        declared
    } else {
        buffer.len() as u64
    };
    (String::from_utf8_lossy(&buffer).into_owned(), bytes)
}

async fn send_once(ctx: &RunCtx) -> Attempt {
    let url = match build_url(&ctx.config, &ctx.vars, &ctx.auth) {
        Ok(url) => url,
        Err(error) => {
            return Attempt {
                status: 0,
                ok: false,
                url: ctx.config.url.clone(),
                error: Some(error),
                snippet: String::new(),
                ttfb_ms: 0,
                bytes: 0,
                body: None,
                params: String::new(),
                headers: String::new(),
                response_headers: None,
                response_body: None,
            }
        }
    };
    let query = url.query().unwrap_or_default().to_string();
    let url_text = url.to_string();

    let mut builder = ctx
        .client
        .request(ctx.method.clone(), url)
        .timeout(Duration::from_millis(ctx.config.timeout_ms.max(500)));

    let mut header_pairs: Vec<(String, String)> = Vec::new();
    let mut has_content_type = false;
    let mut has_authorization = false;
    let mut has_cookie = false;
    for item in &ctx.config.headers {
        if !item.enabled {
            continue;
        }
        let key = render(item.key.trim(), &ctx.vars);
        if key.is_empty() {
            continue;
        }
        if key.eq_ignore_ascii_case("content-type") {
            has_content_type = true;
        }
        if key.eq_ignore_ascii_case("authorization") {
            has_authorization = true;
        }
        if key.eq_ignore_ascii_case("cookie") {
            has_cookie = true;
        }
        let value = render(&item.value, &ctx.vars);
        header_pairs.push((key.clone(), value.clone()));
        builder = builder.header(key, value);
    }

    // 认证头：用户手动配置的 Authorization 优先，与手工请求保持一致
    if !has_authorization {
        match ctx.auth.kind.as_str() {
            "bearer" | "oauth2" if !ctx.auth.token.trim().is_empty() => {
                let value = format!("Bearer {}", ctx.auth.token.trim());
                header_pairs.push(("Authorization".to_string(), value.clone()));
                builder = builder.header(reqwest::header::AUTHORIZATION, value);
            }
            "basic" => {
                let raw = format!("{}:{}", ctx.auth.username, ctx.auth.password);
                let value = format!("Basic {}", base64_encode(raw.as_bytes()));
                header_pairs.push(("Authorization".to_string(), value.clone()));
                builder = builder.header(reqwest::header::AUTHORIZATION, value);
            }
            "apikey" if ctx.auth.add_to != "query" && !ctx.auth.key.trim().is_empty() => {
                header_pairs.push((ctx.auth.key.clone(), ctx.auth.value.clone()));
                builder = builder.header(ctx.auth.key.clone(), ctx.auth.value.clone());
            }
            _ => {}
        }
    }

    // 自动携带本机 Cookie（启动时已按目标 URL 预取）
    if !has_cookie {
        if let Some(cookie) = &ctx.cookie {
            header_pairs.push(("Cookie".to_string(), cookie.clone()));
            builder = builder.header(reqwest::header::COOKIE, cookie.clone());
        }
    }

    let body_type = ctx.config.body_type.as_str();
    let mut request_body: Option<String> = None;
    if !body_type.is_empty() && body_type != "none" {
        if !has_content_type {
            let content_type = match body_type {
                "json" => "application/json",
                "form" => "application/x-www-form-urlencoded",
                _ => "text/plain; charset=utf-8",
            };
            builder = builder.header(reqwest::header::CONTENT_TYPE, content_type);
            header_pairs.push(("Content-Type".to_string(), content_type.to_string()));
        }
        let raw_body = ctx.config.body.as_deref().unwrap_or("");
        // JSON 请求体里的注释在发送前去掉，与 Postman 行为一致
        let source = if body_type == "json" {
            strip_json_comments(raw_body)
        } else {
            raw_body.to_string()
        };
        let rendered = render(&source, &ctx.vars);
        builder = builder.body(rendered.clone());
        request_body = Some(rendered);
    }

    let request_headers = headers_json(&header_pairs);
    let started = Instant::now();
    match builder.send().await {
        Ok(response) => {
            let ttfb_ms = started.elapsed().as_millis() as u64;
            let status = response.status();
            let response_headers = headers_json(
                &response
                    .headers()
                    .iter()
                    .map(|(key, value)| {
                        (
                            key.as_str().to_string(),
                            value.to_str().unwrap_or_default().to_string(),
                        )
                    })
                    .collect::<Vec<_>>(),
            );
            let (text, bytes) = read_snippet(response, MAX_RECORD_RESPONSE).await;
            let snippet = clip(&text, SNIPPET_LIMIT);
            let response_body = if text.trim().is_empty() {
                None
            } else {
                Some(clip(&text, MAX_RECORD_RESPONSE))
            };
            let code = status.as_u16();
            let ok = code < 400;
            Attempt {
                status: code,
                ok,
                url: url_text,
                error: if ok {
                    None
                } else {
                    Some(format!("HTTP {code}"))
                },
                snippet,
                ttfb_ms,
                bytes,
                body: request_body,
                params: query,
                headers: request_headers,
                response_headers: if response_headers.is_empty() {
                    None
                } else {
                    Some(response_headers)
                },
                response_body,
            }
        }
        Err(error) => Attempt {
            status: 0,
            ok: false,
            url: url_text,
            error: Some(error.to_string()),
            snippet: String::new(),
            ttfb_ms: 0,
            bytes: 0,
            body: request_body,
            params: query,
            headers: request_headers,
            response_headers: None,
            response_body: None,
        },
    }
}

fn clip(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let mut out: String = text.chars().take(limit).collect();
    out.push('…');
    out
}

fn classify_error(message: &str) -> &'static str {
    let lower = message.to_lowercase();
    if lower.contains("timed out") || lower.contains("timeout") {
        "TIMEOUT"
    } else if lower.contains("certificate") || lower.contains("tls") {
        "TLS"
    } else if lower.contains("connect") || lower.contains("connection") {
        "CONNECT"
    } else if lower.contains("dns") {
        "DNS"
    } else {
        "NETWORK"
    }
}

/// 当前时刻应生效的并发数与阶段序号
fn stage_at(stages: &[StressStage], elapsed_sec: u64, fallback: u32) -> (u32, i64) {
    if stages.is_empty() {
        return (fallback, 0);
    }
    let mut acc = 0u64;
    for (index, stage) in stages.iter().enumerate() {
        acc += stage.duration_sec.max(1) as u64;
        if elapsed_sec < acc {
            return (stage.concurrent.clamp(1, 500), index as i64);
        }
    }
    let last = stages.len() - 1;
    (stages[last].concurrent.clamp(1, 500), last as i64)
}

async fn take_token(ctx: &RunCtx) -> bool {
    if ctx.control.rate.load(Ordering::Relaxed) == 0 {
        return true;
    }
    loop {
        if ctx.cancel.load(Ordering::Relaxed) || Instant::now() >= ctx.deadline {
            return false;
        }
        let tokens = ctx.control.tokens.load(Ordering::Relaxed);
        if tokens > 0 {
            if ctx
                .control
                .tokens
                .compare_exchange(tokens, tokens - 1, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                return true;
            }
            continue;
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
}

async fn worker(ctx: Arc<RunCtx>, index: u32, workers: u32) {
    let ramp_ms = if workers > 1 {
        (ctx.config.ramp_up_sec as u64 * 1000 * index as u64) / (workers - 1) as u64
    } else {
        0
    };
    let mut remaining = ramp_ms;
    while remaining > 0 {
        if ctx.cancel.load(Ordering::Relaxed) {
            return;
        }
        let step = remaining.min(200);
        tokio::time::sleep(Duration::from_millis(step)).await;
        remaining -= step;
    }

    loop {
        if ctx.cancel.load(Ordering::Relaxed) || Instant::now() >= ctx.deadline {
            break;
        }

        // 阶梯加压：未进入生效区间的 worker 等待
        if index >= ctx.control.active.load(Ordering::Relaxed) {
            tokio::time::sleep(Duration::from_millis(50)).await;
            continue;
        }

        if !take_token(&ctx).await {
            break;
        }

        ctx.metrics.concurrent.fetch_add(1, Ordering::Relaxed);
        let started = Instant::now();
        let attempt = send_once(&ctx).await;
        let latency_ms = started.elapsed().as_millis() as u64;
        ctx.metrics.concurrent.fetch_sub(1, Ordering::Relaxed);
        ctx.metrics.record(
            latency_ms,
            attempt.ttfb_ms.min(latency_ms),
            attempt.status,
            attempt.bytes,
            attempt.ok,
        );

        let error_type = if attempt.ok {
            None
        } else if attempt.status > 0 {
            Some("HTTP".to_string())
        } else {
            Some(classify_error(attempt.error.as_deref().unwrap_or("")).to_string())
        };
        let ttfb_ms = attempt.ttfb_ms.min(latency_ms);
        let _ = ctx.records.send(ReportRequestRecord {
            elapsed_ms: ctx.start.elapsed().as_millis() as i64,
            time_ms: now_ms() - latency_ms as i64,
            method: ctx.method.as_str().to_string(),
            url: attempt.url.clone(),
            status: attempt.status as i64,
            latency_ms: latency_ms as i64,
            ttfb_ms: ttfb_ms as i64,
            bytes: attempt.bytes as i64,
            success: attempt.ok,
            body: attempt
                .body
                .as_deref()
                .map(|text| clip(text, MAX_RECORD_BODY)),
            params: if attempt.params.is_empty() {
                None
            } else {
                Some(attempt.params.clone())
            },
            headers: if attempt.headers.is_empty() {
                None
            } else {
                Some(attempt.headers.clone())
            },
            response_headers: attempt.response_headers.clone(),
            response_body: attempt.response_body.clone(),
            error_type: error_type.clone(),
            error_message: attempt
                .error
                .as_deref()
                .map(|message| clip(message, MAX_RECORD_ERROR)),
        });

        if !attempt.ok {
            ctx.metrics
                .bump_error_type(error_type.as_deref().unwrap_or("NETWORK"));
            let item = ReportError {
                elapsed_sec: ctx.start.elapsed().as_secs() as i64,
                error_type: error_type.unwrap_or_else(|| "NETWORK".to_string()),
                error_message: attempt.error.clone().unwrap_or_default(),
                request_url: attempt.url,
                response_status: if attempt.status > 0 {
                    Some(attempt.status as i64)
                } else {
                    None
                },
                response_body: attempt.snippet,
            };
            let _ = ctx.errors.try_send(item);
        }

        if ctx.config.think_time_ms > 0 {
            tokio::time::sleep(Duration::from_millis(ctx.config.think_time_ms)).await;
        }
    }
}

/// 压测执行结果：命令层与套件 `kind=stress` 用例共用
pub struct StressExecOutcome {
    pub report_id: Option<i64>,
    pub report: StressReport,
    pub message: Option<String>,
}

struct PreparedStress {
    ctx: Arc<RunCtx>,
    name: String,
    stages: Vec<StressStage>,
    peak: u32,
    total_sec: u64,
    rate: u32,
    target_rps: f64,
    config_snapshot: String,
    stop_rate: Option<f64>,
    stop_min: u32,
    concurrent: i64,
    errors_rx: mpsc::Receiver<ReportError>,
    records_rx: mpsc::UnboundedReceiver<ReportRequestRecord>,
}

/// 校验配置、渲染模板、构建客户端并准备 Cookie 快照；不依赖 Tauri 事件
fn prepare_stress(state: &AppState, config: StressConfig) -> Result<PreparedStress, String> {
    let mut vars = match config.env_id {
        Some(env_id) => {
            let conn = state.pool.get().map_err(err)?;
            crate::commands::http::load_vars(&conn, env_id)?
        }
        None => HashMap::new(),
    };
    // 运行期变量优先于环境变量
    for (key, value) in &config.vars {
        if !key.trim().is_empty() {
            vars.insert(key.clone(), value.clone());
        }
    }

    let method = reqwest::Method::from_bytes(config.method.trim().to_uppercase().as_bytes())
        .map_err(|_| format!("不支持的请求方法：{}", config.method))?;

    let auth = config
        .auth
        .clone()
        .map(|auth| render_auth(&auth, &vars))
        .unwrap_or_default();

    let prepared_url = build_url(&config, &vars, &auth)?;

    // Cookie 只按目标 URL 取一次：压测过程中不写回 Cookie 库，避免高并发下的写竞争
    let cookie_enabled = config.cookie_enabled.unwrap_or(true);
    let user_has_cookie = config
        .headers
        .iter()
        .any(|item| item.enabled && item.key.trim().eq_ignore_ascii_case("cookie"));
    let cookie = if cookie_enabled && !user_has_cookie {
        state
            .pool
            .get()
            .ok()
            .map(|conn| crate::commands::cookie::cookies_for_url(&conn, &prepared_url))
            .filter(|items| !items.is_empty())
            .map(|items| items.join("; "))
    } else {
        None
    };

    state.stress_cancel.store(false, Ordering::SeqCst);

    // 固定并发或阶梯加压
    let stages: Vec<StressStage> = config
        .stages
        .iter()
        .filter(|item| item.duration_sec > 0)
        .map(|item| StressStage {
            concurrent: item.concurrent.clamp(1, 500),
            duration_sec: item.duration_sec.clamp(1, 3600),
        })
        .collect();

    let peak = if stages.is_empty() {
        config.concurrent.clamp(1, 500)
    } else {
        stages.iter().map(|item| item.concurrent).max().unwrap_or(1)
    };
    let total_sec = if stages.is_empty() {
        config.duration_sec.clamp(1, 3600) as u64
    } else {
        stages
            .iter()
            .map(|item| item.duration_sec as u64)
            .sum::<u64>()
            .clamp(1, 7200)
    };

    let name = config
        .name
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| config.url.clone());
    let rate = config
        .target_rps
        .map(|value| value.max(0.0).round() as u32)
        .unwrap_or(0);
    let target_rps = rate as f64;

    let metrics = Arc::new(Metrics::new());
    let control = Arc::new(Control::new(
        if stages.is_empty() { peak } else { stages[0].concurrent },
        rate,
    ));
    let cancel = state.stress_cancel.clone();
    // 按当前配置构建客户端：连接超时、重定向策略可单独覆盖，代理与客户端证书沿用应用设置
    let client = state.client_with_options(
        config.insecure,
        config.connect_timeout_ms,
        config.follow_redirects,
    )?;
    let stop_rate = config.stop_error_rate.filter(|value| *value > 0.0);
    let stop_min = config.stop_min_requests.unwrap_or(100).max(1);
    let start = Instant::now();
    let deadline = start + Duration::from_secs(total_sec);
    let concurrent = peak as i64;

    let (errors_tx, errors_rx) = mpsc::channel::<ReportError>(1024);
    let (records_tx, records_rx) = mpsc::unbounded_channel::<ReportRequestRecord>();

    let config_snapshot = json!({
        "mode": if stages.is_empty() { "fixed" } else { "stages" },
        "method": config.method.clone(),
        "url": config.url.clone(),
        "headers": config.headers.iter().filter(|h| h.enabled && !h.key.trim().is_empty()).count(),
        "params": config.params.iter().filter(|p| p.enabled && !p.key.trim().is_empty()).count(),
        "bodyType": config.body_type.clone(),
        "envId": config.env_id,
        // 只记录认证方式与开关，绝不写入密钥值
        "authKind": if auth.kind.trim().is_empty() { "none" } else { auth.kind.as_str() },
        "apiKeyIn": auth.add_to.clone(),
        "cookieEnabled": cookie_enabled,
        "runtimeVars": config.vars.len(),
        "timeoutMs": config.timeout_ms,
        "connectTimeoutMs": config.connect_timeout_ms,
        "followRedirects": config.follow_redirects,
        "stopErrorRate": config.stop_error_rate,
        "stopMinRequests": config.stop_min_requests,
        "thinkTimeMs": config.think_time_ms,
        "rampUpSec": config.ramp_up_sec,
        "targetRps": target_rps,
        "stages": stages,
        "sla": config.sla,
    });
    let config_snapshot = config_snapshot.to_string();

    let ctx = Arc::new(RunCtx {
        client,
        method,
        config,
        vars,
        auth,
        cookie,
        metrics,
        control,
        start,
        deadline,
        cancel,
        errors: errors_tx,
        records: records_tx,
    });

    Ok(PreparedStress {
        ctx,
        name,
        stages,
        peak,
        total_sec,
        rate,
        target_rps,
        config_snapshot,
        stop_rate,
        stop_min,
        concurrent,
        errors_rx,
        records_rx,
    })
}

/// 执行压测并落库；进度通过回调交给调用方（桌面端转 Tauri 事件）
async fn drive_stress(
    prepared: PreparedStress,
    pool: DbPool,
    progress: Option<&(dyn Fn(StressProgress) + Send + Sync)>,
    watch: Option<&Arc<AtomicBool>>,
) -> StressExecOutcome {
    let PreparedStress {
        ctx,
        name,
        stages,
        peak,
        total_sec,
        rate,
        target_rps,
        config_snapshot,
        stop_rate,
        stop_min,
        concurrent,
        mut errors_rx,
        mut records_rx,
    } = prepared;
    let start = ctx.start;
    let deadline = ctx.deadline;
    let cancel = ctx.cancel.clone();
    let metrics = ctx.metrics.clone();
    let control = ctx.control.clone();

    let mut tasks = JoinSet::new();
    for index in 0..peak {
        tasks.spawn(worker(ctx.clone(), index, peak));
    }

    let mut samples: Vec<ReportSample> = Vec::new();
    let mut errors: Vec<ReportError> = Vec::new();
    let mut requests: Vec<ReportRequestRecord> = Vec::new();
    let mut prev_total = 0u64;
    let mut prev_success = 0u64;
    let mut prev_errors = 0u64;
    let mut prev_bytes = 0u64;
    let mut left_sec = total_sec;
    let mut auto_stop: Option<String> = None;

    let mut ticker = tokio::time::interval(Duration::from_millis(1000));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        ticker.tick().await;
        // 套件运行被停止时同步结束压测，最迟 1 秒内生效
        if let Some(flag) = watch {
            if flag.load(Ordering::SeqCst) {
                cancel.store(true, Ordering::SeqCst);
            }
        }
        left_sec = left_sec.saturating_sub(1);

        while let Ok(item) = errors_rx.try_recv() {
            if errors.len() < MAX_ERRORS {
                errors.push(item);
            }
        }
        while let Ok(item) = records_rx.try_recv() {
            if requests.len() < MAX_REQUEST_RECORDS {
                requests.push(item);
            }
        }

        let elapsed = start.elapsed().as_secs();
        let total = metrics.count();
        let success = metrics.success.load(Ordering::Relaxed);
        let failure = metrics.errors.load(Ordering::Relaxed);
        let bytes = metrics.bytes.load(Ordering::Relaxed);

        // 阶梯加压：按时间切换生效并发
        let (active, stage_index) = stage_at(&stages, elapsed, peak);
        control.active.store(active, Ordering::Relaxed);
        control.stage.store(stage_index, Ordering::Relaxed);
        // 限速：每秒补满令牌
        if rate > 0 {
            control.tokens.store(rate as u64, Ordering::Relaxed);
        }

        if elapsed > 0 {
            samples.push(ReportSample {
                elapsed_sec: elapsed,
                concurrent: metrics.concurrent.load(Ordering::Relaxed),
                qps: total.saturating_sub(prev_total) as f64,
                avg_rt: metrics.avg(),
                p50_rt: metrics.percentile(0.5),
                p95_rt: metrics.percentile(0.95),
                p99_rt: metrics.percentile(0.99),
                ttfb_avg: metrics.ttfb_avg(),
                bytes: bytes.saturating_sub(prev_bytes),
                success_count: success.saturating_sub(prev_success),
                error_count: failure.saturating_sub(prev_errors),
            });
            prev_total = total;
            prev_success = success;
            prev_errors = failure;
            prev_bytes = bytes;
        }

        if let Some(sink) = progress {
            sink(StressProgress {
                name: name.clone(),
                running: true,
                elapsed_sec: elapsed,
                total,
                success,
                errors: failure,
                rps: if elapsed > 0 {
                    total as f64 / elapsed as f64
                } else {
                    0.0
                },
                avg_rt: metrics.avg(),
                p95_rt: metrics.percentile(0.95),
                p99_rt: metrics.percentile(0.99),
                concurrent: metrics.concurrent.load(Ordering::Relaxed),
                stage_index,
                active_concurrent: active,
                status_codes: metrics.status_codes(),
                samples: samples.clone(),
            });
        }

        // 自动停止判定：错误率达到阈值且请求数足够时提前结束
        if auto_stop.is_none() {
            if let Some(limit) = stop_rate {
                if total >= stop_min as u64 && total > 0 {
                    let error_rate = failure as f64 / total as f64 * 100.0;
                    if error_rate > limit {
                        auto_stop = Some(format!(
                            "错误率 {:.1}% 超过阈值 {:.1}%，已自动停止",
                            error_rate, limit
                        ));
                        cancel.store(true, Ordering::SeqCst);
                    }
                }
            }
        }

        let stopped = cancel.load(Ordering::SeqCst);
        if stopped || Instant::now() >= deadline || left_sec == 0 {
            break;
        }
    }

    // 等待在途请求结束，最多等 5 秒
    let wait_all = async {
        while tasks.join_next().await.is_some() {}
    };
    if tokio::time::timeout(Duration::from_secs(5), wait_all)
        .await
        .is_err()
    {
        tasks.abort_all();
    }

    while let Ok(item) = errors_rx.try_recv() {
        if errors.len() < MAX_ERRORS {
            errors.push(item);
        }
    }
    while let Ok(item) = records_rx.try_recv() {
        if requests.len() < MAX_REQUEST_RECORDS {
            requests.push(item);
        }
    }

    let elapsed = start.elapsed().as_secs_f64().max(0.001);
    let total = metrics.count();
    let success = metrics.success.load(Ordering::Relaxed);
    let failure = metrics.errors.load(Ordering::Relaxed);
    let status = if cancel.load(Ordering::SeqCst) {
        "stopped"
    } else {
        "completed"
    };
    let end_time = now_ms();
    let start_time = end_time - (elapsed * 1000.0) as i64;
    let max_qps = samples.iter().map(|s| s.qps).fold(0.0, f64::max);

    let histogram = metrics.histogram();
    let status_codes = metrics.status_codes();
    let stage_summaries = summarize_stages(&stages, &samples);
    let error_types = metrics.error_types();

    let report = StressReport {
        id: 0,
        plan_name: name,
        request_id: ctx.config.request_id,
        total_requests: total as i64,
        success_count: success as i64,
        error_count: failure as i64,
        avg_rt: metrics.avg(),
        min_rt: metrics.min(),
        max_rt: metrics.max(),
        p50_rt: metrics.percentile(0.5),
        p75_rt: metrics.percentile(0.75),
        p90_rt: metrics.percentile(0.9),
        p95_rt: metrics.percentile(0.95),
        p99_rt: metrics.percentile(0.99),
        p999_rt: metrics.percentile(0.999),
        stddev_rt: metrics.stddev(),
        ttfb_avg: metrics.ttfb_avg(),
        ttfb_p95: metrics.ttfb_percentile(0.95),
        apdex: metrics.apdex(),
        bytes_total: metrics.bytes.load(Ordering::Relaxed) as i64,
        max_qps,
        avg_qps: if elapsed > 0.0 {
            total as f64 / elapsed
        } else {
            0.0
        },
        target_rps,
        error_rate: if total > 0 {
            failure as f64 / total as f64 * 100.0
        } else {
            0.0
        },
        concurrency: concurrent,
        status: status.to_string(),
        start_time: Some(start_time),
        end_time: Some(end_time),
        created_at: end_time,
    };

    let payload = ReportPayload {
        report: &report,
        samples: &samples,
        errors: &errors,
        requests: &requests,
        stages: &stage_summaries,
        status_codes: &status_codes,
        histogram: &histogram,
        error_types: &error_types,
        config: &config_snapshot,
    };
    let (report_id, message) = match save_report(&pool, payload) {
        Ok(id) => (Some(id), None),
        Err(error) => (None, Some(format!("报告保存失败：{error}"))),
    };
    let message = message.or(auto_stop);

    StressExecOutcome {
        report_id,
        report,
        message,
    }
}

#[tauri::command]
pub async fn stress_start(
    state: State<'_, AppState>,
    app: AppHandle,
    config: StressConfig,
) -> Result<(), String> {
    if state.stress_running.swap(true, Ordering::SeqCst) {
        return Err("已有压测任务正在运行，请先停止或等待结束".to_string());
    }
    let prepared = match prepare_stress(state.inner(), config) {
        Ok(prepared) => prepared,
        Err(error) => {
            state.stress_running.store(false, Ordering::SeqCst);
            return Err(error);
        }
    };
    let pool = state.pool.clone();
    let running = state.stress_running.clone();
    tauri::async_runtime::spawn(async move {
        let progress_app = app.clone();
        let sink = move |progress: StressProgress| {
            let _ = progress_app.emit("stress:progress", &progress);
        };
        let outcome = drive_stress(prepared, pool, Some(&sink), None).await;
        running.store(false, Ordering::SeqCst);
        let elapsed_sec = outcome
            .report
            .end_time
            .unwrap_or(0)
            .saturating_sub(outcome.report.start_time.unwrap_or(0)) as u64
            / 1000;
        let _ = app.emit(
            "stress:done",
            &StressDone {
                report_id: outcome.report_id,
                name: outcome.report.plan_name.clone(),
                status: outcome.report.status.clone(),
                total: outcome.report.total_requests.max(0) as u64,
                success: outcome.report.success_count.max(0) as u64,
                errors: outcome.report.error_count.max(0) as u64,
                elapsed_sec,
                message: outcome.message,
            },
        );
    });

    Ok(())
}

/// 套件内执行压测用例：与手工压测互斥，结束（含出错）后释放标记
pub async fn run_stress_case(
    state: &AppState,
    config: StressConfig,
    watch: Option<&Arc<AtomicBool>>,
) -> Result<StressExecOutcome, String> {
    if state.stress_running.swap(true, Ordering::SeqCst) {
        return Err("已有压测任务正在运行（手工压测或另一个套件），请先停止或等待结束".to_string());
    }
    let prepared = match prepare_stress(state, config) {
        Ok(prepared) => prepared,
        Err(error) => {
            state.stress_running.store(false, Ordering::SeqCst);
            return Err(error);
        }
    };
    let pool = state.pool.clone();
    let outcome = drive_stress(prepared, pool, None, watch).await;
    state.stress_running.store(false, Ordering::SeqCst);
    Ok(outcome)
}

/// 按阶段把每秒采样归并成阶段性汇总
fn summarize_stages(stages: &[StressStage], samples: &[ReportSample]) -> Vec<StageSummary> {
    if stages.is_empty() {
        return Vec::new();
    }
    let mut result = Vec::new();
    let mut start_sec = 0u64;
    for (index, stage) in stages.iter().enumerate() {
        let end_sec = start_sec + stage.duration_sec as u64;
        let window: Vec<&ReportSample> = samples
            .iter()
            .filter(|s| s.elapsed_sec > start_sec && s.elapsed_sec <= end_sec)
            .collect();
        let requests: i64 = window
            .iter()
            .map(|s| (s.success_count + s.error_count) as i64)
            .sum();
        let errors: i64 = window.iter().map(|s| s.error_count as i64).sum();
        let avg_qps = if window.is_empty() {
            0.0
        } else {
            requests as f64 / window.len() as f64
        };
        let avg_rt = if window.is_empty() {
            0.0
        } else {
            window.iter().map(|s| s.avg_rt).sum::<f64>() / window.len() as f64
        };
        let p95_rt = window.iter().map(|s| s.p95_rt).fold(0.0, f64::max);
        result.push(StageSummary {
            index: index as i64,
            concurrent: stage.concurrent as i64,
            duration_sec: stage.duration_sec as i64,
            requests,
            errors,
            avg_qps,
            avg_rt,
            p95_rt,
        });
        start_sec = end_sec;
    }
    result
}

#[tauri::command]
pub fn stress_stop(state: State<'_, AppState>) -> Result<(), String> {
    state.stress_cancel.store(true, Ordering::SeqCst);
    Ok(())
}

struct ReportPayload<'a> {
    report: &'a StressReport,
    samples: &'a [ReportSample],
    errors: &'a [ReportError],
    requests: &'a [ReportRequestRecord],
    stages: &'a [StageSummary],
    status_codes: &'a [StatusCount],
    histogram: &'a [HistogramBucket],
    error_types: &'a [ErrorTypeCount],
    config: &'a str,
}

fn save_report(pool: &DbPool, payload: ReportPayload<'_>) -> Result<i64, String> {
    let report = payload.report;
    let mut conn = pool.get().map_err(err)?;
    let tx = conn.transaction().map_err(err)?;

    let stages_text = serde_json::to_string(payload.stages).map_err(err)?;
    let codes_text = serde_json::to_string(payload.status_codes).map_err(err)?;
    let histogram_text = serde_json::to_string(payload.histogram).map_err(err)?;
    let types_text = serde_json::to_string(payload.error_types).map_err(err)?;

    tx.execute(
        "INSERT INTO test_reports
            (plan_id, plan_name, request_id, total_requests, success_count, error_count,
             avg_rt, min_rt, max_rt, p50_rt, p75_rt, p90_rt, p95_rt, p99_rt, p999_rt,
             stddev_rt, ttfb_avg, ttfb_p95, apdex, bytes_total, max_qps, avg_qps, target_rps,
             error_rate, concurrency, status, config_json, stages_json, codes_json,
             histogram_json, error_types_json, start_time, end_time, created_at)
         VALUES (NULL, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14,
                 ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28,
                 ?29, ?30, ?31, ?32, ?33)",
        params![
            report.plan_name,
            report.request_id,
            report.total_requests,
            report.success_count,
            report.error_count,
            report.avg_rt,
            report.min_rt,
            report.max_rt,
            report.p50_rt,
            report.p75_rt,
            report.p90_rt,
            report.p95_rt,
            report.p99_rt,
            report.p999_rt,
            report.stddev_rt,
            report.ttfb_avg,
            report.ttfb_p95,
            report.apdex,
            report.bytes_total,
            report.max_qps,
            report.avg_qps,
            report.target_rps,
            report.error_rate,
            report.concurrency,
            report.status,
            payload.config,
            stages_text,
            codes_text,
            histogram_text,
            types_text,
            report.start_time,
            report.end_time,
            report.created_at,
        ],
    )
    .map_err(err)?;

    let report_id = tx.last_insert_rowid();

    {
        let mut stmt = tx
            .prepare(
                "INSERT INTO report_samples
                    (report_id, elapsed_sec, concurrent, qps, avg_rt, p50_rt, p95_rt, p99_rt,
                     ttfb_avg, bytes, success_count, error_count)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            )
            .map_err(err)?;
        for sample in payload.samples {
            stmt.execute(params![
                report_id,
                sample.elapsed_sec as i64,
                sample.concurrent as i64,
                sample.qps,
                sample.avg_rt,
                sample.p50_rt,
                sample.p95_rt,
                sample.p99_rt,
                sample.ttfb_avg,
                sample.bytes as i64,
                sample.success_count as i64,
                sample.error_count as i64,
            ])
            .map_err(err)?;
        }
    }

    {
        let mut stmt = tx
            .prepare(
                "INSERT INTO report_errors
                    (report_id, elapsed_sec, error_type, error_message, request_url, response_status, response_body)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )
            .map_err(err)?;
        for item in payload.errors {
            stmt.execute(params![
                report_id,
                item.elapsed_sec,
                item.error_type,
                item.error_message,
                item.request_url,
                item.response_status,
                item.response_body,
            ])
            .map_err(err)?;
        }
    }

    // 兼容旧库：逐请求明细表可能尚未创建，缺失时在此补建，避免整份报告保存失败
    let _ = tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS report_requests (
            id            INTEGER PRIMARY KEY AUTOINCREMENT,
            report_id     INTEGER NOT NULL REFERENCES test_reports(id) ON DELETE CASCADE,
            elapsed_ms    INTEGER DEFAULT 0,
            time_ms       INTEGER DEFAULT 0,
            method        TEXT,
            url           TEXT,
            status        INTEGER DEFAULT 0,
            latency_ms    INTEGER DEFAULT 0,
            ttfb_ms       INTEGER DEFAULT 0,
            bytes         INTEGER DEFAULT 0,
            success       INTEGER DEFAULT 1,
            body          TEXT,
            params        TEXT,
            req_headers   TEXT,
            res_headers   TEXT,
            res_body      TEXT,
            error_type    TEXT,
            error_message TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_report_requests_report ON report_requests(report_id, id);",
    );

    let _ = tx.execute_batch("SAVEPOINT req_rows");
    let detail_result = (|| -> Result<(), String> {
        let mut stmt = tx
            .prepare(
                "INSERT INTO report_requests
                    (report_id, elapsed_ms, time_ms, method, url, status, latency_ms, ttfb_ms,
                     bytes, success, body, params, req_headers, res_headers, res_body,
                     error_type, error_message)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14,
                         ?15, ?16, ?17)",
            )
            .map_err(err)?;
        for item in payload.requests {
            stmt.execute(params![
                report_id,
                item.elapsed_ms,
                item.time_ms,
                item.method,
                item.url,
                item.status,
                item.latency_ms,
                item.ttfb_ms,
                item.bytes,
                item.success as i64,
                item.body,
                item.params,
                item.headers,
                item.response_headers,
                item.response_body,
                item.error_type,
                item.error_message,
            ])
            .map_err(err)?;
        }
        Ok(())
    })();
    // 明细写入失败只回滚这一段，报告主体照常提交
    if detail_result.is_ok() {
        let _ = tx.execute_batch("RELEASE req_rows");
    } else {
        log::warn!("压测请求明细写入失败：{:?}", detail_result.as_ref().err());
        let _ = tx.execute_batch("ROLLBACK TO req_rows; RELEASE req_rows");
    }

    tx.commit().map_err(err)?;
    Ok(report_id)
}
