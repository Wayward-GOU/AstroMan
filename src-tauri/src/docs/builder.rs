use crate::commands::request::map_detail;
use crate::models::{HttpRequestDetail, KeyValueItem};
use crate::secret;
use crate::util::{collect_placeholders, err, now_ms, render_tracked};
use rusqlite::{params, Connection};
use std::collections::{HashMap, HashSet};

use super::examples;
use super::masking::{self, SecretSet};
use super::model::*;

pub const MAX_REQUESTS: usize = 5000;
pub const MAX_BODY_BYTES: usize = 64 * 1024;

struct CollectionRow {
    id: i64,
    parent_id: Option<i64>,
    name: String,
}

struct RequestRow {
    id: i64,
    collection_id: i64,
    detail: HttpRequestDetail,
}

struct EnvData {
    id: i64,
    name: String,
    vars: HashMap<String, String>,
    secrets: Vec<String>,
    /// 是否存在需要主密钥才能解密的变量（锁定状态下必须阻止导出）
    encrypted_secrets: bool,
    variables: Vec<DocEnvVar>,
}

struct RunData {
    url: String,
    params: Vec<KeyValueItem>,
    body: String,
    body_type: String,
    truncated: bool,
}

struct ResponseData {
    url: String,
    status: i64,
    status_text: String,
    headers: Vec<(String, String)>,
    body: String,
    truncated: bool,
}

struct BuildCtx<'a> {
    env: Option<&'a EnvData>,
    runs: &'a HashMap<i64, RunData>,
    responses: &'a HashMap<i64, ResponseData>,
    options: &'a DocsOptions,
    secrets: &'a mut SecretSet,
    only_request: Option<i64>,
    unresolved: usize,
}

struct ScopeCtx {
    kind: String,
    project_id: i64,
    project_name: String,
    title: String,
    collection_id: Option<i64>,
}

pub fn build(conn: &Connection, payload: &DocsPreparePayload) -> Result<DocBundle, String> {
    let scope = resolve_scope(conn, payload)?;
    let collections = load_collections(conn, scope.project_id)?;
    let children = children_map(&collections);

    let env = load_environment(conn, scope.project_id, payload.environment_id)?;
    // 主密钥存储缺失时按“已锁定”处理，避免把密文当作真实值写进文档
    let locked = secret::store()
        .map(|store| store.is_locked())
        .unwrap_or(true);

    let mut requests = load_requests(conn, scope.project_id)?;
    let only_request = if scope.kind == "request" {
        Some(payload.target_id)
    } else {
        None
    };
    let (roots, root_path) = match scope.collection_id {
        Some(collection_id) => {
            let subtree = subtree_ids(collection_id, &children);
            if let Some(only) = only_request {
                requests.retain(|row| row.id == only);
            } else {
                requests.retain(|row| subtree.contains(&row.collection_id));
            }
            (
                vec![collection_id],
                ancestor_path(&collections, collection_id),
            )
        }
        None => {
            let roots = collections
                .iter()
                .filter(|item| item.parent_id.is_none())
                .map(|item| item.id)
                .collect();
            (roots, Vec::new())
        }
    };

    if requests.len() > MAX_REQUESTS {
        return Err(format!(
            "E_TOO_LARGE: 本次导出包含 {} 个请求，超过 {} 个上限，请按集合分批导出",
            requests.len(),
            MAX_REQUESTS
        ));
    }

    if locked
        && (env
            .as_ref()
            .map(|item| item.encrypted_secrets)
            .unwrap_or(false)
            || requests
                .iter()
                .any(|row| !row.detail.auth.kind.is_empty() && row.detail.auth.kind != "none"))
    {
        return Err("E_SECRET_LOCKED: 主密钥未解锁，请先在设置页解锁后再导出".to_string());
    }

    let runs = if payload.options.use_run_examples {
        load_runs(
            conn,
            scope.project_id,
            payload.options.run_examples_max_age_days,
        )?
    } else {
        HashMap::new()
    };
    let responses = if payload.options.include_response_example {
        load_responses(
            conn,
            scope.project_id,
            payload.options.run_examples_max_age_days,
        )?
    } else {
        HashMap::new()
    };

    let mut secrets = SecretSet::default();
    if let Some(env) = &env {
        secrets.extend(env.secrets.iter().cloned());
    }
    let mut ctx = BuildCtx {
        env: env.as_ref(),
        runs: &runs,
        responses: &responses,
        options: &payload.options,
        secrets: &mut secrets,
        only_request,
        unresolved: 0,
    };

    let requests_by_collection = requests_map(&requests);
    let keep_empty: HashSet<i64> = scope.collection_id.into_iter().collect();
    let mut groups = Vec::new();
    for id in roots {
        if let Some(group) = build_group(
            id,
            &collections,
            &children,
            &requests_by_collection,
            &requests,
            &root_path,
            &keep_empty,
            &mut ctx,
        )? {
            groups.push(group);
        }
    }

    let warnings = if ctx.unresolved > 0 {
        vec![format!("{} 处变量未解析，已保留占位符", ctx.unresolved)]
    } else {
        Vec::new()
    };
    let secrets = ctx.secrets.clone();
    for group in &mut groups {
        mask_group(group, &secrets);
    }

    let mut environments = match env.as_ref() {
        Some(env) if payload.options.include_environment => vec![DocEnvironment {
            name: env.name.clone(),
            is_active: true,
            variables: env.variables.clone(),
        }],
        _ => Vec::new(),
    };
    for item in &mut environments {
        item.name = secrets.mask_text(&item.name);
        for variable in &mut item.variables {
            // 非敏感变量也可能恰好引用了敏感值，统一再过一遍掩码
            variable.value = if variable.is_secret {
                MASK.to_string()
            } else {
                secrets.mask_text(&variable.value)
            };
        }
    }

    Ok(DocBundle {
        generated_at: now_ms(),
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        revision: revision_of(conn, scope.project_id)?,
        subtitle: String::new(),
        include_generated_at: payload.options.include_generated_at,
        page_break_per_request: payload.options.page_break_per_request,
        scope: DocScopeInfo {
            kind: scope.kind,
            title: scope.title,
            project_name: scope.project_name,
            target_id: payload.target_id,
            environment_id: env.as_ref().map(|item| item.id),
            environment_name: env.as_ref().map(|item| item.name.clone()),
        },
        groups,
        environments,
        warnings,
    })
}

