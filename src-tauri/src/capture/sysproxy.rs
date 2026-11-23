//! 系统代理接管。
//!
//! - Windows：只改当前用户的 WinINET 代理（`ProxyEnable` / `ProxyServer`），关闭时按备份还原。
//! - macOS：通过 `networksetup` 修改当前默认网卡对应服务的 HTTP / HTTPS 代理；
//!   普通调用失败时用 `osascript` 弹一次管理员授权。
//! - 备份文件落在数据目录，应用异常退出后下次启动会先把残留代理恢复掉。
//! - Linux 暂不自动改系统代理，返回明确错误，让用户手动设置。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const BACKUP_FILE: &str = "capture-proxy-backup.json";

#[cfg(windows)]
const REG_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProxyBackup {
    active: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    proxy_enable: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    proxy_server: Option<String>,
    #[cfg(target_os = "macos")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    macos: Option<MacProxyBackup>,
}

#[cfg(target_os = "macos")]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MacProxyBackup {
    service: String,
    web: MacProxyState,
    secure: MacProxyState,
}

#[cfg(target_os = "macos")]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MacProxyState {
    enabled: bool,
    server: String,
    port: u16,
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

#[cfg(any(windows, target_os = "macos"))]
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

#[cfg(target_os = "macos")]
pub fn enable(data_dir: &Path, port: u16) -> Result<(), String> {
    enable_macos(data_dir, port, true)
}

#[cfg(all(not(windows), not(target_os = "macos")))]
pub fn enable(_data_dir: &Path, _port: u16) -> Result<(), String> {
    Err("E_SYSPROXY: 当前平台暂不支持自动设置系统代理，请手动把系统代理指向 127.0.0.1:端口".into())
}

/// 还原系统代理。没有备份时是空操作，便于停止录制时无条件调用。
pub fn disable(data_dir: &Path) -> Result<(), String> {
    disable_with_prompt(data_dir, true)
}

