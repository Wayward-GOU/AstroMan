use tauri::command;

/// 读取文本文件（用于 Runner 数据文件等场景），最大 20 MB
#[command]
pub fn file_read_text(path: String) -> Result<String, String> {
    let path = path.trim();
    if path.is_empty() {
        return Err("文件路径为空".to_string());
    }
    let meta = std::fs::metadata(path).map_err(|e| format!("读取文件失败（{path}）：{e}"))?;
    if meta.len() > 20 * 1024 * 1024 {
        return Err("文件超过 20 MB，请拆分后再导入".to_string());
    }
    std::fs::read_to_string(path).map_err(|e| format!("读取文件失败（{path}）：{e}"))
}

/// 判断文件是否已存在：导出前用于提示覆盖（T-17）
#[command]
pub fn file_exists(path: String) -> bool {
    let path = path.trim();
    !path.is_empty() && std::path::Path::new(path).exists()
}

/// 写入文本文件（用于导出结果、保存响应体等场景）
#[command]
pub fn file_write_text(path: String, content: String) -> Result<(), String> {
    let path = path.trim();
    if path.is_empty() {
        return Err("文件路径为空".to_string());
    }
    if let Some(parent) = std::path::Path::new(path).parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("创建目录失败（{}）：{e}", parent.display()))?;
        }
    }
    std::fs::write(path, content).map_err(|e| format!("写入文件失败（{path}）：{e}"))
}

/* ---------- v1.0.8：二进制与响应文件（G-04） ---------- */

use std::path::{Path, PathBuf};

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use tauri::State;

use crate::state::AppState;

/// 写入二进制文件（Base64 入参）：另存图片、导出小文件用
#[command]
pub fn file_write_binary(path: String, base64: String) -> Result<(), String> {
    let target = path.trim();
    if target.is_empty() {
        return Err("文件路径为空".to_string());
    }
    let bytes = B64
        .decode(base64.as_bytes())
        .map_err(|_| "Base64 内容不合法".to_string())?;
    let path = Path::new(target);
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("创建目录失败（{}）：{e}", parent.display()))?;
        }
    }
    std::fs::write(path, bytes).map_err(|e| format!("写入文件失败（{target}）：{e}"))
}

/// 复制文件：响应落盘文件「另存为」优先用它，避免大文件过 IPC
#[command]
pub fn file_copy(src: String, dst: String) -> Result<u64, String> {
    let from = Path::new(src.trim());
    let to = Path::new(dst.trim());
    if !from.exists() {
        return Err("源文件不存在".to_string());
    }
    if let Some(parent) = to.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("创建目录失败（{}）：{e}", parent.display()))?;
        }
    }
    std::fs::copy(from, to)
        .map_err(|e| format!("复制文件失败（{} → {}）：{e}", from.display(), to.display()))
}

/// 把响应文件的相对路径换成绝对路径，供 `asset:` 预览使用
///
/// 只接受 `responses/` 开头且不含 `..` 的相对路径，防止前端传任意路径读本机文件。
#[command]
pub fn response_file_path(state: State<'_, AppState>, rel_path: String) -> Result<String, String> {
    let rel = rel_path.trim().replace('\\', "/");
    if !rel.starts_with("responses/") || rel.contains("..") {
        return Err("响应文件路径不合法".to_string());
    }
    let path: PathBuf = state.data_dir.join(&rel);
    if !path.exists() {
        return Err("响应文件已被清理（默认保留 30 天 / 1GB）".to_string());
    }
    Ok(path.to_string_lossy().to_string())
}

/// 清理响应文件：30 天过期 + 总量 1GB 上限
///
/// 启动时后台跑一次即可，不需要定时器：文件只会在发请求时增长。
pub fn cleanup_responses(data_dir: &Path) -> (usize, u64) {
    const MAX_TOTAL: u64 = 1024 * 1024 * 1024;
    const MAX_AGE_SECS: u64 = 30 * 24 * 3600;

    let root = data_dir.join("responses");
    let Ok(days) = std::fs::read_dir(&root) else {
        return (0, 0);
    };
    let now = std::time::SystemTime::now();
    let mut files: Vec<(std::time::SystemTime, u64, PathBuf)> = Vec::new();
    for day in days.flatten() {
        let Ok(entries) = std::fs::read_dir(day.path()) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(meta) = entry.metadata() else { continue };
            if !meta.is_file() {
                continue;
            }
            let modified = meta.modified().unwrap_or(now);
            files.push((modified, meta.len(), entry.path()));
        }
    }

    let mut deleted = 0usize;
    let mut freed = 0u64;
    let mut remaining: Vec<(std::time::SystemTime, u64, PathBuf)> = Vec::new();
    for (modified, size, path) in files {
        let age = now
            .duration_since(modified)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        if age > MAX_AGE_SECS {
            if std::fs::remove_file(&path).is_ok() {
                deleted += 1;
                freed += size;
            }
        } else {
            remaining.push((modified, size, path));
        }
    }

    let mut total: u64 = remaining.iter().map(|(_, size, _)| *size).sum();
    if total > MAX_TOTAL {
        remaining.sort_by_key(|(time, _, _)| *time);
        for (_, size, path) in remaining {
            if total <= MAX_TOTAL {
                break;
            }
            if std::fs::remove_file(&path).is_ok() {
                deleted += 1;
                freed += size;
                total = total.saturating_sub(size);
            }
        }
    }
    if deleted > 0 {
        log::info!("已清理 {deleted} 个过期响应文件，释放 {freed} 字节");
    }
    (deleted, freed)
}
