//! 一键诊断包（A-22）。
//!
//! 用户报障时最缺的是「现场」：版本、档位、库状态、最近日志、抓包 / 文档站状态。
//! 这里把它们打成一个可脱敏的 ZIP，让用户直接发出来。
//!
//! 硬约束：绝不打包凭据。客户端证书口令、AI Key、环境变量密文都不在采集范围内；
//! 代理 URL 里的账号口令与系统用户名做掩码。

use std::io::Write;
use std::path::Path;

use serde::Serialize;
use serde_json::{json, Value};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::state::{AppSettings, AppState};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsResult {
    pub path: String,
    pub bytes: u64,
}

/// 代理 URL 中的口令掩码：`http://user:pass@host` → `http://user:***@host`
pub fn mask_credentials(url: &str) -> String {
    let Some((scheme, rest)) = url.split_once("://") else {
        return url.to_string();
    };
    let Some(at) = rest.find('@') else {
        return url.to_string();
    };
    let (userinfo, tail) = rest.split_at(at);
    match userinfo.split_once(':') {
        Some((user, _)) => format!("{scheme}://{user}:***{tail}"),
        None => format!("{scheme}://***{tail}"),
    }
}

/// 把本机用户名从路径里抹掉：`C:\Users\PC\...` → `C:\Users\<user>\...`
pub fn mask_home(text: &str) -> String {
    let mut out = text.to_string();
    if let Ok(home) = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")) {
        if !home.trim().is_empty() {
            out = out.replace(home.trim(), "<home>");
        }
    }
    for user in [std::env::var("USERNAME").ok(), std::env::var("USER").ok()]
        .into_iter()
        .flatten()
    {
        let user = user.trim();
        if user.is_empty() {
            continue;
        }
        for prefix in ["\\Users\\", "/home/", "/Users/"] {
            out = out.replace(&format!("{prefix}{user}"), &format!("{prefix}<user>"));
        }
    }
    out
}

/// 数据目录风险：同步盘 / 网络共享 / 超长路径 / 剩余空间不足 / FAT32
///
/// 这些场景不会让应用直接启动失败，但会显著拖慢 SQLite 并放大损坏风险，
/// 诊断包里主动列出来，避免排障时来回猜。
pub fn data_dir_risks(path: &Path) -> Vec<String> {
    let mut risks = path_text_risks(&path.to_string_lossy());
    let disks = sysinfo::Disks::new_with_refreshed_list();
    let target = path.to_string_lossy().to_ascii_lowercase();
    for disk in disks.list() {
        let mount = disk.mount_point().to_string_lossy().to_ascii_lowercase();
        if mount.is_empty() || !target.starts_with(&mount) {
            continue;
        }
        if disk.available_space() > 0 && disk.available_space() < 1024 * 1024 * 1024 {
            risks.push(format!(
                "数据盘剩余空间不足 1GB（{} MB），备份与抓包正文可能写入失败",
                disk.available_space() / 1024 / 1024
            ));
        }
        let fs = disk.file_system().to_string_lossy().to_ascii_lowercase();
        if fs.contains("fat") {
            risks.push(format!(
                "数据目录位于 {fs} 分区，单文件上限 4GB，长期抓包 / 备份可能超限"
            ));
        }
        break;
    }
    risks
}

/// 只依赖路径文本的检查（便于单测，不触碰真实磁盘）
fn path_text_risks(text: &str) -> Vec<String> {
    let lower = text.to_ascii_lowercase();
    let mut risks = Vec::new();
    for (keyword, hint) in [
        ("onedrive", "OneDrive 同步目录"),
        ("dropbox", "Dropbox 同步目录"),
        ("google drive", "Google Drive 同步目录"),
        ("icloud", "iCloud 同步目录"),
    ] {
        if lower.contains(keyword) {
            risks.push(format!(
                "数据目录位于{hint}，同步进程会与 SQLite 争抢文件锁，建议移到本地磁盘"
            ));
        }
    }
    if lower.starts_with("\\\\") {
        risks.push("数据目录位于网络共享，SQLite 锁与延迟都不可控，建议移到本地磁盘".to_string());
    }
    if text.chars().count() > 200 {
        risks.push("数据目录路径超过 200 字符，Windows 未开启长路径时会写入失败".to_string());
    }
    risks
}

