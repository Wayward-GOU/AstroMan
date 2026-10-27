use rusqlite::{params, Connection, Transaction};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use tauri::State;

use crate::models::{KeyValueItem, Project};
use crate::state::AppState;
use crate::util::{err, now_ms};

/// 导出使用的格式标记
const FORMAT: &str = "fbinhttp.project";
/// 改名前的格式标记，仍需接受，保证旧导出文件可以导入
const LEGACY_FORMAT: &str = "finkmeter.project";
const VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExportBundle {
    format: String,
    version: u32,
    exported_at: i64,
    project: ExportProject,
    #[serde(default)]
    collections: Vec<ExportCollection>,
    #[serde(default)]
    environments: Vec<ExportEnvironment>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExportProject {
    name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    color: Option<String>,
    #[serde(default)]
    sort_order: i64,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExportCollection {
    name: String,
    #[serde(default)]
    sort_order: i64,
    #[serde(default)]
    requests: Vec<ExportRequest>,
    #[serde(default)]
    children: Vec<ExportCollection>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExportRequest {
    name: String,
    #[serde(default = "default_method")]
    method: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    headers: Vec<KeyValueItem>,
    #[serde(default)]
    params: Vec<KeyValueItem>,
    #[serde(default = "default_body_type")]
    body_type: String,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    pre_script: Option<String>,
    #[serde(default)]
    post_script: Option<String>,
    #[serde(default)]
    assertions: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExportEnvironment {
    name: String,
    #[serde(default)]
    is_active: bool,
    #[serde(default)]
    script: Option<String>,
    #[serde(default)]
    variables: Vec<ExportVariable>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExportVariable {
    #[serde(default)]
    key: String,
    #[serde(default)]
    value: Option<String>,
    #[serde(default)]
    is_secret: bool,
    #[serde(default)]
    sort_order: i64,
}

fn default_method() -> String {
    "GET".to_string()
}

fn default_body_type() -> String {
    "none".to_string()
}

fn parse_items(raw: Option<String>) -> Vec<KeyValueItem> {
    raw.and_then(|text| serde_json::from_str::<Vec<KeyValueItem>>(&text).ok())
        .unwrap_or_default()
}

fn read_requests(conn: &Connection, collection_id: i64) -> Result<Vec<ExportRequest>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT name, method, url, headers, params, body_type, body, pre_script, post_script, assertions
             FROM requests WHERE collection_id = ?1
             ORDER BY sort_order ASC, id ASC",
        )
        .map_err(err)?;
    let rows = stmt
        .query_map(params![collection_id], |row| {
            Ok(ExportRequest {
                name: row.get(0)?,
                method: row.get(1)?,
                url: row.get(2)?,
                headers: parse_items(row.get(3)?),
                params: parse_items(row.get(4)?),
                body_type: row
                    .get::<_, Option<String>>(5)?
                    .unwrap_or_else(default_body_type),
                body: row.get(6)?,
                pre_script: row.get(7)?,
                post_script: row.get(8)?,
                assertions: row.get(9)?,
            })
        })
        .map_err(err)?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(err)
}

fn build_collections(
    rows: &[(i64, Option<i64>, String, i64)],
    parent: Option<i64>,
    conn: &Connection,
) -> Result<Vec<ExportCollection>, String> {
    let mut result = Vec::new();
    for (id, parent_id, name, sort_order) in rows {
        if *parent_id != parent {
            continue;
        }
        result.push(ExportCollection {
            name: name.clone(),
            sort_order: *sort_order,
            requests: read_requests(conn, *id)?,
            children: build_collections(rows, Some(*id), conn)?,
        });
    }
    Ok(result)
}

fn read_environments(conn: &Connection, project_id: i64) -> Result<Vec<ExportEnvironment>, String> {
    let mut env_stmt = conn
        .prepare(
            "SELECT id, name, is_active, script
             FROM environments WHERE project_id = ?1 ORDER BY id ASC",
        )
        .map_err(err)?;
    let envs = env_stmt
        .query_map(params![project_id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<i64>>(2)?.unwrap_or(0) == 1,
                row.get::<_, Option<String>>(3)?,
            ))
        })
        .map_err(err)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(err)?;

    let mut result = Vec::new();
    for (env_id, name, is_active, script) in envs {
        let mut var_stmt = conn
            .prepare(
                "SELECT \"key\", value, is_secret, sort_order FROM env_variables
                 WHERE env_id = ?1 ORDER BY sort_order ASC, id ASC",
            )
            .map_err(err)?;
        let variables = var_stmt
            .query_map(params![env_id], |row| {
                Ok(ExportVariable {
                    key: row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                    value: row.get(1)?,
                    is_secret: row.get::<_, Option<i64>>(2)?.unwrap_or(0) == 1,
                    sort_order: row.get::<_, Option<i64>>(3)?.unwrap_or(0),
                })
            })
            .map_err(err)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(err)?;
        result.push(ExportEnvironment {
            name,
            is_active,
            script,
            variables,
        });
    }
    Ok(result)
}

#[tauri::command]
pub fn project_export(
    state: State<'_, AppState>,
    project_id: i64,
    path: String,
) -> Result<(), String> {
    let conn = state.pool.get().map_err(err)?;

    let project = conn
        .query_row(
            "SELECT name, description, color, sort_order FROM projects WHERE id = ?1",
            params![project_id],
            |row| {
                Ok(ExportProject {
                    name: row.get(0)?,
                    description: row.get(1)?,
                    color: row.get(2)?,
                    sort_order: row.get::<_, Option<i64>>(3)?.unwrap_or(0),
                })
            },
        )
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => "项目不存在".to_string(),
            other => err(other),
        })?;

    let mut collection_rows: Vec<(i64, Option<i64>, String, i64)> = conn
        .prepare(
            "SELECT id, parent_id, name, sort_order FROM collections
             WHERE project_id = ?1 ORDER BY sort_order ASC, id ASC",
        )
        .map_err(err)?
        .query_map(params![project_id], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .map_err(err)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(err)?;

    // 让父集合先出现，子集合按 sort_order 归组
    collection_rows.sort_by_key(|(id, parent, _, sort)| (parent.unwrap_or(0), sort.clone(), *id));

    let bundle = ExportBundle {
        format: FORMAT.to_string(),
        version: VERSION,
        exported_at: now_ms(),
        project,
        collections: build_collections(&collection_rows, None, &conn)?,
        environments: read_environments(&conn, project_id)?,
    };

    let json = serde_json::to_string_pretty(&bundle).map_err(err)?;
    std::fs::write(&path, json).map_err(|e| format!("写入文件失败：{e}"))?;
    Ok(())
}

