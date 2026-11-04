//! 抓包 HTTP handler。
//!
//! hudsucker 对每个连接/请求都会 clone 一份 handler，因此 `pending` 只需要
//! 保存「当前这一条请求」的上下文，不会和同连接上的其它请求串味。
//! `pending` 用独立 Arc 实现内部可变：请求体需要异步读取，读完后才能登记；
//! 手动实现 Clone，保证每个请求拿到的都是全新的槽位。
//!
//! 拦截顺序：弱网（延迟/丢包）→ 断点（请求）→ 本地映射 → 上游；
//! 响应侧：断点（响应）→ 弱网限速 → 下发。
//! 断点会挂起等待前端决定，但挂起前不持任何锁，超时或队列满都自动放行，
//! 保证前端卡住时不会把代理拖死。
//!
//! body 会做两层限制：单条最多保留 2 MB，超过只存前缀并标记 truncated；
//! 另外只对文本、表单、JSON、XML 与 1 MB 内的图片做完整读取，避免大文件/SSE
//! 被整体缓冲。请求体同理，超过上限时只转发不记录；这种情况在断点里只能改
//! 方法、URL 和头，正文保持原样。

use crate::capture::intercept::{
    guess_content_type, is_hop_by_hop, resolve_remote_target, BreakpointRule, Decision,
    InterceptState, MapLocalRule, MapSource, PendingInfo, PendingPhase, RequestEdit, ResponseEdit,
    MAX_MAP_FILE,
};
use crate::capture::store::{BodyPayload, CaptureStore, FlowBrief, FlowRecord, HeaderEntry};
use crate::capture::CaptureConfig;
use http_body_util::BodyExt;
use hudsucker::hyper::body::Bytes;
use hudsucker::hyper::header::{
    HeaderMap, HeaderName, HeaderValue, CONTENT_ENCODING, CONTENT_LENGTH, CONTENT_TYPE, HOST,
    TRANSFER_ENCODING,
};
use hudsucker::hyper::{Method, Request, Response, StatusCode, Uri, Version};
use hudsucker::hyper_util::client::legacy::Error as ClientError;
use hudsucker::{decode_response, Body, HttpContext, HttpHandler, RequestOrResponse};
use parking_lot::Mutex;
use std::future::Future;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// 单条 body 最多保留 2 MB（超出部分丢弃但请求/响应照常完整转发）。
const MAX_STORED_BODY: usize = 2 * 1024 * 1024;
/// 超过这个大小的文本响应不缓冲，避免把大文件整个读进内存。
const MAX_COLLECT_BODY: u64 = 8 * 1024 * 1024;
/// 图片只预览 1 MB 以内的，超过就当普通二进制跳过。
const MAX_IMAGE_BODY: u64 = 1024 * 1024;

/// 静态资源扩展名，`captureStatic=false` 时跳过。
const STATIC_EXTENSIONS: &[&str] = &[
    ".js", ".mjs", ".css", ".map", ".png", ".jpg", ".jpeg", ".gif", ".webp", ".avif", ".svg",
    ".ico", ".woff", ".woff2", ".ttf", ".otf", ".eot", ".mp4", ".webm", ".mp3", ".wav",
];

struct Pending {
    id: u64,
    started: Instant,
    started_at: i64,
    method: String,
    scheme: String,
    host: String,
    path: String,
    /// 当前完整 URL，改包后会同步，用于断点 / 弱网的二次匹配
    url: String,
    http_version: String,
    request_headers: Vec<HeaderEntry>,
    request_size: u64,
    request_body: BodyPayload,
    /// 已缓冲的请求体字节，断点改包时用它重建请求
    body_bytes: Option<Bytes>,
    /// 请求体是未压缩的纯文本，允许在断点里编辑
    request_editable: bool,
    client_addr: String,
    /// 由本地映射直接应答，没有走上游
    mapped: bool,
    /// 请求或响应被断点改过
    modified: bool,
    /// 远程映射真正发往的上游地址
    remote: Option<String>,
}

pub struct CaptureHandler {
    store: Arc<CaptureStore>,
    config: CaptureConfig,
    intercept: Arc<InterceptState>,
    /// 当前请求的上下文；请求体异步读取，因此需要内部可变。
    pending: Arc<Mutex<Option<Pending>>>,
}

impl Clone for CaptureHandler {
    fn clone(&self) -> Self {
        // 每个请求使用独立槽位，不能共享 pending
        Self {
            store: self.store.clone(),
            config: self.config.clone(),
            intercept: self.intercept.clone(),
            pending: Arc::new(Mutex::new(None)),
        }
    }
}

