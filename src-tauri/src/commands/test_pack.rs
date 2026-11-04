//! 套件包导入导出（T-25 / 文档 8.6）
//!
//! 单文件 JSON（`*.astromantest.json`），包含套件、用例、请求快照与数据集；
//! 导入端不依赖目标机器的接口库，保证 CI 与跨机器结果可复现。
//! 不含运行期密钥：变量值默认不导出，CI 通过 `--env-file` 注入。

use crate::commands::test_case::load_cases;
use crate::commands::test_suite::{find_suite, load_datasets};
use crate::state::AppState;
use crate::util::{err, now_ms};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::Path;
use tauri::State;

const PACK_SCHEMA: &str = "1.0";

fn json_or_null(text: Option<&str>) -> Value {
    text.map(str::trim)
        .filter(|text| !text.is_empty())
        .and_then(|text| serde_json::from_str::<Value>(text).ok())
        .unwrap_or(Value::Null)
}

fn split_csv(text: Option<&str>) -> Vec<String> {
    text.unwrap_or("")
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_string)
        .collect()
}

fn request_snapshot(conn: &Connection, id: i64) -> Result<Value, String> {
    conn.query_row(
        "SELECT name, method, url, headers, params, body_type, body, auth
         FROM requests WHERE id = ?1",
        params![id],
        |row| {
            let headers: Option<String> = row.get(3)?;
            let params_text: Option<String> = row.get(4)?;
            let auth: Option<String> = row.get(7)?;
            let auth = auth.map(|text| crate::secret::decrypt_auth_json(&text));
            Ok(json!({
                "name": row.get::<_, String>(0)?,
                "method": row.get::<_, String>(1)?,
                "url": row.get::<_, String>(2)?,
                "headers": json_or_null(headers.as_deref()),
                "params": json_or_null(params_text.as_deref()),
                "bodyType": row.get::<_, Option<String>>(5)?,
                "body": row.get::<_, Option<String>>(6)?,
                "auth": json_or_null(auth.as_deref()),
            }))
        },
    )
    .map_err(|error| match error {
        rusqlite::Error::QueryReturnedNoRows => format!("接口 #{id} 不存在，无法导出快照"),
        other => err(other),
    })
}

/// 组装套件包（不写文件）
pub fn export_pack(state: &AppState, suite_id: i64) -> Result<Value, String> {
    let conn = state.pool.get().map_err(err)?;
    let suite = find_suite(&conn, suite_id)?;
    let cases = load_cases(&conn, suite_id)?;
    let datasets = load_datasets(&conn, suite_id, true)?;

    let mut dataset_names: HashMap<i64, String> = HashMap::new();
    let mut datasets_out = Vec::new();
    for item in &datasets {
        dataset_names.insert(item.id, item.name.clone());
        let inline = item
            .content
            .as_deref()
            .and_then(|text| serde_json::from_str::<Value>(text).ok())
            .filter(Value::is_array);
        datasets_out.push(json!({
            "name": item.name,
            "format": item.format,
            // JSON 数据集内联为数组；CSV 保留原文，导入端按 content 处理
            "inline": inline,
            "content": if inline.is_none() { item.content.clone() } else { None },
            "path": item.path,
            "sha256": item.sha256,
            "rows": item.row_count,
        }));
    }

    let mut cases_out = Vec::new();
    for case in &cases {
        let request = match (case.kind.as_str(), case.request_id) {
            ("request", Some(id)) => Some(request_snapshot(&conn, id)?),
            _ => None,
        };
        cases_out.push(json!({
            "name": case.name,
            "kind": case.kind,
            "request": request,
            "dataset": case.dataset_id.and_then(|id| dataset_names.get(&id).cloned()),
            "dependsOn": split_csv(case.depends_on.as_deref()),
            "assertions": json_or_null(case.assertions_json.as_deref()),
            "variables": json_or_null(case.variables_json.as_deref()),
            "tags": split_csv(case.tags.as_deref()),
            "timeoutMs": case.timeout_ms,
            "retries": case.retries,
            "retryOnFailure": case.retry_on_failure,
            "continueOnFailure": case.continue_on_failure,
            "enabled": case.enabled,
            "preScript": case.pre_script,
            "postScript": case.post_script,
            "mainScript": case.main_script,
            "stress": json_or_null(case.stress_json.as_deref()),
        }));
    }

    Ok(json!({
        "schemaVersion": PACK_SCHEMA,
        "exportedAt": now_ms(),
        "appVersion": env!("CARGO_PKG_VERSION"),
        "suite": {
            "name": suite.name,
            "description": suite.description,
            "tags": split_csv(suite.tags.as_deref()),
            "config": json_or_null(suite.config_json.as_deref()),
            "setupScript": suite.setup_script,
            "teardownScript": suite.teardown_script,
        },
        "cases": cases_out,
        "datasets": datasets_out,
        // 变量值默认不导出，CI 通过 --env-file 注入
        "envProfiles": [],
    }))
}

