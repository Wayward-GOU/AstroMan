//! 本地 Mock Server（G-04）
//!
//! 技术方案 4.3：从项目接口定义直接起本地 HTTP 服务，前端联调不依赖后端。
//!
//! 实现取舍：**不引入 axum / tower-http**，复用文档站（4.4）已经验证过的
//! 标准库 HTTP 线程。原因是发布门禁要求 NSIS 安装包 ≤ 25MB，Mock 只需要
//! 动态路由 + CORS + 延迟，不值得再背一套 Web 框架；路由与中间件在这里
//! 自己实现，匹配逻辑集中在 `router.rs`，可独立单测。
//!
//! - 默认只监听 `127.0.0.1`；`0.0.0.0` 视为局域网模式，必须带一次性令牌；
//! - 同一项目只允许一个实例，全局最多 3 个；
//! - 请求日志只放内存环形缓冲（500 条），不落库。

use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::util::{err, now_ms};

use super::router::{self, Resolution};

pub const MAX_INSTANCES: usize = 3;
pub const LOG_CAPACITY: usize = 500;
const MAX_HEADER_BYTES: usize = 64 * 1024;
const MAX_BODY_BYTES: usize = 8 * 1024 * 1024;
const TOKEN_HEADER: &str = "x-astroman-mock-token";
const TOKEN_COOKIE: &str = "astroman_mock_token";

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MockStartPayload {
    pub project_id: i64,
    /// 为空或 0 时由系统分配空闲端口
    #[serde(default)]
    pub port: Option<u16>,
    /// 仅支持 127.0.0.1 / localhost / 0.0.0.0
    #[serde(default)]
    pub host: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MockStatus {
    pub running: bool,
    pub project_id: Option<i64>,
    pub url: String,
    pub host: String,
    pub port: u16,
    pub lan: bool,
    pub token: Option<String>,
    pub route_count: usize,
    pub started_at: i64,
    pub requests: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MockLog {
    pub id: u64,
    pub at: i64,
    pub method: String,
    pub path: String,
    pub status: u16,
    pub duration_ms: u64,
    pub source: String,
    pub route_id: Option<i64>,
    pub generated: bool,
    pub error: Option<String>,
}

struct Shared {
    project_id: i64,
    conn: Mutex<Connection>,
    token: Option<String>,
    logs: Mutex<VecDeque<MockLog>>,
    counter: AtomicU64,
    requests: AtomicU64,
    shutdown: Arc<AtomicBool>,
}

struct RunningMock {
    display_host: String,
    port: u16,
    lan: bool,
    token: Option<String>,
    route_count: usize,
    started_at: i64,
    shared: Arc<Shared>,
    handle: Option<JoinHandle<()>>,
}

impl RunningMock {
    fn status(&self) -> MockStatus {
        MockStatus {
            running: true,
            project_id: Some(self.shared.project_id),
            url: format!("http://{}:{}/", self.display_host, self.port),
            host: self.display_host.clone(),
            port: self.port,
            lan: self.lan,
            token: self.token.clone(),
            route_count: self.route_count,
            started_at: self.started_at,
            requests: self.shared.requests.load(Ordering::Relaxed),
        }
    }

    fn stop(mut self) {
        self.shared.shutdown.store(true, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

/// Mock 服务管理器：随 AppState 存活，进程退出时监听线程一并结束
#[derive(Default)]
pub struct MockServer {
    running: Mutex<HashMap<i64, RunningMock>>,
}

impl MockServer {
    pub fn start(&self, db_path: &Path, payload: &MockStartPayload) -> Result<MockStatus, String> {
        let (bind_host, display_host, lan) = resolve_host(payload.host.as_deref())?;
        let mut guard = self.running.lock();
        if let Some(previous) = guard.remove(&payload.project_id) {
            previous.stop();
        }
        if guard.len() >= MAX_INSTANCES {
            return Err(format!(
                "E_MOCK_LIMIT: 最多同时运行 {MAX_INSTANCES} 个 Mock 服务，请先停止其它项目"
            ));
        }

        let conn = open_read_conn(db_path)?;
        let route_count = super::store::enabled_routes(&conn, payload.project_id)?.len();
        if route_count == 0 {
            return Err("E_MOCK_EMPTY: 该项目还没有启用的 Mock 路由，请先生成或新增路由".into());
        }

        let listener = bind_listener(&bind_host, payload.port)?;
        let port = listener.local_addr().map_err(err)?.port();
        let token = if lan {
            Some(Uuid::new_v4().to_string())
        } else {
            None
        };
        let shared = Arc::new(Shared {
            project_id: payload.project_id,
            conn: Mutex::new(conn),
            token: token.clone(),
            logs: Mutex::new(VecDeque::with_capacity(LOG_CAPACITY)),
            counter: AtomicU64::new(1),
            requests: AtomicU64::new(0),
            shutdown: Arc::new(AtomicBool::new(false)),
        });

        let worker = shared.clone();
        let handle = thread::Builder::new()
            .name(format!("astroman-mock-{}", payload.project_id))
            .spawn(move || serve(listener, worker))
            .map_err(err)?;

        let item = RunningMock {
            display_host,
            port,
            lan,
            token,
            route_count,
            started_at: now_ms(),
            shared,
            handle: Some(handle),
        };
        log::info!(
            "Mock 服务已启动：项目 {}，端口 {}，{} 条路由{}",
            payload.project_id,
            port,
            route_count,
            if lan { "（局域网）" } else { "" }
        );
        let status = item.status();
        guard.insert(payload.project_id, item);
        Ok(status)
    }

    pub fn stop(&self, project_id: i64) -> MockStatus {
        let mut guard = self.running.lock();
        if let Some(item) = guard.remove(&project_id) {
            item.stop();
        }
        MockStatus::default()
    }

    pub fn status(&self, project_id: i64) -> MockStatus {
        self.running
            .lock()
            .get(&project_id)
            .map(RunningMock::status)
            .unwrap_or_default()
    }

    pub fn statuses(&self) -> Vec<MockStatus> {
        self.running
            .lock()
            .values()
            .map(RunningMock::status)
            .collect()
    }

    pub fn logs(&self, project_id: i64) -> Vec<MockLog> {
        let guard = self.running.lock();
        let Some(item) = guard.get(&project_id) else {
            return Vec::new();
        };
        let logs = item.shared.logs.lock();
        logs.iter().rev().cloned().collect()
    }

    pub fn clear_logs(&self, project_id: i64) {
        let guard = self.running.lock();
        if let Some(item) = guard.get(&project_id) {
            item.shared.logs.lock().clear();
        }
    }

    /// 应用退出前调用：显式停掉所有实例，释放端口
    pub fn stop_all(&self) {
        let mut guard = self.running.lock();
        while let Some((_, item)) = guard.drain().next() {
            item.stop();
        }
    }
}

fn open_read_conn(db_path: &Path) -> Result<Connection, String> {
    let conn =
        Connection::open(db_path).map_err(|error| format!("E_MOCK_DB: 打开数据库失败：{error}"))?;
    // 与主进程共用 WAL：读多写少，busy_timeout 避免瞬时写锁直接失败
    let _ = conn.busy_timeout(Duration::from_secs(5));
    Ok(conn)
}

fn resolve_host(raw: Option<&str>) -> Result<(String, String, bool), String> {
    let host = raw.map(str::trim).unwrap_or("127.0.0.1");
    match host {
        "" | "127.0.0.1" | "localhost" | "::1" => {
            Ok(("127.0.0.1".to_string(), "127.0.0.1".to_string(), false))
        }
        "0.0.0.0" => Ok(("0.0.0.0".to_string(), lan_ip(), true)),
        other => Err(format!(
            "E_MOCK_HOST: 不支持的监听地址 {other}，仅支持 127.0.0.1 或 0.0.0.0"
        )),
    }
}

fn lan_ip() -> String {
    crate::netinfo::adapters()
        .into_iter()
        .filter(|adapter| adapter.up)
        .flat_map(|adapter| adapter.ipv4)
        .find(|ip| !ip.starts_with("127.") && !ip.starts_with("169.254."))
        .unwrap_or_else(|| "127.0.0.1".to_string())
}

/// 端口冲突直接报错，不悄悄换端口：用户按固定端口配置了前端代理
fn bind_listener(host: &str, requested: Option<u16>) -> Result<TcpListener, String> {
    let port = requested.unwrap_or(0);
    TcpListener::bind((host, port))
        .map_err(|error| format!("E_MOCK_PORT: 绑定 {host}:{port} 失败：{error}"))
}

fn serve(listener: TcpListener, shared: Arc<Shared>) {
    let _ = listener.set_nonblocking(true);
    while !shared.shutdown.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _)) => {
                let worker = shared.clone();
                // 每个连接一个短线程：本地联调并发很低，比维护线程池更简单
                let _ = thread::Builder::new()
                    .name("astroman-mock-http".to_string())
                    .spawn(move || handle_connection(stream, worker));
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(20));
            }
            Err(_) => thread::sleep(Duration::from_millis(50)),
        }
    }
    log::info!("Mock 服务监听线程退出：项目 {}", shared.project_id);
}