impl CaptureHandler {
    pub fn new(
        store: Arc<CaptureStore>,
        config: CaptureConfig,
        intercept: Arc<InterceptState>,
    ) -> Self {
        Self {
            store,
            config,
            intercept,
            pending: Arc::new(Mutex::new(None)),
        }
    }

    fn should_capture(&self, method: &Method, uri: &Uri) -> bool {
        if *method == Method::CONNECT {
            return false;
        }
        let path = uri.path();
        if !self.config.capture_static && looks_static(path) {
            return false;
        }
        let host = uri.host().unwrap_or_default();
        self.config
            .ignore_patterns
            .iter()
            .map(|pattern| pattern.trim())
            .filter(|pattern| !pattern.is_empty())
            .all(|pattern| !host.contains(pattern) && !path.contains(pattern))
    }
}

impl HttpHandler for CaptureHandler {
    fn handle_request(
        &mut self,
        ctx: &HttpContext,
        req: Request<Body>,
    ) -> impl Future<Output = RequestOrResponse> + Send {
        let matched = self.should_capture(req.method(), req.uri());
        let eligible = *req.method() != Method::CONNECT;
        let url = request_target(&req);
        let method = req.method().to_string();
        let breakpoint = eligible
            .then(|| {
                self.intercept
                    .match_breakpoint(&url, &method, PendingPhase::Request)
            })
            .flatten();
        // 规则命中时即使被静态资源 / 忽略规则过滤，也要录下来，否则用户看不见效果
        let map_rule = eligible
            .then(|| self.intercept.match_map(&url, &method))
            .flatten();
        let remote_rule = eligible
            .then(|| self.intercept.match_remote(&url, &method))
            .flatten();
        let weak_plan = eligible
            .then(|| self.intercept.weak_request_plan(&url))
            .flatten();
        let record = matched || breakpoint.is_some() || map_rule.is_some() || remote_rule.is_some();
        let collect_body = record && plan_request_body(req.method(), req.headers());

        let pending = record.then(|| build_pending(&self.store, ctx, &req, url));
        let pending_slot = self.pending.clone();
        let store = self.store.clone();
        let intercept = self.intercept.clone();

        async move {
            let mut pending = pending;

            // 弱网（请求阶段）：先延迟，再按丢包率直接判死
            if let Some(plan) = weak_plan {
                sleep_for(plan.delay).await;
                if plan.drop {
                    match pending.take() {
                        Some(entry) => {
                            record_error(&store, entry, "弱网模拟：请求被丢弃（504）".to_string())
                        }
                        None => {
                            pending_slot.lock().take();
                        }
                    }
                    return RequestOrResponse::Response(gateway_timeout());
                }
            }

            // 请求体只读一次：录制要展示、断点要改、本地映射要把连接读干净
            let mut req = req;
            if collect_body {
                match collect_request_body(req, &mut pending, &store).await {
                    Ok(value) => req = value,
                    Err(response) => return RequestOrResponse::Response(*response),
                }
            }

            // 断点（请求阶段）
            if let Some(rule) = breakpoint {
                if let Some(entry) = pending.as_ref() {
                    let info = request_pause_info(entry, &rule, intercept.next_pause_id());
                    match intercept.pause(info).await {
                        Decision::Abort => {
                            if let Some(entry) = pending.take() {
                                record_error(
                                    &store,
                                    entry,
                                    "断点中止：请求没有发往上游".to_string(),
                                );
                            }
                            return RequestOrResponse::Response(abort_response());
                        }
                        Decision::Request(edit) => {
                            if let Some(entry) = pending.as_mut() {
                                req = apply_request_edit(entry, edit, req);
                            }
                        }
                        _ => {}
                    }
                }
            }

            // 远程映射：把请求改发到规则里的目标地址；本地映射优先级更高，两者不叠加
            if map_rule.is_none() {
                if let Some(rule) = remote_rule.as_ref() {
                    let original = pending
                        .as_ref()
                        .map(|entry| entry.url.clone())
                        .unwrap_or_default();
                    let outcome = match resolve_remote_target(rule, &original) {
                        Some(target) => rewrite_target(req, &target).map(|req| (req, target)),
                        None => Err("规则没有填写目标地址".to_string()),
                    };
                    match outcome {
                        Ok((rewritten, target)) => {
                            req = rewritten;
                            if let Some(entry) = pending.as_mut() {
                                entry.remote = Some(target);
                            }
                        }
                        Err(error) => {
                            if let Some(entry) = pending.take() {
                                record_error(&store, entry, format!("远程映射失败：{error}"));
                            }
                            return RequestOrResponse::Response(bad_gateway());
                        }
                    }
                }
            }

            // 本地映射：直接用本地内容应答，不上游
            if let Some(rule) = map_rule {
                let served = match serve_local(&rule).await {
                    Ok(served) => served,
                    Err(error) => {
                        if let Some(entry) = pending.take() {
                            record_error(&store, entry, format!("本地映射失败：{error}"));
                        }
                        return RequestOrResponse::Response(bad_gateway());
                    }
                };
                if let Some(mut entry) = pending.take() {
                    entry.mapped = true;
                    record_success(
                        &store,
                        entry,
                        served.status,
                        served.headers,
                        served.body,
                        served.size,
                    );
                }
                return RequestOrResponse::Response(served.response);
            }

            match pending {
                Some(entry) => *pending_slot.lock() = Some(entry),
                None => {
                    pending_slot.lock().take();
                }
            }
            RequestOrResponse::Request(req)
        }
    }

