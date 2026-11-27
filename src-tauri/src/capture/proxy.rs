//! 代理组装：MITM + 录制 handler。
//!
//! 两种上游模式：
//! - 直连：交给 hudsucker 自带的 rustls 连接器，行为与之前完全一致；
//! - 经上游代理（Clash / Surge 等）：先用 CONNECT 建隧道，再在隧道上做 TLS。
//!   用户本来就在用代理分流时走这条，抓得到流量又不抢它的规则。

use crate::capture::handler::CaptureHandler;
use crate::capture::intercept::InterceptState;
use crate::capture::llm_usage::UsageSink;
use crate::capture::store::CaptureStore;
use crate::capture::CaptureConfig;
use hudsucker::certificate_authority::RcgenAuthority;
use hudsucker::hyper::Uri;
use hudsucker::Proxy;
use hyper_util::client::legacy::connect::proxy::Tunnel;
use hyper_util::client::legacy::connect::{Connect, HttpConnector};
use std::future::Future;
use std::sync::Arc;
use tokio::net::TcpListener;

/// 起代理需要的一整套零件。打包成一个结构体，避免每加一个能力就多几个参数。
pub struct ProxyParts {
    pub listener: TcpListener,
    pub authority: RcgenAuthority,
    pub config: CaptureConfig,
    pub store: Arc<CaptureStore>,
    pub intercept: Arc<InterceptState>,
    pub usage_sink: Arc<UsageSink>,
}

/// 直连模式：仍用 hudsucker 的 `with_rustls_connector`，不改变既有行为。
pub async fn run<F>(parts: ProxyParts, shutdown: F) -> Result<(), String>
where
    F: Future<Output = ()> + Send + 'static,
{
    let handler = CaptureHandler::new(parts.store, parts.config, parts.intercept, parts.usage_sink);
    Proxy::builder()
        .with_listener(parts.listener)
        .with_ca(parts.authority)
        // C-01 先用内置 webpki 根；C-23 换成系统根证书 connector
        .with_rustls_connector(hudsucker::rustls::crypto::aws_lc_rs::default_provider())
        .with_http_handler(handler)
        .with_graceful_shutdown(shutdown)
        .build()
        .map_err(|error| format!("E_PROXY_BUILD: {error}"))?
        .start()
        .await
        .map_err(|error| format!("E_PROXY: {error}"))
}

/// 经上游 HTTP 代理转发：CONNECT 建隧道 → 隧道上做 TLS → 再交给 hudsucker。
///
/// 这样客户端看到的仍是本机 MITM（能记 usage），实际出口仍是用户原来的代理，
/// 不会把「本该出海的接口」变成直连。
pub async fn run_via_upstream<F>(
    parts: ProxyParts,
    upstream: Uri,
    shutdown: F,
) -> Result<(), String>
where
    F: Future<Output = ()> + Send + 'static,
{
    let handler = CaptureHandler::new(parts.store, parts.config, parts.intercept, parts.usage_sink);
    let connector = upstream_connector(upstream)?;
    Proxy::builder()
        .with_listener(parts.listener)
        .with_ca(parts.authority)
        .with_http_connector(connector)
        .with_http_handler(handler)
        .with_graceful_shutdown(shutdown)
        .build()
        .map_err(|error| format!("E_PROXY_BUILD: {error}"))?
        .start()
        .await
        .map_err(|error| format!("E_PROXY: {error}"))
}

/// 解析 `127.0.0.1:7897` / `http://127.0.0.1:7897` 形式的上游地址。
pub fn parse_upstream(value: &str) -> Option<Uri> {
    let text = value.trim();
    if text.is_empty() {
        return None;
    }
    let text = if text.contains("://") {
        text.to_string()
    } else {
        format!("http://{text}")
    };
    text.parse::<Uri>().ok().filter(|uri| uri.host().is_some())
}

/// HttpsConnector 包一层 CONNECT 隧道：TLS 握手发生在隧道之上，目标是真实站点。
fn upstream_connector(
    upstream: Uri,
) -> Result<impl Connect + Clone + Send + Sync + 'static, String> {
    use hyper_rustls::{ConfigBuilderExt, HttpsConnectorBuilder};

    let provider = hudsucker::rustls::crypto::aws_lc_rs::default_provider();
    let tls = hudsucker::rustls::ClientConfig::builder_with_provider(Arc::new(provider))
        .with_safe_default_protocol_versions()
        .map_err(|error| format!("E_PROXY_TLS: {error}"))?
        .with_webpki_roots()
        .with_no_client_auth();

    let mut inner: HttpConnector = HttpConnector::new();
    // 目标是任意端口的 TCP，不能被「只允许 http」拦住
    inner.enforce_http(false);

    Ok(HttpsConnectorBuilder::new()
        .with_tls_config(tls)
        .https_or_http()
        .enable_http1()
        .enable_http2()
        // wrap_connector 直接给出最终连接器（隧道建好后由 hyper-rustls 做 TLS）
        .wrap_connector(Tunnel::new(upstream, inner)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_upstream_with_and_without_scheme() {
        let a = parse_upstream("127.0.0.1:7897").expect("bare addr");
        assert_eq!(a.host(), Some("127.0.0.1"));
        assert_eq!(a.port_u16(), Some(7897));
        assert_eq!(a.scheme_str(), Some("http"));

        let b = parse_upstream("http://127.0.0.1:7897").expect("with scheme");
        assert_eq!(b.port_u16(), Some(7897));
    }

    #[test]
    fn rejects_empty_or_hostless() {
        assert!(parse_upstream("   ").is_none());
        assert!(parse_upstream("http://").is_none());
    }
}
