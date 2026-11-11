//! Git 友好工作区导出 / 导入（G-11）
//!
//! 目标不是服务端同步，而是把库变成可读、可 diff、可提交的目录：
//!
//! ```text
//! workspace/
//!   manifest.json
//!   project-<id>-<slug>/
//!     project.json
//!     collections/<目录>/collection.json
//!     requests/<slug>.json
//!     environments/<slug>.json
//! ```
//!
//! 约定：
//! - JSON 固定键顺序、2 空格缩进、文件末尾换行，减少无意义 diff；
//! - 敏感环境变量只写 `{{secret:NAME}}`，导入后需要用户补值；
//! - 导入按 id 合并：不存在则创建，内容相同则跳过，内容不同记冲突且不覆盖
//!   （显式 `overwrite = true` 时才更新）；
//! - 不做文件监听，避免半保存文件损坏库。

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OptionalExtension};
use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::util::{err, now_ms};

/// 导出查询的请求行：列较多，用别名压住 clippy 的 type_complexity
type RequestRow = (
    i64,
    i64,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    i64,
    i64,
    i64,
);

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceExportPayload {
    pub path: String,
    #[serde(default)]
    pub project_ids: Vec<i64>,
}

#[derive(Debug, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceCounts {
    pub projects: usize,
    pub collections: usize,
    pub requests: usize,
    pub environments: usize,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceExportResult {
    pub path: String,
    pub counts: WorkspaceCounts,
    pub warnings: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceImportPayload {
    pub path: String,
    #[serde(default)]
    pub overwrite: bool,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceConflict {
    pub kind: String,
    pub id: i64,
    pub name: String,
    pub reason: String,
}

#[derive(Debug, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceImportResult {
    pub created: WorkspaceCounts,
    pub updated: WorkspaceCounts,
    pub skipped: usize,
    pub conflicts: Vec<WorkspaceConflict>,
    pub warnings: Vec<String>,
}

fn slug(text: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for ch in text.trim().chars() {
        if ch.is_alphanumeric() || ch == '_' {
            out.push(ch.to_lowercase().next().unwrap_or(ch));
            last_dash = false;
        } else if !last_dash && !out.is_empty() {
            out.push('-');
            last_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    let out: String = out.chars().take(60).collect();
    if out.is_empty() {
        "item".to_string()
    } else {
        out
    }
}

fn unique_name(used: &mut HashSet<String>, base: String, id: i64) -> String {
    if used.insert(base.clone()) {
        base
    } else {
        let candidate = format!("{base}-{id}");
        used.insert(candidate.clone());
        candidate
    }
}

fn write_json(path: &Path, value: &Value) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("创建目录失败（{}）：{error}", parent.display()))?;
    }
    let mut text = serde_json::to_string_pretty(value).map_err(err)?;
    text.push('\n');
    fs::write(path, text).map_err(|error| format!("写入文件失败（{}）：{error}", path.display()))
}

fn read_json(path: &Path) -> Result<Value, String> {
    let text = fs::read_to_string(path)
        .map_err(|error| format!("读取文件失败（{}）：{error}", path.display()))?;
    serde_json::from_str(&text)
        .map_err(|error| format!("JSON 解析失败（{}）：{error}", path.display()))
}

fn masked_secret(name: &str) -> String {
    format!("{{{{secret:{name}}}}}")
}

fn query_projects(conn: &Connection, ids: &[i64]) -> Result<Vec<(i64, String)>, String> {
    let mut out = Vec::new();
    if ids.is_empty() {
        let mut stmt = conn
            .prepare("SELECT id, name FROM projects ORDER BY id")
            .map_err(err)?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(err)?;
        for row in rows {
            out.push(row.map_err(err)?);
        }
    } else {
        for id in ids {
            let name: Option<String> = conn
                .query_row(
                    "SELECT name FROM projects WHERE id = ?1",
                    params![id],
                    |row| row.get(0),
                )
                .optional()
                .map_err(err)?;
            if let Some(name) = name {
                out.push((*id, name));
            }
        }
    }
    Ok(out)
}

