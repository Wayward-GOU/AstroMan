use std::collections::HashMap;

use rusqlite::{params, OptionalExtension, Transaction};
use serde_json::Value;
use tauri::State;

use crate::commands::transfer::unique_project_name;
use crate::models::{KeyValueItem, Project};
use crate::state::AppState;
use crate::util::{err, now_ms};

#[derive(Default)]
struct ParsedCollection {
    name: String,
    requests: Vec<ParsedRequest>,
    children: Vec<ParsedCollection>,
}

struct ParsedRequest {
    name: String,
    method: String,
    url: String,
    headers: Vec<KeyValueItem>,
    params: Vec<KeyValueItem>,
    body_type: String,
    body: String,
}

#[derive(Default)]
struct ParsedImport {
    name: String,
    description: Option<String>,
    collections: Vec<ParsedCollection>,
    variables: Vec<(String, String)>,
}

/// 导入 Postman Collection（v2.0 / v2.1）或 Apifox 导出的 OpenAPI（JSON）。
/// 传入 project_id 时导入到现有项目，否则创建一个新项目。
#[tauri::command]
pub fn api_import(
    state: State<'_, AppState>,
    path: String,
    project_id: Option<i64>,
) -> Result<Project, String> {
    let content = std::fs::read_to_string(&path).map_err(|e| format!("读取文件失败：{e}"))?;
    let content = content.trim_start_matches('\u{feff}');
    let root: Value = serde_json::from_str(content).map_err(|e| format!("JSON 解析失败：{e}"))?;
    let parsed = parse_import(&root)?;

    let mut conn = state.pool.get().map_err(err)?;
    let now = now_ms();
    let tx = conn.transaction().map_err(err)?;

    let project = match project_id {
        Some(id) => {
            let project = tx
                .query_row(
                    "SELECT id, name, description, color, sort_order, created_at, updated_at
                     FROM projects WHERE id = ?1",
                    params![id],
                    |row| {
                        Ok(Project {
                            id: row.get(0)?,
                            name: row.get(1)?,
                            description: row.get(2)?,
                            color: row.get(3)?,
                            sort_order: row.get::<_, Option<i64>>(4)?.unwrap_or(0),
                            created_at: row.get(5)?,
                            updated_at: row.get(6)?,
                        })
                    },
                )
                .map_err(|e| match e {
                    rusqlite::Error::QueryReturnedNoRows => "目标项目不存在".to_string(),
                    other => err(other),
                })?;
            project
        }
        None => {
            let base = parsed.name.trim();
            let base = if base.is_empty() { "导入项目" } else { base };
            let name = unique_project_name(&tx, base);
            tx.execute(
                "INSERT INTO projects (name, description, color, sort_order, created_at, updated_at)
                 VALUES (?1, ?2, NULL, 0, ?3, ?3)",
                params![name, parsed.description, now],
            )
            .map_err(err)?;
            let id = tx.last_insert_rowid();
            Project {
                id,
                name,
                description: parsed.description.clone(),
                color: None,
                sort_order: 0,
                created_at: now,
                updated_at: now,
            }
        }
    };

    for (index, collection) in parsed.collections.iter().enumerate() {
        insert_collection(&tx, project.id, None, collection, index as i64, now)?;
    }
    apply_variables(&tx, project.id, &parsed.variables, now)?;

    tx.commit().map_err(err)?;
    Ok(project)
}

fn parse_import(root: &Value) -> Result<ParsedImport, String> {
    if root.get("info").is_some() && root.get("item").and_then(|v| v.as_array()).is_some() {
        return parse_postman(root);
    }
    if root.get("openapi").is_some() || root.get("swagger").is_some() {
        return parse_openapi(root);
    }
    if root.get("requests").is_some() {
        return Err(
            "暂不支持 Postman v1 格式，请在 Postman 中重新导出为 Collection v2.1".to_string(),
        );
    }
    Err("无法识别文件格式：请选择 Postman Collection v2.0/v2.1 导出的 JSON，或 Apifox 导出的 OpenAPI JSON".to_string())
}

// ---------- 通用工具 ----------

