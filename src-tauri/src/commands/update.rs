use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::State;

use crate::state::AppState;

const RELEASE_API: &str = "https://api.github.com/repos/Wayward-GOU/AstroMan/releases/latest";
const RELEASES_API: &str = "https://api.github.com/repos/Wayward-GOU/AstroMan/releases?per_page=20";
const USER_AGENT: &str = "AstroMan-Desktop";

/// `open_external` 只接受 https / http 链接或本机真实文件。
/// 拒绝其他 scheme 与 cmd 元字符，避免把参数拼进 `cmd /C start` 时被二次解释。
fn validate_external_target(target: &str) -> Result<(), String> {
    let lower = target.to_ascii_lowercase();
    let is_url = lower.starts_with("https://") || lower.starts_with("http://");
    if !is_url && lower.contains("://") {
        return Err(format!("E_PATH: 不允许打开该协议（{target}）"));
    }
    if target
        .chars()
        .any(|c| matches!(c, '&' | '|' | '^' | '<' | '>' | '"' | '\r' | '\n'))
    {
        return Err("E_PATH: 路径或链接包含不支持的字符".to_string());
    }
    if !is_url {
        let path = crate::services::paths::prepare_read(target)?;
        if path.is_dir() {
            return Err("E_PATH: 目标是目录".to_string());
        }
    }
    Ok(())
}

#[derive(Deserialize)]
struct GithubRelease {
    tag_name: String,
    #[serde(default)]
    html_url: String,
    #[serde(default)]
    body: String,
    #[serde(default)]
    published_at: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<GithubAsset>,
}

#[derive(Deserialize)]
struct GithubAsset {
    name: String,
    #[serde(default)]
    browser_download_url: String,
    /// API 资源地址，走 api.github.com，可在 github.com 直连受限时下载
    #[serde(default)]
    url: String,
    #[serde(default)]
    size: u64,
    /// GitHub 提供的 `sha256:<hex>` 摘要，用于下载后校验
    #[serde(default)]
    digest: String,
}

