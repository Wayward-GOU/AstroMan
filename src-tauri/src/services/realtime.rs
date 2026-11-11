//! SSE / WebSocket 实时调试（G-09）
//!
//! 设计取舍：
//! - 连接生命周期由 `RealtimeManager` 持有，命令层只做 Tauri 适配；
//! - SSE 复用 GET 请求定义，事件逐条推送；
//! - WebSocket 用 tokio-tungstenite，文本 / 二进制消息都推给前端；
//! - 单连接上限 10 MB / 5000 条，超限主动关闭并给出原因；
//! - 事件通过 sink 推送（桌面端接 Tauri event，测试接闭包），不依赖窗口。

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;

use crate::models::KeyValueItem;
use crate::util::{now_ms, render};

/// 单连接累计正文上限
pub const MAX_BYTES: u64 = 10 * 1024 * 1024;
/// 单连接消息条数上限
pub const MAX_MESSAGES: u64 = 5000;
/// 同时保留的会话数（关闭后的会话仍可查询状态，超出后淘汰最旧的已关闭会话）
const MAX_SESSIONS: usize = 20;
const SSE_RECONNECT_LIMIT: u32 = 5;
const SSE_RECONNECT_DELAY: Duration = Duration::from_secs(1);

pub const EVENT_MESSAGE: &str = "realtime:event";
pub const EVENT_STATUS: &str = "realtime:status";

