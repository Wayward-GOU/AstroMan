//! 抓包 HTTP handler。
//!
//! hudsucker 对每个连接/请求都会 clone 一份 handler，因此 `pending` 只需要
//! 保存「当前这一条请求」的上下文，不会和同连接上的其它请求串味。
//! `pending` 用独立 Arc 实现内部可变：请求体需要异步读取，读完后才能登记；
//! 手动实现 Clone，保证每个请求拿到的都是全新的槽位。
//!
//! body 会做两层限制：单条最多保留 2 MB，超过只存前缀并标记 truncated；
//! 另外只对文本、表单、JSON、XML 与 1 MB 内的图片做完整读取，避免大文件/SSE
//! 被整体缓冲。请求体同理，超过上限时只转发不记录。

use crate::capture::store::{BodyPayload, CaptureStore, FlowBrief, FlowRecord, HeaderEntry};
use crate::capture::CaptureConfig;
use http_body_util::BodyExt;
use hudsucker::hyper::body::Bytes;
use hudsucker::hyper::header::{
    HeaderMap, CONTENT_ENCODING, CONTENT_LENGTH, CONTENT_TYPE, TRANSFER_ENCODING,
};
use hudsucker::hyper::{Method, Request, Response, StatusCode, Uri, Version};
use hudsucker::hyper_util::client::legacy::Error as ClientError;
use hudsucker::{decode_response, Body, HttpContext, HttpHandler, RequestOrResponse};
use parking_lot::Mutex;
use std::future::Future;
use std::sync::Arc;
use std::time::Instant;

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
    http_version: String,
    request_headers: Vec<HeaderEntry>,
    request_size: u64,
    request_body: BodyPayload,
    client_addr: String,
}

pub struct CaptureHandler {
    store: Arc<CaptureStore>,
    config: CaptureConfig,
    /// 当前请求的上下文；请求体异步读取，因此需要内部可变。
    pending: Arc<Mutex<Option<Pending>>>,
}

impl Clone for CaptureHandler {
    fn clone(&self) -> Self {
        // 每个请求使用独立槽位，不能共享 pending
        Self {
            store: self.store.clone(),
            config: self.config.clone(),
            pending: Arc::new(Mutex::new(None)),
        }
    }
}

impl CaptureHandler {
    pub fn new(store: Arc<CaptureStore>, config: CaptureConfig) -> Self {
        Self {
            store,
            config,
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
        let capture = self.should_capture(req.method(), req.uri());
        let pending_slot = self.pending.clone();
        let store = self.store.clone();

        let pending = if capture {
            let uri = req.uri();
            let method = req.method().to_string();
            let scheme = uri.scheme_str().unwrap_or("http").to_string();
            // 用 authority 而不是 host：本地开发经常同域名不同端口，分组时要能区分
            let host = uri
                .authority()
                .map(|value| value.as_str().to_string())
                .or_else(|| {
                    req.headers()
                        .get("host")
                        .and_then(|value| value.to_str().ok())
                        .map(str::to_string)
                })
                .unwrap_or_default();
            let path = uri
                .path_and_query()
                .map(|value| value.as_str().to_string())
                .unwrap_or_else(|| "/".to_string());
            Some(Pending {
                id: store.next_id(),
                started: Instant::now(),
                started_at: crate::util::now_ms(),
                method,
                scheme,
                host,
                path,
                http_version: version_text(req.version()),
                request_headers: headers(req.headers()),
                request_size: content_length(req.headers()),
                request_body: BodyPayload {
                    content_type: content_type(req.headers()),
                    ..BodyPayload::default()
                },
                client_addr: ctx.client_addr.to_string(),
            })
        } else {
            None
        };
        let capture_body = capture && plan_request_body(req.method(), req.headers());

        async move {
            let Some(mut pending) = pending else {
                pending_slot.lock().take();
                return RequestOrResponse::Request(req);
            };

            let req = if capture_body {
                let content_type = pending.request_body.content_type.clone();
                let (mut parts, body) = req.into_parts();
                match body.collect().await {
                    Ok(collected) => {
                        let bytes = collected.to_bytes();
                        pending.request_size = bytes.len() as u64;
                        pending.request_body = build_body(
                            &bytes,
                            &content_type,
                            !parts.headers.contains_key(CONTENT_ENCODING),
                        );
                        // 已经完整读入，补回长度并重新组装，保证上游收到同样的 body
                        parts.headers.remove(CONTENT_LENGTH);
                        parts.headers.remove(TRANSFER_ENCODING);
                        parts
                            .headers
                            .insert(CONTENT_LENGTH, bytes.len().to_string().parse().unwrap());
                        // 记录客户端实际发出的请求头（长度与分块方式已由本代理重写）
                        remove_header(&mut pending.request_headers, "transfer-encoding");
                        update_header(
                            &mut pending.request_headers,
                            "content-length",
                            bytes.len().to_string(),
                        );
                        Request::from_parts(parts, Body::from(bytes))
                    }
                    Err(error) => {
                        record_error(&store, pending, format!("读取请求体失败：{error}"));
                        return RequestOrResponse::Response(bad_gateway());
                    }
                }
            } else {
                req
            };

            *pending_slot.lock() = Some(pending);
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

        async move {
            let Some(pending) = pending_slot.lock().take() else {
                return res;
            };

            let status = res.status().as_u16();
            let mut response_headers = headers(res.headers());
            let response_content_type = content_type(res.headers());
            let initial_size = content_length(res.headers());
            let mut res = res;

            if !plan_response_body(res.headers()) {
                record_success(
                    &store,
                    pending,
                    status,
                    response_headers,
                    BodyPayload {
                        content_type: response_content_type,
                        ..BodyPayload::default()
                    },
                    initial_size,
                );
                return res;
            }

            // 先按原始头判断是否需要解码；支持 gzip / deflate / br / zstd
            let encoded = res.headers().contains_key(CONTENT_ENCODING);
            let decoded = encoded && can_decode(res.headers());
            if decoded {
                match decode_response(res) {
                    Ok(value) => {
                        res = value;
                        remove_header(&mut response_headers, "content-encoding");
                    }
                    Err(error) => {
                        record_error(&store, pending, format!("解码响应失败：{error}"));
                        return bad_gateway();
                    }
                }
            }

            let (mut parts, body) = res.into_parts();
            let bytes = match body.collect().await {
                Ok(collected) => collected.to_bytes(),
                Err(error) => {
                    record_error(&store, pending, format!("读取响应体失败：{error}"));
                    return bad_gateway();
                }
            };
            let response_body = build_body(&bytes, &response_content_type, !encoded);
            parts.headers.remove(CONTENT_LENGTH);
            parts.headers.remove(TRANSFER_ENCODING);
            parts
                .headers
                .insert(CONTENT_LENGTH, bytes.len().to_string().parse().unwrap());
            // 记录的是客户端最终收到的响应头：长度与分块方式已经由本代理重写
            remove_header(&mut response_headers, "transfer-encoding");
            update_header(
                &mut response_headers,
                "content-length",
                bytes.len().to_string(),
            );

            record_success(
                &store,
                pending,
                status,
                response_headers,
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
    Response::builder()
        .status(StatusCode::BAD_GATEWAY)
        .body(Body::empty())
        .unwrap_or_else(|_| Response::new(Body::empty()))
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
