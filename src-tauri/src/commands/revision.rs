//! 请求版本命令薄壳：核心逻辑在 `services::revision`。

use crate::models::HttpRequestDetail;
use crate::services::revision;
pub use crate::services::revision::{
    RevisionBrief, RevisionDetail, RevisionDiff, TraceRollbackResult,
};
use crate::state::AppState;
use crate::util::err;
use tauri::State;

#[tauri::command]
pub fn request_revision_list(
    state: State<'_, AppState>,
    request_id: i64,
    limit: Option<i64>,
) -> Result<Vec<RevisionBrief>, String> {
    let conn = state.pool.get().map_err(err)?;
    revision::list_revisions(&conn, request_id, limit)
}

#[tauri::command]
pub fn request_revision_get(
    state: State<'_, AppState>,
    request_id: i64,
    revision: i64,
) -> Result<RevisionDetail, String> {
    let conn = state.pool.get().map_err(err)?;
    revision::get_revision(&conn, request_id, revision)
}

#[tauri::command]
pub fn request_revision_diff(
    state: State<'_, AppState>,
    request_id: i64,
    from: i64,
    to: i64,
) -> Result<RevisionDiff, String> {
    let conn = state.pool.get().map_err(err)?;
    revision::diff_revisions(&conn, request_id, from, to)
}

#[tauri::command]
pub fn request_revision_restore(
    state: State<'_, AppState>,
    request_id: i64,
    revision: i64,
) -> Result<HttpRequestDetail, String> {
    let conn = state.pool.get().map_err(err)?;
    revision::restore_revision(&conn, request_id, revision)
}

/// AI 写操作执行前调用：把目标请求当前状态记为 source=ai 的快照
#[tauri::command]
pub fn request_revision_snapshot(
    state: State<'_, AppState>,
    request_id: i64,
    trace_id: String,
) -> Result<(), String> {
    let conn = state.pool.get().map_err(err)?;
    revision::record_before_ai(&conn, request_id, &trace_id)
}

/// 某轮提问关联的 AI 前置快照（只读回放）
#[tauri::command]
pub fn request_revision_trace(
    state: State<'_, AppState>,
    trace_id: String,
    limit: Option<i64>,
) -> Result<Vec<RevisionBrief>, String> {
    let conn = state.pool.get().map_err(err)?;
    revision::list_by_trace(&conn, &trace_id, limit)
}

/// 按 trace_id 一键回滚该轮全部 AI 写操作
#[tauri::command]
pub fn request_revision_rollback_trace(
    state: State<'_, AppState>,
    trace_id: String,
) -> Result<TraceRollbackResult, String> {
    let conn = state.pool.get().map_err(err)?;
    revision::rollback_trace(&conn, &trace_id)
}
