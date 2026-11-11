//! 只读文档站
//!
//! 技术方案 4.4 / G-03：复用文档导出的 HTML，在后台线程里起一个极简
//! HTTP 服务，给同事一个链接而不是发文件。
//!
//! - 页面在内存里渲染与保存，「刷新」只替换页面内容，不重启服务；
//! - 只处理 `GET` / `HEAD`，其它方法 405，路径穿越直接 403；
//! - 默认只绑 `127.0.0.1`；`0.0.0.0` 视为局域网模式，必须携带一次性令牌；
//! - 依赖只有标准库，避免为了一个只读站点引入额外 Web 框架。

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use chrono::TimeZone;
use parking_lot::{Mutex, RwLock};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::docs::{
    builder,
    model::{DocBundle, DocGroup, DocsOptions, DocsPreparePayload},
    render_html,
};
use crate::util::{err, now_ms};

/// 超过该接口数后按顶级集合拆分为多页，首页变为目录
pub const MAX_SINGLE_PAGE_REQUESTS: usize = 200;
/// 请求头读取上限，超出直接断开
const MAX_HEADER_BYTES: usize = 16 * 1024;
/// 局域网访问令牌的 Cookie 名
const TOKEN_COOKIE: &str = "astroman_docs_token";
/// 局域网访问令牌还可能通过该请求头传递
const TOKEN_HEADER: &str = "x-astroman-docs-token";

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocsSiteStartPayload {
    pub project_id: i64,
    #[serde(default)]
    pub environment_id: Option<i64>,
    /// 为空或 0 时由系统分配空闲端口
    #[serde(default)]
    pub port: Option<u16>,
    /// 仅支持 127.0.0.1 / localhost / 0.0.0.0
    #[serde(default)]
    pub host: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocsSiteStatus {
    pub running: bool,
    pub url: String,
    pub host: String,
    pub port: u16,
    pub lan: bool,
    pub token: Option<String>,
    pub project_id: Option<i64>,
    pub environment_id: Option<i64>,
    pub request_count: usize,
    pub page_count: usize,
    pub started_at: i64,
    pub warnings: Vec<String>,
}

/// 一次渲染的结果：路径 → HTML
struct RenderedSite {
    pages: BTreeMap<String, String>,
    request_count: usize,
    warnings: Vec<String>,
}

struct RunningSite {
    display_host: String,
    port: u16,
    lan: bool,
    token: Option<String>,
    project_id: i64,
    environment_id: Option<i64>,
    request_count: usize,
    page_count: usize,
    started_at: i64,
    warnings: Vec<String>,
    pages: Arc<RwLock<BTreeMap<String, String>>>,
    shutdown: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl RunningSite {
    fn status(&self) -> DocsSiteStatus {
        DocsSiteStatus {
            running: true,
            url: format!("http://{}:{}/", self.display_host, self.port),
            host: self.display_host.clone(),
            port: self.port,
            lan: self.lan,
            token: self.token.clone(),
            project_id: Some(self.project_id),
            environment_id: self.environment_id,
            request_count: self.request_count,
            page_count: self.page_count,
            started_at: self.started_at,
            warnings: self.warnings.clone(),
        }
    }

    fn stop(mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

/// 文档站管理器：进程内最多一个实例
#[derive(Default)]
pub struct DocsSite {
    running: Mutex<Option<RunningSite>>,
}

impl DocsSite {
    pub fn status(&self) -> DocsSiteStatus {
        match self.running.lock().as_ref() {
            Some(site) => site.status(),
            None => DocsSiteStatus::default(),
        }
    }

    /// 启动（或替换）文档站；渲染失败时不会影响正在运行的服务
    pub fn start(
        &self,
        conn: &Connection,
        payload: &DocsSiteStartPayload,
    ) -> Result<DocsSiteStatus, String> {
        let (bind_host, display_host, lan) = resolve_host(payload.host.as_deref())?;
        let rendered = render_site(conn, payload.project_id, payload.environment_id)?;
        let listener = bind_listener(&bind_host, payload.port)?;
        let port = listener.local_addr().map_err(err)?.port();
        let token = if lan {
            Some(Uuid::new_v4().to_string())
        } else {
            None
        };
        let started_at = now_ms();
        let pages = Arc::new(RwLock::new(rendered.pages));
        let shutdown = Arc::new(AtomicBool::new(false));
        let ctx = HandlerCtx {
            pages: pages.clone(),
            token: token.clone(),
            started_at,
        };
        let worker_shutdown = shutdown.clone();
        let handle = thread::Builder::new()
            .name("astroman-docs-site".to_string())
            .spawn(move || serve(listener, ctx, worker_shutdown))
            .map_err(err)?;

        let page_count = pages.read().len();
        let site = RunningSite {
            display_host,
            port,
            lan,
            token,
            project_id: payload.project_id,
            environment_id: payload.environment_id,
            request_count: rendered.request_count,
            page_count,
            started_at,
            warnings: rendered.warnings,
            pages,
            shutdown,
            handle: Some(handle),
        };

        let mut guard = self.running.lock();
        if let Some(previous) = guard.take() {
            previous.stop();
        }
        let status = site.status();
        guard.replace(site);
        Ok(status)
    }

    /// 重新渲染并替换内存页面，服务不重启、端口不变
    pub fn refresh(&self, conn: &Connection) -> Result<DocsSiteStatus, String> {
        let mut guard = self.running.lock();
        let site = guard
            .as_mut()
            .ok_or_else(|| "E_DOCS_SITE_STOPPED: 文档站未运行".to_string())?;
        let rendered = render_site(conn, site.project_id, site.environment_id)?;
        *site.pages.write() = rendered.pages;
        site.request_count = rendered.request_count;
        site.warnings = rendered.warnings;
        site.page_count = site.pages.read().len();
        Ok(site.status())
    }

    /// 停止并等待监听线程退出，返回后端口已释放
    pub fn stop(&self) -> DocsSiteStatus {
        let mut guard = self.running.lock();
        if let Some(site) = guard.take() {
            site.stop();
        }
        DocsSiteStatus::default()
    }
}

/// 解析监听地址：默认只监听本机，`0.0.0.0` 走局域网模式
fn resolve_host(raw: Option<&str>) -> Result<(String, String, bool), String> {
    let host = raw.map(str::trim).unwrap_or("127.0.0.1");
    match host {
        "" | "127.0.0.1" | "localhost" | "::1" => {
            Ok(("127.0.0.1".to_string(), "127.0.0.1".to_string(), false))
        }
        "0.0.0.0" => Ok(("0.0.0.0".to_string(), lan_ip(), true)),
        other => Err(format!(
            "E_DOCS_SITE_HOST: 不支持的监听地址 {other}，仅支持 127.0.0.1 或 0.0.0.0"
        )),
    }
}

/// 取第一个已连接、非回环的 IPv4 地址，供局域网链接展示
fn lan_ip() -> String {
    crate::netinfo::adapters()
        .into_iter()
        .filter(|adapter| adapter.up)
        .flat_map(|adapter| adapter.ipv4)
        .find(|ip| !ip.starts_with("127.") && !ip.starts_with("169.254."))
        .unwrap_or_else(|| "127.0.0.1".to_string())
}

/// 绑定端口：显式端口被占用时向后探测最多 3 个，仍失败给出明确错误
fn bind_listener(host: &str, requested: Option<u16>) -> Result<TcpListener, String> {
    let base = requested.unwrap_or(0);
    if base == 0 {
        return TcpListener::bind((host, 0))
            .map_err(|error| format!("E_DOCS_SITE_PORT: 绑定 {host} 失败：{error}"));
    }
    let mut last = String::new();
    for offset in 0..4u16 {
        let Some(port) = base.checked_add(offset) else {
            break;
        };
        match TcpListener::bind((host, port)) {
            Ok(listener) => return Ok(listener),
            Err(error) => last = format!("{port}: {error}"),
        }
    }
    Err(format!(
        "E_DOCS_SITE_PORT: 从 {base} 起连续 4 个端口均不可用（{last}）"
    ))
}

fn render_site(
    conn: &Connection,
    project_id: i64,
    environment_id: Option<i64>,
) -> Result<RenderedSite, String> {
    let payload = DocsPreparePayload {
        scope: "project".to_string(),
        target_id: project_id,
        environment_id,
        options: DocsOptions::default(),
    };
    let bundle = builder::build(conn, &payload)?;
    let request_count = builder::request_count(&bundle);
    let env_name = bundle.scope.environment_name.clone();
    let groups = bundle.groups.clone();

    let mut pages = BTreeMap::new();
    if request_count <= MAX_SINGLE_PAGE_REQUESTS || groups.len() <= 1 {
        pages.insert("/".to_string(), render_page(&bundle, env_name.as_deref()));
    } else {
        pages.insert("/".to_string(), index_page(&bundle, env_name.as_deref()));
        for group in &groups {
            let mut page = bundle.clone();
            page.groups = vec![group.clone()];
            pages.insert(
                format!("/c/{}.html", group.id),
                render_page(&page, env_name.as_deref()),
            );
        }
    }

    Ok(RenderedSite {
        pages,
        request_count,
        warnings: bundle.warnings.clone(),
    })
}

fn render_page(bundle: &DocBundle, env_name: Option<&str>) -> String {
    inject_banner(&render_html::render(bundle), env_name)
}

/// 顶部固定提示：本文档由 AstroMan 生成 · 时间 · 环境
fn inject_banner(html: &str, env_name: Option<&str>) -> String {
    let banner = format!(
        "<div style=\"position:sticky;top:0;z-index:9999;background:#0f172a;color:#e2e8f0;padding:8px 16px;font-size:12px;text-align:center;letter-spacing:.04em\">{}</div>",
        esc(&banner_text(env_name))
    );
    let Some(start) = html.find("<body") else {
        return format!("{banner}{html}");
    };
    match html[start..].find('>') {
        Some(offset) => {
            let index = start + offset + 1;
            format!("{}{}{}", &html[..index], banner, &html[index..])
        }
        None => format!("{banner}{html}"),
    }
}

fn banner_text(env_name: Option<&str>) -> String {
    let moment = chrono::Local
        .timestamp_millis_opt(now_ms())
        .single()
        .unwrap_or_else(chrono::Local::now);
    let env = env_name
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or("未选择");
    format!(
        "本文档由 AstroMan 生成 · {} · 环境 {} · 只读站点",
        moment.format("%Y-%m-%d %H:%M"),
        env
    )
}

fn esc(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn group_request_count(group: &DocGroup) -> usize {
    group.requests.len()
        + group
            .children
            .iter()
            .map(group_request_count)
            .sum::<usize>()
}

/// 多页模式首页：只做目录，列出每个顶级集合及接口数量
fn index_page(bundle: &DocBundle, env_name: Option<&str>) -> String {
    let mut cards = String::new();
    for group in &bundle.groups {
        let count = group_request_count(group);
        let description = group.description.trim();
        cards.push_str(&format!(
            "<a class=\"card\" href=\"/c/{}.html\"><div class=\"title\">{}</div><div class=\"count\">{} 个接口</div>{}</a>",
            group.id,
            esc(&group.name),
            count,
            if description.is_empty() {
                String::new()
            } else {
                format!("<div class=\"desc\">{}</div>", esc(description))
            }
        ));
    }
    let html = format!(
        "<!doctype html><html lang=\"zh-CN\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>{title}</title><style>body{{margin:0;font-family:-apple-system,'Segoe UI',sans-serif;background:#f8fafc;color:#0f172a}}main{{max-width:860px;margin:0 auto;padding:32px 20px 64px}}h1{{font-size:22px;margin:0 0 6px}}.meta{{color:#64748b;font-size:13px;margin-bottom:24px}}.card{{display:block;padding:14px 16px;margin-bottom:10px;border:1px solid #e2e8f0;border-radius:10px;background:#fff;color:inherit;text-decoration:none}}.card:hover{{border-color:#94a3b8}}.title{{font-weight:600}}.count{{color:#64748b;font-size:12px;margin-top:2px}}.desc{{color:#475569;font-size:13px;margin-top:4px}}</style></head><body><main><h1>{title}</h1><div class=\"meta\">{count} 个接口，按集合拆分为 {pages} 个页面</div>{cards}</main></body></html>",
        title = esc(&bundle.scope.title),
        count = builder::request_count(bundle),
        pages = bundle.groups.len(),
        cards = cards,
    );
    inject_banner(&html, env_name)
}

/// 监听线程共享的只读上下文
struct HandlerCtx {
    pages: Arc<RwLock<BTreeMap<String, String>>>,
    token: Option<String>,
    started_at: i64,
}

fn serve(listener: TcpListener, ctx: HandlerCtx, shutdown: Arc<AtomicBool>) {
    let _ = listener.set_nonblocking(true);
    while !shutdown.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _)) => {
                let _ = stream.set_nonblocking(false);
                let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
                handle_connection(stream, &ctx);
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(20));
            }
            Err(_) => thread::sleep(Duration::from_millis(50)),
        }
    }
}