pub type RealtimeSink = Arc<dyn Fn(&str, Value) + Send + Sync>;

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RealtimeConnectPayload {
    /// 前端可指定会话 ID；为空时后端生成
    #[serde(default)]
    pub session_id: Option<String>,
    /// sse | websocket
    pub protocol: String,
    pub url: String,
    #[serde(default)]
    pub headers: Vec<KeyValueItem>,
    #[serde(default)]
    pub env_id: Option<i64>,
    /// SSE 断线自动重连，默认开启
    #[serde(default)]
    pub auto_reconnect: Option<bool>,
    /// WS 心跳间隔（毫秒），0 关闭，默认 30000
    #[serde(default)]
    pub ping_interval_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RealtimeStatus {
    pub session_id: String,
    pub protocol: String,
    /// connecting | open | closed | error
    pub state: String,
    pub url: String,
    pub message_count: u64,
    pub bytes: u64,
    pub last_event_id: Option<String>,
    pub error: Option<String>,
    pub close_reason: Option<String>,
}

impl RealtimeStatus {
    fn new(session_id: String, protocol: String, url: String) -> Self {
        Self {
            session_id,
            protocol,
            state: "connecting".to_string(),
            url,
            message_count: 0,
            bytes: 0,
            last_event_id: None,
            error: None,
            close_reason: None,
        }
    }
}

enum SocketCommand {
    Text(String),
    Binary(Vec<u8>),
    Close,
}

struct Session {
    status: Arc<Mutex<RealtimeStatus>>,
    cancel: Arc<AtomicBool>,
    outbound: Option<mpsc::UnboundedSender<SocketCommand>>,
}

#[derive(Default)]
pub struct RealtimeManager {
    sessions: Mutex<HashMap<String, Session>>,
}

impl RealtimeManager {
    fn register(
        &self,
        session_id: &str,
        protocol: &str,
        url: &str,
        outbound: Option<mpsc::UnboundedSender<SocketCommand>>,
    ) -> (Arc<Mutex<RealtimeStatus>>, Arc<AtomicBool>) {
        let status = Arc::new(Mutex::new(RealtimeStatus::new(
            session_id.to_string(),
            protocol.to_string(),
            crate::logging::redact(url),
        )));
        let cancel = Arc::new(AtomicBool::new(false));
        let mut sessions = self.sessions.lock();
        if sessions.len() >= MAX_SESSIONS {
            let stale = sessions
                .iter()
                .find(|(_, session)| session.status.lock().state != "open")
                .map(|(id, _)| id.clone());
            if let Some(id) = stale {
                sessions.remove(&id);
            }
        }
        sessions.insert(
            session_id.to_string(),
            Session {
                status: status.clone(),
                cancel: cancel.clone(),
                outbound,
            },
        );
        (status, cancel)
    }

    pub fn status(&self, session_id: &str) -> Option<RealtimeStatus> {
        self.sessions
            .lock()
            .get(session_id)
            .map(|session| session.status.lock().clone())
    }

    pub fn close(&self, session_id: &str) -> bool {
        let sessions = self.sessions.lock();
        let Some(session) = sessions.get(session_id) else {
            return false;
        };
        session.cancel.store(true, Ordering::SeqCst);
        if let Some(outbound) = &session.outbound {
            let _ = outbound.send(SocketCommand::Close);
        }
        true
    }

    pub fn send_text(&self, session_id: &str, text: String) -> Result<(), String> {
        let sessions = self.sessions.lock();
        let session = sessions
            .get(session_id)
            .ok_or_else(|| "实时会话不存在".to_string())?;
        if session.status.lock().state != "open" {
            return Err("连接未建立或已关闭".to_string());
        }
        session
            .outbound
            .as_ref()
            .ok_or_else(|| "当前连接不支持发送消息".to_string())?
            .send(SocketCommand::Text(text))
            .map_err(|_| "连接已关闭".to_string())
    }

    pub fn send_binary(&self, session_id: &str, data: Vec<u8>) -> Result<(), String> {
        let sessions = self.sessions.lock();
        let session = sessions
            .get(session_id)
            .ok_or_else(|| "实时会话不存在".to_string())?;
        session
            .outbound
            .as_ref()
            .ok_or_else(|| "当前连接不支持发送消息".to_string())?
            .send(SocketCommand::Binary(data))
            .map_err(|_| "连接已关闭".to_string())
    }
}

fn emit_status(sink: &RealtimeSink, status: &Arc<Mutex<RealtimeStatus>>) {
    let snapshot = status.lock().clone();
    sink(
        EVENT_STATUS,
        serde_json::to_value(snapshot).unwrap_or(Value::Null),
    );
}

fn mark_open(status: &Arc<Mutex<RealtimeStatus>>) {
    status.lock().state = "open".to_string();
}

fn mark_closed(
    sink: &RealtimeSink,
    status: &Arc<Mutex<RealtimeStatus>>,
    state: &str,
    error: Option<String>,
    reason: Option<String>,
) {
    {
        let mut guard = status.lock();
        guard.state = state.to_string();
        if error.is_some() {
            guard.error = error;
        }
        if reason.is_some() {
            guard.close_reason = reason.clone();
        }
    }
    emit_status(sink, status);
    let snapshot = status.lock().clone();
    sink(
        EVENT_MESSAGE,
        json!({
            "sessionId": snapshot.session_id,
            "protocol": snapshot.protocol,
            "type": state,
            "error": snapshot.error,
            "closeReason": snapshot.close_reason,
            "ts": now_ms(),
        }),
    );
}

fn push_message(sink: &RealtimeSink, status: &Arc<Mutex<RealtimeStatus>>, payload: Value) {
    sink(EVENT_MESSAGE, payload);
    {
        let mut guard = status.lock();
        guard.message_count += 1;
    }
    emit_status(sink, status);
}

fn resolved_headers(
    headers: &[KeyValueItem],
    vars: &HashMap<String, String>,
) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for item in headers {
        if !item.enabled {
            continue;
        }
        let key = render(item.key.trim(), vars);
        if key.is_empty() {
            continue;
        }
        out.push((key, render(&item.value, vars)));
    }
    out
}