/// 企业内网更新清单（`ASTROMAN_UPDATE_MANIFEST` 指向的 https JSON）
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateManifest {
    version: String,
    #[serde(default)]
    channel: String,
    #[serde(default = "full_rollout")]
    rollout_percent: u32,
    #[serde(default)]
    notes: String,
    #[serde(default)]
    published_at: String,
    #[serde(default)]
    release_url: String,
    #[serde(default)]
    assets: Vec<ManifestAsset>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ManifestAsset {
    name: String,
    url: String,
    #[serde(default)]
    sha256: String,
    #[serde(default)]
    size: u64,
}

fn full_rollout() -> u32 {
    100
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    current_version: String,
    latest_version: String,
    has_update: bool,
    release_url: String,
    notes: String,
    published_at: String,
    asset_name: Option<String>,
    asset_url: Option<String>,
    asset_api_url: Option<String>,
    asset_size: Option<u64>,
    /// 安装包 SHA-256（GitHub digest 或企业清单），下载后强制校验
    asset_sha256: Option<String>,
    /// stable / beta / 企业清单自定义
    channel: String,
    /// 灰度比例；0 表示当前设备不在灰度范围内
    rollout_percent: u32,
    /// 当前设备命中的灰度桶（0-99），用于排障
    rollout_bucket: u32,
}

fn version_parts(version: &str) -> Vec<u64> {
    version
        .trim()
        .trim_start_matches('v')
        .split(|c: char| !c.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .filter_map(|part| part.parse::<u64>().ok())
        .collect()
}

fn is_newer(latest: &str, current: &str) -> bool {
    let left = version_parts(latest);
    let right = version_parts(current);
    let len = left.len().max(right.len());
    for index in 0..len {
        let a = *left.get(index).unwrap_or(&0);
        let b = *right.get(index).unwrap_or(&0);
        if a != b {
            return a > b;
        }
    }
    false
}

fn wanted_asset_suffixes() -> &'static [&'static str] {
    if cfg!(target_os = "windows") {
        if std::env::consts::ARCH == "x86" {
            &["_x86-setup.exe", "setup.exe"]
        } else {
            &["_x64-setup.exe", "setup.exe"]
        }
    } else if cfg!(target_os = "macos") {
        &[".dmg"]
    } else {
        &[".appimage", ".deb"]
    }
}

fn pick_asset(assets: &[GithubAsset]) -> Option<&GithubAsset> {
    for suffix in wanted_asset_suffixes() {
        if let Some(hit) = assets
            .iter()
            .find(|asset| asset.name.to_lowercase().ends_with(suffix))
        {
            return Some(hit);
        }
    }
    None
}

fn pick_manifest_asset(assets: &[ManifestAsset]) -> Option<&ManifestAsset> {
    for suffix in wanted_asset_suffixes() {
        if let Some(hit) = assets
            .iter()
            .find(|asset| asset.name.to_lowercase().ends_with(suffix))
        {
            return Some(hit);
        }
    }
    None
}

/// 去掉 GitHub digest 的 `sha256:` 前缀，统一成小写十六进制
fn normalize_sha256(value: &str) -> Option<String> {
    let text = value.trim();
    if text.is_empty() {
        return None;
    }
    let hex = text
        .strip_prefix("sha256:")
        .or_else(|| text.strip_prefix("SHA256:"))
        .unwrap_or(text);
    let hex = hex.trim().to_ascii_lowercase();
    (hex.len() == 64 && hex.chars().all(|c| c.is_ascii_hexdigit())).then_some(hex)
}

fn digest_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

/// 每台机器一个稳定安装 ID，用于灰度分桶；写不进数据目录时退回进程内随机值
fn install_id(data_dir: &std::path::Path) -> String {
    let path = data_dir.join("install-id");
    if let Ok(text) = std::fs::read_to_string(&path) {
        let text = text.trim();
        if !text.is_empty() {
            return text.to_string();
        }
    }
    let seed = format!(
        "{}:{}:{}:{}",
        crate::util::now_ms(),
        std::process::id(),
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    let id = digest_hex(seed.as_bytes());
    let id = id[..32].to_string();
    let _ = std::fs::write(&path, &id);
    id
}

/// 0-99 的稳定灰度桶：同一台机器每次算出来都一样
fn rollout_bucket(id: &str) -> u32 {
    let digest = digest_hex(id.as_bytes());
    let head = &digest[..8];
    u32::from_str_radix(head, 16).unwrap_or(0) % 100
}

fn update_channel() -> String {
    std::env::var("ASTROMAN_UPDATE_CHANNEL")
        .map(|value| value.trim().to_ascii_lowercase())
        .ok()
        .filter(|value| value == "beta")
        .unwrap_or_else(|| "stable".to_string())
}

#[tauri::command]
pub async fn update_check(state: State<'_, AppState>) -> Result<UpdateInfo, String> {
    let client = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| format!("初始化网络请求失败：{e}"))?;

    let current = env!("CARGO_PKG_VERSION").to_string();
    let channel = update_channel();
    let bucket = rollout_bucket(&install_id(&state.data_dir));

    // 企业内网：管理员用清单文件控制版本、灰度比例与 SHA-256
    if let Ok(manifest_url) = std::env::var("ASTROMAN_UPDATE_MANIFEST") {
        let manifest_url = manifest_url.trim();
        if manifest_url.starts_with("https://") {
            return check_manifest(&client, manifest_url, current, channel, bucket).await;
        }
    }

    let url = if channel == "beta" {
        RELEASES_API
    } else {
        RELEASE_API
    };
    let response = client
        .get(url)
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| format!("无法连接 GitHub 检查更新：{e}"))?;

    if !response.status().is_success() {
        return Err(format!("检查更新失败，GitHub 返回 {}", response.status()));
    }

    let release = if channel == "beta" {
        let mut releases: Vec<GithubRelease> = response
            .json()
            .await
            .map_err(|e| format!("解析更新信息失败：{e}"))?;
        // Beta 频道优先取预发布版本，没有再退回普通发布
        let index = releases
            .iter()
            .position(|item| !item.draft && item.prerelease)
            .or_else(|| releases.iter().position(|item| !item.draft));
        match index {
            Some(index) => releases.swap_remove(index),
            None => return Err("没有可用的 Beta 版本".to_string()),
        }
    } else {
        response
            .json()
            .await
            .map_err(|e| format!("解析更新信息失败：{e}"))?
    };

    let latest = release.tag_name.trim_start_matches('v').to_string();
    let asset = pick_asset(&release.assets);

    Ok(UpdateInfo {
        // 只允许升级，不允许被远端「降级」到旧版本
        has_update: is_newer(&latest, &current),
        current_version: current,
        latest_version: latest,
        release_url: release.html_url,
        notes: release.body,
        published_at: release.published_at,
        asset_name: asset.map(|item| item.name.clone()),
        asset_url: asset.map(|item| item.browser_download_url.clone()),
        asset_api_url: asset
            .map(|item| item.url.clone())
            .filter(|url| url.starts_with("https://")),
        asset_size: asset.map(|item| item.size),
        asset_sha256: asset.and_then(|item| normalize_sha256(&item.digest)),
        channel,
        rollout_percent: 100,
        rollout_bucket: bucket,
    })
}