/// 设置快照：只保留排障需要的开关，不带上任何口令与完整路径
pub fn redact_settings(settings: &AppSettings) -> Value {
    json!({
        "proxyEnabled": settings.proxy_enabled,
        "proxyUrl": mask_credentials(&mask_home(settings.proxy_url.trim())),
        "certConfigured": !settings.cert_path.trim().is_empty(),
        "pythonEnabled": settings.python_enabled,
        "pythonPath": mask_home(settings.python_path.trim()),
        "performanceMode": settings.performance_mode,
        "aiPermissionMode": settings.ai_permission_mode,
    })
}

/// 从运行中的 AppState 采集现场并写出诊断包。
///
/// 目标路径没有扩展名时补 `.zip`，避免用户拿到一个打不开的「文件」。
pub fn export(state: &AppState, path: &Path) -> Result<DiagnosticsResult, String> {
    let target = if path.extension().is_none() {
        path.with_extension("zip")
    } else {
        path.to_path_buf()
    };
    let conn = state.pool.get().map_err(crate::util::err)?;
    let db_version: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap_or(0);
    let journal: String = conn
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .unwrap_or_default();
    let tables: Vec<Value> = crate::db::table_counts(&conn)
        .into_iter()
        .map(|(table, rows)| json!({ "table": table, "rows": rows }))
        .collect();

    // 文档站状态里的 token 是局域网访问令牌，不能进诊断包
    let mut docs = serde_json::to_value(state.docs_site.status()).unwrap_or(Value::Null);
    if let Value::Object(map) = &mut docs {
        map.remove("token");
    }
    // 抓包上游代理可能带账号口令
    let mut capture = serde_json::to_value(state.capture.status()).unwrap_or(Value::Null);
    if let Value::Object(map) = &mut capture {
        if let Some(Value::String(proxy)) = map.get_mut("upstreamProxy") {
            *proxy = mask_credentials(proxy);
        }
    }

    let settings = state.settings();
    let profile = state.profile();
    let policy = state.policy();
    let db_size: u64 = std::fs::metadata(&state.db_path)
        .map(|meta| meta.len())
        .unwrap_or(0);
    let summary = json!({
        "generatedAt": crate::util::now_ms(),
        "app": { "version": env!("CARGO_PKG_VERSION") },
        "os": { "name": std::env::consts::OS, "arch": std::env::consts::ARCH },
        // Windows 上 WebView2 版本决定了很多兼容性问题的排查方向
        "webview": { "version": tauri::webview_version().ok() },
        "dataDir": mask_home(&state.data_dir.display().to_string()),
        "dataDirRisks": data_dir_risks(&state.data_dir),
        "database": {
            "version": db_version,
            "journalMode": journal,
            "path": mask_home(&state.db_path.display().to_string()),
            "sizeBytes": db_size,
            "tables": tables,
        },
        "profile": {
            "requested": settings.performance_mode,
            "tier": profile.tier,
            "limits": profile.limits,
            "totalMemoryBytes": profile.total_memory_bytes,
        },
        "policy": {
            "policy": policy,
            "error": state.policy_error(),
        },
        "capture": capture,
        "docsSite": docs,
    });

    let redacted = redact_settings(&settings);
    let logs = crate::logging::tail(&state.data_dir.join("logs"), 200);
    let bytes = write_archive(&target, &summary, &redacted, &logs)?;
    let display = target.to_string_lossy().to_string();
    if let Err(error) = crate::services::audit::write(
        &conn,
        "diagnostics_export",
        Some(&display),
        "ok",
        Some(&json!({ "bytes": bytes })),
        None,
    ) {
        log::warn!("审计写入失败：{error}");
    }
    Ok(DiagnosticsResult {
        path: display,
        bytes,
    })
}