fn export_project(
    conn: &Connection,
    project_id: i64,
    project_name: &str,
    root: &Path,
    counts: &mut WorkspaceCounts,
    warnings: &mut Vec<String>,
) -> Result<Map<String, Value>, String> {
    let dir_name = format!("project-{project_id}-{}", slug(project_name));
    let project_dir = root.join(&dir_name);
    fs::create_dir_all(&project_dir)
        .map_err(|error| format!("创建目录失败（{}）：{error}", project_dir.display()))?;

    let (name, description, color, sort_order, created_at, updated_at): (
        String,
        Option<String>,
        Option<String>,
        i64,
        i64,
        i64,
    ) = conn
        .query_row(
            "SELECT name, description, color, sort_order, created_at, updated_at
             FROM projects WHERE id = ?1",
            params![project_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            },
        )
        .map_err(err)?;
    let project_json = json!({
        "id": project_id,
        "name": name,
        "description": description.unwrap_or_default(),
        "color": color.unwrap_or_default(),
        "sortOrder": sort_order,
        "createdAt": created_at,
        "updatedAt": updated_at,
    });
    write_json(&project_dir.join("project.json"), &project_json)?;

    // 集合：按父子关系落到目录，目录名在同级内唯一
    let mut stmt = conn
        .prepare(
            "SELECT id, parent_id, name, sort_order, created_at, updated_at
             FROM collections WHERE project_id = ?1 ORDER BY sort_order ASC, id ASC",
        )
        .map_err(err)?;
    let collections: Vec<(i64, Option<i64>, String, i64, i64, i64)> = stmt
        .query_map(params![project_id], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
            ))
        })
        .map_err(err)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(err)?;
    let mut by_parent: HashMap<Option<i64>, Vec<(i64, String)>> = HashMap::new();
    for (id, parent, name, ..) in &collections {
        by_parent
            .entry(*parent)
            .or_default()
            .push((*id, name.clone()));
    }
    let mut folder: HashMap<i64, String> = HashMap::new();
    for (parent, siblings) in by_parent.iter_mut() {
        let mut used = HashSet::new();
        for (id, name) in siblings.iter() {
            let dir = unique_name(&mut used, slug(name), *id);
            let parent_dir = parent
                .and_then(|parent_id| folder.get(&parent_id).cloned())
                .unwrap_or_default();
            folder.insert(
                *id,
                if parent_dir.is_empty() {
                    dir
                } else {
                    format!("{parent_dir}/{dir}")
                },
            );
        }
    }
    for (id, parent, name, sort_order, created_at, updated_at) in &collections {
        let relative = folder
            .get(id)
            .cloned()
            .unwrap_or_else(|| format!("item-{id}"));
        let collection_dir = project_dir.join("collections").join(relative);
        let collection_json = json!({
            "id": id,
            "projectId": project_id,
            "parentId": parent,
            "name": name,
            "sortOrder": sort_order,
            "createdAt": created_at,
            "updatedAt": updated_at,
        });
        write_json(&collection_dir.join("collection.json"), &collection_json)?;
        counts.collections += 1;
    }

    // 请求：封面目录平铺，冲突时追加 id
    let mut stmt = conn
        .prepare(
            "SELECT id, collection_id, name, method, COALESCE(protocol, 'http'), url,
                    COALESCE(headers, '[]'), COALESCE(params, '[]'),
                    COALESCE(body_type, 'none'), COALESCE(body, ''),
                    COALESCE(body_meta, ''), COALESCE(pre_script, ''),
                    COALESCE(post_script, ''), COALESCE(assertions, ''),
                    request_entity, response_entity, sort_order, created_at, updated_at
             FROM requests
             WHERE collection_id IN (SELECT id FROM collections WHERE project_id = ?1)
             ORDER BY sort_order ASC, id ASC",
        )
        .map_err(err)?;
    let requests: Vec<RequestRow> = stmt
        .query_map(params![project_id], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
                row.get(7)?,
                row.get(8)?,
                row.get(9)?,
                row.get(10)?,
                row.get(11)?,
                row.get(12)?,
                row.get(13)?,
                row.get(14)?,
                row.get(15)?,
                row.get(16)?,
                row.get(17)?,
                row.get(18)?,
            ))
        })
        .map_err(err)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(err)?;
    let mut request_used = HashSet::new();
    let mut auth_omitted = 0usize;
    for (
        id,
        collection_id,
        name,
        method,
        protocol,
        url,
        headers,
        query,
        body_type,
        body,
        body_meta,
        pre_script,
        post_script,
        assertions,
        request_entity,
        response_entity,
        sort_order,
        created_at,
        updated_at,
    ) in requests
    {
        let file_name = format!("{}.json", unique_name(&mut request_used, slug(&name), id));
        let headers: Value = serde_json::from_str(&headers).unwrap_or_else(|_| json!([]));
        let query: Value = serde_json::from_str(&query).unwrap_or_else(|_| json!([]));
        let request_json = json!({
            "id": id,
            "collectionId": collection_id,
            "name": name,
            "method": method,
            "protocol": protocol,
            "url": url,
            "headers": headers,
            "params": query,
            "bodyType": body_type,
            "body": body,
            "bodyMeta": body_meta,
            "preScript": pre_script,
            "postScript": post_script,
            "assertions": assertions,
            "requestEntity": request_entity,
            "responseEntity": response_entity,
            "sortOrder": sort_order,
            "createdAt": created_at,
            "updatedAt": updated_at,
        });
        write_json(&project_dir.join("requests").join(file_name), &request_json)?;
        counts.requests += 1;
        let auth_configured: bool = conn
            .query_row(
                "SELECT COALESCE(auth, '') NOT IN ('', '{\"kind\":\"none\"}')
                 FROM requests WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .unwrap_or(false);
        if auth_configured {
            auth_omitted += 1;
        }
    }
    if auth_omitted > 0 {
        warnings.push(format!(
            "项目「{project_name}」有 {auth_omitted} 个请求配置了认证，认证凭据不导出"
        ));
    }

    // 环境：敏感值只写占位符
    let mut stmt = conn
        .prepare(
            "SELECT id, name, is_active, is_prod, script, created_at
             FROM environments WHERE project_id = ?1 ORDER BY id",
        )
        .map_err(err)?;
    let environments: Vec<(i64, String, i64, i64, Option<String>, i64)> = stmt
        .query_map(params![project_id], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
            ))
        })
        .map_err(err)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(err)?;
    let mut env_used = HashSet::new();
    let mut secret_count = 0usize;
    for (id, name, is_active, is_prod, script, created_at) in environments {
        let mut var_stmt = conn
            .prepare(
                "SELECT id, \"key\", COALESCE(value, ''), is_secret, COALESCE(sort_order, 0)
                 FROM env_variables WHERE env_id = ?1 ORDER BY sort_order ASC, id ASC",
            )
            .map_err(err)?;
        let variables: Vec<(i64, String, String, i64, i64)> = var_stmt
            .query_map(params![id], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            })
            .map_err(err)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(err)?;
        let mut items = Vec::new();
        for (var_id, key, value, is_secret, sort_order) in variables {
            let secret = is_secret != 0;
            if secret {
                secret_count += 1;
            }
            items.push(json!({
                "id": var_id,
                "key": key,
                "value": if secret { masked_secret(&key) } else { value },
                "isSecret": secret,
                "sortOrder": sort_order,
            }));
        }
        let file_name = format!("{}.json", unique_name(&mut env_used, slug(&name), id));
        let env_json = json!({
            "id": id,
            "projectId": project_id,
            "name": name,
            "isActive": is_active != 0,
            "isProd": is_prod != 0,
            "script": script.unwrap_or_default(),
            "createdAt": created_at,
            "variables": items,
        });
        write_json(&project_dir.join("environments").join(file_name), &env_json)?;
        counts.environments += 1;
    }
    if secret_count > 0 {
        warnings.push(format!(
            "项目「{project_name}」有 {secret_count} 个敏感环境变量只导出变量名，导入后需要补值"
        ));
    }

    let mut entry = Map::new();
    entry.insert("id".into(), json!(project_id));
    entry.insert("name".into(), json!(name));
    entry.insert("dir".into(), json!(dir_name));
    Ok(entry)
}