fn normalize_ws_url(raw: &str) -> String {
    let url = raw.trim();
    if let Some(rest) = url.strip_prefix("https://") {
        format!("wss://{rest}")
    } else if let Some(rest) = url.strip_prefix("http://") {
        format!("ws://{rest}")
    } else if url.starts_with("ws://") || url.starts_with("wss://") {
        url.to_string()
    } else {
        format!("ws://{url}")
    }
}

/// 建立连接。SSE / WebSocket 都在后台任务里读，命令立即返回。
pub async fn connect(
    manager: &RealtimeManager,
    client: reqwest::Client,
    payload: RealtimeConnectPayload,
    vars: HashMap<String, String>,
    sink: RealtimeSink,
) -> Result<RealtimeStatus, String> {
    let protocol = payload.protocol.trim().to_ascii_lowercase();
    if protocol != "sse" && protocol != "websocket" {
        return Err("E_PROTOCOL: 仅支持 sse / websocket".to_string());
    }
    let raw_url = render(payload.url.trim(), &vars);
    if raw_url.trim().is_empty() {
        return Err("URL 不能为空".to_string());
    }
    let session_id = payload
        .session_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let headers = resolved_headers(&payload.headers, &vars);

    if protocol == "sse" {
        let url = if raw_url.contains("://") {
            raw_url.clone()
        } else {
            format!("http://{raw_url}")
        };
        let (status, cancel) = manager.register(&session_id, &protocol, &url, None);
        let snapshot = status.lock().clone();
        let task_status = status.clone();
        let task_cancel = cancel.clone();
        let task_sink = sink.clone();
        let auto_reconnect = payload.auto_reconnect.unwrap_or(true);
        tauri::async_runtime::spawn(async move {
            run_sse(
                client,
                url,
                headers,
                auto_reconnect,
                task_status,
                task_cancel,
                task_sink,
            )
            .await;
        });
        return Ok(snapshot);
    }

    let url = normalize_ws_url(&raw_url);
    let ping = Duration::from_millis(payload.ping_interval_ms.unwrap_or(30_000).min(300_000));
    let (outbound, receiver) = mpsc::unbounded_channel();
    let (status, cancel) = manager.register(&session_id, &protocol, &url, Some(outbound));
    let snapshot = status.lock().clone();
    let task_status = status.clone();
    let task_cancel = cancel.clone();
    let task_sink = sink.clone();
    tauri::async_runtime::spawn(async move {
        run_ws(
            url,
            headers,
            ping,
            receiver,
            task_status,
            task_cancel,
            task_sink,
        )
        .await;
    });
    Ok(snapshot)
}