#[tauri::command]
pub fn test_suite_export(
    state: State<'_, AppState>,
    suite_id: i64,
    path: String,
) -> Result<String, String> {
    let path = path.trim();
    if path.is_empty() {
        return Err("请选择导出路径".to_string());
    }
    let pack = export_pack(state.inner(), suite_id)?;
    let text = serde_json::to_string_pretty(&pack).map_err(err)?;
    std::fs::write(path, text).map_err(|error| format!("写入套件包失败：{error}"))?;
    Ok(path.to_string())
}

/* ---------------- 导入 ---------------- */

fn text_field(map: &serde_json::Map<String, Value>, key: &str) -> Option<String> {
    map.get(key).and_then(Value::as_str).map(str::to_string)
}

fn string_list(value: Option<&Value>) -> Option<String> {
    let items = value?.as_array()?;
    let list: Vec<String> = items
        .iter()
        .filter_map(|item| item.as_str().map(str::to_string))
        .collect();
    if list.is_empty() {
        None
    } else {
        Some(list.join(","))
    }
}

fn ensure_project(conn: &Connection, project_id: Option<i64>) -> Result<i64, String> {
    if let Some(id) = project_id {
        return Ok(id);
    }
    let name = "导入套件";
    let existing: Option<i64> = conn
        .query_row(
            "SELECT id FROM projects WHERE name = ?1 ORDER BY id LIMIT 1",
            params![name],
            |row| row.get(0),
        )
        .optional()
        .map_err(err)?;
    if let Some(id) = existing {
        return Ok(id);
    }
    let now = now_ms();
    conn.execute(
        "INSERT INTO projects (name, created_at, updated_at) VALUES (?1, ?2, ?2)",
        params![name, now],
    )
    .map_err(err)?;
    Ok(conn.last_insert_rowid())
}

fn unique_suite_name(conn: &Connection, project_id: i64, base: &str) -> String {
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let mut candidate = format!("{base}-{stamp}");
    let mut index = 1;
    while conn
        .query_row(
            "SELECT COUNT(*) FROM test_suites WHERE project_id = ?1 AND name = ?2",
            params![project_id, candidate],
            |row| Ok(row.get::<_, i64>(0)? > 0),
        )
        .unwrap_or(false)
    {
        index += 1;
        candidate = format!("{base}-{stamp}-{index}");
    }
    candidate
}

fn ensure_collection(conn: &Connection, project_id: i64, suite_name: &str) -> Result<i64, String> {
    let name = format!("套件包 · {suite_name}");
    let existing: Option<i64> = conn
        .query_row(
            "SELECT id FROM collections WHERE project_id = ?1 AND name = ?2 ORDER BY id LIMIT 1",
            params![project_id, name],
            |row| row.get(0),
        )
        .optional()
        .map_err(err)?;
    if let Some(id) = existing {
        return Ok(id);
    }
    let now = now_ms();
    conn.execute(
        "INSERT INTO collections (project_id, name, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?3)",
        params![project_id, name, now],
    )
    .map_err(err)?;
    Ok(conn.last_insert_rowid())
}