fn resolve_scope(conn: &Connection, payload: &DocsPreparePayload) -> Result<ScopeCtx, String> {
    match payload.scope.as_str() {
        "request" => {
            let row = conn
                .query_row(
                    "SELECT r.name, r.collection_id, c.project_id, p.name
                     FROM requests r
                     JOIN collections c ON c.id = r.collection_id
                     JOIN projects p ON p.id = c.project_id
                     WHERE r.id = ?1",
                    params![payload.target_id],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, i64>(1)?,
                            row.get::<_, i64>(2)?,
                            row.get::<_, String>(3)?,
                        ))
                    },
                )
                .map_err(|error| match error {
                    rusqlite::Error::QueryReturnedNoRows => "E_SCOPE: 请求不存在".to_string(),
                    other => err(other),
                })?;
            Ok(ScopeCtx {
                kind: "request".into(),
                project_id: row.2,
                project_name: row.3,
                title: row.0,
                collection_id: Some(row.1),
            })
        }
        "collection" => {
            let row = conn
                .query_row(
                    "SELECT c.name, c.project_id, p.name
                     FROM collections c JOIN projects p ON p.id = c.project_id
                     WHERE c.id = ?1",
                    params![payload.target_id],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, i64>(1)?,
                            row.get::<_, String>(2)?,
                        ))
                    },
                )
                .map_err(|error| match error {
                    rusqlite::Error::QueryReturnedNoRows => "E_SCOPE: 集合不存在".to_string(),
                    other => err(other),
                })?;
            Ok(ScopeCtx {
                kind: "collection".into(),
                project_id: row.1,
                project_name: row.2,
                title: row.0,
                collection_id: Some(payload.target_id),
            })
        }
        "project" => {
            let name = conn
                .query_row(
                    "SELECT name FROM projects WHERE id = ?1",
                    params![payload.target_id],
                    |row| row.get::<_, String>(0),
                )
                .map_err(|error| match error {
                    rusqlite::Error::QueryReturnedNoRows => "E_SCOPE: 项目不存在".to_string(),
                    other => err(other),
                })?;
            Ok(ScopeCtx {
                kind: "project".into(),
                project_id: payload.target_id,
                project_name: name.clone(),
                title: name,
                collection_id: None,
            })
        }
        _ => Err("E_SCOPE: 不支持的导出范围".to_string()),
    }
}

fn load_collections(conn: &Connection, project_id: i64) -> Result<Vec<CollectionRow>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, parent_id, name FROM collections
             WHERE project_id = ?1 ORDER BY sort_order ASC, id ASC",
        )
        .map_err(err)?;
    let rows = stmt
        .query_map(params![project_id], |row| {
            Ok(CollectionRow {
                id: row.get(0)?,
                parent_id: row.get(1)?,
                name: row.get(2)?,
            })
        })
        .map_err(err)?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(err)
}

fn load_requests(conn: &Connection, project_id: i64) -> Result<Vec<RequestRow>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT r.id, r.collection_id, r.name, r.method, r.url, r.headers, r.params,
                    r.body_type, r.body, r.pre_script, r.post_script, r.assertions, r.auth,
                    r.body_meta, r.request_entity, r.response_entity, r.protocol
             FROM requests r JOIN collections c ON c.id = r.collection_id
             WHERE c.project_id = ?1 ORDER BY r.sort_order ASC, r.id ASC",
        )
        .map_err(err)?;
    let rows = stmt
        .query_map(params![project_id], |row| {
            Ok(RequestRow {
                id: row.get(0)?,
                collection_id: row.get(1)?,
                detail: map_detail(row)?,
            })
        })
        .map_err(err)?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(err)
}

fn load_environment(
    conn: &Connection,
    project_id: i64,
    environment_id: Option<i64>,
) -> Result<Option<EnvData>, String> {
    let Some(environment_id) = environment_id else {
        return Ok(None);
    };
    let name = conn
        .query_row(
            "SELECT name FROM environments WHERE id = ?1 AND project_id = ?2",
            params![environment_id, project_id],
            |row| row.get::<_, String>(0),
        )
        .map_err(|error| match error {
            rusqlite::Error::QueryReturnedNoRows => {
                "E_SCOPE: 环境不存在或不属于当前项目".to_string()
            }
            other => err(other),
        })?;
    let mut stmt = conn
        .prepare(
            "SELECT \"key\", value, is_secret FROM env_variables
             WHERE env_id = ?1 ORDER BY sort_order ASC, id ASC",
        )
        .map_err(err)?;
    let rows = stmt
        .query_map(params![environment_id], |row| {
            Ok((
                row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                row.get::<_, Option<i64>>(2)?.unwrap_or(0) != 0,
            ))
        })
        .map_err(err)?;

    let mut vars = HashMap::new();
    let mut secrets = Vec::new();
    let mut variables = Vec::new();
    let mut encrypted_secrets = false;
    for item in rows {
        let (key, raw, is_secret) = item.map_err(err)?;
        if key.trim().is_empty() {
            continue;
        }
        if is_secret && raw.starts_with(secret::ENC_PREFIX) {
            encrypted_secrets = true;
        }
        let plain = if is_secret {
            secret::decrypt(secret::AAD_ENV, &raw)
        } else {
            raw
        };
        if is_secret && !plain.is_empty() {
            secrets.push(plain.clone());
        }
        variables.push(DocEnvVar {
            key: key.clone(),
            value: if is_secret && !plain.is_empty() {
                MASK.to_string()
            } else {
                plain.clone()
            },
            is_secret,
        });
        vars.insert(key, plain);
    }
    Ok(Some(EnvData {
        id: environment_id,
        name,
        vars,
        secrets,
        encrypted_secrets,
        variables,
    }))
}