async fn run_sse(
    client: reqwest::Client,
    url: String,
    headers: Vec<(String, String)>,
    auto_reconnect: bool,
    status: Arc<Mutex<RealtimeStatus>>,
    cancel: Arc<AtomicBool>,
    sink: RealtimeSink,
) {
    let mut attempts = 0u32;
    let mut total_bytes = 0u64;
    loop {
        if cancel.load(Ordering::SeqCst) {
            mark_closed(&sink, &status, "closed", None, Some("用户断开".into()));
            return;
        }
        let last_id = status.lock().last_event_id.clone();
        let mut request = client.get(&url).header("Accept", "text/event-stream");
        for (name, value) in &headers {
            match (
                reqwest::header::HeaderName::from_bytes(name.as_bytes()),
                reqwest::header::HeaderValue::from_str(value),
            ) {
                (Ok(name), Ok(value)) => {
                    request = request.header(name, value);
                }
                _ => {
                    mark_closed(
                        &sink,
                        &status,
                        "error",
                        Some(format!("请求头不合法：{name}")),
                        None,
                    );
                    return;
                }
            }
        }
        if let Some(id) = last_id.as_deref() {
            request = request.header("Last-Event-ID", id);
        }
        let response = match request.send().await {
            Ok(response) => response,
            Err(error) => {
                if attempts < SSE_RECONNECT_LIMIT && auto_reconnect {
                    attempts += 1;
                    emit_reconnect(&sink, &status, attempts, &error.to_string());
                    sleep_or_cancel(&cancel, SSE_RECONNECT_DELAY).await;
                    continue;
                }
                mark_closed(
                    &sink,
                    &status,
                    "error",
                    Some(format!("连接失败：{error}")),
                    None,
                );
                return;
            }
        };
        if !response.status().is_success() {
            mark_closed(
                &sink,
                &status,
                "error",
                Some(format!("SSE 响应状态 {}", response.status())),
                None,
            );
            return;
        }
        mark_open(&status);
        emit_status(&sink, &status);

        let mut parser = SseParser::default();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            if cancel.load(Ordering::SeqCst) {
                mark_closed(&sink, &status, "closed", None, Some("用户断开".into()));
                return;
            }
            let chunk = match chunk {
                Ok(chunk) => chunk,
                Err(error) => {
                    emit_reconnect_note(&sink, &status, &format!("流中断：{error}"));
                    break;
                }
            };
            total_bytes += chunk.len() as u64;
            if total_bytes > MAX_BYTES {
                mark_closed(
                    &sink,
                    &status,
                    "error",
                    Some(format!(
                        "E_LIMIT: 单连接超过 {} MB",
                        MAX_BYTES / 1024 / 1024
                    )),
                    Some("超出流量上限".into()),
                );
                return;
            }
            if status.lock().message_count >= MAX_MESSAGES {
                mark_closed(
                    &sink,
                    &status,
                    "error",
                    Some(format!("E_LIMIT: 单连接超过 {MAX_MESSAGES} 条消息")),
                    Some("超出消息上限".into()),
                );
                return;
            }
            let drained = parser.feed(&chunk, |event| {
                if status.lock().message_count >= MAX_MESSAGES {
                    return;
                }
                if let Some(id) = event.id.as_deref() {
                    status.lock().last_event_id = Some(id.to_string());
                }
                let session_id = status.lock().session_id.clone();
                push_message(
                    &sink,
                    &status,
                    json!({
                        "sessionId": session_id,
                        "protocol": "sse",
                        "type": "message",
                        "event": event.event,
                        "id": event.id,
                        "retry": event.retry,
                        "data": event.data,
                        "ts": now_ms(),
                    }),
                );
            });
            if let Some(reason) = drained {
                mark_closed(
                    &sink,
                    &status,
                    "error",
                    Some(reason),
                    Some("超出消息上限".into()),
                );
                return;
            }
        }
        if cancel.load(Ordering::SeqCst) {
            mark_closed(&sink, &status, "closed", None, Some("用户断开".into()));
            return;
        }
        if auto_reconnect && attempts < SSE_RECONNECT_LIMIT {
            attempts += 1;
            emit_reconnect(&sink, &status, attempts, "流已结束");
            sleep_or_cancel(&cancel, SSE_RECONNECT_DELAY).await;
            continue;
        }
        mark_closed(&sink, &status, "closed", None, Some("服务端结束流".into()));
        return;
    }
}

fn emit_reconnect(
    sink: &RealtimeSink,
    status: &Arc<Mutex<RealtimeStatus>>,
    attempt: u32,
    reason: &str,
) {
    let snapshot = status.lock().clone();
    sink(
        EVENT_MESSAGE,
        json!({
            "sessionId": snapshot.session_id,
            "protocol": "sse",
            "type": "reconnect",
            "attempt": attempt,
            "reason": crate::logging::redact(reason),
            "ts": now_ms(),
        }),
    );
}

fn emit_reconnect_note(sink: &RealtimeSink, status: &Arc<Mutex<RealtimeStatus>>, reason: &str) {
    let snapshot = status.lock().clone();
    sink(
        EVENT_MESSAGE,
        json!({
            "sessionId": snapshot.session_id,
            "protocol": "sse",
            "type": "notice",
            "message": crate::logging::redact(reason),
            "ts": now_ms(),
        }),
    );
}

