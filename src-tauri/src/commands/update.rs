use serde::{Deserialize, Serialize};

const RELEASE_API: &str = "https://api.github.com/repos/Wayward-GOU/FbinHttp/releases/latest";
const USER_AGENT: &str = "FbinHttp-Desktop";

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

fn pick_asset(assets: &[GithubAsset]) -> Option<&GithubAsset> {
    let wanted: &[&str] = if cfg!(target_os = "windows") {
        &["setup.exe", ".msi"]
    } else if cfg!(target_os = "macos") {
        &[".dmg"]
    } else {
        &[".appimage", ".deb"]
    };
    for suffix in wanted {
        if let Some(hit) = assets
            .iter()
            .find(|asset| asset.name.to_lowercase().ends_with(suffix))
        {
            return Some(hit);
        }
    }
    None
}

#[tauri::command]
pub async fn update_check() -> Result<UpdateInfo, String> {
    let client = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| format!("初始化网络请求失败：{e}"))?;

    let response = client
        .get(RELEASE_API)
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| format!("无法连接 GitHub 检查更新：{e}"))?;

    if !response.status().is_success() {
        return Err(format!("检查更新失败，GitHub 返回 {}", response.status()));
    }

    let release: GithubRelease = response
        .json()
        .await
        .map_err(|e| format!("解析更新信息失败：{e}"))?;

    let current = env!("CARGO_PKG_VERSION").to_string();
    let latest = release.tag_name.trim_start_matches('v').to_string();
    let asset = pick_asset(&release.assets);

    Ok(UpdateInfo {
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
    })
}

fn safe_file_name(name: &str) -> String {
    let base = name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("fbinhttp-update")
        .trim();
    let cleaned: String = base
        .chars()
        .map(|c| match c {
            '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            other => other,
        })
        .collect();
    if cleaned.is_empty() {
        "fbinhttp-update".to_string()
    } else {
        cleaned
    }
}

/// 下载新版本安装包到系统临时目录，返回本地文件路径。
/// urls 按优先级依次尝试：先 API 资源地址（api.github.com），再浏览器下载地址（github.com），
/// 后者的域名在部分网络环境下无法直连。
#[tauri::command]
pub async fn update_download(urls: Vec<String>, file_name: String) -> Result<String, String> {
    let candidates: Vec<&str> = urls
        .iter()
        .map(|url| url.trim())
        .filter(|url| url.starts_with("https://"))
        .collect();
    if candidates.is_empty() {
        return Err("下载地址无效".to_string());
    }

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

        let dir = std::env::temp_dir().join("fbinhttp-update");
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
    #[cfg(windows)]
    {
        // 链接直接交给默认浏览器，本地文件交给安装程序
        if target.starts_with("http://") || target.starts_with("https://") {
            std::process::Command::new("cmd")
                .args(["/C", "start", "", &target])
                .spawn()
                .map_err(|e| format!("打开链接失败：{e}"))?;
        } else {
            std::process::Command::new(&target)
                .spawn()
                .map_err(|e| format!("启动安装程序失败：{e}"))?;
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let opener = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
        std::process::Command::new(opener)
            .arg(&target)
            .spawn()
            .map_err(|e| format!("打开失败：{e}"))?;
        Ok(())
    }
}