    fn handle_response(
        &mut self,
        _ctx: &HttpContext,
        res: Response<Body>,
    ) -> impl Future<Output = Response<Body>> + Send {
        let pending_slot = self.pending.clone();
        let store = self.store.clone();
        let intercept = self.intercept.clone();

        async move {
            let Some(mut pending) = pending_slot.lock().take() else {
                return res;
            };

            let mut status = res.status().as_u16();
            let mut res = res;

            // 流式（SSE）或超大响应不缓冲正文，断点仍可改状态码和响应头
            if !plan_response_body(res.headers()) {
                if let Some(rule) = intercept.match_breakpoint(
                    &pending.url,
                    &pending.method,
                    PendingPhase::Response,
                ) {
                    let info = response_pause_info(
                        &pending,
                        &rule,
                        Some(status),
                        headers(res.headers()),
                        BodyPayload {
                            size: content_length(res.headers()),
                            content_type: content_type(res.headers()),
                            ..BodyPayload::default()
                        },
                        false,
                        intercept.next_pause_id(),
                    );
                    match intercept.pause(info).await {
                        Decision::Abort => {
                            record_error(&store, pending, "断点中止：响应没有下发".to_string());
                            return abort_response();
                        }
                        Decision::Response(edit) => {
                            let (mut parts, body) = res.into_parts();
                            pending.modified = true;
                            status = apply_head(&mut parts.status, &mut parts.headers, &edit);
                            record_success(
                                &store,
                                pending,
                                status,
                                headers(&parts.headers),
                                BodyPayload {
                                    content_type: content_type(&parts.headers),
                                    ..BodyPayload::default()
                                },
                                content_length(&parts.headers),
                            );
                            return Response::from_parts(parts, body);
                        }
                        _ => {}
                    }
                }
                record_success(
                    &store,
                    pending,
                    status,
                    headers(res.headers()),
                    BodyPayload {
                        content_type: content_type(res.headers()),
                        ..BodyPayload::default()
                    },
                    content_length(res.headers()),
                );
                return res;
            }

            // 先按原始头判断是否需要解码；支持 gzip / deflate / br / zstd
            let encoded = res.headers().contains_key(CONTENT_ENCODING);
            let decoded = encoded && can_decode(res.headers());
            if decoded {
                match decode_response(res) {
                    Ok(value) => res = value,
                    Err(error) => {
                        record_error(&store, pending, format!("解码响应失败：{error}"));
                        return bad_gateway();
                    }
                }
            }

            let response_content_type = content_type(res.headers());
            let (mut parts, body) = res.into_parts();
            let mut bytes = match body.collect().await {
                Ok(collected) => collected.to_bytes(),
                Err(error) => {
                    record_error(&store, pending, format!("读取响应体失败：{error}"));
                    return bad_gateway();
                }
            };
            // 解过码的字节就是明文；没解码的压缩内容不能按 UTF-8 解释
            let plain = !encoded || decoded;
            let mut response_body = build_body(&bytes, &response_content_type, plain);
            let response_editable =
                plain && response_body.text.is_some() && !response_body.truncated;
            status = parts.status.as_u16();

            if let Some(rule) =
                intercept.match_breakpoint(&pending.url, &pending.method, PendingPhase::Response)
            {
                let info = response_pause_info(
                    &pending,
                    &rule,
                    Some(status),
                    headers(&parts.headers),
                    response_body.clone(),
                    response_editable,
                    intercept.next_pause_id(),
                );
                match intercept.pause(info).await {
                    Decision::Abort => {
                        record_error(&store, pending, "断点中止：响应没有下发".to_string());
                        return abort_response();
                    }
                    Decision::Response(edit) => {
                        pending.modified = true;
                        status = apply_head(&mut parts.status, &mut parts.headers, &edit);
                        if let Some(text) = edit.body.filter(|_| response_editable) {
                            let edited_type = parts
                                .headers
                                .get(CONTENT_TYPE)
                                .and_then(|value| value.to_str().ok())
                                .map(|value| {
                                    value
                                        .split(';')
                                        .next()
                                        .unwrap_or(value)
                                        .trim()
                                        .to_ascii_lowercase()
                                })
                                .filter(|value| !value.is_empty())
                                .unwrap_or_else(|| guess_content_type(&text, false));
                            bytes = Bytes::from(text.into_bytes());
                            if !parts.headers.contains_key(CONTENT_TYPE) {
                                if let Ok(value) = HeaderValue::from_str(&edited_type) {
                                    parts.headers.insert(CONTENT_TYPE, value);
                                }
                            }
                            response_body = build_body(&bytes, &edited_type, true);
                        }
                    }
                    _ => {}
                }
            }

            // 长度与分块方式由本代理重写，保证与最终下发的字节一致
            parts.headers.remove(CONTENT_LENGTH);
            parts.headers.remove(TRANSFER_ENCODING);
            if let Ok(value) = HeaderValue::from_str(&bytes.len().to_string()) {
                parts.headers.insert(CONTENT_LENGTH, value);
            }

            // 弱网（响应阶段）：按整体大小把带宽折算成一段等待
            if let Some(delay) = intercept.weak_response_delay(&pending.url, bytes.len() as u64) {
                sleep_for(delay).await;
            }

            record_success(
                &store,
                pending,
                status,
                headers(&parts.headers),
                response_body,
                bytes.len() as u64,
            );
            Response::from_parts(parts, Body::from(bytes))
        }
    }

