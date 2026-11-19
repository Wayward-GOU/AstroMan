//! 抓包上游代理与 SSL 直通（R6）。
//!
//! - `ProxySpec`：解析 `http://user:pass@host:port`，只在启动时校验一次；
//! - `build`：用 hyper-http-proxy 的 CONNECT 隧道包住 hudsucker 的明文 connector，
//!   https 走隧道后再由 ProxyConnector 做 TLS，http 直接以 absolute-form 发给代理；
//! - `PassthroughRules`：ssl_hosts 白名单，命中域名 / CIDR 的 CONNECT 或 TLS 握手
//!   不解密，直接 TCP 隧道转发。

use hudsucker::hyper::Uri;
use hudsucker::hyper_util::client::legacy::connect::HttpConnector;
use hudsucker::rustls::crypto::CryptoProvider;
use hudsucker::rustls::ClientConfig;
use hyper_http_proxy::{Intercept, Proxy, ProxyConnector};
use hyper_rustls::ConfigBuilderExt;
use std::sync::Arc;

pub struct UpstreamConnector {
    pub connector: ProxyConnector<HttpConnector>,
    /// wss 直连使用的客户端配置；上游代理模式下 hudsucker 不经过代理建立 WebSocket
    pub websocket_config: Arc<ClientConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProxySpec {
    /// 原始地址（保留 userinfo，交给 ProxyConnector 提取认证头）
    pub uri: Uri,
    pub host: String,
    pub port: u16,
    pub auth: Option<(String, String)>,
}

impl ProxySpec {
    pub fn parse(raw: &str) -> Result<Self, String> {
        let raw = raw.trim();
        if raw.is_empty() {
            return Err("E_UPSTREAM_INVALID: 上游代理地址为空".into());
        }
        let uri: Uri = raw
            .parse()
            .map_err(|_| format!("E_UPSTREAM_INVALID: 无法解析上游代理地址 {raw}"))?;
        let scheme = uri.scheme_str().unwrap_or_default();
        if scheme != "http" {
            return Err(format!(
                "E_UPSTREAM_SCHEME: 仅支持 http:// 上游代理，当前为 {scheme}://；\
socks5 与 https 代理暂不支持"
            ));
        }
        let authority = uri
            .authority()
            .ok_or_else(|| "E_UPSTREAM_INVALID: 上游代理缺少 host:port".to_string())?;
        let host = authority.host().trim().to_string();
        if host.is_empty() {
            return Err("E_UPSTREAM_INVALID: 上游代理缺少主机名".into());
        }
        let port = authority.port_u16().unwrap_or(80);
        let auth = authority
            .as_str()
            .rsplit_once('@')
            .and_then(|(userinfo, _)| {
                userinfo
                    .split_once(':')
                    .map(|(user, pass)| (user.to_string(), pass.to_string()))
            });
        Ok(Self {
            uri,
            host,
            port,
            auth,
        })
    }

    pub fn label(&self) -> String {
        match &self.auth {
            Some((user, _)) => format!("http://{user}:***@{}:{}", self.host, self.port),
            None => format!("http://{}:{}", self.host, self.port),
        }
    }
}

pub fn build(spec: &ProxySpec, provider: CryptoProvider) -> Result<UpstreamConnector, String> {
    let proxy = Proxy::new(Intercept::All, spec.uri.clone());
    let connector = ProxyConnector::from_proxy(HttpConnector::new(), proxy)
        .map_err(|error| format!("E_UPSTREAM_BUILD: {error}"))?;
    let config = ClientConfig::builder_with_provider(Arc::new(provider))
        .with_safe_default_protocol_versions()
        .map_err(|error| format!("E_UPSTREAM_BUILD: {error}"))?
        .with_webpki_roots()
        .with_no_client_auth();
    Ok(UpstreamConnector {
        connector,
        websocket_config: Arc::new(config),
    })
}

#[derive(Debug, Clone)]
enum HostRule {
    /// `*.example.com`：后缀匹配，包含裸域
    Suffix(String),
    /// `api.example.com:443`：精确到端口
    ExactPort(String, u16),
    /// `10.0.0.0/8`：仅 IPv4 字面量
    Cidr(u32, u8),
}

#[derive(Debug, Clone, Default)]
pub struct PassthroughRules {
    rules: Vec<HostRule>,
}

impl PassthroughRules {
    pub fn new(patterns: &[String]) -> Self {
        let rules = patterns
            .iter()
            .filter_map(|pattern| Self::parse_rule(pattern.trim()))
            .collect();
        Self { rules }
    }

