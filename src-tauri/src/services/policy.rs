//! 企业策略文件 `policy.json`（W4-2 / §12.4）
//!
//! 策略由企业管理员随安装包 / 组策略下发，放在数据目录根下，用户界面只读展示。
//! 设计口径：
//! - 缺省文件 = 不限制（个人用户无感）；
//! - 字段全部可选，未知字段忽略，老版本读到新字段不会拒绝启动；
//! - 文件格式错误时不阻断启动，保留错误信息供设置页 / 诊断包提示，避免
//!   管理员写错一个逗号就让整台机器打不开；
//! - 策略只做“能力闸门”，不改变已有数据的可读性，关闭 AI / LAN 服务后
//!   历史记录仍然可以查看和导出。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// 数据目录下的策略文件名
pub const FILE_NAME: &str = "policy.json";
/// `retentionDays` 允许的最大值，避免管理员误填超大数字
const MAX_RETENTION_DAYS: u32 = 3650;

/// 企业策略（字段与 `policy.json` 一一对应）
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Policy {
    /// 禁用 AI 助手（含会话、模型列表、连通性测试、页面分析）
    pub disable_ai: bool,
    /// 只允许本地模型：拒绝非回环地址的 AI 服务（AI-13）
    pub disable_cloud_ai: bool,
    /// 禁止 Mock / 文档站监听非回环地址；本机 127.0.0.1 仍然可用
    pub disable_lan_services: bool,
    /// 禁用遥测 / 更新检查等对外上报（当前版本没有遥测，保留给后续）
    pub disable_telemetry: bool,
    /// 强制异地备份加密：上传必须带口令（实际上传链路本来就是加密的）
    pub force_encrypt_backup: bool,
    /// 允许导入的格式；空数组表示不限制。支持 curl / postman / insomnia / openapi / har / bruno
    pub allowed_import_formats: Vec<String>,
    /// 请求历史保留天数；0 表示只按条数上限保留
    pub retention_days: u32,
}

/// 加载结果：即使解析失败也返回一份可用策略，`error` 供界面提示
#[derive(Debug, Clone)]
pub struct LoadedPolicy {
    pub policy: Policy,
    pub error: Option<String>,
}

impl LoadedPolicy {
    fn default_with_error(error: Option<String>) -> Self {
        Self {
            policy: Policy::default(),
            error,
        }
    }
}

/// 策略文件位置（数据目录根下，随自定义数据目录一起移动）
pub fn policy_path(data_dir: &Path) -> PathBuf {
    data_dir.join(FILE_NAME)
}

/// 读取策略；文件不存在时返回默认策略且不报错
pub fn load(data_dir: &Path) -> LoadedPolicy {
    let path = policy_path(data_dir);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return LoadedPolicy::default_with_error(None)
        }
        Err(error) => {
            let message = format!("读取策略文件失败（{}）：{error}", path.display());
            log::warn!("{message}");
            return LoadedPolicy::default_with_error(Some(message));
        }
    };
    match serde_json::from_str::<Policy>(&text) {
        Ok(policy) => LoadedPolicy {
            policy: policy.normalized(),
            error: None,
        },
        Err(error) => {
            let message = format!(
                "策略文件格式错误（{}），已按默认策略启动：{error}",
                path.display()
            );
            log::error!("{message}");
            LoadedPolicy::default_with_error(Some(message))
        }
    }
}

impl Policy {
    /// 归一化：格式名去空格 / 转小写 / 去重，保留天数限幅
    pub fn normalized(&self) -> Self {
        let mut formats = Vec::new();
        for item in &self.allowed_import_formats {
            let name = item.trim().to_ascii_lowercase();
            if !name.is_empty() && !formats.contains(&name) {
                formats.push(name);
            }
        }
        Self {
            disable_ai: self.disable_ai,
            disable_cloud_ai: self.disable_cloud_ai,
            disable_lan_services: self.disable_lan_services,
            disable_telemetry: self.disable_telemetry,
            force_encrypt_backup: self.force_encrypt_backup,
            allowed_import_formats: formats,
            retention_days: self.retention_days.min(MAX_RETENTION_DAYS),
        }
    }