async fn check_manifest(
    client: &reqwest::Client,
    url: &str,
    current: String,
    fallback_channel: String,
    bucket: u32,
) -> Result<UpdateInfo, String> {
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("无法连接更新清单：{e}"))?;
    if !response.status().is_success() {
        return Err(format!("更新清单返回 {}", response.status()));
    }
    let manifest: UpdateManifest = response
        .json()
        .await
        .map_err(|e| format!("更新清单格式错误：{e}"))?;
    let latest = manifest.version.trim_start_matches('v').to_string();
    let rollout = manifest.rollout_percent.min(100);
    let in_rollout = rollout >= 100 || bucket < rollout;
    let asset = pick_manifest_asset(&manifest.assets);
    // 清单模式强制要求摘要，避免内网镜像被替换后静默安装
    if manifest
        .assets
        .iter()
        .any(|item| item.sha256.trim().is_empty())
    {
        return Err("更新清单缺少 sha256，拒绝自动更新".to_string());
    }
    Ok(UpdateInfo {
        has_update: in_rollout && is_newer(&latest, &current),
        current_version: current,
        latest_version: latest,
        release_url: manifest.release_url,
        notes: manifest.notes,
        published_at: manifest.published_at,
        asset_name: asset.map(|item| item.name.clone()),
        asset_url: asset
            .map(|item| item.url.clone())
            .filter(|url| url.starts_with("https://") || url.starts_with("http://")),
        asset_api_url: None,
        asset_size: asset.map(|item| item.size),
        asset_sha256: asset.and_then(|item| normalize_sha256(&item.sha256)),
        channel: if manifest.channel.trim().is_empty() {
            fallback_channel
        } else {
            manifest.channel.trim().to_ascii_lowercase()
        },
        rollout_percent: if in_rollout { rollout } else { 0 },
        rollout_bucket: bucket,
    })
}

fn safe_file_name(name: &str) -> String {
    let base = name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("astroman-update")
        .trim();
    let cleaned: String = base
        .chars()
        .map(|c| match c {
            '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            other => other,
        })
        .collect();
    if cleaned.is_empty() {
        "astroman-update".to_string()
    } else {
        cleaned
    }
}

