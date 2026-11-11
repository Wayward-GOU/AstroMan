//! 文档命令薄壳：核心逻辑在 `services::docs`。

use tauri::State;

use crate::docs::model::*;
use crate::services::docs;
use crate::state::AppState;
use crate::util::err;

#[tauri::command(async)]
pub fn docs_prepare(
    state: State<'_, AppState>,
    payload: DocsPreparePayload,
) -> Result<DocsPrepareResult, String> {
    let conn = state.pool.get().map_err(err)?;
    docs::prepare(&conn, &state.docs_cache, &payload)
}

#[tauri::command(async)]
pub fn docs_render(
    state: State<'_, AppState>,
    payload: DocsRenderPayload,
) -> Result<DocsRenderResult, String> {
    docs::render(&state.docs_cache, &payload)
}

#[tauri::command(async)]
pub fn docs_export(
    state: State<'_, AppState>,
    payload: DocsExportPayload,
) -> Result<DocsExportResult, String> {
    let conn = state.pool.get().map_err(err)?;
    let outcome = docs::export(&conn, &state.docs_cache, &payload);
    // 审计：导出属于数据出边界操作，成功/失败都留痕（P-07）
    let (result, detail) = match &outcome {
        Ok(value) => (
            "ok",
            serde_json::json!({
                "format": value.format,
                "scope": value.scope,
                "requests": value.request_count,
                "collections": value.collection_count,
                "bytes": value.bytes,
            }),
        ),
        Err(error) => (
            "failed",
            serde_json::json!({ "format": payload.format, "error": error }),
        ),
    };
    let target = outcome.as_ref().ok().map(|value| value.path.as_str());
    if let Err(error) =
        crate::services::audit::write(&conn, "docs_export", target, result, Some(&detail), None)
    {
        log::warn!("审计写入失败：{error}");
    }
    outcome
}
