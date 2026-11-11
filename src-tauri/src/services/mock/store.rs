//! Mock 路由存储（G-04）
//!
//! 表结构随 `schema.sql` 幂等创建，不升 `DB_VERSION`（技术方案 5.1）。
//! 手工路由与「从项目生成」的路由共用一张表，区别只在 `body_type`：
//! - `auto`：启动时生成，运行时优先回放该请求最近一次运行记录；
//! - `json` / `text` / `html` / `xml` / `raw`：用户在 UI 里写死的响应。

use std::collections::{HashMap, HashSet};

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::entity::schema::schema_from_raw;
use crate::models::KeyValueItem;
use crate::util::{err, now_ms};

pub const BODY_AUTO: &str = "auto";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MockRoute {
    #[serde(default)]
    pub id: i64,
    pub project_id: i64,
    #[serde(default)]
    pub request_id: Option<i64>,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub method: String,
    #[serde(default)]
    pub path: String,
    #[serde(default = "default_status")]
    pub status_code: i64,
    #[serde(default)]
    pub headers: Vec<KeyValueItem>,
    #[serde(default = "default_body_type")]
    pub body_type: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub delay_ms: i64,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub sort_order: i64,
    #[serde(default)]
    pub updated_at: i64,
    /// 只读：与同 method + path 的其它启用路由冲突
    #[serde(default)]
    pub conflict: bool,
}

fn default_status() -> i64 {
    200
}
fn default_body_type() -> String {
    BODY_AUTO.to_string()
}
fn default_enabled() -> bool {
    true
}

impl MockRoute {
    fn from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        let headers_json: String = row.get(7)?;
        Ok(Self {
            id: row.get(0)?,
            project_id: row.get(1)?,
            request_id: row.get(2)?,
            name: row.get(3)?,
            method: row.get(4)?,
            path: row.get(5)?,
            status_code: row.get(6)?,
            headers: serde_json::from_str::<Vec<KeyValueItem>>(&headers_json).unwrap_or_default(),
            body_type: row.get(8)?,
            body: row.get(9)?,
            delay_ms: row.get(10)?,
            enabled: row.get::<_, i64>(11)? != 0,
            sort_order: row.get(12)?,
            updated_at: row.get(13)?,
            conflict: false,
        })
    }
}

/// 列表（含冲突标记）
pub fn list_routes(conn: &Connection, project_id: i64) -> Result<Vec<MockRoute>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, project_id, request_id, name, method, path, status_code,
                    headers_json, body_type, body, delay_ms, enabled, sort_order, updated_at
             FROM mock_routes WHERE project_id = ?1
             ORDER BY sort_order ASC, id ASC",
        )
        .map_err(err)?;
    let mut routes = stmt
        .query_map(params![project_id], MockRoute::from_row)
        .map_err(err)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(err)?;
    mark_conflicts(&mut routes);
    Ok(routes)
}

/// 只取启用路由，供运行时匹配
pub fn enabled_routes(conn: &Connection, project_id: i64) -> Result<Vec<MockRoute>, String> {
    Ok(list_routes(conn, project_id)?
        .into_iter()
        .filter(|route| route.enabled)
        .collect())
}

/// 同 method + 归一化 path 视为冲突，按 sort_order 第一条生效
fn mark_conflicts(routes: &mut [MockRoute]) {
    let mut seen: HashMap<(String, String), usize> = HashMap::new();
    let mut duplicated: HashSet<(String, String)> = HashSet::new();
    for route in routes.iter() {
        if !route.enabled {
            continue;
        }
        let key = (
            route.method.trim().to_ascii_uppercase(),
            normalize_path(&route.path),
        );
        if seen.insert(key.clone(), 1).is_some() {
            duplicated.insert(key);
        }
    }
    for route in routes.iter_mut() {
        let key = (
            route.method.trim().to_ascii_uppercase(),
            normalize_path(&route.path),
        );
        route.conflict = route.enabled && duplicated.contains(&key);
    }
}

