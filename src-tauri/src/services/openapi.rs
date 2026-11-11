//! OpenAPI 3.0 构建
//!
//! 从 `commands/transfer.rs` 下沉：导出与契约快照共用同一份实体 → schema 转换，
//! 保证 JSON 导出与契约判定不会各自解析示例。

use rusqlite::{params, Connection};
use serde_json::{json, Map, Value};

use crate::entity::schema::schema_from_raw;
use crate::models::KeyValueItem;
use crate::util::err;

fn parse_items(raw: Option<String>) -> Vec<KeyValueItem> {
    raw.and_then(|text| serde_json::from_str::<Vec<KeyValueItem>>(&text).ok())
        .unwrap_or_default()
}

/// 从完整 URL 中提取 path 部分，尽量保留 {{变量}} 前缀之后的路径。
fn openapi_path(url: &str) -> String {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return "/".to_string();
    }
    let after_scheme = match trimmed.find("://") {
        Some(pos) => &trimmed[pos + 3..],
        None => trimmed,
    };
    let path = match after_scheme.find('/') {
        Some(pos) => &after_scheme[pos..],
        None => "/",
    };
    let path = path.split('?').next().unwrap_or("/");
    if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    }
}

fn openapi_body(body_type: &str, body: &str) -> Option<(&'static str, Value)> {
    let raw = body.trim();
    Some(match body_type {
        "json" | "graphql" => {
            let example = serde_json::from_str::<Value>(raw)
                .unwrap_or_else(|_| Value::String(raw.to_string()));
            ("application/json", json!({ "example": example }))
        }
        "form" | "multipart" => {
            let mut props = Map::new();
            if let Ok(fields) = serde_json::from_str::<Vec<KeyValueItem>>(raw) {
                for field in fields
                    .iter()
                    .filter(|f| f.enabled && !f.key.trim().is_empty())
                {
                    props.insert(field.key.trim().to_string(), json!({ "type": "string" }));
                }
            }
            let mime = if body_type == "multipart" {
                "multipart/form-data"
            } else {
                "application/x-www-form-urlencoded"
            };
            (
                mime,
                json!({ "schema": { "type": "object", "properties": Value::Object(props) } }),
            )
        }
        "binary" => (
            "application/octet-stream",
            json!({ "schema": { "type": "string", "format": "binary" } }),
        ),
        "raw" => ("text/plain", json!({ "example": raw })),
        _ => return None,
    })
}

// 对应 OpenAPI 一个 operation 的各个字段，本身就是扁平的
#[allow(clippy::too_many_arguments)]
fn openapi_operation(
    name: &str,
    method: &str,
    url: &str,
    headers: &[KeyValueItem],
    params: &[KeyValueItem],
    body_type: &str,
    body: &str,
    request_schema: Option<&Value>,
    response_schema: Option<&Value>,
    tag: &str,
) -> (String, Value) {
    let mut parameters: Vec<Value> = Vec::new();
    for item in params
        .iter()
        .filter(|i| i.enabled && !i.key.trim().is_empty())
    {
        parameters.push(json!({
            "name": item.key.trim(),
            "in": "query",
            "required": false,
            "schema": { "type": "string" }
        }));
    }
    const SKIP: [&str; 6] = [
        "content-type",
        "authorization",
        "cookie",
        "host",
        "content-length",
        "accept-encoding",
    ];
    for item in headers
        .iter()
        .filter(|i| i.enabled && !i.key.trim().is_empty())
    {
        let key = item.key.trim().to_lowercase();
        if SKIP.contains(&key.as_str()) {
            continue;
        }
        parameters.push(json!({
            "name": item.key.trim(),
            "in": "header",
            "required": false,
            "schema": { "type": "string" }
        }));
    }

    let mut operation = Map::new();
    operation.insert("tags".to_string(), json!([tag]));
    operation.insert("summary".to_string(), json!(name));
    operation.insert("description".to_string(), json!(format!("{method} {url}")));
    if !parameters.is_empty() {
        operation.insert("parameters".to_string(), Value::Array(parameters));
    }
    let mut request_content = openapi_body(body_type, body);
    if let Some(schema) = request_schema {
        let mime = request_content
            .as_ref()
            .map(|(mime, _)| *mime)
            .unwrap_or("application/json");
        let (_, content) = request_content.get_or_insert_with(|| (mime, json!({})));
        if let Value::Object(map) = content {
            map.insert("schema".to_string(), schema.clone());
        }
    }
    if let Some((mime, content)) = request_content {
        let mut content_map = Map::new();
        content_map.insert(mime.to_string(), content);
        operation.insert(
            "requestBody".to_string(),
            json!({ "content": Value::Object(content_map) }),
        );
    }
    let mut responses = json!({ "200": { "description": "OK" } });
    if let Some(schema) = response_schema {
        responses["200"]["content"] = json!({
            "application/json": { "schema": schema.clone() }
        });
    }
    operation.insert("responses".to_string(), responses);
    (method.to_lowercase(), Value::Object(operation))
}