#[cfg(windows)]
fn disable_with_prompt(data_dir: &Path, _prompt: bool) -> Result<(), String> {
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

#[cfg(target_os = "macos")]
fn disable_with_prompt(data_dir: &Path, prompt: bool) -> Result<(), String> {
    let backup = read_backup(data_dir);
    if !backup.active {
        return Ok(());
    }
    if let Some(mac) = backup.macos.as_ref() {
        let commands = vec![
            restore_proxy_command("-setwebproxy", "-setwebproxystate", &mac.service, &mac.web),
            restore_proxy_command(
                "-setsecurewebproxy",
                "-setsecurewebproxystate",
                &mac.service,
                &mac.secure,
            ),
        ];
        run_macos_commands(&commands, prompt)?;
    } else {
        log::warn!("macOS 代理备份缺少网络服务信息，跳过自动恢复");
    }
    write_backup(
        data_dir,
        &ProxyBackup {
            active: false,
            ..backup
        },
    )?;
    log::info!("已恢复系统代理");
    Ok(())
}

#[cfg(all(not(windows), not(target_os = "macos")))]
fn disable_with_prompt(_data_dir: &Path, _prompt: bool) -> Result<(), String> {
    Ok(())
}

#[cfg(target_os = "macos")]
fn enable_macos(data_dir: &Path, port: u16, prompt: bool) -> Result<(), String> {
    let mut backup = read_backup(data_dir);
    if !backup.active {
        let service = active_service()?;
        let web = read_proxy_state(&service, false)?;
        let secure = read_proxy_state(&service, true)?;
        backup = ProxyBackup {
            active: true,
            macos: Some(MacProxyBackup {
                service,
                web,
                secure,
            }),
            ..Default::default()
        };
        // 先落备份再改系统设置，失败时下次启动或用户关闭时仍能按原值恢复
        write_backup(data_dir, &backup)?;
    }
    let mac = backup.macos.as_ref().ok_or_else(|| {
        "E_SYSPROXY: 代理备份缺少 macOS 网络服务信息，请删除备份文件后重试".to_string()
    })?;
    let commands = vec![
        vec![
            "-setwebproxy".into(),
            mac.service.clone(),
            "127.0.0.1".into(),
            port.to_string(),
        ],
        vec![
            "-setsecurewebproxy".into(),
            mac.service.clone(),
            "127.0.0.1".into(),
            port.to_string(),
        ],
    ];
    run_macos_commands(&commands, prompt)?;
    log::info!("已接管系统代理：{} → 127.0.0.1:{port}", mac.service);
    Ok(())
}

/// 取默认路由所在网卡，再映射到「系统设置 → 网络」里的服务名。
#[cfg(target_os = "macos")]
fn active_service() -> Result<String, String> {
    let route = std::process::Command::new("/sbin/route")
        .args(["-n", "get", "default"])
        .output()
        .map_err(|error| format!("E_SYSPROXY: 读取默认路由失败 {error}"))?;
    let route_text = String::from_utf8_lossy(&route.stdout);
    let device = parse_default_interface(&route_text)
        .ok_or_else(|| "E_SYSPROXY: 未找到默认网络接口，请先连接网络".to_string())?;
    let order = networksetup_output(&["-listnetworkserviceorder"])?;
    parse_service_for_device(&order, &device).ok_or_else(|| {
        format!("E_SYSPROXY: 默认网卡 {device} 没有对应的网络服务，请手动设置系统代理")
    })
}

#[cfg(target_os = "macos")]
fn read_proxy_state(service: &str, secure: bool) -> Result<MacProxyState, String> {
    let flag = if secure {
        "-getsecurewebproxy"
    } else {
        "-getwebproxy"
    };
    let text = networksetup_output(&[flag, service])?;
    Ok(parse_proxy_state(&text))
}

#[cfg(target_os = "macos")]
fn restore_proxy_command(
    set_flag: &str,
    state_flag: &str,
    service: &str,
    state: &MacProxyState,
) -> Vec<String> {
    if state.enabled && !state.server.is_empty() && state.port > 0 {
        vec![
            set_flag.into(),
            service.into(),
            state.server.clone(),
            state.port.to_string(),
        ]
    } else {
        vec![state_flag.into(), service.into(), "off".into()]
    }
}

/// 先按普通权限执行；失败且允许交互时，用一次管理员授权重跑整组命令。
#[cfg(target_os = "macos")]
fn run_macos_commands(commands: &[Vec<String>], prompt: bool) -> Result<(), String> {
    let mut first_error = None;
    for args in commands {
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        if let Err(error) = networksetup_output(&refs) {
            first_error = Some(error);
            break;
        }
    }
    let Some(error) = first_error else {
        return Ok(());
    };
    if !prompt {
        return Err(error);
    }
    let script = commands
        .iter()
        .map(|args| {
            let mut parts = vec!["/usr/sbin/networksetup".to_string()];
            parts.extend(args.iter().map(|value| shell_quote(value)));
            parts.join(" ")
        })
        .collect::<Vec<_>>()
        .join(" && ");
    run_with_admin(&script)
}

#[cfg(target_os = "macos")]
fn networksetup_output(args: &[&str]) -> Result<String, String> {
    let output = std::process::Command::new("/usr/sbin/networksetup")
        .args(args)
        .output()
        .map_err(|error| format!("E_SYSPROXY: 调用 networksetup 失败 {error}"))?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let detail = if stderr.trim().is_empty() {
        stdout.trim()
    } else {
        stderr.trim()
    };
    Err(format!(
        "E_SYSPROXY: networksetup {} 失败：{detail}",
        args.join(" ")
    ))
}

#[cfg(target_os = "macos")]
fn run_with_admin(command: &str) -> Result<(), String> {
    let script = format!(
        "do shell script \"{}\" with administrator privileges",
        applescript_escape(command)
    );
    let output = std::process::Command::new("/usr/bin/osascript")
        .args(["-e", &script])
        .output()
        .map_err(|error| format!("E_SYSPROXY: 调用 osascript 失败 {error}"))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let message = stderr.trim();
    if message.contains("User canceled") || message.contains("(-128)") {
        return Err("E_SYSPROXY: 已取消管理员授权，系统代理未修改".into());
    }
    Err(format!(
        "E_SYSPROXY: 获取管理员权限后设置代理失败 {message}"
    ))
}

#[cfg(target_os = "macos")]
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(target_os = "macos")]
fn applescript_escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(target_os = "macos")]
fn parse_default_interface(text: &str) -> Option<String> {
    text.lines().find_map(|line| {
        line.trim()
            .strip_prefix("interface:")
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    })
}

#[cfg(target_os = "macos")]
fn parse_service_for_device(order: &str, device: &str) -> Option<String> {
    let marker = format!("Device: {device})");
    let mut current = None;
    for line in order.lines().map(str::trim) {
        if let Some(name) = service_line_name(line) {
            current = Some(name);
        } else if line.starts_with("(Hardware Port:") && line.ends_with(&marker) {
            return current;
        }
    }
    None
}