pub fn save_route(conn: &Connection, route: &MockRoute) -> Result<MockRoute, String> {
    let method = route.method.trim().to_ascii_uppercase();
    if method.is_empty() {
        return Err("E_MOCK_ROUTE: method 不能为空".to_string());
    }
    let path = normalize_path(&route.path);
    if path.is_empty() || !path.starts_with('/') {
        return Err("E_MOCK_ROUTE: path 必须以 / 开头".to_string());
    }
    let name = if route.name.trim().is_empty() {
        format!("{method} {path}")
    } else {
        route.name.trim().to_string()
    };
    let headers = serde_json::to_string(&route.headers).map_err(err)?;
    let body_type = if route.body_type.trim().is_empty() {
        BODY_AUTO.to_string()
    } else {
        route.body_type.trim().to_string()
    };
    let now = now_ms();

    if route.id > 0 {
        let changed = conn
            .execute(
                "UPDATE mock_routes SET request_id = ?1, name = ?2, method = ?3, path = ?4,
                        status_code = ?5, headers_json = ?6, body_type = ?7, body = ?8,
                        delay_ms = ?9, enabled = ?10, sort_order = ?11, updated_at = ?12
                 WHERE id = ?13",
                params![
                    route.request_id,
                    name,
                    method,
                    path,
                    route.status_code.clamp(100, 599),
                    headers,
                    body_type,
                    route.body,
                    route.delay_ms.clamp(0, 60_000),
                    route.enabled as i64,
                    route.sort_order,
                    now,
                    route.id,
                ],
            )
            .map_err(err)?;
        if changed == 0 {
            return Err("E_MOCK_ROUTE: 路由不存在".to_string());
        }
        get_route(conn, route.id)
    } else {
        conn.execute(
            "INSERT INTO mock_routes
                (project_id, request_id, name, method, path, status_code, headers_json,
                 body_type, body, delay_ms, enabled, sort_order, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                route.project_id,
                route.request_id,
                name,
                method,
                path,
                route.status_code.clamp(100, 599),
                headers,
                body_type,
                route.body,
                route.delay_ms.clamp(0, 60_000),
                route.enabled as i64,
                route.sort_order,
                now,
            ],
        )
        .map_err(err)?;
        get_route(conn, conn.last_insert_rowid())
    }
}

pub fn get_route(conn: &Connection, id: i64) -> Result<MockRoute, String> {
    conn.query_row(
        "SELECT id, project_id, request_id, name, method, path, status_code,
                headers_json, body_type, body, delay_ms, enabled, sort_order, updated_at
         FROM mock_routes WHERE id = ?1",
        params![id],
        MockRoute::from_row,
    )
    .map_err(|error| match error {
        rusqlite::Error::QueryReturnedNoRows => "路由不存在".to_string(),
        other => err(other),
    })
}

pub fn delete_route(conn: &Connection, id: i64) -> Result<(), String> {
    conn.execute("DELETE FROM mock_routes WHERE id = ?1", params![id])
        .map_err(err)?;
    Ok(())
}

pub fn delete_project_routes(conn: &Connection, project_id: i64) -> Result<usize, String> {
    conn.execute(
        "DELETE FROM mock_routes WHERE project_id = ?1",
        params![project_id],
    )
    .map_err(err)
}

/// 从项目接口生成路由：已有 request_id 的不重复生成
pub fn generate_routes(conn: &Connection, project_id: i64) -> Result<usize, String> {
    let mut stmt = conn
        .prepare(
            "SELECT r.id, r.name, r.method, r.url
             FROM requests r
             JOIN collections c ON c.id = r.collection_id
             WHERE c.project_id = ?1
             ORDER BY c.sort_order ASC, c.id ASC, r.sort_order ASC, r.id ASC",
        )
        .map_err(err)?;
    let rows = stmt
        .query_map(params![project_id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })
        .map_err(err)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(err)?;

    let existing: HashSet<i64> = conn
        .prepare(
            "SELECT request_id FROM mock_routes WHERE project_id = ?1 AND request_id IS NOT NULL",
        )
        .map_err(err)?
        .query_map(params![project_id], |row| row.get(0))
        .map_err(err)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(err)?
        .into_iter()
        .collect();

    let now = now_ms();
    let mut created = 0usize;
    for (index, (id, name, method, url)) in rows.into_iter().enumerate() {
        if existing.contains(&id) {
            continue;
        }
        let path = path_from_url(&url);
        let method = method.trim().to_ascii_uppercase();
        conn.execute(
            "INSERT INTO mock_routes
                (project_id, request_id, name, method, path, status_code, headers_json,
                 body_type, body, delay_ms, enabled, sort_order, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 200, '[]', 'auto', '', 0, 1, ?6, ?7)",
            params![project_id, id, name, method, path, index as i64, now],
        )
        .map_err(err)?;
        created += 1;
    }
    Ok(created)
}

