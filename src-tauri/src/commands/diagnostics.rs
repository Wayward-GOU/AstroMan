//! 诊断与前端日志转储。
//!
//! 前端异常只留在 DevTools 里，用户遇到问题时帮助不到我们；
//! 这里把「界面异常 / 未处理 Promise / 手动上报」转成后端日志，
//! 统一走脱敏与按天滚动，诊断包也能直接带走。
//!
//! 诊断包本身的实现在 `services::diagnostics`，命令壳仍在 `commands::backup`
//!（保持既有前端调用不变）。

use serde::Deserialize;
use tauri::State;

use crate::state::AppState;
use crate::util::err;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FrontendLogPayload {
    /// error / warn / info / debug
    #[serde(default)]
    pub level: String,
    pub message: String,
    /// 附加上下文：当前页面、触发位置等
    #[serde(default)]
    pub context: Option<String>,
}

/// 前端日志落盘；不返回错误，日志失败不能影响界面
#[tauri::command]
pub fn frontend_log(payload: FrontendLogPayload) {
    let message = payload.message.trim();
    if message.is_empty() {
        return;
    }
    let text = match payload.context.as_deref().map(str::trim) {
        Some(context) if !context.is_empty() => format!("[frontend] {message} | {context}"),
        _ => format!("[frontend] {message}"),
    };
    match payload.level.trim().to_ascii_lowercase().as_str() {
        "error" => log::error!("{text}"),
        "warn" => log::warn!("{text}"),
        "debug" => log::debug!("{text}"),
        _ => log::info!("{text}"),
    }
}

/// 最近审计（P-07）：默认 100 条，按时间倒序
#[tauri::command]
pub fn audit_log_list(
    state: State<'_, AppState>,
    action: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
) -> Result<Vec<crate::models::AuditLog>, String> {
    let conn = state.pool.get().map_err(err)?;
    crate::services::audit::list(
        &conn,
        action.as_deref(),
        limit.unwrap_or(100),
        offset.unwrap_or(0),
    )
}

/// 手动清理超过保留期的审计，返回删除条数
#[tauri::command]
pub fn audit_log_cleanup(state: State<'_, AppState>) -> Result<usize, String> {
    let conn = state.pool.get().map_err(err)?;
    let removed = crate::services::audit::cleanup(&conn, crate::services::audit::RETENTION_DAYS)?;
    log::info!("审计日志清理完成，删除 {removed} 条");
    Ok(removed)
}