pub(crate) fn unique_project_name(conn: &Connection, base: &str) -> String {
    let mut candidate = base.to_string();
    let mut index = 1;
    loop {
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM projects WHERE name = ?1",
                params![candidate],
                |row| row.get(0),
            )
            .unwrap_or(0);
        if count == 0 {
            return candidate;
        }
        candidate = if index == 1 {
            format!("{base}（导入）")
        } else {
            format!("{base}（导入 {index}）")
        };
        index += 1;
    }
}

fn insert_collection(
    tx: &Transaction<'_>,
    project_id: i64,
    parent_id: Option<i64>,
    collection: &ExportCollection,
    now: i64,
) -> Result<(), String> {
    tx.execute(
        "INSERT INTO collections (project_id, parent_id, name, sort_order, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
        params![project_id, parent_id, collection.name, collection.sort_order, now],
    )
    .map_err(err)?;
    let collection_id = tx.last_insert_rowid();

    for (index, request) in collection.requests.iter().enumerate() {
        tx.execute(
            "INSERT INTO requests
                (collection_id, name, method, url, headers, params, body_type, body,
                 pre_script, post_script, assertions, sort_order, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?13)",
            params![
                collection_id,
                request.name,
                request.method,
                request.url,
                serde_json::to_string(&request.headers).map_err(err)?,
                serde_json::to_string(&request.params).map_err(err)?,
                request.body_type,
                request.body,
                request.pre_script,
                request.post_script,
                request.assertions,
                index as i64,
                now,
            ],
        )
        .map_err(err)?;
    }

    for child in &collection.children {
        insert_collection(tx, project_id, Some(collection_id), child, now)?;
    }

    Ok(())
}