/// 归一化路径：补前导 `/`、去掉末尾 `/` 与查询串
pub fn normalize_path(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return "/".to_string();
    }
    let without_query = trimmed.split(['?', '#']).next().unwrap_or(trimmed);
    let mut path = without_query.trim();
    // 用户可能直接粘贴完整 URL
    if let Some(pos) = path.find("://") {
        let rest = &path[pos + 3..];
        path = match rest.find('/') {
            Some(index) => &rest[index..],
            None => "/",
        };
    }
    let path = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    };
    if path.len() > 1 {
        path.trim_end_matches('/').to_string()
    } else {
        path
    }
}

/// 从请求 URL 推导 Mock 路径：保留 `{{base_url}}` 之后的路径与 `{id}` 占位
pub fn path_from_url(url: &str) -> String {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return "/".to_string();
    }
    let mut rest = trimmed;
    if let Some(pos) = rest.find("://") {
        rest = &rest[pos + 3..];
        rest = match rest.find('/') {
            Some(index) => &rest[index..],
            None => "/",
        };
    } else if let Some(pos) = rest.find("}}") {
        // {{base_url}}/api/users → /api/users
        rest = &rest[pos + 2..];
    }
    if rest.is_empty() {
        return "/".to_string();
    }
    normalize_path(rest)
}

/// 运行时需要的请求定义
#[derive(Debug, Clone)]
pub struct MockRequestInfo {
    pub params: Vec<KeyValueItem>,
    pub body_type: String,
    pub body: String,
    pub response_entity: Option<String>,
}

pub fn request_info(conn: &Connection, request_id: i64) -> Option<MockRequestInfo> {
    conn.query_row(
        "SELECT params, body_type, body, response_entity FROM requests WHERE id = ?1",
        params![request_id],
        |row| {
            Ok(MockRequestInfo {
                params: serde_json::from_str::<Vec<KeyValueItem>>(
                    &row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                )
                .unwrap_or_default(),
                body_type: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                body: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                response_entity: row.get(3)?,
            })
        },
    )
    .optional()
    .ok()
    .flatten()
}

