use crate::python_exec::{self, PythonCheck};
use crate::state::AppState;
use tauri::State;

/// 探测 Python 解释器（T-08）
///
/// `path` 为空时按「设置项 → python3 → python → py -3」自动探测；传入路径时
/// 只校验该解释器，供设置页在保存前试一下。
#[tauri::command]
pub fn python_check(state: State<'_, AppState>, path: Option<String>) -> PythonCheck {
    let settings = state.settings();
    let configured = path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(settings.python_path.trim());
    python_exec::check(configured, settings.python_enabled)
}