pub fn export(
    conn: &Connection,
    payload: &WorkspaceExportPayload,
) -> Result<WorkspaceExportResult, String> {
    let root = crate::services::paths::prepare_dir(&payload.path)?;

    let projects = query_projects(conn, &payload.project_ids)?;
    let mut counts = WorkspaceCounts::default();
    let mut warnings = Vec::new();
    let mut entries = Vec::new();
    for (id, name) in &projects {
        entries.push(export_project(
            conn,
            *id,
            name,
            &root,
            &mut counts,
            &mut warnings,
        )?);
        counts.projects += 1;
    }
    let manifest = json!({
        "version": 1,
        "exportedAt": now_ms(),
        "projects": entries,
    });
    write_json(&root.join("manifest.json"), &manifest)?;
    Ok(WorkspaceExportResult {
        path: root.to_string_lossy().to_string(),
        counts,
        warnings,
    })
}

fn text(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn integer(value: &Value, key: &str) -> i64 {
    value.get(key).and_then(Value::as_i64).unwrap_or(0)
}

fn boolean(value: &Value, key: &str) -> bool {
    value.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn conflict(result: &mut WorkspaceImportResult, kind: &str, id: i64, name: &str, reason: &str) {
    result.conflicts.push(WorkspaceConflict {
        kind: kind.to_string(),
        id,
        name: name.to_string(),
        reason: reason.to_string(),
    });
}

fn safe_dir_name(raw: &str) -> Option<String> {
    let name = raw.trim();
    if name.is_empty() || name.contains('/') || name.contains('\\') || name == ".." {
        return None;
    }
    Some(name.to_string())
}

fn read_dir_json(dir: &Path, file: &str, out: &mut Vec<(PathBuf, Value)>) -> Result<(), String> {
    if !dir.is_dir() {
        return Ok(());
    }
    let entries = fs::read_dir(dir).map_err(|error| format!("读取目录失败：{error}"))?;
    for entry in entries {
        let entry = entry.map_err(|error| format!("读取目录失败：{error}"))?;
        let path = entry.path();
        if path.is_dir() {
            let target = path.join(file);
            if target.is_file() {
                out.push((target.clone(), read_json(&target)?));
            }
            read_dir_json(&path, file, out)?;
        }
    }
    Ok(())
}

fn apply_collection(
    conn: &Connection,
    project_id: i64,
    parent: Option<i64>,
    value: &Value,
    overwrite: bool,
    result: &mut WorkspaceImportResult,
) -> Result<(), String> {
    let id = integer(value, "id");
    let name = text(value, "name");
    let current: Option<(Option<i64>, String, i64)> = conn
        .query_row(
            "SELECT parent_id, name, sort_order FROM collections WHERE id = ?1",
            params![id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(err)?;
    let incoming = (parent, name.clone(), integer(value, "sortOrder"));
    match current {
        Some(existing) if existing == incoming => result.skipped += 1,
        Some(_) if overwrite => {
            conn.execute(
                "UPDATE collections SET project_id = ?1, parent_id = ?2, name = ?3,
                        sort_order = ?4, updated_at = ?5 WHERE id = ?6",
                params![project_id, incoming.0, incoming.1, incoming.2, now_ms(), id],
            )
            .map_err(err)?;
            result.updated.collections += 1;
        }
        Some(_) => conflict(
            result,
            "collection",
            id,
            &name,
            "同 id 集合内容不同，已跳过",
        ),
        None => {
            conn.execute(
                "INSERT INTO collections
                    (id, project_id, parent_id, name, sort_order, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    id,
                    project_id,
                    incoming.0,
                    incoming.1,
                    incoming.2,
                    integer(value, "createdAt"),
                    integer(value, "updatedAt"),
                ],
            )
            .map_err(err)?;
            result.created.collections += 1;
        }
    }
    Ok(())
}

fn read_all_json(dir: &Path, out: &mut Vec<(PathBuf, Value)>) -> Result<(), String> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(dir).map_err(|error| format!("读取目录失败：{error}"))? {
        let entry = entry.map_err(|error| format!("读取目录失败：{error}"))?;
        let path = entry.path();
        if path.is_dir() {
            read_all_json(&path, out)?;
        } else if path.extension().and_then(|value| value.to_str()) == Some("json") {
            out.push((path.clone(), read_json(&path)?));
        }
    }
    Ok(())
}

fn import_project(
    conn: &Connection,
    project_dir: &Path,
    overwrite: bool,
    result: &mut WorkspaceImportResult,
) -> Result<(), String> {
    let project_json = read_json(&project_dir.join("project.json"))?;
    let project_id = integer(&project_json, "id");
    let name = text(&project_json, "name");
    if project_id <= 0 || name.trim().is_empty() {
        result.warnings.push(format!(
            "跳过 {}：project.json 缺少 id / name",
            project_dir.display()
        ));
        return Ok(());
    }
    let existing: Option<(String, String, String, i64)> = conn
        .query_row(
            "SELECT name, COALESCE(description, ''), COALESCE(color, ''), sort_order
             FROM projects WHERE id = ?1",
            params![project_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()
        .map_err(err)?;
    let incoming = (
        name.clone(),
        text(&project_json, "description"),
        text(&project_json, "color"),
        integer(&project_json, "sortOrder"),
    );
    match existing {
        Some(current) if current == incoming => result.skipped += 1,
        Some(_) if overwrite => {
            conn.execute(
                "UPDATE projects SET name = ?1, description = ?2, color = ?3, sort_order = ?4,
                        updated_at = ?5 WHERE id = ?6",
                params![
                    name,
                    incoming.1,
                    incoming.2,
                    incoming.3,
                    now_ms(),
                    project_id
                ],
            )
            .map_err(err)?;
            result.updated.projects += 1;
        }
        Some(_) => {
            conflict(
                result,
                "project",
                project_id,
                &name,
                "同 id 项目内容不同，已跳过（可用 overwrite 覆盖）",
            );
            return Ok(());
        }
        None => {
            conn.execute(
                "INSERT INTO projects
                    (id, name, description, color, sort_order, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    project_id,
                    name,
                    incoming.1,
                    incoming.2,
                    incoming.3,
                    integer(&project_json, "createdAt"),
                    integer(&project_json, "updatedAt"),
                ],
            )
            .map_err(err)?;
            result.created.projects += 1;
        }
    }

    // 集合：先按 json 排序，父目录可能还没入库；两轮插入保证父子顺序
    let mut collections = Vec::new();
    read_dir_json(
        &project_dir.join("collections"),
        "collection.json",
        &mut collections,
    )?;
    let mut pending: Vec<Value> = collections.into_iter().map(|(_, value)| value).collect();
    let mut guard = 0;
    while !pending.is_empty() && guard < 64 {
        guard += 1;
        let mut next = Vec::new();
        let mut progressed = false;
        for value in pending.drain(..) {
            let id = integer(&value, "id");
            if id <= 0 {
                result.warnings.push("跳过缺少 id 的集合".to_string());
                continue;
            }
            let parent = value
                .get("parentId")
                .and_then(Value::as_i64)
                .filter(|parent| *parent > 0);
            if let Some(parent) = parent {
                let parent_exists: bool = conn
                    .query_row(
                        "SELECT COUNT(*) FROM collections WHERE id = ?1 AND project_id = ?2",
                        params![parent, project_id],
                        |row| Ok(row.get::<_, i64>(0)? > 0),
                    )
                    .unwrap_or(false);
                if !parent_exists {
                    next.push(value);
                    continue;
                }
            }
            apply_collection(conn, project_id, parent, &value, overwrite, result)?;
            progressed = true;
        }
        if !progressed {
            // 父集合缺失 / 成环：降级为根集合导入，保证数据不丢
            for value in next {
                let name = text(&value, "name");
                let parent = value
                    .get("parentId")
                    .and_then(Value::as_i64)
                    .filter(|parent| *parent > 0);
                result.warnings.push(format!(
                    "集合「{name}」的父集合 #{} 不存在，已作为根集合导入",
                    parent.unwrap_or(0)
                ));
                apply_collection(conn, project_id, None, &value, overwrite, result)?;
            }
            break;
        }
        pending = next;
    }
    for value in pending {
        conflict(
            result,
            "collection",
            integer(&value, "id"),
            &text(&value, "name"),
            "父子层级过深或成环，已跳过",
        );
    }

    // 请求
    let mut requests = Vec::new();
    read_all_json(&project_dir.join("requests"), &mut requests)?;
    for (path, value) in requests {
        let id = integer(&value, "id");
        let name = text(&value, "name");
        let collection_id = integer(&value, "collectionId");
        if id <= 0 || name.trim().is_empty() || collection_id <= 0 {
            result.warnings.push(format!(
                "跳过 {}：id / name / collectionId 缺失",
                path.display()
            ));
            continue;
        }
        let collection_in_project: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM collections WHERE id = ?1 AND project_id = ?2",
                params![collection_id, project_id],
                |row| Ok(row.get::<_, i64>(0)? > 0),
            )
            .unwrap_or(false);
        if !collection_in_project {
            conflict(
                result,
                "request",
                id,
                &name,
                "所属集合不存在或不属于该项目，已跳过",
            );
            continue;
        }
        let protocol = {
            let raw = text(&value, "protocol");
            if matches!(raw.as_str(), "graphql" | "sse" | "websocket") {
                raw
            } else {
                "http".to_string()
            }
        };
        let headers = Value::Array(
            value
                .get("headers")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default(),
        );
        let query = Value::Array(
            value
                .get("params")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default(),
        );
        let existing: Option<crate::models::HttpRequestDetail> =
            crate::services::request::find_request(conn, id).ok();
        let incoming = (
            collection_id,
            name.clone(),
            text(&value, "method"),
            protocol.clone(),
            text(&value, "url"),
            headers.to_string(),
            query.to_string(),
            text(&value, "bodyType"),
            text(&value, "body"),
            text(&value, "bodyMeta"),
            text(&value, "preScript"),
            text(&value, "postScript"),
            text(&value, "assertions"),
            value
                .get("requestEntity")
                .and_then(Value::as_str)
                .map(str::to_string),
            value
                .get("responseEntity")
                .and_then(Value::as_str)
                .map(str::to_string),
        );
        let same = existing.as_ref().map(|item| {
            item.collection_id == incoming.0
                && item.name == incoming.1
                && item.method == incoming.2
                && item.protocol == incoming.3
                && item.url == incoming.4
                && serde_json::to_string(&item.headers).unwrap_or_default() == incoming.5
                && serde_json::to_string(&item.params).unwrap_or_default() == incoming.6
                && item.body_type == incoming.7
                && item.body == incoming.8
                && item.body_meta == incoming.9
                && item.pre_script == incoming.10
                && item.post_script == incoming.11
                && item.assertions == incoming.12
                && item.request_entity == incoming.13
                && item.response_entity == incoming.14
        });
        if same == Some(true) {
            result.skipped += 1;
            continue;
        }
        if same.is_some() && !overwrite {
            conflict(
                result,
                "request",
                id,
                &name,
                "同 id 请求内容不同，已跳过（可用 overwrite 覆盖）",
            );
            continue;
        }
        if same.is_some() {
            conn.execute(
                "UPDATE requests SET collection_id = ?1, name = ?2, method = ?3, protocol = ?4,
                        url = ?5, headers = ?6, params = ?7, body_type = ?8, body = ?9,
                        body_meta = ?10, pre_script = ?11, post_script = ?12, assertions = ?13,
                        request_entity = ?14, response_entity = ?15, sort_order = ?16,
                        updated_at = ?17 WHERE id = ?18",
                params![
                    incoming.0,
                    incoming.1,
                    incoming.2,
                    incoming.3,
                    incoming.4,
                    incoming.5,
                    incoming.6,
                    incoming.7,
                    incoming.8,
                    incoming.9,
                    incoming.10,
                    incoming.11,
                    incoming.12,
                    incoming.13,
                    incoming.14,
                    integer(&value, "sortOrder"),
                    now_ms(),
                    id
                ],
            )
            .map_err(err)?;
            result.updated.requests += 1;
        } else {
            conn.execute(
                "INSERT INTO requests
                    (id, collection_id, name, method, protocol, url, headers, params, body_type,
                     body, body_meta, pre_script, post_script, assertions, auth,
                     request_entity, response_entity, sort_order, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14,
                         '{\"kind\":\"none\"}', ?15, ?16, ?17, ?18, ?19)",
                params![
                    id,
                    incoming.0,
                    incoming.1,
                    incoming.2,
                    incoming.3,
                    incoming.4,
                    incoming.5,
                    incoming.6,
                    incoming.7,
                    incoming.8,
                    incoming.9,
                    incoming.10,
                    incoming.11,
                    incoming.12,
                    incoming.13,
                    incoming.14,
                    integer(&value, "sortOrder"),
                    integer(&value, "createdAt"),
                    integer(&value, "updatedAt"),
                ],
            )
            .map_err(err)?;
            let _ = crate::services::revision::record_in_tx(conn, id, "import", None);
            result.created.requests += 1;
        }
    }

    // 环境
    let mut environments = Vec::new();
    read_all_json(&project_dir.join("environments"), &mut environments)?;
    for (path, value) in environments {
        let id = integer(&value, "id");
        let name = text(&value, "name");
        if id <= 0 || name.trim().is_empty() {
            result
                .warnings
                .push(format!("跳过 {}：id / name 缺失", path.display()));
            continue;
        }
        let is_active = boolean(&value, "isActive") as i64;
        let is_prod = boolean(&value, "isProd") as i64;
        let script = text(&value, "script");
        let existing: Option<(String, i64, i64, String)> = conn
            .query_row(
                "SELECT name, is_active, is_prod, COALESCE(script, '')
                 FROM environments WHERE id = ?1",
                params![id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()
            .map_err(err)?;
        let incoming = (name.clone(), is_active, is_prod, script.clone());
        let same = existing
            .as_ref()
            .map(|item| item == &incoming)
            .unwrap_or(false);
        match existing {
            Some(_) if same => result.skipped += 1,
            Some(_) if overwrite => {
                conn.execute(
                    "UPDATE environments SET project_id = ?1, name = ?2, is_active = ?3,
                            is_prod = ?4, script = ?5 WHERE id = ?6",
                    params![project_id, incoming.0, incoming.1, incoming.2, incoming.3, id],
                )
                .map_err(err)?;
                result.updated.environments += 1;
            }
            Some(_) => conflict(
                result,
                "environment",
                id,
                &name,
                "同 id 环境内容不同，已跳过",
            ),
            None => {
                conn.execute(
                    "INSERT INTO environments
                        (id, project_id, name, is_active, is_prod, script, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![
                        id,
                        project_id,
                        incoming.0,
                        incoming.1,
                        incoming.2,
                        incoming.3,
                        integer(&value, "createdAt"),
                    ],
                )
                .map_err(err)?;
                result.created.environments += 1;
            }
        }
        let Some(variables) = value.get("variables").and_then(Value::as_array) else {
            continue;
        };
        for variable in variables {
            let key = text(variable, "key");
            if key.trim().is_empty() {
                continue;
            }
            let is_secret = boolean(variable, "isSecret");
            let raw_value = text(variable, "value");
            let needs_fill = is_secret && raw_value == masked_secret(&key);
            let existing: Option<(String, i64)> = conn
                .query_row(
                    "SELECT COALESCE(value, ''), is_secret FROM env_variables
                     WHERE env_id = ?1 AND \"key\" = ?2",
                    params![id, key],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()
                .map_err(err)?;
            // 导出里只有占位符，说明明文本来就没落盘；已存在时保留原值，
            // 不存在时才建一条待补值（空密文）的变量，避免把用户的密钥清空。
            if needs_fill {
                result
                    .warnings
                    .push(format!("敏感变量 {key} 需要补值（环境「{name}」）"));
                if existing.is_some() {
                    continue;
                }
            }
            let same = existing
                .as_ref()
                .map(|(old_value, old_secret)| {
                    if is_secret {
                        *old_secret != 0
                            && crate::secret::decrypt(crate::secret::AAD_ENV, old_value)
                                == raw_value
                    } else {
                        *old_secret == 0 && old_value == &raw_value
                    }
                })
                .unwrap_or(false);
            if same {
                continue;
            }
            let value = if is_secret && !needs_fill {
                crate::secret::encrypt(crate::secret::AAD_ENV, &raw_value)?
            } else if needs_fill {
                crate::secret::encrypt(crate::secret::AAD_ENV, "")?
            } else {
                raw_value
            };
            if existing.is_some() && overwrite {
                conn.execute(
                    "UPDATE env_variables SET value = ?1, is_secret = ?2, sort_order = ?3
                     WHERE env_id = ?4 AND \"key\" = ?5",
                    params![
                        value,
                        is_secret as i64,
                        integer(variable, "sortOrder"),
                        id,
                        key
                    ],
                )
                .map_err(err)?;
            } else if existing.is_some() {
                conflict(
                    result,
                    "env_variable",
                    id,
                    &key,
                    "同环境变量内容不同，已跳过",
                );
            } else {
                let var_id = integer(variable, "id");
                let id_taken = var_id > 0
                    && conn
                        .query_row(
                            "SELECT COUNT(*) FROM env_variables WHERE id = ?1",
                            params![var_id],
                            |row| row.get::<_, i64>(0),
                        )
                        .map_err(err)?
                        > 0;
                if var_id > 0 && !id_taken {
                    conn.execute(
                        "INSERT INTO env_variables
                            (id, env_id, \"key\", value, is_secret, sort_order)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                        params![
                            var_id,
                            id,
                            key,
                            value,
                            is_secret as i64,
                            integer(variable, "sortOrder"),
                        ],
                    )
                    .map_err(err)?;
                } else {
                    // 显式 id 已被其它环境的变量占用时退回自增，避免整库导入中断
                    conn.execute(
                        "INSERT INTO env_variables (env_id, \"key\", value, is_secret, sort_order)
                         VALUES (?1, ?2, ?3, ?4, ?5)",
                        params![
                            id,
                            key,
                            value,
                            is_secret as i64,
                            integer(variable, "sortOrder"),
                        ],
                    )
                    .map_err(err)?;
                    if id_taken {
                        result
                            .warnings
                            .push(format!("变量 {key} 的 id {var_id} 已被占用，改用自增 id"));
                    }
                }
            }
        }
    }
    Ok(())
}

pub fn import(
    conn: &mut Connection,
    payload: &WorkspaceImportPayload,
) -> Result<WorkspaceImportResult, String> {
    let root = crate::services::paths::prepare_existing_dir(&payload.path)?;
    let manifest = read_json(&root.join("manifest.json"))?;
    if integer(&manifest, "version") != 1 {
        return Err("E_FORMAT: 仅支持 version = 1 的工作区".to_string());
    }
    let mut result = WorkspaceImportResult::default();
    let Some(projects) = manifest.get("projects").and_then(Value::as_array) else {
        return Err("manifest.json 缺少 projects".to_string());
    };
    let tx = conn.unchecked_transaction().map_err(err)?;
    for project in projects {
        let Some(dir_name) = safe_dir_name(&text(project, "dir")) else {
            result.warnings.push("跳过非法项目目录名".to_string());
            continue;
        };
        let project_dir = root.join(dir_name);
        if !project_dir.is_dir() {
            result
                .warnings
                .push(format!("项目目录不存在：{}", project_dir.display()));
            continue;
        }
        import_project(&tx, &project_dir, payload.overwrite, &mut result)?;
    }
    tx.commit().map_err(err)?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use r2d2_sqlite::SqliteConnectionManager;

    fn pool(tag: &str) -> crate::state::DbPool {
        crate::secret::install_for_test();
        let dir = crate::util::test_dir(tag);
        let manager = SqliteConnectionManager::file(dir.join("astroman.db"));
        let pool = r2d2::Pool::builder().max_size(2).build(manager).unwrap();
        db::init(&pool).unwrap();
        pool
    }

    /// 新库自带示例项目，id 取当前最大值之后，返回 (projectId, requestId)
    fn seed(conn: &Connection) -> (i64, i64) {
        let base: i64 = conn
            .query_row(
                "SELECT COALESCE(MAX(id), 0) + 100 FROM projects",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let project = base;
        let collection = base + 1;
        let request = base + 2;
        let env = base + 3;
        let var_base = base + 4;
        conn.execute(
            "INSERT INTO projects (id, name, description, color, sort_order, created_at, updated_at)
             VALUES (?1, 'Demo', 'desc', '#000', 0, 100, 100)",
            params![project],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO collections (id, project_id, parent_id, name, sort_order, created_at, updated_at)
             VALUES (?1, ?2, NULL, 'Users', 0, 100, 100)",
            params![collection, project],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO requests (id, collection_id, name, method, protocol, url, headers, params,
                body_type, body, body_meta, pre_script, post_script, assertions, auth,
                sort_order, created_at, updated_at)
             VALUES (?1, ?2, 'List users', 'GET', 'http', '{{base_url}}/users', '[]', '[]',
                'none', '', '', '', '', '', '{\"kind\":\"none\"}', 0, 100, 100)",
            params![request, collection],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO environments (id, project_id, name, is_active, is_prod, script, created_at)
             VALUES (?1, ?2, 'dev', 1, 0, '', 100)",
            params![env, project],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO env_variables (id, env_id, \"key\", value, is_secret, sort_order)
             VALUES (?1, ?2, 'base_url', 'https://api.example.com', 0, 0)",
            params![var_base, env],
        )
        .unwrap();
        let secret = crate::secret::encrypt(crate::secret::AAD_ENV, "token-value").unwrap();
        conn.execute(
            "INSERT INTO env_variables (id, env_id, \"key\", value, is_secret, sort_order)
             VALUES (?1, ?2, 'token', ?3, 1, 1)",
            params![var_base + 1, env, secret],
        )
        .unwrap();
        (project, request)
    }

    #[test]
    fn export_then_import_round_trip_is_idempotent() {
        let pool = pool("workspace_round_trip");
        let mut conn = pool.get().unwrap();
        let (project, request) = seed(&conn);
        let dir = crate::util::test_dir("workspace_round_trip_out");
        let payload = WorkspaceExportPayload {
            path: dir.to_string_lossy().to_string(),
            project_ids: vec![project],
        };
        let exported = export(&conn, &payload).unwrap();
        assert_eq!(exported.counts.projects, 1);
        assert_eq!(exported.counts.requests, 1);

        let token_file = std::fs::read_to_string(
            dir.join(format!("project-{project}-demo/environments/dev.json")),
        )
        .unwrap();
        assert!(token_file.contains("{{secret:token}}"));
        assert!(!token_file.contains("token-value"));

        // 清库后导入：id / 数量 / 敏感值占位都恢复
        conn.execute("DELETE FROM env_variables", []).unwrap();
        conn.execute("DELETE FROM environments", []).unwrap();
        conn.execute("DELETE FROM requests", []).unwrap();
        conn.execute("DELETE FROM collections", []).unwrap();
        conn.execute("DELETE FROM projects", []).unwrap();
        let imported = import(
            &mut conn,
            &WorkspaceImportPayload {
                path: dir.to_string_lossy().to_string(),
                overwrite: false,
            },
        )
        .unwrap();
        assert_eq!(imported.created.projects, 1);
        assert_eq!(imported.created.requests, 1);
        assert!(imported.warnings.iter().any(|item| item.contains("补值")));
        let (name, protocol): (String, String) = conn
            .query_row(
                "SELECT name, protocol FROM requests WHERE id = ?1",
                params![request],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(name, "List users");
        assert_eq!(protocol, "http");

        // 再导入一次：全部跳过，无重复
        let again = import(
            &mut conn,
            &WorkspaceImportPayload {
                path: dir.to_string_lossy().to_string(),
                overwrite: false,
            },
        )
        .unwrap();
        assert_eq!(again.created.requests, 0);
        assert!(again.conflicts.is_empty());
    }

    #[test]
    fn conflicting_content_is_reported_without_overwrite() {
        let pool = pool("workspace_conflict");
        let mut conn = pool.get().unwrap();
        let (project, _) = seed(&conn);
        let dir = crate::util::test_dir("workspace_conflict_out");
        export(
            &conn,
            &WorkspaceExportPayload {
                path: dir.to_string_lossy().to_string(),
                project_ids: vec![project],
            },
        )
        .unwrap();
        conn.execute(
            "UPDATE projects SET name = 'Renamed' WHERE id = ?1",
            params![project],
        )
        .unwrap();
        let result = import(
            &mut conn,
            &WorkspaceImportPayload {
                path: dir.to_string_lossy().to_string(),
                overwrite: false,
            },
        )
        .unwrap();
        assert!(result.conflicts.iter().any(|item| item.kind == "project"));
        let name: String = conn
            .query_row(
                "SELECT name FROM projects WHERE id = ?1",
                params![project],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(name, "Renamed");
    }

    #[test]
    fn rejects_manifest_with_unknown_version() {
        let pool = pool("workspace_version");
        let mut conn = pool.get().unwrap();
        let dir = crate::util::test_dir("workspace_version_out");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("manifest.json"), "{\"version\":2,\"projects\":[]}").unwrap();
        let error = import(
            &mut conn,
            &WorkspaceImportPayload {
                path: dir.to_string_lossy().to_string(),
                overwrite: false,
            },
        )
        .unwrap_err();
        assert!(error.contains("E_FORMAT"));
    }
}
