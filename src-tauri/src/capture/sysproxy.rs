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
    /// 接管方式：`proxy`（全局 HTTP/HTTPS 代理）/ `pac`（按域名分流）。
    /// 老备份文件没有这个字段，按 `proxy` 处理即可保持兼容。
    #[serde(default)]
    mode: ProxyMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    proxy_enable: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    proxy_server: Option<String>,
    #[cfg(target_os = "macos")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    macos: Option<MacProxyBackup>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum ProxyMode {
    #[default]
    Proxy,
    Pac,
}

#[cfg(target_os = "macos")]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MacProxyBackup {
    service: String,
    web: MacProxyState,
    secure: MacProxyState,
    /// PAC 模式下的原状态；`proxy` 模式为 None。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pac: Option<MacPacState>,
}

/// 系统「自动代理配置」的原始状态，用于 PAC 模式退出时还原。
#[cfg(target_os = "macos")]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MacPacState {
    url: String,
    enabled: bool,
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

/// 当前系统已启用的 HTTP 代理（如 Clash / Surge），形如 `http://127.0.0.1:7897`。
///
/// 自动捕获要先看这个：用户已经在用别的代理分流时，我们再接管会抢走它的规则，
/// 反而让本该走代理的接口变成直连。
#[cfg(target_os = "macos")]
pub fn current_http_proxy() -> Option<String> {
    let service = active_service().ok()?;
    for secure in [true, false] {
        if let Ok(state) = read_proxy_state(&service, secure) {
            if state.enabled && !state.server.is_empty() && state.port > 0 {
                return Some(format!("http://{}:{}", state.server, state.port));
            }
        }
    }
    None
}

#[cfg(windows)]
pub fn current_http_proxy() -> Option<String> {
    let enabled = read_value("ProxyEnable")?;
    if enabled.trim() != "0x1" && enabled.trim() != "1" {
        return None;
    }
    let server = read_value("ProxyServer")?;
    let server = server.trim();
    if server.is_empty() {
        return None;
    }
    Some(if server.starts_with("http") {
        server.to_string()
    } else {
        format!("http://{server}")
    })
}

#[cfg(all(not(windows), not(target_os = "macos")))]
pub fn current_http_proxy() -> Option<String> {
    None
}