fn collect_openapi_paths(
    conn: &Connection,
    rows: &[(i64, Option<i64>, String, i64)],
    parent: Option<i64>,
    prefix: &str,
    paths: &mut Map<String, Value>,
) -> Result<(), String> {
    for (id, parent_id, name, _) in rows {
        if *parent_id != parent {
            continue;
        }
        let tag = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix} / {name}")
        };

        let mut stmt = conn
            .prepare(
                "SELECT name, method, url, headers, params, body_type, body,
                        request_entity, response_entity
                 FROM requests WHERE collection_id = ?1 ORDER BY sort_order ASC, id ASC",
            )
            .map_err(err)?;
        let requests = stmt
            .query_map(params![id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, Option<String>>(7)?,
                    row.get::<_, Option<String>>(8)?,
                ))
            })
            .map_err(err)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(err)?;

        for (req_name, method, url, headers, query, body_type, body, req_entity, res_entity) in
            requests
        {
            let request_schema = schema_from_raw(req_entity.as_deref());
            let response_schema = schema_from_raw(res_entity.as_deref());
            let (verb, operation) = openapi_operation(
                &req_name,
                &method,
                &url,
                &parse_items(headers),
                &parse_items(query),
                &body_type.unwrap_or_default(),
                &body.unwrap_or_default(),
                request_schema.as_ref(),
                response_schema.as_ref(),
                &tag,
            );
            let base = openapi_path(&url);
            let mut candidate = base.clone();
            let mut index = 2;
            loop {
                let entry = paths
                    .entry(candidate.clone())
                    .or_insert_with(|| Value::Object(Map::new()));
                if let Value::Object(map) = entry {
                    if !map.contains_key(&verb) {
                        map.insert(verb.clone(), operation);
                        break;
                    }
                }
                candidate = format!("{base}-{index}");
                index += 1;
            }
        }

        collect_openapi_paths(conn, rows, Some(*id), &tag, paths)?;
    }
    Ok(())
}

/// 构建项目 OpenAPI 3.0 文档（导出与契约快照共用同一数据源）
pub(crate) fn build_openapi_spec(conn: &Connection, project_id: i64) -> Result<Value, String> {
    let (name, description): (String, Option<String>) = conn
        .query_row(
            "SELECT name, description FROM projects WHERE id = ?1",
            params![project_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => "项目不存在".to_string(),
            other => err(other),
        })?;

    let rows = conn
        .prepare(
            "SELECT id, parent_id, name, sort_order FROM collections
             WHERE project_id = ?1 ORDER BY sort_order ASC, id ASC",
        )
        .map_err(err)?
        .query_map(params![project_id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, Option<i64>>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<i64>>(3)?.unwrap_or(0),
            ))
        })
        .map_err(err)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(err)?;

    let mut paths = Map::new();
    collect_openapi_paths(conn, &rows, None, "", &mut paths)?;

    Ok(json!({
        "openapi": "3.0.3",
        "info": {
            "title": name,
            "version": "1.0.0",
            "description": description.unwrap_or_default(),
        },
        "servers": [{ "url": "http://localhost" }],
        "paths": Value::Object(paths),
    }))
}