fn load_runs(
    conn: &Connection,
    project_id: i64,
    max_age_days: u32,
) -> Result<HashMap<i64, RunData>, String> {
    let recent = if max_age_days == 0 {
        String::new()
    } else {
        format!(
            " AND rr.created_at >= {}",
            now_ms() - i64::from(max_age_days) * 86_400_000
        )
    };
    let sql = format!(
        "SELECT request_id, url, params, body, body_type, request_body_truncated
         FROM (
             SELECT rr.request_id, rr.url, rr.params, rr.body, rr.body_type,
                    rr.request_body_truncated,
                    ROW_NUMBER() OVER (
                        PARTITION BY rr.request_id ORDER BY rr.created_at DESC, rr.id DESC
                    ) AS rn
             FROM request_runs rr
             JOIN requests r ON r.id = rr.request_id
             JOIN collections c ON c.id = r.collection_id
             WHERE c.project_id = ?1{recent}
         ) WHERE rn = 1"
    );
    let mut stmt = conn.prepare(&sql).map_err(err)?;
    let rows = stmt
        .query_map(params![project_id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                RunData {
                    url: row.get::<_, String>(1)?,
                    params: parse_items(row.get::<_, Option<String>>(2)?),
                    body: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                    body_type: row.get::<_, Option<String>>(4)?.unwrap_or_default(),
                    truncated: row.get::<_, Option<i64>>(5)?.unwrap_or(0) != 0,
                },
            ))
        })
        .map_err(err)?;
    let mut result = HashMap::new();
    for item in rows {
        let (request_id, run) = item.map_err(err)?;
        result.insert(request_id, run);
    }
    Ok(result)
}

/// 每个请求最近一次「成功」运行（有响应且无错误）的响应，用于文档响应示例
fn load_responses(
    conn: &Connection,
    project_id: i64,
    max_age_days: u32,
) -> Result<HashMap<i64, ResponseData>, String> {
    let recent = if max_age_days == 0 {
        String::new()
    } else {
        format!(
            " AND rr.created_at >= {}",
            now_ms() - i64::from(max_age_days) * 86_400_000
        )
    };
    let sql = format!(
        "SELECT request_id, url, response_status, response_status_text, response_headers,
                response_body, response_body_truncated
         FROM (
             SELECT rr.request_id, rr.url, rr.response_status, rr.response_status_text,
                    rr.response_headers, rr.response_body, rr.response_body_truncated,
                    ROW_NUMBER() OVER (
                        PARTITION BY rr.request_id ORDER BY rr.created_at DESC, rr.id DESC
                    ) AS rn
             FROM request_runs rr
             JOIN requests r ON r.id = rr.request_id
             JOIN collections c ON c.id = r.collection_id
             WHERE c.project_id = ?1
               AND (rr.error IS NULL OR rr.error = '')
               AND rr.response_status > 0{recent}
         ) WHERE rn = 1"
    );
    let mut stmt = conn.prepare(&sql).map_err(err)?;
    let rows = stmt
        .query_map(params![project_id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                ResponseData {
                    url: row.get::<_, String>(1)?,
                    status: row.get::<_, i64>(2)?,
                    status_text: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                    headers: parse_response_headers(row.get::<_, Option<String>>(4)?),
                    body: row.get::<_, Option<String>>(5)?.unwrap_or_default(),
                    truncated: row.get::<_, Option<i64>>(6)?.unwrap_or(0) != 0,
                },
            ))
        })
        .map_err(err)?;
    let mut result = HashMap::new();
    for item in rows {
        let (request_id, response) = item.map_err(err)?;
        result.insert(request_id, response);
    }
    Ok(result)
}

fn parse_response_headers(raw: Option<String>) -> Vec<(String, String)> {
    let map = raw
        .and_then(|text| serde_json::from_str::<HashMap<String, String>>(&text).ok())
        .unwrap_or_default();
    let mut items: Vec<(String, String)> = map.into_iter().collect();
    items.sort_by_key(|(name, _)| name.to_ascii_lowercase());
    items
}

fn parse_items(raw: Option<String>) -> Vec<KeyValueItem> {
    raw.and_then(|text| serde_json::from_str::<Vec<KeyValueItem>>(&text).ok())
        .unwrap_or_default()
}

fn children_map(collections: &[CollectionRow]) -> HashMap<Option<i64>, Vec<i64>> {
    let mut map: HashMap<Option<i64>, Vec<i64>> = HashMap::new();
    for item in collections {
        map.entry(item.parent_id).or_default().push(item.id);
    }
    map
}

fn subtree_ids(root: i64, children: &HashMap<Option<i64>, Vec<i64>>) -> HashSet<i64> {
    let mut seen = HashSet::new();
    let mut stack = vec![root];
    while let Some(id) = stack.pop() {
        if !seen.insert(id) {
            continue;
        }
        if let Some(list) = children.get(&Some(id)) {
            stack.extend(list.iter().copied());
        }
    }
    seen
}

