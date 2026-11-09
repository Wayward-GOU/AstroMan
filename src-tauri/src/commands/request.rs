//! 请求命令薄壳：核心读写已下沉到 `services::request`。
//!
//! 保留原命令签名与 `find_request` / `map_detail` / `save_detail` 的导出，
//! 让文档构建、测试运行等旧调用方无需改动。

pub(crate) use crate::services::request::{find_request, map_detail, save_detail};

use crate::models::{HttpRequestDetail, RequestSearchHit, RequestSummary};
use crate::services::request;
use crate::state::AppState;
use crate::util::err;
use tauri::State;

#[tauri::command]
pub fn request_list(
    state: State<'_, AppState>,
    collection_id: i64,
) -> Result<Vec<RequestSummary>, String> {
    let conn = state.pool.get().map_err(err)?;
    request::list_requests(&conn, collection_id)
}

#[tauri::command]
pub fn request_search(
    state: State<'_, AppState>,
    project_id: i64,
    keyword: String,
) -> Result<Vec<RequestSearchHit>, String> {
    let conn = state.pool.get().map_err(err)?;
    request::search_requests(&conn, project_id, &keyword)
}

#[tauri::command]
pub fn request_options(
    state: State<'_, AppState>,
    project_id: i64,
) -> Result<Vec<RequestSearchHit>, String> {
    let conn = state.pool.get().map_err(err)?;
    request::request_options(&conn, project_id)
}

#[tauri::command]
pub fn request_get(state: State<'_, AppState>, id: i64) -> Result<HttpRequestDetail, String> {
    let conn = state.pool.get().map_err(err)?;
    find_request(&conn, id)
}

#[tauri::command]
pub fn request_save(
    state: State<'_, AppState>,
    request: HttpRequestDetail,
) -> Result<HttpRequestDetail, String> {
    let conn = state.pool.get().map_err(err)?;
    save_detail(&conn, &request, "save", false)
}

/// 自动保存专用：只写库，不把整份详情（含大 Body）再传回前端
#[tauri::command]
pub fn request_save_quick(
    state: State<'_, AppState>,
    request: HttpRequestDetail,
) -> Result<(), String> {
    if request.id.is_none() {
        return Err("请求尚未保存，无法自动保存".to_string());
    }
    let conn = state.pool.get().map_err(err)?;
    save_detail(&conn, &request, "save", false)?;
    Ok(())
}

#[tauri::command]
pub fn request_delete(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    let conn = state.pool.get().map_err(err)?;
    request::delete_request(&conn, id)
}

/// 按给定顺序重排某集合下的请求（拖拽排序）。
#[tauri::command]
pub fn request_reorder(
    state: State<'_, AppState>,
    collection_id: i64,
    ids: Vec<i64>,
) -> Result<(), String> {
    let mut conn = state.pool.get().map_err(err)?;
    request::reorder_requests(&mut conn, collection_id, &ids)
}
