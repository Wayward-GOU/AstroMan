//! 集群 Token 刷新单飞（8.10.5）
//!
//! 多节点各自刷新会让彼此的 `refreshToken` 失效，因此集群模式下由主节点统一刷新：
//! 从节点通过 `PoolRefreshRequest` 把 `(pool, item_id)` 交给主节点，主节点按条目加锁，
//! 同一时刻只发一次 HTTP，再把结果用会话密钥加密回传。
//!
//! 主节点本地引擎也走同一个 `PoolRefreshCore`（进程内直接调用），避免主从并发刷新同一条目。

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use tokio::sync::Mutex as AsyncMutex;

use crate::commands::stress::refresh_http;
use crate::services::cluster::protocol::PoolItem;
use crate::services::pool::{load_items, pool_refresh_configs, PoolRefreshConfig};
use crate::state::DbPool;
use crate::util::now_ms;

/// 距过期不足该值即视为需要刷新：与单机运行期一致（8.6）
const REFRESH_AHEAD_MS: i64 = 30_000;

/// 刷新数据源：主节点从本机库读取池配置与条目
pub(crate) trait RefreshSource: Send + Sync {
    /// 池的 HTTP 刷新配置；未配置 HTTP 刷新时返回 None
    fn config(&self, pool: &str) -> Result<Option<PoolRefreshConfig>, String>;
    /// 按 id 读取池条目（只读，用于取当前 token 与账号）
    fn item(&self, pool: &str, item_id: i64) -> Result<Option<PoolItem>, String>;
    /// 池列定义（v20）：用于确定刷新写入哪一列
    fn columns(&self, pool: &str) -> Result<Vec<String>, String>;
}

/// 主节点本机数据库数据源
pub(crate) struct DbRefreshSource {
    db: DbPool,
}

impl DbRefreshSource {
    pub(crate) fn new(db: DbPool) -> Self {
        Self { db }
    }
}

impl RefreshSource for DbRefreshSource {
    fn config(&self, pool: &str) -> Result<Option<PoolRefreshConfig>, String> {
        let conn = self
            .db
            .get()
            .map_err(|error| format!("读取本机池失败：{error}"))?;
        let mut map = pool_refresh_configs(&conn, &[pool.to_string()])?;
        Ok(map.remove(pool))
    }

    fn item(&self, pool: &str, item_id: i64) -> Result<Option<PoolItem>, String> {
        let conn = self
            .db
            .get()
            .map_err(|error| format!("读取本机池失败：{error}"))?;
        Ok(load_items(&conn, pool)?
            .into_iter()
            .find(|item| item.id == item_id))
    }

    fn columns(&self, pool: &str) -> Result<Vec<String>, String> {
        let conn = self
            .db
            .get()
            .map_err(|error| format!("读取本机池失败：{error}"))?;
        let kind = crate::services::pool::pool_kind(&conn, pool);
        match crate::services::pool::pool_columns(&conn, pool) {
            Ok(columns) if !columns.is_empty() => Ok(columns),
            // 池不存在或没有列定义：按类型给出默认列，兼容旧池
            _ => Ok(crate::services::pool::default_columns(&kind)),
        }
    }
}

/// 主节点维护的内存条目：成功后缓存新值与过期时间，失败按同一套规则退避
#[derive(Debug, Clone, Default)]
struct Cached {
    value: String,
    expires_at: i64,
    fail_count: u32,
    cooldown_until: i64,
    invalid: bool,
}

/// 单条目键：池名 + 条目 id
type ItemKey = (String, i64);
/// 单条目锁：同一时刻只允许一个刷新请求在飞
type ItemLock = Arc<AsyncMutex<()>>;

/// 主节点单飞刷新核心：按 `(pool, item_id)` 加锁
pub(crate) struct PoolRefreshCore {
    source: Box<dyn RefreshSource>,
    /// 渲染刷新地址/请求体用的环境变量（主节点环境）
    vars: HashMap<String, String>,
    client: reqwest::Client,
    entries: Mutex<HashMap<ItemKey, Cached>>,
    locks: Mutex<HashMap<ItemKey, ItemLock>>,
}

impl PoolRefreshCore {
    pub(crate) fn new(source: Box<dyn RefreshSource>, vars: HashMap<String, String>) -> Self {
        Self {
            source,
            vars,
            client: reqwest::Client::new(),
            entries: Mutex::new(HashMap::new()),
            locks: Mutex::new(HashMap::new()),
        }
    }

