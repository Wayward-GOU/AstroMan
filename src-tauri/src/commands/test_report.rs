//! 测试运行报告与附件命令（T-13~T-16）

use crate::report_builder::{self, ArtifactEntry, TestRunReport};
use crate::state::AppState;
use std::path::PathBuf;
use tauri::State;

/// 生成报告（HTML / JSON / JUnit / summary），默认全部生成
#[tauri::command]
pub fn test_report_generate(
    state: State<'_, AppState>,
    run_id: i64,
    formats: Option<Vec<String>>,
    out_dir: Option<String>,
) -> Result<TestRunReport, String> {
    let formats = formats.unwrap_or_default();
    let out = out_dir
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    report_builder::generate(state.inner(), run_id, &formats, out.as_deref())
}

/// 某次运行的附件索引
#[tauri::command]
pub fn test_artifact_list(
    state: State<'_, AppState>,
    run_id: i64,
) -> Result<Vec<ArtifactEntry>, String> {
    report_builder::list_artifacts(state.inner(), run_id)
}

/// 把附件复制到目标路径（下载）
#[tauri::command]
pub fn test_artifact_export(
    state: State<'_, AppState>,
    artifact_id: i64,
    path: String,
) -> Result<(), String> {
    report_builder::export_artifact(state.inner(), artifact_id, &path)
}