/// 开启系统代理，指向 `127.0.0.1:port`。
#[cfg(windows)]
pub fn enable(data_dir: &Path, port: u16) -> Result<(), String> {
    let mut backup = read_backup(data_dir);
    if !backup.active {
        backup = ProxyBackup {
            active: true,
            mode: ProxyMode::Proxy,
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

/// 以 PAC 方式接管：只让 PAC 脚本命中的域名走 `127.0.0.1:<port>`，其余直连。
#[cfg(target_os = "macos")]
pub fn enable_pac(data_dir: &Path, pac_url: &str) -> Result<(), String> {
    enable_pac_macos(data_dir, pac_url, true)
}

/// Windows 的系统级 PAC 需要写 `AutoConfigURL`，这里先让上层退回全局代理。
#[cfg(windows)]
pub fn enable_pac(_data_dir: &Path, _pac_url: &str) -> Result<(), String> {
    Err("E_SYSPROXY_PAC: 当前平台暂不支持按域名分流，回退为全局代理".into())
}

#[cfg(all(not(windows), not(target_os = "macos")))]
pub fn enable_pac(_data_dir: &Path, _pac_url: &str) -> Result<(), String> {
    Err("E_SYSPROXY_PAC: 当前平台暂不支持按域名分流，回退为全局代理".into())
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
        let commands = if backup.mode == ProxyMode::Pac {
            restore_pac_commands(&mac.service, mac.pac.as_ref())
        } else {
            vec![
                restore_proxy_command("-setwebproxy", "-setwebproxystate", &mac.service, &mac.web),
                restore_proxy_command(
                    "-setsecurewebproxy",
                    "-setsecurewebproxystate",
                    &mac.service,
                    &mac.secure,
                ),
            ]
        };
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
    if backup.active && backup.mode != ProxyMode::Proxy {
        // 上次是 PAC 模式，先还原再重备，避免两套系统设置互相覆盖
        disable_with_prompt(data_dir, prompt)?;
        backup = read_backup(data_dir);
    }
    if !backup.active {
        let service = active_service()?;
        let web = read_proxy_state(&service, false)?;
        let secure = read_proxy_state(&service, true)?;
        backup = ProxyBackup {
            active: true,
            mode: ProxyMode::Proxy,
            macos: Some(MacProxyBackup {
                service,
                web,
                secure,
                pac: None,
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

/// PAC 接管：只改「自动代理配置」，完全不碰用户原有的 HTTP/HTTPS 代理设置。
#[cfg(target_os = "macos")]
fn enable_pac_macos(data_dir: &Path, pac_url: &str, prompt: bool) -> Result<(), String> {
    let mut backup = read_backup(data_dir);
    if backup.active && backup.mode != ProxyMode::Pac {
        disable_with_prompt(data_dir, prompt)?;
        backup = read_backup(data_dir);
    }
    if !backup.active {
        let service = active_service()?;
        backup = ProxyBackup {
            active: true,
            mode: ProxyMode::Pac,
            macos: Some(MacProxyBackup {
                web: read_proxy_state(&service, false)?,
                secure: read_proxy_state(&service, true)?,
                pac: Some(read_pac_state(&service)?),
                service,
            }),
            ..Default::default()
        };
        write_backup(data_dir, &backup)?;
    }
    let mac = backup.macos.as_ref().ok_or_else(|| {
        "E_SYSPROXY: 代理备份缺少 macOS 网络服务信息，请删除备份文件后重试".to_string()
    })?;
    let commands = vec![
        vec![
            "-setautoproxyurl".into(),
            mac.service.clone(),
            pac_url.to_string(),
        ],
        vec![
            "-setautoproxystate".into(),
            mac.service.clone(),
            "on".into(),
        ],
    ];
    run_macos_commands(&commands, prompt)?;
    log::info!("已接管系统代理（PAC 分流）：{} → {pac_url}", mac.service);
    Ok(())
}

#[cfg(target_os = "macos")]
fn read_pac_state(service: &str) -> Result<MacPacState, String> {
    let text = networksetup_output(&["-getautoproxyurl", service])?;
    Ok(parse_pac_state(&text))
}

/// `-getautoproxyurl` 输出两行：`URL: ...` 与 `Enabled: Yes/No`；未配置时 URL 为 `(null)`。
#[cfg(target_os = "macos")]
fn parse_pac_state(text: &str) -> MacPacState {
    let mut state = MacPacState::default();
    for line in text.lines().map(str::trim) {
        if let Some(rest) = line.strip_prefix("URL:") {
            let url = rest.trim();
            if !url.is_empty() && url != "(null)" {
                state.url = url.to_string();
            }
        } else if let Some(rest) = line.strip_prefix("Enabled:") {
            state.enabled = rest.trim().eq_ignore_ascii_case("yes");
        }
    }
    state
}

#[cfg(target_os = "macos")]
fn restore_pac_commands(service: &str, state: Option<&MacPacState>) -> Vec<Vec<String>> {
    match state {
        // 原来有 PAC 地址：先还原地址，再还原开关；顺序反了会被 setautoproxyurl 重新打开
        Some(saved) if !saved.url.is_empty() => vec![
            vec![
                "-setautoproxyurl".into(),
                service.to_string(),
                saved.url.clone(),
            ],
            vec![
                "-setautoproxystate".into(),
                service.to_string(),
                if saved.enabled { "on" } else { "off" }.into(),
            ],
        ],
        _ => vec![vec![
            "-setautoproxystate".into(),
            service.to_string(),
            "off".into(),
        ]],
    }
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

    #[test]
    fn pac_state_parses_networksetup_output() {
        let text = "URL: http://127.0.0.1:53113/commands/pac\nEnabled: No\n";
        let state = parse_pac_state(text);
        assert_eq!(state.url, "http://127.0.0.1:53113/commands/pac");
        assert!(!state.enabled);

        // 从未配置过时 networksetup 给的是 (null)
        let empty = parse_pac_state("URL: (null)\nEnabled: No\n");
        assert!(empty.url.is_empty());
    }

    #[test]
    fn pac_restore_prefers_saved_url_then_state() {
        let saved = MacPacState {
            url: "http://127.0.0.1:53113/commands/pac".into(),
            enabled: false,
        };
        let commands = restore_pac_commands("Wi-Fi", Some(&saved));
        assert_eq!(commands.len(), 2);
        // 顺序必须是先 URL 后开关：setautoproxyurl 会顺手把开关打开
        assert_eq!(commands[0][0], "-setautoproxyurl");
        assert_eq!(commands[1], vec!["-setautoproxystate", "Wi-Fi", "off"]);

        let untouched = restore_pac_commands("Wi-Fi", None);
        assert_eq!(untouched, vec![vec!["-setautoproxystate", "Wi-Fi", "off"]]);
    }

    /// 真机验证：接管系统 PAC → 校验 networksetup 状态 → 还原。
    /// 会短暂修改系统代理设置，默认 ignore，手动跑：
    /// `cargo test --lib -- --ignored sysproxy::tests::pac_takeover_round_trip`
    #[test]
    #[ignore]
    fn pac_takeover_round_trip() {
        let service = active_service().expect("active service");
        let before = networksetup_output(&["-getautoproxyurl", &service]).expect("read before");
        let dir = std::env::temp_dir().join("astroman-pac-live");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("mk dir");

        enable_pac_macos(&dir, "http://127.0.0.1:45678/proxy.pac", false).expect("enable pac");
        let during = networksetup_output(&["-getautoproxyurl", &service]).expect("read during");
        assert!(during.contains("45678"), "PAC 未写入：{during}");
        assert!(during.contains("Enabled: Yes"), "PAC 未启用：{during}");

        disable_with_prompt(&dir, false).expect("restore");
        let after = networksetup_output(&["-getautoproxyurl", &service]).expect("read after");
        assert_eq!(after.trim(), before.trim(), "PAC 未按原值还原");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