#[tauri::command]
pub fn project_import(state: State<'_, AppState>, path: String) -> Result<Project, String> {
    let content = std::fs::read_to_string(&path).map_err(|e| format!("读取文件失败：{e}"))?;
    let bundle: ExportBundle =
        serde_json::from_str(&content).map_err(|e| format!("文件格式不正确：{e}"))?;

    if bundle.format != FORMAT && bundle.format != LEGACY_FORMAT {
        return Err("这不是 FbinHttp 项目文件".to_string());
    }
    if bundle.version > VERSION {
        return Err("项目文件版本高于当前应用，请升级后再导入".to_string());
    }

    let mut conn = state.pool.get().map_err(err)?;
    let now = now_ms();
    let raw_name = bundle.project.name.trim();
    let base_name = if raw_name.is_empty() { "导入项目" } else { raw_name };
    let name = unique_project_name(&conn, base_name);

    let tx = conn.transaction().map_err(err)?;
    tx.execute(
        "INSERT INTO projects (name, description, color, sort_order, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
        params![
            name,
            bundle.project.description,
            bundle.project.color,
            bundle.project.sort_order,
            now
        ],
    )
    .map_err(err)?;
    let project_id = tx.last_insert_rowid();

    for collection in &bundle.collections {
        insert_collection(&tx, project_id, None, collection, now)?;
    }

    let mut active_taken = false;
    for environment in &bundle.environments {
        let active = environment.is_active && !active_taken;
        if active {
            active_taken = true;
        }
        tx.execute(
            "INSERT INTO environments (project_id, name, is_active, script, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                project_id,
                environment.name,
                active as i64,
                environment.script.clone().unwrap_or_default(),
                now
            ],
        )
        .map_err(err)?;
        let env_id = tx.last_insert_rowid();

        for variable in &environment.variables {
            tx.execute(
                "INSERT INTO env_variables (env_id, \"key\", value, is_secret, sort_order)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    env_id,
                    variable.key,
                    variable.value,
                    variable.is_secret as i64,
                    variable.sort_order
                ],
            )
            .map_err(err)?;
        }
    }

    tx.commit().map_err(err)?;

    Ok(Project {
        id: project_id,
        name,
        description: bundle.project.description,
        color: bundle.project.color,
        sort_order: bundle.project.sort_order,
        created_at: now,
        updated_at: now,
    })
}