/// 下载新版本安装包到系统临时目录，返回本地文件路径。
/// urls 按优先级依次尝试：先 API 资源地址（api.github.com），再浏览器下载地址（github.com），
/// 后者的域名在部分网络环境下无法直连。sha256 非空时必须校验通过才落盘。
#[tauri::command]
pub async fn update_download(
    urls: Vec<String>,
    file_name: String,
    sha256: Option<String>,
) -> Result<String, String> {
    let candidates: Vec<&str> = urls
        .iter()
        .map(|url| url.trim())
        .filter(|url| url.starts_with("https://") || url.starts_with("http://"))
        .collect();
    if candidates.is_empty() {
        return Err("下载地址无效".to_string());
    }
    let expected = sha256.as_deref().and_then(normalize_sha256);

    let client = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(std::time::Duration::from_secs(600))
        .build()
        .map_err(|e| format!("初始化网络请求失败：{e}"))?;

    let mut last_error = String::new();
    for url in candidates {
        let response = match client
            .get(url)
            .header(reqwest::header::ACCEPT, "application/octet-stream")
            .send()
            .await
        {
            Ok(response) => response,
            Err(error) => {
                last_error = format!("{error}");
                continue;
            }
        };
        if !response.status().is_success() {
            last_error = format!("服务器返回 {}", response.status());
            continue;
        }
        let bytes = match response.bytes().await {
            Ok(bytes) => bytes,
            Err(error) => {
                last_error = format!("读取安装包失败：{error}");
                continue;
            }
        };
        if bytes.is_empty() {
            last_error = "下载内容为空".to_string();
            continue;
        }
        if let Some(expected) = &expected {
            let actual = digest_hex(&bytes);
            if &actual != expected {
                last_error = format!("安装包校验失败：期望 {expected}，实际 {actual}");
                continue;
            }
        }

        let dir = std::env::temp_dir().join("astroman-update");
        std::fs::create_dir_all(&dir).map_err(|e| format!("创建临时目录失败：{e}"))?;
        let target = dir.join(safe_file_name(&file_name));
        std::fs::write(&target, &bytes).map_err(|e| format!("保存安装包失败：{e}"))?;
        return Ok(target.to_string_lossy().to_string());
    }

    Err(format!(
        "下载安装包失败：{last_error}。当前网络可能无法直连 GitHub 下载域名，可在浏览器中打开发布页手动下载"
    ))
}

/// 打开已下载的安装包或系统允许的外部链接
#[tauri::command]
pub fn open_external(target: String) -> Result<(), String> {
    let target = target.trim().to_string();
    if target.is_empty() {
        return Err("目标地址为空".to_string());
    }
    validate_external_target(&target)?;
    #[cfg(windows)]
    {
        // 链接直接交给默认浏览器，本地文件交给安装程序
        if target.starts_with("http://") || target.starts_with("https://") {
            std::process::Command::new("cmd")
                .args(["/C", "start", "", &target])
                .spawn()
                .map_err(|e| format!("打开链接失败：{e}"))?;
        } else if std::path::Path::new(&target).is_file()
            && !target.to_ascii_lowercase().ends_with(".exe")
        {
            // 报告 HTML / PDF 等普通文件交给系统默认程序
            std::process::Command::new("cmd")
                .args(["/C", "start", "", &target])
                .spawn()
                .map_err(|e| format!("打开文件失败：{e}"))?;
        } else {
            std::process::Command::new(&target)
                .spawn()
                .map_err(|e| format!("启动安装程序失败：{e}"))?;
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let opener = if cfg!(target_os = "macos") {
            "open"
        } else {
            "xdg-open"
        };
        std::process::Command::new(opener)
            .arg(&target)
            .spawn()
            .map_err(|e| format!("打开失败：{e}"))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_comparison_rejects_downgrade() {
        assert!(is_newer("1.2.0", "1.1.9"));
        assert!(!is_newer("1.1.0", "1.1.0"));
        assert!(!is_newer("1.0.9", "1.1.0"));
        assert!(is_newer("v2.0.0", "1.9.9"));
    }

    #[test]
    fn normalizes_github_digest() {
        let hex = "a".repeat(64);
        assert_eq!(
            normalize_sha256(&format!("sha256:{hex}")),
            Some(hex.clone())
        );
        assert_eq!(normalize_sha256(&hex.to_ascii_uppercase()), Some(hex));
        assert_eq!(normalize_sha256(""), None);
        assert_eq!(normalize_sha256("sha256:xyz"), None);
    }

    #[test]
    fn rollout_bucket_is_stable_and_bounded() {
        let first = rollout_bucket("install-abc");
        let second = rollout_bucket("install-abc");
        assert_eq!(first, second);
        assert!(first < 100);
        assert_ne!(rollout_bucket("install-abc"), 100);
    }

    #[test]
    fn install_id_is_persisted_between_calls() {
        let dir = crate::util::test_dir("update-install-id");
        let first = install_id(&dir);
        let second = install_id(&dir);
        assert_eq!(first, second);
        assert!(!first.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