fn ancestor_path(collections: &[CollectionRow], collection_id: i64) -> Vec<String> {
    let mut path = Vec::new();
    let mut current = collections
        .iter()
        .find(|item| item.id == collection_id)
        .and_then(|item| item.parent_id);
    let mut guard = 0;
    while let Some(id) = current {
        if let Some(item) = collections.iter().find(|item| item.id == id) {
            path.push(item.name.clone());
            current = item.parent_id;
        } else {
            break;
        }
        guard += 1;
        if guard > 64 {
            break;
        }
    }
    path.reverse();
    path
}

fn requests_map(requests: &[RequestRow]) -> HashMap<i64, Vec<usize>> {
    let mut map: HashMap<i64, Vec<usize>> = HashMap::new();
    for (index, item) in requests.iter().enumerate() {
        map.entry(item.collection_id).or_default().push(index);
    }
    map
}

#[allow(clippy::too_many_arguments)]
fn build_group(
    collection_id: i64,
    collections: &[CollectionRow],
    children: &HashMap<Option<i64>, Vec<i64>>,
    requests: &HashMap<i64, Vec<usize>>,
    all_requests: &[RequestRow],
    ancestors: &[String],
    keep_empty: &HashSet<i64>,
    ctx: &mut BuildCtx<'_>,
) -> Result<Option<DocGroup>, String> {
    let Some(collection) = collections.iter().find(|item| item.id == collection_id) else {
        return Ok(None);
    };
    let mut path = ancestors.to_vec();
    path.push(collection.name.clone());
    let mut doc_requests = Vec::new();
    if let Some(indexes) = requests.get(&collection_id) {
        for index in indexes {
            let row = &all_requests[*index];
            if ctx.only_request.map(|id| id != row.id).unwrap_or(false) {
                continue;
            }
            doc_requests.push(build_request(row, ctx)?);
        }
    }
    let mut doc_children = Vec::new();
    if let Some(ids) = children.get(&Some(collection_id)) {
        for id in ids {
            if let Some(group) = build_group(
                *id,
                collections,
                children,
                requests,
                all_requests,
                &path,
                keep_empty,
                ctx,
            )? {
                doc_children.push(group);
            }
        }
    }
    if doc_requests.is_empty() && doc_children.is_empty() && !keep_empty.contains(&collection_id) {
        return Ok(None);
    }
    Ok(Some(DocGroup {
        id: collection_id,
        name: collection.name.clone(),
        description: collection.name.clone(),
        path,
        requests: doc_requests,
        children: doc_children,
    }))
}

fn build_request(row: &RequestRow, ctx: &mut BuildCtx<'_>) -> Result<DocRequest, String> {
    let detail = &row.detail;

    let (raw_url, _) = resolve_text(&detail.url, ctx);
    let path = path_of(&detail.url);
    let run_ok = run_matches(&raw_url, ctx.runs.get(&row.id));

    let mut query = Vec::new();
    for (seq, item) in detail
        .params
        .iter()
        .filter(|item| item.enabled && !item.key.trim().is_empty())
        .enumerate()
    {
        let sensitive = masking::is_sensitive_name(&item.key);
        let (defined, _) = resolve_text(&item.value, ctx);
        let run_value = if run_ok {
            ctx.runs.get(&row.id).and_then(|run| {
                run.params
                    .iter()
                    .find(|param| param.enabled && param.key == item.key)
                    .map(|param| param.value.clone())
                    .filter(|value| !value.is_empty())
            })
        } else {
            None
        };
        let (value, source) = if sensitive {
            (MASK.to_string(), "defined".to_string())
        } else if !defined.trim().is_empty() && !defined.contains("{{") {
            (defined, "defined".to_string())
        } else if let Some(value) = run_value {
            (value, "run".to_string())
        } else if ctx.options.auto_examples {
            (
                examples::auto_value(&item.key, "", seq, row.id),
                "auto".to_string(),
            )
        } else {
            (String::new(), "defined".to_string())
        };
        query.push(DocParam {
            name: item.key.clone(),
            value,
            required: false,
            location: "query".to_string(),
            example_source: source,
        });
    }

    let mut path_params = Vec::new();
    let mut seen_placeholders = HashSet::new();
    for (index, name) in collect_placeholders(&path).iter().enumerate() {
        if !seen_placeholders.insert(name.clone()) {
            continue;
        }
        let sensitive = masking::is_sensitive_name(name);
        let run_value = if run_ok {
            ctx.runs
                .get(&row.id)
                .and_then(|run| path_segment(&detail.url, &run.url, name))
        } else {
            None
        };
        let (value, source) = if sensitive {
            (MASK.to_string(), "defined".to_string())
        } else if let Some(value) = run_value {
            (value, "run".to_string())
        } else if ctx.options.auto_examples {
            (
                examples::auto_value(name, "", index, row.id),
                "auto".to_string(),
            )
        } else {
            (String::new(), "defined".to_string())
        };
        path_params.push(DocParam {
            name: name.clone(),
            value,
            required: true,
            location: "path".to_string(),
            example_source: source,
        });
    }

    let mut headers = Vec::new();
    for item in detail
        .headers
        .iter()
        .filter(|item| item.enabled && !item.key.trim().is_empty())
    {
        let lower = item.key.to_ascii_lowercase();
        if matches!(lower.as_str(), "authorization" | "host" | "content-length") {
            continue;
        }
        let sensitive = masking::is_sensitive_name(&item.key);
        let (defined, _) = resolve_text(&item.value, ctx);
        headers.push(DocParam {
            name: item.key.clone(),
            value: if sensitive { MASK.to_string() } else { defined },
            required: false,
            location: "header".to_string(),
            example_source: "defined".to_string(),
        });
    }

    let auth = build_auth(&detail.auth, ctx.secrets);
    let body = build_body(detail, row.id, run_ok, ctx);

    let response = ctx.responses.get(&row.id).and_then(|data| {
        // 响应来自历史运行：域名与当前地址不一致时不再输出，避免示例串环境
        if !hosts_match(&raw_url, &data.url) {
            return None;
        }
        Some(DocResponse {
            status: data.status,
            status_text: data.status_text.clone(),
            headers: data
                .headers
                .iter()
                .map(|(name, value)| DocParam {
                    name: name.clone(),
                    value: value.clone(),
                    required: false,
                    location: "response".into(),
                    example_source: "run".into(),
                })
                .collect(),
            body: masking::truncate_body(&data.body, MAX_BODY_BYTES),
            truncated: data.truncated || data.body.len() > MAX_BODY_BYTES,
        })
    });

    let scripts = if ctx.options.include_scripts
        && (!detail.pre_script.trim().is_empty() || !detail.post_script.trim().is_empty())
    {
        Some(DocScripts {
            pre: detail.pre_script.clone(),
            post: detail.post_script.clone(),
        })
    } else {
        None
    };
    let assertions = if ctx.options.include_assertions && !detail.assertions.trim().is_empty() {
        Some(detail.assertions.clone())
    } else {
        None
    };

    let request_entity = build_entity(detail.request_entity.as_deref(), ctx.secrets);
    let response_entity = build_entity(detail.response_entity.as_deref(), ctx.secrets);

    Ok(DocRequest {
        id: row.id,
        name: detail.name.clone(),
        method: detail.method.to_uppercase(),
        url: masking::mask_userinfo(&raw_url),
        path,
        description: detail.name.clone(),
        query,
        path_params,
        headers,
        auth,
        body,
        request_entity,
        response_entity,
        response,
        scripts,
        assertions,
    })
}

