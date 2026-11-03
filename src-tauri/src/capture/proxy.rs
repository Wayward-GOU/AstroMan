//! 代理组装：MITM + 录制 handler。

use crate::capture::handler::CaptureHandler;
use crate::capture::store::CaptureStore;
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
    shutdown: F,
) -> Result<(), String>
where
    F: Future<Output = ()> + Send + 'static,
{
    let handler = CaptureHandler::new(store, config);
    Proxy::builder()
        .with_listener(listener)
        .with_ca(authority)
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