async fn sleep_or_cancel(cancel: &Arc<AtomicBool>, duration: Duration) {
    let step = Duration::from_millis(100);
    let mut remaining = duration;
    while remaining > Duration::ZERO {
        if cancel.load(Ordering::SeqCst) {
            return;
        }
        let sleep = remaining.min(step);
        tokio::time::sleep(sleep).await;
        remaining = remaining.saturating_sub(sleep);
    }
}

async fn run_ws(
    url: String,
    headers: Vec<(String, String)>,
    ping_interval: Duration,
    mut receiver: mpsc::UnboundedReceiver<SocketCommand>,
    status: Arc<Mutex<RealtimeStatus>>,
    cancel: Arc<AtomicBool>,
    sink: RealtimeSink,
) {
    let mut request = match url.as_str().into_client_request() {
        Ok(request) => request,
        Err(error) => {
            mark_closed(
                &sink,
                &status,
                "error",
                Some(format!("WebSocket 地址不合法：{error}")),
                None,
            );
            return;
        }
    };
    for (name, value) in &headers {
        let name = match reqwest::header::HeaderName::from_bytes(name.as_bytes()) {
            Ok(name) => name,
            Err(_) => {
                mark_closed(
                    &sink,
                    &status,
                    "error",
                    Some(format!("请求头不合法：{name}")),
                    None,
                );
                return;
            }
        };
        let value = match reqwest::header::HeaderValue::from_str(value) {
            Ok(value) => value,
            Err(_) => {
                mark_closed(
                    &sink,
                    &status,
                    "error",
                    Some(format!("请求头值不合法：{name}")),
                    None,
                );
                return;
            }
        };
        request.headers_mut().insert(name, value);
    }

    let socket = match tokio_tungstenite::connect_async(request).await {
        Ok((socket, _)) => socket,
        Err(error) => {
            mark_closed(
                &sink,
                &status,
                "error",
                Some(format!("WebSocket 连接失败：{error}")),
                None,
            );
            return;
        }
    };
    mark_open(&status);
    emit_status(&sink, &status);

    let (mut write, mut read) = socket.split();
    let mut ticker = tokio::time::interval(ping_interval.max(Duration::from_millis(250)));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut poll = tokio::time::interval(Duration::from_millis(200));
    poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    ticker.tick().await;
    poll.tick().await;
    let ping_enabled = ping_interval >= Duration::from_millis(250);

    loop {
        tokio::select! {
            _ = poll.tick() => {
                if cancel.load(Ordering::SeqCst) {
                    let _ = write.send(Message::Close(None)).await;
                    mark_closed(&sink, &status, "closed", None, Some("用户断开".into()));
                    return;
                }
            }
            command = receiver.recv() => {
                match command {
                    Some(SocketCommand::Text(text)) => {
                        if let Err(error) = write.send(Message::text(text.clone())).await {
                            mark_closed(&sink, &status, "error", Some(format!("发送失败：{error}")), None);
                            return;
                        }
                        let snapshot = status.lock().session_id.clone();
                        sink(EVENT_MESSAGE, json!({
                            "sessionId": snapshot,
                            "protocol": "websocket",
                            "type": "sent",
                            "messageType": "text",
                            "data": text,
                            "ts": now_ms(),
                        }));
                    }
                    Some(SocketCommand::Binary(data)) => {
                        if let Err(error) = write.send(Message::binary(data.clone())).await {
                            mark_closed(&sink, &status, "error", Some(format!("发送失败：{error}")), None);
                            return;
                        }
                        let snapshot = status.lock().session_id.clone();
                        sink(EVENT_MESSAGE, json!({
                            "sessionId": snapshot,
                            "protocol": "websocket",
                            "type": "sent",
                            "messageType": "binary",
                            "bytes": data.len(),
                            "data": B64.encode(&data),
                            "ts": now_ms(),
                        }));
                    }
                    Some(SocketCommand::Close) | None => {
                        let _ = write.send(Message::Close(None)).await;
                        mark_closed(&sink, &status, "closed", None, Some("用户断开".into()));
                        return;
                    }
                }
            }
            _ = ticker.tick(), if ping_enabled => {
                if let Err(error) = write.send(Message::Ping(Vec::new().into())).await {
                    mark_closed(&sink, &status, "error", Some(format!("心跳失败：{error}")), None);
                    return;
                }
            }
            message = read.next() => {
                match message {
                    Some(Ok(Message::Text(text))) => {
                        let data = text.to_string();
                        if status.lock().bytes + data.len() as u64 > MAX_BYTES {
                            mark_closed(&sink, &status, "error", Some(format!("E_LIMIT: 单连接超过 {} MB", MAX_BYTES / 1024 / 1024)), Some("超出流量上限".into()));
                            return;
                        }
                        status.lock().bytes += data.len() as u64;
                        push_ws_message(&sink, &status, "text", Value::String(data));
                        if status.lock().message_count >= MAX_MESSAGES {
                            mark_closed(&sink, &status, "error", Some(format!("E_LIMIT: 单连接超过 {MAX_MESSAGES} 条消息")), Some("超出消息上限".into()));
                            return;
                        }
                    }
                    Some(Ok(Message::Binary(data))) => {
                        if status.lock().bytes + data.len() as u64 > MAX_BYTES {
                            mark_closed(&sink, &status, "error", Some(format!("E_LIMIT: 单连接超过 {} MB", MAX_BYTES / 1024 / 1024)), Some("超出流量上限".into()));
                            return;
                        }
                        status.lock().bytes += data.len() as u64;
                        let bytes = data.len();
                        push_ws_message(&sink, &status, "binary", json!({"bytes": bytes, "base64": B64.encode(&data)}));
                        if status.lock().message_count >= MAX_MESSAGES {
                            mark_closed(&sink, &status, "error", Some(format!("E_LIMIT: 单连接超过 {MAX_MESSAGES} 条消息")), Some("超出消息上限".into()));
                            return;
                        }
                    }
                    Some(Ok(Message::Ping(data))) => {
                        let _ = write.send(Message::Pong(data)).await;
                    }
                    Some(Ok(Message::Pong(_))) => {}
                    Some(Ok(Message::Close(frame))) => {
                        let reason = frame
                            .map(|frame| {
                                let text = frame.reason.to_string();
                                if text.is_empty() {
                                    format!("对方关闭（{}）", frame.code)
                                } else {
                                    format!("对方关闭（{}）：{text}", frame.code)
                                }
                            })
                            .unwrap_or_else(|| "对方关闭连接".to_string());
                        mark_closed(&sink, &status, "closed", None, Some(reason));
                        return;
                    }
                    Some(Ok(Message::Frame(_))) => {}
                    Some(Err(error)) => {
                        mark_closed(&sink, &status, "error", Some(format!("连接异常：{error}")), None);
                        return;
                    }
                    None => {
                        mark_closed(&sink, &status, "closed", None, Some("连接已结束".into()));
                        return;
                    }
                }
            }
        }
    }
}

