//! Mock 路由匹配与响应构造（G-04）
//!
//! 响应来源优先级（技术方案 4.3）：
//! 1. `mock_routes` 手工配置（`body_type != auto`）；
//! 2. 该请求最近一条运行记录（成功或失败都保留，原状态码 + 脱敏后的头与体）；
//! 3. 按响应实体 / 请求 Body / 参数生成骨架，并标 `X-Mock-Generated: 1`。
//!
//! 只要内容来自运行记录或实体推断，就必须走脱敏链路；手工配置的 Body 视为
//! 用户主动构造的假数据，不强制脱敏。

use std::sync::OnceLock;

use regex::Regex;
use rusqlite::Connection;

use crate::docs::masking::{self, SecretSet};
use crate::docs::model::MASK;

use super::store::{self, MockRoute, RunResponse};

#[derive(Debug, Clone)]
pub struct MockHttpResponse {
    pub status: u16,
    pub status_text: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub source: &'static str,
    pub generated: bool,
}

#[derive(Debug)]
pub enum Resolution {
    Matched {
        route: Box<MockRoute>,
        response: Box<MockHttpResponse>,
    },
    MethodNotAllowed(Vec<String>),
    NotFound,
}

/// 匹配并构造响应
pub fn resolve(
    conn: &Connection,
    project_id: i64,
    method: &str,
    path: &str,
) -> Result<Resolution, String> {
    let method = method.trim().to_ascii_uppercase();
    let path = store::normalize_path(path);
    let routes = store::enabled_routes(conn, project_id)?;

    let hit = find_route(&routes, &method, &path);
    if let Some(route) = hit {
        let response = build_response(conn, project_id, route)?;
        return Ok(Resolution::Matched {
            route: Box::new(route.clone()),
            response: Box::new(response),
        });
    }

    let mut allowed: Vec<String> = routes
        .iter()
        .filter(|route| path_matches(&path, &route.path))
        .map(|route| route.method.trim().to_ascii_uppercase())
        .filter(|item| !item.is_empty())
        .collect();
    if !allowed.is_empty() {
        allowed.sort();
        allowed.dedup();
        allowed.push("OPTIONS".to_string());
        return Ok(Resolution::MethodNotAllowed(allowed));
    }
    Ok(Resolution::NotFound)
}

fn find_route<'a>(routes: &'a [MockRoute], method: &str, path: &str) -> Option<&'a MockRoute> {
    let same_method = |route: &&MockRoute| route.method.trim().eq_ignore_ascii_case(method);
    // 先精确匹配，再模板匹配；同条件按 sort_order 第一条（列表已排序）
    routes
        .iter()
        .filter(same_method)
        .find(|route| store::normalize_path(&route.path) == path)
        .or_else(|| {
            routes
                .iter()
                .filter(same_method)
                .find(|route| template_matches(path, &route.path))
        })
}

fn path_matches(path: &str, template: &str) -> bool {
    store::normalize_path(template) == path || template_matches(path, template)
}

/// `{id}` / `:id` / `{{id}}` 三种占位写法都转成 `[^/]+`
pub fn template_matches(path: &str, template: &str) -> bool {
    if !template.contains('{') && !template.contains(':') {
        return store::normalize_path(template) == path;
    }
    match compile_template(template) {
        Some(regex) => regex.is_match(path),
        None => false,
    }
}

fn compile_template(template: &str) -> Option<Regex> {
    let template = store::normalize_path(template);
    let mut pattern = String::from("^");
    let chars: Vec<char> = template.chars().collect();
    let mut index = 0;
    while index < chars.len() {
        let current = chars[index];
        if current == '{' && chars.get(index + 1) == Some(&'{') {
            if let Some(offset) = template[index..].find("}}") {
                pattern.push_str("[^/]+");
                index += offset + 2;
                continue;
            }
        }
        if current == '{' {
            if let Some(offset) = template[index..].find('}') {
                let name = &template[index + 1..index + offset];
                // `{id:uuid}` 这类带格式说明的只取名字
                if !name.is_empty()
                    && name
                        .chars()
                        .all(|ch| ch.is_alphanumeric() || ch == '_' || ch == '-')
                {
                    pattern.push_str("[^/]+");
                    index += offset + 1;
                    continue;
                }
            }
        }
        if current == ':' {
            let mut end = index + 1;
            while end < chars.len() && (chars[end].is_alphanumeric() || chars[end] == '_') {
                end += 1;
            }
            // 只有段首的 `:id` 才是占位，避免误伤 `http://`
            let at_segment_start = index == 0 || chars[index - 1] == '/';
            if at_segment_start && end > index + 1 {
                pattern.push_str("[^/]+");
                index = end;
                continue;
            }
        }
        // 正则转义
        if "\\.+*?()|[]{}^$#&-~".contains(current) {
            pattern.push('\\');
        }
        pattern.push(current);
        index += 1;
    }
    pattern.push('$');
    Regex::new(&pattern).ok()
}

