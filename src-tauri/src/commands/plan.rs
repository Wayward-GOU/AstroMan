use std::collections::HashMap;

use crate::state::AppState;
use crate::util::{err, now_ms, render};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::State;

/// 计划列表行：除基础字段外补齐目标/环境/最近运行，便于全局计划中心直接渲染（2.1.2）
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanSummary {
    pub id: i64,
    pub project_id: i64,
    pub project_name: Option<String>,
    pub request_id: Option<i64>,
    pub request_name: Option<String>,
    /// 接口已被删除或计划未绑定接口
    pub request_missing: bool,
    pub name: String,
    pub method: Option<String>,
    pub url_template: Option<String>,
    pub resolved_url: Option<String>,
    pub env_id: Option<i64>,
    pub env_name: Option<String>,
    pub mode: String,
    pub stages_summary: String,
    pub last_run_at: Option<i64>,
    pub last_status: Option<String>,
    pub report_count: i64,
    pub updated_at: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestPlan {
    pub id: i64,
    pub project_id: i64,
    pub request_id: Option<i64>,
    pub name: String,
    pub config: Value,
    pub updated_at: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanInput {
    #[serde(default)]
    pub id: Option<i64>,
    pub project_id: i64,
    #[serde(default)]
    pub request_id: Option<i64>,
    pub name: String,
    #[serde(default)]
    pub config: Value,
}

fn config_value(raw: Option<String>) -> Value {
    raw.and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .unwrap_or(Value::Null)
}

fn config_str(config: &Value, key: &str) -> Option<String> {
    config
        .get(key)
        .and_then(|value| value.as_str())
        .map(|v| v.to_string())
}

fn config_i64(config: &Value, key: &str) -> Option<i64> {
    config.get(key).and_then(|value| value.as_i64())
}

/// 计划里保存的目标快照（2.2.1 配置快照 v2）
struct TargetSnapshot {
    method: Option<String>,
    url_template: Option<String>,
    resolved_url: Option<String>,
}

fn target_snapshot(config: &Value) -> TargetSnapshot {
    let target = config.get("target").cloned().unwrap_or(Value::Null);
    TargetSnapshot {
        method: target
            .get("method")
            .and_then(|v| v.as_str())
            .map(|v| v.to_string()),
        url_template: target
            .get("urlTemplate")
            .and_then(|v| v.as_str())
            .map(|v| v.to_string()),
        resolved_url: target
            .get("resolvedUrl")
            .and_then(|v| v.as_str())
            .map(|v| v.to_string()),
    }
}

fn plan_mode(config: &Value) -> String {
    let mode = config
        .get("settings")
        .and_then(|v| v.get("mode"))
        .and_then(|v| v.as_str());
    match mode {
        Some("stages") => "stages".to_string(),
        Some(_) => "fixed".to_string(),
        None => {
            if config
                .get("stages")
                .and_then(|v| v.as_array())
                .map(|items| !items.is_empty())
                .unwrap_or(false)
            {
                "stages".to_string()
            } else {
                "fixed".to_string()
            }
        }
    }
}

fn stages_summary(config: &Value) -> String {
    if let Some(items) = config.get("stages").and_then(|v| v.as_array()) {
        if !items.is_empty() {
            let peak = items
                .iter()
                .filter_map(|item| item.get("concurrent").and_then(|v| v.as_i64()))
                .max()
                .unwrap_or(0);
            let total: i64 = items
                .iter()
                .filter_map(|item| item.get("durationSec").and_then(|v| v.as_i64()))
                .sum();
            return format!("阶梯 {} 段 · 峰值 {peak} 并发 · 共 {total}s", items.len());
        }
    }
    let settings = config.get("settings").cloned().unwrap_or(Value::Null);
    let concurrent = settings
        .get("concurrent")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let duration = settings
        .get("durationSec")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    format!("固定 {concurrent} 并发 · {duration}s")
}

/// 环境名与变量缓存，避免列表中逐计划重复查库
struct EnvCache<'a> {
    conn: &'a Connection,
    names: HashMap<i64, Option<String>>,
    vars: HashMap<i64, HashMap<String, String>>,
}

impl<'a> EnvCache<'a> {
    fn new(conn: &'a Connection) -> Self {
        Self {
            conn,
            names: HashMap::new(),
            vars: HashMap::new(),
        }
    }

    fn name(&mut self, env_id: i64) -> Option<String> {
        if !self.names.contains_key(&env_id) {
            let name = self
                .conn
                .query_row(
                    "SELECT name FROM environments WHERE id = ?1",
                    params![env_id],
                    |row| row.get::<_, String>(0),
                )
                .optional()
                .unwrap_or(None);
            self.names.insert(env_id, name);
        }
        self.names.get(&env_id).cloned().flatten()
    }

    fn vars(&mut self, env_id: i64) -> HashMap<String, String> {
        if !self.vars.contains_key(&env_id) {
            let vars = crate::commands::http::load_vars(self.conn, env_id).unwrap_or_default();
            self.vars.insert(env_id, vars);
        }
        self.vars.get(&env_id).cloned().unwrap_or_default()
    }

    fn resolve(&mut self, env_id: Option<i64>, template: &str) -> Option<String> {
        let env_id = env_id?;
        let vars = self.vars(env_id);
        if vars.is_empty() {
            return None;
        }
        let text = render(template, &vars);
        if text.trim().is_empty() || text.contains("{{") {
            return None;
        }
        Some(text)
    }
}

#[tauri::command]
pub fn plan_list(
    state: State<'_, AppState>,
    project_id: Option<i64>,
) -> Result<Vec<PlanSummary>, String> {
    let conn = state.pool.get().map_err(err)?;
    let mut stmt = conn
        .prepare(
            "SELECT p.id, p.project_id, pr.name, p.request_id, r.name, r.method, r.url,
                    p.name, p.config_json, p.updated_at,
                    (SELECT COUNT(*) FROM test_reports tr WHERE tr.plan_id = p.id),
                    (SELECT tr.status FROM test_reports tr
                       WHERE tr.plan_id = p.id ORDER BY tr.created_at DESC, tr.id DESC LIMIT 1),
                    (SELECT tr.created_at FROM test_reports tr
                       WHERE tr.plan_id = p.id ORDER BY tr.created_at DESC, tr.id DESC LIMIT 1)
             FROM test_plans p
             LEFT JOIN requests r ON r.id = p.request_id
             LEFT JOIN projects pr ON pr.id = p.project_id
             WHERE (?1 IS NULL OR p.project_id = ?1)
             ORDER BY p.updated_at DESC, p.id DESC",
        )
        .map_err(err)?;
    let rows = stmt
        .query_map(params![project_id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<i64>>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, Option<String>>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, Option<String>>(8)?,
                row.get::<_, i64>(9)?,
                row.get::<_, i64>(10)?,
                row.get::<_, Option<String>>(11)?,
                row.get::<_, Option<i64>>(12)?,
            ))
        })
        .map_err(err)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(err)?;
    drop(stmt);

    let mut cache = EnvCache::new(&conn);
    let mut result = Vec::with_capacity(rows.len());
    for row in rows {
        let (
            id,
            project_id,
            project_name,
            request_id,
            request_name,
            request_method,
            request_url,
            name,
            config_raw,
            updated_at,
            report_count,
            last_status,
            last_run_at,
        ) = row;
        let config = config_value(config_raw);
        let target = target_snapshot(&config);
        let env_id = config_i64(&config, "envId");
        let env_name =
            config_str(&config, "envName").or_else(|| env_id.and_then(|id| cache.name(id)));
        let method = target.method.clone().or(request_method);
        let url_template = target.url_template.clone().or(request_url);
        let resolved_url = target.resolved_url.clone().or_else(|| {
            url_template
                .as_deref()
                .and_then(|template| cache.resolve(env_id, template))
        });
        result.push(PlanSummary {
            id,
            project_id,
            project_name,
            request_id,
            request_missing: request_name.is_none(),
            request_name,
            name,
            method,
            url_template,
            resolved_url,
            env_id,
            env_name,
            mode: plan_mode(&config),
            stages_summary: stages_summary(&config),
            last_run_at,
            last_status,
            report_count,
            updated_at,
        });
    }
    Ok(result)
}