    /// 请求历史保留天数；0 表示不按时间清理
    pub fn retention_days(&self) -> Option<i64> {
        (self.retention_days > 0).then_some(self.retention_days as i64)
    }

    /// AI 能力闸门
    pub fn ensure_ai(&self) -> Result<(), String> {
        if self.disable_ai {
            return Err("E_POLICY: 企业策略已禁用 AI 助手（policy.json: disableAi）".to_string());
        }
        Ok(())
    }

    /// 云端 AI 闸门：`disableCloudAi` 打开时只允许回环地址（本地 Ollama 等）
    pub fn ensure_cloud_ai(&self, base_url: &str) -> Result<(), String> {
        if !self.disable_cloud_ai {
            return Ok(());
        }
        let host = url_host(base_url);
        if is_loopback_host(&host) {
            return Ok(());
        }
        Err(format!(
            "E_POLICY: 企业策略仅允许本地 AI 模型（policy.json: disableCloudAi），当前服务地址为 {host}"
        ))
    }

    /// LAN 服务闸门：禁用时只允许回环地址
    pub fn ensure_lan(&self, host: Option<&str>) -> Result<(), String> {
        if !self.disable_lan_services || !is_lan_host(host) {
            return Ok(());
        }
        Err(
            "E_POLICY: 企业策略已禁用局域网访问（policy.json: disableLanServices），请把监听地址改为 127.0.0.1"
                .to_string(),
        )
    }

    /// 导入格式闸门；空列表表示不限制
    pub fn ensure_import(&self, format: &str) -> Result<(), String> {
        if self.allowed_import_formats.is_empty() {
            return Ok(());
        }
        let name = format.trim().to_ascii_lowercase();
        if self.allowed_import_formats.iter().any(|item| item == &name) {
            return Ok(());
        }
        Err(format!(
            "E_POLICY: 企业策略不允许导入 {name} 格式（允许：{}）",
            self.allowed_import_formats.join(", ")
        ))
    }

    /// 异地备份是否必须加密
    pub fn backup_must_encrypt(&self) -> bool {
        self.force_encrypt_backup
    }
}

/// 判断监听地址是否会暴露到局域网
fn is_lan_host(host: Option<&str>) -> bool {
    let Some(host) = host else {
        return false;
    };
    let host = host.trim().to_ascii_lowercase();
    if host.is_empty() {
        return false;
    }
    !matches!(host.as_str(), "127.0.0.1" | "localhost" | "::1" | "[::1]")
}

/// 从 base_url 提取主机名：去掉 scheme、用户信息、端口与路径
fn url_host(base_url: &str) -> String {
    let rest = base_url.trim();
    let rest = match rest.split_once("://") {
        Some((_, tail)) => tail,
        None => rest,
    };
    let authority = rest.split('/').next().unwrap_or("");
    let authority = authority.rsplit('@').next().unwrap_or(authority);
    let host = if authority.starts_with('[') {
        authority
            .trim_start_matches('[')
            .split(']')
            .next()
            .unwrap_or("")
    } else {
        authority.split(':').next().unwrap_or("")
    };
    host.trim().to_ascii_lowercase()
}

/// 回环地址（含 localhost 与 IPv6 环回）
fn is_loopback_host(host: &str) -> bool {
    matches!(host, "localhost" | "::1" | "0:0:0:0:0:0:0:1") || host.starts_with("127.")
}