fn push_ws_message(
    sink: &RealtimeSink,
    status: &Arc<Mutex<RealtimeStatus>>,
    message_type: &str,
    data: Value,
) {
    let session_id = status.lock().session_id.clone();
    push_message(
        sink,
        status,
        json!({
            "sessionId": session_id,
            "protocol": "websocket",
            "type": "message",
            "messageType": message_type,
            "data": data,
            "ts": now_ms(),
        }),
    );
}

// ---------------------------------------------------------------------------
// SSE 解析
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Clone, PartialEq)]
pub struct SseEvent {
    pub event: Option<String>,
    pub id: Option<String>,
    pub retry: Option<u64>,
    pub data: String,
}

#[derive(Default)]
struct SseParser {
    buffer: Vec<u8>,
    event: Option<String>,
    id: Option<String>,
    retry: Option<u64>,
    data: Vec<String>,
}

impl SseParser {
    /// 喂入一块字节，逐条派发完整事件。
    ///
    /// 返回 `Some(reason)` 表示触发上限保护，调用方应关闭连接。
    fn feed<F>(&mut self, chunk: &[u8], mut on_event: F) -> Option<String>
    where
        F: FnMut(SseEvent),
    {
        self.buffer.extend_from_slice(chunk);
        loop {
            let Some(pos) = self.buffer.iter().position(|byte| *byte == b'\n') else {
                if self.buffer.len() > MAX_BYTES as usize {
                    return Some(format!(
                        "E_LIMIT: 单连接超过 {} MB",
                        MAX_BYTES / 1024 / 1024
                    ));
                }
                return None;
            };
            let mut line = self.buffer.drain(..=pos).collect::<Vec<u8>>();
            line.pop();
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            let text = String::from_utf8_lossy(&line).to_string();
            if text.is_empty() {
                if let Some(event) = self.take_event() {
                    on_event(event);
                }
                continue;
            }
            if text.starts_with(':') {
                continue;
            }
            let (field, value) = match text.split_once(':') {
                Some((field, value)) => (field, value.strip_prefix(' ').unwrap_or(value)),
                None => (text.as_str(), ""),
            };
            match field {
                "event" => self.event = Some(value.to_string()),
                "id" => self.id = Some(value.to_string()),
                "data" => self.data.push(value.to_string()),
                "retry" => {
                    if let Ok(ms) = value.trim().parse::<u64>() {
                        self.retry = Some(ms);
                    }
                }
                _ => {}
            }
        }
    }