struct HttpRequest {
    method: String,
    target: String,
    headers: Vec<(String, String)>,
}

fn handle_connection(mut stream: TcpStream, shared: Arc<Shared>) {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(10)));
    let started = Instant::now();

    let Some(request) = read_request(&mut stream) else {
        return;
    };
    let (raw_path, query) = match request.target.split_once('?') {
        Some((path, query)) => (path, query),
        None => (request.target.as_str(), ""),
    };
    let path = raw_path.to_string();

    let cors = cors_headers(&request);
    if request.method.eq_ignore_ascii_case("OPTIONS") {
        let mut headers = cors.clone();
        headers.push((
            "Access-Control-Allow-Methods".into(),
            "GET, POST, PUT, PATCH, DELETE, HEAD, OPTIONS".into(),
        ));
        let requested = header_value(&request, "access-control-request-headers")
            .unwrap_or_else(|| "Content-Type, Authorization".into());
        headers.push(("Access-Control-Allow-Headers".into(), requested));
        headers.push(("Access-Control-Max-Age".into(), "600".into()));
        let _ = write_response(&mut stream, 204, "No Content", &headers, &[]);
        return;
    }

    if let Some(expected) = shared.token.as_deref() {
        let supplied = query_param(query, "token")
            .or_else(|| header_value(&request, TOKEN_HEADER))
            .or_else(|| bearer_token(&request))
            .or_else(|| cookie_value(&request, TOKEN_COOKIE));
        if supplied.as_deref() != Some(expected) {
            let body = "{\"code\":\"MOCK_UNAUTHORIZED\",\"message\":\"需要访问令牌\"}".as_bytes();
            let mut headers = cors.clone();
            headers.push((
                "Content-Type".into(),
                "application/json; charset=utf-8".into(),
            ));
            let _ = write_response(&mut stream, 401, "Unauthorized", &headers, body);
            push_log(
                &shared,
                &request,
                &path,
                401,
                started,
                "auth",
                None,
                false,
                Some("缺少或错误的访问令牌".to_string()),
            );
            return;
        }
    }

    let response = {
        let conn = shared.conn.lock();
        router::resolve(&conn, shared.project_id, &request.method, &path)
    };

    match response {
        Ok(Resolution::Matched { route, response }) => {
            if route.delay_ms > 0 {
                thread::sleep(Duration::from_millis(route.delay_ms as u64));
            }
            let mut headers = response.headers;
            headers.extend(cors);
            headers.push(("X-Mock-Source".into(), response.source.to_string()));
            if let Some(id) = (route.id > 0).then_some(route.id) {
                headers.push(("X-Mock-Route-Id".into(), id.to_string()));
            }
            if response.generated {
                headers.push(("X-Mock-Generated".into(), "1".into()));
            }
            if let Some(expected) = shared.token.as_deref() {
                headers.push((
                    "Set-Cookie".into(),
                    format!("{TOKEN_COOKIE}={expected}; Path=/; HttpOnly; SameSite=Lax"),
                ));
            }
            let _ = write_response(
                &mut stream,
                response.status,
                &response.status_text,
                &headers,
                &response.body,
            );
            shared.requests.fetch_add(1, Ordering::Relaxed);
            push_log(
                &shared,
                &request,
                &path,
                response.status,
                started,
                response.source,
                (route.id > 0).then_some(route.id),
                response.generated,
                None,
            );
        }
        Ok(Resolution::MethodNotAllowed(allowed)) => {
            let body = br#"{"code":"MOCK_METHOD_NOT_ALLOWED"}"#;
            let mut headers = cors;
            headers.push(("Allow".into(), allowed.join(", ")));
            headers.push((
                "Content-Type".into(),
                "application/json; charset=utf-8".into(),
            ));
            let _ = write_response(&mut stream, 405, "Method Not Allowed", &headers, body);
            push_log(
                &shared, &request, &path, 405, started, "route", None, false, None,
            );
        }
        Ok(Resolution::NotFound) => {
            let (status, body) = router::not_found(&path);
            let mut headers = cors;
            headers.push((
                "Content-Type".into(),
                "application/json; charset=utf-8".into(),
            ));
            let _ = write_response(&mut stream, status, "Not Found", &headers, &body);
            push_log(
                &shared, &request, &path, status, started, "route", None, false, None,
            );
        }
        Err(error) => {
            let body = serde_json::json!({ "code": "MOCK_ERROR", "message": error });
            let payload = serde_json::to_vec(&body).unwrap_or_else(|_| b"{}".to_vec());
            let mut headers = cors;
            headers.push((
                "Content-Type".into(),
                "application/json; charset=utf-8".into(),
            ));
            let _ = write_response(
                &mut stream,
                500,
                "Internal Server Error",
                &headers,
                &payload,
            );
            push_log(
                &shared,
                &request,
                &path,
                500,
                started,
                "error",
                None,
                false,
                Some(body["message"].to_string()),
            );
        }
    }
    let _ = stream.shutdown(Shutdown::Both);
}