    fn handle_error(
        &mut self,
        _ctx: &HttpContext,
        err: ClientError,
    ) -> impl Future<Output = Response<Body>> + Send {
        let pending_slot = self.pending.clone();
        let store = self.store.clone();

        async move {
            if let Some(pending) = pending_slot.lock().take() {
                record_error(&store, pending, err.to_string());
            }
            bad_gateway()
        }
    }
}

fn build_pending(
    store: &CaptureStore,
    ctx: &HttpContext,
    req: &Request<Body>,
    url: String,
) -> Pending {
    let uri = req.uri();
    // 用 authority 而不是 host：本地开发经常同域名不同端口，分组时要能区分
    Pending {
        id: store.next_id(),
        started: Instant::now(),
        started_at: crate::util::now_ms(),
        method: req.method().to_string(),
        scheme: uri.scheme_str().unwrap_or("http").to_string(),
        host: uri
            .authority()
            .map(|value| value.as_str().to_string())
            .or_else(|| header_text(req.headers(), "host"))
            .unwrap_or_default(),
        path: uri
            .path_and_query()
            .map(|value| value.as_str().to_string())
            .unwrap_or_else(|| "/".to_string()),
        url,
        http_version: version_text(req.version()),
        request_headers: headers(req.headers()),
        request_size: content_length(req.headers()),
        request_body: BodyPayload {
            content_type: content_type(req.headers()),
            ..BodyPayload::default()
        },
        body_bytes: None,
        request_editable: false,
        client_addr: ctx.client_addr.to_string(),
        mapped: false,
        modified: false,
        remote: None,
    }
}

/// 用于规则匹配的完整 URL：优先绝对 URI，其次用 Host 头补全。
fn request_target<B>(req: &Request<B>) -> String {
    let uri = req.uri();
    let scheme = uri.scheme_str().unwrap_or("http");
    let host = uri
        .authority()
        .map(|value| value.as_str().to_string())
        .or_else(|| header_text(req.headers(), "host"))
        .unwrap_or_default();
    let path = uri
        .path_and_query()
        .map(|value| value.as_str().to_string())
        .unwrap_or_else(|| "/".to_string());
    format!("{scheme}://{host}{path}")
}

/// 改包后把 Pending 里的展示字段同步成最终发出的请求。
fn sync_target(pending: &mut Pending, req: &Request<Body>) {
    let uri = req.uri();
    pending.method = req.method().to_string();
    pending.scheme = uri.scheme_str().unwrap_or("http").to_string();
    pending.host = uri
        .authority()
        .map(|value| value.as_str().to_string())
        .or_else(|| header_text(req.headers(), "host"))
        .unwrap_or_default();
    pending.path = uri
        .path_and_query()
        .map(|value| value.as_str().to_string())
        .unwrap_or_else(|| "/".to_string());
    pending.url = request_target(req);
    pending.http_version = version_text(req.version());
}

