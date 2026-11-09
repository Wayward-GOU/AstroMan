//! G-04 本地 Mock Server 命令薄壳：核心逻辑在 `services::mock`。

use tauri::State;

use crate::services::mock::server::{MockLog, MockStartPayload, MockStatus};
use crate::services::mock::store::{self, MockRoute};
use crate::state::AppState;
use crate::util::err;

#[tauri::command(async)]
pub fn mock_routes_list(
    state: State<'_, AppState>,
    project_id: i64,
) -> Result<Vec<MockRoute>, String> {
    let conn = state.pool.get().map_err(err)?;
    store::list_routes(&conn, project_id)
}

#[tauri::command(async)]
pub fn mock_route_save(state: State<'_, AppState>, route: MockRoute) -> Result<MockRoute, String> {
    let conn = state.pool.get().map_err(err)?;
    store::save_route(&conn, &route)
}

#[tauri::command(async)]
pub fn mock_route_delete(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    let conn = state.pool.get().map_err(err)?;
    store::delete_route(&conn, id)
}

/// 按项目已有请求批量生成路由（只补缺失的请求，不覆盖手工编辑）
#[tauri::command(async)]
pub fn mock_routes_generate(
    state: State<'_, AppState>,
    project_id: i64,
) -> Result<Vec<MockRoute>, String> {
    let conn = state.pool.get().map_err(err)?;
    store::generate_routes(&conn, project_id)?;
    store::list_routes(&conn, project_id)
}

#[tauri::command(async)]
pub fn mock_routes_clear(state: State<'_, AppState>, project_id: i64) -> Result<usize, String> {
    let conn = state.pool.get().map_err(err)?;
    store::delete_project_routes(&conn, project_id)
}

#[tauri::command(async)]
pub fn mock_start(
    state: State<'_, AppState>,
    payload: MockStartPayload,
) -> Result<MockStatus, String> {
    state.policy().ensure_lan(payload.host.as_deref())?;
    state.mock.start(&state.db_path, &payload)
}

#[tauri::command(async)]
pub fn mock_stop(state: State<'_, AppState>, project_id: i64) -> MockStatus {
    state.mock.stop(project_id)
}

#[tauri::command]
pub fn mock_status(state: State<'_, AppState>, project_id: i64) -> MockStatus {
    state.mock.status(project_id)
}

#[tauri::command]
pub fn mock_statuses(state: State<'_, AppState>) -> Vec<MockStatus> {
    state.mock.statuses()
}

#[tauri::command]
pub fn mock_logs(state: State<'_, AppState>, project_id: i64) -> Vec<MockLog> {
    state.mock.logs(project_id)
}

#[tauri::command]
pub fn mock_logs_clear(state: State<'_, AppState>, project_id: i64) {
    state.mock.clear_logs(project_id);
}
