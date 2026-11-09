use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use rusqlite::{params, OptionalExtension, Transaction};
use serde_json::Value;
use tauri::State;

use crate::commands::transfer::unique_project_name;
use crate::models::{FormField, KeyValueItem, Project};
use crate::state::AppState;
use crate::util::{err, now_ms};

#[derive(Default)]
struct ParsedCollection {
    name: String,
    requests: Vec<ParsedRequest>,
    children: Vec<ParsedCollection>,
}

#[derive(Default)]
struct ParsedRequest {
    name: String,
    method: String,
    url: String,
    headers: Vec<KeyValueItem>,
    params: Vec<KeyValueItem>,
    body_type: String,
    body: String,
    /// OpenAPI 导入时由 schema 直接转成的实体；其它格式保持为空
    request_entity: Option<String>,
    response_entity: Option<String>,
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
    let path = crate::services::paths::prepare_read(&path)?;
    let content = std::fs::read_to_string(&path).map_err(|e| format!("读取文件失败：{e}"))?;
    let content = content.trim_start_matches('\u{feff}');
    let root: Value = serde_json::from_str(content).map_err(|e| format!("JSON 解析失败：{e}"))?;
    let format = crate::services::policy::import_format_from_json(&root);
    state.policy().ensure_import(format)?;
    let mut parsed = parse_import(&root)?;
    // HAR 本身没有项目名，用文件名兜底
    if parsed.name.trim().is_empty() {
        parsed.name = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().to_string())
            .unwrap_or_default();
    }
    import_parsed(state.inner(), parsed, project_id)
}

/// 已解析的导入结果写入数据库（多个入口共用：文件、URL、Bruno 目录）
fn import_parsed(
    state: &AppState,
    parsed: ParsedImport,
    project_id: Option<i64>,
) -> Result<Project, String> {
    let mut conn = state.pool.get().map_err(err)?;
    let now = now_ms();
    let tx = conn.transaction().map_err(err)?;

    let project = match project_id {
        Some(id) => tx
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
            })?,
        None => {
            let base = parsed.name.trim();
            let base = if base.is_empty() {
                "导入项目"
            } else {
                base
            };
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
    if root
        .get("log")
        .and_then(|log| log.get("entries"))
        .and_then(|entries| entries.as_array())
        .is_some()
    {
        return parse_har(root);
    }
    if root.get("_type").and_then(|v| v.as_str()) == Some("export") {
        return parse_insomnia(root);
    }
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
    Err("无法识别文件格式：请选择 Postman Collection v2.0/v2.1、Insomnia v4 导出、Apifox 导出的 OpenAPI JSON，或浏览器 / Charles 导出的 HAR".to_string())
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
            loose
                .requests
                .push(parse_postman_request(&item_name, request));
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
            folder
                .children
                .push(build_postman_folder(&item_name, children));
        } else if let Some(request) = item.get("request") {
            folder
                .requests
                .push(parse_postman_request(&item_name, request));
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
            ..Default::default()
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
        ..Default::default()
    }
}

