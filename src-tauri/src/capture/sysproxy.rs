//! Windows 系统代理接管。
//!
//! 只改当前用户的 WinINET 代理（`ProxyEnable` / `ProxyServer`），关闭时按备份还原。
//! 备份文件落在数据目录，应用异常退出后下次启动会先把残留代理恢复掉。
//! macOS / Linux 暂不自动改系统代理，返回明确错误，让用户手动设置。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const BACKUP_FILE: &str = "capture-proxy-backup.json";

#[cfg(windows)]
const REG_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProxyBackup {
    active: bool,
    proxy_enable: Option<String>,
    proxy_server: Option<String>,
}

fn backup_path(data_dir: &Path) -> PathBuf {
    data_dir.join(BACKUP_FILE)
}

fn read_backup(data_dir: &Path) -> ProxyBackup {
    std::fs::read_to_string(backup_path(data_dir))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn write_backup(data_dir: &Path, backup: &ProxyBackup) -> Result<(), String> {
    if let Some(parent) = backup_path(data_dir).parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("E_SYSPROXY: 创建代理备份目录失败 {e}"))?;
    }
    let text = serde_json::to_string_pretty(backup)
        .map_err(|e| format!("E_SYSPROXY: 序列化代理备份失败 {e}"))?;
    std::fs::write(backup_path(data_dir), text)
        .map_err(|e| format!("E_SYSPROXY: 写入代理备份失败 {e}"))
}

/// 开启系统代理，指向 `127.0.0.1:port`。
#[cfg(windows)]
pub fn enable(data_dir: &Path, port: u16) -> Result<(), String> {
    let mut backup = read_backup(data_dir);
    if !backup.active {
        backup = ProxyBackup {
            active: true,
            proxy_enable: read_value("ProxyEnable"),
            proxy_server: read_value("ProxyServer"),
        };
        // 先落备份再改注册表：写注册表失败时最多留下一个未生效的备份，
        // 恢复时按「原值为空就删除」处理，不会破坏用户配置。
        write_backup(data_dir, &backup)?;
    }

    let address = format!("127.0.0.1:{port}");
    write_string("ProxyServer", &address)?;
    write_dword("ProxyEnable", 1)?;
    refresh();
    log::info!("已接管系统代理：{address}");
    Ok(())
}

#[cfg(not(windows))]
pub fn enable(_data_dir: &Path, _port: u16) -> Result<(), String> {
    Err("E_SYSPROXY: 当前平台暂不支持自动设置系统代理，请手动把系统代理指向 127.0.0.1:端口".into())
}

/// 还原系统代理。没有备份时是空操作，便于停止录制时无条件调用。
#[cfg(windows)]
pub fn disable(data_dir: &Path) -> Result<(), String> {
    let backup = read_backup(data_dir);
    if !backup.active {
        return Ok(());
    }
    restore_key("ProxyServer", backup.proxy_server.as_deref(), false)?;
    restore_key("ProxyEnable", backup.proxy_enable.as_deref(), true)?;
    write_backup(
        data_dir,
        &ProxyBackup {
            active: false,
            ..backup
        },
    )?;
    refresh();
    log::info!("已恢复系统代理");
    Ok(())
}

#[cfg(not(windows))]
pub fn disable(_data_dir: &Path) -> Result<(), String> {
    Ok(())
}

/// 应用启动时清理上次崩溃留下的系统代理。
pub fn restore_stale(data_dir: &Path) {
    let backup = read_backup(data_dir);
    if !backup.active {
        return;
    }
    match disable(data_dir) {
        Ok(()) => log::info!("检测到上次抓包残留的系统代理，已恢复"),
        Err(error) => log::warn!("恢复残留系统代理失败：{error}"),
    }
}

#[cfg(windows)]
fn read_value(name: &str) -> Option<String> {
    let output = std::process::Command::new("reg")
        .args(["query", REG_KEY, "/v", name])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let line = text
        .lines()
        .find(|line| line.trim_start().starts_with(name))?;
    let tokens: Vec<&str> = line.split_whitespace().collect();
    if tokens.len() < 3 {
        return None;
    }
    Some(tokens[2..].join(" "))
}

#[cfg(windows)]
fn write_string(name: &str, value: &str) -> Result<(), String> {
    run_reg(&[
        "add", REG_KEY, "/v", name, "/t", "REG_SZ", "/d", value, "/f",
    ])
}

#[cfg(windows)]
fn write_dword(name: &str, value: u32) -> Result<(), String> {
    let value = value.to_string();
    run_reg(&[
        "add",
        REG_KEY,
        "/v",
        name,
        "/t",
        "REG_DWORD",
        "/d",
        &value,
        "/f",
    ])
}

#[cfg(windows)]
fn restore_key(name: &str, value: Option<&str>, dword: bool) -> Result<(), String> {
    match value {
        Some(value) if dword => write_dword(name, parse_dword(value)),
        Some(value) => write_string(name, value),
        None => {
            // 原值不存在就删掉，避免把一个空字符串写回用户配置
            let _ = std::process::Command::new("reg")
                .args(["delete", REG_KEY, "/v", name, "/f"])
                .output();
            Ok(())
        }
    }
}

#[cfg(windows)]
fn parse_dword(value: &str) -> u32 {
    let value = value.trim();
    if let Some(hex) = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
    {
        u32::from_str_radix(hex, 16).unwrap_or(1)
    } else {
        value.parse::<u32>().unwrap_or(1)
    }
}

#[cfg(windows)]
fn run_reg(args: &[&str]) -> Result<(), String> {
    let output = std::process::Command::new("reg")
        .args(args)
        .output()
        .map_err(|e| format!("E_SYSPROXY: 调用 reg 失败 {e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "E_SYSPROXY: 修改系统代理失败 {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

#[cfg(windows)]
fn refresh() {
    use windows_sys::Win32::Networking::WinInet::{
        InternetSetOptionW, INTERNET_OPTION_REFRESH, INTERNET_OPTION_SETTINGS_CHANGED,
    };
    unsafe {
        InternetSetOptionW(
            std::ptr::null(),
            INTERNET_OPTION_SETTINGS_CHANGED,
            std::ptr::null(),
            0,
        );
        InternetSetOptionW(
            std::ptr::null(),
            INTERNET_OPTION_REFRESH,
            std::ptr::null(),
            0,
        );
    }
}