/// 远程映射：只改 URI（协议 / 主机 / 路径），请求体与请求头原样保留。
/// 发往上游前 hudsucker 会按新 URI 重新生成 Host 头，这里同步一份便于展示与调试。
fn rewrite_target(req: Request<Body>, target: &str) -> Result<Request<Body>, String> {
    let uri = target
        .parse::<Uri>()
        .map_err(|error| format!("目标地址无效：{error}"))?;
    if uri.scheme().is_none() || uri.authority().is_none() {
        return Err(format!("目标地址缺少协议或主机：{target}"));
    }
    let (mut parts, body) = req.into_parts();
    if let Some(authority) = uri.authority() {
        if let Ok(value) = HeaderValue::from_str(authority.as_str()) {
            parts.headers.insert(HOST, value);
        }
    }
    parts.uri = uri;
    Ok(Request::from_parts(parts, body))
}

/// 读出请求体（同时更新 Pending 里的展示与可编辑标记），再原样重新组装。
async fn collect_request_body(
    req: Request<Body>,
    pending: &mut Option<Pending>,
    store: &CaptureStore,
) -> Result<Request<Body>, Box<Response<Body>>> {
    let content_type = pending
        .as_ref()
        .map(|entry| entry.request_body.content_type.clone())
        .unwrap_or_default();
    let (mut parts, body) = req.into_parts();
    let bytes = match body.collect().await {
        Ok(collected) => collected.to_bytes(),
        Err(error) => {
            if let Some(entry) = pending.take() {
                record_error(store, entry, format!("读取请求体失败：{error}"));
            }
            return Err(Box::new(bad_gateway()));
        }
    };
    // 压缩过的请求体不能在断点里按文本编辑
    let plain = !parts.headers.contains_key(CONTENT_ENCODING);
    if let Some(entry) = pending.as_mut() {
        entry.request_size = bytes.len() as u64;
        entry.request_body = build_body(&bytes, &content_type, plain);
        entry.request_editable =
            plain && entry.request_body.text.is_some() && !entry.request_body.truncated;
        entry.body_bytes = Some(bytes.clone());
        // 记录客户端实际发出的请求头（长度与分块方式已由本代理重写）
        remove_header(&mut entry.request_headers, "transfer-encoding");
        update_header(
            &mut entry.request_headers,
            "content-length",
            bytes.len().to_string(),
        );
    }
    parts.headers.remove(CONTENT_LENGTH);
    parts.headers.remove(TRANSFER_ENCODING);
    if let Ok(value) = HeaderValue::from_str(&bytes.len().to_string()) {
        parts.headers.insert(CONTENT_LENGTH, value);
    }
    Ok(Request::from_parts(parts, Body::from(bytes)))
}

/// 应用请求断点的改动：方法、URL、头、正文。
fn apply_request_edit(
    pending: &mut Pending,
    edit: RequestEdit,
    req: Request<Body>,
) -> Request<Body> {
    let (mut parts, body) = req.into_parts();

    if let Some(method) = edit
        .method
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if let Ok(value) = Method::from_str(method) {
            parts.method = value;
        }
    }

    if let Some(list) = edit.headers.as_deref() {
        apply_header_list(&mut parts.headers, list);
    }

    // 改 URL 放在改头之后：前端会回传原始 Host，URL 的 authority 必须覆盖它
    if let Some(url) = edit
        .url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if let Ok(uri) = url.parse::<Uri>() {
            if let Some(authority) = uri.authority() {
                if let Ok(value) = HeaderValue::from_str(authority.as_str()) {
                    parts.headers.insert(HOST, value);
                }
                parts.uri = uri;
            }
        }
    }

    let body = if let Some(text) = edit.body.filter(|_| pending.request_editable) {
        let bytes = Bytes::from(text.into_bytes());
        pending.body_bytes = Some(bytes.clone());
        Body::from(bytes)
    } else {
        body
    };

    // 缓冲过才需要重算长度；没缓冲时正文流原样透传
    if let Some(len) = pending.body_bytes.as_ref().map(|bytes| bytes.len()) {
        let content_type = pending.request_body.content_type.clone();
        if let Some(bytes) = pending.body_bytes.as_ref() {
            pending.request_body = build_body(bytes, &content_type, true);
        }
        pending.request_size = len as u64;
        pending.request_editable =
            !pending.request_body.truncated && pending.request_body.text.is_some();
        parts.headers.remove(CONTENT_LENGTH);
        parts.headers.remove(TRANSFER_ENCODING);
        if let Ok(value) = HeaderValue::from_str(&len.to_string()) {
            parts.headers.insert(CONTENT_LENGTH, value);
        }
    }

    let req = Request::from_parts(parts, body);
    sync_target(pending, &req);
    pending.modified = true;
    req
}