    /// 进程内刷新入口：主节点本地引擎直接调用，从节点经 WebSocket 间接触发
    ///
    /// 返回 `(新值, 过期时间, 写入列名)`；列名随结果回传给从节点同步 `data`。
    pub(crate) async fn refresh(
        &self,
        pool: &str,
        item_id: i64,
        item_index: usize,
    ) -> Result<(String, i64, Option<String>), String> {
        let key = (pool.to_string(), item_id);
        let lock = {
            let mut locks = self.locks.lock();
            locks
                .entry(key.clone())
                .or_insert_with(|| Arc::new(AsyncMutex::new(())))
                .clone()
        };
        let _guard = lock.lock().await;

        // 刷新列：含 token 列优先，否则首列（旧池退回 value）
        let column = crate::commands::stress::refresh_column_of(&self.source.columns(pool)?);

        // 锁内二次检查：等待期间可能已被其它节点刷新
        let cached = self.entries.lock().get(&key).cloned().unwrap_or_default();
        let now = now_ms();
        if !cached.invalid && cached.expires_at != 0 && cached.expires_at - now > REFRESH_AHEAD_MS {
            return Ok((cached.value, cached.expires_at, Some(column)));
        }

        let config = self
            .source
            .config(pool)?
            .filter(PoolRefreshConfig::is_http)
            .ok_or_else(|| format!("池「{pool}」未配置 HTTP 刷新"))?;
        // 首次刷新时缓存里没有值，回源读取当前 token
        let item = if cached.value.is_empty() {
            self.source.item(pool, item_id)?
        } else {
            None
        };
        // 刷新 token 取刷新列；该列为空时退回 value（兼容旧池）
        let token = if cached.value.is_empty() {
            item.as_ref()
                .map(|item| {
                    item.data
                        .get(&column)
                        .filter(|value| !value.trim().is_empty())
                        .cloned()
                        .unwrap_or_else(|| item.value.clone())
                })
                .unwrap_or_default()
        } else {
            cached.value.clone()
        };
        let account = item.as_ref().and_then(|item| item.account.clone());

        match refresh_http(
            &self.client,
            &config,
            &self.vars,
            &token,
            account.as_deref(),
        )
        .await
        {
            Ok((value, expires_at)) => {
                log::debug!("池「{pool}」#{} 刷新成功", item_index + 1);
                self.entries.lock().insert(
                    key,
                    Cached {
                        value: value.clone(),
                        expires_at,
                        fail_count: 0,
                        cooldown_until: 0,
                        invalid: false,
                    },
                );
                Ok((value, expires_at, Some(column)))
            }
            Err(error) => {
                log::warn!("池「{pool}」token 刷新失败：{error}");
                let mut entries = self.entries.lock();
                let entry = entries.entry(key).or_default();
                entry.fail_count = entry.fail_count.saturating_add(1);
                let backoff = (1i64 << entry.fail_count.min(5)).saturating_mul(1000);
                entry.cooldown_until = now + backoff.min(60_000);
                if entry.fail_count >= 3 {
                    entry.invalid = true;
                }
                Err(error)
            }
        }
    }

    /// 本节点负责的条目是否仍可刷新（用于诊断与前端提示）
    #[allow(dead_code)]
    pub(crate) fn is_invalid(&self, pool: &str, item_id: i64) -> bool {
        self.entries
            .lock()
            .get(&(pool.to_string(), item_id))
            .map(|entry| entry.invalid)
            .unwrap_or(false)
    }
}

/// 主节点本地引擎的进程内中继：与从节点共用同一份单飞核心
pub(crate) struct LocalPoolRefreshRelay {
    core: Arc<PoolRefreshCore>,
}

impl LocalPoolRefreshRelay {
    pub(crate) fn new(core: Arc<PoolRefreshCore>) -> Self {
        Self { core }
    }
}