fn handle_connection(mut stream: TcpStream, ctx: &HandlerCtx) {
    let Some(head) = read_head(&mut stream) else {
        return;
    };
    let Some(request_line) = head.lines().next() else {
        return;
    };
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_ascii_uppercase();
    let target = parts.next().unwrap_or("/");
    let head_only = method == "HEAD";

    if method != "GET" && !head_only {
        let body = "只支持 GET / HEAD";
        let _ = respond(
            &mut stream,
            405,
            "Method Not Allowed",
            body,
            "text/plain; charset=utf-8",
            false,
            &[("Allow", "GET, HEAD".to_string())],
        );
        return;
    }

    let (raw_path, query) = match target.split_once('?') {
        Some((path, query)) => (path, query),
        None => (target, ""),
    };

    let Some(path) = safe_path(raw_path) else {
        let body = "非法路径";
        let _ = respond(
            &mut stream,
            403,
            "Forbidden",
            body,
            "text/plain; charset=utf-8",
            head_only,
            &[],
        );
        return;
    };

    let mut extra: Vec<(&str, String)> = Vec::new();
    if let Some(expected) = ctx.token.as_deref() {
        let supplied = query_param(query, "token")
            .or_else(|| header_value(&head, TOKEN_HEADER))
            .or_else(|| bearer_token(&head))
            .or_else(|| cookie_value(&head, TOKEN_COOKIE));
        if supplied.as_deref() != Some(expected) {
            let body = "需要访问令牌：请在链接后追加 ?token=…";
            let _ = respond(
                &mut stream,
                401,
                "Unauthorized",
                body,
                "text/plain; charset=utf-8",
                head_only,
                &[],
            );
            return;
        }
        extra.push((
            "Set-Cookie",
            format!("{TOKEN_COOKIE}={expected}; Path=/; HttpOnly; SameSite=Lax"),
        ));
    }

    if path == "/health" {
        // 健康检查只在回环模式暴露，局域网模式视为不存在
        if ctx.token.is_some() {
            let body = "页面不存在";
            let _ = respond(
                &mut stream,
                404,
                "Not Found",
                body,
                "text/plain; charset=utf-8",
                head_only,
                &extra,
            );
            return;
        }
        let body = format!(
            "{{\"name\":\"AstroMan\",\"version\":\"{}\",\"startedAt\":{}}}",
            env!("CARGO_PKG_VERSION"),
            ctx.started_at
        );
        let _ = respond(
            &mut stream,
            200,
            "OK",
            &body,
            "application/json; charset=utf-8",
            head_only,
            &extra,
        );
        return;
    }

    let page = ctx.pages.read().get(&path).cloned();
    match page {
        Some(html) => {
            let _ = respond(
                &mut stream,
                200,
                "OK",
                &html,
                "text/html; charset=utf-8",
                head_only,
                &extra,
            );
        }
        None => {
            let body = "页面不存在";
            let _ = respond(
                &mut stream,
                404,
                "Not Found",
                body,
                "text/plain; charset=utf-8",
                head_only,
                &extra,
            );
        }
    }
}

