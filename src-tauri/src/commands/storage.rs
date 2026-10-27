use crate::state::{AppState, StorageConfig, DB_FILE};
use serde::Serialize;
use std::path::{Path, PathBuf};
use tauri::State;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageInfo {
    /// 当前正在使用的数据目录
    pub data_dir: String,
    pub db_path: String,
    /// 未自定义时的默认目录
    pub default_dir: String,
    pub is_custom: bool,
    /// 数据库主文件大小（字节）
    pub db_size: u64,
    pub wal_size: u64,
    /// 是否存在等待重启生效的目录
    pub pending_dir: Option<String>,
}

fn build_info(state: &AppState) -> StorageInfo {
    let db_size = file_size(&state.db_path);
    let wal_size = file_size(&state.db_path.with_extension("db-wal"));
    let pending = crate::state::read_storage_config(&state.config_file)
        .filter(|dir| dir != &state.data_dir)
        .map(|dir| dir.to_string_lossy().to_string());
    StorageInfo {
        data_dir: state.data_dir.to_string_lossy().to_string(),
        db_path: state.db_path.to_string_lossy().to_string(),
        default_dir: state.default_data_dir.to_string_lossy().to_string(),
        is_custom: state.is_custom,
        db_size,
        wal_size,
        pending_dir: pending,
    }
}

fn file_size(path: &Path) -> u64 {
    std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0)
}

/// 写入配置：dataDir 为空表示恢复默认目录
fn write_config(config_file: &Path, data_dir: Option<&Path>) -> Result<(), String> {
    match data_dir {
        Some(dir) => {
            let config = StorageConfig {
                data_dir: dir.to_string_lossy().to_string(),
            };
            let text = serde_json::to_string_pretty(&config).map_err(crate::util::err)?;
            std::fs::write(config_file, text).map_err(crate::util::err)
        }
        None => match std::fs::remove_file(config_file) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(crate::util::err(error)),
        },
    }
}

/// 校验目录可写：实际创建一次临时文件
fn ensure_writable(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|error| format!("无法创建目录：{error}"))?;
    let probe = dir.join(".fbinhttp-write-test");
    std::fs::write(&probe, b"ok").map_err(|error| format!("目录不可写：{error}"))?;
    let _ = std::fs::remove_file(&probe);
    Ok(())
}

#[tauri::command]
pub fn storage_info(state: State<'_, AppState>) -> StorageInfo {
    build_info(&state)
}

/// 设置数据目录；migrate 为 true 时把当前数据库复制到新目录
#[tauri::command]
pub fn storage_set(
    state: State<'_, AppState>,
    path: String,
    migrate: bool,
) -> Result<StorageInfo, String> {
    let target = PathBuf::from(path.trim());
    if target.as_os_str().is_empty() {
        return Err("请选择数据目录".to_string());
    }
    if target == state.data_dir {
        return Err("新目录与当前目录相同".to_string());
    }
    ensure_writable(&target)?;

    if migrate {
        // 先把 WAL 合并回主库，保证复制出来的是完整数据
        if let Ok(conn) = state.pool.get() {
            let _ = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
        }
        let source = &state.db_path;
        if source.exists() {
            let destination = target.join(DB_FILE);
            if destination.exists() && destination != *source {
                return Err(format!(
                    "目标目录已存在数据库文件：{}",
                    destination.to_string_lossy()
                ));
            }
            std::fs::copy(source, &destination)
                .map_err(|error| format!("复制数据库失败：{error}"))?;
        }
    }

    write_config(&state.config_file, Some(&target))?;
    Ok(build_info(&state))
}

/// 恢复默认数据目录（不删除自定义目录中的文件）
#[tauri::command]
pub fn storage_reset(state: State<'_, AppState>) -> Result<StorageInfo, String> {
    write_config(&state.config_file, None)?;
    Ok(build_info(&state))
}

/// 在系统文件管理器中打开目录
#[tauri::command]
pub fn storage_open(path: String) -> Result<(), String> {
    let target = PathBuf::from(path.trim());
    if !target.exists() {
        return Err("目录不存在".to_string());
    }
    #[cfg(windows)]
    {
        std::process::Command::new("explorer")
            .arg(target)
            .spawn()
            .map_err(|error| format!("打开目录失败：{error}"))?;
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let opener = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
        std::process::Command::new(opener)
            .arg(target)
            .spawn()
            .map_err(|error| format!("打开目录失败：{error}"))?;
        Ok(())
    }
}

/// 重启应用使数据目录变更生效
#[tauri::command]
pub fn app_restart(app: tauri::AppHandle) {
    app.restart();
}