    fn parse_rule(raw: &str) -> Option<HostRule> {
        if raw.is_empty() {
            return None;
        }
        if let Some((addr, bits)) = raw.split_once('/') {
            let ip: std::net::Ipv4Addr = addr.parse().ok()?;
            let bits: u8 = bits.parse().ok()?;
            if bits > 32 {
                return None;
            }
            let mask = if bits == 0 {
                0
            } else {
                u32::MAX << (32 - bits)
            };
            return Some(HostRule::Cidr(u32::from(ip) & mask, bits));
        }
        if let Some(rest) = raw.strip_prefix("*.") {
            return Some(HostRule::Suffix(rest.to_ascii_lowercase()));
        }
        if let Some((host, port)) = raw.rsplit_once(':') {
            if let Ok(port) = port.parse::<u16>() {
                return Some(HostRule::ExactPort(host.to_ascii_lowercase(), port));
            }
        }
        Some(HostRule::Suffix(raw.to_ascii_lowercase()))
    }

    /// `host` 为不带方括号的主机名或 IP 字面量
    pub fn matches(&self, host: &str, port: u16) -> bool {
        let host = host.trim_matches(['[', ']']).to_ascii_lowercase();
        if host.is_empty() {
            return false;
        }
        self.rules.iter().any(|rule| match rule {
            HostRule::Suffix(suffix) => host == *suffix || host.ends_with(&format!(".{suffix}")),
            HostRule::ExactPort(name, rule_port) => host == *name && port == *rule_port,
            HostRule::Cidr(masked, bits) => host
                .parse::<std::net::Ipv4Addr>()
                .map(|ip| {
                    let mask = if *bits == 0 {
                        0
                    } else {
                        u32::MAX << (32 - *bits)
                    };
                    (u32::from(ip) & mask) == *masked
                })
                .unwrap_or(false),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proxy_spec_parses_auth_and_scheme() {
        let spec = ProxySpec::parse(" http://u:p@10.0.0.1:3128 ").expect("parse");
        assert_eq!(spec.host, "10.0.0.1");
        assert_eq!(spec.port, 3128);
        assert_eq!(spec.auth, Some(("u".to_string(), "p".to_string())));
        assert_eq!(spec.label(), "http://u:***@10.0.0.1:3128");

        assert!(ProxySpec::parse("socks5://127.0.0.1:1080")
            .unwrap_err()
            .starts_with("E_UPSTREAM_SCHEME"));
        assert!(ProxySpec::parse("https://proxy.local:8443")
            .unwrap_err()
            .starts_with("E_UPSTREAM_SCHEME"));
        assert!(ProxySpec::parse("not a url").is_err());
    }

    #[test]
    fn passthrough_matches_wildcard_port_and_cidr() {
        let rules = PassthroughRules::new(&[
            "*.example.test".to_string(),
            "api.other.test:8443".to_string(),
            "10.0.0.0/8".to_string(),
        ]);
        assert!(rules.matches("a.example.test", 443));
        assert!(rules.matches("example.test", 443));
        assert!(!rules.matches("example.test.evil", 443));
        assert!(rules.matches("api.other.test", 8443));
        assert!(!rules.matches("api.other.test", 443));
        assert!(rules.matches("10.1.2.3", 443));
        assert!(!rules.matches("11.1.2.3", 443));
        assert!(!rules.matches("not-an-ip", 443));
    }
}