/// 服务行形如 `(3) Wi-Fi`，停用服务形如 `(*) Wi-Fi`；硬件行由 `(Hardware Port:` 开头。
#[cfg(target_os = "macos")]
fn service_line_name(line: &str) -> Option<String> {
    let close = line.find(')')?;
    if !line.starts_with('(') {
        return None;
    }
    let prefix = &line[1..close];
    if prefix != "*" && !prefix.chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }
    let name = line[close + 1..].trim();
    (!name.is_empty()).then(|| name.to_string())
}

#[cfg(target_os = "macos")]
fn parse_proxy_state(text: &str) -> MacProxyState {
    let mut state = MacProxyState::default();
    for line in text.lines() {
        if let Some(value) = line.strip_prefix("Enabled:") {
            state.enabled = value.trim().eq_ignore_ascii_case("yes");
        } else if let Some(value) = line.strip_prefix("Server:") {
            state.server = value.trim().to_string();
        } else if let Some(value) = line.strip_prefix("Port:") {
            state.port = value.trim().parse().unwrap_or(0);
        }
    }
    state
}

/// 应用启动时清理上次崩溃留下的系统代理。
pub fn restore_stale(data_dir: &Path) {
    let backup = read_backup(data_dir);
    if !backup.active {
        return;
    }
    // 启动阶段不弹管理员授权：优先无提权恢复，失败时保留备份由用户手动关闭
    match disable_with_prompt(data_dir, false) {
        Ok(()) => log::info!("检测到上次抓包残留的系统代理，已恢复"),
        Err(error) => log::warn!("恢复残留系统代理失败：{error}"),
    }
}

#[cfg(windows)]
fn read_value(name: &str) -> Option<String> {
    let output = crate::util::hidden_command("reg")
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
            let _ = crate::util::hidden_command("reg")
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
    let output = crate::util::hidden_command("reg")
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

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    fn parses_default_interface_from_route_output() {
        let text =
            "   route to: default\ndestination: default\n  interface: en0\n      flags: <UP>";
        assert_eq!(parse_default_interface(text).as_deref(), Some("en0"));
    }

    #[test]
    fn maps_device_to_enabled_or_disabled_service() {
        let order = "An asterisk (*) denotes that a network service is disabled.\n\
(1) USB 10/100/1000 LAN\n\
(Hardware Port: USB 10/100/1000 LAN, Device: en7)\n\n\
(*) Wi-Fi\n\
(Hardware Port: Wi-Fi, Device: en0)\n";
        assert_eq!(
            parse_service_for_device(order, "en0").as_deref(),
            Some("Wi-Fi")
        );
        assert_eq!(
            parse_service_for_device(order, "en7").as_deref(),
            Some("USB 10/100/1000 LAN")
        );
        assert!(parse_service_for_device(order, "en9").is_none());
    }

    #[test]
    fn parses_proxy_state_including_disabled_server() {
        let text = "Enabled: No\nServer: 127.0.0.1\nPort: 7897\nAuthenticated Proxy Enabled: 0\n";
        let state = parse_proxy_state(text);
        assert!(!state.enabled);
        assert_eq!(state.server, "127.0.0.1");
        assert_eq!(state.port, 7897);
    }

    #[test]
    fn restore_command_keeps_original_proxy_or_turns_it_off() {
        let enabled = MacProxyState {
            enabled: true,
            server: "10.0.0.1".into(),
            port: 8080,
        };
        assert_eq!(
            restore_proxy_command("-setwebproxy", "-setwebproxystate", "Wi-Fi", &enabled),
            vec!["-setwebproxy", "Wi-Fi", "10.0.0.1", "8080"]
        );
        let disabled = MacProxyState::default();
        assert_eq!(
            restore_proxy_command("-setwebproxy", "-setwebproxystate", "Wi-Fi", &disabled),
            vec!["-setwebproxystate", "Wi-Fi", "off"]
        );
    }

    #[test]
    fn shell_quote_escapes_service_names() {
        assert_eq!(shell_quote("Wi-Fi"), "'Wi-Fi'");
        assert_eq!(shell_quote("Bob's LAN"), "'Bob'\\''s LAN'");
        assert_eq!(applescript_escape("a\"b\\c"), "a\\\"b\\\\c");
    }
}
