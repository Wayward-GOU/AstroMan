//! 契约命令薄壳：核心逻辑在 `services::contract`。

use crate::services::contract;
pub(crate) use crate::services::contract::{ContractCheckResult, ContractSnapshot};
use crate::state::AppState;
use crate::util::err;
use tauri::State;

#[tauri::command]
pub fn contract_snapshot_create(
    state: State<'_, AppState>,
    project_id: i64,
    label: Option<String>,
) -> Result<ContractSnapshot, String> {
    let conn = state.pool.get().map_err(err)?;
    contract::create_snapshot(&conn, project_id, label.as_deref().unwrap_or(""))
}

#[tauri::command]
pub fn contract_snapshot_list(
    state: State<'_, AppState>,
    project_id: i64,
) -> Result<Vec<ContractSnapshot>, String> {
    let conn = state.pool.get().map_err(err)?;
    contract::list_snapshots(&conn, project_id)
}

#[tauri::command]
pub fn contract_check(
    state: State<'_, AppState>,
    project_id: i64,
    baseline_id: i64,
) -> Result<ContractCheckResult, String> {
    let conn = state.pool.get().map_err(err)?;
    contract::check_contract(&conn, project_id, baseline_id)
}

#[tauri::command]
pub fn contract_report(
    state: State<'_, AppState>,
    project_id: i64,
    baseline_id: i64,
    format: String,
    path: Option<String>,
) -> Result<String, String> {
    let conn = state.pool.get().map_err(err)?;
    let text = contract::report_text(&conn, project_id, baseline_id, &format)?;
    if let Some(path) = path.filter(|item| !item.trim().is_empty()) {
        std::fs::write(&path, &text).map_err(|e| format!("写入文件失败：{e}"))?;
    }
    Ok(text)
}