fn build_response(
    conn: &Connection,
    project_id: i64,
    route: &MockRoute,
) -> Result<MockHttpResponse, String> {
    let secrets = SecretSet::default();
    let secret_values = store::secret_values(conn, project_id);
    let mut secrets = secrets;
    secrets.extend(secret_values);

    if route.body_type.eq_ignore_ascii_case("auto") {
        if let Some(request_id) = route.request_id {
            if let Some(run) = store::latest_run(conn, request_id) {
                return Ok(replay(run, &secrets));
            }
            if let Some(info) = store::request_info(conn, request_id) {
                let (body, generated) = generated_body(&info);
                return Ok(MockHttpResponse {
                    status: 200,
                    status_text: "OK".to_string(),
                    headers: vec![(
                        "Content-Type".to_string(),
                        "application/json; charset=utf-8".to_string(),
                    )],
                    body,
                    source: "generated",
                    generated,
                });
            }
        }
    }

    let mut headers: Vec<(String, String)> = route
        .headers
        .iter()
        .filter(|item| item.enabled && !item.key.trim().is_empty())
        .map(|item| (item.key.trim().to_string(), item.value.clone()))
        .collect();
    if !headers
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case("content-type"))
    {
        headers.push((
            "Content-Type".to_string(),
            content_type(&route.body_type).to_string(),
        ));
    }
    Ok(MockHttpResponse {
        status: route.status_code.clamp(100, 599) as u16,
        status_text: status_text(route.status_code).to_string(),
        headers,
        body: route.body.as_bytes().to_vec(),
        source: "manual",
        generated: false,
    })
}

fn replay(run: RunResponse, secrets: &SecretSet) -> MockHttpResponse {
    let mut headers = Vec::new();
    for (name, value) in run.headers {
        let lower = name.to_ascii_lowercase();
        // 回放的体已解压 / 长度已变化，保留这些头会让客户端解析失败
        if matches!(
            lower.as_str(),
            "content-length" | "transfer-encoding" | "connection" | "content-encoding"
        ) {
            continue;
        }
        let value = if masking::is_sensitive_name(&name) {
            MASK.to_string()
        } else {
            masking::mask_userinfo(&secrets.mask_text(&value))
        };
        headers.push((name, value));
    }
    if !headers
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case("content-type"))
    {
        headers.push((
            "Content-Type".to_string(),
            "application/json; charset=utf-8".to_string(),
        ));
    }
    MockHttpResponse {
        status: run.status.clamp(100, 599) as u16,
        status_text: if run.status_text.trim().is_empty() {
            status_text(run.status).to_string()
        } else {
            run.status_text
        },
        headers,
        body: mask_body(&run.body, secrets).into_bytes(),
        source: "run",
        generated: false,
    }
}

fn mask_body(body: &str, secrets: &SecretSet) -> String {
    if body.trim().is_empty() {
        return String::new();
    }
    let masked = if serde_json::from_str::<serde_json::Value>(body).is_ok() {
        secrets.mask_json(body)
    } else {
        secrets.mask_text(body)
    };
    mask_contacts(&masked)
}

/// 手机号 / 邮箱与本机无关，按模式统一掩码；运行记录回放不能把生产数据原样带出
fn mask_contacts(text: &str) -> String {
    static EMAIL: OnceLock<Regex> = OnceLock::new();
    static PHONE: OnceLock<Regex> = OnceLock::new();
    let email = EMAIL.get_or_init(|| {
        Regex::new(r"[A-Za-z0-9._%+\-]+@[A-Za-z0-9.\-]+\.[A-Za-z]{2,}").expect("邮箱正则")
    });
    let phone = PHONE.get_or_init(|| Regex::new(r"1[3-9]\d{9}").expect("手机号正则"));
    let text = email.replace_all(text, MASK);
    phone.replace_all(&text, MASK).to_string()
}