    fn take_event(&mut self) -> Option<SseEvent> {
        if self.data.is_empty() && self.event.is_none() && self.id.is_none() {
            return None;
        }
        Some(SseEvent {
            event: self.event.take(),
            id: self.id.take(),
            retry: self.retry.take(),
            data: std::mem::take(&mut self.data).join("\n"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn parses_events_with_multiline_data() {
        let mut parser = SseParser::default();
        let mut events = Vec::new();
        let raw = b"event: message\nid: 7\ndata: hello\ndata: world\nretry: 1500\n\n";
        parser.feed(raw, |event| events.push(event));
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event.as_deref(), Some("message"));
        assert_eq!(events[0].id.as_deref(), Some("7"));
        assert_eq!(events[0].data, "hello\nworld");
        assert_eq!(events[0].retry, Some(1500));
    }

    #[test]
    fn keeps_partial_lines_between_chunks() {
        let mut parser = SseParser::default();
        let mut events = Vec::new();
        parser.feed(b"event: tick\ndata: 1", |event| events.push(event));
        assert!(events.is_empty());
        parser.feed(b"2\n\n", |event| events.push(event));
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data, "12");
        assert_eq!(events[0].event.as_deref(), Some("tick"));
    }

    #[test]
    fn ignores_comments_and_blank_blocks() {
        let mut parser = SseParser::default();
        let mut events = Vec::new();
        parser.feed(b": keepalive\n\n", |event| events.push(event));
        assert!(events.is_empty());
    }

    #[test]
    fn manager_tracks_lifecycle() {
        let manager = RealtimeManager::default();
        manager.register("s1", "sse", "http://localhost/events", None);
        assert_eq!(manager.status("s1").unwrap().state, "connecting");
        assert!(manager.close("s1"));
        assert!(!manager.close("missing"));
    }

    #[test]
    fn realtime_sink_is_shareable() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink_seen = seen.clone();
        let sink: RealtimeSink = Arc::new(move |event, _| sink_seen.lock().push(event.to_string()));
        sink("x", Value::Null);
        assert_eq!(seen.lock().as_slice(), ["x"]);
    }
}
