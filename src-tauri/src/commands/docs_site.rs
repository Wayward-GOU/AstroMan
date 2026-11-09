//! 文档站命令薄壳：核心逻辑在 `services::docs_site`。

use tauri::State;

use crate::services::docs_site::{DocsSiteStartPayload, DocsSiteStatus};
use crate::state::AppState;
use crate::util::err;

#[tauri::command(async)]
pub fn docs_site_start(
    state: State<'_, AppState>,
    payload: DocsSiteStartPayload,
) -> Result<DocsSiteStatus, String> {
    state.policy().ensure_lan(payload.host.as_deref())?;
    let conn = state.pool.get().map_err(err)?;
    state.docs_site.start(&conn, &payload)
}

#[tauri::command(async)]
pub fn docs_site_stop(state: State<'_, AppState>) -> DocsSiteStatus {
    state.docs_site.stop()
}

#[tauri::command]
pub fn docs_site_status(state: State<'_, AppState>) -> DocsSiteStatus {
    state.docs_site.status()
}

#[tauri::command(async)]
pub fn docs_site_refresh(state: State<'_, AppState>) -> Result<DocsSiteStatus, String> {
    let conn = state.pool.get().map_err(err)?;
    state.docs_site.refresh(&conn)
}