/// 断点提交的是完整头列表：列表里没有的非逐跳头会被删掉。
fn apply_header_list(target: &mut HeaderMap, list: &[HeaderEntry]) {
    let mut rebuilt = HeaderMap::new();
    for (name, value) in target.iter() {
        if is_hop_by_hop(name.as_str()) {
            rebuilt.insert(name.clone(), value.clone());
        }
    }
    for header in list {
        let name = header.name.trim();
        if name.is_empty() || is_hop_by_hop(name) {
            continue;
        }
        let (Ok(name), Ok(value)) = (
            HeaderName::from_bytes(name.as_bytes()),
            HeaderValue::from_str(&header.value),
        ) else {
            continue;
        };
        // append 保留 set-cookie 这类多值头
        rebuilt.append(name, value);
    }
    *target = rebuilt;
}

/// 应用响应断点的状态码与响应头改动，返回最终状态码。
fn apply_head(status: &mut StatusCode, map: &mut HeaderMap, edit: &ResponseEdit) -> u16 {
    if let Some(code) = edit.status {
        if let Ok(value) = StatusCode::from_u16(code) {
            *status = value;
        }
    }
    if let Some(list) = edit.headers.as_deref() {
        apply_header_list(map, list);
    }
    status.as_u16()
}

fn request_pause_info(pending: &Pending, rule: &BreakpointRule, pause_id: u64) -> PendingInfo {
    PendingInfo {
        pause_id,
        phase: PendingPhase::Request,
        rule: rule_label(rule),
        method: pending.method.clone(),
        url: pending.url.clone(),
        http_version: pending.http_version.clone(),
        status: None,
        request_headers: pending.request_headers.clone(),
        request_body: pending.request_body.clone(),
        request_editable: pending.request_editable,
        response_headers: Vec::new(),
        response_body: BodyPayload::default(),
        response_editable: false,
        created_at: crate::util::now_ms(),
    }
}

#[allow(clippy::too_many_arguments)]
fn response_pause_info(
    pending: &Pending,
    rule: &BreakpointRule,
    status: Option<u16>,
    response_headers: Vec<HeaderEntry>,
    response_body: BodyPayload,
    response_editable: bool,
    pause_id: u64,
) -> PendingInfo {
    PendingInfo {
        pause_id,
        phase: PendingPhase::Response,
        rule: rule_label(rule),
        method: pending.method.clone(),
        url: pending.url.clone(),
        http_version: pending.http_version.clone(),
        status,
        request_headers: pending.request_headers.clone(),
        request_body: pending.request_body.clone(),
        request_editable: false,
        response_headers,
        response_body,
        response_editable,
        created_at: crate::util::now_ms(),
    }
}

fn rule_label(rule: &BreakpointRule) -> String {
    let name = rule.name.trim();
    if name.is_empty() {
        rule.pattern.clone()
    } else {
        name.to_string()
    }
}

struct ServedLocal {
    response: Response<Body>,
    status: u16,
    headers: Vec<HeaderEntry>,
    body: BodyPayload,
    size: u64,
}

/// 本地映射应答：文件每次命中实时读取，改完文件即可生效。
async fn serve_local(rule: &MapLocalRule) -> Result<ServedLocal, String> {
    let (bytes, content_type) = match rule.source {
        MapSource::Inline => {
            let content_type = if rule.content_type.trim().is_empty() {
                guess_content_type(&rule.body, false)
            } else {
                rule.content_type.clone()
            };
            (Bytes::from(rule.body.clone().into_bytes()), content_type)
        }
        MapSource::File => {
            let path = rule.file_path.trim();
            if path.is_empty() {
                return Err("没有选择本地文件".to_string());
            }
            let metadata = tokio::fs::metadata(path)
                .await
                .map_err(|error| format!("读取 {path} 失败：{error}"))?;
            if metadata.len() > MAX_MAP_FILE {
                return Err(format!(
                    "{path} 超过 {} MB 上限",
                    MAX_MAP_FILE / 1024 / 1024
                ));
            }
            let data = tokio::fs::read(path)
                .await
                .map_err(|error| format!("读取 {path} 失败：{error}"))?;
            let content_type = if rule.content_type.trim().is_empty() {
                guess_content_type(path, true)
            } else {
                rule.content_type.clone()
            };
            (Bytes::from(data), content_type)
        }
    };

    let textual = is_textual(&content_type);
    let body = BodyPayload {
        size: bytes.len() as u64,
        truncated: false,
        binary: !textual,
        text: textual.then(|| String::from_utf8_lossy(&bytes).to_string()),
        base64: None,
        content_type: content_type.clone(),
    };

    let mut map = HeaderMap::new();
    for header in &rule.headers {
        let name = header.name.trim();
        if name.is_empty() || is_hop_by_hop(name) {
            continue;
        }
        let (Ok(name), Ok(value)) = (
            HeaderName::from_bytes(name.as_bytes()),
            HeaderValue::from_str(&header.value),
        ) else {
            continue;
        };
        map.append(name, value);
    }
    if let Ok(value) = HeaderValue::from_str(&content_type) {
        map.insert(CONTENT_TYPE, value);
    }
    if let Ok(value) = HeaderValue::from_str(&bytes.len().to_string()) {
        map.insert(CONTENT_LENGTH, value);
    }
    let response_headers = headers(&map);

    let status = StatusCode::from_u16(rule.status).unwrap_or(StatusCode::OK);
    let size = bytes.len() as u64;
    let response = Response::builder()
        .status(status)
        .body(Body::from(bytes))
        .map(|mut value| {
            *value.headers_mut() = map;
            value
        })
        .unwrap_or_else(|_| bad_gateway());

    Ok(ServedLocal {
        response,
        status: status.as_u16(),
        headers: response_headers,
        body,
        size,
    })
}

