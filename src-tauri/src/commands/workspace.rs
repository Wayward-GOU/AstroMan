//! Git 友好工作区导入 / 导出命令（G-11）：核心逻辑在 `services::workspace`。

use tauri::State;

use crate::services::workspace::{
    self, WorkspaceExportPayload, WorkspaceExportResult, WorkspaceImportPayload,
    WorkspaceImportResult,
};
use crate::state::AppState;
use crate::util::err;

#[tauri::command]
pub fn workspace_export_dir(
    state: State<'_, AppState>,
    payload: WorkspaceExportPayload,
) -> Result<WorkspaceExportResult, String> {
    let conn = state.pool.get().map_err(err)?;
    workspace::export(&conn, &payload)
}

#[tauri::command]
pub fn workspace_import_dir(
    state: State<'_, AppState>,
    payload: WorkspaceImportPayload,
) -> Result<WorkspaceImportResult, String> {
    let mut conn = state.pool.get().map_err(err)?;
    workspace::import(&mut conn, &payload)
}
