//! 危险路径统一校验（W4 安全基线）
//!
//! 文件写、另存、导出、导入都会从前端拿路径。前端一旦被注入，路径就是
//! 任意文件读写能力，因此统一在命令入口做：
//! - 非空、绝对路径、无 NUL；
//! - Windows 保留设备名与结尾点 / 空格；
//! - 路径长度阈值（>240 提示，避免静默写失败）；
//! - 写操作先建父目录再 canonicalize，读操作要求目标存在；
//! - 响应文件必须落在数据目录的 `responses/` 内。

use std::path::{Component, Path, PathBuf};

/// Windows 单段路径长度建议上限；超过后部分 API 会失败
const LONG_PATH_HINT: usize = 240;

/// 校验写路径，并在需要时创建父目录
pub fn prepare_write(path: &str) -> Result<PathBuf, String> {
    let path = base_check(path)?;
    if path.exists() && path.is_dir() {
        return Err(format!(
            "E_PATH: 目标是目录，不能写入文件（{}）",
            path.display()
        ));
    }
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|error| {
                format!("E_PATH: 创建目录失败（{}）：{error}", parent.display())
            })?;
            // 父目录能 canonicalize 才说明路径真实可达；写文件本身可以新建
            if let Err(error) = parent.canonicalize() {
                return Err(format!(
                    "E_PATH: 父目录不可用（{}）：{error}",
                    parent.display()
                ));
            }
        }
    }
    Ok(path)
}

/// 校验读路径：必须已存在
pub fn prepare_read(path: &str) -> Result<PathBuf, String> {
    let path = base_check(path)?;
    if !path.exists() {
        return Err(format!("E_PATH: 文件不存在（{}）", path.display()));
    }
    if path.is_dir() {
        return Err(format!(
            "E_PATH: 目标是目录，不能按文件读取（{}）",
            path.display()
        ));
    }
    path.canonicalize()
        .map_err(|error| format!("E_PATH: 无法解析路径（{}）：{error}", path.display()))
}

/// 校验并创建导出目录
pub fn prepare_dir(path: &str) -> Result<PathBuf, String> {
    let path = base_check(path)?;
    if path.exists() && !path.is_dir() {
        return Err(format!(
            "E_PATH: 目标已存在且不是目录（{}）",
            path.display()
        ));
    }
    std::fs::create_dir_all(&path)
        .map_err(|error| format!("E_PATH: 创建目录失败（{}）：{error}", path.display()))?;
    path.canonicalize()
        .map_err(|error| format!("E_PATH: 无法解析目录（{}）：{error}", path.display()))
}

/// 校验一个必须已存在的目录
pub fn prepare_existing_dir(path: &str) -> Result<PathBuf, String> {
    let path = base_check(path)?;
    if !path.is_dir() {
        return Err(format!("E_PATH: 目录不存在（{}）", path.display()));
    }
    path.canonicalize()
        .map_err(|error| format!("E_PATH: 无法解析目录（{}）：{error}", path.display()))
}

/// `target` 是否位于 `base` 内（二者都做 canonicalize 后的前缀比较）
#[cfg_attr(not(test), allow(dead_code))]
pub fn is_within(base: &Path, target: &Path) -> bool {
    let Ok(base) = base.canonicalize() else {
        return false;
    };
    let Ok(target) = target.canonicalize() else {
        return false;
    };
    target.starts_with(&base)
}

fn base_check(path: &str) -> Result<PathBuf, String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err("E_PATH: 文件路径为空".to_string());
    }
    if trimmed.contains('\0') {
        return Err("E_PATH: 路径包含非法字符".to_string());
    }
    let path = PathBuf::from(trimmed);
    if !path.is_absolute() {
        return Err(format!("E_PATH: 需要绝对路径（{trimmed}）"));
    }
    if trimmed.chars().count() > LONG_PATH_HINT {
        return Err(format!(
            "E_PATH: 路径过长（{} 字符），请缩短到 {LONG_PATH_HINT} 字符以内，或把文件放到更短的目录",
            trimmed.chars().count()
        ));
    }
    #[cfg(windows)]
    check_windows_name(&path)?;
    // 允许 `..` 只在已经规范化后不越过根；这里更严格：直接拒绝，避免路径穿越
    if path
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err("E_PATH: 路径不能包含 `..`".to_string());
    }
    Ok(path)
}

#[cfg(windows)]
fn check_windows_name(path: &Path) -> Result<(), String> {
    let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
        return Ok(());
    };
    if name.ends_with('.') || name.ends_with(' ') {
        return Err("E_PATH: Windows 文件名不能以点或空格结尾".to_string());
    }
    let stem = name
        .split('.')
        .next()
        .unwrap_or(name)
        .trim_end_matches(['.', ' '])
        .to_ascii_uppercase();
    let reserved = ["CON", "PRN", "AUX", "NUL"];
    let is_reserved = reserved.contains(&stem.as_str())
        || ((stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.len() == 4
            && stem.as_bytes()[3].is_ascii_digit());
    if is_reserved {
        return Err(format!("E_PATH: `{stem}` 是 Windows 保留设备名"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_and_relative() {
        assert!(prepare_write("").is_err());
        assert!(prepare_write("relative/file.txt").is_err());
    }

    #[test]
    fn rejects_parent_dir() {
        let error = prepare_write("C:\\tmp\\..\\secret.txt").unwrap_err();
        assert!(error.starts_with("E_PATH:"), "{error}");
    }

    #[test]
    fn accepts_temp_file() {
        let dir = crate::util::test_dir("paths");
        let file = dir.join("a.txt");
        let prepared = prepare_write(file.to_str().unwrap()).unwrap();
        std::fs::write(&prepared, "ok").unwrap();
        assert_eq!(
            prepare_read(file.to_str().unwrap()).unwrap(),
            file.canonicalize().unwrap()
        );
        assert!(is_within(&dir, &prepared));
    }
}
