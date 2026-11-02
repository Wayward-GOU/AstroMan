//! 主密钥状态、口令模式、密钥包导入导出（G-02）

use std::path::PathBuf;

use tauri::State;

use crate::secret::{self, KeyPack, SecretStatus};
use crate::state::AppState;

fn store() -> Result<&'static secret::SecretStore, String> {
    secret::store().ok_or_else(|| "E_LOCKED: 密钥模块尚未初始化".to_string())
}

#[tauri::command]
pub fn secrets_status(state: State<'_, AppState>) -> Result<SecretStatus, String> {
    Ok(store()?.status(&state.pool))
}

/// 口令模式解锁；口令错误时返回可读原因，**不清空任何数据**
#[tauri::command]
pub fn secrets_unlock(
    state: State<'_, AppState>,
    password: String,
) -> Result<SecretStatus, String> {
    let store = store()?;
    store.unlock(&state.pool, &password)?;
    Ok(store.status(&state.pool))
}

/// 切换到口令模式：用口令派生新主密钥，并整体重加密已有密钥值
#[tauri::command]
pub fn secrets_set_passphrase(
    state: State<'_, AppState>,
    password: String,
    confirm: String,
) -> Result<SecretStatus, String> {
    if password != confirm {
        return Err("两次输入的口令不一致".to_string());
    }
    let store = store()?;
    store.set_passphrase(&state.pool, &password)?;
    Ok(store.status(&state.pool))
}

/// 导出密钥包（换机 / 重装前必须做）
#[tauri::command]
pub fn secrets_export(password: String, path: String) -> Result<String, String> {
    let target = PathBuf::from(path.trim());
    if target.as_os_str().is_empty() {
        return Err("请选择保存位置".to_string());
    }
    let store = store()?;
    store.export_keypack(&target, &password)?;
    Ok(target.to_string_lossy().to_string())
}

/// 导入密钥包：密钥丢失时用它恢复
#[tauri::command]
pub fn secrets_import(
    state: State<'_, AppState>,
    password: String,
    path: String,
) -> Result<SecretStatus, String> {
    let source = PathBuf::from(path.trim());
    if !source.exists() {
        return Err("密钥包文件不存在".to_string());
    }
    let store = store()?;
    store.import_keypack(&state.pool, &source, &password)?;
    Ok(store.status(&state.pool))
}

/// 重置全部密钥值（清空环境变量里的 secret 与 AI Key），需二次确认
#[tauri::command]
pub fn secrets_reset(state: State<'_, AppState>, confirmed: bool) -> Result<usize, String> {
    if !confirmed {
        return Err("需要二次确认才能重置密钥".to_string());
    }
    let store = store()?;
    store.reset_values(&state.pool)
}

/// 只读：查看密钥包文件是否合法（导入前预检，避免用户点错文件）
#[tauri::command]
pub fn secrets_inspect_pack(path: String) -> Result<KeyPack, String> {
    let text = std::fs::read_to_string(path.trim()).map_err(|e| format!("读取密钥包失败：{e}"))?;
    let pack: KeyPack =
        serde_json::from_str(&text).map_err(|e| format!("密钥包格式不正确：{e}"))?;
    if pack.app != "AstroMan" {
        return Err("这不是 AstroMan 导出的密钥包".to_string());
    }
    Ok(pack)
}

/// 诊断：把最近用到的敏感值从脱敏清单里清掉（切换环境时调用，避免旧值被误打码）
#[tauri::command]
pub fn secrets_clear_cache() {
    crate::logging::clear_secrets();
}