/// 写诊断 ZIP。失败删除半成品，避免用户拿到打不开的包。
pub fn write_archive(
    path: &Path,
    summary: &Value,
    settings: &Value,
    logs: &str,
) -> Result<u64, String> {
    let result = (|| -> Result<u64, String> {
        let file = std::fs::File::create(path)
            .map_err(|error| format!("E_DIAGNOSTICS: 创建诊断包失败：{error}"))?;
        let mut zip = ZipWriter::new(file);
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

        write_entry(
            &mut zip,
            "README.txt",
            "AstroMan 诊断包\n\n\
             包含：summary.json（版本 / WebView2 版本 / 数据目录风险 / 档位 / 数据库摘要 / 企业策略）、\n\
             settings.json（脱敏设置）、logs/recent.log（最近 200 行日志）。\n\
             不包含：AI Key、客户端证书口令、环境变量密文、请求正文。\n"
                .as_bytes(),
            options,
        )?;
        write_entry(
            &mut zip,
            "summary.json",
            serde_json::to_string_pretty(summary)
                .map_err(|error| format!("序列化诊断摘要失败：{error}"))?
                .as_bytes(),
            options,
        )?;
        write_entry(
            &mut zip,
            "settings.json",
            serde_json::to_string_pretty(settings)
                .map_err(|error| format!("序列化设置快照失败：{error}"))?
                .as_bytes(),
            options,
        )?;
        write_entry(
            &mut zip,
            "logs/recent.log",
            mask_home(&crate::logging::redact(logs)).as_bytes(),
            options,
        )?;
        zip.finish()
            .map_err(|error| format!("E_DIAGNOSTICS: 写入诊断包失败：{error}"))?;
        let bytes = std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);
        Ok(bytes)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(path);
    }
    result
}

fn write_entry(
    zip: &mut ZipWriter<std::fs::File>,
    name: &str,
    content: &[u8],
    options: SimpleFileOptions,
) -> Result<(), String> {
    zip.start_file(name, options)
        .map_err(|error| format!("E_DIAGNOSTICS: 创建条目 {name} 失败：{error}"))?;
    zip.write_all(content)
        .map_err(|error| format!("E_DIAGNOSTICS: 写入条目 {name} 失败：{error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_proxy_credentials() {
        assert_eq!(
            mask_credentials("http://alice:s3cret@127.0.0.1:7890"),
            "http://alice:***@127.0.0.1:7890"
        );
        assert_eq!(
            mask_credentials("socks5h://bob@127.0.0.1:1080"),
            "socks5h://***@127.0.0.1:1080"
        );
        assert_eq!(
            mask_credentials("http://127.0.0.1:7890"),
            "http://127.0.0.1:7890"
        );
    }

    #[test]
    fn flags_sync_network_and_long_paths() {
        let risks = path_text_risks("C:\\Users\\PC\\OneDrive\\AstroMan");
        assert_eq!(risks.len(), 1);
        assert!(risks[0].contains("同步目录"));

        let risks = path_text_risks("\\\\nas\\share\\AstroMan");
        assert_eq!(risks.len(), 1);
        assert!(risks[0].contains("网络共享"));

        let long = format!("C:\\{}\\data", "a".repeat(220));
        let risks = path_text_risks(&long);
        assert!(risks.iter().any(|item| item.contains("200")));

        assert!(path_text_risks("C:\\Users\\PC\\AppData\\AstroMan").is_empty());
    }

    #[test]
    fn redacted_settings_never_include_password() {
        let settings = AppSettings {
            proxy_enabled: true,
            proxy_url: "http://alice:s3cret@proxy.local:8080".to_string(),
            cert_path: "C:/certs/client.pfx".to_string(),
            cert_password: "pfx-pass".to_string(),
            python_path: String::new(),
            python_enabled: false,
            performance_mode: "auto".to_string(),
            ai_permission_mode: "confirm_write".to_string(),
            ..AppSettings::default()
        };
        let value = redact_settings(&settings);
        let text = value.to_string();
        assert!(!text.contains("s3cret"));
        assert!(!text.contains("pfx-pass"));
        assert!(!text.contains("client.pfx"));
        assert_eq!(value["certConfigured"], true);
        assert_eq!(value["aiPermissionMode"], "confirm_write");
    }

    #[test]
    fn archive_contains_summary_settings_and_logs() {
        let path =
            std::env::temp_dir().join(format!("astroman-diagnostics-{}.zip", std::process::id()));
        let summary = json!({ "app": { "version": "1.1.0" } });
        let settings = json!({ "aiPermissionMode": "confirm_write" });
        let bytes = write_archive(&path, &summary, &settings, "启动完成").unwrap();
        assert!(bytes > 0);

        let mut archive = zip::ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap();
        let mut summary_text = String::new();
        std::io::Read::read_to_string(
            &mut archive.by_name("summary.json").unwrap(),
            &mut summary_text,
        )
        .unwrap();
        assert!(summary_text.contains("1.1.0"));
        assert!(archive.by_name("settings.json").is_ok());
        assert!(archive.by_name("logs/recent.log").is_ok());
        let _ = std::fs::remove_file(path);
    }
}