// ---------- OpenAPI 3.0 导出 ----------

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
                for field in fields.iter().filter(|f| f.enabled && !f.key.trim().is_empty()) {
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

fn openapi_operation(
    name: &str,
    method: &str,
    url: &str,
    headers: &[KeyValueItem],
    params: &[KeyValueItem],
    body_type: &str,
    body: &str,
    tag: &str,
) -> (String, Value) {
    let mut parameters: Vec<Value> = Vec::new();
    for item in params.iter().filter(|i| i.enabled && !i.key.trim().is_empty()) {
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
    for item in headers.iter().filter(|i| i.enabled && !i.key.trim().is_empty()) {
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
    if let Some((mime, content)) = openapi_body(body_type, body) {
        let mut content_map = Map::new();
        content_map.insert(mime.to_string(), content);
        operation.insert(
            "requestBody".to_string(),
            json!({ "content": Value::Object(content_map) }),
        );
    }
    operation.insert(
        "responses".to_string(),
        json!({ "200": { "description": "OK" } }),
    );
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
                "SELECT name, method, url, headers, params, body_type, body
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
                ))
            })
            .map_err(err)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(err)?;

        for (req_name, method, url, headers, query, body_type, body) in requests {
            let (verb, operation) = openapi_operation(
                &req_name,
                &method,
                &url,
                &parse_items(headers),
                &parse_items(query),
                &body_type.unwrap_or_default(),
                &body.unwrap_or_default(),
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

#[tauri::command]
pub fn openapi_export(
    state: State<'_, AppState>,
    project_id: i64,
    path: String,
) -> Result<(), String> {
    let conn = state.pool.get().map_err(err)?;
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
    collect_openapi_paths(&conn, &rows, None, "", &mut paths)?;

    let spec = json!({
        "openapi": "3.0.3",
        "info": {
            "title": name,
            "version": "1.0.0",
            "description": description.unwrap_or_default(),
        },
        "servers": [{ "url": "http://localhost" }],
        "paths": Value::Object(paths),
    });

    let text = serde_json::to_string_pretty(&spec).map_err(err)?;
    std::fs::write(&path, text).map_err(|e| format!("写入文件失败：{e}"))?;
    Ok(())
}

// ---------- OpenAPI 3.0 / Swagger 2.0 导入 ----------

fn value_str(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(text)) => text.clone(),
        Some(other) => other.to_string(),
        None => String::new(),
    }
}

/// 参数示例值：优先 example，其次 schema.example / schema.default / 首个 enum。
fn param_example(param: &Value) -> String {
    if let Some(example) = param.get("example") {
        return value_str(Some(example));
    }
    if let Some(example) = param.get("default") {
        return value_str(Some(example));
    }
    if let Some(schema) = param.get("schema") {
        if let Some(example) = schema.get("example") {
            return value_str(Some(example));
        }
        if let Some(default) = schema.get("default") {
            return value_str(Some(default));
        }
        if let Some(Value::Array(items)) = schema.get("enum") {
            if let Some(first) = items.first() {
                return value_str(Some(first));
            }
        }
    }
    String::new()
}

fn openapi_request_body(body: &Value) -> (String, String) {
    let Some(content) = body.get("content").and_then(|v| v.as_object()) else {
        return ("none".to_string(), String::new());
    };
    if let Some(media) = content.get("application/json") {
        if let Some(example) = media.get("example") {
            return (
                "json".to_string(),
                serde_json::to_string_pretty(example).unwrap_or_default(),
            );
        }
        if let Some(first) = media
            .get("examples")
            .and_then(|v| v.as_object())
            .and_then(|map| map.values().next())
        {
            let value = first.get("value").cloned().unwrap_or(Value::Null);
            if !value.is_null() {
                return (
                    "json".to_string(),
                    serde_json::to_string_pretty(&value).unwrap_or_default(),
                );
            }
        }
        if let Some(schema) = media.get("schema") {
            return ("json".to_string(), schema_template(schema, 0));
        }
        return ("json".to_string(), String::new());
    }
    if let Some(media) = content
        .get("application/x-www-form-urlencoded")
        .or_else(|| content.get("multipart/form-data"))
    {
        let multi = content.get("multipart/form-data").is_some();
        let mut fields = Vec::new();
        if let Some(props) = media
            .pointer("/schema/properties")
            .and_then(|v| v.as_object())
        {
            for (key, schema) in props {
                let value = value_str(schema.get("example"));
                fields.push(KeyValueItem {
                    key: key.clone(),
                    value,
                    enabled: true,
                });
            }
        }
        let body_type = if multi { "multipart" } else { "form" };
        return (
            body_type.to_string(),
            serde_json::to_string(&fields).unwrap_or_else(|_| "[]".to_string()),
        );
    }
    ("raw".to_string(), String::new())
}