#[allow(clippy::too_many_arguments)]
fn push_log(
    shared: &Shared,
    request: &HttpRequest,
    path: &str,
    status: u16,
    started: Instant,
    source: &str,
    route_id: Option<i64>,
    generated: bool,
    error: Option<String>,
) {
    let mut logs = shared.logs.lock();
    if logs.len() >= LOG_CAPACITY {
        logs.pop_front();
    }
    logs.push_back(MockLog {
        id: shared.counter.fetch_add(1, Ordering::Relaxed),
        at: now_ms(),
        method: request.method.clone(),
        path: path.to_string(),
        status,
        duration_ms: started.elapsed().as_millis() as u64,
        source: source.to_string(),
        route_id,
        generated,
        error,
    });
}

fn read_request(stream: &mut TcpStream) -> Option<HttpRequest> {
    let mut buffer: Vec<u8> = Vec::with_capacity(2048);
    let mut chunk = [0u8; 2048];
    let header_end = loop {
        let read = stream.read(&mut chunk).ok()?;
        if read == 0 {
            return None;
        }
        buffer.extend_from_slice(&chunk[..read]);
        if let Some(index) = find_bytes(&buffer, b"\r\n\r\n") {
            break index;
        }
        if buffer.len() > MAX_HEADER_BYTES {
            return None;
        }
    };

    let head = String::from_utf8_lossy(&buffer[..header_end]).to_string();
    let mut lines = head.lines();
    let request_line = lines.next()?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next()?.to_ascii_uppercase();
    let target = parts.next()?.to_string();
    let mut headers = Vec::new();
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            headers.push((name.trim().to_ascii_lowercase(), value.trim().to_string()));
        }
    }

    // 本服务不按请求体匹配路由；仍把请求体读掉，避免客户端还没写完就收到 RST
    let mut drained = buffer.len().saturating_sub(header_end + 4);
    let chunked = header_value_raw(&headers, "transfer-encoding")
        .map(|value| value.to_ascii_lowercase().contains("chunked"))
        .unwrap_or(false);
    if chunked {
        let mut raw = buffer[header_end + 4..].to_vec();
        let mut chunk_buffer = [0u8; 4096];
        while !chunked_complete(&raw) {
            let read = stream.read(&mut chunk_buffer).ok()?;
            if read == 0 {
                break;
            }
            raw.extend_from_slice(&chunk_buffer[..read]);
            if raw.len() > MAX_BODY_BYTES {
                break;
            }
        }
    } else {
        let expected = header_value_raw(&headers, "content-length")
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(0)
            .min(MAX_BODY_BYTES);
        while drained < expected {
            let read = stream.read(&mut chunk).ok()?;
            if read == 0 {
                break;
            }
            drained += read;
        }
    }

    // 读完请求体丢弃：本服务不按请求体匹配路由，但要避免客户端因半包收到 RST
    Some(HttpRequest {
        method,
        target,
        headers,
    })
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn chunked_complete(body: &[u8]) -> bool {
    body.ends_with(b"\r\n0\r\n\r\n") || body.ends_with(b"0\r\n\r\n")
}