/// 实体列 → 文档 IR：说明文本同样过敏感值替换，字段名 / 类型原样保留
fn build_entity(raw: Option<&str>, secrets: &mut SecretSet) -> Option<DocEntity> {
    let doc = crate::entity::decode_lenient(raw)?;
    if doc.is_empty() {
        return None;
    }
    let fields = doc
        .flatten()
        .into_iter()
        .map(|item| DocEntityField {
            name: item.path,
            type_name: item.type_name,
            required: item.required,
            description: secrets.mask_text(&item.description),
            stale: item.stale,
            depth: item.depth,
        })
        .collect();
    Some(DocEntity { fields })
}

fn resolve_text(input: &str, ctx: &mut BuildCtx<'_>) -> (String, bool) {
    if !ctx.options.resolve_environment {
        return (input.to_string(), input.contains("{{"));
    }
    let Some(env) = ctx.env else {
        return (input.to_string(), input.contains("{{"));
    };
    let mut used = Vec::new();
    let out = render_tracked(input, &env.vars, &mut used);
    for value in used {
        ctx.secrets.add(&value);
    }
    let unresolved = out.contains("{{");
    if unresolved {
        ctx.unresolved += 1;
    }
    (out, unresolved)
}

fn run_matches(url: &str, run: Option<&RunData>) -> bool {
    run.map(|run| hosts_match(url, &run.url)).unwrap_or(false)
}

/// 定义地址与运行记录地址的域名一致性：未解析的模板地址一律视为不匹配
fn hosts_match(url: &str, actual: &str) -> bool {
    if url.contains("{{") {
        return false;
    }
    match (host_of(url), host_of(actual)) {
        (Some(left), Some(right)) => left == right,
        _ => false,
    }
}

fn host_of(url: &str) -> Option<String> {
    let rest = url.split("://").nth(1)?;
    let authority = rest.split(['/', '?', '#']).next()?;
    let host = authority.rsplit('@').next()?;
    let host = host.split(':').next()?;
    if host.is_empty() {
        None
    } else {
        Some(host.to_ascii_lowercase())
    }
}

fn build_auth(auth: &crate::models::RequestAuth, secrets: &mut SecretSet) -> DocAuth {
    match auth.kind.as_str() {
        "bearer" => {
            secrets.add(&auth.token);
            DocAuth {
                kind: "bearer".into(),
                add_to: auth.add_to.clone(),
                key_name: "Authorization".into(),
                note: "Bearer Token（值已隐藏）".into(),
            }
        }
        "basic" => {
            secrets.add(&auth.username);
            secrets.add(&auth.password);
            DocAuth {
                kind: "basic".into(),
                add_to: auth.add_to.clone(),
                key_name: "Authorization".into(),
                note: "Basic（用户名与口令已隐藏）".into(),
            }
        }
        "apikey" => {
            secrets.add(&auth.value);
            DocAuth {
                kind: "apikey".into(),
                add_to: if auth.add_to.is_empty() {
                    "header".into()
                } else {
                    auth.add_to.clone()
                },
                key_name: auth.key.clone(),
                note: "API Key（值已隐藏）".into(),
            }
        }
        "oauth2" => {
            secrets.add(&auth.client_secret);
            DocAuth {
                kind: "oauth2".into(),
                add_to: "header".into(),
                key_name: "Authorization".into(),
                note: format!(
                    "OAuth2 Client Credentials · clientId={} · tokenUrl={} · scope={}",
                    auth.client_id, auth.token_url, auth.scope
                ),
            }
        }
        _ => DocAuth::default(),
    }
}