#[derive(Debug, Clone)]
pub struct RunResponse {
    pub status: i64,
    pub status_text: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

/// 最近一条有响应的运行记录：成功或失败都保留，才能 mock 错误分支
pub fn latest_run(conn: &Connection, request_id: i64) -> Option<RunResponse> {
    conn.query_row(
        "SELECT response_status, response_status_text, response_headers, response_body
         FROM request_runs
         WHERE request_id = ?1 AND response_status > 0
         ORDER BY created_at DESC, id DESC LIMIT 1",
        params![request_id],
        |row| {
            let headers: HashMap<String, String> =
                serde_json::from_str(&row.get::<_, Option<String>>(2)?.unwrap_or_default())
                    .unwrap_or_default();
            let mut headers: Vec<(String, String)> = headers.into_iter().collect();
            headers.sort_by_key(|(name, _)| name.to_ascii_lowercase());
            Ok(RunResponse {
                status: row.get(0)?,
                status_text: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                headers,
                body: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
            })
        },
    )
    .optional()
    .ok()
    .flatten()
}

/// 项目下所有敏感变量明文，用于回放脱敏
pub fn secret_values(conn: &Connection, project_id: i64) -> Vec<String> {
    let env_ids: Vec<i64> = conn
        .prepare("SELECT id FROM environments WHERE project_id = ?1")
        .and_then(|mut stmt| {
            stmt.query_map(params![project_id], |row| row.get(0))?
                .collect::<rusqlite::Result<Vec<_>>>()
        })
        .unwrap_or_default();
    let mut values = Vec::new();
    for env_id in env_ids {
        if let Ok((vars, secrets)) = crate::commands::http::load_vars_with_secrets(conn, env_id) {
            for key in secrets {
                if let Some(value) = vars.get(&key) {
                    if !value.is_empty() {
                        values.push(value.clone());
                    }
                }
            }
        }
    }
    values
}

/// 实体 schema → 骨架 JSON；没有 schema 时返回 None
pub fn skeleton_from_entity(raw: Option<&str>) -> Option<Value> {
    let schema = schema_from_raw(raw)?;
    sample_from_schema(&schema)
}

fn sample_from_schema(schema: &Value) -> Option<Value> {
    if let Some(example) = schema.get("example").filter(|value| !value.is_null()) {
        return Some(example.clone());
    }
    if let Some(first) = schema
        .get("enum")
        .and_then(|value| value.as_array())
        .and_then(|list| list.first())
    {
        return Some(first.clone());
    }
    let fallback_type = if schema.get("properties").is_some() {
        "object"
    } else {
        ""
    };
    match schema
        .get("type")
        .and_then(|value| value.as_str())
        .unwrap_or(fallback_type)
    {
        "object" => {
            let mut map = serde_json::Map::new();
            if let Some(properties) = schema.get("properties").and_then(|value| value.as_object()) {
                for (key, child) in properties {
                    map.insert(
                        key.clone(),
                        sample_from_schema(child).unwrap_or(Value::Null),
                    );
                }
            }
            if map.is_empty() {
                None
            } else {
                Some(Value::Object(map))
            }
        }
        "array" => {
            let item = schema
                .get("items")
                .and_then(sample_from_schema)
                .unwrap_or(Value::Null);
            Some(Value::Array(vec![item]))
        }
        "string" => {
            let sample = match schema.get("format").and_then(|value| value.as_str()) {
                Some("date-time") => "2026-01-01T12:00:00+08:00",
                Some("date") => "2026-01-01",
                Some("email") => "user10001@example.com",
                Some("uuid") => "00000000-0000-0000-0000-000000000000",
                _ => "string",
            };
            Some(Value::String(sample.to_string()))
        }
        "integer" | "number" => Some(serde_json::json!(0)),
        "boolean" => Some(serde_json::json!(true)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use r2d2_sqlite::SqliteConnectionManager;

    fn pool(tag: &str) -> crate::state::DbPool {
        let dir = crate::util::test_dir(tag);
        let manager = SqliteConnectionManager::file(dir.join("astroman.db"));
        let pool = r2d2::Pool::builder().max_size(2).build(manager).unwrap();
        crate::db::init(&pool).unwrap();
        pool
    }

    fn seed_request(conn: &Connection) -> (i64, i64) {
        let now = now_ms();
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
            "INSERT INTO requests (collection_id, name, method, url, created_at, updated_at)
             VALUES (?1, '详情', 'GET', '{{base_url}}/api/users/{id}?a=1', ?2, ?2)",
            params![collection, now],
        )
        .unwrap();
        (project, conn.last_insert_rowid())
    }

    #[test]
    fn path_is_derived_from_url_and_placeholders_are_kept() {
        assert_eq!(path_from_url("https://api.example.com/users/7"), "/users/7");
        assert_eq!(
            path_from_url("{{base_url}}/api/users/{id}?x=1"),
            "/api/users/{id}"
        );
        assert_eq!(path_from_url("http://localhost:8080"), "/");
        assert_eq!(normalize_path("/api/users/"), "/api/users");
    }

    #[test]
    fn generate_skips_existing_and_marks_conflicts() {
        let pool = pool("mock-store");
        let conn = pool.get().unwrap();
        let (project, _request) = seed_request(&conn);
        assert_eq!(generate_routes(&conn, project).unwrap(), 1);
        assert_eq!(generate_routes(&conn, project).unwrap(), 0);

        let route = MockRoute {
            project_id: project,
            name: "手写".into(),
            method: "GET".into(),
            path: "/api/users/{id}".into(),
            status_code: 200,
            headers: Vec::new(),
            body_type: "json".into(),
            body: "{}".into(),
            delay_ms: 0,
            enabled: true,
            sort_order: 99,
            ..Default::default()
        };
        let route = save_route(&conn, &route).unwrap();
        assert_eq!(route.sort_order, 99);
        let routes = list_routes(&conn, project).unwrap();
        assert_eq!(routes.len(), 2);
        assert!(routes.iter().all(|item| item.conflict));
    }
}