fn insert_snapshot_request(
    conn: &Connection,
    collection_id: i64,
    name: &str,
    request: &serde_json::Map<String, Value>,
    order: i64,
) -> Result<i64, String> {
    let headers = request.get("headers").map(|value| match value {
        Value::Object(map) => {
            let items: Vec<Value> = map
                .iter()
                .map(|(key, value)| {
                    json!({ "key": key, "value": value.as_str().unwrap_or_default(), "enabled": true })
                })
                .collect();
            Value::Array(items)
        }
        other => other.clone(),
    });
    let now = now_ms();
    conn.execute(
        "INSERT INTO requests
            (collection_id, name, method, url, headers, params, body_type, body,
             pre_script, post_script, assertions, auth, sort_order, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, '', '', '', ?9, ?10, ?11, ?11)",
        params![
            collection_id,
            name,
            text_field(request, "method").unwrap_or_else(|| "GET".into()),
            text_field(request, "url").unwrap_or_default(),
            headers.map(|value| value.to_string()),
            request.get("params").map(|value| value.to_string()),
            text_field(request, "bodyType").unwrap_or_else(|| "none".into()),
            text_field(request, "body"),
            match request.get("auth").map(|value| value.to_string()) {
                Some(text) => Some(crate::secret::encrypt_auth_json(&text)?),
                None => None,
            },
            order,
            now
        ],
    )
    .map_err(err)?;
    Ok(conn.last_insert_rowid())
}

fn collect_scripts(suite: &serde_json::Map<String, Value>, pack: &Value) -> Vec<String> {
    let mut scripts = Vec::new();
    for key in ["setupScript", "teardownScript"] {
        if text_field(suite, key).is_some_and(|text| !text.trim().is_empty()) {
            scripts.push(format!("suite.{key}"));
        }
    }
    if let Some(cases) = pack.get("cases").and_then(Value::as_array) {
        for (index, item) in cases.iter().enumerate() {
            let Some(map) = item.as_object() else {
                continue;
            };
            let name = text_field(map, "name").unwrap_or_else(|| format!("#{}", index + 1));
            for key in ["preScript", "postScript", "mainScript"] {
                if text_field(map, key).is_some_and(|text| !text.trim().is_empty()) {
                    scripts.push(format!("{name}.{key}"));
                }
            }
        }
    }
    scripts
}