/// 从导入文件中识别格式名，供 `allowedImportFormats` 判断
pub fn import_format_from_json(root: &serde_json::Value) -> &'static str {
    if root
        .get("log")
        .and_then(|log| log.get("entries"))
        .and_then(|entries| entries.as_array())
        .is_some()
    {
        return "har";
    }
    if root.get("_type").and_then(|value| value.as_str()) == Some("export") {
        return "insomnia";
    }
    if root.get("info").is_some()
        && root
            .get("item")
            .and_then(|value| value.as_array())
            .is_some()
    {
        return "postman";
    }
    if root.get("openapi").is_some() || root.get("swagger").is_some() {
        return "openapi";
    }
    if root.get("requests").is_some() {
        return "postman-v1";
    }
    "unknown"
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::test_dir;

    #[test]
    fn missing_file_uses_permissive_defaults() {
        let dir = test_dir("policy-missing");
        let loaded = load(&dir);
        assert!(loaded.error.is_none());
        assert!(!loaded.policy.disable_ai);
        assert!(loaded.policy.allowed_import_formats.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn parses_and_normalizes_existing_file() {
        let dir = test_dir("policy-parse");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(FILE_NAME),
            r#"{
                "disableAi": true,
                "disableLanServices": true,
                "allowedImportFormats": [" OpenAPI ", "har", "openapi"],
                "retentionDays": 30
            }"#,
        )
        .unwrap();
        let loaded = load(&dir);
        assert!(loaded.error.is_none());
        assert!(loaded.policy.disable_ai);
        assert!(loaded.policy.disable_lan_services);
        assert_eq!(
            loaded.policy.allowed_import_formats,
            vec!["openapi".to_string(), "har".to_string()]
        );
        assert_eq!(loaded.policy.retention_days(), Some(30));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn malformed_file_falls_back_with_error() {
        let dir = test_dir("policy-broken");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(FILE_NAME), "{ not json").unwrap();
        let loaded = load(&dir);
        assert!(loaded.error.is_some());
        assert!(!loaded.policy.disable_ai);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn gates_ai_lan_and_import_formats() {
        let policy = Policy {
            disable_ai: true,
            disable_lan_services: true,
            allowed_import_formats: vec!["openapi".to_string()],
            ..Policy::default()
        };
        assert!(policy.ensure_ai().unwrap_err().starts_with("E_POLICY:"));
        assert!(policy.ensure_lan(None).is_ok());
        assert!(policy.ensure_lan(Some("127.0.0.1")).is_ok());
        assert!(policy.ensure_lan(Some("localhost")).is_ok());
        assert!(policy.ensure_lan(Some("0.0.0.0")).is_err());
        assert!(policy.ensure_import("openapi").is_ok());
        assert!(policy.ensure_import("har").is_err());

        let open = Policy::default();
        assert!(open.ensure_ai().is_ok());
        assert!(open.ensure_lan(Some("0.0.0.0")).is_ok());
        assert!(open.ensure_import("anything").is_ok());
    }

    #[test]
    fn detects_import_format_from_json_root() {
        use serde_json::json;
        assert_eq!(
            import_format_from_json(&json!({"log": {"entries": []}})),
            "har"
        );
        assert_eq!(
            import_format_from_json(&json!({"_type": "export", "resources": []})),
            "insomnia"
        );
        assert_eq!(
            import_format_from_json(&json!({"info": {}, "item": []})),
            "postman"
        );
        assert_eq!(
            import_format_from_json(&json!({"openapi": "3.0.0"})),
            "openapi"
        );
        assert_eq!(import_format_from_json(&json!({"foo": 1})), "unknown");
    }

    #[test]
    fn disable_cloud_ai_allows_only_loopback() {
        let policy = Policy {
            disable_cloud_ai: true,
            ..Policy::default()
        };
        assert!(policy.ensure_cloud_ai("http://127.0.0.1:11434/v1").is_ok());
        assert!(policy.ensure_cloud_ai("http://localhost:11434").is_ok());
        assert!(policy.ensure_cloud_ai("http://[::1]:8080/v1").is_ok());
        assert!(policy.ensure_cloud_ai("https://api.openai.com/v1").is_err());
        assert!(policy
            .ensure_cloud_ai("https://user:pass@gateway.example.com/v1")
            .is_err());
        assert!(Policy::default()
            .ensure_cloud_ai("https://api.openai.com/v1")
            .is_ok());
    }

    #[test]
    fn retention_days_zero_means_disabled() {
        let policy = Policy::default();
        assert_eq!(policy.retention_days(), None);
        let policy = Policy {
            retention_days: 7,
            ..Policy::default()
        };
        assert_eq!(policy.retention_days(), Some(7));
    }
}
