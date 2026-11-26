use crate::state::{AppSettings, AppState};
use tauri::{AppHandle, State};

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
    app: AppHandle,
    state: State<'_, AppState>,
    settings: AppSettings,
) -> Result<AppSettings, String> {
    let saved = state.update_settings(settings)?;
    // 设备发现开关 / 名称 / 邀请策略跟随设置即时生效，不要求重启
    if saved.cluster_presence_enabled {
        if let Err(error) = crate::services::cluster::presence::start(
            &app,
            &state.data_dir,
            &saved.cluster_device_name,
            true,
            &saved.cluster_invite_policy,
        ) {
            // 端口被占用时不要让「保存设置」整体失败：设置页单独展示发现错误
            log::warn!("启动设备发现失败：{error}");
        }
        crate::services::cluster::presence::update_identity(
            &saved.cluster_device_name,
            true,
            &saved.cluster_invite_policy,
        );
    } else {
        crate::services::cluster::presence::stop();
    }
    Ok(saved)
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