fn value_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => number.to_string(),
        Value::Bool(flag) => flag.to_string(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn optional_text(value: Option<&Value>) -> Option<String> {
    value
        .and_then(|v| match v {
            Value::String(text) => Some(text.trim().to_string()),
            Value::Number(_) | Value::Bool(_) => Some(value_text(v)),
            _ => None,
        })
        .filter(|text| !text.is_empty())
}

fn looks_like_json(text: &str) -> bool {
    let trimmed = text.trim();
    (trimmed.starts_with('{') || trimmed.starts_with('['))
        && serde_json::from_str::<Value>(trimmed).is_ok()
}

/// 读取 Postman 集合变量或环境中的 `[{ key, value }]`
fn read_variables(value: Option<&Value>) -> Vec<(String, String)> {
    let mut result = Vec::new();
    if let Some(list) = value.and_then(|v| v.as_array()) {
        for item in list {
            let key = optional_text(item.get("key"))
                .or_else(|| optional_text(item.get("name")))
                .unwrap_or_default();
            if key.is_empty() {
                continue;
            }
            let value = item.get("value").map(value_text).unwrap_or_default();
            result.push((key, value));
        }
    }
    result
}

// ---------- Postman ----------

fn parse_postman(root: &Value) -> Result<ParsedImport, String> {
    let info = root.get("info").unwrap_or(&Value::Null);
    let name = optional_text(info.get("name"))
        .or_else(|| optional_text(root.get("name")))
        .unwrap_or_else(|| "Postman 导入".to_string());
    let description = optional_text(info.get("description"));
    let variables = read_variables(root.get("variable"));
    let items = root
        .get("item")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let mut collections = Vec::new();
    let mut loose = ParsedCollection {
        name: name.clone(),
        ..Default::default()
    };
    for item in &items {
        let item_name = optional_text(item.get("name")).unwrap_or_else(|| "未命名".to_string());
        if let Some(children) = item.get("item").and_then(|v| v.as_array()) {
            collections.push(build_postman_folder(&item_name, children));
        } else if let Some(request) = item.get("request") {
            loose.requests.push(parse_postman_request(&item_name, request));
        }
    }
    if !loose.requests.is_empty() {
        collections.push(loose);
    }
    if collections.is_empty() {
        return Err("Postman 文件中没有可导入的接口".to_string());
    }

    Ok(ParsedImport {
        name,
        description,
        collections,
        variables,
    })
}

fn build_postman_folder(name: &str, items: &[Value]) -> ParsedCollection {
    let mut folder = ParsedCollection {
        name: name.to_string(),
        ..Default::default()
    };
    for item in items {
        let item_name = optional_text(item.get("name")).unwrap_or_else(|| "未命名".to_string());
        if let Some(children) = item.get("item").and_then(|v| v.as_array()) {
            folder.children.push(build_postman_folder(&item_name, children));
        } else if let Some(request) = item.get("request") {
            folder.requests.push(parse_postman_request(&item_name, request));
        }
    }
    folder
}

fn parse_postman_request(name: &str, request: &Value) -> ParsedRequest {
    if let Some(url) = request.as_str() {
        return ParsedRequest {
            name: name.to_string(),
            method: "GET".to_string(),
            url: url.to_string(),
            headers: Vec::new(),
            params: Vec::new(),
            body_type: "none".to_string(),
            body: String::new(),
        };
    }

    let method = optional_text(request.get("method"))
        .unwrap_or_else(|| "GET".to_string())
        .to_uppercase();
    let (url, params) = parse_postman_url(request.get("url").unwrap_or(&Value::Null));

    let mut headers = Vec::new();
    if let Some(list) = request.get("header").and_then(|v| v.as_array()) {
        for item in list {
            let key = optional_text(item.get("key")).unwrap_or_default();
            if key.is_empty() {
                continue;
            }
            let value = item.get("value").map(value_text).unwrap_or_default();
            let disabled = item
                .get("disabled")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            headers.push(KeyValueItem {
                key,
                value,
                enabled: !disabled,
            });
        }
    }

    let (body_type, body) = request
        .get("body")
        .map(parse_postman_body)
        .unwrap_or_else(|| ("none".to_string(), String::new()));

    ParsedRequest {
        name: name.to_string(),
        method,
        url,
        headers,
        params,
        body_type,
        body,
    }
}

fn parse_postman_url(value: &Value) -> (String, Vec<KeyValueItem>) {
    let raw = match value {
        Value::String(text) => text.clone(),
        Value::Object(_) => optional_text(value.get("raw")).unwrap_or_else(|| build_postman_url(value)),
        _ => String::new(),
    };

    let mut base = raw;
    if let Some(index) = base.find('#') {
        base.truncate(index);
    }
    let mut raw_query = None;
    if let Some(index) = base.find('?') {
        raw_query = Some(base[index + 1..].to_string());
        base.truncate(index);
    }

    let mut params = Vec::new();
    if let Some(query) = value.get("query").and_then(|v| v.as_array()) {
        for item in query {
            let key = optional_text(item.get("key")).unwrap_or_default();
            if key.is_empty() {
                continue;
            }
            let param_value = item.get("value").map(value_text).unwrap_or_default();
            let disabled = item
                .get("disabled")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            params.push(KeyValueItem {
                key,
                value: param_value,
                enabled: !disabled,
            });
        }
    }

    if params.is_empty() {
        if let Some(query) = raw_query {
            for pair in query.split('&') {
                if pair.is_empty() {
                    continue;
                }
                let (key, value) = match pair.split_once('=') {
                    Some((key, value)) => (key, value),
                    None => (pair, ""),
                };
                params.push(KeyValueItem {
                    key: key.to_string(),
                    value: value.to_string(),
                    enabled: true,
                });
            }
        }
    }

    (base.trim().to_string(), params)
}

fn build_postman_url(url: &Value) -> String {
    let protocol = optional_text(url.get("protocol")).unwrap_or_else(|| "https".to_string());
    let host = match url.get("host") {
        Some(Value::Array(parts)) => parts.iter().map(value_text).collect::<Vec<_>>().join("."),
        Some(Value::String(text)) => text.clone(),
        _ => String::new(),
    };
    if host.is_empty() {
        return String::new();
    }
    let port = optional_text(url.get("port"))
        .map(|port| format!(":{port}"))
        .unwrap_or_default();
    let path = match url.get("path") {
        Some(Value::Array(parts)) => parts.iter().map(value_text).collect::<Vec<_>>().join("/"),
        Some(Value::String(text)) => text.trim_start_matches('/').to_string(),
        _ => String::new(),
    };
    if path.is_empty() {
        format!("{protocol}://{host}{port}")
    } else {
        format!("{protocol}://{host}{port}/{path}")
    }
}

fn parse_postman_body(body: &Value) -> (String, String) {
    let mode = body.get("mode").and_then(|v| v.as_str()).unwrap_or("none");
    match mode {
        "raw" => {
            let raw = body
                .get("raw")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if raw.trim().is_empty() {
                return ("none".to_string(), String::new());
            }
            let language = body
                .pointer("/options/raw/language")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let is_json = language.eq_ignore_ascii_case("json") || looks_like_json(&raw);
            (
                if is_json { "json" } else { "text" }.to_string(),
                raw,
            )
        }
        "urlencoded" | "formdata" => {
            let field = if mode == "urlencoded" {
                "urlencoded"
            } else {
                "formdata"
            };
            let mut pairs = Vec::new();
            if let Some(list) = body.get(field).and_then(|v| v.as_array()) {
                for item in list {
                    if item
                        .get("disabled")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false)
                    {
                        continue;
                    }
                    if item.get("type").and_then(|v| v.as_str()) == Some("file") {
                        continue;
                    }
                    let key = optional_text(item.get("key")).unwrap_or_default();
                    if key.is_empty() {
                        continue;
                    }
                    let value = item.get("value").map(value_text).unwrap_or_default();
                    pairs.push(format!("{key}={value}"));
                }
            }
            if pairs.is_empty() {
                ("none".to_string(), String::new())
            } else {
                ("form".to_string(), pairs.join("&"))
            }
        }
        "graphql" => {
            let graphql = body.get("graphql").unwrap_or(&Value::Null);
            let query = graphql.get("query").and_then(|v| v.as_str()).unwrap_or("");
            let variables = graphql
                .get("variables")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let payload = serde_json::json!({
                "query": query,
                "variables": serde_json::from_str::<Value>(variables).unwrap_or(Value::Null),
            });
            (
                "json".to_string(),
                serde_json::to_string_pretty(&payload).unwrap_or_default(),
            )
        }
        _ => ("none".to_string(), String::new()),
    }
}