async fn sleep_for(delay: Duration) {
    if !delay.is_zero() {
        tokio::time::sleep(delay).await;
    }
}

fn record_success(
    store: &CaptureStore,
    pending: Pending,
    status: u16,
    response_headers: Vec<HeaderEntry>,
    response_body: BodyPayload,
    response_size: u64,
) {
    store.record(FlowRecord {
        brief: FlowBrief {
            id: pending.id,
            started_at: pending.started_at,
            method: pending.method,
            scheme: pending.scheme,
            host: pending.host,
            path: pending.path,
            http_version: pending.http_version,
            status: Some(status),
            duration_ms: pending.started.elapsed().as_millis() as i64,
            request_size: pending.request_size,
            response_size,
            content_type: response_body.content_type.clone(),
            client_addr: pending.client_addr,
            error: None,
            mapped: pending.mapped,
            modified: pending.modified,
            remote: pending.remote,
        },
        request_headers: pending.request_headers,
        response_headers,
        request_body: pending.request_body,
        response_body,
    });
}

fn record_error(store: &CaptureStore, pending: Pending, error: String) {
    let content_type = pending.request_body.content_type.clone();
    store.record(FlowRecord {
        brief: FlowBrief {
            id: pending.id,
            started_at: pending.started_at,
            method: pending.method,
            scheme: pending.scheme,
            host: pending.host,
            path: pending.path,
            http_version: pending.http_version,
            status: None,
            duration_ms: pending.started.elapsed().as_millis() as i64,
            request_size: pending.request_size,
            response_size: 0,
            content_type,
            client_addr: pending.client_addr,
            error: Some(error),
            mapped: pending.mapped,
            modified: pending.modified,
            remote: pending.remote,
        },
        request_headers: pending.request_headers,
        response_headers: Vec::new(),
        request_body: pending.request_body,
        response_body: BodyPayload::default(),
    });
}

/// 把字节转成前端可直接展示的 body：
/// 文本给 `text`；图片给 `base64`；其它二进制只给大小。
/// `allow_text=false` 表示内容仍带压缩编码，不能按 UTF-8 解释。
pub(crate) fn build_body(bytes: &Bytes, content_type: &str, allow_text: bool) -> BodyPayload {
    let size = bytes.len() as u64;
    let truncated = bytes.len() > MAX_STORED_BODY;
    let slice = &bytes[..bytes.len().min(MAX_STORED_BODY)];
    let textual = allow_text
        && (is_textual(content_type)
            || (content_type.is_empty() && std::str::from_utf8(slice).is_ok()));

    if textual {
        BodyPayload {
            size,
            truncated,
            binary: false,
            text: Some(String::from_utf8_lossy(slice).to_string()),
            base64: None,
            content_type: content_type.to_string(),
        }
    } else if allow_text && is_image(content_type) && !truncated && size <= MAX_IMAGE_BODY {
        use base64::Engine as _;
        BodyPayload {
            size,
            truncated,
            binary: true,
            text: None,
            base64: Some(base64::engine::general_purpose::STANDARD.encode(bytes)),
            content_type: content_type.to_string(),
        }
    } else {
        BodyPayload {
            size,
            truncated,
            binary: true,
            text: None,
            base64: None,
            content_type: content_type.to_string(),
        }
    }
}