fn generated_body(info: &store::MockRequestInfo) -> (Vec<u8>, bool) {
    if let Some(value) = store::skeleton_from_entity(info.response_entity.as_deref()) {
        return (
            serde_json::to_string_pretty(&value)
                .unwrap_or_default()
                .into_bytes(),
            true,
        );
    }
    let body_type = info.body_type.trim().to_ascii_lowercase();
    if (body_type == "json" || body_type == "graphql") && !info.body.trim().is_empty() {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&info.body) {
            return (
                serde_json::to_string_pretty(&value)
                    .unwrap_or_default()
                    .into_bytes(),
                true,
            );
        }
    }
    if matches!(body_type.as_str(), "form" | "multipart") {
        if let Ok(fields) = serde_json::from_str::<Vec<crate::models::KeyValueItem>>(&info.body) {
            let mut map = serde_json::Map::new();
            for field in fields.into_iter().filter(|item| item.enabled) {
                map.insert(field.key, serde_json::Value::String(field.value));
            }
            return (
                serde_json::to_string_pretty(&serde_json::Value::Object(map))
                    .unwrap_or_else(|_| "{}".to_string())
                    .into_bytes(),
                true,
            );
        }
    }
    let mut map = serde_json::Map::new();
    for param in info.params.iter().filter(|item| item.enabled) {
        let value = crate::util::render(&param.value, &std::collections::HashMap::new());
        map.insert(param.key.clone(), serde_json::Value::String(value));
    }
    if map.is_empty() {
        return (b"{}".to_vec(), true);
    }
    (
        serde_json::to_string_pretty(&serde_json::Value::Object(map))
            .unwrap_or_else(|_| "{}".to_string())
            .into_bytes(),
        true,
    )
}

fn content_type(body_type: &str) -> &'static str {
    match body_type.trim().to_ascii_lowercase().as_str() {
        "json" | "auto" => "application/json; charset=utf-8",
        "html" => "text/html; charset=utf-8",
        "xml" => "application/xml; charset=utf-8",
        "text" | "raw" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

pub fn status_text(status: i64) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        204 => "No Content",
        301 => "Moved Permanently",
        302 => "Found",
        304 => "Not Modified",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        413 => "Payload Too Large",
        422 => "Unprocessable Entity",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ => "OK",
    }
}