fn insert_pack(
    conn: &Connection,
    project_id: i64,
    name: &str,
    suite: &serde_json::Map<String, Value>,
    pack: &Value,
) -> Result<i64, String> {
    let now = now_ms();
    let tags = string_list(suite.get("tags"));
    let config = suite
        .get("config")
        .filter(|value| !value.is_null())
        .map(|value| value.to_string());
    conn.execute(
        "INSERT INTO test_suites
            (project_id, name, tags, config_json, setup_script, teardown_script, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
        params![
            project_id,
            name,
            tags,
            config,
            text_field(suite, "setupScript"),
            text_field(suite, "teardownScript"),
            now
        ],
    )
    .map_err(err)?;
    let suite_id = conn.last_insert_rowid();

    let mut dataset_ids: HashMap<String, i64> = HashMap::new();
    if let Some(items) = pack.get("datasets").and_then(Value::as_array) {
        for item in items {
            let Some(map) = item.as_object() else {
                continue;
            };
            let Some(dataset_name) = text_field(map, "name") else {
                continue;
            };
            let format = text_field(map, "format").unwrap_or_else(|| "json".into());
            let inline = map.get("inline").filter(|value| value.is_array());
            let content = inline
                .map(|value| value.to_string())
                .or_else(|| text_field(map, "content"));
            let rows = inline
                .and_then(Value::as_array)
                .map(|items| items.len() as i64)
                .unwrap_or(0);
            conn.execute(
                "INSERT INTO test_datasets
                    (suite_id, name, format, path, content, row_count, sha256, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    suite_id,
                    dataset_name,
                    format,
                    text_field(map, "path"),
                    content,
                    rows,
                    text_field(map, "sha256"),
                    now
                ],
            )
            .map_err(err)?;
            dataset_ids.insert(dataset_name, conn.last_insert_rowid());
        }
    }

    let collection_id = ensure_collection(conn, project_id, name)?;
    let mut order: i64 = 0;
    if let Some(cases) = pack.get("cases").and_then(Value::as_array) {
        for item in cases {
            let Some(map) = item.as_object() else {
                continue;
            };
            order += 1;
            let case_name = text_field(map, "name").unwrap_or_else(|| format!("用例 {order}"));
            let kind = text_field(map, "kind").unwrap_or_else(|| "request".into());
            let request_id = if kind == "request" {
                match map.get("request").and_then(Value::as_object) {
                    Some(request) => Some(insert_snapshot_request(
                        conn,
                        collection_id,
                        &case_name,
                        request,
                        order,
                    )?),
                    None => None,
                }
            } else {
                None
            };
            let dataset_id =
                text_field(map, "dataset").and_then(|key| dataset_ids.get(&key).copied());
            conn.execute(
                "INSERT INTO test_cases
                    (suite_id, name, kind, request_id, dataset_id, depends_on, sort_order, enabled,
                     timeout_ms, retries, retry_on_failure, continue_on_failure, tags,
                     variables_json, assertions_json, pre_script, post_script, main_script,
                     stress_json, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15,
                         ?16, ?17, ?18, ?19, ?20, ?20)",
                params![
                    suite_id,
                    case_name,
                    kind,
                    request_id,
                    dataset_id,
                    string_list(map.get("dependsOn")),
                    order,
                    map.get("enabled").and_then(Value::as_bool).unwrap_or(true) as i64,
                    map.get("timeoutMs")
                        .and_then(Value::as_i64)
                        .unwrap_or(30_000),
                    map.get("retries").and_then(Value::as_i64).unwrap_or(0),
                    map.get("retryOnFailure")
                        .and_then(Value::as_bool)
                        .unwrap_or(false) as i64,
                    map.get("continueOnFailure")
                        .and_then(Value::as_bool)
                        .unwrap_or(false) as i64,
                    string_list(map.get("tags")),
                    map.get("variables")
                        .filter(|value| !value.is_null())
                        .map(Value::to_string),
                    map.get("assertions")
                        .filter(|value| !value.is_null())
                        .map(Value::to_string),
                    text_field(map, "preScript"),
                    text_field(map, "postScript"),
                    text_field(map, "mainScript"),
                    map.get("stress")
                        .filter(|value| !value.is_null())
                        .map(Value::to_string),
                    now
                ],
            )
            .map_err(err)?;
        }
    }
    Ok(suite_id)
}

/// 导入套件包；返回套件 id（`skip` 命中同名时返回已有套件）
pub fn import_pack(
    state: &AppState,
    path: &str,
    project_id: Option<i64>,
    on_conflict: &str,
) -> Result<i64, String> {
    let conflict = if on_conflict.trim().is_empty() {
        "rename"
    } else {
        on_conflict.trim()
    };
    if !matches!(conflict, "skip" | "overwrite" | "rename") {
        return Err("冲突处理只支持 skip / overwrite / rename".to_string());
    }
    if !Path::new(path).is_file() {
        return Err(format!("套件包不存在：{path}"));
    }
    let text = std::fs::read_to_string(path).map_err(|error| format!("读取套件包失败：{error}"))?;
    let pack: Value =
        serde_json::from_str(&text).map_err(|error| format!("套件包不是合法 JSON：{error}"))?;
    let suite = pack
        .get("suite")
        .and_then(Value::as_object)
        .ok_or_else(|| "套件包缺少 suite 节点".to_string())?;
    let suite_name = suite
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .ok_or_else(|| "套件包缺少 suite.name".to_string())?
        .to_string();

    // Python 脚本等同引入任意代码：未授权时拒绝导入（文档 8.6 第 6 条）
    let scripts = collect_scripts(suite, &pack);
    if !scripts.is_empty() && !state.settings().python_enabled {
        return Err(format!(
            "套件包包含 {} 个 Python 脚本，但本机未开启「允许执行本地 Python 脚本」，已拒绝导入\n脚本清单：{}",
            scripts.len(),
            scripts.join("、")
        ));
    }

    let conn = state.pool.get().map_err(err)?;
    let project_id = ensure_project(&conn, project_id)?;
    let existing: Option<i64> = conn
        .query_row(
            "SELECT id FROM test_suites WHERE project_id = ?1 AND name = ?2 ORDER BY id LIMIT 1",
            params![project_id, suite_name],
            |row| row.get(0),
        )
        .optional()
        .map_err(err)?;
    let final_name = match (existing, conflict) {
        (None, _) => suite_name.clone(),
        (Some(id), "skip") => return Ok(id),
        (Some(id), "overwrite") => {
            conn.execute("DELETE FROM test_suites WHERE id = ?1", params![id])
                .map_err(err)?;
            suite_name.clone()
        }
        (Some(_), _) => unique_suite_name(&conn, project_id, &suite_name),
    };
    insert_pack(&conn, project_id, &final_name, suite, &pack)
}

#[tauri::command]
pub fn test_suite_import(
    state: State<'_, AppState>,
    path: String,
    project_id: Option<i64>,
    on_conflict: Option<String>,
) -> Result<i64, String> {
    import_pack(
        state.inner(),
        path.trim(),
        project_id,
        on_conflict.as_deref().unwrap_or("rename"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_state(tag: &str) -> (PathBuf, AppState) {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|item| item.as_nanos())
            .unwrap_or(0);
        let root = std::env::temp_dir().join(format!(
            "astroman-pack-{tag}-{}-{stamp}",
            std::process::id()
        ));
        let state = AppState::headless(root.join("data"), root.join("config")).unwrap();
        (root, state)
    }

    #[test]
    fn export_then_import_keeps_cases_requests_and_datasets() {
        let (root, state) = temp_state("roundtrip");
        let now = now_ms();
        let (suite_id, project_id) = {
            let conn = state.pool.get().unwrap();
            conn.execute(
                "INSERT INTO projects (name, created_at, updated_at) VALUES ('p', ?1, ?1)",
                params![now],
            )
            .unwrap();
            let project_id = conn.last_insert_rowid();
            conn.execute(
                "INSERT INTO collections (project_id, name, created_at, updated_at)
                 VALUES (?1, 'c', ?2, ?2)",
                params![project_id, now],
            )
            .unwrap();
            let collection_id = conn.last_insert_rowid();
            conn.execute(
                "INSERT INTO requests (collection_id, name, method, url, created_at, updated_at)
                 VALUES (?1, 'r', 'POST', 'http://localhost/x', ?2, ?2)",
                params![collection_id, now],
            )
            .unwrap();
            let request_id = conn.last_insert_rowid();
            conn.execute(
                "INSERT INTO test_suites
                    (project_id, name, tags, config_json, created_at, updated_at)
                 VALUES (?1, '订单回归', 'smoke,order', '{\"failFast\":true}', ?2, ?2)",
                params![project_id, now],
            )
            .unwrap();
            let suite_id = conn.last_insert_rowid();
            conn.execute(
                "INSERT INTO test_datasets
                    (suite_id, name, format, content, row_count, created_at)
                 VALUES (?1, 'accounts', 'json', '[{\"user\":\"a\"}]', 1, ?2)",
                params![suite_id, now],
            )
            .unwrap();
            let dataset_id = conn.last_insert_rowid();
            conn.execute(
                "INSERT INTO test_cases
                    (suite_id, name, kind, request_id, dataset_id, sort_order, assertions_json,
                     created_at, updated_at)
                 VALUES (?1, '下单', 'request', ?2, ?3, 1,
                         '[{\"type\":\"status\",\"op\":\"eq\",\"value\":200}]', ?4, ?4)",
                params![suite_id, request_id, dataset_id, now],
            )
            .unwrap();
            (suite_id, project_id)
        };

        let pack = export_pack(&state, suite_id).unwrap();
        assert_eq!(pack["schemaVersion"], "1.0");
        assert_eq!(pack["cases"].as_array().unwrap().len(), 1);
        assert_eq!(pack["cases"][0]["dataset"], "accounts");
        assert_eq!(pack["cases"][0]["request"]["method"], "POST");
        assert_eq!(pack["datasets"][0]["inline"].as_array().unwrap().len(), 1);

        let path = root.join("order.astromantest.json");
        std::fs::write(&path, serde_json::to_string_pretty(&pack).unwrap()).unwrap();
        let imported =
            import_pack(&state, path.to_str().unwrap(), Some(project_id), "rename").unwrap();
        assert_ne!(imported, suite_id);

        let conn = state.pool.get().unwrap();
        let name: String = conn
            .query_row(
                "SELECT name FROM test_suites WHERE id = ?1",
                params![imported],
                |row| row.get(0),
            )
            .unwrap();
        assert!(name.starts_with("订单回归-"), "{name}");
        let cases: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM test_cases WHERE suite_id = ?1",
                params![imported],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(cases, 1);
        let datasets: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM test_datasets WHERE suite_id = ?1",
                params![imported],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(datasets, 1);
        drop(conn);
        let _ = std::fs::remove_dir_all(&root);
    }
}