#[cfg(test)]
fn decode_chunked(raw: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut rest = raw;
    loop {
        let line_end = find_bytes(rest, b"\r\n")?;
        let size_text = String::from_utf8_lossy(&rest[..line_end]);
        let size = usize::from_str_radix(size_text.trim().split(';').next()?, 16).ok()?;
        rest = &rest[line_end + 2..];
        if size == 0 {
            break;
        }
        if rest.len() < size {
            return None;
        }
        out.extend_from_slice(&rest[..size]);
        rest = &rest[size..];
        if rest.starts_with(b"\r\n") {
            rest = &rest[2..];
        }
    }
    Some(out)
}

fn write_response(
    stream: &mut TcpStream,
    status: u16,
    status_text: &str,
    headers: &[(String, String)],
    body: &[u8],
) -> std::io::Result<()> {
    let mut response = format!("HTTP/1.1 {status} {status_text}\r\n");
    let mut has_length = false;
    for (name, value) in headers {
        if name.eq_ignore_ascii_case("content-length") {
            has_length = true;
        }
        response.push_str(name);
        response.push_str(": ");
        response.push_str(value);
        response.push_str("\r\n");
    }
    if !has_length {
        response.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    response.push_str("Connection: close\r\n\r\n");
    stream.write_all(response.as_bytes())?;
    stream.write_all(body)?;
    stream.flush()
}

fn cors_headers(request: &HttpRequest) -> Vec<(String, String)> {
    let origin = header_value(request, "origin").unwrap_or_else(|| "*".into());
    vec![
        ("Access-Control-Allow-Origin".into(), origin),
        ("Access-Control-Allow-Credentials".into(), "true".into()),
        ("Vary".into(), "Origin".into()),
        ("Access-Control-Expose-Headers".into(), "*".into()),
    ]
}

fn header_value(request: &HttpRequest, name: &str) -> Option<String> {
    header_value_raw(&request.headers, name)
}

fn header_value_raw(headers: &[(String, String)], name: &str) -> Option<String> {
    headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.clone())
}