#[tauri::command]
pub fn plan_get(state: State<'_, AppState>, id: i64) -> Result<TestPlan, String> {
    let conn = state.pool.get().map_err(err)?;
    conn.query_row(
        "SELECT id, project_id, request_id, name, config_json, updated_at
         FROM test_plans WHERE id = ?1",
        params![id],
        |row| {
            Ok(TestPlan {
                id: row.get(0)?,
                project_id: row.get(1)?,
                request_id: row.get(2)?,
                name: row.get(3)?,
                config: config_value(row.get(4)?),
                updated_at: row.get(5)?,
            })
        },
    )
    .map_err(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => "压测计划不存在".to_string(),
        other => err(other),
    })
}

fn project_active_env(conn: &Connection, project_id: i64) -> Result<Option<(i64, String)>, String> {
    conn.query_row(
        "SELECT id, name FROM environments
         WHERE project_id = ?1 AND is_active = 1 ORDER BY id ASC LIMIT 1",
        params![project_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .optional()
    .map_err(err)
}

fn request_target(conn: &Connection, request_id: i64) -> Result<(String, String), String> {
    conn.query_row(
        "SELECT method, url FROM requests WHERE id = ?1",
        params![request_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .optional()
    .map_err(err)?
    .ok_or_else(|| "计划绑定的接口不存在或已删除".to_string())
}

/// 保存计划时补齐目标快照与环境名（2.2.1 / 2.6.3）
///
/// 计划本身只存项目/接口引用；换机器或接口被删后，列表仍要能显示
/// 当时的 method / URL 模板 / 环境名，因此在这里把快照写进 config_json。
fn enrich_plan_config(
    conn: &Connection,
    config: &mut Value,
    project_id: i64,
    request_id: Option<i64>,
) -> Result<(), String> {
    if !config.is_object() {
        *config = json!({});
    }
    let target = target_snapshot(config);
    let Some(map) = config.as_object_mut() else {
        return Ok(());
    };

    // 环境：显式 envId > 项目激活环境
    let env_id = map
        .get("envId")
        .and_then(|v| v.as_i64())
        .filter(|id| *id > 0);
    let env = match env_id {
        Some(id) => conn
            .query_row(
                "SELECT id, name FROM environments WHERE id = ?1",
                params![id],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()
            .map_err(err)?
            .ok_or_else(|| "计划绑定的环境不存在或已删除".to_string())?,
        None => match project_active_env(conn, project_id)? {
            Some(env) => env,
            None => (0, String::new()),
        },
    };
    if env.0 > 0 {
        map.insert("envId".to_string(), json!(env.0));
        map.insert("envName".to_string(), json!(env.1));
    }

    // 目标：优先接口当前值，其次已保存快照
    let (method, url_template) = match request_id {
        Some(id) => {
            let (method, url) = request_target(conn, id)?;
            (Some(method), Some(url))
        }
        None => (target.method.clone(), target.url_template.clone()),
    };
    let Some(url_template) = url_template else {
        return Ok(());
    };
    let vars = if env.0 > 0 {
        crate::commands::http::load_vars(conn, env.0).unwrap_or_default()
    } else {
        HashMap::new()
    };
    let resolved = {
        let text = render(&url_template, &vars);
        if text.trim().is_empty() || text.contains("{{") {
            None
        } else {
            Some(text)
        }
    };
    map.insert(
        "target".to_string(),
        json!({
            "requestId": request_id,
            "method": method,
            "urlTemplate": url_template,
            "resolvedUrl": resolved,
            "capturedAt": now_ms(),
        }),
    );
    Ok(())
}

#[tauri::command]
pub fn plan_save(state: State<'_, AppState>, plan: PlanInput) -> Result<TestPlan, String> {
    let name = plan.name.trim().to_string();
    if name.is_empty() {
        return Err("请填写计划名称".to_string());
    }
    let mut plan = plan;
    let conn = state.pool.get().map_err(err)?;
    // 项目必须存在，否则外键报错难以理解
    let project_exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM projects WHERE id = ?1)",
            params![plan.project_id],
            |row| row.get(0),
        )
        .map_err(err)?;
    if !project_exists {
        return Err("所属项目不存在".to_string());
    }
    enrich_plan_config(&conn, &mut plan.config, plan.project_id, plan.request_id)?;
    let config = serde_json::to_string(&plan.config).map_err(err)?;
    let now = now_ms();

    let id = match plan.id {
        Some(id) => {
            let changed = conn
                .execute(
                    "UPDATE test_plans SET request_id = ?1, name = ?2, config_json = ?3,
                     updated_at = ?4 WHERE id = ?5",
                    params![plan.request_id, name, config, now, id],
                )
                .map_err(err)?;
            if changed == 0 {
                return Err("压测计划不存在".to_string());
            }
            id
        }
        None => {
            conn.execute(
                "INSERT INTO test_plans (project_id, request_id, name, config_json, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
                params![plan.project_id, plan.request_id, name, config, now],
            )
            .map_err(err)?;
            conn.last_insert_rowid()
        }
    };

    drop(conn);
    plan_get(state, id)
}

#[tauri::command]
pub fn plan_delete(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    // 运行中的计划禁止删除（2.3.3）
    if state
        .stress_jobs
        .snapshot()
        .iter()
        .any(|job| job.plan_id == Some(id))
    {
        return Err("该计划正在运行，停止任务后再删除".to_string());
    }
    let conn = state.pool.get().map_err(err)?;
    conn.execute("DELETE FROM test_plans WHERE id = ?1", params![id])
        .map_err(err)?;
    Ok(())
}

/// 复制计划：仅同项目，副本命名「原名 副本」，不复制历史报告（2.3.2）
#[tauri::command]
pub fn plan_duplicate(state: State<'_, AppState>, id: i64) -> Result<TestPlan, String> {
    let (project_id, request_id, name, config_raw) = {
        let conn = state.pool.get().map_err(err)?;
        conn.query_row(
            "SELECT project_id, request_id, name, config_json FROM test_plans WHERE id = ?1",
            params![id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, Option<i64>>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            },
        )
        .optional()
        .map_err(err)?
        .ok_or_else(|| "压测计划不存在".to_string())?
    };
    let mut config = config_value(config_raw);
    let now = now_ms();
    let new_id = {
        let conn = state.pool.get().map_err(err)?;
        enrich_plan_config(&conn, &mut config, project_id, request_id)?;
        conn.execute(
            "INSERT INTO test_plans (project_id, request_id, name, config_json, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
            params![
                project_id,
                request_id,
                format!("{name} 副本"),
                serde_json::to_string(&config).map_err(err)?,
                now
            ],
        )
        .map_err(err)?;
        conn.last_insert_rowid()
    };
    plan_get(state, new_id)
}

// ---------------------------------------------------------------------------
// 计划导入导出（2.4）
// ---------------------------------------------------------------------------

pub const PLAN_EXPORT_FORMAT: &str = "astroman.stress-plan/v1";

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanExportFile {
    pub format: String,
    pub exported_at: i64,
    /// 是否包含敏感信息：默认 false，剔除 auth 与变量值
    pub includes_sensitive: bool,
    pub plans: Vec<PlanExportItem>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanExportItem {
    pub name: String,
    pub project_name: Option<String>,
    /// settings + stages 原样保留（默认剔除敏感字段）
    pub config: Value,
    pub request: Option<PlanRequestSnapshot>,
    pub env: Option<PlanEnvSnapshot>,
    /// 解析后地址：默认不导出，只有 includeSensitive 才写入（2.4.2）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_url: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanRequestSnapshot {
    #[serde(default)]
    pub name: Option<String>,
    pub method: String,
    pub url: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanEnvSnapshot {
    pub name: String,
    /// 变量 key 列表
    #[serde(default)]
    pub vars: Vec<String>,
    /// 仅在 includeSensitive 时存在
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub values: HashMap<String, String>,
}

/// 递归剔除敏感字段：auth 整体、常见密钥字段、`secret:` 引用
fn strip_sensitive(value: &mut Value) {
    const SECRET_KEYS: [&str; 11] = [
        "auth",
        "authorization",
        "token",
        "accesstoken",
        "password",
        "secret",
        "clientsecret",
        "apikey",
        "cookie",
        "privatekey",
        "refreshtoken",
    ];
    match value {
        Value::Object(map) => {
            map.retain(|key, child| {
                let lower = key.to_lowercase();
                if SECRET_KEYS.contains(&lower.as_str()) {
                    return false;
                }
                if let Value::String(text) = child {
                    if text.starts_with("secret:") {
                        return false;
                    }
                }
                strip_sensitive(child);
                true
            });
        }
        Value::Array(items) => {
            for item in items {
                strip_sensitive(item);
            }
        }
        _ => {}
    }
}

#[tauri::command]
pub fn plan_export(
    state: State<'_, AppState>,
    plan_ids: Vec<i64>,
    include_sensitive: Option<bool>,
) -> Result<String, String> {
    let include_sensitive = include_sensitive.unwrap_or(false);
    let conn = state.pool.get().map_err(err)?;
    let mut plans = Vec::new();
    let mut env_cache: HashMap<i64, (String, Vec<String>, HashMap<String, String>)> =
        HashMap::new();
    for id in plan_ids {
        let row: Option<(
            String,
            Option<String>,
            Option<i64>,
            Option<String>,
            Option<String>,
            Option<String>,
        )> = conn
            .query_row(
                "SELECT p.name, pr.name, p.request_id, r.method, r.url, p.config_json
                 FROM test_plans p
                 LEFT JOIN projects pr ON pr.id = p.project_id
                 LEFT JOIN requests r ON r.id = p.request_id
                 WHERE p.id = ?1",
                params![id],
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
            .optional()
            .map_err(err)?;
        let Some((name, project_name, request_id, method, url, config_raw)) = row else {
            continue;
        };
        let mut config = config_value(config_raw);
        let target = target_snapshot(&config);
        let env_id = config_i64(&config, "envId");
        // 解析后地址只在 includeSensitive 时导出（2.4.2）
        let resolved_url = if include_sensitive {
            target.resolved_url.clone()
        } else {
            None
        };

        let request = match (request_id, method, url) {
            (Some(request_id), Some(method), Some(url)) => Some(PlanRequestSnapshot {
                name: conn
                    .query_row(
                        "SELECT name FROM requests WHERE id = ?1",
                        params![request_id],
                        |row| row.get::<_, String>(0),
                    )
                    .optional()
                    .map_err(err)?,
                method,
                url,
            }),
            _ => target.url_template.clone().map(|url| PlanRequestSnapshot {
                name: None,
                method: target.method.clone().unwrap_or_else(|| "GET".to_string()),
                url,
            }),
        };

        let env = match env_id {
            Some(env_id) => {
                if !env_cache.contains_key(&env_id) {
                    let name: Option<String> = conn
                        .query_row(
                            "SELECT name FROM environments WHERE id = ?1",
                            params![env_id],
                            |row| row.get(0),
                        )
                        .optional()
                        .map_err(err)?;
                    let vars_map =
                        crate::commands::http::load_vars(&conn, env_id).unwrap_or_default();
                    let mut keys: Vec<String> = vars_map.keys().cloned().collect();
                    keys.sort();
                    env_cache.insert(env_id, (name.unwrap_or_default(), keys, vars_map));
                }
                env_cache.get(&env_id).map(|(name, keys, values)| PlanEnvSnapshot {
                    name: name.clone(),
                    vars: keys.clone(),
                    values: if include_sensitive {
                        values.clone()
                    } else {
                        HashMap::new()
                    },
                })
            }
            None => None,
        };

        // 导出 config 时按敏感口径处理
        if !include_sensitive {
            strip_sensitive(&mut config);
            if let Some(map) = config.as_object_mut() {
                map.remove("resolvedUrl");
                map.remove("machineCapacity");
            }
        }
        plans.push(PlanExportItem {
            name,
            project_name,
            config,
            request,
            env,
            resolved_url,
        });
    }
    let file = PlanExportFile {
        format: PLAN_EXPORT_FORMAT.to_string(),
        exported_at: now_ms(),
        includes_sensitive: include_sensitive,
        plans,
    };
    serde_json::to_string_pretty(&file).map_err(err)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanImportMatch {
    pub plan_name: String,
    /// 按 method + url 命中的现有接口
    pub request_id: Option<i64>,
    pub request_method: String,
    pub request_url: String,
    /// 按名称命中的现有环境
    pub env_id: Option<i64>,
    pub env_name: Option<String>,
    /// 导入环境里缺少值的变量 key
    pub missing_vars: Vec<String>,
    pub has_sensitive: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanImportPreview {
    pub format_ok: bool,
    pub includes_sensitive: bool,
    pub plans: Vec<PlanImportMatch>,
}

fn parse_export_file(payload: &str) -> Result<PlanExportFile, String> {
    let file: PlanExportFile =
        serde_json::from_str(payload).map_err(|e| format!("计划文件无法解析：{e}"))?;
    if file.format != PLAN_EXPORT_FORMAT {
        return Err(format!(
            "不支持的计划文件格式：{}（期望 {PLAN_EXPORT_FORMAT}）",
            file.format
        ));
    }
    Ok(file)
}

fn find_request_by_target(
    conn: &Connection,
    project_id: i64,
    method: &str,
    url: &str,
) -> Result<Option<i64>, String> {
    conn.query_row(
        "SELECT r.id FROM requests r
         JOIN collections c ON c.id = r.collection_id
         WHERE c.project_id = ?1 AND UPPER(r.method) = UPPER(?2) AND r.url = ?3
         ORDER BY r.id ASC LIMIT 1",
        params![project_id, method, url],
        |row| row.get(0),
    )
    .optional()
    .map_err(err)
}

fn find_env_by_name(
    conn: &Connection,
    project_id: i64,
    name: &str,
) -> Result<Option<i64>, String> {
    conn.query_row(
        "SELECT id FROM environments WHERE project_id = ?1 AND name = ?2 ORDER BY id ASC LIMIT 1",
        params![project_id, name],
        |row| row.get(0),
    )
    .optional()
    .map_err(err)
}

/// 导入前映射预览：环境按名称、接口按 method + URL 匹配（2.4.3）
#[tauri::command]
pub fn plan_import_preview(
    state: State<'_, AppState>,
    payload: String,
    project_id: i64,
) -> Result<PlanImportPreview, String> {
    let file = parse_export_file(&payload)?;
    let conn = state.pool.get().map_err(err)?;
    let mut plans = Vec::with_capacity(file.plans.len());
    for item in &file.plans {
        let target = target_snapshot(&item.config);
        let (method, url) = match &item.request {
            Some(request) => (request.method.clone(), request.url.clone()),
            None => (
                target.method.clone().unwrap_or_else(|| "GET".to_string()),
                target.url_template.clone().unwrap_or_default(),
            ),
        };
        let request_id = find_request_by_target(&conn, project_id, &method, &url)?;
        let env_id = match &item.env {
            Some(env) => find_env_by_name(&conn, project_id, &env.name)?,
            None => None,
        };
        let mut missing_vars = Vec::new();
        let mut warnings = Vec::new();
        if let Some(env) = &item.env {
            match env_id {
                Some(env_id) => {
                    let existing =
                        crate::commands::http::load_vars(&conn, env_id).unwrap_or_default();
                    for key in &env.vars {
                        if !existing.contains_key(key) {
                            missing_vars.push(key.clone());
                        }
                    }
                }
                None => {
                    missing_vars = env.vars.clone();
                    warnings.push(format!("环境「{}」不存在，导入时将新建", env.name));
                }
            }
        }
        if request_id.is_none() {
            warnings.push("未找到同 method + URL 的接口，导入时将新建".to_string());
        }
        plans.push(PlanImportMatch {
            plan_name: item.name.clone(),
            request_id,
            request_method: method,
            request_url: url,
            env_id,
            env_name: item.env.as_ref().map(|env| env.name.clone()),
            missing_vars,
            has_sensitive: file.includes_sensitive,
            warnings,
        });
    }
    Ok(PlanImportPreview {
        format_ok: true,
        includes_sensitive: file.includes_sensitive,
        plans,
    })
}

/// 导入执行：按预览口径落库（新环境变量值只在 includeSensitive 时写入）
#[tauri::command]
pub fn plan_import_apply(
    state: State<'_, AppState>,
    payload: String,
    project_id: i64,
) -> Result<Vec<i64>, String> {
    let file = parse_export_file(&payload)?;
    let now = now_ms();
    let mut created = Vec::new();
    {
        let conn = state.pool.get().map_err(err)?;
        let project_exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM projects WHERE id = ?1)",
                params![project_id],
                |row| row.get(0),
            )
            .map_err(err)?;
        if !project_exists {
            return Err("目标项目不存在".to_string());
        }
        for item in &file.plans {
            let target = target_snapshot(&item.config);
            let (method, url) = match &item.request {
                Some(request) => (request.method.clone(), request.url.clone()),
                None => (
                    target.method.clone().unwrap_or_else(|| "GET".to_string()),
                    target.url_template.clone().unwrap_or_default(),
                ),
            };
            if url.trim().is_empty() {
                continue;
            }

            // 接口：复用或新建（新建时放进「导入的压测计划」集合）
            let request_id = match find_request_by_target(&conn, project_id, &method, &url)? {
                Some(id) => id,
                None => {
                    let collection_id: Option<i64> = conn
                        .query_row(
                            "SELECT id FROM collections WHERE project_id = ?1 AND name = ?2
                             ORDER BY id ASC LIMIT 1",
                            params![project_id, "导入的压测计划"],
                            |row| row.get(0),
                        )
                        .optional()
                        .map_err(err)?;
                    let collection_id = match collection_id {
                        Some(id) => id,
                        None => {
                            conn.execute(
                                "INSERT INTO collections (project_id, name, created_at, updated_at)
                                 VALUES (?1, ?2, ?3, ?3)",
                                params![project_id, "导入的压测计划", now],
                            )
                            .map_err(err)?;
                            conn.last_insert_rowid()
                        }
                    };
                    let name = item
                        .request
                        .as_ref()
                        .and_then(|request| request.name.clone())
                        .unwrap_or_else(|| format!("{method} {url}"));
                    conn.execute(
                        "INSERT INTO requests (collection_id, name, method, url, created_at, updated_at)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
                        params![collection_id, name, method, url, now],
                    )
                    .map_err(err)?;
                    conn.last_insert_rowid()
                }
            };

            // 环境：按名称复用或新建
            let env_id = match &item.env {
                Some(env) => match find_env_by_name(&conn, project_id, &env.name)? {
                    Some(id) => id,
                    None => {
                        conn.execute(
                            "INSERT INTO environments (project_id, name, is_active, created_at)
                             VALUES (?1, ?2, 0, ?3)",
                            params![project_id, env.name, now],
                        )
                        .map_err(err)?;
                        let env_id = conn.last_insert_rowid();
                        if env.values.is_empty() {
                            // 默认导出只有 key，导入后由用户补齐值
                            for key in &env.vars {
                                conn.execute(
                                    "INSERT INTO env_variables (env_id, key, value, is_secret, sort_order)
                                     VALUES (?1, ?2, '', 0, 0)",
                                    params![env_id, key],
                                )
                                .map_err(err)?;
                            }
                        } else {
                            let mut keys: Vec<&String> = env.values.keys().collect();
                            keys.sort();
                            for key in keys {
                                conn.execute(
                                    "INSERT INTO env_variables (env_id, key, value, is_secret, sort_order)
                                     VALUES (?1, ?2, ?3, 0, 0)",
                                    params![
                                        env_id,
                                        key,
                                        env.values.get(key.as_str()).cloned().unwrap_or_default()
                                    ],
                                )
                                .map_err(err)?;
                            }
                        }
                        env_id
                    }
                },
                None => 0,
            };

            let mut config = item.config.clone();
            if !config.is_object() {
                config = json!({});
            }
            if let Some(map) = config.as_object_mut() {
                if env_id > 0 {
                    map.insert("envId".to_string(), json!(env_id));
                }
                map.remove("planId");
                map.remove("projectId");
            }
            enrich_plan_config(&conn, &mut config, project_id, Some(request_id))?;

            conn.execute(
                "INSERT INTO test_plans (project_id, request_id, name, config_json, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
                params![
                    project_id,
                    request_id,
                    item.name,
                    serde_json::to_string(&config).map_err(err)?,
                    now
                ],
            )
            .map_err(err)?;
            created.push(conn.last_insert_rowid());
        }
    }
    Ok(created)
}

/// 计划模板：低并发基线 / 细化阶梯 / 长稳（2.5）
#[tauri::command]
pub fn plan_templates() -> Vec<Value> {
    fn stage(concurrent: u32, duration_sec: u32) -> Value {
        json!({ "concurrent": concurrent, "durationSec": duration_sec })
    }
    vec![
        json!({
            "id": "baseline",
            "name": "低并发基线",
            "description": "50 / 100 并发各 5 分钟，用于建立单接口基线",
            "mode": "stages",
            "stages": [stage(50, 300), stage(100, 300)],
            "captureMode": "locate",
        }),
        json!({
            "id": "refined",
            "name": "细化阶梯",
            "description": "200-280-340-400-450-500 并发，每档 3 分钟",
            "mode": "stages",
            "stages": [
                stage(200, 180),
                stage(280, 180),
                stage(340, 180),
                stage(400, 180),
                stage(450, 180),
                stage(500, 180),
            ],
            "captureMode": "capacity",
        }),
        json!({
            "id": "long-run-4h",
            "name": "长稳 4 小时",
            "description": "350 并发 × 4h，需勾选长稳风险确认",
            "mode": "fixed",
            "concurrent": 350,
            "durationSec": 14_400,
            "captureMode": "capacity",
        }),
        json!({
            "id": "long-run-8h",
            "name": "长稳 8 小时",
            "description": "350 并发 × 8h，需勾选长稳风险确认",
            "mode": "fixed",
            "concurrent": 350,
            "durationSec": 28_800,
            "captureMode": "capacity",
        }),
        json!({
            "id": "long-run-24h",
            "name": "长稳 24 小时",
            "description": "350 并发 × 24h，需独立施压机",
            "mode": "fixed",
            "concurrent": 350,
            "durationSec": 86_400,
            "captureMode": "capacity",
        }),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_export_file_rejects_foreign_format() {
        let payload = r#"{"format":"other/v1","exportedAt":0,"includesSensitive":false,"plans":[]}"#;
        let error = parse_export_file(payload).unwrap_err();
        assert!(error.contains("不支持的计划文件格式"));

        let ok = format!(
            r#"{{"format":"{PLAN_EXPORT_FORMAT}","exportedAt":0,"includesSensitive":false,"plans":[]}}"#
        );
        assert!(parse_export_file(&ok).is_ok());
    }

    #[test]
    fn strip_sensitive_removes_auth_and_secret_refs() {
        let mut config = json!({
            "settings": {
                "auth": {"kind": "bearer", "token": "abc"},
                "baselineReportId": 12,
            },
            "vars": {"apiKey": "x", "base": "http://h"},
            "trace": "secret:prod-token",
            "baseline": 12,
        });
        strip_sensitive(&mut config);
        let text = serde_json::to_string(&config).unwrap();
        assert!(!text.contains("bearer"));
        assert!(!text.contains("apiKey"));
        assert!(!text.contains("secret:prod-token"));
        // 业务字段保留
        assert!(text.contains("baselineReportId"));
        assert!(text.contains("http://h"));
    }

    #[test]
    fn templates_cover_baseline_ladder_and_long_run() {
        let templates = plan_templates();
        let ids: Vec<String> = templates
            .iter()
            .filter_map(|item| item.get("id").and_then(|id| id.as_str()).map(str::to_string))
            .collect();
        for expected in [
            "baseline",
            "refined",
            "long-run-4h",
            "long-run-8h",
            "long-run-24h",
        ] {
            assert!(ids.contains(&expected.to_string()), "缺少模板 {expected}");
        }
        let baseline = templates
            .iter()
            .find(|item| item["id"] == "baseline")
            .unwrap();
        let stages = baseline["stages"].as_array().unwrap();
        assert_eq!(stages[0]["concurrent"], 50);
        assert_eq!(stages[1]["concurrent"], 100);
    }
}