fn build_body(
    detail: &HttpRequestDetail,
    request_id: i64,
    run_ok: bool,
    ctx: &mut BuildCtx<'_>,
) -> DocBody {
    let kind = detail.body_type.as_str();
    if kind == "none" || kind.is_empty() {
        return DocBody::default();
    }
    let (resolved, _) = resolve_text(&detail.body, ctx);
    match kind {
        "json" => build_json_body(&resolved, request_id, run_ok, ctx),
        "form" => {
            let mut fields = Vec::new();
            for (index, pair) in resolved.split('&').enumerate() {
                if pair.is_empty() {
                    continue;
                }
                let mut parts = pair.splitn(2, '=');
                let key = parts.next().unwrap_or_default();
                let value = parts.next().unwrap_or_default();
                if key.trim().is_empty() {
                    continue;
                }
                let sensitive = masking::is_sensitive_name(key);
                fields.push(DocParam {
                    name: key.to_string(),
                    value: if sensitive {
                        MASK.to_string()
                    } else if !value.is_empty() {
                        value.to_string()
                    } else if ctx.options.auto_examples {
                        examples::auto_value(key, "", index, request_id)
                    } else {
                        String::new()
                    },
                    required: false,
                    location: "form".into(),
                    example_source: "defined".into(),
                });
            }
            DocBody {
                kind: "form".into(),
                text: masking::truncate_body(&resolved, MAX_BODY_BYTES),
                fields,
            }
        }
        "multipart" => {
            let items = parse_items(Some(resolved.clone()));
            let mut fields = Vec::new();
            for (index, item) in items.iter().filter(|item| item.enabled).enumerate() {
                if item.key.trim().is_empty() {
                    continue;
                }
                let sensitive = masking::is_sensitive_name(&item.key);
                fields.push(DocParam {
                    name: item.key.clone(),
                    value: if sensitive {
                        MASK.to_string()
                    } else if !item.value.is_empty() {
                        item.value.clone()
                    } else if ctx.options.auto_examples {
                        examples::auto_value(&item.key, "", index, request_id)
                    } else {
                        String::new()
                    },
                    required: false,
                    location: "multipart".into(),
                    example_source: "defined".into(),
                });
            }
            DocBody {
                kind: "multipart".into(),
                text: masking::truncate_body(&resolved, MAX_BODY_BYTES),
                fields,
            }
        }
        _ => DocBody {
            kind: kind.to_string(),
            text: masking::truncate_body(&resolved, MAX_BODY_BYTES),
            fields: Vec::new(),
        },
    }
}

fn build_json_body(raw: &str, request_id: i64, run_ok: bool, ctx: &mut BuildCtx<'_>) -> DocBody {
    let Ok(mut value) = serde_json::from_str::<serde_json::Value>(raw) else {
        return DocBody {
            kind: "json".into(),
            text: masking::truncate_body(raw, MAX_BODY_BYTES),
            fields: Vec::new(),
        };
    };
    let run_body = if run_ok {
        ctx.runs
            .get(&request_id)
            .filter(|run| run.body_type == "json" && !run.truncated)
            .and_then(|run| serde_json::from_str::<serde_json::Value>(&run.body).ok())
    } else {
        None
    };
    let mut fields = Vec::new();
    if let serde_json::Value::Object(map) = &mut value {
        for (index, (key, item)) in map.iter_mut().enumerate() {
            let sensitive = masking::is_sensitive_name(key);
            if sensitive {
                *item = serde_json::Value::String(MASK.to_string());
                fields.push(DocParam {
                    name: key.clone(),
                    value: MASK.to_string(),
                    required: false,
                    location: "body".into(),
                    example_source: "defined".into(),
                });
                continue;
            }
            let defined = json_text(item);
            let run_value = run_body
                .as_ref()
                .and_then(|value| value.get(key))
                .map(json_text)
                .filter(|value| !value.is_empty());
            let (text, source) = if !defined.trim().is_empty() && !defined.contains("{{") {
                (defined, "defined")
            } else if let Some(value) = run_value {
                (value, "run")
            } else if ctx.options.auto_examples {
                (examples::auto_value(key, "", index, request_id), "auto")
            } else {
                (String::new(), "defined")
            };
            if source != "defined" || text != json_text(item) {
                *item = serde_json::Value::String(text.clone());
            }
            fields.push(DocParam {
                name: key.clone(),
                value: text,
                required: false,
                location: "body".into(),
                example_source: source.into(),
            });
        }
    }
    let text = serde_json::to_string_pretty(&value).unwrap_or_else(|_| raw.to_string());
    DocBody {
        kind: "json".into(),
        text: masking::truncate_body(&text, MAX_BODY_BYTES),
        fields,
    }
}