// ---------- OpenAPI / Apifox ----------

fn parse_openapi(root: &Value) -> Result<ParsedImport, String> {
    let info = root.get("info").unwrap_or(&Value::Null);
    let name = optional_text(info.get("title")).unwrap_or_else(|| "Apifox 导入".to_string());
    let description = optional_text(info.get("description"));
    let paths = root
        .get("paths")
        .and_then(|v| v.as_object())
        .ok_or_else(|| "OpenAPI 文件缺少 paths 字段".to_string())?;
    let is_v3 = root.get("openapi").is_some();
    let server_url = read_server_url(root, is_v3);

    let mut groups: Vec<ParsedCollection> = Vec::new();
    let mut indexes: HashMap<String, usize> = HashMap::new();

    for (path, item) in paths {
        if !item.is_object() {
            continue;
        }
        let path_parameters = item
            .get("parameters")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        for method in ["get", "post", "put", "patch", "delete", "head", "options", "trace"] {
            let Some(operation) = item.get(method) else {
                continue;
            };
            let request = parse_openapi_operation(
                root,
                path,
                method,
                operation,
                &path_parameters,
                &server_url,
                is_v3,
            );
            let tag = optional_text(operation.get("x-apifox-folder"))
                .or_else(|| {
                    operation
                        .get("tags")
                        .and_then(|v| v.as_array())
                        .and_then(|tags| tags.first())
                        .map(value_text)
                        .filter(|tag| !tag.trim().is_empty())
                })
                .unwrap_or_else(|| "默认分组".to_string());
            let index = match indexes.get(&tag) {
                Some(index) => *index,
                None => {
                    let index = groups.len();
                    indexes.insert(tag.clone(), index);
                    groups.push(ParsedCollection {
                        name: tag,
                        ..Default::default()
                    });
                    index
                }
            };
            groups[index].requests.push(request);
        }
    }

    if groups.is_empty() {
        return Err("OpenAPI 文件中没有可导入的接口".to_string());
    }

    let mut variables = Vec::new();
    if !server_url.is_empty() {
        variables.push(("base_url".to_string(), server_url));
    }

    Ok(ParsedImport {
        name,
        description,
        collections: groups,
        variables,
    })
}

