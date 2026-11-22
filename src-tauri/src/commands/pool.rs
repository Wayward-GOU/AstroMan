//! 参数 / Token 池命令（v1.1.2 W6）
//!
//! 所有返回值默认脱敏：管理页可显式 `reveal = true` 才返回原文。

use std::collections::{HashMap, HashSet};

use serde::Serialize;
use serde_json::json;
use tauri::State;

use crate::models::PoolBinding;
use crate::services::{pool, pool_excel};
use crate::state::AppState;
use crate::util::mask_secret_value;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatePoolResult {
    pub id: i64,
}

/// 整行数据脱敏：`show = false` 时逐值打码
fn mask_data(data: &HashMap<String, String>, show: bool) -> HashMap<String, String> {
    data.iter()
        .map(|(key, value)| {
            (
                key.clone(),
                if show {
                    value.clone()
                } else {
                    mask_secret_value(value)
                },
            )
        })
        .collect()
}

#[tauri::command]
pub fn pool_list(state: State<'_, AppState>) -> Result<Vec<pool::PoolRow>, String> {
    let conn = state
        .pool
        .get()
        .map_err(|error| format!("数据库不可用：{error}"))?;
    pool::list_pools(&conn)
}

/// 池详情：池摘要 + 第一页脱敏条目（敏感池永不下发明文）
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PoolDetail {
    pub pool: pool::PoolRow,
    pub items: pool::PoolItemsPage,
}

#[tauri::command]
pub fn pool_get(
    state: State<'_, AppState>,
    pool_id: i64,
    page_size: Option<i64>,
) -> Result<PoolDetail, String> {
    let conn = state
        .pool
        .get()
        .map_err(|error| format!("数据库不可用：{error}"))?;
    let pool_row = pool::list_pools(&conn)?
        .into_iter()
        .find(|item| item.id == pool_id)
        .ok_or_else(|| format!("池不存在：{pool_id}"))?;
    let sensitive = pool::pool_sensitive(&conn, pool_id)?;
    let items = pool::pool_items_page(
        &conn,
        pool_id,
        1,
        page_size.unwrap_or(20).clamp(1, 200),
        "",
        "",
        sensitive,
    )?;
    Ok(PoolDetail {
        pool: pool_row,
        items,
    })
}

#[tauri::command]
pub fn pool_create(
    state: State<'_, AppState>,
    name: String,
    kind: Option<String>,
    sensitive: Option<bool>,
    description: Option<String>,
    columns: Option<Vec<String>>,
) -> Result<CreatePoolResult, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("池名不能为空".to_string());
    }
    let kind = kind
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("parameter")
        .to_string();
    if !matches!(kind.as_str(), "parameter" | "token") {
        return Err("池类型只能是 parameter 或 token".to_string());
    }
    let columns = match columns {
        Some(list) => pool::normalize_columns(&list)?,
        None => pool::default_columns(&kind),
    };
    let raw_columns =
        serde_json::to_string(&columns).map_err(|error| format!("序列化池列失败：{error}"))?;
    let conn = state
        .pool
        .get()
        .map_err(|error| format!("数据库不可用：{error}"))?;
    if pool::pool_id_by_name(&conn, name)?.is_some() {
        return Err(format!("池「{name}」已存在"));
    }
    let now = crate::util::now_ms();
    conn.execute(
        "INSERT INTO data_pools (name, kind, sensitive, description, columns_json, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
        rusqlite::params![
            name,
            kind,
            sensitive.unwrap_or(kind == "token") as i64,
            description.unwrap_or_default(),
            raw_columns,
            now
        ],
    )
    .map_err(|error| format!("创建池失败：{error}"))?;
    Ok(CreatePoolResult {
        id: conn.last_insert_rowid(),
    })
}

#[tauri::command]
pub fn pool_delete(state: State<'_, AppState>, pool_id: i64) -> Result<(), String> {
    let conn = state
        .pool
        .get()
        .map_err(|error| format!("数据库不可用：{error}"))?;
    conn.execute(
        "DELETE FROM data_pools WHERE id = ?1",
        rusqlite::params![pool_id],
    )
    .map_err(|error| format!("删除池失败：{error}"))?;
    Ok(())
}