fn query_param(query: &str, name: &str) -> Option<String> {
    for pair in query.split('&') {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        if key == name {
            return Some(percent_decode(value));
        }
    }
    None
}

fn percent_decode(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let Ok(value) = u8::from_str_radix(&raw[index + 1..index + 3], 16) {
                out.push(value);
                index += 3;
                continue;
            }
        }
        if bytes[index] == b'+' {
            out.push(b' ');
        } else {
            out.push(bytes[index]);
        }
        index += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

fn bearer_token(request: &HttpRequest) -> Option<String> {
    header_value(request, "authorization")
        .and_then(|value| value.strip_prefix("Bearer ").map(str::to_string))
}

fn cookie_value(request: &HttpRequest, name: &str) -> Option<String> {
    let cookies = header_value(request, "cookie")?;
    for pair in cookies.split(';') {
        let (key, value) = pair.trim().split_once('=')?;
        if key == name {
            return Some(value.to_string());
        }
    }
    None
}

/// 给 Tauri 命令与应用退出流程复用
pub fn stop_all(manager: &MockServer) {
    manager.stop_all();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunked_body_is_decoded() {
        let raw = b"4\r\nWiki\r\n5\r\npedia\r\n0\r\n\r\n";
        assert_eq!(decode_chunked(raw).unwrap(), b"Wikipedia");
        assert!(chunked_complete(raw));
        assert!(!chunked_complete(b"4\r\nWi"));
    }

    #[test]
    fn query_and_cookie_parsing_are_lenient() {
        assert_eq!(query_param("a=1&token=abc%2D1", "token").unwrap(), "abc-1");
        assert_eq!(
            query_param("a=1&x=hello+world", "x").unwrap(),
            "hello world"
        );
        assert!(query_param("a=1", "token").is_none());
    }

    #[test]
    fn host_resolution_rejects_public_bind() {
        assert!(!resolve_host(Some("127.0.0.1")).unwrap().2);
        assert!(resolve_host(Some("0.0.0.0")).unwrap().2);
        assert!(resolve_host(Some("8.8.8.8")).is_err());
    }

    #[test]
    fn serves_manual_route_end_to_end() {
        use std::io::{Read, Write};

        let dir = crate::util::test_dir("mock-server");
        let db = dir.join("astroman.db");
        let conn = Connection::open(&db).unwrap();
        conn.execute_batch(include_str!("../../db/schema.sql"))
            .unwrap();
        let now = crate::util::now_ms();
        conn.execute(
            "INSERT INTO projects (name, created_at, updated_at) VALUES ('Mock', ?1, ?1)",
            [now],
        )
        .unwrap();
        let project = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO mock_routes
               (project_id, name, method, path, status_code, headers_json, body_type,
                body, delay_ms, enabled, sort_order, updated_at)
             VALUES (?1, 'ping', 'GET', '/ping', 200, '[]', 'json',
                     '{\"ok\":true}', 0, 1, 0, ?2)",
            rusqlite::params![project, now],
        )
        .unwrap();
        drop(conn);

        let manager = MockServer::default();
        let status = manager
            .start(
                &db,
                &MockStartPayload {
                    project_id: project,
                    port: None,
                    host: None,
                },
            )
            .unwrap();
        assert!(status.running);
        assert_eq!(status.route_count, 1);

        let mut stream = TcpStream::connect(("127.0.0.1", status.port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        stream
            .write_all(b"GET /ping HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
            .unwrap();
        let mut response = Vec::new();
        stream.read_to_end(&mut response).unwrap();
        let text = String::from_utf8_lossy(&response);
        assert!(text.starts_with("HTTP/1.1 200"), "{text}");
        assert!(text.contains("{\"ok\":true}"), "{text}");
        assert!(text.contains("X-Mock-Source: manual"), "{text}");

        manager.stop(project);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