fn parse_postman_url(value: &Value) -> (String, Vec<KeyValueItem>) {
    let raw = match value {
        Value::String(text) => text.clone(),
        Value::Object(_) => {
            optional_text(value.get("raw")).unwrap_or_else(|| build_postman_url(value))
        }
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
            (if is_json { "json" } else { "text" }.to_string(), raw)
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

// ---------- HAR ----------

/// 解析浏览器 DevTools / Charles / Fiddler 导出的 HAR 1.2：按 host 分组，
/// 每个 entry 生成一个请求，查询串拆到 params，敏感头保留占位但默认不启用。
fn parse_har(root: &Value) -> Result<ParsedImport, String> {
    let entries = root
        .get("log")
        .and_then(|log| log.get("entries"))
        .and_then(|entries| entries.as_array())
        .ok_or_else(|| "HAR 文件缺少 log.entries".to_string())?;

    let mut groups: Vec<ParsedCollection> = Vec::new();
    let mut indexes: HashMap<String, usize> = HashMap::new();

    for entry in entries {
        let request = entry.get("request").unwrap_or(&Value::Null);
        let url = optional_text(request.get("url")).unwrap_or_default();
        if url.is_empty() {
            continue;
        }

        let host = url_host(&url);
        let (base, mut params) = split_har_url(&url);
        if let Some(list) = request
            .get("queryString")
            .and_then(|value| value.as_array())
            .filter(|list| !list.is_empty())
        {
            params = list
                .iter()
                .filter_map(|item| {
                    let key = optional_text(item.get("name"))?;
                    Some(KeyValueItem {
                        key,
                        value: item.get("value").map(value_text).unwrap_or_default(),
                        enabled: true,
                    })
                })
                .collect();
        }

        let headers = parse_har_headers(request.get("headers"));
        let (body_type, body) = parse_har_body(request.get("postData"));
        let parsed = ParsedRequest {
            name: har_request_name(&base, request),
            method: optional_text(request.get("method"))
                .unwrap_or_else(|| "GET".to_string())
                .to_uppercase(),
            url: base,
            headers,
            params,
            body_type,
            body,
            ..Default::default()
        };

        let index = match indexes.get(&host) {
            Some(index) => *index,
            None => {
                let index = groups.len();
                indexes.insert(host.clone(), index);
                groups.push(ParsedCollection {
                    name: host,
                    ..Default::default()
                });
                index
            }
        };
        groups[index].requests.push(parsed);
    }

    if groups.is_empty() {
        return Err("HAR 文件中没有可导入的请求".to_string());
    }

    Ok(ParsedImport {
        name: String::new(),
        description: None,
        collections: groups,
        variables: Vec::new(),
    })
}

/// 请求名取 URL 路径最后一段，HAR 本身不记录接口名。
fn har_request_name(url: &str, request: &Value) -> String {
    let path = url.split_once("://").map(|(_, rest)| rest).unwrap_or(url);
    let path = path.split(['?', '#']).next().unwrap_or(path);
    let last = path.split('/').rfind(|part| !part.is_empty()).unwrap_or("");
    if !last.is_empty() {
        return last.to_string();
    }
    optional_text(request.get("method"))
        .map(|method| method.to_uppercase())
        .unwrap_or_else(|| "请求".to_string())
}

fn url_host(url: &str) -> String {
    let rest = url.split_once("://").map(|(_, rest)| rest).unwrap_or(url);
    let host = rest
        .split(['/', '?', '#'])
        .find(|part| !part.is_empty())
        .unwrap_or("");
    if host.is_empty() {
        "HAR 导入".to_string()
    } else {
        host.to_string()
    }
}

/// 拆出查询串；HAR 的 url 往往已经带完整 query。
fn split_har_url(url: &str) -> (String, Vec<KeyValueItem>) {
    let (base, query) = match url.split_once('?') {
        Some((base, query)) => (base.to_string(), Some(query.to_string())),
        None => (url.to_string(), None),
    };
    let base = base.split('#').next().unwrap_or("").to_string();
    let mut params = Vec::new();
    if let Some(query) = query {
        for pair in query.split('#').next().unwrap_or("").split('&') {
            if pair.is_empty() {
                continue;
            }
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            params.push(KeyValueItem {
                key: key.to_string(),
                value: value.to_string(),
                enabled: true,
            });
        }
    }
    (base, params)
}

/// 伪头（`:authority` 等）与由客户端重算的长度/连接头直接丢弃；
/// Authorization / Cookie / Proxy-Authorization 保留值但默认不启用，避免明文凭据被重放。
fn parse_har_headers(value: Option<&Value>) -> Vec<KeyValueItem> {
    let mut items = Vec::new();
    let Some(list) = value.and_then(|value| value.as_array()) else {
        return items;
    };
    for item in list {
        let key = optional_text(item.get("name")).unwrap_or_default();
        if key.is_empty() || key.starts_with(':') {
            continue;
        }
        let lower = key.to_ascii_lowercase();
        if lower == "content-length" || lower == "connection" {
            continue;
        }
        let value = item.get("value").map(value_text).unwrap_or_default();
        let sensitive = matches!(
            lower.as_str(),
            "authorization" | "cookie" | "proxy-authorization"
        );
        items.push(KeyValueItem {
            key,
            value,
            enabled: !sensitive,
        });
    }
    items
}

fn parse_har_body(post: Option<&Value>) -> (String, String) {
    let Some(post) = post else {
        return ("none".to_string(), String::new());
    };
    // base64 编码的上传体无法还原成可编辑文本，宁可不导入也不写坏
    if post.get("encoding").and_then(|value| value.as_str()) == Some("base64") {
        return ("none".to_string(), String::new());
    }
    let mime = optional_text(post.get("mimeType"))
        .unwrap_or_default()
        .to_ascii_lowercase();
    let params = post.get("params").and_then(|value| value.as_array());

    if mime.contains("multipart/form-data") {
        let fields: Vec<FormField> = params
            .map(|list| {
                list.iter()
                    .filter_map(|item| {
                        let key = optional_text(item.get("name"))?;
                        let file_name = optional_text(item.get("fileName"));
                        let is_file = file_name.is_some();
                        Some(FormField {
                            key,
                            value: file_name.unwrap_or_else(|| {
                                item.get("value").map(value_text).unwrap_or_default()
                            }),
                            // 文件字段只有文件名，不能直接当路径发送，先留占位不启用
                            enabled: !is_file,
                            kind: if is_file { "file" } else { "text" }.to_string(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        if !fields.is_empty() {
            return (
                "multipart".to_string(),
                serde_json::to_string(&fields).unwrap_or_else(|_| "[]".to_string()),
            );
        }
    }

    if mime.contains("application/x-www-form-urlencoded") {
        if let Some(list) = params {
            let pairs: Vec<String> = list
                .iter()
                .filter_map(|item| {
                    let key = optional_text(item.get("name"))?;
                    Some(format!(
                        "{key}={}",
                        item.get("value").map(value_text).unwrap_or_default()
                    ))
                })
                .collect();
            if !pairs.is_empty() {
                return ("form".to_string(), pairs.join("&"));
            }
        }
    }

    let text = optional_text(post.get("text")).unwrap_or_default();
    if text.is_empty() {
        return ("none".to_string(), String::new());
    }
    if mime.contains("json") || looks_like_json(&text) {
        return ("json".to_string(), text);
    }
    ("raw".to_string(), text)
}

// ---------- Insomnia v4 ----------

/// 解析 Insomnia v4 导出（`{"_type":"export","resources":[...]}`）：
/// workspace / request_group / request / environment 四类资源映射到集合树。
fn parse_insomnia(root: &Value) -> Result<ParsedImport, String> {
    let resources = root
        .get("resources")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "Insomnia 导出文件缺少 resources 数组".to_string())?;

    let mut workspace_name = String::new();
    let mut workspace_id = String::new();
    let mut groups: Vec<(String, Option<String>, String)> = Vec::new();
    let mut requests: Vec<(String, Option<String>, ParsedRequest)> = Vec::new();
    let mut variables: Vec<(String, String)> = Vec::new();

    for resource in resources {
        match resource.get("_type").and_then(|v| v.as_str()).unwrap_or("") {
            "workspace" => {
                workspace_name = optional_text(resource.get("name")).unwrap_or_default();
                workspace_id = optional_text(resource.get("_id")).unwrap_or_default();
            }
            "request_group" => {
                let id = optional_text(resource.get("_id")).unwrap_or_default();
                let parent = optional_text(resource.get("parentId"));
                let name =
                    optional_text(resource.get("name")).unwrap_or_else(|| "未命名分组".to_string());
                groups.push((id, parent, name));
            }
            "request" => {
                let id = optional_text(resource.get("_id")).unwrap_or_default();
                let parent = optional_text(resource.get("parentId"));
                requests.push((id, parent, parse_insomnia_request(resource)));
            }
            "environment" => {
                if let Some(data) = resource.get("data").and_then(|v| v.as_object()) {
                    for (key, value) in data {
                        let text = match value {
                            Value::Object(_) => {
                                value.get("value").map(value_text).unwrap_or_default()
                            }
                            other => value_text(other),
                        };
                        variables.push((key.clone(), text));
                    }
                }
            }
            _ => {}
        }
    }

    let name = if workspace_name.trim().is_empty() {
        "Insomnia 导入".to_string()
    } else {
        workspace_name
    };
    let root_parent = if workspace_id.is_empty() {
        None
    } else {
        Some(workspace_id)
    };

    let mut collections = build_insomnia_groups(&groups, &requests, None, root_parent.as_deref());
    // 直接挂在 workspace 根下的请求收进同名集合
    let loose: Vec<&(String, Option<String>, ParsedRequest)> = requests
        .iter()
        .filter(|(_, parent, _)| parent.is_none() || parent.as_deref() == root_parent.as_deref())
        .collect();
    if !loose.is_empty() {
        collections.push(ParsedCollection {
            name: name.clone(),
            requests: loose.iter().map(|(_, _, req)| clone_request(req)).collect(),
            children: Vec::new(),
        });
    }
    if collections.is_empty() {
        return Err("Insomnia 文件中没有可导入的接口".to_string());
    }
    Ok(ParsedImport {
        name,
        description: None,
        collections,
        variables,
    })
}

fn build_insomnia_groups(
    groups: &[(String, Option<String>, String)],
    requests: &[(String, Option<String>, ParsedRequest)],
    parent: Option<&str>,
    root_parent: Option<&str>,
) -> Vec<ParsedCollection> {
    let mut result = Vec::new();
    for (id, group_parent, name) in groups {
        let matches = match (parent, group_parent.as_deref()) {
            (None, Some(value)) => Some(value) == root_parent,
            (None, None) => false,
            (Some(parent), Some(value)) => value == parent,
            (Some(_), None) => false,
        };
        if !matches {
            continue;
        }
        let direct: Vec<ParsedRequest> = requests
            .iter()
            .filter(|(_, request_parent, _)| request_parent.as_deref() == Some(id.as_str()))
            .map(|(_, _, request)| clone_request(request))
            .collect();
        result.push(ParsedCollection {
            name: name.clone(),
            requests: direct,
            children: build_insomnia_groups(groups, requests, Some(id), root_parent),
        });
    }
    result
}

fn clone_request(request: &ParsedRequest) -> ParsedRequest {
    ParsedRequest {
        name: request.name.clone(),
        method: request.method.clone(),
        url: request.url.clone(),
        headers: request
            .headers
            .iter()
            .map(|header| KeyValueItem {
                key: header.key.clone(),
                value: header.value.clone(),
                enabled: header.enabled,
            })
            .collect(),
        params: request
            .params
            .iter()
            .map(|param| KeyValueItem {
                key: param.key.clone(),
                value: param.value.clone(),
                enabled: param.enabled,
            })
            .collect(),
        body_type: request.body_type.clone(),
        body: request.body.clone(),
        request_entity: None,
        response_entity: None,
    }
}

fn parse_insomnia_request(resource: &Value) -> ParsedRequest {
    let mut headers = Vec::new();
    if let Some(list) = resource.get("headers").and_then(|v| v.as_array()) {
        for item in list {
            let key = optional_text(item.get("name")).unwrap_or_default();
            if key.is_empty() {
                continue;
            }
            headers.push(KeyValueItem {
                key,
                value: item.get("value").map(value_text).unwrap_or_default(),
                enabled: !item
                    .get("disabled")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
            });
        }
    }
    if let Some(header) =
        insomnia_auth_header(resource.get("authentication").unwrap_or(&Value::Null))
    {
        headers.push(header);
    }

    let mut params = Vec::new();
    if let Some(list) = resource.get("parameters").and_then(|v| v.as_array()) {
        for item in list {
            let key = optional_text(item.get("name")).unwrap_or_default();
            if key.is_empty() {
                continue;
            }
            params.push(KeyValueItem {
                key,
                value: item.get("value").map(value_text).unwrap_or_default(),
                enabled: !item
                    .get("disabled")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
            });
        }
    }

    let (body_type, body) = resource
        .get("body")
        .map(parse_insomnia_body)
        .unwrap_or_else(|| ("none".to_string(), String::new()));
    ParsedRequest {
        name: optional_text(resource.get("name")).unwrap_or_else(|| "未命名".to_string()),
        method: optional_text(resource.get("method"))
            .unwrap_or_else(|| "GET".to_string())
            .to_uppercase(),
        url: optional_text(resource.get("url")).unwrap_or_default(),
        headers,
        params,
        body_type,
        body,
        request_entity: None,
        response_entity: None,
    }
}

fn insomnia_auth_header(auth: &Value) -> Option<KeyValueItem> {
    let kind = auth.get("type").and_then(|v| v.as_str()).unwrap_or("none");
    match kind {
        "bearer" => {
            let token = auth.get("token").map(value_text).unwrap_or_default();
            if token.trim().is_empty() {
                return None;
            }
            Some(KeyValueItem {
                key: "Authorization".to_string(),
                value: format!("Bearer {token}"),
                enabled: true,
            })
        }
        "basic" => {
            let username = auth.get("username").map(value_text).unwrap_or_default();
            let password = auth.get("password").map(value_text).unwrap_or_default();
            if username.is_empty() && password.is_empty() {
                return None;
            }
            Some(KeyValueItem {
                key: "Authorization".to_string(),
                value: format!(
                    "Basic {}",
                    base64_encode(format!("{username}:{password}").as_bytes())
                ),
                enabled: true,
            })
        }
        _ => None,
    }
}

fn parse_insomnia_body(body: &Value) -> (String, String) {
    let mime = body
        .get("mimeType")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let text = body
        .get("text")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let params = body.get("params").and_then(|v| v.as_array());
    if mime.contains("multipart") {
        let fields: Vec<FormField> = params
            .map(|list| {
                list.iter()
                    .filter_map(|item| {
                        let key = optional_text(item.get("name"))?;
                        let is_file = item.get("type").and_then(|v| v.as_str()) == Some("file");
                        Some(FormField {
                            key,
                            value: item.get("value").map(value_text).unwrap_or_default(),
                            enabled: !item
                                .get("disabled")
                                .and_then(|v| v.as_bool())
                                .unwrap_or(false)
                                && !is_file,
                            kind: if is_file { "file" } else { "text" }.to_string(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        if !fields.is_empty() {
            return (
                "multipart".to_string(),
                serde_json::to_string(&fields).unwrap_or_else(|_| "[]".to_string()),
            );
        }
    }
    if mime.contains("x-www-form-urlencoded") {
        let pairs: Vec<String> = params
            .map(|list| {
                list.iter()
                    .filter_map(|item| {
                        let key = optional_text(item.get("name"))?;
                        Some(format!(
                            "{key}={}",
                            item.get("value").map(value_text).unwrap_or_default()
                        ))
                    })
                    .collect()
            })
            .unwrap_or_default();
        if !pairs.is_empty() {
            return ("form".to_string(), pairs.join("&"));
        }
    }
    if text.trim().is_empty() {
        return ("none".to_string(), String::new());
    }
    if mime.contains("json") || looks_like_json(&text) {
        ("json".to_string(), text)
    } else {
        ("text".to_string(), text)
    }
}

/// 小工具：Basic 认证头需要 base64，避免为一次导入引入新依赖
fn base64_encode(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let value = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[((value >> 18) & 63) as usize] as char);
        out.push(TABLE[((value >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            TABLE[((value >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[(value & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

// ---------- Bruno（.bru 目录） ----------

/// 解析 Bruno 工作目录：目录结构映射成集合树，每个 `.bru` 文件是一个请求。
#[tauri::command]
pub fn bruno_import(
    state: State<'_, AppState>,
    path: String,
    project_id: Option<i64>,
) -> Result<Project, String> {
    state.policy().ensure_import("bruno")?;
    let root = crate::services::paths::prepare_existing_dir(&path)?;
    let name = root
        .file_name()
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_else(|| "Bruno 导入".to_string());
    let mut count = 0usize;
    let requests = scan_bru_dir(&root, &mut count)?;
    if requests.is_empty() {
        return Err("目录中没有可导入的 .bru 文件".to_string());
    }
    let collections = vec![ParsedCollection {
        name: name.clone(),
        requests: Vec::new(),
        children: requests,
    }];
    let parsed = ParsedImport {
        name,
        description: None,
        collections,
        variables: Vec::new(),
    };
    import_parsed(state.inner(), parsed, project_id)
}

/// 返回本目录下的子集合；本目录自己的请求放进第一个集合（同名目录）
fn scan_bru_dir(dir: &Path, count: &mut usize) -> Result<Vec<ParsedCollection>, String> {
    let mut entries: Vec<std::path::PathBuf> = std::fs::read_dir(dir)
        .map_err(|error| format!("读取目录失败（{}）：{error}", dir.display()))?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            !path
                .file_name()
                .and_then(|name| name.to_str())
                .map(|name| name.starts_with('.'))
                .unwrap_or(false)
        })
        .collect();
    entries.sort();

    let mut collections = Vec::new();
    let mut requests = Vec::new();
    for path in entries {
        if path.is_dir() {
            let name = path
                .file_name()
                .map(|value| value.to_string_lossy().to_string())
                .unwrap_or_else(|| "分组".to_string());
            let children = scan_bru_dir(&path, count)?;
            if !children.is_empty() {
                collections.push(ParsedCollection {
                    name,
                    requests: Vec::new(),
                    children,
                });
            }
        } else if path.extension().and_then(|value| value.to_str()) == Some("bru") {
            if let Some(request) = parse_bru_file(&path) {
                requests.push(request);
                *count += 1;
            }
        }
    }
    if !requests.is_empty() {
        let name = dir
            .file_name()
            .map(|value| value.to_string_lossy().to_string())
            .unwrap_or_else(|| "Bruno".to_string());
        collections.insert(
            0,
            ParsedCollection {
                name,
                requests,
                children: Vec::new(),
            },
        );
    }
    Ok(collections)
}

fn parse_bru_file(path: &Path) -> Option<ParsedRequest> {
    let text = std::fs::read_to_string(path).ok()?;
    let blocks = bru_blocks(&text);
    let mut name = path
        .file_stem()
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_else(|| "请求".to_string());
    let mut method = "GET".to_string();
    let mut url = String::new();
    let mut headers = Vec::new();
    let mut params = Vec::new();
    let mut body_type = "none".to_string();
    let mut body = String::new();

    for (block, content) in blocks {
        match block.as_str() {
            "meta" => {
                if let Some(value) = bru_field(&content, "name") {
                    if !value.is_empty() {
                        name = value;
                    }
                }
            }
            "headers" => headers.extend(bru_pairs(&content)),
            "params:query" => params.extend(bru_pairs(&content)),
            "body:json" => {
                if !content.trim().is_empty() {
                    body = content.trim().to_string();
                    body_type = "json".to_string();
                }
            }
            "body:text" => {
                if !content.trim().is_empty() {
                    body = content.trim().to_string();
                    body_type = "text".to_string();
                }
            }
            "body:form-urlencoded" => {
                let pairs: Vec<String> = bru_pairs(&content)
                    .into_iter()
                    .filter(|item| item.enabled)
                    .map(|item| format!("{}={}", item.key, item.value))
                    .collect();
                if !pairs.is_empty() {
                    body = pairs.join("&");
                    body_type = "form".to_string();
                }
            }
            "body:multipart-form" => {
                let fields: Vec<FormField> = bru_pairs(&content)
                    .into_iter()
                    .map(|item| FormField {
                        key: item.key,
                        value: item.value,
                        enabled: item.enabled,
                        kind: "text".to_string(),
                    })
                    .collect();
                if !fields.is_empty() {
                    body = serde_json::to_string(&fields).unwrap_or_else(|_| "[]".to_string());
                    body_type = "multipart".to_string();
                }
            }
            "auth:bearer" => {
                if let Some(token) = bru_field(&content, "token") {
                    if !token.is_empty() {
                        headers.push(KeyValueItem {
                            key: "Authorization".to_string(),
                            value: format!("Bearer {token}"),
                            enabled: true,
                        });
                    }
                }
            }
            other
                if ["get", "post", "put", "patch", "delete", "head", "options"]
                    .contains(&other) =>
            {
                method = other.to_uppercase();
                if let Some(value) = bru_field(&content, "url") {
                    url = value;
                }
            }
            _ => {}
        }
    }
    if url.trim().is_empty() {
        return None;
    }
    Some(ParsedRequest {
        name,
        method,
        url,
        headers,
        params,
        body_type,
        body,
        request_entity: None,
        response_entity: None,
    })
}

/// 拆分 Bruno 的 `name { ... }` 块，按花括号深度找结束位置
fn bru_blocks(text: &str) -> Vec<(String, String)> {
    let mut blocks = Vec::new();
    let mut current: Option<(String, String, i32)> = None;
    for line in text.lines() {
        match &mut current {
            None => {
                if let Some(open) = line.trim().strip_suffix('{') {
                    let name = open.trim();
                    if !name.is_empty()
                        && !name.contains(' ')
                        && name
                            .chars()
                            .all(|c| c.is_ascii_alphanumeric() || c == ':' || c == '-' || c == '_')
                    {
                        current = Some((name.to_string(), String::new(), 1));
                    }
                }
            }
            Some((_, content, depth)) => {
                let opens = line.matches('{').count() as i32;
                let closes = line.matches('}').count() as i32;
                *depth += opens - closes;
                if *depth <= 0 {
                    let (name, body, _) = current.take().unwrap();
                    blocks.push((name, body));
                } else {
                    content.push_str(line);
                    content.push('\n');
                }
            }
        }
    }
    blocks
}

fn bru_field(content: &str, key: &str) -> Option<String> {
    for line in content.lines() {
        let line = line.trim();
        if line.starts_with('#') {
            continue;
        }
        if let Some((name, value)) = line.split_once(':') {
            if name.trim() == key {
                return Some(value.trim().to_string());
            }
        }
    }
    None
}

fn bru_pairs(content: &str) -> Vec<KeyValueItem> {
    let mut items = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (disabled, line) = match line.strip_prefix('~') {
            Some(rest) => (true, rest.trim()),
            None => (false, line),
        };
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim();
        if key.is_empty() {
            continue;
        }
        items.push(KeyValueItem {
            key: key.to_string(),
            value: value.trim().to_string(),
            enabled: !disabled,
        });
    }
    items
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
        for method in [
            "get", "post", "put", "patch", "delete", "head", "options", "trace",
        ] {
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
            "formData" if !key.is_empty() => form_fields.push((key, param_value(root, parameter))),
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

    // G-07：schema 直接转实体，说明一并带入（其它格式没有实体结构）
    let (request_entity, response_entity) = openapi_entities(root, operation, is_v3);

    ParsedRequest {
        name,
        method: method.to_uppercase(),
        url,
        headers,
        params,
        body_type,
        body,
        request_entity,
        response_entity,
    }
}

/// 从 OpenAPI 操作里取出请求 / 响应 schema 并转成实体
fn openapi_entities(
    root: &Value,
    operation: &Value,
    is_v3: bool,
) -> (Option<String>, Option<String>) {
    let resolve = |value: &Value| resolve_schema(root, value).clone();
    let encode = |schema: Option<&Value>| {
        schema
            .and_then(|schema| crate::entity::schema::entity_from_schema(schema, &resolve))
            .and_then(|doc| doc.encode().ok())
    };

    let request_schema = if is_v3 {
        operation.get("requestBody").and_then(json_content_schema)
    } else {
        operation
            .get("parameters")
            .and_then(|v| v.as_array())
            .and_then(|list| {
                list.iter()
                    .find(|param| param.get("in").and_then(|v| v.as_str()) == Some("body"))
            })
            .and_then(|param| param.get("schema"))
    };

    let response_schema = operation
        .get("responses")
        .and_then(|v| v.as_object())
        .and_then(|map| {
            map.get("200").or_else(|| map.get("201")).or_else(|| {
                map.iter()
                    .find(|(code, _)| code.starts_with('2'))
                    .map(|(_, value)| value)
            })
        })
        .and_then(|response| {
            if is_v3 {
                json_content_schema(response)
            } else {
                response.get("schema")
            }
        });

    (encode(request_schema), encode(response_schema))
}

/// 从 content 节点里找 JSON media 的 schema（优先 application/json）
fn json_content_schema(node: &Value) -> Option<&Value> {
    let content = node.get("content")?.as_object()?;
    if let Some(media) = content.get("application/json") {
        return media.get("schema");
    }
    content
        .iter()
        .find(|(name, _)| name.to_ascii_lowercase().contains("json"))
        .and_then(|(_, media)| media.get("schema"))
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
                 pre_script, post_script, assertions, request_entity, response_entity,
                 sort_order, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?15)",
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
                request.request_entity.as_deref().unwrap_or(""),
                request.response_entity.as_deref().unwrap_or(""),
                index as i64,
                now,
            ],
        )
        .map_err(err)?;
        crate::services::revision::record_in_tx(tx, tx.last_insert_rowid(), "import", None)?;
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

// ---------- OpenAPI URL 拉取（G-07） ----------

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenApiUrlPayload {
    pub url: String,
    #[serde(default)]
    pub headers: Vec<KeyValueItem>,
    #[serde(default)]
    pub project_id: Option<i64>,
    /// 保存来源，便于后续刷新（写入 openapi_sources）
    #[serde(default)]
    pub save: bool,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenApiSource {
    pub id: i64,
    pub project_id: i64,
    pub url: String,
    pub headers: Vec<KeyValueItem>,
    pub updated_at: i64,
}

/// 拉取 OpenAPI 地址并导入；保存来源时记录 URL 与 Header 供刷新
#[tauri::command]
pub async fn openapi_import_url(
    state: State<'_, AppState>,
    payload: OpenApiUrlPayload,
) -> Result<Project, String> {
    state.policy().ensure_import("openapi")?;
    let root = fetch_openapi(state.inner(), &payload.url, &payload.headers).await?;
    let parsed = parse_openapi(&root)?;
    let project = import_parsed(state.inner(), parsed, payload.project_id)?;
    if payload.save {
        let conn = state.pool.get().map_err(err)?;
        conn.execute(
            "INSERT INTO openapi_sources (project_id, url, headers_json, updated_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                project.id,
                payload.url.trim(),
                serde_json::to_string(&payload.headers).map_err(err)?,
                now_ms()
            ],
        )
        .map_err(err)?;
    }
    Ok(project)
}

#[tauri::command]
pub fn openapi_source_list(
    state: State<'_, AppState>,
    project_id: i64,
) -> Result<Vec<OpenApiSource>, String> {
    let conn = state.pool.get().map_err(err)?;
    let mut stmt = conn
        .prepare(
            "SELECT id, project_id, url, headers_json, updated_at
               FROM openapi_sources WHERE project_id = ?1 ORDER BY id DESC",
        )
        .map_err(err)?;
    let rows = stmt
        .query_map(params![project_id], |row| {
            let raw: Option<String> = row.get(3)?;
            Ok(OpenApiSource {
                id: row.get(0)?,
                project_id: row.get(1)?,
                url: row.get(2)?,
                headers: raw
                    .and_then(|text| serde_json::from_str(&text).ok())
                    .unwrap_or_default(),
                updated_at: row.get::<_, Option<i64>>(4)?.unwrap_or(0),
            })
        })
        .map_err(err)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(err)?;
    Ok(rows)
}

#[tauri::command]
pub fn openapi_source_delete(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    let conn = state.pool.get().map_err(err)?;
    conn.execute("DELETE FROM openapi_sources WHERE id = ?1", params![id])
        .map_err(err)?;
    Ok(())
}

/// 按保存的来源重新拉取并导入同一项目（生成的请求会追加到现有集合）
#[tauri::command]
pub async fn openapi_source_refresh(
    state: State<'_, AppState>,
    id: i64,
) -> Result<Project, String> {
    state.policy().ensure_import("openapi")?;
    let (project_id, url, headers) = {
        let conn = state.pool.get().map_err(err)?;
        conn.query_row(
            "SELECT project_id, url, headers_json FROM openapi_sources WHERE id = ?1",
            params![id],
            |row| {
                let raw: Option<String> = row.get(2)?;
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    raw.and_then(|text| serde_json::from_str::<Vec<KeyValueItem>>(&text).ok())
                        .unwrap_or_default(),
                ))
            },
        )
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => "来源不存在".to_string(),
            other => err(other),
        })?
    };

    let root = fetch_openapi(state.inner(), &url, &headers).await?;
    let parsed = parse_openapi(&root)?;
    let project = import_parsed(state.inner(), parsed, Some(project_id))?;
    let conn = state.pool.get().map_err(err)?;
    conn.execute(
        "UPDATE openapi_sources SET updated_at = ?1 WHERE id = ?2",
        params![now_ms(), id],
    )
    .map_err(err)?;
    Ok(project)
}

async fn fetch_openapi(
    state: &AppState,
    url: &str,
    headers: &[KeyValueItem],
) -> Result<Value, String> {
    let url = url.trim();
    if url.is_empty() {
        return Err("OpenAPI 地址为空".to_string());
    }
    let mut request = state
        .client(false)
        .get(url)
        .timeout(Duration::from_secs(30));
    for header in headers.iter().filter(|header| header.enabled) {
        if header.key.trim().is_empty() {
            continue;
        }
        request = request.header(header.key.trim(), header.value.as_str());
    }
    let response = request
        .send()
        .await
        .map_err(|error| format!("拉取 OpenAPI 失败：{error}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("拉取 OpenAPI 失败：HTTP {}", status.as_u16()));
    }
    let text = response
        .text()
        .await
        .map_err(|error| format!("读取 OpenAPI 响应失败：{error}"))?;
    let text = text.trim_start_matches('\u{feff}');
    serde_json::from_str(text).map_err(|error| {
        format!("仅支持 JSON 格式的 OpenAPI / Swagger 文档（YAML 请先转换为 JSON）：{error}")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn har_groups_by_host_and_splits_query() {
        let har = json!({
            "log": {
                "entries": [
                    {
                        "request": {
                            "method": "GET",
                            "url": "https://api.example.com/orders?page=2&size=10",
                            "headers": [
                                {"name": "Authorization", "value": "Bearer token"},
                                {"name": ":authority", "value": "api.example.com"}
                            ]
                        }
                    },
                    {
                        "request": {
                            "method": "POST",
                            "url": "https://api.example.com/orders",
                            "headers": [{"name": "Content-Type", "value": "application/json"}],
                            "postData": {
                                "mimeType": "application/json",
                                "text": "{\"sku\":\"A1\"}"
                            }
                        }
                    },
                    {
                        "request": {
                            "method": "GET",
                            "url": "https://cdn.example.com/assets/app.js",
                            "headers": []
                        }
                    }
                ]
            }
        });

        let parsed = parse_import(&har).unwrap();
        assert_eq!(parsed.collections.len(), 2);

        let api = parsed
            .collections
            .iter()
            .find(|collection| collection.name == "api.example.com")
            .unwrap();
        assert_eq!(api.requests.len(), 2);

        let get = api
            .requests
            .iter()
            .find(|request| request.method == "GET")
            .unwrap();
        assert_eq!(get.url, "https://api.example.com/orders");
        assert_eq!(get.name, "orders");
        assert_eq!(get.params.len(), 2);
        let auth = get
            .headers
            .iter()
            .find(|header| header.key == "Authorization")
            .unwrap();
        assert!(!auth.enabled);
        assert!(get
            .headers
            .iter()
            .all(|header| !header.key.starts_with(':')));

        let post = api
            .requests
            .iter()
            .find(|request| request.method == "POST")
            .unwrap();
        assert_eq!(post.body_type, "json");
        assert_eq!(post.body, "{\"sku\":\"A1\"}");

        let cdn = parsed
            .collections
            .iter()
            .find(|collection| collection.name == "cdn.example.com")
            .unwrap();
        assert_eq!(cdn.requests[0].name, "app.js");
    }

    #[test]
    fn import_records_revision_with_import_source() {
        use r2d2_sqlite::SqliteConnectionManager;

        let dir = crate::util::test_dir("importer-revision");
        let manager = SqliteConnectionManager::file(dir.join("astroman.db"));
        let pool = r2d2::Pool::builder().max_size(2).build(manager).unwrap();
        crate::db::init(&pool).unwrap();
        let mut conn = pool.get().unwrap();
        let now = now_ms();
        conn.execute(
            "INSERT INTO projects (name, created_at, updated_at) VALUES ('P', ?1, ?1)",
            params![now],
        )
        .unwrap();
        let project = conn.last_insert_rowid();

        let collection = ParsedCollection {
            name: "导入集合".into(),
            requests: vec![ParsedRequest {
                name: "详情".into(),
                method: "GET".into(),
                url: "https://api.example.com/users/{id}".into(),
                headers: Vec::new(),
                params: Vec::new(),
                body_type: "none".into(),
                body: String::new(),
                ..Default::default()
            }],
            ..Default::default()
        };

        let tx = conn.transaction().unwrap();
        insert_collection(&tx, project, None, &collection, 0, now).unwrap();
        tx.commit().unwrap();

        let (source, count): (String, i64) = conn
            .query_row(
                "SELECT source, (SELECT COUNT(*) FROM request_revisions)
                   FROM request_revisions LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(source, "import");
        assert_eq!(count, 1);
    }

    #[test]
    fn har_form_and_multipart_bodies() {
        let form = json!({
            "mimeType": "application/x-www-form-urlencoded",
            "params": [
                {"name": "a", "value": "1"},
                {"name": "b", "value": "2"}
            ]
        });
        assert_eq!(
            parse_har_body(Some(&form)),
            ("form".to_string(), "a=1&b=2".to_string())
        );

        let multipart = json!({
            "mimeType": "multipart/form-data",
            "params": [
                {"name": "note", "value": "hi"},
                {"name": "file", "fileName": "report.pdf"}
            ]
        });
        let (kind, body) = parse_har_body(Some(&multipart));
        assert_eq!(kind, "multipart");
        let fields: Vec<FormField> = serde_json::from_str(&body).unwrap();
        assert_eq!(fields.len(), 2);
        assert!(fields
            .iter()
            .any(|field| field.key == "note" && field.enabled && field.kind == "text"));
        assert!(fields.iter().any(|field| {
            field.key == "file"
                && !field.enabled
                && field.kind == "file"
                && field.value == "report.pdf"
        }));
    }

    #[test]
    fn insomnia_maps_groups_requests_and_auth() {
        let root = json!({
            "_type": "export",
            "resources": [
                {"_type": "workspace", "_id": "wrk_1", "name": "演示工作区"},
                {"_type": "request_group", "_id": "fld_1", "parentId": "wrk_1", "name": "用户"},
                {
                    "_type": "request", "_id": "req_1", "parentId": "fld_1",
                    "name": "创建用户", "method": "POST",
                    "url": "https://api.example.com/users",
                    "headers": [{"name": "Content-Type", "value": "application/json"}],
                    "parameters": [{"name": "debug", "value": "1"}],
                    "body": {"mimeType": "application/json", "text": "{\"name\":\"张三\"}"},
                    "authentication": {"type": "bearer", "token": "t-1"}
                },
                {
                    "_type": "request", "_id": "req_2", "parentId": "wrk_1",
                    "name": "列表", "method": "GET",
                    "url": "https://api.example.com/users",
                    "headers": [], "parameters": []
                },
                {
                    "_type": "environment", "_id": "env_1", "parentId": "wrk_1",
                    "data": {"base_url": {"value": "https://api.example.com"}, "page": "1"}
                }
            ]
        });

        let parsed = parse_import(&root).unwrap();
        assert_eq!(parsed.name, "演示工作区");
        assert_eq!(parsed.variables.len(), 2);
        let group = parsed
            .collections
            .iter()
            .find(|collection| collection.name == "用户")
            .unwrap();
        assert_eq!(group.requests.len(), 1);
        let request = &group.requests[0];
        assert_eq!(request.name, "创建用户");
        assert_eq!(request.method, "POST");
        assert_eq!(request.body_type, "json");
        assert!(request.body.contains("张三"));
        assert!(request
            .headers
            .iter()
            .any(|header| header.key == "Authorization" && header.value == "Bearer t-1"));

        let loose = parsed
            .collections
            .iter()
            .find(|collection| collection.name == "演示工作区")
            .unwrap();
        assert_eq!(loose.requests[0].name, "列表");
    }

    #[test]
    fn bruno_blocks_and_request_are_parsed() {
        let text = r#"
meta {
  name: 创建用户
  type: http
  seq: 1
}

post {
  url: {{base_url}}/users
  body: json
  auth: bearer
}

headers {
  Content-Type: application/json
}

params:query {
  page: 2
  ~debug: 1
}

body:json {
  {
    "name": "张三"
  }
}

auth:bearer {
  token: abc
}
"#;
        let blocks = bru_blocks(text);
        assert!(blocks.iter().any(|(name, _)| name == "body:json"));
        assert!(blocks.iter().any(|(name, _)| name == "post"));

        let dir = crate::util::test_dir("bruno-parse");
        let file = dir.join("create.bru");
        std::fs::write(&file, text).unwrap();
        let request = parse_bru_file(&file).unwrap();
        assert_eq!(request.name, "创建用户");
        assert_eq!(request.method, "POST");
        assert_eq!(request.url, "{{base_url}}/users");
        assert_eq!(request.body_type, "json");
        assert!(request.body.contains("\"name\": \"张三\""));
        assert!(request
            .params
            .iter()
            .any(|param| param.key == "page" && param.enabled));
        assert!(request
            .params
            .iter()
            .any(|param| param.key == "debug" && !param.enabled));
        assert!(request
            .headers
            .iter()
            .any(|header| header.key == "Authorization" && header.value == "Bearer abc"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn openapi_import_carries_entity_descriptions() {
        let spec = json!({
            "openapi": "3.0.0",
            "info": {"title": "实体说明"},
            "paths": {
                "/users": {
                    "post": {
                        "requestBody": {
                            "content": {
                                "application/json": {
                                    "schema": {"$ref": "#/components/schemas/User"}
                                }
                            }
                        },
                        "responses": {
                            "200": {
                                "content": {
                                    "application/json": {
                                        "schema": {
                                            "type": "object",
                                            "properties": {
                                                "id": {"type": "integer", "description": "用户编号"}
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            },
            "components": {
                "schemas": {
                    "User": {
                        "type": "object",
                        "required": ["name"],
                        "properties": {
                            "name": {"type": "string", "description": "昵称"}
                        }
                    }
                }
            }
        });

        let parsed = parse_import(&spec).unwrap();
        let request = &parsed.collections[0].requests[0];
        let request_doc =
            crate::entity::model::EntityDoc::decode(request.request_entity.as_deref().unwrap())
                .unwrap();
        let name = request_doc
            .flatten()
            .into_iter()
            .find(|field| field.path == "name")
            .unwrap();
        assert_eq!(name.type_name, "string");
        assert!(name.required);
        assert_eq!(name.description, "昵称");

        let response_doc =
            crate::entity::model::EntityDoc::decode(request.response_entity.as_deref().unwrap())
                .unwrap();
        let id = response_doc
            .flatten()
            .into_iter()
            .find(|field| field.path == "id")
            .unwrap();
        assert_eq!(id.description, "用户编号");
    }

    #[test]
    fn har_base64_body_is_skipped() {
        let post = json!({
            "mimeType": "application/octet-stream",
            "encoding": "base64",
            "text": "AAAA"
        });
        assert_eq!(
            parse_har_body(Some(&post)),
            ("none".to_string(), String::new())
        );
    }
}