/// 池条目；`reveal = false`（默认）时返回脱敏值
#[tauri::command]
pub fn pool_items(
    state: State<'_, AppState>,
    pool_id: i64,
    reveal: Option<bool>,
) -> Result<Vec<serde_json::Value>, String> {
    let conn = state
        .pool
        .get()
        .map_err(|error| format!("数据库不可用：{error}"))?;
    let sensitive = conn
        .query_row(
            "SELECT sensitive FROM data_pools WHERE id = ?1",
            rusqlite::params![pool_id],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(1)
        != 0;
    let show = reveal.unwrap_or(false) || !sensitive;
    let items = pool::pool_items(&conn, pool_id)?;
    Ok(items
        .into_iter()
        .map(|item| {
            json!({
                "id": item.id,
                "value": if show { item.value.clone() } else { mask_secret_value(&item.value) },
                "account": item.account,
                "weight": item.weight,
                "data": mask_data(&item.data, show),
                "enabled": item.enabled,
                "createdAt": item.created_at,
            })
        })
        .collect())
}

#[tauri::command]
pub fn pool_import(
    state: State<'_, AppState>,
    pool_id: i64,
    text: String,
) -> Result<usize, String> {
    let conn = state
        .pool
        .get()
        .map_err(|error| format!("数据库不可用：{error}"))?;
    pool::import_items(&conn, pool_id, &text)
}

/// 保存池列定义并迁移存量数据（v20）
#[tauri::command]
pub fn pool_columns_set(
    state: State<'_, AppState>,
    pool_id: i64,
    columns: Vec<pool::PoolColumnChange>,
) -> Result<Vec<String>, String> {
    let conn = state
        .pool
        .get()
        .map_err(|error| format!("数据库不可用：{error}"))?;
    pool::save_columns(&conn, pool_id, &columns)
}

/// 导出 Excel 导入模板（v20）
#[tauri::command]
pub fn pool_excel_template(
    state: State<'_, AppState>,
    pool_id: i64,
    path: String,
) -> Result<(), String> {
    let conn = state
        .pool
        .get()
        .map_err(|error| format!("数据库不可用：{error}"))?;
    let (name, sensitive): (String, i64) = conn
        .query_row(
            "SELECT name, sensitive FROM data_pools WHERE id = ?1",
            rusqlite::params![pool_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|_| format!("池不存在：{pool_id}"))?;
    let columns = pool::pool_columns_by_id(&conn, pool_id)?;
    pool_excel::write_template(&path, &name, &columns, sensitive != 0)
}

/// 从 Excel 导入池条目（v20）
#[tauri::command]
pub fn pool_excel_import(
    state: State<'_, AppState>,
    pool_id: i64,
    path: String,
) -> Result<pool_excel::ExcelImportResult, String> {
    let conn = state
        .pool
        .get()
        .map_err(|error| format!("数据库不可用：{error}"))?;
    pool_excel::import_excel(&conn, pool_id, &path)
}

#[tauri::command]
pub fn pool_delete_item(state: State<'_, AppState>, item_id: i64) -> Result<(), String> {
    let conn = state
        .pool
        .get()
        .map_err(|error| format!("数据库不可用：{error}"))?;
    conn.execute(
        "DELETE FROM data_pool_items WHERE id = ?1",
        rusqlite::params![item_id],
    )
    .map_err(|error| format!("删除条目失败：{error}"))?;
    Ok(())
}

/// 管理页分页：默认脱敏，敏感池列表接口永不下发明文
#[tauri::command]
pub fn pool_items_page(
    state: State<'_, AppState>,
    pool_id: i64,
    page: Option<i64>,
    page_size: Option<i64>,
    search: Option<String>,
    status: Option<String>,
) -> Result<pool::PoolItemsPage, String> {
    let conn = state
        .pool
        .get()
        .map_err(|error| format!("数据库不可用：{error}"))?;
    let sensitive = pool::pool_sensitive(&conn, pool_id)?;
    pool::pool_items_page(
        &conn,
        pool_id,
        page.unwrap_or(1),
        page_size.unwrap_or(50),
        search.as_deref().unwrap_or(""),
        status.as_deref().unwrap_or(""),
        sensitive,
    )
}

/// 单条明文：管理页二次确认后才调用，前端 30 秒后自动恢复掩码
#[tauri::command]
pub fn pool_item_reveal(
    state: State<'_, AppState>,
    item_id: i64,
) -> Result<pool::RevealedItem, String> {
    let conn = state
        .pool
        .get()
        .map_err(|error| format!("数据库不可用：{error}"))?;
    pool::reveal_item(&conn, item_id)
}

/// 启用 / 停用单条
#[tauri::command]
pub fn pool_item_toggle(
    state: State<'_, AppState>,
    item_id: i64,
    enabled: bool,
) -> Result<(), String> {
    let conn = state
        .pool
        .get()
        .map_err(|error| format!("数据库不可用：{error}"))?;
    pool::set_item_enabled(&conn, item_id, enabled)
}

/// 清空池条目（保留池本身）
#[tauri::command]
pub fn pool_clear_items(state: State<'_, AppState>, pool_id: i64) -> Result<usize, String> {
    let conn = state
        .pool
        .get()
        .map_err(|error| format!("数据库不可用：{error}"))?;
    pool::clear_items(&conn, pool_id)
}

/// 刷新健康度：清理过期冷却并重新统计
#[tauri::command]
pub fn pool_health_refresh(
    state: State<'_, AppState>,
    pool_id: i64,
) -> Result<pool::PoolHealth, String> {
    let conn = state
        .pool
        .get()
        .map_err(|error| format!("数据库不可用：{error}"))?;
    pool::refresh_health(&conn, pool_id)
}

/// 执行前预览：绑定汇总、样例数据、VU 分配、身份预览与校验
#[tauri::command]
pub fn pool_preview(
    state: State<'_, AppState>,
    bindings: Vec<PoolBinding>,
    vu_count: Option<u32>,
    reveal: Option<bool>,
) -> Result<serde_json::Value, String> {
    let conn = state
        .pool
        .get()
        .map_err(|error| format!("数据库不可用：{error}"))?;
    let vu_count = vu_count.unwrap_or(0).clamp(0, 100_000);
    let show = reveal.unwrap_or(false);
    let mut summaries = Vec::new();
    let mut problems = Vec::new();
    let mut identity_preview = Vec::new();
    let mut columns_cache: HashMap<String, Vec<String>> = HashMap::new();
    let mut injected: HashMap<String, HashSet<String>> = HashMap::new();
    // 变量名 → 注入它的池：跨池同名会在运行期静默覆盖，预览阶段先提示
    let mut owners: HashMap<String, String> = HashMap::new();
    for binding in &bindings {
        let pool_name = binding.pool.trim().to_string();
        let column = binding.column.trim().to_string();
        if pool_name.is_empty() || (binding.target.trim().is_empty() && column != "*") {
            problems.push("绑定缺少池名或目标变量".to_string());
            continue;
        }
        if !columns_cache.contains_key(&pool_name) {
            let columns = pool::pool_columns(&conn, &pool_name).unwrap_or_default();
            columns_cache.insert(pool_name.clone(), columns);
        }
        let columns = columns_cache.get(&pool_name).cloned().unwrap_or_default();
        if !column.is_empty() && column != "*" && !columns.iter().any(|item| item == &column) {
            problems.push(format!("池「{pool_name}」没有列「{column}」"));
        }
        let injected_names: Vec<String> = if column == "*" {
            columns.clone()
        } else {
            vec![binding.target.trim().to_string()]
        };
        let seen = injected.entry(pool_name.clone()).or_default();
        for name in injected_names {
            if name.is_empty() {
                continue;
            }
            if !seen.insert(name.clone()) {
                problems.push(format!("池「{pool_name}」的多个绑定重复注入变量「{name}」"));
                continue;
            }
            if let Some(previous) = owners.insert(name.clone(), pool_name.clone()) {
                if previous != pool_name {
                    problems.push(format!(
                        "池「{previous}」和池「{pool_name}」都注入变量「{name}」，后绑定的会覆盖前者"
                    ));
                }
            }
        }
        let items = pool::load_items(&conn, &pool_name)?;
        let sample = items
            .iter()
            .take(10)
            .map(|item| {
                json!({
                    "id": item.id,
                    "value": if show { item.value.clone() } else { mask_secret_value(&item.value) },
                    "account": item.account,
                    "weight": item.weight,
                    "data": mask_data(&item.data, show),
                })
            })
            .collect::<Vec<_>>();
        let mode = if binding.mode.trim().is_empty() {
            "per_vu"
        } else {
            binding.mode.trim()
        };
        let assignments = if vu_count > 0 && !items.is_empty() && mode == "per_vu" {
            (0..vu_count.min(20))
                .map(|vu| {
                    let item = &items[vu as usize % items.len()];
                    json!({
                        "vu": vu,
                        "itemId": item.id,
                        "value": if show { item.value.clone() } else { mask_secret_value(&item.value) },
                        "account": item.account,
                    })
                })
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        if items.is_empty() {
            problems.push(format!("池「{}」没有启用条目", binding.pool));
        }
        if vu_count > 0 && mode == "per_vu" && !items.is_empty() && (items.len() as u32) < vu_count
        {
            problems.push(format!(
                "池「{}」只有 {} 条，{} 个 VU 会循环复用（同一 token 可能被多个 VU 共用）",
                binding.pool,
                items.len(),
                vu_count
            ));
        }
        if mode == "per_vu" && !items.is_empty() {
            identity_preview.push(json!({
                "target": binding.target,
                "pool": binding.pool,
                "uniqueItems": items.len(),
                "vusCovered": vu_count,
            }));
        }
        let mut summary = json!({
            "pool": binding.pool,
            "target": binding.target,
            "column": column,
            "mode": mode,
            "refreshOn401": binding.refresh_on_401,
            "itemCount": items.len(),
            "vusCovered": vu_count,
            "sample": sample,
            "assignments": assignments,
        });
        if binding.column.trim() == "*" {
            summary["targets"] = json!(columns);
        }
        summaries.push(summary);
    }
    Ok(json!({
        "bindings": summaries,
        "identityPreview": identity_preview,
        "problems": problems,
        "vuCount": vu_count,
        "masked": !show,
    }))
}

/// 读取池的 Token 刷新配置（v19）
#[tauri::command]
pub fn pool_refresh_get(
    state: State<'_, AppState>,
    name: String,
) -> Result<pool::PoolRefreshConfig, String> {
    let conn = state
        .pool
        .get()
        .map_err(|error| format!("数据库不可用：{error}"))?;
    pool::load_refresh_config(&conn, &name)
}

/// 保存池的 Token 刷新配置（v19）
#[tauri::command]
pub fn pool_refresh_set(
    state: State<'_, AppState>,
    name: String,
    config: pool::PoolRefreshConfig,
) -> Result<(), String> {
    let conn = state
        .pool
        .get()
        .map_err(|error| format!("数据库不可用：{error}"))?;
    pool::save_refresh_config(&conn, &name, &config)
}
