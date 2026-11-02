//! 备份 / 恢复 / 自检 / 诊断包命令（G-03）

use std::path::PathBuf;

use tauri::{AppHandle, Emitter, State};

use crate::backup::{self, BackupItem, CheckResult};
use crate::state::AppState;

/// 备份列表
#[tauri::command]
pub fn backup_list(state: State<'_, AppState>) -> Vec<BackupItem> {
    backup::list(&state.data_dir)
}

/// 手动创建备份
#[tauri::command]
pub fn backup_create(app: AppHandle, state: State<'_, AppState>) -> Result<BackupItem, String> {
    let _ = app.emit(
        "backup://progress",
        serde_json::json!({ "phase": "start", "percent": 0 }),
    );
    let result = backup::create(&state.pool, &state.data_dir, backup::BackupKind::Manual);
    let _ = app.emit(
        "backup://progress",
        serde_json::json!({ "phase": if result.is_ok() { "done" } else { "error" }, "percent": 100 }),
    );
    result
}

/// 请求恢复：只做校验 + 写标记，真正的替换在下次启动时完成
///
/// 返回后前端应立即调用 `app_restart`。
#[tauri::command]
pub fn backup_restore(state: State<'_, AppState>, name: String) -> Result<BackupItem, String> {
    let path = resolve_backup(&state, &name)?;
    let item = backup::validate(&path)?;
    backup::mark_restore(&state.config_file, &path)?;
    Ok(item)
}

/// 取消已排队的恢复（用户改主意时）
#[tauri::command]
pub fn backup_cancel_restore(state: State<'_, AppState>) -> Result<(), String> {
    let marker = state
        .config_file
        .parent()
        .map(|dir| dir.join("restore.pending"))
        .unwrap_or_else(|| PathBuf::from("restore.pending"));
    if marker.exists() {
        std::fs::remove_file(&marker).map_err(|e| format!("取消恢复失败：{e}"))?;
    }
    Ok(())
}

#[tauri::command]
pub fn backup_delete(state: State<'_, AppState>, name: String) -> Result<(), String> {
    let path = resolve_backup(&state, &name)?;
    std::fs::remove_file(&path).map_err(|e| format!("删除备份失败：{e}"))
}

/// 打开备份目录（交给前端调 `storage_open`，这里只负责把路径拼好）
#[tauri::command]
pub fn backup_dir_path(state: State<'_, AppState>) -> Result<String, String> {
    let dir = backup::backup_dir(&state.data_dir);
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建备份目录失败：{e}"))?;
    Ok(dir.to_string_lossy().to_string())
}

#[tauri::command]
pub fn db_check(state: State<'_, AppState>, deep: bool) -> Result<CheckResult, String> {
    let _guard = state
        .heavy_ops
        .try_lock()
        .ok_or_else(|| "E_BUSY: 正在执行其它重任务（备份 / 检查），请稍后重试".to_string())?;
    backup::check(&state.pool, deep)
}

/// 导出诊断包到指定目录，返回目录路径
#[tauri::command]
pub fn diagnostics_export(state: State<'_, AppState>, path: String) -> Result<String, String> {
    let target = PathBuf::from(path.trim());
    if target.as_os_str().is_empty() {
        return Err("请选择保存位置".to_string());
    }
    backup::write_diagnostics(&state, &target)
}

/// 只允许删除 / 恢复备份目录内的文件，避免前端传任意路径
fn resolve_backup(state: &AppState, name: &str) -> Result<PathBuf, String> {
    let trimmed = name.trim();
    if trimmed.is_empty()
        || trimmed.contains("..")
        || trimmed.contains('/')
        || trimmed.contains('\\')
    {
        return Err("备份文件名不合法".to_string());
    }
    let path = backup::backup_dir(&state.data_dir).join(trimmed);
    if !path.exists() {
        return Err("备份不存在，可能已被删除".to_string());
    }
    Ok(path)
}