fn json_text(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(text) => text.clone(),
        serde_json::Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn mask_group(group: &mut DocGroup, secrets: &SecretSet) {
    group.name = secrets.mask_text(&group.name);
    group.description = secrets.mask_text(&group.description);
    for request in &mut group.requests {
        mask_request(request, secrets);
    }
    for child in &mut group.children {
        mask_group(child, secrets);
    }
}

fn mask_param(param: &mut DocParam, secrets: &SecretSet) {
    if masking::is_sensitive_name(&param.name) {
        param.value = MASK.to_string();
        return;
    }
    param.value = secrets.mask_text(&param.value);
}

fn mask_request(request: &mut DocRequest, secrets: &SecretSet) {
    request.name = secrets.mask_text(&request.name);
    request.description = secrets.mask_text(&request.description);
    request.url = secrets.mask_text(&request.url);
    request.path = secrets.mask_text(&request.path);
    for param in request
        .query
        .iter_mut()
        .chain(request.path_params.iter_mut())
        .chain(request.headers.iter_mut())
    {
        mask_param(param, secrets);
    }
    for param in &mut request.body.fields {
        mask_param(param, secrets);
    }
    request.body.text = if request.body.kind == "json" {
        secrets.mask_json(&request.body.text)
    } else {
        secrets.mask_text(&request.body.text)
    };
    if let Some(response) = &mut request.response {
        response.status_text = secrets.mask_text(&response.status_text);
        for header in &mut response.headers {
            mask_param(header, secrets);
        }
        response.body = if response.body.trim().is_empty() {
            String::new()
        } else {
            secrets.mask_json(&response.body)
        };
    }
    if let Some(scripts) = &mut request.scripts {
        scripts.pre = secrets.mask_text(&scripts.pre);
        scripts.post = secrets.mask_text(&scripts.post);
    }
    if let Some(assertions) = &mut request.assertions {
        *assertions = secrets.mask_text(assertions);
    }
}

pub fn outline(bundle: &DocBundle) -> DocOutline {
    DocOutline {
        groups: bundle.groups.iter().map(outline_group).collect(),
    }
}

fn outline_group(group: &DocGroup) -> DocOutlineGroup {
    DocOutlineGroup {
        key: format!("g-{}", group.id),
        name: group.name.clone(),
        path: group.path.clone(),
        requests: group.requests.iter().map(outline_request).collect(),
        children: group.children.iter().map(outline_group).collect(),
    }
}

fn outline_request(request: &DocRequest) -> DocOutlineRequest {
    let mut params = Vec::new();
    push_fields(&mut params, &request.query, request.id, "query", "p");
    push_fields(&mut params, &request.path_params, request.id, "path", "p");
    push_fields(&mut params, &request.headers, request.id, "header", "p");
    let mut seen = HashMap::new();
    for item in &request.body.fields {
        let occurrence = seen.entry(item.name.clone()).or_insert(0usize);
        *occurrence += 1;
        let (key, location) = match request.body.kind.as_str() {
            "json" => (
                format!("b-{}-{}", request.id, pointer_of(&item.name)),
                "body",
            ),
            _ => (
                format!("f-{}-{}-{}", request.id, item.name, occurrence),
                "form",
            ),
        };
        params.push(DocOutlineField {
            key,
            name: item.name.clone(),
            location: location.into(),
            value: item.value.clone(),
            source: item.example_source.clone(),
            sensitive: masking::is_sensitive_name(&item.name),
        });
    }
    DocOutlineRequest {
        key: format!("r-{}", request.id),
        name: request.name.clone(),
        method: request.method.clone(),
        url: request.url.clone(),
        description: request.description.clone(),
        included: true,
        params,
        warnings: Vec::new(),
    }
}

fn push_fields(
    out: &mut Vec<DocOutlineField>,
    list: &[DocParam],
    request_id: i64,
    location: &str,
    prefix: &str,
) {
    let mut seen: HashMap<String, usize> = HashMap::new();
    for item in list {
        let occurrence = seen.entry(item.name.clone()).or_insert(0usize);
        *occurrence += 1;
        out.push(DocOutlineField {
            key: format!(
                "{prefix}-{request_id}-{location}-{}-{occurrence}",
                item.name
            ),
            name: item.name.clone(),
            location: location.to_string(),
            value: item.value.clone(),
            source: item.example_source.clone(),
            sensitive: masking::is_sensitive_name(&item.name),
        });
    }
}

pub(crate) fn pointer_of(name: &str) -> String {
    format!("/{}", name.replace('~', "~0").replace('/', "~1"))
}

fn path_of(url: &str) -> String {
    let after_scheme = url.split("://").nth(1).unwrap_or(url);
    if let Some(index) = after_scheme.find('/') {
        after_scheme[index..]
            .split(['?', '#'])
            .next()
            .unwrap_or("/")
            .to_string()
    } else {
        "/".to_string()
    }
}

fn path_segment(template: &str, actual: &str, name: &str) -> Option<String> {
    let template_path = path_of(template);
    let actual_path = path_of(actual);
    let left: Vec<&str> = template_path.split('/').collect();
    let right: Vec<&str> = actual_path.split('/').collect();
    if left.len() != right.len() {
        return None;
    }
    let needle = format!("{{{{{name}}}}}");
    left.iter()
        .position(|segment| segment.contains(&needle))
        .map(|index| right[index].to_string())
}

fn revision_of(conn: &Connection, project_id: i64) -> Result<String, String> {
    conn.query_row(
        "SELECT
             (SELECT COUNT(*) FROM collections WHERE project_id = ?1),
             (SELECT COALESCE(MAX(updated_at), 0) FROM collections WHERE project_id = ?1),
             (SELECT COUNT(*) FROM requests r JOIN collections c ON c.id = r.collection_id
              WHERE c.project_id = ?1),
             (SELECT COALESCE(MAX(r.updated_at), 0) FROM requests r
              JOIN collections c ON c.id = r.collection_id WHERE c.project_id = ?1)",
        params![project_id],
        |row| {
            Ok(format!(
                "{}-{}-{}-{}",
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?
            ))
        },
    )
    .map_err(err)
}

pub fn request_count(bundle: &DocBundle) -> usize {
    fn count_group(group: &DocGroup) -> usize {
        group.requests.len() + group.children.iter().map(count_group).sum::<usize>()
    }
    bundle.groups.iter().map(count_group).sum()
}

pub fn collection_count(bundle: &DocBundle) -> usize {
    fn count_group(group: &DocGroup) -> usize {
        1 + group.children.iter().map(count_group).sum::<usize>()
    }
    bundle.groups.iter().map(count_group).sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use r2d2_sqlite::SqliteConnectionManager;
    use rusqlite::params;

    fn pool(tag: &str) -> crate::state::DbPool {
        let dir = crate::util::test_dir(tag);
        let manager = SqliteConnectionManager::file(dir.join("astroman.db"));
        let pool = r2d2::Pool::builder().max_size(2).build(manager).unwrap();
        db::init(&pool).unwrap();
        pool
    }

    fn seed(conn: &rusqlite::Connection) -> (i64, i64, i64) {
        conn.execute(
            "INSERT INTO projects (name, description, color, sort_order, created_at, updated_at) VALUES ('P','',NULL,0,0,0)",
            [],
        )
        .unwrap();
        let project = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO collections (project_id, parent_id, name, sort_order, created_at, updated_at) VALUES (?1,NULL,'C',0,0,0)",
            params![project],
        )
        .unwrap();
        let collection = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO requests (collection_id,name,method,url,headers,params,body_type,body,body_meta,pre_script,post_script,assertions,auth,sort_order,created_at,updated_at)
             VALUES (?1,'登录','POST','https://{{host}}/login?debug=1','[]','[{\"key\":\"page\",\"value\":\"\",\"enabled\":true}]','json','{\"username\":\"u\",\"password\":\"p\"}','','','','','{\"kind\":\"none\"}',0,0,0)",
            params![collection],
        )
        .unwrap();
        let request = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO environments (project_id,name,is_active,script,created_at) VALUES (?1,'测试',1,'',0)",
            params![project],
        )
        .unwrap();
        let env = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO env_variables (env_id,\"key\",value,is_secret,sort_order) VALUES (?1,'host','api.example.com',0,0), (?1,'api_token','tok-secret',1,1)",
            params![env],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO request_runs (request_id,method,url,headers,params,body_type,body,response_status,response_status_text,response_headers,response_body,created_at,origin)
             VALUES (?1,'POST','https://api.example.com/login?debug=1','[]','[{\"key\":\"page\",\"value\":\"2\",\"enabled\":true}]','json','{\"username\":\"run-user\"}',200,'OK','{\"Content-Type\":\"application/json\",\"Set-Cookie\":\"sid=tok-secret\"}','{\"token\":\"tok-secret-value\",\"ok\":true}',?2,'manual')",
            params![request, crate::util::now_ms()],
        )
        .unwrap();
        (project, env, request)
    }

    #[test]
    fn builds_project_bundle_with_env_and_run_examples() {
        let pool = pool("docs-builder");
        let conn = pool.get().unwrap();
        let (project_id, env_id, _request_id) = seed(&conn);
        let payload = DocsPreparePayload {
            scope: "project".into(),
            target_id: project_id,
            environment_id: Some(env_id),
            options: DocsOptions::default(),
        };
        let bundle = build(&conn, &payload).unwrap();
        let request = &bundle.groups[0].requests[0];
        assert_eq!(bundle.scope.environment_name.as_deref(), Some("测试"));
        assert!(request.url.contains("api.example.com"));
        assert_eq!(request.query[0].value, "2");
        assert_eq!(request.query[0].example_source, "run");
        assert!(request
            .body
            .fields
            .iter()
            .any(|field| field.name == "username"));
        assert_eq!(bundle.environments[0].variables[1].value, "••••••");
        let response = request.response.as_ref().expect("应输出响应示例");
        assert_eq!(response.status, 200);
        assert_eq!(response.status_text, "OK");
        assert!(response
            .headers
            .iter()
            .any(|header| header.name == "Set-Cookie" && header.value == "••••••"));
        assert!(response.body.contains("••••••"));
        assert!(!serde_json::to_string(&bundle)
            .unwrap()
            .contains("tok-secret"));
        let outline = outline(&bundle);
        assert_eq!(
            outline.groups[0].requests[0].key,
            format!("r-{}", request.id)
        );
    }

    #[test]
    fn builds_entities_from_columns_and_masks_descriptions() {
        use crate::entity::model::{EntityDoc, EntityNode};

        let pool = pool("docs-builder-entity");
        let conn = pool.get().unwrap();
        let (_project_id, env_id, request_id) = seed(&conn);

        let mut request_root = EntityNode::new_root("object");
        request_root.children.push(EntityNode {
            name: "password".into(),
            type_name: "string".into(),
            required: true,
            description: "含 tok-secret 的说明".into(),
            ..EntityNode::default()
        });
        let request_entity = EntityDoc::new(request_root, 0).encode().unwrap();

        let mut response_root = EntityNode::new_root("object");
        response_root.children.push(EntityNode {
            name: "roles".into(),
            type_name: "array".into(),
            item_type: "string".into(),
            required: false,
            description: "角色".into(),
            ..EntityNode::default()
        });
        let response_entity = EntityDoc::new(response_root, 0).encode().unwrap();
        conn.execute(
            "UPDATE requests SET request_entity = ?1, response_entity = ?2 WHERE id = ?3",
            params![request_entity, response_entity, request_id],
        )
        .unwrap();

        let payload = DocsPreparePayload {
            scope: "request".into(),
            target_id: request_id,
            environment_id: Some(env_id),
            options: DocsOptions::default(),
        };
        let bundle = build(&conn, &payload).unwrap();
        let request = &bundle.groups[0].requests[0];
        let entity = request.request_entity.as_ref().expect("请求实体");
        assert_eq!(entity.fields[0].name, "password");
        assert_eq!(entity.fields[0].type_name, "string");
        assert!(!entity.fields[0].description.contains("tok-secret"));
        assert!(entity.fields[0].description.contains("••••••"));
        let response = request.response_entity.as_ref().expect("返回实体");
        assert_eq!(response.fields[0].name, "roles[]");
        assert_eq!(response.fields[0].type_name, "string");
    }

    #[test]
    fn broken_entity_json_is_ignored_without_failing_export() {
        let pool = pool("docs-builder-entity-broken");
        let conn = pool.get().unwrap();
        let (project_id, _env_id, request_id) = seed(&conn);
        conn.execute(
            "UPDATE requests SET request_entity = '{' WHERE id = ?1",
            params![request_id],
        )
        .unwrap();

        let payload = DocsPreparePayload {
            scope: "project".into(),
            target_id: project_id,
            environment_id: None,
            options: DocsOptions::default(),
        };
        let bundle = build(&conn, &payload).unwrap();
        assert!(bundle.groups[0].requests[0].request_entity.is_none());
    }
}