impl crate::commands::stress::PoolRefreshRelay for LocalPoolRefreshRelay {
    fn refresh<'a>(
        &'a self,
        pool: &'a str,
        item_id: i64,
        item_index: usize,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<(String, i64, Option<String>), String>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async move {
            // 主节点本地调用没有网络往返，但保留与从节点一致的超时语义
            tokio::time::timeout(
                Duration::from_secs(5),
                self.core.refresh(pool, item_id, item_index),
            )
            .await
            .unwrap_or_else(|_| Err("主节点刷新超时（5 秒）".to_string()))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::cluster::protocol::{
        ClusterMessage, PoolRefreshRequestPayload, PoolRefreshResultPayload,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// 测试数据源：固定配置 + 单条条目
    struct TestSource {
        config: PoolRefreshConfig,
        item: PoolItem,
    }

    impl RefreshSource for TestSource {
        fn config(&self, _pool: &str) -> Result<Option<PoolRefreshConfig>, String> {
            Ok(Some(self.config.clone()))
        }

        fn item(&self, _pool: &str, _item_id: i64) -> Result<Option<PoolItem>, String> {
            Ok(Some(self.item.clone()))
        }

        fn columns(&self, _pool: &str) -> Result<Vec<String>, String> {
            Ok(vec!["token".to_string(), "account".to_string()])
        }
    }

    fn base_config(url: String) -> PoolRefreshConfig {
        PoolRefreshConfig {
            mode: "http".to_string(),
            method: "POST".to_string(),
            url,
            headers: Vec::new(),
            body: "{\"refreshToken\":\"{{refreshToken}}\"}".to_string(),
            token_path: "$.token".to_string(),
            expires_path: "$.expiresIn".to_string(),
            expires_in_sec: 0,
        }
    }

    /// 起一个最小 HTTP 服务：每次请求延迟 delay_ms 后回固定状态与 JSON
    async fn spawn_refresh_server(
        delay_ms: u64,
        status: &'static str,
        body: &'static str,
        hits: Arc<AtomicUsize>,
    ) -> (u16, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = tokio::spawn(async move {
            loop {
                let (mut stream, _) = match listener.accept().await {
                    Ok(item) => item,
                    Err(_) => break,
                };
                hits.fetch_add(1, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                let mut buf = vec![0u8; 4096];
                let _ =
                    tokio::time::timeout(Duration::from_millis(500), stream.read(&mut buf)).await;
                let response = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes()).await;
                let _ = stream.flush().await;
            }
        });
        (port, handle)
    }

    fn core_with(url: String) -> PoolRefreshCore {
        PoolRefreshCore::new(
            Box::new(TestSource {
                config: base_config(url),
                item: PoolItem {
                    id: 7,
                    value: "old-token".to_string(),
                    account: Some("acc".to_string()),
                    weight: 1.0,
                    expires_at: 0,
                    data: HashMap::new(),
                },
            }),
            HashMap::new(),
        )
    }

    #[tokio::test]
    async fn concurrent_refresh_of_same_item_hits_http_once() {
        let hits = Arc::new(AtomicUsize::new(0));
        let (port, server) = spawn_refresh_server(
            120,
            "200 OK",
            "{\"token\":\"new-token\",\"expiresIn\":3600}",
            hits.clone(),
        )
        .await;
        let core = Arc::new(core_with(format!("http://127.0.0.1:{port}/refresh")));
        let (a, b) = tokio::join!(core.refresh("pool", 7, 0), core.refresh("pool", 7, 0),);
        assert_eq!(a.unwrap().0, "new-token");
        assert_eq!(b.unwrap().0, "new-token");
        // 服务端只收到一次刷新请求
        assert_eq!(hits.load(Ordering::SeqCst), 1);
        server.abort();
    }

    #[tokio::test]
    async fn cached_value_skips_second_http_call() {
        let hits = Arc::new(AtomicUsize::new(0));
        let (port, server) = spawn_refresh_server(
            0,
            "200 OK",
            "{\"token\":\"cached\",\"expiresIn\":3600}",
            hits.clone(),
        )
        .await;
        let core = core_with(format!("http://127.0.0.1:{port}/refresh"));
        core.refresh("pool", 7, 0).await.unwrap();
        core.refresh("pool", 7, 0).await.unwrap();
        assert_eq!(hits.load(Ordering::SeqCst), 1);
        server.abort();
    }

    #[tokio::test]
    async fn three_failures_mark_item_invalid() {
        let hits = Arc::new(AtomicUsize::new(0));
        let (port, server) =
            spawn_refresh_server(0, "500 Internal Server Error", "{}", hits.clone()).await;
        let core = core_with(format!("http://127.0.0.1:{port}/refresh"));
        for _ in 0..3 {
            assert!(core.refresh("pool", 7, 0).await.is_err());
        }
        assert!(core.is_invalid("pool", 7));
        assert_eq!(hits.load(Ordering::SeqCst), 3);
        server.abort();
    }

    #[test]
    fn refresh_messages_roundtrip() {
        let request = ClusterMessage::PoolRefreshRequest(PoolRefreshRequestPayload {
            request_id: "req-1".to_string(),
            pool: "token-pool".to_string(),
            item_id: 42,
            item_index: 3,
        });
        let text = serde_json::to_string(&request).unwrap();
        assert!(text.contains("\"type\":\"poolRefreshRequest\""));
        let back: ClusterMessage = serde_json::from_str(&text).unwrap();
        assert!(matches!(back, ClusterMessage::PoolRefreshRequest(_)));

        let result = ClusterMessage::PoolRefreshResult(PoolRefreshResultPayload {
            request_id: "req-1".to_string(),
            ok: false,
            error: Some("HTTP 500".to_string()),
            nonce: String::new(),
            payload: String::new(),
            digest: String::new(),
            column: Some("token".to_string()),
        });
        let text = serde_json::to_string(&result).unwrap();
        assert!(text.contains("\"type\":\"poolRefreshResult\""));
        let back: ClusterMessage = serde_json::from_str(&text).unwrap();
        assert!(matches!(back, ClusterMessage::PoolRefreshResult(_)));
    }
}
