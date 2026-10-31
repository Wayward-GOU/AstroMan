//! 建连计时探针（G-05）
//!
//! reqwest 不告诉调用方「这次请求有没有新建连接」。可行且稳定的口径是：
//! **连接器的 `call()` 有没有被调用**——
//! - 被调用 = 新建了 TCP（含 TLS），`connectMs` 有值；
//! - 没被调用 = 用的是池里的连接，`connectMs` 记 0 且 `reusedConnection = true`。
//!
//! 注意两个由此产生的口径事实（必须写进 UI 文案，否则用户会以为功能坏了）：
//! 1. 复用连接时 `connectMs = 0` 不代表「连接极快」，而是「没有建连」；
//! 2. 启用代理时本探针插在内层，代理握手不计入 `connectMs`。
//!
//! 并发场景下（套件 / 压测）同一个客户端会被多个请求共用，探针的增量可能来自
//! 别的请求，因此这里是**近似值**；压测引擎不使用该探针，不受影响。

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tower::{Layer, Service};

/// 累计新建连接数 + 最近一次建连耗时（微秒）
#[derive(Debug, Default)]
pub struct ConnectorProbe {
    connects: AtomicU64,
    last_us: AtomicU64,
}

impl ConnectorProbe {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// 发送前拍快照，发送后比较是否增加
    pub fn snapshot(&self) -> u64 {
        self.connects.load(Ordering::SeqCst)
    }

    pub fn last_connect_us(&self) -> u64 {
        self.last_us.load(Ordering::SeqCst)
    }

    fn record(&self, elapsed: Duration) {
        self.last_us.store(
            elapsed.as_micros().min(u64::MAX as u128) as u64,
            Ordering::SeqCst,
        );
        self.connects.fetch_add(1, Ordering::SeqCst);
    }
}

/// 包在连接器外层的计时服务
#[derive(Clone)]
pub struct ProbeService<S> {
    inner: S,
    probe: Arc<ConnectorProbe>,
}

impl<S> ProbeService<S> {
    pub fn new(inner: S, probe: Arc<ConnectorProbe>) -> Self {
        Self { inner, probe }
    }
}

impl<S, R> Service<R> for ProbeService<S>
where
    S: Service<R>,
    S::Future: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future =
        std::pin::Pin<Box<dyn std::future::Future<Output = Result<S::Response, S::Error>> + Send>>;

    fn poll_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: R) -> Self::Future {
        let started = std::time::Instant::now();
        let probe = self.probe.clone();
        let future = self.inner.call(req);
        Box::pin(async move {
            let outcome = future.await;
            if outcome.is_ok() {
                probe.record(started.elapsed());
            }
            outcome
        })
    }
}

/// 生成一个可传给 `reqwest::ClientBuilder::connector_layer` 的层
#[derive(Clone)]
pub struct ProbeLayer {
    probe: Arc<ConnectorProbe>,
}

impl ProbeLayer {
    pub fn new(probe: Arc<ConnectorProbe>) -> Self {
        Self { probe }
    }
}

impl<S> Layer<S> for ProbeLayer {
    type Service = ProbeService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        ProbeService::new(inner, self.probe.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_counts_connections_and_keeps_last_duration() {
        let probe = ConnectorProbe::new();
        assert_eq!(probe.snapshot(), 0);
        probe.record(Duration::from_micros(1234));
        probe.record(Duration::from_micros(4321));
        assert_eq!(probe.snapshot(), 2);
        assert_eq!(probe.last_connect_us(), 4321);
    }
}
