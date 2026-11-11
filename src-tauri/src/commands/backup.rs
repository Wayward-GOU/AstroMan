//! 备份 / 恢复 / 自检 / 诊断包命令（G-03）

use std::path::PathBuf;

use tauri::{AppHandle, Emitter, State};

use crate::backup::{self, BackupItem, CheckResult};
use crate::services::backup_target::{
    self, BackupTarget, BackupTargetInput, BackupUploadResult, RemoteBackup, TargetTestReport,
};
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

/// 导出诊断包到指定路径，返回实际写入路径（D-01 / A-22）
#[tauri::command]
pub fn diagnostics_export(state: State<'_, AppState>, path: String) -> Result<String, String> {
    let target = PathBuf::from(path.trim());
    if target.as_os_str().is_empty() {
        return Err("请选择保存位置".to_string());
    }
    let result = crate::services::diagnostics::export(state.inner(), &target)?;
    Ok(result.path)
}

#[tauri::command]
pub fn backup_target_list(state: State<'_, AppState>) -> Result<Vec<BackupTarget>, String> {
    let conn = state.pool.get().map_err(crate::util::err)?;
    backup_target::list(&conn)
}

/// 新建 / 修改异地备份目标；密码为空表示保留原值
#[tauri::command]
pub fn backup_target_save(
    state: State<'_, AppState>,
    input: BackupTargetInput,
) -> Result<BackupTarget, String> {
    let conn = state.pool.get().map_err(crate::util::err)?;
    backup_target::save(&conn, input)
}

#[tauri::command]
pub fn backup_target_delete(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    let conn = state.pool.get().map_err(crate::util::err)?;
    backup_target::delete(&conn, id)
}

#[tauri::command]
pub fn backup_target_test(state: State<'_, AppState>, id: i64) -> Result<TargetTestReport, String> {
    let conn = state.pool.get().map_err(crate::util::err)?;
    backup_target::test(&conn, id)
}

#[tauri::command]
pub fn backup_target_remote_list(
    state: State<'_, AppState>,
    id: i64,
) -> Result<Vec<RemoteBackup>, String> {
    let conn = state.pool.get().map_err(crate::util::err)?;
    backup_target::remote_list(&conn, id)
}

/// 上传加密备份：建快照 → 加密 → 推送；口令只在本命令内使用，不落库
#[tauri::command]
pub fn backup_upload(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
    passphrase: String,
) -> Result<BackupUploadResult, String> {
    if state.policy().backup_must_encrypt() && passphrase.trim().is_empty() {
        return Err(
            "E_POLICY: 企业策略要求备份必须加密，请先设置备份口令（policy.json: forceEncryptBackup）"
                .to_string(),
        );
    }
    let _guard = state
        .heavy_ops
        .try_lock()
        .ok_or_else(|| "E_BUSY: 正在执行其它重任务（备份 / 检查），请稍后重试".to_string())?;
    let _ = app.emit(
        "backup://progress",
        serde_json::json!({ "phase": "snapshot", "percent": 10 }),
    );
    let result = backup_target::upload(&state.pool, &state.data_dir, id, &passphrase);
    let _ = app.emit(
        "backup://progress",
        serde_json::json!({
            "phase": if result.is_ok() { "done" } else { "error" },
            "percent": 100
        }),
    );
    result
}

/// 从异地备份恢复：下载 + 解密 + 校验 + 写待恢复标记；返回后前端应调用 app_restart
#[tauri::command]
pub fn backup_restore_from_target(
    state: State<'_, AppState>,
    id: i64,
    name: String,
    passphrase: String,
) -> Result<BackupItem, String> {
    backup_target::restore_from_target(
        &state.pool,
        &state.data_dir,
        &state.config_file,
        id,
        &name,
        &passphrase,
    )
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
