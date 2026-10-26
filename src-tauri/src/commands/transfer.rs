use rusqlite::{params, Connection, Transaction};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::models::{KeyValueItem, Project};
use crate::state::AppState;
use crate::util::{err, now_ms};

const FORMAT: &str = "finkmeter.project";
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
            "SELECT id, name, is_active FROM environments WHERE project_id = ?1 ORDER BY id ASC",
        )
        .map_err(err)?;
    let envs = env_stmt
        .query_map(params![project_id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<i64>>(2)?.unwrap_or(0) == 1,
            ))
        })
        .map_err(err)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(err)?;

    let mut result = Vec::new();
    for (env_id, name, is_active) in envs {
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

    if bundle.format != FORMAT {
        return Err("这不是 FinkMeter 项目文件".to_string());
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
            "INSERT INTO environments (project_id, name, is_active, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![project_id, environment.name, active as i64, now],
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
