use crate::models::AppInfo;
use crate::state::AppState;
use tauri::State;

#[tauri::command]
pub fn app_info(state: State<'_, AppState>) -> AppInfo {
    AppInfo {
        version: env!("CARGO_PKG_VERSION").to_string(),
        data_dir: state.data_dir.to_string_lossy().to_string(),
        db_path: state.db_path.to_string_lossy().to_string(),
    }
}
