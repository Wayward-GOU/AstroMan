use crate::state::{AppSettings, AppState};
use tauri::State;

#[tauri::command]
pub fn settings_get(state: State<'_, AppState>) -> AppSettings {
    state.settings()
}

/// 保存网络与安全设置，同时重建 HTTP 客户端
#[tauri::command]
pub fn settings_set(
    state: State<'_, AppState>,
    settings: AppSettings,
) -> Result<AppSettings, String> {
    state.update_settings(settings)
}
