use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[cfg(test)]
use serde_json::json;
use tauri::State;

use crate::models::{KeyValueItem, Project};
use crate::services::openapi::build_openapi_spec;
use crate::state::AppState;
use crate::util::{err, now_ms};

/// 导出使用的格式标记
const FORMAT: &str = "astroman.project";
/// 改名前的格式标记（FbinHttp / FinkMeter 时期），仍需接受，保证旧导出文件可以导入
const LEGACY_FORMATS: [&str; 2] = ["fbinhttp.project", "finkmeter.project"];
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
    /// 仅导出时用于查最近一次响应对应，不进入项目包 JSON
    #[serde(skip)]
    id: i64,
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
    /// 实体列随项目包迁移；旧包缺列时按空实体处理
    #[serde(default)]
    request_entity: Option<String>,
    #[serde(default)]
    response_entity: Option<String>,
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
            "SELECT id, name, method, url, headers, params, body_type, body, pre_script, post_script,
                    assertions, request_entity, response_entity
             FROM requests WHERE collection_id = ?1
             ORDER BY sort_order ASC, id ASC",
        )
        .map_err(err)?;
    let rows = stmt
        .query_map(params![collection_id], |row| {
            Ok(ExportRequest {
                id: row.get(0)?,
                name: row.get(1)?,
                method: row.get(2)?,
                url: row.get(3)?,
                headers: parse_items(row.get(4)?),
                params: parse_items(row.get(5)?),
                body_type: row
                    .get::<_, Option<String>>(6)?
                    .unwrap_or_else(default_body_type),
                body: row.get(7)?,
                pre_script: row.get(8)?,
                post_script: row.get(9)?,
                assertions: row.get(10)?,
                request_entity: row.get(11)?,
                response_entity: row.get(12)?,
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
                let is_secret = row.get::<_, Option<i64>>(2)?.unwrap_or(0) == 1;
                let stored: Option<String> = row.get(1)?;
                // 导出是「换机搬迁」语义，密钥必须还原成明文，否则导入方解不开；
                // 这也意味着项目包本身要当敏感文件保管（设置页有提示）
                let value = if is_secret {
                    stored
                        .as_deref()
                        .map(|v| crate::secret::decrypt(crate::secret::AAD_ENV, v))
                } else {
                    stored
                };
                Ok(ExportVariable {
                    key: row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                    value,
                    is_secret,
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
    collection_rows.sort_by_key(|(id, parent, _, sort)| (parent.unwrap_or(0), *sort, *id));

    let bundle = ExportBundle {
        format: FORMAT.to_string(),
        version: VERSION,
        exported_at: now_ms(),
        project,
        collections: build_collections(&collection_rows, None, &conn)?,
        environments: read_environments(&conn, project_id)?,
    };

    let json = serde_json::to_string_pretty(&bundle).map_err(err)?;
    let path = crate::services::paths::prepare_write(&path)?;
    std::fs::write(&path, json).map_err(|e| format!("写入文件失败（{}）：{e}", path.display()))?;
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
        params![
            project_id,
            parent_id,
            collection.name,
            collection.sort_order,
            now
        ],
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
                request.pre_script,
                request.post_script,
                request.assertions,
                request.request_entity.as_deref().unwrap_or(""),
                request.response_entity.as_deref().unwrap_or(""),
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
    let path = crate::services::paths::prepare_read(&path)?;
    let content = std::fs::read_to_string(&path)
        .map_err(|e| format!("读取文件失败（{}）：{e}", path.display()))?;
    let bundle: ExportBundle =
        serde_json::from_str(&content).map_err(|e| format!("文件格式不正确：{e}"))?;

    if bundle.format != FORMAT && !LEGACY_FORMATS.contains(&bundle.format.as_str()) {
        return Err("这不是 AstroMan 项目文件".to_string());
    }
    if bundle.version > VERSION {
        return Err("项目文件版本高于当前应用，请升级后再导入".to_string());
    }

    let mut conn = state.pool.get().map_err(err)?;
    let now = now_ms();
    let raw_name = bundle.project.name.trim();
    let base_name = if raw_name.is_empty() {
        "导入项目"
    } else {
        raw_name
    };
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
            // 导入的敏感值重新加密入库（项目包内是明文，落库必须是密文）
            let stored = match (&variable.value, variable.is_secret) {
                (Some(value), true) if !value.is_empty() => {
                    crate::secret::encrypt(crate::secret::AAD_ENV, value)?
                }
                (value, _) => value.clone().unwrap_or_default(),
            };
            tx.execute(
                "INSERT INTO env_variables (env_id, \"key\", value, is_secret, sort_order)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    env_id,
                    variable.key,
                    stored,
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

// ---------- Postman Collection v2.1 导出 ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PostmanExportReport {
    pub requests: usize,
    pub collections: usize,
    /// 导不出的内容（脚本、文件字段等），导回后需要手工补
    pub skipped: Vec<String>,
}

/// 导出 Postman Collection v2.1：集合树、变量、请求头 / 查询 / Body 与最近一次
/// 响应对应保留。字段说明来自实体，写进 request.description。
/// 响应体与敏感头按导出脱敏规则处理，不输出真实凭据。
#[tauri::command]
pub fn postman_export(
    state: State<'_, AppState>,
    project_id: i64,
    path: String,
) -> Result<PostmanExportReport, String> {
    let conn = state.pool.get().map_err(err)?;
    let (project_name, project_description) = conn
        .query_row(
            "SELECT name, description FROM projects WHERE id = ?1",
            params![project_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
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
    collection_rows.sort_by_key(|(id, parent, _, sort)| (parent.unwrap_or(0), *sort, *id));

    let mut secrets = crate::docs::masking::SecretSet::default();
    {
        let mut stmt = conn
            .prepare(
                "SELECT v.value FROM env_variables v
                   JOIN environments e ON e.id = v.env_id
                  WHERE e.project_id = ?1 AND v.is_secret = 1",
            )
            .map_err(err)?;
        let values = stmt
            .query_map(params![project_id], |row| row.get::<_, Option<String>>(0))
            .map_err(err)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(err)?;
        for stored in values.into_iter().flatten() {
            let plain = crate::secret::decrypt(crate::secret::AAD_ENV, &stored);
            if !plain.is_empty() {
                secrets.add(&plain);
            }
        }
    }

    let mut report = PostmanExportReport {
        requests: 0,
        collections: collection_rows.len(),
        skipped: Vec::new(),
    };
    let items = postman_items(&conn, &collection_rows, None, &secrets, &mut report)?;
    let variables = postman_variables(&conn, project_id)?;

    let collection = serde_json::json!({
        "info": {
            "name": project_name,
            "description": project_description.unwrap_or_default(),
            "schema": "https://schema.getpostman.com/json/collection/v2.1.0/collection.json",
        },
        "item": items,
        "variable": variables,
    });
    let text = serde_json::to_string_pretty(&collection).map_err(err)?;
    let path = crate::services::paths::prepare_write(&path)?;
    std::fs::write(&path, text).map_err(|e| format!("写入文件失败（{}）：{e}", path.display()))?;
    Ok(report)
}

fn postman_items(
    conn: &Connection,
    rows: &[(i64, Option<i64>, String, i64)],
    parent: Option<i64>,
    secrets: &crate::docs::masking::SecretSet,
    report: &mut PostmanExportReport,
) -> Result<Vec<Value>, String> {
    let mut items = Vec::new();
    for (id, parent_id, name, _) in rows {
        if *parent_id != parent {
            continue;
        }
        let mut item = serde_json::json!({ "name": name });
        let children = postman_items(conn, rows, Some(*id), secrets, report)?;
        if !children.is_empty() {
            item["item"] = Value::Array(children);
        }
        let requests = read_requests(conn, *id)?;
        if !requests.is_empty() {
            let mut list = Vec::new();
            for request in &requests {
                list.push(postman_request(conn, request, secrets, report)?);
            }
            item["item"] = Value::Array(list);
        }
        items.push(item);
    }
    Ok(items)
}

fn postman_request(
    conn: &Connection,
    request: &ExportRequest,
    secrets: &crate::docs::masking::SecretSet,
    report: &mut PostmanExportReport,
) -> Result<Value, String> {
    report.requests += 1;
    let headers: Vec<Value> = request
        .headers
        .iter()
        .map(|header| {
            let mut item = serde_json::json!({
                "key": header.key,
                "value": header.value,
            });
            if !header.enabled {
                item["disabled"] = Value::Bool(true);
            }
            item
        })
        .collect();

    let mut url = serde_json::json!({ "raw": request.url });
    if !request.params.is_empty() {
        let query: Vec<Value> = request
            .params
            .iter()
            .map(|param| {
                let mut item = serde_json::json!({
                    "key": param.key,
                    "value": param.value,
                });
                if !param.enabled {
                    item["disabled"] = Value::Bool(true);
                }
                item
            })
            .collect();
        url["query"] = Value::Array(query);
        let enabled: Vec<String> = request
            .params
            .iter()
            .filter(|param| param.enabled)
            .map(|param| format!("{}={}", param.key, param.value))
            .collect();
        if !enabled.is_empty() && !request.url.contains('?') {
            url["raw"] = Value::String(format!(
                "{}?{}",
                request.url.trim_end_matches('&'),
                enabled.join("&")
            ));
        }
    }

    let mut entry = serde_json::json!({
        "name": request.name,
        "request": {
            "method": request.method,
            "header": headers,
            "url": url,
        },
    });
    if let Some(body) = postman_body(request, report) {
        entry["request"]["body"] = body;
    }
    let description = entity_description(
        request.request_entity.as_deref(),
        request.response_entity.as_deref(),
    );
    if !description.is_empty() {
        entry["request"]["description"] = Value::String(description);
    }
    let has_script =
        |script: Option<&str>| script.map(|text| !text.trim().is_empty()).unwrap_or(false);
    if has_script(request.pre_script.as_deref()) || has_script(request.post_script.as_deref()) {
        report
            .skipped
            .push(format!("{}：前置 / 后置脚本未导出", request.name));
    }
    if let Some(response) = postman_response(conn, request, secrets)? {
        entry["response"] = Value::Array(vec![response]);
    }
    Ok(entry)
}

fn postman_body(request: &ExportRequest, report: &mut PostmanExportReport) -> Option<Value> {
    let body = request.body.as_deref().unwrap_or("").trim();
    match request.body_type.as_str() {
        "json" if !body.is_empty() => Some(serde_json::json!({
            "mode": "raw",
            "raw": body,
            "options": { "raw": { "language": "json" } },
        })),
        "text" | "raw" if !body.is_empty() => Some(serde_json::json!({
            "mode": "raw",
            "raw": body,
            "options": { "raw": { "language": "text" } },
        })),
        "form" if !body.is_empty() => {
            let fields: Vec<Value> = body
                .split('&')
                .filter(|pair| !pair.is_empty())
                .map(|pair| {
                    let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
                    serde_json::json!({ "key": key, "value": value })
                })
                .collect();
            Some(serde_json::json!({ "mode": "urlencoded", "urlencoded": fields }))
        }
        "multipart" if !body.is_empty() => {
            let fields: Vec<crate::models::FormField> =
                serde_json::from_str(body).unwrap_or_default();
            let mut list = Vec::new();
            for field in fields {
                if field.kind == "file" {
                    report
                        .skipped
                        .push(format!("{}：文件字段 {} 未导出", request.name, field.key));
                    continue;
                }
                list.push(serde_json::json!({ "key": field.key, "value": field.value }));
            }
            if list.is_empty() {
                None
            } else {
                Some(serde_json::json!({ "mode": "formdata", "formdata": list }))
            }
        }
        _ => None,
    }
}

/// 最近一次成功 / 失败的运行记录转成 Postman 示例，先脱敏再写出
fn postman_response(
    conn: &Connection,
    request: &ExportRequest,
    secrets: &crate::docs::masking::SecretSet,
) -> Result<Option<Value>, String> {
    if request.id <= 0 {
        return Ok(None);
    }
    let row = conn
        .query_row(
            "SELECT response_status, response_status_text, response_headers, response_body
               FROM request_runs
              WHERE request_id = ?1 AND response_status > 0
              ORDER BY id DESC LIMIT 1",
            params![request.id],
            |row| {
                Ok((
                    row.get::<_, Option<i64>>(0)?.unwrap_or(0),
                    row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                    row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                    row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                ))
            },
        )
        .optional()
        .map_err(err)?;
    let Some((status, status_text, raw_headers, raw_body)) = row else {
        return Ok(None);
    };
    if status <= 0 {
        return Ok(None);
    }
    let headers: Vec<Value> = parse_items(Some(raw_headers))
        .into_iter()
        .map(|header| {
            let value = if crate::docs::masking::is_sensitive_name(&header.key) {
                crate::docs::model::MASK.to_string()
            } else {
                header.value
            };
            serde_json::json!({ "key": header.key, "value": value })
        })
        .collect();
    let body = if raw_body.trim().is_empty() {
        String::new()
    } else {
        secrets.mask_json(&raw_body)
    };
    Ok(Some(serde_json::json!({
        "name": format!("示例响应 {status}"),
        "status": status_text,
        "code": status,
        "header": headers,
        "body": body,
        "_postman_previewlanguage": "json",
    })))
}

fn postman_variables(conn: &Connection, project_id: i64) -> Result<Vec<Value>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT v.\"key\", v.value, v.is_secret FROM env_variables v
               JOIN environments e ON e.id = v.env_id
              WHERE e.project_id = ?1
              ORDER BY e.is_active DESC, v.sort_order ASC, v.id ASC",
        )
        .map_err(err)?;
    let rows = stmt
        .query_map(params![project_id], |row| {
            Ok((
                row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                row.get::<_, Option<i64>>(2)?.unwrap_or(0) == 1,
            ))
        })
        .map_err(err)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(err)?;
    let mut seen = std::collections::HashSet::new();
    let mut values = Vec::new();
    for (key, stored, is_secret) in rows {
        if key.is_empty() || !seen.insert(key.clone()) {
            continue;
        }
        let value = if is_secret {
            crate::secret::decrypt(crate::secret::AAD_ENV, &stored)
        } else {
            stored
        };
        values.push(serde_json::json!({ "key": key, "value": value }));
    }
    Ok(values)
}

fn entity_description(request_entity: Option<&str>, response_entity: Option<&str>) -> String {
    fn section(title: &str, raw: Option<&str>) -> String {
        let Some(doc) = raw
            .and_then(|raw| crate::entity::model::EntityDoc::decode(raw).ok())
            .filter(|doc| !doc.is_empty())
        else {
            return String::new();
        };
        let lines: Vec<String> = doc
            .flatten()
            .into_iter()
            .filter(|field| !field.description.trim().is_empty() && !field.stale)
            .map(|field| format!("{}：{}", field.path, field.description.trim()))
            .collect();
        if lines.is_empty() {
            String::new()
        } else {
            format!("{title}\n{}", lines.join("\n"))
        }
    }
    let parts = [
        section("请求字段", request_entity),
        section("响应字段", response_entity),
    ];
    let joined = parts
        .iter()
        .filter(|part| !part.is_empty())
        .cloned()
        .collect::<Vec<_>>()
        .join("\n\n");
    joined
}

// ---------- OpenAPI 3.0 导出 ----------

#[tauri::command]
pub fn openapi_export(
    state: State<'_, AppState>,
    project_id: i64,
    path: String,
) -> Result<(), String> {
    let conn = state.pool.get().map_err(err)?;
    let spec = build_openapi_spec(&conn, project_id)?;
    let text = serde_json::to_string_pretty(&spec).map_err(err)?;
    let path = crate::services::paths::prepare_write(&path)?;
    std::fs::write(&path, text).map_err(|e| format!("写入文件失败（{}）：{e}", path.display()))?;
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
    state.policy().ensure_import("openapi")?;
    let path = crate::services::paths::prepare_read(&path)?;
    let content = std::fs::read_to_string(&path)
        .map_err(|e| format!("读取文件失败（{}）：{e}", path.display()))?;
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
        let host = spec
            .get("host")
            .map(|v| value_str(Some(v)))
            .unwrap_or_default();
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
            .query_row(
                "SELECT COUNT(*) FROM projects WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .map_err(err)?;
        if exists == 0 {
            return Err("目标项目不存在".to_string());
        }
        id
    } else {
        let base_name = if title.trim().is_empty() {
            "OpenAPI 导入"
        } else {
            title.trim()
        };
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
        .query_row(
            "SELECT name FROM projects WHERE id = ?1",
            params![project_id],
            |row| row.get(0),
        )
        .map_err(err)?;

    // 先按 tag 分组，再按路径顺序写入集合
    let methods = ["get", "post", "put", "patch", "delete", "head", "options"];
    // 一行待写入的接口：(name, method, url, body_type, query, headers, body)
    type PendingRequest = (
        String,
        String,
        String,
        String,
        Vec<KeyValueItem>,
        Vec<KeyValueItem>,
        String,
    );
    let mut groups: Vec<(String, Vec<PendingRequest>)> = Vec::new();

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
            let parameter_sets = [item.get("parameters"), operation.get("parameters")];
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

    for (collection_order, (group_name, requests)) in groups.into_iter().enumerate() {
        let collection_order = collection_order as i64;
        tx.execute(
            "INSERT INTO collections (project_id, parent_id, name, sort_order, created_at, updated_at)
             VALUES (?1, NULL, ?2, ?3, ?4, ?4)",
            params![project_id, group_name, collection_order, now],
        )
        .map_err(err)?;
        let collection_id = tx.last_insert_rowid();

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::entity::model::{EntityDoc, EntityNode};
    use r2d2_sqlite::SqliteConnectionManager;

    fn pool(tag: &str) -> crate::state::DbPool {
        let dir = crate::util::test_dir(tag);
        let manager = SqliteConnectionManager::file(dir.join("astroman.db"));
        let pool = r2d2::Pool::builder().max_size(2).build(manager).unwrap();
        db::init(&pool).unwrap();
        pool
    }

    fn node(name: &str, type_name: &str) -> EntityNode {
        EntityNode {
            name: name.to_string(),
            type_name: type_name.to_string(),
            ..EntityNode::default()
        }
    }

    fn entity(children: Vec<EntityNode>) -> String {
        let root = EntityNode {
            children,
            ..EntityNode::new_root("object")
        };
        EntityDoc::new(root, 0).encode().unwrap()
    }

    #[test]
    fn openapi_includes_entity_schemas_and_keeps_examples() {
        let pool = pool("openapi-entity");
        let conn = pool.get().unwrap();
        let now = now_ms();
        conn.execute(
            "INSERT INTO projects (name, created_at, updated_at) VALUES ('P', ?1, ?1)",
            params![now],
        )
        .unwrap();
        let project = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO collections (project_id, name, created_at, updated_at)
             VALUES (?1, 'C', ?2, ?2)",
            params![project, now],
        )
        .unwrap();
        let collection = conn.last_insert_rowid();
        let mut title = node("title", "string");
        title.required = true;
        title.description = "标题".into();
        let mut data = node("data", "string");
        data.description = "返回数据".into();
        let request_entity = entity(vec![title]);
        let response_entity = entity(vec![data]);
        conn.execute(
            "INSERT INTO requests
               (collection_id, name, method, url, body_type, body,
                request_entity, response_entity, created_at, updated_at)
             VALUES (?1, '创建', 'POST', 'https://api.example.com/users',
                     'json', '{\"title\":\"x\"}', ?2, ?3, ?4, ?4)",
            params![collection, request_entity, response_entity, now],
        )
        .unwrap();
        // 无实体的接口：响应侧退回 description-only
        conn.execute(
            "INSERT INTO requests (collection_id, name, method, url, created_at, updated_at)
             VALUES (?1, '列表', 'GET', 'https://api.example.com/users', ?2, ?2)",
            params![collection, now],
        )
        .unwrap();

        let spec = build_openapi_spec(&conn, project).unwrap();
        let post = &spec["paths"]["/users"]["post"];
        let body_schema = &post["requestBody"]["content"]["application/json"]["schema"];
        assert_eq!(body_schema["properties"]["title"]["type"], "string");
        assert_eq!(body_schema["properties"]["title"]["description"], "标题");
        assert_eq!(body_schema["required"], json!(["title"]));
        assert_eq!(
            post["requestBody"]["content"]["application/json"]["example"],
            json!({"title":"x"})
        );
        let response_schema = &post["responses"]["200"]["content"]["application/json"]["schema"];
        assert_eq!(
            response_schema["properties"]["data"]["description"],
            "返回数据"
        );

        let get = &spec["paths"]["/users"]["get"];
        assert!(get["responses"]["200"].get("content").is_none());
    }

    #[test]
    fn postman_export_maps_request_and_masks_response_secrets() {
        let pool = pool("postman-export");
        let conn = pool.get().unwrap();
        let now = now_ms();
        conn.execute(
            "INSERT INTO projects (name, created_at, updated_at) VALUES ('P', ?1, ?1)",
            params![now],
        )
        .unwrap();
        let project = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO collections (project_id, name, created_at, updated_at)
             VALUES (?1, '用户', ?2, ?2)",
            params![project, now],
        )
        .unwrap();
        let collection = conn.last_insert_rowid();

        let mut title = node("title", "string");
        title.required = true;
        title.description = "标题".into();
        let request_entity = entity(vec![title]);
        let params_json = serde_json::to_string(&vec![KeyValueItem {
            key: "debug".into(),
            value: "1".into(),
            enabled: true,
        }])
        .unwrap();
        conn.execute(
            "INSERT INTO requests
               (collection_id, name, method, url, headers, params, body_type, body,
                request_entity, created_at, updated_at)
             VALUES (?1, '创建', 'POST', 'https://api.example.com/users', '[]', ?2,
                     'json', '{\"title\":\"x\"}', ?3, ?4, ?4)",
            params![collection, params_json, request_entity, now],
        )
        .unwrap();
        let request_id = conn.last_insert_rowid();
        let headers = serde_json::to_string(&vec![KeyValueItem {
            key: "Set-Cookie".into(),
            value: "sid=s3cr3t".into(),
            enabled: true,
        }])
        .unwrap();
        conn.execute(
            "INSERT INTO request_runs
               (request_id, method, url, response_status, response_status_text,
                response_headers, response_body, created_at)
             VALUES (?1, 'POST', 'https://api.example.com/users', 201, 'Created', ?2, ?3, ?4)",
            params![
                request_id,
                headers,
                "{\"token\":\"s3cr3t\",\"ok\":true}",
                now
            ],
        )
        .unwrap();

        let requests = read_requests(&conn, collection).unwrap();
        let mut secrets = crate::docs::masking::SecretSet::default();
        secrets.add("s3cr3t");
        let mut report = PostmanExportReport {
            requests: 0,
            collections: 1,
            skipped: Vec::new(),
        };
        let entry = postman_request(&conn, &requests[0], &secrets, &mut report).unwrap();
        assert_eq!(entry["request"]["method"], "POST");
        assert!(entry["request"]["description"]
            .as_str()
            .unwrap()
            .contains("标题"));
        assert_eq!(entry["request"]["url"]["query"][0]["key"], "debug");
        assert_eq!(entry["request"]["body"]["mode"], "raw");
        assert_eq!(entry["response"][0]["code"], 201);
        assert_eq!(
            entry["response"][0]["header"][0]["value"],
            crate::docs::model::MASK
        );
        assert!(!entry["response"][0]["body"]
            .as_str()
            .unwrap()
            .contains("s3cr3t"));
    }

    #[test]
    fn project_bundle_without_entity_columns_is_accepted() {
        let raw = r#"{
            "format": "astroman.project",
            "version": 1,
            "exportedAt": 0,
            "project": {"name": "旧包"},
            "collections": [{"name": "C", "requests": [{"name": "R", "method": "GET", "url": "https://x"}]}],
            "environments": []
        }"#;
        let bundle: ExportBundle = serde_json::from_str(raw).unwrap();
        let request = &bundle.collections[0].requests[0];
        assert!(request.request_entity.is_none());
        assert!(request.response_entity.is_none());
    }
}