fn read_server_url(root: &Value, is_v3: bool) -> String {
    if is_v3 {
        root.get("servers")
            .and_then(|v| v.as_array())
            .and_then(|servers| servers.first())
            .and_then(|server| optional_text(server.get("url")))
            .unwrap_or_default()
    } else {
        let scheme = optional_text(root.get("scheme")).unwrap_or_else(|| "https".to_string());
        let Some(host) = optional_text(root.get("host")) else {
            return String::new();
        };
        let base_path = optional_text(root.get("basePath")).unwrap_or_default();
        format!("{scheme}://{host}{base_path}")
    }
}

fn parse_openapi_operation(
    root: &Value,
    path: &str,
    method: &str,
    operation: &Value,
    path_parameters: &[Value],
    server_url: &str,
    is_v3: bool,
) -> ParsedRequest {
    let name = optional_text(operation.get("summary"))
        .or_else(|| optional_text(operation.get("operationId")))
        .unwrap_or_else(|| format!("{} {}", method.to_uppercase(), path));

    let mut headers = Vec::new();
    let mut params = Vec::new();
    let mut cookies = Vec::new();
    let mut form_fields: Vec<(String, String)> = Vec::new();
    let mut body_type = "none".to_string();
    let mut body = String::new();

    let mut parameters: Vec<&Value> = path_parameters.iter().collect();
    if let Some(list) = operation.get("parameters").and_then(|v| v.as_array()) {
        parameters.extend(list.iter());
    }

    for parameter in parameters {
        let location = parameter.get("in").and_then(|v| v.as_str()).unwrap_or("");
        let key = optional_text(parameter.get("name")).unwrap_or_default();
        match location {
            "query" if !key.is_empty() => params.push(KeyValueItem {
                key,
                value: param_value(root, parameter),
                enabled: true,
            }),
            "header" if !key.is_empty() => headers.push(KeyValueItem {
                key,
                value: param_value(root, parameter),
                enabled: true,
            }),
            "cookie" if !key.is_empty() => {
                cookies.push(format!("{key}={}", param_value(root, parameter)))
            }
            "formData" if !key.is_empty() => {
                form_fields.push((key, param_value(root, parameter)))
            }
            "body" if !is_v3 => {
                let schema = resolve_schema(root, parameter.get("schema").unwrap_or(&Value::Null));
                body = body_from_schema(root, schema);
                if !body.trim().is_empty() {
                    body_type = "json".to_string();
                }
            }
            _ => {}
        }
    }

    if !cookies.is_empty() {
        headers.push(KeyValueItem {
            key: "Cookie".to_string(),
            value: cookies.join("; "),
            enabled: true,
        });
    }

    if is_v3 {
        if let Some(content) = operation
            .pointer("/requestBody/content")
            .and_then(|v| v.as_object())
        {
            let json_media = content.get("application/json").or_else(|| {
                content
                    .iter()
                    .find(|(name, _)| name.to_ascii_lowercase().contains("json"))
                    .map(|(_, value)| value)
            });
            if let Some(media) = json_media {
                let sample = media_body(root, media);
                if !sample.trim().is_empty() {
                    body = sample;
                    body_type = "json".to_string();
                }
            } else if let Some(media) = content.get("application/x-www-form-urlencoded") {
                let fields = form_from_schema(root, media.get("schema").unwrap_or(&Value::Null));
                if !fields.is_empty() {
                    body = fields
                        .iter()
                        .map(|(key, value)| format!("{key}={value}"))
                        .collect::<Vec<_>>()
                        .join("&");
                    body_type = "form".to_string();
                }
            } else if let Some((_, media)) = content.iter().next() {
                let sample = media_body(root, media);
                if !sample.trim().is_empty() {
                    body = sample;
                    body_type = "text".to_string();
                }
            }
        }
    } else if !form_fields.is_empty() {
        body = form_fields
            .iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect::<Vec<_>>()
            .join("&");
        body_type = "form".to_string();
    }

    let url = if server_url.is_empty() {
        path.to_string()
    } else if path.starts_with('/') {
        format!("{{{{base_url}}}}{path}")
    } else {
        format!("{{{{base_url}}}}/{path}")
    };

    ParsedRequest {
        name,
        method: method.to_uppercase(),
        url,
        headers,
        params,
        body_type,
        body,
    }
}

