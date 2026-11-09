//! 抓包流量内存存储。
//!
//! 第一版先保留最近若干条流量供 UI 展示，写库与历史检索随后续任务接入。
//! 所有读写都在 `parking_lot::Mutex` 内完成，代理线程和 IPC 命令可以并发访问。
//!
//! 请求/响应体会占用较多内存，因此除了条数上限，还按「已存储 body 字节数」
//! 做一次总预算淘汰：超出预算时和条数超限一样从最旧一条开始丢。

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::{HashSet, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};

/// HAR 流式导出时每批深拷贝的条数
pub const EXPORT_BATCH: usize = 200;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HeaderEntry {
    pub name: String,
    pub value: String,
}

/// 请求 / 响应体。文本直接给 `text`；图片给 `base64` 供前端预览；
/// 其它二进制只记录大小与类型。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BodyPayload {
    /// 原始 body 字节数（未截断时的真实大小）
    pub size: u64,
    /// 是否因为超过单体内存上限而只保留了前缀
    pub truncated: bool,
    /// 是否为二进制内容
    pub binary: bool,
    pub text: Option<String>,
    pub base64: Option<String>,
    pub content_type: String,
}

impl BodyPayload {
    /// 在内存里实际占用的字节数（用于总预算淘汰）。
    fn stored_bytes(&self) -> u64 {
        self.text
            .as_ref()
            .map(|value| value.len() as u64)
            .unwrap_or(0)
            + self
                .base64
                .as_ref()
                .map(|value| value.len() as u64)
                .unwrap_or(0)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FlowBrief {
    pub id: u64,
    /// 请求开始时间（毫秒时间戳）
    pub started_at: i64,
    pub method: String,
    pub scheme: String,
    pub host: String,
    pub path: String,
    pub http_version: String,
    /// 响应状态码；连接失败时为 None
    pub status: Option<u16>,
    pub duration_ms: i64,
    pub request_size: u64,
    pub response_size: u64,
    pub content_type: String,
    pub client_addr: String,
    pub error: Option<String>,
    /// 由本地映射直接应答，没有走上游
    #[serde(default)]
    pub mapped: bool,
    /// 被断点改过包（请求或响应任一）
    #[serde(default)]
    pub modified: bool,
    /// 远程映射真正发往的上游地址；未命中远程映射时为 None
    #[serde(default)]
    pub remote: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FlowDetail {
    #[serde(flatten)]
    pub brief: FlowBrief,
    pub request_headers: Vec<HeaderEntry>,
    pub response_headers: Vec<HeaderEntry>,
    pub request_body: BodyPayload,
    pub response_body: BodyPayload,
}

#[derive(Debug, Clone)]
pub struct FlowRecord {
    pub brief: FlowBrief,
    pub request_headers: Vec<HeaderEntry>,
    pub response_headers: Vec<HeaderEntry>,
    pub request_body: BodyPayload,
    pub response_body: BodyPayload,
}

impl FlowRecord {
    fn body_bytes(&self) -> u64 {
        self.request_body.stored_bytes() + self.response_body.stored_bytes()
    }
}

struct Inner {
    flows: VecDeque<FlowRecord>,
    body_bytes: u64,
    limits: crate::services::profile::CaptureLimits,
}

pub struct CaptureStore {
    inner: Mutex<Inner>,
    next_id: AtomicU64,
    total: AtomicU64,
    dropped: AtomicU64,
}

impl CaptureStore {
    pub fn new() -> Self {
        Self::new_with_limits(crate::services::profile::CaptureLimits::default())
    }

    pub fn new_with_limits(limits: crate::services::profile::CaptureLimits) -> Self {
        Self {
            inner: Mutex::new(Inner {
                flows: VecDeque::with_capacity(256),
                body_bytes: 0,
                limits,
            }),
            next_id: AtomicU64::new(1),
            total: AtomicU64::new(0),
            dropped: AtomicU64::new(0),
        }
    }

    /// 运行中切换档位：立即按新预算裁剪已有流量。
    pub fn set_limits(&self, limits: crate::services::profile::CaptureLimits) {
        let mut inner = self.inner.lock();
        inner.limits = limits;
        while (inner.flows.len() > inner.limits.max_flows
            || inner.body_bytes > inner.limits.max_body_bytes)
            && !inner.flows.is_empty()
        {
            if let Some(old) = inner.flows.pop_front() {
                inner.body_bytes = inner.body_bytes.saturating_sub(old.body_bytes());
                self.dropped.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    pub fn next_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    pub fn record(&self, record: FlowRecord) {
        let added = record.body_bytes();
        let mut inner = self.inner.lock();
        if inner.flows.len() >= inner.limits.max_flows {
            if let Some(old) = inner.flows.pop_front() {
                inner.body_bytes = inner.body_bytes.saturating_sub(old.body_bytes());
                self.dropped.fetch_add(1, Ordering::Relaxed);
            }
        }
        inner.flows.push_back(record);
        inner.body_bytes = inner.body_bytes.saturating_add(added);
        self.total.fetch_add(1, Ordering::Relaxed);

        // body 总预算超限：从最旧一条开始淘汰，直到回到预算内
        while inner.body_bytes > inner.limits.max_body_bytes && inner.flows.len() > 1 {
            if let Some(old) = inner.flows.pop_front() {
                inner.body_bytes = inner.body_bytes.saturating_sub(old.body_bytes());
                self.dropped.fetch_add(1, Ordering::Relaxed);
            } else {
                break;
            }
        }
    }

    /// 返回最近 `limit` 条，按时间正序排列（旧 → 新）。
    pub fn list(&self, limit: usize) -> Vec<FlowBrief> {
        let inner = self.inner.lock();
        let take = limit.min(inner.flows.len());
        inner
            .flows
            .iter()
            .skip(inner.flows.len() - take)
            .map(|record| record.brief.clone())
            .collect()
    }

    pub fn detail(&self, id: u64) -> Option<FlowDetail> {
        let inner = self.inner.lock();
        inner
            .flows
            .iter()
            .find(|record| record.brief.id == id)
            .map(to_detail)
    }

    /// 导出 HAR 时用，按时间正序返回全部流量。
    pub fn details(&self) -> Vec<FlowDetail> {
        let inner = self.inner.lock();
        inner.flows.iter().map(to_detail).collect()
    }

    /// 分批遍历全部流量：每次只深拷贝 `batch` 条，避免满载时一次性复制全部 body。
    pub fn for_each_detail<F>(&self, batch: usize, f: F) -> Result<(), String>
    where
        F: FnMut(&FlowDetail) -> Result<(), String>,
    {
        let ids: Vec<u64> = self
            .inner
            .lock()
            .flows
            .iter()
            .map(|record| record.brief.id)
            .collect();
        self.for_each_detail_ids(&ids, batch, f)
    }

    /// 按 id 分批遍历（分组导出），被淘汰的 id 自动跳过，顺序仍是时间正序。
    pub fn for_each_detail_ids<F>(&self, ids: &[u64], batch: usize, mut f: F) -> Result<(), String>
    where
        F: FnMut(&FlowDetail) -> Result<(), String>,
    {
        let batch = batch.max(1);
        for chunk in ids.chunks(batch) {
            let wanted: HashSet<u64> = chunk.iter().copied().collect();
            let details: Vec<FlowDetail> = {
                let inner = self.inner.lock();
                inner
                    .flows
                    .iter()
                    .filter(|record| wanted.contains(&record.brief.id))
                    .map(to_detail)
                    .collect()
            };
            for detail in &details {
                f(detail)?;
            }
        }
        Ok(())
    }

    /// 现存 body 字节数；`ids` 为空表示全部流量，用于导出前的体积提示。
    pub fn body_bytes(&self, ids: Option<&[u64]>) -> u64 {
        let inner = self.inner.lock();
        match ids {
            Some(ids) => {
                let wanted: HashSet<u64> = ids.iter().copied().collect();
                inner
                    .flows
                    .iter()
                    .filter(|record| wanted.contains(&record.brief.id))
                    .map(FlowRecord::body_bytes)
                    .sum()
            }
            None => inner.body_bytes,
        }
    }

    /// 按 id 导出（例如某个域名分组），忽略已淘汰的 id，保持时间正序。
    pub fn details_by_ids(&self, ids: &[u64]) -> Vec<FlowDetail> {
        if ids.is_empty() {
            return Vec::new();
        }
        let wanted: HashSet<u64> = ids.iter().copied().collect();
        let inner = self.inner.lock();
        inner
            .flows
            .iter()
            .filter(|record| wanted.contains(&record.brief.id))
            .map(to_detail)
            .collect()
    }

    pub fn clear(&self) {
        let mut inner = self.inner.lock();
        inner.flows.clear();
        inner.body_bytes = 0;
        self.next_id.store(1, Ordering::Relaxed);
        self.total.store(0, Ordering::Relaxed);
        self.dropped.store(0, Ordering::Relaxed);
    }

    pub fn total(&self) -> u64 {
        self.total.load(Ordering::Relaxed)
    }

    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }
}

fn to_detail(record: &FlowRecord) -> FlowDetail {
    FlowDetail {
        brief: record.brief.clone(),
        request_headers: record.request_headers.clone(),
        response_headers: record.response_headers.clone(),
        request_body: record.request_body.clone(),
        response_body: record.response_body.clone(),
    }
}

impl Default for CaptureStore {
    fn default() -> Self {
        Self::new()
    }
}