fn read_head(stream: &mut TcpStream) -> Option<String> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 1024];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(size) => {
                buffer.extend_from_slice(&chunk[..size]);
                if buffer.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
                if buffer.len() > MAX_HEADER_BYTES {
                    return None;
                }
            }
            Err(error)
                if error.kind() == std::io::ErrorKind::WouldBlock
                    || error.kind() == std::io::ErrorKind::TimedOut =>
            {
                break
            }
            Err(_) => break,
        }
    }
    if buffer.is_empty() {
        return None;
    }
    Some(String::from_utf8_lossy(&buffer).into_owned())
}

#[allow(clippy::too_many_arguments)]
fn respond(
    stream: &mut TcpStream,
    status: u16,
    reason: &str,
    body: &str,
    content_type: &str,
    head_only: bool,
    extra: &[(&str, String)],
) -> std::io::Result<()> {
    let mut head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n",
        body.len()
    );
    for (name, value) in extra {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes())?;
    if !head_only {
        stream.write_all(body.as_bytes())?;
    }
    let _ = stream.flush();
    let _ = stream.shutdown(Shutdown::Both);
    Ok(())
}

/// 归一化路径；含 `..`、反斜杠或 NUL 的一律视为穿越
fn safe_path(raw: &str) -> Option<String> {
    let decoded = percent_decode(raw)?;
    if decoded.contains('\\') || decoded.contains('\0') {
        return None;
    }
    let mut segments = Vec::new();
    for segment in decoded.split('/') {
        match segment {
            "" | "." => continue,
            ".." => return None,
            other => segments.push(other),
        }
    }
    if segments.is_empty() {
        Some("/".to_string())
    } else {
        Some(format!("/{}", segments.join("/")))
    }
}