fn param_value(root: &Value, parameter: &Value) -> String {
    if let Some(example) = parameter.get("example").filter(|v| !v.is_null()) {
        return value_text(example);
    }
    let schema = resolve_schema(root, parameter.get("schema").unwrap_or(parameter));
    if let Some(default) = schema.get("default").filter(|v| !v.is_null()) {
        return value_text(default);
    }
    if let Some(example) = schema.get("example").filter(|v| !v.is_null()) {
        return value_text(example);
    }
    if let Some(first) = schema
        .get("enum")
        .and_then(|v| v.as_array())
        .and_then(|v| v.first())
    {
        return value_text(first);
    }
    String::new()
}

/// 解析本地 `$ref`（#/components/schemas/... 或 #/definitions/...）
fn resolve_schema<'a>(root: &'a Value, schema: &'a Value) -> &'a Value {
    let mut current = schema;
    for _ in 0..8 {
        let Some(reference) = current.get("$ref").and_then(|v| v.as_str()) else {
            break;
        };
        let path = reference.strip_prefix("#/").unwrap_or(reference);
        let mut node = root;
        let mut found = true;
        for part in path.split('/') {
            match node.get(part) {
                Some(next) => node = next,
                None => {
                    found = false;
                    break;
                }
            }
        }
        if !found {
            break;
        }
        current = node;
    }
    current
}

fn media_body(root: &Value, media: &Value) -> String {
    if let Some(example) = media.get("example").filter(|v| !v.is_null()) {
        return serde_json::to_string_pretty(example).unwrap_or_default();
    }
    if let Some(examples) = media.get("examples").and_then(|v| v.as_object()) {
        for example in examples.values() {
            let value = example.get("value").unwrap_or(example);
            if !value.is_null() {
                return serde_json::to_string_pretty(value).unwrap_or_default();
            }
        }
    }
    if let Some(schema) = media.get("schema") {
        return body_from_schema(root, schema);
    }
    String::new()
}

fn body_from_schema(root: &Value, schema: &Value) -> String {
    let schema = resolve_schema(root, schema);
    if let Some(example) = schema.get("example").filter(|v| !v.is_null()) {
        return serde_json::to_string_pretty(example).unwrap_or_default();
    }
    sample_from_schema(root, schema)
        .map(|value| serde_json::to_string_pretty(&value).unwrap_or_default())
        .unwrap_or_default()
}