fn plan_request_body(method: &Method, headers: &HeaderMap) -> bool {
    if *method == Method::GET || *method == Method::HEAD {
        return false;
    }
    let len = content_length(headers);
    if len > MAX_COLLECT_BODY {
        return false;
    }
    len > 0 || headers.contains_key(TRANSFER_ENCODING)
}

fn plan_response_body(headers: &HeaderMap) -> bool {
    let content_type = content_type(headers);
    if content_type == "text/event-stream" {
        return false;
    }
    let len = content_length(headers);
    if len > MAX_COLLECT_BODY {
        return false;
    }
    if is_textual(&content_type) {
        return true;
    }
    if is_image(&content_type) {
        return len == 0 || len <= MAX_IMAGE_BODY;
    }
    // 未知类型但体积很小的响应，也读进来看一眼（很多接口不带 content-type）
    len > 0 && len <= 256 * 1024
}

fn can_decode(headers: &HeaderMap) -> bool {
    headers
        .get_all(CONTENT_ENCODING)
        .iter()
        .flat_map(|value| value.to_str().unwrap_or_default().split(','))
        .map(|value| value.trim().to_ascii_lowercase())
        .all(|value| {
            matches!(
                value.as_str(),
                "identity" | "gzip" | "x-gzip" | "deflate" | "br" | "zstd"
            )
        })
}

fn content_type(headers: &HeaderMap) -> String {
    headers
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(|value| {
            value
                .split(';')
                .next()
                .unwrap_or(value)
                .trim()
                .to_ascii_lowercase()
        })
        .unwrap_or_default()
}

fn is_textual(content_type: &str) -> bool {
    content_type.starts_with("text/")
        || content_type.contains("json")
        || content_type.contains("xml")
        || content_type.contains("javascript")
        || content_type.contains("x-www-form-urlencoded")
        || content_type.contains("graphql")
        || content_type.contains("yaml")
        || content_type == "application/x-ndjson"
}

fn is_image(content_type: &str) -> bool {
    content_type.starts_with("image/")
}

fn version_text(version: Version) -> String {
    match version {
        Version::HTTP_09 => "HTTP/0.9".into(),
        Version::HTTP_10 => "HTTP/1.0".into(),
        Version::HTTP_11 => "HTTP/1.1".into(),
        Version::HTTP_2 => "HTTP/2".into(),
        Version::HTTP_3 => "HTTP/3".into(),
        _ => format!("{version:?}"),
    }
}

fn bad_gateway() -> Response<Body> {
    text_response(
        StatusCode::BAD_GATEWAY,
        "AstroMan 抓包代理：上游响应读取失败",
    )
}

/// 弱网丢包：直接当超时处理，前端能明确看出是模拟的。
fn gateway_timeout() -> Response<Body> {
    text_response(
        StatusCode::GATEWAY_TIMEOUT,
        "AstroMan 弱网模拟：请求被丢弃（504）",
    )
}

/// 断点中止：请求/响应没有继续走完链路。
fn abort_response() -> Response<Body> {
    text_response(
        StatusCode::FORBIDDEN,
        "AstroMan 断点中止：该流量没有继续转发",
    )
}

fn text_response(status: StatusCode, text: &str) -> Response<Body> {
    Response::builder()
        .status(status)
        .header(CONTENT_TYPE, "text/plain; charset=utf-8")
        .body(Body::from(text.to_string()))
        .unwrap_or_else(|_| Response::new(Body::empty()))
}

fn header_text(map: &HeaderMap, name: &str) -> Option<String> {
    map.get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
}

fn headers(map: &HeaderMap) -> Vec<HeaderEntry> {
    map.iter()
        .map(|(name, value)| HeaderEntry {
            name: name.as_str().to_string(),
            value: String::from_utf8_lossy(value.as_bytes()).to_string(),
        })
        .collect()
}

fn remove_header(list: &mut Vec<HeaderEntry>, name: &str) {
    list.retain(|header| !header.name.eq_ignore_ascii_case(name));
}

fn update_header(list: &mut Vec<HeaderEntry>, name: &str, value: String) {
    if let Some(header) = list
        .iter_mut()
        .find(|header| header.name.eq_ignore_ascii_case(name))
    {
        header.value = value;
    } else {
        list.push(HeaderEntry {
            name: name.to_string(),
            value,
        });
    }
}

fn content_length(map: &HeaderMap) -> u64 {
    map.get(CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0)
}

fn looks_static(path: &str) -> bool {
    let path = path.split('?').next().unwrap_or(path).to_ascii_lowercase();
    STATIC_EXTENSIONS.iter().any(|ext| path.ends_with(ext))
}
