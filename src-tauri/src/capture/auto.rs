//! 自动捕获开关：应用启动即接管，不用每次去抓包页点「开始录制」。
//!
//! 配置落在 `<数据目录>/capture/auto.json`，默认开启且只代理 LLM 接口域名
//! （PAC 分流），退出时自动还原系统代理。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const FILE: &str = "auto.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoCapture {
    pub enabled: bool,
    /// `llm`：只把模型接口域名指向本机代理；`all`：全部流量走代理。
    /// 空串 / 未知值按 `llm` 处理。
    #[serde(default = "default_scope")]
    pub scope: String,
}

fn default_scope() -> String {
    "llm".to_string()
}

impl Default for AutoCapture {
    fn default() -> Self {
        // 用户要的是「自己捕获」：默认就开，且默认只碰 LLM 域名
        Self {
            enabled: true,
            scope: default_scope(),
        }
    }
}

pub fn path(data_dir: &Path) -> PathBuf {
    data_dir.join("capture").join(FILE)
}

pub fn load(data_dir: &Path) -> AutoCapture {
    std::fs::read_to_string(path(data_dir))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn save(data_dir: &Path, config: &AutoCapture) -> Result<(), String> {
    let target = path(data_dir);
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("E_AUTO: 创建目录失败 {e}"))?;
    }
    let text =
        serde_json::to_string_pretty(config).map_err(|e| format!("E_AUTO: 序列化失败 {e}"))?;
    std::fs::write(&target, text).map_err(|e| format!("E_AUTO: 写入 {} 失败 {e}", target.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_auto_llm() {
        let config = AutoCapture::default();
        assert!(config.enabled);
        assert_eq!(config.scope, "llm");
    }

    #[test]
    fn loads_missing_file_as_default() {
        let dir = std::env::temp_dir().join("astroman-auto-missing");
        let _ = std::fs::remove_dir_all(&dir);
        assert!(load(&dir).enabled);
    }

    #[test]
    fn round_trips() {
        let dir = std::env::temp_dir().join("astroman-auto-roundtrip");
        let _ = std::fs::remove_dir_all(&dir);
        save(
            &dir,
            &AutoCapture {
                enabled: false,
                scope: "all".into(),
            },
        )
        .expect("save");
        let loaded = load(&dir);
        assert!(!loaded.enabled);
        assert_eq!(loaded.scope, "all");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