fn form_from_schema(root: &Value, schema: &Value) -> Vec<(String, String)> {
    let schema = resolve_schema(root, schema);
    let mut result = Vec::new();
    if let Some(properties) = schema.get("properties").and_then(|v| v.as_object()) {
        for (key, child) in properties {
            let child = resolve_schema(root, child);
            let value = if let Some(example) = child.get("example").filter(|v| !v.is_null()) {
                value_text(example)
            } else if let Some(default) = child.get("default").filter(|v| !v.is_null()) {
                value_text(default)
            } else if let Some(first) = child
                .get("enum")
                .and_then(|v| v.as_array())
                .and_then(|v| v.first())
            {
                value_text(first)
            } else {
                String::new()
            };
            result.push((key.clone(), value));
        }
    }
    result
}

fn sample_from_schema(root: &Value, schema: &Value) -> Option<Value> {
    let schema = resolve_schema(root, schema);
    if !schema.is_object() {
        return None;
    }
    if let Some(example) = schema.get("example").filter(|v| !v.is_null()) {
        return Some(example.clone());
    }
    if let Some(default) = schema.get("default").filter(|v| !v.is_null()) {
        return Some(default.clone());
    }
    if let Some(first) = schema
        .get("enum")
        .and_then(|v| v.as_array())
        .and_then(|v| v.first())
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
        .and_then(|v| v.as_str())
        .unwrap_or(fallback_type)
    {
        "object" => {
            let mut map = serde_json::Map::new();
            if let Some(properties) = schema.get("properties").and_then(|v| v.as_object()) {
                for (key, child) in properties {
                    map.insert(
                        key.clone(),
                        sample_from_schema(root, child).unwrap_or(Value::Null),
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
                .and_then(|items| sample_from_schema(root, items))
                .unwrap_or(Value::Null);
            Some(Value::Array(vec![item]))
        }
        "string" => {
            let sample = match schema.get("format").and_then(|v| v.as_str()) {
                Some("date-time") => "2024-01-01T00:00:00Z",
                Some("date") => "2024-01-01",
                Some("email") => "user@example.com",
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

// ---------- 写入数据库 ----------

fn insert_collection(
    tx: &Transaction<'_>,
    project_id: i64,
    parent_id: Option<i64>,
    collection: &ParsedCollection,
    sort_order: i64,
    now: i64,
) -> Result<(), String> {
    tx.execute(
        "INSERT INTO collections (project_id, parent_id, name, sort_order, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
        params![project_id, parent_id, collection.name, sort_order, now],
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
                "",
                "",
                "",
                index as i64,
                now,
            ],
        )
        .map_err(err)?;
    }

    for (index, child) in collection.children.iter().enumerate() {
        insert_collection(
            tx,
            project_id,
            Some(collection_id),
            child,
            index as i64,
            now,
        )?;
    }

    Ok(())
}

/// 把导入的变量写入当前激活环境；没有激活环境时新建「导入变量」环境
fn apply_variables(
    tx: &Transaction<'_>,
    project_id: i64,
    variables: &[(String, String)],
    now: i64,
) -> Result<(), String> {
    if variables.is_empty() {
        return Ok(());
    }

    let active: Option<i64> = tx
        .query_row(
            "SELECT id FROM environments WHERE project_id = ?1 AND is_active = 1 LIMIT 1",
            params![project_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(err)?;

    let env_id = match active {
        Some(id) => id,
        None => {
            tx.execute(
                "INSERT INTO environments (project_id, name, is_active, created_at)
                 VALUES (?1, ?2, 1, ?3)",
                params![project_id, "导入变量", now],
            )
            .map_err(err)?;
            tx.last_insert_rowid()
        }
    };

    let mut order: i64 = tx
        .query_row(
            "SELECT COALESCE(MAX(sort_order), -1) + 1 FROM env_variables WHERE env_id = ?1",
            params![env_id],
            |row| row.get(0),
        )
        .unwrap_or(0);

    for (key, value) in variables {
        let exists: i64 = tx
            .query_row(
                "SELECT COUNT(*) FROM env_variables WHERE env_id = ?1 AND \"key\" = ?2",
                params![env_id, key],
                |row| row.get(0),
            )
            .unwrap_or(0);
        if exists > 0 {
            continue;
        }
        tx.execute(
            "INSERT INTO env_variables (env_id, \"key\", value, is_secret, sort_order)
             VALUES (?1, ?2, ?3, 0, ?4)",
            params![env_id, key, value, order],
        )
        .map_err(err)?;
        order += 1;
    }

    Ok(())
}
