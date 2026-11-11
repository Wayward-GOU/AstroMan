use crate::state::{AppSettings, AppState};
use tauri::State;

#[tauri::command]
pub fn settings_get(state: State<'_, AppState>) -> AppSettings {
    state.settings()
}

/// 当前运行档位与抓包 / 响应体预算（P-05），设置页据此展示「自动」解析结果
#[tauri::command]
pub fn performance_profile(state: State<'_, AppState>) -> serde_json::Value {
    let profile = state.profile();
    serde_json::json!({
        "requested": state.settings().performance_mode,
        "tier": profile.tier,
        "limits": profile.limits,
        "totalMemoryBytes": profile.total_memory_bytes,
        "description": profile.limits.describe(),
    })
}

/// 保存网络与安全设置，同时重建 HTTP 客户端
#[tauri::command]
pub fn settings_set(
    state: State<'_, AppState>,
    settings: AppSettings,
) -> Result<AppSettings, String> {
    state.update_settings(settings)
}

/// 企业策略：当前生效值、文件位置与解析错误。界面只读展示，由管理员改文件。
#[tauri::command]
pub fn policy_get(state: State<'_, AppState>) -> serde_json::Value {
    serde_json::json!({
        "policy": state.policy(),
        "path": state.policy_path().to_string_lossy(),
        "error": state.policy_error(),
    })
}

/// 管理员改完 policy.json 后热加载，无需重启
#[tauri::command]
pub fn policy_reload(state: State<'_, AppState>) -> serde_json::Value {
    state.reload_policy();
    policy_get(state)
}