pub fn not_found(path: &str) -> (u16, Vec<u8>) {
    let body = serde_json::json!({ "code": "MOCK_NOT_FOUND", "path": path });
    (
        404,
        serde_json::to_vec(&body).unwrap_or_else(|_| b"{}".to_vec()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::KeyValueItem;
    use rusqlite::params;
    use serde_json::json;

    fn conn(_tag: &str) -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(include_str!("../../db/schema.sql"))
            .unwrap();
        conn
    }

    fn seed(conn: &Connection) -> (i64, i64) {
        let now = crate::util::now_ms();
        conn.execute(
            "INSERT INTO projects (name, created_at, updated_at) VALUES ('Mock', ?1, ?1)",
            params![now],
        )
        .unwrap();
        let project = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO collections (project_id, name, created_at, updated_at)
             VALUES (?1, 'api', ?2, ?2)",
            params![project, now],
        )
        .unwrap();
        let collection = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO requests
                (collection_id, name, method, url, headers, params, body_type, body,
                 request_entity, response_entity, created_at, updated_at)
             VALUES (?1, '详情', 'GET', 'https://api.example.com/users/{id}', '[]', '[]',
                     'none', '', NULL, NULL, ?2, ?2)",
            params![collection, now],
        )
        .unwrap();
        (project, conn.last_insert_rowid())
    }

    fn insert_route(
        conn: &Connection,
        project: i64,
        request: Option<i64>,
        method: &str,
        path: &str,
    ) {
        let now = crate::util::now_ms();
        conn.execute(
            "INSERT INTO mock_routes
                (project_id, request_id, name, method, path, status_code, headers_json,
                 body_type, body, delay_ms, enabled, sort_order, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 200, '[]', 'auto', '', 0, 1, 0, ?6)",
            params![
                project,
                request,
                format!("{method} {path}"),
                method,
                path,
                now
            ],
        )
        .unwrap();
    }

    #[test]
    fn template_matching_supports_three_placeholder_styles() {
        assert!(template_matches("/users/7", "/users/{id}"));
        assert!(template_matches("/users/7", "/users/:id"));
        assert!(template_matches("/users/7", "/users/{{id}}"));
        assert!(!template_matches("/users/7/orders", "/users/{id}"));
        assert!(template_matches("/users/7/orders", "/users/{id}/orders"));
    }

    #[test]
    fn method_mismatch_returns_405_with_allowed() {
        let conn = conn("mock-router-405");
        let (project, _request) = seed(&conn);
        insert_route(&conn, project, None, "POST", "/users/{id}");
        match resolve(&conn, project, "GET", "/users/7").unwrap() {
            Resolution::MethodNotAllowed(allowed) => {
                assert!(allowed.contains(&"POST".to_string()));
                assert!(allowed.contains(&"OPTIONS".to_string()));
            }
            other => panic!("应返回 405：{other:?}"),
        }
    }

    #[test]
    fn run_record_is_replayed_with_non_2xx_and_masked() {
        let conn = conn("mock-router-run");
        let (project, request) = seed(&conn);
        insert_route(&conn, project, Some(request), "GET", "/users/{id}");
        let now = crate::util::now_ms();
        conn.execute(
            "INSERT INTO request_runs
                (request_id, method, url, response_status, response_status_text,
                 response_headers, response_body, created_at)
             VALUES (?1, 'GET', 'https://api.example.com/users/7', 500, 'Server Error',
                     '{\"Content-Type\":\"application/json\",\"Set-Cookie\":\"sid=abc\",\"Content-Encoding\":\"gzip\"}',
                     '{\"error\":\"boom\",\"email\":\"real@corp.com\",\"phone\":\"13800138000\"}', ?2)",
            params![request, now],
        )
        .unwrap();

        let Resolution::Matched { response, .. } =
            resolve(&conn, project, "GET", "/users/7").unwrap()
        else {
            panic!("应命中路由");
        };
        assert_eq!(response.status, 500);
        assert_eq!(response.source, "run");
        assert!(!response
            .headers
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case("content-encoding")));
        let cookie = response
            .headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("set-cookie"))
            .map(|(_, value)| value.as_str())
            .unwrap();
        assert_eq!(cookie, MASK);
        let body = String::from_utf8(response.body).unwrap();
        assert!(!body.contains("real@corp.com"));
        assert!(!body.contains("13800138000"));
        assert!(body.contains("boom"));
    }

    #[test]
    fn manual_route_wins_and_keeps_custom_headers() {
        let conn = conn("mock-router-manual");
        let (project, _request) = seed(&conn);
        let now = crate::util::now_ms();
        conn.execute(
            "INSERT INTO mock_routes
                (project_id, request_id, name, method, path, status_code, headers_json,
                 body_type, body, delay_ms, enabled, sort_order, updated_at)
             VALUES (?1, NULL, '手写', 'GET', '/users/{id}', 201,
                     '[{\"key\":\"X-Trace\",\"value\":\"1\",\"enabled\":true}]',
                     'json', '{\"ok\":true}', 0, 1, 0, ?2)",
            params![project, now],
        )
        .unwrap();
        let Resolution::Matched { response, .. } =
            resolve(&conn, project, "get", "/users/9").unwrap()
        else {
            panic!("应命中路由");
        };
        assert_eq!(response.status, 201);
        assert_eq!(response.source, "manual");
        assert!(response
            .headers
            .iter()
            .any(|(name, value)| name == "X-Trace" && value == "1"));
        assert!(response
            .headers
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case("content-type")));
    }

    #[test]
    fn generated_skeleton_uses_entity_and_marks_generated() {
        use crate::entity::model::{EntityDoc, EntityNode};
        let conn = conn("mock-router-generated");
        let (project, request) = seed(&conn);
        insert_route(&conn, project, Some(request), "GET", "/users/{id}");
        let root = EntityNode {
            children: vec![EntityNode {
                name: "name".into(),
                type_name: "string".into(),
                required: true,
                ..EntityNode::default()
            }],
            ..EntityNode::new_root("object")
        };
        let entity = EntityDoc::new(root, 0).encode().unwrap();
        conn.execute(
            "UPDATE requests SET response_entity = ?1 WHERE id = ?2",
            params![entity, request],
        )
        .unwrap();

        let Resolution::Matched { response, .. } =
            resolve(&conn, project, "GET", "/users/1").unwrap()
        else {
            panic!("应命中路由");
        };
        assert!(response.generated);
        let body: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
        assert_eq!(body["name"], json!("string"));
    }

    #[test]
    fn unknown_path_is_not_found() {
        let conn = conn("mock-router-404");
        let (project, _request) = seed(&conn);
        assert!(matches!(
            resolve(&conn, project, "GET", "/nope").unwrap(),
            Resolution::NotFound
        ));
        let (status, body) = not_found("/nope");
        assert_eq!(status, 404);
        assert!(String::from_utf8(body).unwrap().contains("MOCK_NOT_FOUND"));
    }

    #[test]
    fn key_value_headers_are_masked_when_sensitive() {
        let mut secrets = SecretSet::default();
        secrets.add("plain-secret-value");
        let headers = [KeyValueItem {
            key: "Authorization".into(),
            value: "Bearer plain-secret-value".into(),
            enabled: true,
        }];
        assert!(masking::is_sensitive_name(&headers[0].key));
    }
}