fn percent_decode(input: &str) -> Option<String> {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len() {
                return None;
            }
            let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(out).ok()
}

fn query_param(query: &str, name: &str) -> Option<String> {
    for pair in query.split('&') {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        if key == name {
            return Some(percent_decode(value).unwrap_or_default());
        }
    }
    None
}

fn header_value(head: &str, name: &str) -> Option<String> {
    for line in head.lines().skip(1) {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        if key.trim().eq_ignore_ascii_case(name) {
            return Some(value.trim().to_string());
        }
    }
    None
}

fn bearer_token(head: &str) -> Option<String> {
    let raw = header_value(head, "authorization")?;
    let token = raw
        .strip_prefix("Bearer ")
        .or_else(|| raw.strip_prefix("bearer "))?;
    Some(token.trim().to_string())
}

fn cookie_value(head: &str, name: &str) -> Option<String> {
    let raw = header_value(head, "cookie")?;
    for item in raw.split(';') {
        let (key, value) = item.split_once('=').unwrap_or((item, ""));
        if key.trim() == name {
            return Some(value.trim().to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use r2d2_sqlite::SqliteConnectionManager;
    use rusqlite::params;

    fn pool(tag: &str) -> crate::state::DbPool {
        crate::secret::install_for_test();
        let dir = crate::util::test_dir(tag);
        let manager = SqliteConnectionManager::file(dir.join("astroman.db"));
        let pool = r2d2::Pool::builder().max_size(2).build(manager).unwrap();
        db::init(&pool).unwrap();
        pool
    }

    fn seed(conn: &Connection) -> i64 {
        conn.execute(
            "INSERT INTO projects (name, description, color, sort_order, created_at, updated_at) VALUES ('演示项目','',NULL,0,0,0)",
            [],
        )
        .unwrap();
        let project = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO collections (project_id, parent_id, name, sort_order, created_at, updated_at) VALUES (?1,NULL,'用户接口',0,0,0)",
            params![project],
        )
        .unwrap();
        let collection = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO requests (collection_id,name,method,url,headers,params,body_type,body,body_meta,pre_script,post_script,assertions,auth,sort_order,created_at,updated_at)
             VALUES (?1,'查询用户','GET','https://example.com/users','[]','[]','none','','','','','','{\"kind\":\"none\"}',0,0,0)",
            params![collection],
        )
        .unwrap();
        project
    }

    fn http(port: u16, request: &str) -> String {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream.write_all(request.as_bytes()).unwrap();
        stream.shutdown(Shutdown::Write).unwrap();
        let mut text = String::new();
        stream.read_to_string(&mut text).unwrap();
        text
    }

    #[test]
    fn serves_index_head_and_rejects_methods() {
        let pool = pool("docs-site-basic");
        let conn = pool.get().unwrap();
        let project = seed(&conn);
        let site = DocsSite::default();
        let status = site
            .start(
                &conn,
                &DocsSiteStartPayload {
                    project_id: project,
                    environment_id: None,
                    port: Some(0),
                    host: None,
                },
            )
            .unwrap();
        assert!(status.running);
        assert!(!status.lan);
        assert!(status.token.is_none());
        assert!(status.url.starts_with("http://127.0.0.1:"));
        assert!(status.request_count >= 1);

        let response = http(
            status.port,
            "GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
        );
        assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
        assert!(response.contains("本文档由 AstroMan 生成"), "{response}");
        assert!(response.contains("查询用户"), "{response}");

        let head = http(
            status.port,
            "HEAD / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
        );
        assert!(head.starts_with("HTTP/1.1 200 OK"), "{head}");
        assert!(!head.contains("<!doctype html>"), "{head}");

        let post = http(
            status.port,
            "POST / HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\n\r\n",
        );
        assert!(post.starts_with("HTTP/1.1 405"), "{post}");
        assert!(post.contains("Allow: GET, HEAD"), "{post}");

        let health = http(
            status.port,
            "GET /health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
        );
        assert!(health.starts_with("HTTP/1.1 200 OK"), "{health}");
        assert!(health.contains("\"name\":\"AstroMan\""), "{health}");

        site.stop();
    }

    #[test]
    fn unknown_and_traversal_paths_are_rejected() {
        let pool = pool("docs-site-path");
        let conn = pool.get().unwrap();
        let project = seed(&conn);
        let site = DocsSite::default();
        let status = site
            .start(
                &conn,
                &DocsSiteStartPayload {
                    project_id: project,
                    environment_id: None,
                    port: Some(0),
                    host: None,
                },
            )
            .unwrap();

        let unknown = http(
            status.port,
            "GET /missing HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
        );
        assert!(unknown.starts_with("HTTP/1.1 404"), "{unknown}");

        for target in ["/%2e%2e/%2e%2e/etc/passwd", "/..%5cwindows", "/../secret"] {
            let response = http(
                status.port,
                &format!("GET {target} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"),
            );
            assert!(
                response.starts_with("HTTP/1.1 403"),
                "{target} → {response}"
            );
        }

        site.stop();
    }

    #[test]
    fn lan_requires_token_and_refresh_replaces_pages() {
        let pool = pool("docs-site-lan");
        let conn = pool.get().unwrap();
        let project = seed(&conn);
        let site = DocsSite::default();
        let status = site
            .start(
                &conn,
                &DocsSiteStartPayload {
                    project_id: project,
                    environment_id: None,
                    port: Some(0),
                    host: Some("0.0.0.0".to_string()),
                },
            )
            .unwrap();
        assert!(status.lan);
        let token = status.token.clone().expect("局域网模式必须生成令牌");
        assert!(!token.is_empty());

        let denied = http(
            status.port,
            "GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
        );
        assert!(denied.starts_with("HTTP/1.1 401"), "{denied}");

        let allowed = http(
            status.port,
            &format!(
                "GET /?token={token} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"
            ),
        );
        assert!(allowed.starts_with("HTTP/1.1 200"), "{allowed}");
        assert!(
            allowed.contains("Set-Cookie: astroman_docs_token="),
            "{allowed}"
        );

        let cookie = http(
            status.port,
            &format!(
                "GET / HTTP/1.1\r\nHost: localhost\r\nCookie: astroman_docs_token={token}\r\nConnection: close\r\n\r\n"
            ),
        );
        assert!(cookie.starts_with("HTTP/1.1 200"), "{cookie}");

        conn.execute(
            "UPDATE requests SET name = '刷新后的接口'
             WHERE collection_id IN (SELECT id FROM collections WHERE project_id = ?1)",
            params![project],
        )
        .unwrap();
        let payload = DocsPreparePayload {
            scope: "project".to_string(),
            target_id: project,
            environment_id: None,
            options: DocsOptions::default(),
        };
        let bundle = builder::build(&conn, &payload).unwrap();
        let names: Vec<&str> = bundle.groups[0]
            .requests
            .iter()
            .map(|request| request.name.as_str())
            .collect();
        assert_eq!(names, vec!["刷新后的接口"]);
        let refreshed = site.refresh(&conn).unwrap();
        assert!(refreshed.running);
        assert_eq!(refreshed.port, status.port);
        let cached = site
            .running
            .lock()
            .as_ref()
            .unwrap()
            .pages
            .read()
            .get("/")
            .cloned()
            .unwrap();
        assert!(
            cached.contains("刷新后的接口"),
            "{}",
            &cached[..200.min(cached.len())]
        );

        let page = http(
            status.port,
            &format!(
                "GET /?token={token} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"
            ),
        );
        assert!(page.contains("刷新后的接口"), "{page}");

        let port = status.port;
        site.stop();
        assert!(!site.status().running);
        // 停止后端口立即可复用
        let listener = TcpListener::bind(("127.0.0.1", port)).unwrap();
        drop(listener);
    }

    #[test]
    fn rejects_unsupported_host() {
        let pool = pool("docs-site-host");
        let conn = pool.get().unwrap();
        let project = seed(&conn);
        let site = DocsSite::default();
        let error = site
            .start(
                &conn,
                &DocsSiteStartPayload {
                    project_id: project,
                    environment_id: None,
                    port: Some(0),
                    host: Some("192.168.1.10".to_string()),
                },
            )
            .unwrap_err();
        assert!(error.contains("E_DOCS_SITE_HOST"), "{error}");
    }

    #[test]
    fn refuses_refresh_when_stopped() {
        let site = DocsSite::default();
        let pool = pool("docs-site-refresh");
        let conn = pool.get().unwrap();
        let error = site.refresh(&conn).unwrap_err();
        assert!(error.contains("E_DOCS_SITE_STOPPED"), "{error}");
    }
}