/// 根据 JSON Schema 生成一个结构示例。
fn schema_template(schema: &Value, depth: usize) -> String {
    if depth > 4 {
        return "null".to_string();
    }
    if let Some(example) = schema.get("example") {
        return serde_json::to_string_pretty(example).unwrap_or_default();
    }
    let kind = value_str(schema.get("type"));
    match kind.as_str() {
        "object" => {
            let mut map = Map::new();
            if let Some(props) = schema.get("properties").and_then(|v| v.as_object()) {
                for (key, child) in props {
                    map.insert(
                        key.clone(),
                        serde_json::from_str(&schema_template(child, depth + 1))
                            .unwrap_or(Value::Null),
                    );
                }
            }
            serde_json::to_string_pretty(&Value::Object(map)).unwrap_or_default()
        }
        "array" => {
            let item = schema
                .get("items")
                .map(|child| schema_template(child, depth + 1))
                .unwrap_or_else(|| "null".to_string());
            format!("[{}]", item)
        }
        "integer" | "number" => "0".to_string(),
        "boolean" => "false".to_string(),
        _ => "\"\"".to_string(),
    }
}

#[tauri::command]
pub fn openapi_import(
    state: State<'_, AppState>,
    path: String,
    project_id: Option<i64>,
) -> Result<Project, String> {
    let content = std::fs::read_to_string(&path).map_err(|e| format!("读取文件失败：{e}"))?;
    let spec: Value =
        serde_json::from_str(&content).map_err(|e| format!("文件不是有效的 JSON：{e}"))?;

    let paths = spec
        .get("paths")
        .and_then(|v| v.as_object())
        .ok_or_else(|| "缺少 paths 字段，不是有效的 OpenAPI / Swagger 文档".to_string())?;

    let title = spec
        .pointer("/info/title")
        .map(|v| value_str(Some(v)))
        .unwrap_or_else(|| "OpenAPI 导入".to_string());
    let description = spec
        .pointer("/info/description")
        .map(|v| value_str(Some(v)))
        .unwrap_or_default();

    // OpenAPI 3: servers[0].url；Swagger 2: scheme + host + basePath
    let base_url = if let Some(url) = spec.pointer("/servers/0/url").map(|v| value_str(Some(v))) {
        url
    } else {
        let host = spec.get("host").map(|v| value_str(Some(v))).unwrap_or_default();
        let base = spec
            .get("basePath")
            .map(|v| value_str(Some(v)))
            .unwrap_or_default();
        let scheme = spec
            .pointer("/schemes/0")
            .map(|v| value_str(Some(v)))
            .unwrap_or_else(|| "http".to_string());
        if host.is_empty() {
            base
        } else {
            format!("{scheme}://{host}{base}")
        }
    };
    let base_url = base_url.trim_end_matches('/').to_string();

    let mut conn = state.pool.get().map_err(err)?;
    let now = now_ms();
    let tx = conn.transaction().map_err(err)?;

    let project_id = if let Some(id) = project_id {
        let exists: i64 = tx
            .query_row("SELECT COUNT(*) FROM projects WHERE id = ?1", params![id], |row| {
                row.get(0)
            })
            .map_err(err)?;
        if exists == 0 {
            return Err("目标项目不存在".to_string());
        }
        id
    } else {
        let base_name = if title.trim().is_empty() { "OpenAPI 导入" } else { title.trim() };
        let name = unique_project_name(&tx, base_name);
        tx.execute(
            "INSERT INTO projects (name, description, color, sort_order, created_at, updated_at)
             VALUES (?1, ?2, NULL, 0, ?3, ?3)",
            params![name, description, now],
        )
        .map_err(err)?;
        tx.last_insert_rowid()
    };

    let project_name: String = tx
        .query_row("SELECT name FROM projects WHERE id = ?1", params![project_id], |row| {
            row.get(0)
        })
        .map_err(err)?;

    // 先按 tag 分组，再按路径顺序写入集合
    let methods = ["get", "post", "put", "patch", "delete", "head", "options"];
    let mut groups: Vec<(
        String,
        Vec<(
            String,
            String,
            String,
            String,
            Vec<KeyValueItem>,
            Vec<KeyValueItem>,
            String,
        )>,
    )> = Vec::new();

    for (raw_path, item) in paths {
        let Some(operations) = item.as_object() else {
            continue;
        };
        for method in methods {
            let Some(operation) = operations.get(method) else {
                continue;
            };
            let name = operation
                .get("summary")
                .map(|v| value_str(Some(v)))
                .filter(|v| !v.trim().is_empty())
                .or_else(|| {
                    operation
                        .get("operationId")
                        .map(|v| value_str(Some(v)))
                        .filter(|v| !v.trim().is_empty())
                })
                .unwrap_or_else(|| format!("{} {}", method.to_uppercase(), raw_path));
            let tag = operation
                .pointer("/tags/0")
                .map(|v| value_str(Some(v)))
                .unwrap_or_else(|| "默认分组".to_string());

            let mut query_params = Vec::new();
            let mut header_params = Vec::new();
            let parameter_sets = [
                item.get("parameters"),
                operation.get("parameters"),
            ];
            for set in parameter_sets.into_iter().flatten() {
                let Some(list) = set.as_array() else { continue };
                for param in list {
                    let location = value_str(param.get("in"));
                    let key = value_str(param.get("name"));
                    if key.trim().is_empty() {
                        continue;
                    }
                    let entry = KeyValueItem {
                        key,
                        value: param_example(param),
                        enabled: true,
                    };
                    if location == "query" {
                        query_params.push(entry);
                    } else if location == "header" {
                        header_params.push(entry);
                    }
                }
            }

            let mut body_type = "none".to_string();
            let mut body = String::new();
            if let Some(request_body) = operation.get("requestBody") {
                let parsed = openapi_request_body(request_body);
                body_type = parsed.0;
                body = parsed.1;
            } else if let Some(consumes) = spec.get("consumes").and_then(|v| v.as_array()) {
                if consumes.iter().any(|v| value_str(Some(v)).contains("json")) {
                    body_type = "json".to_string();
                }
            }

            let url = format!("{base_url}{raw_path}");
            let target = groups.iter_mut().find(|(group, _)| *group == tag);
            let bucket = if let Some(existing) = target {
                &mut existing.1
            } else {
                groups.push((tag.clone(), Vec::new()));
                &mut groups.last_mut().unwrap().1
            };
            bucket.push((
                name,
                method.to_uppercase(),
                url,
                body_type,
                query_params,
                header_params,
                body,
            ));
        }
    }

    let mut collection_order = 0_i64;
    for (group_name, requests) in groups {
        tx.execute(
            "INSERT INTO collections (project_id, parent_id, name, sort_order, created_at, updated_at)
             VALUES (?1, NULL, ?2, ?3, ?4, ?4)",
            params![project_id, group_name, collection_order, now],
        )
        .map_err(err)?;
        let collection_id = tx.last_insert_rowid();
        collection_order += 1;

        for (index, request) in requests.into_iter().enumerate() {
            let (name, method, url, body_type, query, headers, body) = request;
            tx.execute(
                "INSERT INTO requests
                    (collection_id, name, method, url, headers, params, body_type, body,
                     pre_script, post_script, assertions, sort_order, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, '', '', '', ?9, ?10, ?10)",
                params![
                    collection_id,
                    name,
                    method,
                    url,
                    serde_json::to_string(&headers).map_err(err)?,
                    serde_json::to_string(&query).map_err(err)?,
                    body_type,
                    body,
                    index as i64,
                    now,
                ],
            )
            .map_err(err)?;
        }
    }

    tx.commit().map_err(err)?;

    Ok(Project {
        id: project_id,
        name: project_name,
        description: None,
        color: None,
        sort_order: 0,
        created_at: now,
        updated_at: now,
    })
}
