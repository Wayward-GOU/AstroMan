//! 代理组装：MITM + 录制 handler。
//!
//! 无上游代理时保持原来的 `with_rustls_connector` 路径；配置了上游代理才切到
//! `with_http_connector`（CONNECT 隧道），避免给直连模式引入 TLS 栈回归。

use crate::capture::handler::CaptureHandler;
use crate::capture::intercept::InterceptState;
use crate::capture::store::CaptureStore;
use crate::capture::upstream::{self, ProxySpec};
use crate::capture::CaptureConfig;
use hudsucker::certificate_authority::RcgenAuthority;
use hudsucker::Proxy;
use std::future::Future;
use std::sync::Arc;
use tokio::net::TcpListener;

pub async fn run<F>(
    listener: TcpListener,
    authority: RcgenAuthority,
    config: CaptureConfig,
    store: Arc<CaptureStore>,
    intercept: Arc<InterceptState>,
    shutdown: F,
) -> Result<(), String>
where
    F: Future<Output = ()> + Send + 'static,
{
    let upstream_spec = match config.upstream_proxy.as_deref().map(str::trim) {
        Some(raw) if !raw.is_empty() => Some(ProxySpec::parse(raw)?),
        _ => None,
    };
    let handler = CaptureHandler::new(store, config, intercept);
    let provider = hudsucker::rustls::crypto::aws_lc_rs::default_provider();
    let builder = Proxy::builder().with_listener(listener).with_ca(authority);

    match upstream_spec {
        Some(spec) => {
            let built = upstream::build(&spec, provider)?;
            builder
                .with_http_connector(built.connector)
                .with_websocket_connector(hudsucker::tokio_tungstenite::Connector::Rustls(
                    built.websocket_config,
                ))
                .with_http_handler(handler)
                .with_graceful_shutdown(shutdown)
                .build()
                .map_err(|error| format!("E_PROXY_BUILD: {error}"))?
                .start()
                .await
                .map_err(|error| format!("E_PROXY: {error}"))
        }
        None => {
            // C-01 先用内置 webpki 根；C-23 换成系统根证书 connector
            builder
                .with_rustls_connector(provider)
                .with_http_handler(handler)
                .with_graceful_shutdown(shutdown)
                .build()
                .map_err(|error| format!("E_PROXY_BUILD: {error}"))?
                .start()
                .await
                .map_err(|error| format!("E_PROXY: {error}"))
        }
    }
}
