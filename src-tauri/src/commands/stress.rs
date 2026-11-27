use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use rusqlite::{params, OptionalExtension};
use serde::Serialize;
use serde_json::json;
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder};
use tokio::sync::{mpsc, Mutex as AsyncMutex};
use tokio::task::JoinSet;

use crate::commands::http::base64_encode;
use crate::models::{
    CapturePreset, ErrorTypeCount, HistogramBucket, KeyValueItem, PoolBinding, RecordDetailMode,
    ReportError, ReportRequestRecord, ReportSample, RequestAuth, SlowRequest, StageSummary,
    StatusCount, StressConfig, StressDone, StressProgress, StressReport, StressRequestDef,
    StressStage, UrlStatRow,
};
use crate::services::cluster::protocol::PoolItem;
use crate::services::cluster::protocol::SamplePayload as ClusterSamplePayload;
use crate::state::{AppState, DbPool, StressJobDraft};
use crate::util::{err, now_ms, render, strip_json_comments};

/// 并发绝对安全上限（防止误输入导致内存/句柄耗尽）。
///
/// v1.1.2 起不再用它表达「产品上限」：实际能否启动由本机并发预算
/// （`stress_budget_get`）决定，前端也不应再各自写死 5000。
const MAX_CONCURRENT: u32 = 100_000;
/// 延迟直方图上限（毫秒），超出的请求计入最后一个桶
const MAX_RT_MS: usize = 60_000;
/// 首字节时间直方图上限：与总耗时对齐，避免慢服务 TTFB 被 10s 截断
const MAX_TTFB_MS: usize = MAX_RT_MS;
/// 状态码计数的分片数：每请求都要更新，单锁会成为高 QPS 下的争用点
const CODES_SHARDS: usize = 8;
/// 报告中最多保留的错误明细条数
const MAX_ERRORS: usize = 500;
/// 报告中最多保留的逐请求明细条数
const MAX_REQUEST_RECORDS: usize = 50_000;
/// 请求明细中错误信息的最大长度
const MAX_RECORD_ERROR: usize = 500;
/// 请求明细中请求体的最大长度
const MAX_RECORD_BODY: usize = 1000;
/// 请求明细中响应体的最大字符数
const MAX_RECORD_RESPONSE: usize = 2000;
const SNIPPET_LIMIT: usize = 256;
/// APDEX 的 T 值（毫秒）
const APDEX_T: f64 = 500.0;
/// sample 模式下每 N 个请求记录 1 条明细
const SAMPLE_EVERY: u64 = 100;
/// auto 模式定档阈值：预估/实测 QPS 达到该值即关闭明细
const AUTO_OFF_QPS: f64 = 5_000.0;
/// auto 模式定档阈值：达到该值降为采样
const AUTO_SAMPLE_QPS: f64 = 2_000.0;
/// 运行期连续多少个采样点超过阈值后降档
const AUTO_DOWNGRADE_WINDOW: usize = 3;
/// 长压测降低采样频率的阈值（秒）
const SAMPLE_DECIMATE_2S_SEC: u64 = 2 * 3600;
const SAMPLE_DECIMATE_5S_SEC: u64 = 6 * 3600;
/// 固定并发快速压测上限（2h）；走压测计划可到 24h
const MAX_FIXED_QUICK_SEC: u32 = 7_200;
/// 固定并发（计划）与阶梯总时长上限（24h）
const MAX_FIXED_PLAN_SEC: u32 = 86_400;
/// 阶梯单阶段上限
const MAX_STAGE_SEC: u32 = 3_600;
/// 超过该时长必须显式确认长稳风险
const LONG_RUN_ACK_SEC: u64 = 7_200;
/// 定位模式默认保留的慢请求条数
const DEFAULT_TOP_N: u32 = 200;
/// 慢请求条数硬上限
const MAX_TOP_N: u32 = 2_000;
/// URL 聚合分片数：按 key 哈希分片降低单锁竞争
const URL_STATS_SHARDS: usize = 16;
/// URL 聚合全局键上限：超出后归入 `__other__`，报告标记不完整
const URL_STATS_MAX_KEYS: usize = 2_000;
/// URL 入库前最大长度（字符）
const MAX_URL_LEN: usize = 2_048;
/// URL 统计中错误样例的最大长度（字符）
const MAX_URL_ERROR: usize = 500;
/// 溢出桶的展示占位
const URL_OTHER: &str = "__other__";

/// URL 统计的聚合键：同一地址按状态码拆行，便于定位错误码分布
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct UrlKey {
    method: String,
    url: String,
    status: u16,
}

// 每线程缓存最近一次聚合键：单计划单地址场景下避免逐请求分配与哈希
thread_local! {
    static URL_KEY_CACHE: RefCell<Option<(UrlKey, usize)>> = const { RefCell::new(None) };
}

/// URL 统计累加器：只做计数与极值，不做字符串格式化
#[derive(Debug, Clone, Default)]
struct UrlAgg {
    requests: u64,
    success: u64,
    errors: u64,
    sum_ms: f64,
    max_ms: f64,
    max_at_ms: i64,
    bytes: u64,
    error_message: Option<String>,
    max_request_id: Option<String>,
}

impl UrlAgg {
    fn update(
        &mut self,
        elapsed_ms: i64,
        latency_ms: u64,
        bytes: u64,
        ok: bool,
        error: Option<&str>,
    ) {
        self.requests += 1;
        if ok {
            self.success += 1;
        } else {
            self.errors += 1;
            if self.error_message.is_none() {
                if let Some(message) = error.map(str::trim).filter(|text| !text.is_empty()) {
                    self.error_message = Some(clip(message, MAX_URL_ERROR));
                }
            }
        }
        self.sum_ms += latency_ms as f64;
        if latency_ms as f64 > self.max_ms {
            self.max_ms = latency_ms as f64;
            self.max_at_ms = elapsed_ms;
        }
        self.bytes = self.bytes.saturating_add(bytes);
    }
}

fn url_shard_index(key: &UrlKey) -> usize {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    key.hash(&mut hasher);
    (hasher.finish() as usize) % URL_STATS_SHARDS
}

/// URL 查询参数脱敏：只掩码敏感参数值，保留地址结构与其余参数
pub(crate) fn sanitize_url(raw: &str) -> String {
    let truncated: String = raw.chars().take(MAX_URL_LEN).collect();
    let Some(query_start) = truncated.find('?') else {
        return truncated;
    };
    let (head, rest) = truncated.split_at(query_start + 1);
    let (query, fragment) = match rest.find('#') {
        Some(index) => (&rest[..index], &rest[index..]),
        None => (rest, ""),
    };
    let masked = query
        .split('&')
        .map(|pair| {
            let Some(eq) = pair.find('=') else {
                return pair.to_string();
            };
            let (name, _) = pair.split_at(eq);
            if is_sensitive_query(&name.to_ascii_lowercase()) {
                format!("{name}=******")
            } else {
                pair.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("&");
    format!("{head}{masked}{fragment}")
}

fn is_sensitive_query(name: &str) -> bool {
    const KEYS: &[&str] = &[
        "token",
        "access_token",
        "refresh_token",
        "id_token",
        "password",
        "passwd",
        "secret",
        "authorization",
        "cookie",
        "api_key",
        "apikey",
        "client_secret",
        "session",
        "sessionid",
        "signature",
        "sig",
    ];
    KEYS.iter()
        .any(|key| name == *key || name.ends_with(&format!("_{key}")))
}

/// 错误信息脱敏：掩码 Bearer / Basic、key=value 与 sk- 形式的密钥
pub(crate) fn sanitize_error(raw: &str) -> String {
    static BEARER: OnceLock<regex::Regex> = OnceLock::new();
    static JSON: OnceLock<regex::Regex> = OnceLock::new();
    static KV: OnceLock<regex::Regex> = OnceLock::new();
    static SK: OnceLock<regex::Regex> = OnceLock::new();

    let text: String = raw.chars().take(MAX_URL_ERROR).collect();
    if text.is_empty() {
        return text;
    }
    let bearer = BEARER.get_or_init(|| {
        regex::Regex::new(r"(?i)((?:Bearer|Basic)\s+)([A-Za-z0-9._~+/=-]{6,})").expect("bearer")
    });
    let out = bearer
        .replace_all(&text, |caps: &regex::Captures| {
            if caps[2].contains("******") {
                caps[0].to_string()
            } else {
                format!("{}{}", &caps[1], "******")
            }
        })
        .into_owned();

    // JSON 响应体里的敏感字段：{"token":"abc"} / {"password":"..."}
    let json = JSON.get_or_init(|| {
        regex::Regex::new(
            r#"(?i)("[^"]*(?:api[_-]?key|token|secret|password|authorization|cookie)[^"]*"\s*:\s*)"([^"]*)""#,
        )
        .expect("json")
    });
    let out = json
        .replace_all(&out, |caps: &regex::Captures| {
            if caps[2].contains("******") {
                caps[0].to_string()
            } else {
                format!("{}\"******\"", &caps[1])
            }
        })
        .into_owned();

    let kv = KV.get_or_init(|| {
        regex::Regex::new(
            r#"(?i)((?:api[_-]?key|token|secret|password|authorization|cookie)\s*[:=]\s*)([^\s,;}\]"'&]+)"#,
        )
        .expect("kv")
    });
    let out = kv
        .replace_all(&out, |caps: &regex::Captures| {
            if caps[2].contains("******")
                || caps[2].eq_ignore_ascii_case("bearer")
                || caps[2].eq_ignore_ascii_case("basic")
            {
                caps[0].to_string()
            } else {
                format!("{}{}", &caps[1], "******")
            }
        })
        .into_owned();

    let sk = SK.get_or_init(|| regex::Regex::new(r"sk-[A-Za-z0-9]{6,}").expect("sk"));
    sk.replace_all(&out, "******").into_owned()
}

/// 运行期明细采集开关（可被 auto 降档动态修改）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Capture {
    Off = 0,
    Sample = 1,
    Full = 2,
}

impl Capture {
    fn from_u8(raw: u8) -> Self {
        match raw {
            0 => Capture::Off,
            1 => Capture::Sample,
            _ => Capture::Full,
        }
    }

    fn label(&self) -> &'static str {
        match self {
            Capture::Off => "off",
            Capture::Sample => "sample",
            Capture::Full => "full",
        }
    }
}

/// 单接口计数器：按配置 requests 下标分片，采样时取增量
#[derive(Default)]
struct InterfaceCounter {
    total: AtomicU64,
    success: AtomicU64,
    errors: AtomicU64,
    sum_ms: AtomicU64,
}

struct Metrics {
    total: AtomicU64,
    success: AtomicU64,
    errors: AtomicU64,
    /// 真正拿到 HTTP 响应的请求数：TTFB / 响应体统计的分母
    responses: AtomicU64,
    sum_ms: AtomicU64,
    /// 延迟平方和：用精确值算标准差，避免直方图桶中值近似
    sum_sq_ms: AtomicU64,
    sum_ttfb: AtomicU64,
    /// 仅响应请求的响应体耗时累加（latency − ttfb）
    sum_body: AtomicU64,
    bytes: AtomicU64,
    /// 发送方向字节（请求行 + 头 + 体），启动时算出的单请求静态字节 × 请求数
    bytes_sent: AtomicU64,
    /// 错误明细因通道满或超出上限被丢弃的条数
    errors_dropped: AtomicU64,
    /// 请求明细因超出上限被丢弃的条数
    records_dropped: AtomicU64,
    /// 慢请求明细因队列满被丢弃的条数
    slow_dropped: AtomicU64,
    min_ms: AtomicU64,
    /// APDEX 分子计数：只统计成功请求，失败请求自动落入「失望」
    apdex_satisfied: AtomicU64,
    apdex_tolerating: AtomicU64,
    max_ms: AtomicU64,
    concurrent: AtomicU32,
    buckets: Vec<AtomicU64>,
    ttfb_buckets: Vec<AtomicU64>,
    /// 响应体读取耗时（latency − ttfb）直方图：均值/分位数都从这里算
    body_buckets: Vec<AtomicU64>,
    codes: Vec<Mutex<HashMap<u16, u64>>>,
    error_kinds: Mutex<HashMap<String, i64>>,
    /// 按 URL 聚合的分片锁：与 recordDetail 无关，off 模式也统计
    url_shards: Vec<Mutex<HashMap<UrlKey, UrlAgg>>>,
    /// 键数超过上限后的统一溢出桶
    url_other: Mutex<UrlAgg>,
    /// 已占用的 URL 键数量
    url_keys: AtomicU64,
    /// 是否发生过键溢出（报告标记 URL 统计不完整）
    url_overflow: AtomicBool,
    /// 按接口下标的计数：用于每秒合并出接口 QPS 曲线（空表示未启用）
    interface_counters: Vec<InterfaceCounter>,
    /// 仅测试用：关闭 URL 聚合以测量单请求开销
    #[cfg(test)]
    url_stats_enabled: bool,
}

impl Metrics {
    #[cfg(test)]
    fn new() -> Self {
        Self::new_with_interfaces(0)
    }

    /// `interfaces` 为本次运行的接口数量：大于 0 时逐请求累加各接口计数
    fn new_with_interfaces(interfaces: usize) -> Self {
        Self {
            total: AtomicU64::new(0),
            success: AtomicU64::new(0),
            errors: AtomicU64::new(0),
            responses: AtomicU64::new(0),
            sum_ms: AtomicU64::new(0),
            sum_sq_ms: AtomicU64::new(0),
            sum_ttfb: AtomicU64::new(0),
            sum_body: AtomicU64::new(0),
            bytes: AtomicU64::new(0),
            bytes_sent: AtomicU64::new(0),
            errors_dropped: AtomicU64::new(0),
            records_dropped: AtomicU64::new(0),
            slow_dropped: AtomicU64::new(0),
            min_ms: AtomicU64::new(u64::MAX),
            apdex_satisfied: AtomicU64::new(0),
            apdex_tolerating: AtomicU64::new(0),
            max_ms: AtomicU64::new(0),
            concurrent: AtomicU32::new(0),
            buckets: (0..=MAX_RT_MS).map(|_| AtomicU64::new(0)).collect(),
            ttfb_buckets: (0..=MAX_TTFB_MS).map(|_| AtomicU64::new(0)).collect(),
            body_buckets: (0..=MAX_RT_MS).map(|_| AtomicU64::new(0)).collect(),
            codes: (0..CODES_SHARDS)
                .map(|_| Mutex::new(HashMap::new()))
                .collect(),
            error_kinds: Mutex::new(HashMap::new()),
            url_shards: (0..URL_STATS_SHARDS)
                .map(|_| Mutex::new(HashMap::new()))
                .collect(),
            url_other: Mutex::new(UrlAgg::default()),
            url_keys: AtomicU64::new(0),
            url_overflow: AtomicBool::new(false),
            interface_counters: (0..interfaces).map(|_| InterfaceCounter::default()).collect(),
            #[cfg(test)]
            url_stats_enabled: true,
        }
    }

    /// 测试专用：关闭 URL 聚合，用于对比聚合前后的单请求开销
    #[cfg(test)]
    fn new_with_url_stats(enabled: bool) -> Self {
        let mut metrics = Self::new();
        metrics.url_stats_enabled = enabled;
        metrics
    }

    #[allow(clippy::too_many_arguments)]
    fn record(
        &self,
        method: &str,
        url: &str,
        elapsed_ms: i64,
        latency_ms: u64,
        ttfb_ms: u64,
        body_ms: u64,
        status: u16,
        bytes: u64,
        sent_bytes: u64,
        ok: bool,
        error: Option<&str>,
        iface: usize,
    ) {
        self.total.fetch_add(1, Ordering::Relaxed);
        if ok {
            self.success.fetch_add(1, Ordering::Relaxed);
        } else {
            self.errors.fetch_add(1, Ordering::Relaxed);
        }
        self.sum_ms.fetch_add(latency_ms, Ordering::Relaxed);
        self.sum_sq_ms
            .fetch_add(latency_ms.saturating_mul(latency_ms), Ordering::Relaxed);
        self.bytes.fetch_add(bytes, Ordering::Relaxed);
        self.bytes_sent.fetch_add(sent_bytes, Ordering::Relaxed);
        self.min_ms.fetch_min(latency_ms, Ordering::Relaxed);
        self.max_ms.fetch_max(latency_ms, Ordering::Relaxed);
        let index = latency_ms.min(MAX_RT_MS as u64) as usize;
        self.buckets[index].fetch_add(1, Ordering::Relaxed);
        // 没拿到响应（连接失败 / 超时）的请求没有 TTFB 和响应体，
        // 不能按 0 计入，否则 TTFB 均值会被网络错误拉低、响应体耗时被拉高
        if ok {
            if (latency_ms as f64) < APDEX_T {
                self.apdex_satisfied.fetch_add(1, Ordering::Relaxed);
            } else if (latency_ms as f64) < APDEX_T * 4.0 {
                self.apdex_tolerating.fetch_add(1, Ordering::Relaxed);
            }
        }
        if status > 0 {
            self.responses.fetch_add(1, Ordering::Relaxed);
            self.sum_ttfb.fetch_add(ttfb_ms, Ordering::Relaxed);
            let ttfb_index = ttfb_ms.min(MAX_TTFB_MS as u64) as usize;
            self.ttfb_buckets[ttfb_index].fetch_add(1, Ordering::Relaxed);
            // body 在 send_once 内单独测量，重试时不会把第一次请求算进来
            self.sum_body.fetch_add(body_ms, Ordering::Relaxed);
            let body_index = body_ms.min(MAX_RT_MS as u64) as usize;
            self.body_buckets[body_index].fetch_add(1, Ordering::Relaxed);
            let shard = status as usize % CODES_SHARDS;
            if let Ok(mut map) = self.codes[shard].lock() {
                *map.entry(status).or_insert(0) += 1;
            }
        }
        self.record_url(
            method, url, elapsed_ms, latency_ms, status, bytes, ok, error,
        );
        // 接口维度：多接口压测时按配置下标累加，单接口时下标恒为 0
        if let Some(counter) = self.interface_counters.get(iface) {
            counter.total.fetch_add(1, Ordering::Relaxed);
            if ok {
                counter.success.fetch_add(1, Ordering::Relaxed);
            } else {
                counter.errors.fetch_add(1, Ordering::Relaxed);
            }
            counter.sum_ms.fetch_add(latency_ms, Ordering::Relaxed);
        }
    }

    /// 各接口累计值快照：(总请求, 成功, 失败, 耗时累加)
    fn interface_snapshot(&self) -> Vec<(u64, u64, u64, u64)> {
        self.interface_counters
            .iter()
            .map(|counter| {
                (
                    counter.total.load(Ordering::Relaxed),
                    counter.success.load(Ordering::Relaxed),
                    counter.errors.load(Ordering::Relaxed),
                    counter.sum_ms.load(Ordering::Relaxed),
                )
            })
            .collect()
    }

    /// URL 聚合：与明细采集解耦，off / sample / full 三种模式都计数
    #[allow(clippy::too_many_arguments)]
    fn record_url(
        &self,
        method: &str,
        url: &str,
        elapsed_ms: i64,
        latency_ms: u64,
        status: u16,
        bytes: u64,
        ok: bool,
        error: Option<&str>,
    ) {
        #[cfg(test)]
        if !self.url_stats_enabled {
            return;
        }
        let method = method.trim();
        let method = if method.is_empty() { "GET" } else { method };
        // 快路径：单计划单地址时直接复用线程内缓存键，避免逐请求字符串分配与哈希
        let cached = URL_KEY_CACHE.with(|cell| {
            let cache = cell.borrow();
            let Some((key, shard)) = cache.as_ref() else {
                return false;
            };
            if key.status != status || key.url != url || !key.method.eq_ignore_ascii_case(method) {
                return false;
            }
            let mut handled = false;
            if let Ok(mut map) = self.url_shards[*shard].lock() {
                if let Some(agg) = map.get_mut(key) {
                    agg.update(elapsed_ms, latency_ms, bytes, ok, error);
                    handled = true;
                }
            }
            handled
        });
        if cached {
            return;
        }

        let key = UrlKey {
            method: method.to_string(),
            url: if url.len() <= MAX_URL_LEN {
                url.to_string()
            } else {
                url.chars().take(MAX_URL_LEN).collect()
            },
            status,
        };
        let shard = url_shard_index(&key);
        let mut handled = false;
        if let Ok(mut map) = self.url_shards[shard].lock() {
            if let Some(agg) = map.get_mut(&key) {
                agg.update(elapsed_ms, latency_ms, bytes, ok, error);
                handled = true;
            } else if self.url_keys.load(Ordering::Relaxed) < URL_STATS_MAX_KEYS as u64 {
                let mut agg = UrlAgg::default();
                agg.update(elapsed_ms, latency_ms, bytes, ok, error);
                map.insert(key.clone(), agg);
                self.url_keys.fetch_add(1, Ordering::Relaxed);
                handled = true;
            }
        }
        if handled {
            URL_KEY_CACHE.with(|cell| *cell.borrow_mut() = Some((key, shard)));
            return;
        }
        // 键已满或锁中毒：统一归入溢出桶，报告标记 URL 统计不完整
        self.url_overflow.store(true, Ordering::Relaxed);
        if let Ok(mut other) = self.url_other.lock() {
            other.update(elapsed_ms, latency_ms, bytes, ok, error);
        }
    }

    /// 收尾导出：脱敏后按 (method, url, status) 合并，溢出桶单独成行
    fn url_stats(&self) -> (Vec<UrlStatRow>, bool) {
        let mut rows: HashMap<(String, String, i64), UrlStatRow> = HashMap::new();
        let mut ingest = |method: &str, url: &str, status: i64, agg: &UrlAgg| {
            let url = sanitize_url(url);
            let row = rows
                .entry((method.to_string(), url.clone(), status))
                .or_insert_with(|| UrlStatRow {
                    method: method.to_string(),
                    url,
                    status_code: status,
                    ..UrlStatRow::default()
                });
            row.requests += agg.requests as i64;
            row.success_count += agg.success as i64;
            row.error_count += agg.errors as i64;
            row.sum_ms += agg.sum_ms;
            row.bytes = row.bytes.saturating_add(agg.bytes as i64);
            if agg.max_ms > row.max_ms {
                row.max_ms = agg.max_ms;
                row.max_at_ms = Some(agg.max_at_ms);
            }
            if let Some(message) = &agg.error_message {
                let message = sanitize_error(message);
                if !message.is_empty() && row.error_message.is_none() {
                    row.error_message = Some(message);
                }
            }
            if let Some(id) = &agg.max_request_id {
                row.max_request_id = Some(id.clone());
            }
        };
        for shard in &self.url_shards {
            if let Ok(map) = shard.lock() {
                for (key, agg) in map.iter() {
                    ingest(&key.method, &key.url, key.status as i64, agg);
                }
            }
        }
        let overflow = self.url_overflow.load(Ordering::Relaxed);
        if let Ok(other) = self.url_other.lock() {
            if other.requests > 0 {
                ingest(URL_OTHER, URL_OTHER, 0, &other);
            }
        }
        let mut items: Vec<UrlStatRow> = rows.into_values().collect();
        if overflow {
            log::warn!(
                "URL 统计键超过 {} 上限，超出部分已归入 {}，报告将标记统计不完整",
                URL_STATS_MAX_KEYS,
                URL_OTHER
            );
        }
        items.sort_by(|a, b| {
            b.requests
                .cmp(&a.requests)
                .then_with(|| a.url.cmp(&b.url))
                .then_with(|| a.status_code.cmp(&b.status_code))
        });
        (items, !overflow)
    }

    fn count(&self) -> u64 {
        self.total.load(Ordering::Relaxed)
    }

    /// 按错误类型累计，不受明细条数上限影响
    fn bump_error_type(&self, kind: &str) {
        if let Ok(mut map) = self.error_kinds.lock() {
            *map.entry(kind.to_string()).or_insert(0) += 1;
        }
    }

    fn error_types(&self) -> Vec<ErrorTypeCount> {
        let mut items: Vec<ErrorTypeCount> = self
            .error_kinds
            .lock()
            .map(|map| {
                map.iter()
                    .map(|(kind, count)| ErrorTypeCount {
                        error_type: kind.clone(),
                        count: *count,
                    })
                    .collect()
            })
            .unwrap_or_default();
        items.sort_by(|a, b| b.count.cmp(&a.count).then(a.error_type.cmp(&b.error_type)));
        items
    }

    fn avg(&self) -> f64 {
        let total = self.count();
        if total == 0 {
            0.0
        } else {
            self.sum_ms.load(Ordering::Relaxed) as f64 / total as f64
        }
    }

    fn ttfb_avg(&self) -> f64 {
        let responses = self.responses.load(Ordering::Relaxed);
        if responses == 0 {
            0.0
        } else {
            self.sum_ttfb.load(Ordering::Relaxed) as f64 / responses as f64
        }
    }

    fn min(&self) -> f64 {
        if self.count() == 0 {
            0.0
        } else {
            self.min_ms.load(Ordering::Relaxed) as f64
        }
    }

    fn max(&self) -> f64 {
        self.max_ms.load(Ordering::Relaxed) as f64
    }

    fn percentile_in(buckets: &[AtomicU64], total: u64, p: f64) -> f64 {
        if total == 0 {
            return 0.0;
        }
        let target = ((total as f64) * p).ceil() as u64;
        let mut acc = 0u64;
        for (index, bucket) in buckets.iter().enumerate() {
            let count = bucket.load(Ordering::Relaxed);
            if count == 0 {
                continue;
            }
            let before = acc;
            acc += count;
            if acc >= target {
                // 桶内按线性插值：取目标名次在桶内的中点，避免固定返回桶下界；
                // 除以 count 后减 0.5 保证结果不会越过桶上界
                let rank = target.saturating_sub(before) as f64;
                let offset = ((rank - 0.5) / count as f64).clamp(0.0, 1.0);
                return index as f64 + offset;
            }
        }
        buckets.len().saturating_sub(1) as f64
    }

    fn percentile(&self, p: f64) -> f64 {
        Self::percentile_in(&self.buckets, self.count(), p)
    }

    fn ttfb_percentile(&self, p: f64) -> f64 {
        Self::percentile_in(
            &self.ttfb_buckets,
            self.responses.load(Ordering::Relaxed),
            p,
        )
    }

    /// 由直方图计算总体标准差（按毫秒桶中值）
    fn stddev(&self) -> f64 {
        let total = self.count();
        if total < 2 {
            return 0.0;
        }
        // 用精确的延迟平方和计算样本标准差；直方图桶中值会引入 ±0.5ms 的系统误差
        let n = total as f64;
        let sum = self.sum_ms.load(Ordering::Relaxed) as f64;
        let sum_sq = self.sum_sq_ms.load(Ordering::Relaxed) as f64;
        let mean = sum / n;
        let variance = ((sum_sq - n * mean * mean) / (n - 1.0)).max(0.0);
        variance.sqrt()
    }

    /// APDEX：满意 (<T)、可容忍 (<4T)、失望 三档打分
    ///
    /// 失败请求直接计入分母（失望），不再按耗时刻度误判为「满意」。
    fn apdex(&self) -> f64 {
        let total = self.count();
        if total == 0 {
            return 0.0;
        }
        let satisfied = self.apdex_satisfied.load(Ordering::Relaxed) as f64;
        let tolerating = self.apdex_tolerating.load(Ordering::Relaxed) as f64;
        ((satisfied + tolerating / 2.0) / total as f64).clamp(0.0, 1.0)
    }

    fn status_codes(&self) -> Vec<StatusCount> {
        let mut merged: HashMap<u16, u64> = HashMap::new();
        for shard in &self.codes {
            if let Ok(map) = shard.lock() {
                for (code, count) in map.iter() {
                    *merged.entry(*code).or_insert(0) += *count;
                }
            }
        }
        let mut items: Vec<StatusCount> = merged
            .into_iter()
            .map(|(code, count)| StatusCount {
                code: code as i64,
                count: count as i64,
            })
            .collect();
        items.sort_by(|a, b| b.count.cmp(&a.count).then(a.code.cmp(&b.code)));
        items
    }

    /// 把逐毫秒直方图归并为便于阅读的区间
    fn histogram(&self) -> Vec<HistogramBucket> {
        buckets_to_ranges(&self.buckets)
    }

    /// 响应体读取耗时直方图（与总耗时共用区间）
    fn body_histogram(&self) -> Vec<HistogramBucket> {
        buckets_to_ranges(&self.body_buckets)
    }
}

/// 总耗时 / 响应体耗时共用的区间定义
const HISTOGRAM_RANGES: [(f64, f64, &str); 17] = [
    (0.0, 10.0, "≤10ms"),
    (10.0, 25.0, "10-25ms"),
    (25.0, 50.0, "25-50ms"),
    (50.0, 100.0, "50-100ms"),
    (100.0, 200.0, "100-200ms"),
    (200.0, 350.0, "200-350ms"),
    (350.0, 500.0, "350-500ms"),
    (500.0, 800.0, "0.5-0.8s"),
    (800.0, 1000.0, "0.8-1s"),
    (1000.0, 1500.0, "1-1.5s"),
    (1500.0, 2000.0, "1.5-2s"),
    (2000.0, 3000.0, "2-3s"),
    (3000.0, 5000.0, "3-5s"),
    (5000.0, 10000.0, "5-10s"),
    (10000.0, 30000.0, "10-30s"),
    (30000.0, 60000.0, "30-60s"),
    (60000.0, f64::MAX, ">60s"),
];

fn buckets_to_ranges(buckets: &[AtomicU64]) -> Vec<HistogramBucket> {
    let mut result: Vec<HistogramBucket> = HISTOGRAM_RANGES
        .iter()
        .map(|(min, max, label)| HistogramBucket {
            label: (*label).to_string(),
            min_ms: *min,
            max_ms: *max,
            count: 0,
        })
        .collect();
    for (index, bucket) in buckets.iter().enumerate() {
        let count = bucket.load(Ordering::Relaxed) as i64;
        if count == 0 {
            continue;
        }
        let value = index as f64;
        for item in result.iter_mut() {
            if value >= item.min_ms && value < item.max_ms {
                item.count += count;
                break;
            }
        }
    }
    result
}

/// 稀疏桶转区间直方图：集群合并后复用同一套区间，报告排版不变
pub(crate) fn histogram_from_sparse(buckets: &[(u32, u64)]) -> Vec<HistogramBucket> {
    let mut result: Vec<HistogramBucket> = HISTOGRAM_RANGES
        .iter()
        .map(|(min, max, label)| HistogramBucket {
            label: (*label).to_string(),
            min_ms: *min,
            max_ms: *max,
            count: 0,
        })
        .collect();
    for (ms, count) in buckets {
        let value = *ms as f64;
        for item in result.iter_mut() {
            if value >= item.min_ms && value < item.max_ms {
                item.count += *count as i64;
                break;
            }
        }
    }
    result
}

/// 运行时调度：阶梯加压时调整生效并发，限速时按秒发放令牌
struct Control {
    active: AtomicU32,
    rate: AtomicU32,
    tokens: AtomicU64,
    stage: std::sync::atomic::AtomicI64,
    /// 阶梯切换唤醒：未进入生效区间的 worker 不再每 50ms 轮询
    wake: tokio::sync::Notify,
    /// 令牌补充唤醒：限速时等待中的 worker 不再毫秒级空转
    token_wake: tokio::sync::Notify,
}

impl Control {
    fn new(active: u32, rate: u32) -> Self {
        Self {
            active: AtomicU32::new(active),
            rate: AtomicU32::new(rate),
            tokens: AtomicU64::new(rate as u64),
            stage: std::sync::atomic::AtomicI64::new(0),
            wake: tokio::sync::Notify::new(),
            token_wake: tokio::sync::Notify::new(),
        }
    }
}

/// Token 刷新前瞻窗口：距过期不足该值即提前刷新（8.6）
const POOL_REFRESH_AHEAD_MS: i64 = 30_000;

/// 刷新写入的列名：含 token 列优先，否则首列；列定义为空时退回 value
pub(crate) fn refresh_column_of(columns: &[String]) -> String {
    if columns.iter().any(|column| column == "token") {
        "token".to_string()
    } else {
        columns
            .first()
            .cloned()
            .unwrap_or_else(|| "value".to_string())
    }
}

/// 绑定轮换模式：空视为 per_vu
pub(crate) fn binding_mode(binding: &PoolBinding) -> &str {
    let mode = binding.mode.trim();
    if mode.is_empty() {
        "per_vu"
    } else {
        mode
    }
}

/// 运行期条目：值可被刷新替换，带过期 / 冷却 / 失效状态
struct RuntimeItem {
    /// 池条目 id：集群刷新按它在主从间对齐同一条目
    id: i64,
    value: String,
    /// 整行数据（v20）：列名 -> 值；旧池为空
    data: HashMap<String, String>,
    /// 刷新写入的列：含 token 列用 token，否则用首列；无列定义时为 value
    refresh_column: String,
    account: Option<String>,
    expires_at: i64,
    fail_count: u32,
    cooldown_until: i64,
    invalid: bool,
}

impl RuntimeItem {
    fn from_pool_item(item: PoolItem, columns: &[String]) -> Self {
        Self {
            id: item.id,
            value: item.value,
            data: item.data,
            refresh_column: refresh_column_of(columns),
            account: item.account,
            expires_at: item.expires_at,
            fail_count: 0,
            cooldown_until: 0,
            invalid: false,
        }
    }

    /// 可用：未失效、值非空、未过期、不在冷却期
    fn available(&self, now: i64) -> bool {
        !self.invalid
            && !self.value.trim().is_empty()
            && (self.expires_at == 0 || self.expires_at > now)
            && self.cooldown_until <= now
    }
}

/// 把一条绑定作用到本次变量：`*` 注入全部非空列，`""` 注入 value，其它注入指定列
fn apply_binding_vars(
    vars: &mut HashMap<String, String>,
    binding: &PoolBinding,
    item: &RuntimeItem,
) {
    let column = binding.column.trim();
    if column == "*" {
        for (name, value) in &item.data {
            if !name.trim().is_empty() && !value.trim().is_empty() {
                vars.insert(name.clone(), value.clone());
            }
        }
        return;
    }
    let target = binding.target.trim();
    if target.is_empty() {
        return;
    }
    if column.is_empty() {
        vars.insert(target.to_string(), item.value.clone());
    } else if let Some(value) = item.data.get(column) {
        if !value.trim().is_empty() {
            vars.insert(target.to_string(), value.clone());
        }
    }
}

/// 单个池的运行期状态
struct PoolState {
    items: Vec<RuntimeItem>,
    /// 池列定义（v20）：用于 "*" 自动注入与刷新列选择；旧池为空
    columns: Vec<String>,
    /// Token 刷新配置；None 表示该池未启用 http 刷新
    refresh: Option<crate::services::pool::PoolRefreshConfig>,
}

/// 一次请求的池覆盖层
struct PoolOverlay {
    vars: HashMap<String, String>,
    /// 本次实际选中的条目（池名, 下标），401 时按它刷新与重试
    picks: Vec<(String, usize)>,
}

/// 集群 token 刷新中继：从节点把刷新请求交给主节点单飞（8.10.5）
///
/// 未启用集群时为 None，走本机 HTTP 刷新；启用后实现方负责 5 秒超时与失败语义。
/// 刷新请求的返回值：新值、条目 ID、可选的下一轮提示
pub(crate) type RefreshOutput = Result<(String, i64, Option<String>), String>;

/// 中继方返回的 future：必须 Send 且可借用到调用期，trait 里只能写成 Pin<Box<...>>
pub(crate) type RefreshFuture<'a> =
    std::pin::Pin<Box<dyn std::future::Future<Output = RefreshOutput> + Send + 'a>>;

pub(crate) trait PoolRefreshRelay: Send + Sync {
    fn refresh<'a>(&'a self, pool: &'a str, item_id: i64, item_index: usize) -> RefreshFuture<'a>;
}

/// 运行期池：按 VU / 按请求从池中取值，注入到同名占位符
struct PoolRuntime {
    bindings: Vec<PoolBinding>,
    pools: parking_lot::Mutex<HashMap<String, PoolState>>,
    /// 每条条目一把异步锁（key = 池名，下标 = 条目位置）：单飞刷新
    refresh_locks: HashMap<String, Vec<Arc<AsyncMutex<()>>>>,
    /// per_request / round_robin 游标（key = 池名，同一池的多条绑定共用）
    cursors: parking_lot::Mutex<HashMap<String, usize>>,
    /// random 模式的伪随机计数器（每节点一个，splitmix64 打散，避免同纳秒撞条）
    rng: AtomicU64,
    /// 无 HTTP 刷新时的 401 轮换手位（key = 池名）
    refresh: parking_lot::Mutex<HashMap<String, usize>>,
    /// 集群切片：本节点在全局 VU 轴上的起点（key = 池名）
    offsets: HashMap<String, usize>,
    /// 报告统计：每条被使用的次数（key = 池名，下标 = 条目位置）
    stat_items: HashMap<String, Vec<AtomicU64>>,
    /// 报告统计：刷新成功次数（key = 池名）
    stat_refresh: parking_lot::Mutex<HashMap<String, u64>>,
    /// 报告统计：401/403 次数（key = 池名）
    stat_unauthorized: parking_lot::Mutex<HashMap<String, u64>>,
    /// 集群刷新中继：Some 时刷新走主节点单飞，不再本机发 HTTP
    relay: Option<Arc<dyn PoolRefreshRelay>>,
}

impl PoolRuntime {
    fn new(
        bindings: Vec<PoolBinding>,
        items: HashMap<String, Vec<PoolItem>>,
        columns: HashMap<String, Vec<String>>,
        refresh: HashMap<String, crate::services::pool::PoolRefreshConfig>,
    ) -> Self {
        let mut pools = HashMap::new();
        let mut stat_items = HashMap::new();
        let mut refresh_locks = HashMap::new();
        for (name, list) in items {
            let len = list.len();
            let config = refresh.get(&name).cloned();
            let pool_columns = columns.get(&name).cloned().unwrap_or_default();
            if let Some(config) = config.as_ref() {
                if config.is_http() {
                    refresh_locks.insert(
                        name.clone(),
                        (0..len).map(|_| Arc::new(AsyncMutex::new(()))).collect(),
                    );
                }
            }
            stat_items.insert(
                name.clone(),
                (0..len).map(|_| AtomicU64::new(0)).collect::<Vec<_>>(),
            );
            pools.insert(
                name,
                PoolState {
                    items: list
                        .into_iter()
                        .map(|item| RuntimeItem::from_pool_item(item, &pool_columns))
                        .collect(),
                    columns: pool_columns,
                    refresh: config,
                },
            );
        }
        Self {
            bindings,
            pools: parking_lot::Mutex::new(pools),
            refresh_locks,
            cursors: parking_lot::Mutex::new(HashMap::new()),
            rng: AtomicU64::new(0x243F_6A88_85A3_08D3),
            refresh: parking_lot::Mutex::new(HashMap::new()),
            offsets: HashMap::new(),
            stat_items,
            stat_refresh: parking_lot::Mutex::new(HashMap::new()),
            stat_unauthorized: parking_lot::Mutex::new(HashMap::new()),
            relay: None,
        }
    }

    /// splitmix64：无锁递增再打散，保证高并发下不会在同一时刻撞同一条
    fn next_random(&self) -> usize {
        let seed = self.rng.fetch_add(0x9E37_79B9_7F4A_7C15, Ordering::Relaxed);
        let mut value = seed;
        value = (value ^ (value >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        value ^= value >> 31;
        value as usize
    }

    /// 写进报告 `config_json.pools`：只含池名、条目数与计数，不含任何明文
    fn report_json(&self) -> serde_json::Value {
        let now = now_ms();
        let pools = self.pools.lock();
        let refresh = self.stat_refresh.lock();
        let unauthorized = self.stat_unauthorized.lock();
        let mut seen = std::collections::HashSet::new();
        let pools_json: Vec<serde_json::Value> = self
            .bindings
            .iter()
            .filter(|binding| seen.insert(binding.pool.trim().to_string()))
            .map(|binding| {
                let name = binding.pool.trim();
                let state = pools.get(name);
                let total = state.map(|state| state.items.len()).unwrap_or(0);
                let invalid = state
                    .map(|state| {
                        state
                            .items
                            .iter()
                            .filter(|item| item.invalid || item.value.trim().is_empty())
                            .count()
                    })
                    .unwrap_or(0);
                let cooling = state
                    .map(|state| {
                        state
                            .items
                            .iter()
                            .filter(|item| !item.invalid && item.cooldown_until > now)
                            .count()
                    })
                    .unwrap_or(0);
                let counts = self.stat_items.get(name);
                let used = counts
                    .map(|list| {
                        list.iter()
                            .filter(|count| count.load(Ordering::Relaxed) > 0)
                            .count()
                    })
                    .unwrap_or(0);
                let requests = counts
                    .map(|list| {
                        list.iter()
                            .map(|count| count.load(Ordering::Relaxed))
                            .sum::<u64>()
                    })
                    .unwrap_or(0);
                let per_item: Vec<serde_json::Value> = counts
                    .map(|list| {
                        // 只保留请求数最高的 TopN：条目明细仅用于 Excel，避免大池把 config_json 撑大
                        let mut top: Vec<(usize, u64)> = list
                            .iter()
                            .enumerate()
                            .filter_map(|(index, count)| {
                                let requests = count.load(Ordering::Relaxed);
                                (requests > 0).then_some((index, requests))
                            })
                            .collect();
                        top.sort_by_key(|(_, requests)| std::cmp::Reverse(*requests));
                        top.truncate(crate::services::pool::PER_ITEM_TOP);
                        top.sort_by_key(|(index, _)| *index);
                        top.into_iter()
                            .map(|(index, requests)| {
                                json!({ "index": index + 1, "requests": requests })
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                json!({
                    "name": name,
                    "items": total,
                    "used": used,
                    "requests": requests,
                    "refreshed": refresh.get(name).copied().unwrap_or(0),
                    "unauthorized": unauthorized.get(name).copied().unwrap_or(0),
                    "invalid": invalid,
                    "cooling": cooling,
                    "exhausted": total > 0 && used >= total,
                    "perItem": per_item,
                })
            })
            .collect();
        json!(pools_json)
    }

    fn with_offsets(mut self, offsets: HashMap<String, usize>) -> Self {
        self.offsets = offsets;
        self
    }

    /// 注入集群刷新中继：Some 时刷新交由主节点单飞
    fn with_relay(mut self, relay: Option<Arc<dyn PoolRefreshRelay>>) -> Self {
        self.relay = relay;
        self
    }

    /// 运行期会自动注入的变量名：`*` 展开为池列名，其它绑定取 target
    fn injected_names(&self) -> Vec<String> {
        let pools = self.pools.lock();
        let mut names: Vec<String> = Vec::new();
        for binding in &self.bindings {
            let pool = binding.pool.trim();
            if binding.column.trim() == "*" {
                if let Some(state) = pools.get(pool) {
                    for column in &state.columns {
                        if !column.trim().is_empty() && !names.iter().any(|item| item == column) {
                            names.push(column.clone());
                        }
                    }
                }
            } else {
                let target = binding.target.trim();
                if !target.is_empty() && !names.iter().any(|item| item == target) {
                    names.push(target.to_string());
                }
            }
        }
        names
    }

    /// 为本次请求生成变量覆盖层；过期前先做单飞刷新
    ///
    /// 按池分组：同一池的多个绑定每次只选同一条条目，避免账号与密码来自不同行。
    async fn overlay(
        &self,
        client: &reqwest::Client,
        base_vars: &HashMap<String, String>,
        vu: u32,
    ) -> Option<PoolOverlay> {
        if self.bindings.is_empty() {
            return None;
        }
        let now = now_ms();
        // 按池分组（保持绑定顺序）：同一池的多条绑定共用本次选中的条目
        let mut grouped: Vec<(String, Vec<&PoolBinding>)> = Vec::new();
        for binding in &self.bindings {
            let pool = binding.pool.trim().to_string();
            if let Some((_, group)) = grouped.iter_mut().find(|(name, _)| *name == pool) {
                group.push(binding);
            } else {
                grouped.push((pool, vec![binding]));
            }
        }
        // (池名, 条目下标)
        let mut picks: Vec<(String, usize)> = Vec::new();
        let mut stale: Vec<(String, usize)> = Vec::new();
        {
            let pools = self.pools.lock();
            let mut cursors = self.cursors.lock();
            let refresh = self.refresh.lock();
            for (pool, group) in grouped.iter() {
                let Some(state) = pools.get(pool.as_str()) else {
                    continue;
                };
                if state.items.is_empty() {
                    continue;
                }
                // 优先在可用条目里选；全部不可用时退回未失效条目，保证压测继续
                let mut candidates: Vec<usize> = (0..state.items.len())
                    .filter(|position| state.items[*position].available(now))
                    .collect();
                if candidates.is_empty() {
                    candidates = (0..state.items.len())
                        .filter(|position| !state.items[*position].invalid)
                        .collect();
                }
                if candidates.is_empty() {
                    candidates = (0..state.items.len()).collect();
                }
                let len = candidates.len();
                let mode = binding_mode(group[0]);
                // 游标 / 集群偏移都按池计算一次：同一池的多条绑定共用同一条身份
                let cursor_key = pool.clone();
                let base = match mode {
                    "per_request" | "sequence" | "round_robin" => {
                        let entry = cursors.entry(cursor_key).or_insert(0);
                        let value = *entry;
                        *entry = entry.wrapping_add(1);
                        value % len
                    }
                    "random" => self.next_random() % len,
                    // per_vu：同一 VU 在整个压测生命周期固定同一条；集群按全局 VU 偏移
                    _ => {
                        (vu as usize + self.offsets.get(pool.as_str()).copied().unwrap_or(0)) % len
                    }
                };
                // 401 换条只做整体平移，不接管轮换：否则 per_request 会退化成 per_vu
                let shift = refresh.get(pool.as_str()).copied().unwrap_or(0);
                let position = (base + shift) % len;
                let picked = candidates[position % len];
                picks.push((pool.clone(), picked));
                if let Some(config) = state.refresh.as_ref() {
                    if config.is_http() {
                        let item = &state.items[picked];
                        if item.expires_at > 0 && item.expires_at - now <= POOL_REFRESH_AHEAD_MS {
                            stale.push((pool.clone(), picked));
                        }
                    }
                }
            }
        }
        // 过期前主动刷新（逐条单飞）；失败不影响本次请求继续，401 后再兜底
        for (pool, index) in stale {
            let _ = self.refresh_item(client, base_vars, &pool, index).await;
        }
        // 池值只覆盖同名变量，环境变量 / 运行期变量仍然可用
        let mut vars = base_vars.clone();
        {
            let pools = self.pools.lock();
            let picked: HashMap<&str, usize> = picks
                .iter()
                .map(|(pool, index)| (pool.as_str(), *index))
                .collect();
            for (pool, group) in &grouped {
                let Some(index) = picked.get(pool.as_str()) else {
                    continue;
                };
                let Some(state) = pools.get(pool.as_str()) else {
                    continue;
                };
                let Some(item) = state.items.get(*index) else {
                    continue;
                };
                for binding in group {
                    apply_binding_vars(&mut vars, binding, item);
                }
                // 同一池的条目使用计数只加一次
                if let Some(counts) = self.stat_items.get(pool.as_str()) {
                    if let Some(count) = counts.get(*index) {
                        count.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
        }
        if vars.is_empty() {
            None
        } else {
            Some(PoolOverlay { vars, picks })
        }
    }

    /// 401/403：有 http 配置时单飞刷新并允许重试一次；否则沿用切下一条的老行为
    async fn refresh_after_unauthorized(
        &self,
        client: &reqwest::Client,
        base_vars: &HashMap<String, String>,
        picks: &[(String, usize)],
    ) -> bool {
        let mut retry = false;
        let mut seen = std::collections::HashSet::new();
        for (pool, index) in picks {
            if !seen.insert(pool.clone()) {
                continue;
            }
            let refresh_on_401 = self
                .bindings
                .iter()
                .any(|binding| binding.pool.trim() == pool && binding.refresh_on_401);
            if !refresh_on_401 {
                continue;
            }
            *self
                .stat_unauthorized
                .lock()
                .entry(pool.clone())
                .or_insert(0) += 1;
            let is_http = self
                .pools
                .lock()
                .get(pool)
                .and_then(|state| state.refresh.as_ref())
                .map(|config| config.is_http())
                .unwrap_or(false);
            if is_http {
                if self.refresh_item(client, base_vars, pool, *index).await {
                    retry = true;
                }
            } else {
                // 无刷新配置：该条 401 视为失败，累计后冷却 / 失效，再切下一条
                self.mark_failure(pool, *index);
                let len = self
                    .pools
                    .lock()
                    .get(pool)
                    .map(|state| state.items.len())
                    .unwrap_or(0);
                if len > 1 {
                    let mut refresh = self.refresh.lock();
                    let entry = refresh.entry(pool.clone()).or_insert(0);
                    *entry = entry.wrapping_add(1);
                    *self.stat_refresh.lock().entry(pool.clone()).or_insert(0) += 1;
                    retry = true;
                }
            }
        }
        retry
    }

    /// 本次请求涉及的池里是否有启用 http 刷新的（用于区分刷新失败与未配置）
    fn http_refresh_enabled(&self, picks: &[(String, usize)]) -> bool {
        if !picks.iter().any(|(pool, _)| {
            self.bindings
                .iter()
                .any(|binding| binding.pool.trim() == pool && binding.refresh_on_401)
        }) {
            return false;
        }
        let pools = self.pools.lock();
        picks.iter().any(|(pool, _)| {
            pools
                .get(pool)
                .and_then(|state| state.refresh.as_ref())
                .map(|config| config.is_http())
                .unwrap_or(false)
        })
    }

    /// 单条 token 的单飞刷新：锁内二次检查，避免大量 VU 同时打刷新接口
    async fn refresh_item(
        &self,
        client: &reqwest::Client,
        base_vars: &HashMap<String, String>,
        pool: &str,
        index: usize,
    ) -> bool {
        let Some(lock) = self
            .refresh_locks
            .get(pool)
            .and_then(|list| list.get(index))
        else {
            return false;
        };
        let _guard = lock.lock().await;
        let (config, item_id, current_value, refresh_column, account, expires_at, invalid) = {
            let pools = self.pools.lock();
            let Some(state) = pools.get(pool) else {
                return false;
            };
            let Some(item) = state.items.get(index) else {
                return false;
            };
            let Some(config) = state.refresh.as_ref().filter(|config| config.is_http()) else {
                return false;
            };
            // 刷新 token 取刷新列；该列为空时退回 value（兼容旧池）
            let token = item
                .data
                .get(&item.refresh_column)
                .filter(|value| !value.trim().is_empty())
                .cloned()
                .unwrap_or_else(|| item.value.clone());
            (
                config.clone(),
                item.id,
                token,
                item.refresh_column.clone(),
                item.account.clone(),
                item.expires_at,
                item.invalid,
            )
        };
        let now = now_ms();
        // 二次检查：进入锁前可能已被其它 VU 刷新
        if !invalid && expires_at != 0 && expires_at - now > POOL_REFRESH_AHEAD_MS {
            return true;
        }
        // 集群：交给主节点单飞，节点不再自行打刷新接口，避免互相失效
        if let Some(relay) = self.relay.clone() {
            let result = match tokio::time::timeout(
                Duration::from_secs(5),
                relay.refresh(pool, item_id, index),
            )
            .await
            {
                Ok(result) => result,
                Err(_) => Err("主节点刷新超时（5 秒）".to_string()),
            };
            return match result {
                Ok((value, expires_at, column)) => {
                    self.apply_refreshed(pool, index, value, expires_at, column.as_deref());
                    true
                }
                Err(error) => {
                    log::warn!("池「{pool}」token 刷新失败：{error}");
                    self.mark_failure(pool, index);
                    false
                }
            };
        }
        match refresh_http(
            client,
            &config,
            base_vars,
            &current_value,
            account.as_deref(),
        )
        .await
        {
            Ok((value, expires_at)) => {
                self.apply_refreshed(pool, index, value, expires_at, Some(&refresh_column));
                true
            }
            Err(error) => {
                log::warn!("池「{pool}」token 刷新失败：{error}");
                self.mark_failure(pool, index);
                false
            }
        }
    }

    /// 刷新成功：写回内存条目（value 与刷新列）并累计刷新计数
    fn apply_refreshed(
        &self,
        pool: &str,
        index: usize,
        value: String,
        expires_at: i64,
        column: Option<&str>,
    ) {
        if let Some(state) = self.pools.lock().get_mut(pool) {
            if let Some(item) = state.items.get_mut(index) {
                item.value = value.clone();
                let column = column
                    .map(str::trim)
                    .filter(|column| !column.is_empty())
                    .map(str::to_string)
                    .unwrap_or_else(|| item.refresh_column.clone());
                if !column.is_empty() {
                    item.data.insert(column, value);
                }
                item.expires_at = expires_at;
                item.fail_count = 0;
                item.cooldown_until = 0;
                item.invalid = false;
            }
        }
        *self
            .stat_refresh
            .lock()
            .entry(pool.to_string())
            .or_insert(0) += 1;
    }

    /// 刷新失败：指数退避冷却，连续失败 3 次标记失效
    fn mark_failure(&self, pool: &str, index: usize) {
        let now = now_ms();
        if let Some(state) = self.pools.lock().get_mut(pool) {
            if let Some(item) = state.items.get_mut(index) {
                item.fail_count = item.fail_count.saturating_add(1);
                let backoff = (1i64 << item.fail_count.min(5)).saturating_mul(1000);
                item.cooldown_until = now + backoff.min(60_000);
                if item.fail_count >= 3 {
                    item.invalid = true;
                }
            }
        }
    }
}

/// 执行一次 token 刷新 HTTP 请求并解析结果
///
/// 单机由 `PoolRuntime` 直接调用；集群模式下主节点统一调用（见 `cluster::pool_refresh`），
/// 保证 `refreshToken` 轮换只发生一次。
pub(crate) async fn refresh_http(
    client: &reqwest::Client,
    config: &crate::services::pool::PoolRefreshConfig,
    base_vars: &HashMap<String, String>,
    token: &str,
    account: Option<&str>,
) -> Result<(String, i64), String> {
    let mut vars = base_vars.clone();
    vars.insert("token".to_string(), token.to_string());
    vars.insert("refreshToken".to_string(), token.to_string());
    if let Some(account) = account {
        vars.insert("account".to_string(), account.to_string());
    }
    let url_text = render(&config.url, &vars);
    let url = reqwest::Url::parse(&url_text).map_err(|_| format!("刷新地址无效：{url_text}"))?;
    let method = reqwest::Method::from_bytes(config.method.trim().as_bytes())
        .unwrap_or(reqwest::Method::POST);
    let mut builder = client.request(method, url).timeout(Duration::from_secs(15));
    for header in &config.headers {
        if !header.enabled {
            continue;
        }
        builder = builder.header(render(&header.key, &vars), render(&header.value, &vars));
    }
    if !config.body.trim().is_empty() {
        builder = builder.body(render(&config.body, &vars));
    }
    let response = builder.send().await.map_err(|error| error.to_string())?;
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(format!("HTTP {}", status.as_u16()));
    }
    parse_refresh_response(&text, config, now_ms())
}

/// 解析刷新响应：token 走 JSON 路径，过期时间兼容秒 / 毫秒
fn parse_refresh_response(
    text: &str,
    config: &crate::services::pool::PoolRefreshConfig,
    now: i64,
) -> Result<(String, i64), String> {
    let value: serde_json::Value =
        serde_json::from_str(text).map_err(|error| format!("响应不是 JSON：{error}"))?;
    let token = json_path(&value, &config.token_path)
        .and_then(|item| item.as_str())
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .ok_or_else(|| format!("响应缺少 token（路径 {}）", config.token_path))?
        .to_string();
    let expires_at = json_path(&value, &config.expires_path)
        .and_then(|item| item.as_f64())
        .map(|seconds| {
            if seconds <= 0.0 {
                0
            } else if seconds < 1e12 {
                now + (seconds * 1000.0) as i64
            } else {
                seconds as i64
            }
        })
        .unwrap_or_else(|| {
            if config.expires_in_sec > 0 {
                now + config.expires_in_sec * 1000
            } else {
                0
            }
        });
    Ok((token, expires_at))
}

/// 极简 JSON 路径：支持 `$.data.token`、`data.list[0].token`
fn json_path<'a>(value: &'a serde_json::Value, path: &str) -> Option<&'a serde_json::Value> {
    let path = path.trim();
    if path.is_empty() || path == "$" {
        return Some(value);
    }
    let path = path.strip_prefix('$').unwrap_or(path);
    let path = path.strip_prefix('.').unwrap_or(path);
    let mut current = value;
    for raw in path.split('.') {
        if raw.is_empty() {
            continue;
        }
        let mut name = raw;
        let mut indices = Vec::new();
        if let Some(start) = raw.find('[') {
            name = &raw[..start];
            let mut rest = &raw[start..];
            while let Some(open) = rest.find('[') {
                let Some(close) = rest.find(']') else {
                    break;
                };
                if let Ok(index) = rest[open + 1..close].parse::<usize>() {
                    indices.push(index);
                }
                rest = &rest[close + 1..];
            }
        }
        if !name.is_empty() {
            current = current.get(name)?;
        }
        for index in indices {
            current = current.get(index)?;
        }
    }
    Some(current)
}

/// 启动时收齐的单个接口：模板已渲染、Cookie 已预取、发送字节已估算
struct PreparedRequest {
    request_id: Option<i64>,
    name: String,
    method: reqwest::Method,
    /// URL 模板原文：报告里保留「用户配置的目标」
    url_template: String,
    /// 渲染后的完整地址（含 Query 与 API Key），错误回显与兜底展示用
    url_text: String,
    headers: Vec<KeyValueItem>,
    params: Vec<KeyValueItem>,
    body_type: String,
    body: Option<String>,
    /// 池命中后需要按运行期变量重新渲染的认证原始配置
    raw_auth: Option<RequestAuth>,
    /// 启动时渲染好的认证配置
    auth: RequestAuth,
    /// 启动时按目标 URL 预取的本机 Cookie
    cookie: Option<String>,
    /// 单请求发送字节（请求行 + 头 + 体）
    req_bytes: u64,
    /// 流量权重（已归一为 > 0）
    weight: f64,
}

/// 生效的接口列表：多接口配置为空时，把单接口字段归一成一条，兼容旧计划。
fn effective_requests(config: &StressConfig) -> Vec<StressRequestDef> {
    if !config.requests.is_empty() {
        return config.requests.clone();
    }
    vec![StressRequestDef {
        request_id: config.request_id,
        name: config.name.clone().unwrap_or_default(),
        method: config.method.clone(),
        url: config.url.clone(),
        headers: config.headers.clone(),
        params: config.params.clone(),
        body_type: config.body_type.clone(),
        body: config.body.clone(),
        auth: config.auth.clone(),
        weight: Some(1.0),
    }]
}

/// 权重归一：非正数视为「未设置」，只要有一个正权重就按 1 参与分配；
/// 全部未设置时平均分配。返回值之和为 1。
fn normalize_request_weights(raw: &[f64]) -> Vec<f64> {
    if raw.is_empty() {
        return Vec::new();
    }
    let any_positive = raw.iter().any(|value| *value > 0.0);
    let weights: Vec<f64> = if any_positive {
        raw.iter()
            .map(|value| if *value > 0.0 { *value } else { 1.0 })
            .collect()
    } else {
        vec![1.0; raw.len()]
    };
    let sum: f64 = weights.iter().sum();
    weights.into_iter().map(|value| value / sum).collect()
}

struct RunCtx {
    client: reqwest::Client,
    config: StressConfig,
    vars: HashMap<String, String>,
    /// 参数/Token 池运行期状态；无池时为 None
    pools: Option<Arc<PoolRuntime>>,
    /// 本次压测的接口列表：单接口时长度为 1
    requests: Vec<PreparedRequest>,
    /// 权重总和，按权重抽样时避免每次求和
    total_weight: f64,
    metrics: Arc<Metrics>,
    control: Arc<Control>,
    start: Instant,
    deadline: Instant,
    cancel: Arc<AtomicBool>,
    errors: mpsc::Sender<ReportError>,
    records: mpsc::UnboundedSender<ReportRequestRecord>,
    /// TopN 慢请求：轻量字段，不复制请求/响应内容；有界队列，满即丢弃
    slow_tx: mpsc::Sender<SlowRequest>,
    /// 保留的慢请求条数上限（0 = 不采集）
    slow_limit: usize,
    /// 只记录慢于该阈值的请求（0 = 不设阈值）
    slow_threshold_ms: u64,
    /// 明细采集开关，auto 降档时会运行期修改
    capture: Arc<AtomicU8>,
}

struct Attempt {
    status: u16,
    ok: bool,
    url: String,
    error: Option<String>,
    snippet: String,
    ttfb_ms: u64,
    /// 响应体读取耗时（毫秒）；重试场景只算最后一次响应
    body_ms: u64,
    bytes: u64,
    body: Option<String>,
    /// 查询参数（query string）
    params: String,
    /// 请求头 [[key, value], ...] 序列化后的 JSON
    headers: String,
    /// 响应头 JSON
    response_headers: Option<String>,
    /// 响应体（截断）
    response_body: Option<String>,
    /// 本次请求实际选中的池条目（池名, 下标）；401 时按它刷新
    picks: Vec<(String, usize)>,
}

fn headers_json(pairs: &[(String, String)]) -> String {
    if pairs.is_empty() {
        return String::new();
    }
    serde_json::to_string(pairs).unwrap_or_default()
}

/// 认证配置在启动时渲染一次，避免每个请求重复解析模板
fn render_auth(auth: &RequestAuth, vars: &HashMap<String, String>) -> RequestAuth {
    RequestAuth {
        kind: auth.kind.clone(),
        token: render(&auth.token, vars),
        username: render(&auth.username, vars),
        password: render(&auth.password, vars),
        key: render(&auth.key, vars),
        value: render(&auth.value, vars),
        add_to: auth.add_to.clone(),
        token_url: render(&auth.token_url, vars),
        client_id: render(&auth.client_id, vars),
        client_secret: render(&auth.client_secret, vars),
        scope: render(&auth.scope, vars),
    }
}

/// 按 URL 模板与参数表渲染目标地址：多接口场景每个接口各调一次
fn build_url_for(
    url_template: &str,
    params: &[KeyValueItem],
    vars: &HashMap<String, String>,
    auth: &RequestAuth,
) -> Result<reqwest::Url, String> {
    let mut raw = render(url_template.trim(), vars).trim().to_string();
    if raw.is_empty() {
        return Err("URL 不能为空".to_string());
    }
    // 残留模板不能交给 reqwest：'{{' 在 URL 里会被编码或直接失败，
    // 表现为每个请求都报错却看不出原因，这里直接拒绝并点名变量。
    let mut missing = find_unresolved(&raw);
    if !missing.is_empty() {
        return Err(format!("URL 仍有未解析变量：{}", missing.join("、")));
    }
    if !raw.contains("://") {
        raw = format!("http://{raw}");
    }
    let mut url = reqwest::Url::parse(&raw).map_err(|e| format!("URL 无法解析：{e}"))?;
    if !params.is_empty() {
        let mut pairs = url.query_pairs_mut();
        for item in params {
            if !item.enabled {
                continue;
            }
            let key = render(item.key.trim(), vars);
            if key.is_empty() {
                continue;
            }
            let value = render(&item.value, vars);
            for name in find_unresolved(&key)
                .into_iter()
                .chain(find_unresolved(&value))
            {
                if !missing.iter().any(|item| item == &name) {
                    missing.push(name);
                }
            }
            pairs.append_pair(&key, &value);
        }
    }
    if !missing.is_empty() {
        return Err(format!("URL 参数仍有未解析变量：{}", missing.join("、")));
    }
    // API Key 认证可以放在查询参数上（auth 已渲染，无需再次替换变量）
    if auth.kind == "apikey" && auth.add_to == "query" && !auth.key.trim().is_empty() {
        let mut pairs = url.query_pairs_mut();
        pairs.append_pair(auth.key.trim(), &auth.value);
    }
    Ok(url)
}

/// 读取响应体：**始终排空**以保证连接可复用，按需只保留前 `keep` 个字节
///
/// 返回（保留的内容, 实际字节数）。
/// - `keep = Some(n)`：明细模式，保留前 n 字节供快照使用；
/// - `keep = None`：高吞吐模式，只计数不保留。
///
/// 旧实现读满 2000 字节即中断，未读完的 body 会让 hyper 关闭连接，
/// 于是在高 QPS 下退化成「每请求一次 TCP 握手」，并把流量统计封顶在 2000。
async fn read_body(mut response: reqwest::Response, keep: Option<usize>) -> (String, u64) {
    let declared = response
        .headers()
        .get(reqwest::header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0);

    let limit = keep.unwrap_or(0);
    let mut buffer: Vec<u8> = Vec::with_capacity(limit.min(256));
    let mut drained: u64 = 0;
    // 读到 None 或出错都停止；出错时已经读到的字节仍然可用
    while let Ok(Some(chunk)) = response.chunk().await {
        drained += chunk.len() as u64;
        if buffer.len() < limit {
            let take = (limit - buffer.len()).min(chunk.len());
            buffer.extend_from_slice(&chunk[..take]);
        }
    }
    // 有 Content-Length 时以声明值为准（gzip 会自动解压并移除该头，此时按解压后字节计）
    let bytes = if declared > 0 { declared } else { drained };
    (String::from_utf8_lossy(&buffer).into_owned(), bytes)
}

async fn send_once(ctx: &RunCtx, req: &PreparedRequest, capture: Capture, vu: u32) -> Attempt {
    // 是否需要为明细采集请求/响应内容
    let keep_detail = capture != Capture::Off;
    // 池覆盖层：per_vu 时同一 VU 固定同一条，per_request 时逐请求轮换；
    // 临近过期的 token 会在这里先做单飞刷新
    let pool_overlay = match ctx.pools.as_ref() {
        Some(pools) => pools.overlay(&ctx.client, &ctx.vars, vu).await,
        None => None,
    };
    let vars = pool_overlay
        .as_ref()
        .map(|item| &item.vars)
        .unwrap_or(&ctx.vars);
    let picks = pool_overlay
        .as_ref()
        .map(|item| item.picks.clone())
        .unwrap_or_default();
    let auth = if pool_overlay.is_some() {
        req.raw_auth
            .clone()
            .map(|item| render_auth(&item, vars))
            .unwrap_or_default()
    } else {
        req.auth.clone()
    };
    let url = match build_url_for(&req.url_template, &req.params, vars, &auth) {
        Ok(url) => url,
        Err(error) => {
            return Attempt {
                status: 0,
                ok: false,
                url: req.url_template.clone(),
                error: Some(error),
                snippet: String::new(),
                ttfb_ms: 0,
                body_ms: 0,
                bytes: 0,
                body: None,
                params: String::new(),
                headers: String::new(),
                response_headers: None,
                response_body: None,
                picks,
            }
        }
    };
    let query = if keep_detail {
        url.query().unwrap_or_default().to_string()
    } else {
        String::new()
    };
    let url_text = url.to_string();

    let mut builder = ctx
        .client
        .request(req.method.clone(), url)
        .timeout(Duration::from_millis(ctx.config.timeout_ms.max(500)));

    // 只有需要落明细时才组装 header 快照，off 模式省掉这些分配
    let mut header_pairs: Vec<(String, String)> = Vec::new();
    let mut has_content_type = false;
    let mut has_authorization = false;
    let mut has_cookie = false;
    for item in &req.headers {
        if !item.enabled {
            continue;
        }
        let key = render(item.key.trim(), vars);
        if key.is_empty() {
            continue;
        }
        if key.eq_ignore_ascii_case("content-type") {
            has_content_type = true;
        }
        if key.eq_ignore_ascii_case("authorization") {
            has_authorization = true;
        }
        if key.eq_ignore_ascii_case("cookie") {
            has_cookie = true;
        }
        let value = render(&item.value, vars);
        if keep_detail {
            header_pairs.push((key.clone(), value.clone()));
        }
        builder = builder.header(key, value);
    }

    // 认证头：用户手动配置的 Authorization 优先，与手工请求保持一致
    if !has_authorization {
        match auth.kind.as_str() {
            "bearer" | "oauth2" if !auth.token.trim().is_empty() => {
                let value = format!("Bearer {}", auth.token.trim());
                if keep_detail {
                    header_pairs.push(("Authorization".to_string(), value.clone()));
                }
                builder = builder.header(reqwest::header::AUTHORIZATION, value);
            }
            "basic" => {
                let raw = format!("{}:{}", auth.username, auth.password);
                let value = format!("Basic {}", base64_encode(raw.as_bytes()));
                if keep_detail {
                    header_pairs.push(("Authorization".to_string(), value.clone()));
                }
                builder = builder.header(reqwest::header::AUTHORIZATION, value);
            }
            "apikey" if auth.add_to != "query" && !auth.key.trim().is_empty() => {
                if keep_detail {
                    header_pairs.push((auth.key.clone(), auth.value.clone()));
                }
                builder = builder.header(auth.key.clone(), auth.value.clone());
            }
            _ => {}
        }
    }

    // 自动携带本机 Cookie（启动时已按目标 URL 预取）
    if !has_cookie {
        if let Some(cookie) = &req.cookie {
            if keep_detail {
                header_pairs.push(("Cookie".to_string(), cookie.clone()));
            }
            builder = builder.header(reqwest::header::COOKIE, cookie.clone());
        }
    }

    let body_type = req.body_type.as_str();
    let mut request_body: Option<String> = None;
    if !body_type.is_empty() && body_type != "none" {
        if !has_content_type {
            let content_type = match body_type {
                "json" => "application/json",
                "form" => "application/x-www-form-urlencoded",
                _ => "text/plain; charset=utf-8",
            };
            builder = builder.header(reqwest::header::CONTENT_TYPE, content_type);
            if keep_detail {
                header_pairs.push(("Content-Type".to_string(), content_type.to_string()));
            }
        }
        let raw_body = req.body.as_deref().unwrap_or("");
        // JSON 请求体里的注释在发送前去掉，与 Postman 行为一致
        let source = if body_type == "json" {
            strip_json_comments(raw_body)
        } else {
            raw_body.to_string()
        };
        let rendered = render(&source, vars);
        builder = builder.body(rendered.clone());
        if keep_detail {
            request_body = Some(rendered);
        }
    }

    let request_headers = if keep_detail {
        headers_json(&header_pairs)
    } else {
        String::new()
    };
    let started = Instant::now();
    match builder.send().await {
        Ok(response) => {
            let ttfb_ms = started.elapsed().as_millis() as u64;
            let status = response.status();
            let response_headers = if keep_detail {
                headers_json(
                    &response
                        .headers()
                        .iter()
                        .map(|(key, value)| {
                            (
                                key.as_str().to_string(),
                                value.to_str().unwrap_or_default().to_string(),
                            )
                        })
                        .collect::<Vec<_>>(),
                )
            } else {
                String::new()
            };
            let (text, bytes) = read_body(
                response,
                if keep_detail {
                    Some(MAX_RECORD_RESPONSE)
                } else {
                    None
                },
            )
            .await;
            // 响应体耗时在单次请求内部测量：重试时总耗时包含两次请求，
            // 不能用总耗时减 TTFB，否则响应体时间会把第一次请求也算进去
            let body_ms = (started.elapsed().as_millis() as u64).saturating_sub(ttfb_ms);
            let snippet = clip(&text, SNIPPET_LIMIT);
            let response_body = if text.trim().is_empty() {
                None
            } else {
                Some(clip(&text, MAX_RECORD_RESPONSE))
            };
            let code = status.as_u16();
            let ok = code < 400;
            Attempt {
                status: code,
                ok,
                url: url_text,
                error: if ok {
                    None
                } else {
                    Some(format!("HTTP {code}"))
                },
                snippet,
                ttfb_ms,
                body_ms,
                bytes,
                body: request_body,
                params: query,
                headers: request_headers,
                response_headers: if response_headers.is_empty() {
                    None
                } else {
                    Some(response_headers)
                },
                response_body,
                picks,
            }
        }
        Err(error) => Attempt {
            status: 0,
            ok: false,
            url: url_text,
            error: Some(error.to_string()),
            snippet: String::new(),
            ttfb_ms: 0,
            body_ms: 0,
            bytes: 0,
            body: request_body,
            params: query,
            headers: request_headers,
            response_headers: None,
            response_body: None,
            picks,
        },
    }
}

fn clip(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let mut out: String = text.chars().take(limit).collect();
    out.push('…');
    out
}

fn classify_error(message: &str) -> &'static str {
    let lower = message.to_lowercase();
    if lower.contains("timed out") || lower.contains("timeout") {
        "TIMEOUT"
    } else if lower.contains("certificate") || lower.contains("tls") {
        "TLS"
    } else if lower.contains("connect") || lower.contains("connection") {
        "CONNECT"
    } else if lower.contains("dns") {
        "DNS"
    } else {
        "NETWORK"
    }
}

/// 估算单请求发送字节：请求行 + 头 + 体
///
/// 压测期间请求内容静态，启动时算一次，逐请求只做一次加法。
/// 头部按实际配置的 key/value 累加，另加 40 字节估算 Host/UA/Accept 等栈自动补充的头。
#[cfg(test)]
fn estimate_request_bytes(config: &StressConfig, method: &reqwest::Method) -> u64 {
    estimate_request_bytes_for(&config.url, &config.headers, config.body.as_deref(), method)
}

/// 多接口版本：按单个接口的 URL / 头 / 体估算发送字节
fn estimate_request_bytes_for(
    url: &str,
    headers: &[KeyValueItem],
    body: Option<&str>,
    method: &reqwest::Method,
) -> u64 {
    let mut total = method.as_str().len() + url.trim().len() + " HTTP/1.1\r\n".len() + 40;
    for item in headers {
        if item.enabled && !item.key.trim().is_empty() {
            total += item.key.len() + item.value.len() + ": \r\n".len();
        }
    }
    total += body.map(|body| body.len()).unwrap_or(0);
    total as u64
}

/// 采集施压端环境信息（写入 config_json.agent）
fn collect_agent_info() -> serde_json::Value {
    let mut sys = System::new();
    sys.refresh_memory();
    json!({
        "tool": "AstroMan",
        "version": env!("CARGO_PKG_VERSION"),
        "os": System::name().unwrap_or_default(),
        "osVersion": System::os_version().unwrap_or_default(),
        "arch": std::env::consts::ARCH,
        "host": System::host_name().unwrap_or_default(),
        "cpuCores": std::thread::available_parallelism().map(|value| value.get()).unwrap_or(0),
        "memTotalMb": sys.total_memory() / 1024 / 1024,
    })
}

/// 压测期间本进程资源峰值采样（每 5 秒一次，避免自身成为负担）
struct ResourceMonitor {
    sys: System,
    pid: sysinfo::Pid,
    last: Instant,
    cpu_peak: f32,
    mem_peak_mb: u64,
}

impl ResourceMonitor {
    fn new() -> Self {
        let mut sys = System::new();
        sys.refresh_memory();
        Self {
            sys,
            pid: sysinfo::Pid::from_u32(std::process::id()),
            // 首次采样延后，让 sysinfo 有足够的时间基准
            last: Instant::now(),
            cpu_peak: 0.0,
            mem_peak_mb: 0,
        }
    }

    fn sample(&mut self) {
        if self.last.elapsed() < Duration::from_secs(5) {
            return;
        }
        self.last = Instant::now();
        self.sys.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[self.pid]),
            true,
            ProcessRefreshKind::nothing().with_cpu().with_memory(),
        );
        if let Some(process) = self.sys.process(self.pid) {
            // sysinfo 的进程 CPU 是「单核百分比」，除以 100 得到核数
            let cores = process.cpu_usage() / 100.0;
            if cores > self.cpu_peak {
                self.cpu_peak = cores;
            }
            let mem_mb = process.memory() / 1024 / 1024;
            if mem_mb > self.mem_peak_mb {
                self.mem_peak_mb = mem_mb;
            }
        }
    }
}

/// 当前时刻应生效的并发数与阶段序号
fn stage_at(stages: &[StressStage], elapsed_sec: u64, fallback: u32) -> (u32, i64) {
    if stages.is_empty() {
        return (fallback, 0);
    }
    let mut acc = 0u64;
    for (index, stage) in stages.iter().enumerate() {
        acc += stage.duration_sec.max(1) as u64;
        if elapsed_sec < acc {
            return (stage.concurrent.clamp(1, MAX_CONCURRENT), index as i64);
        }
    }
    let last = stages.len() - 1;
    (
        stages[last].concurrent.clamp(1, MAX_CONCURRENT),
        last as i64,
    )
}

async fn take_token(ctx: &RunCtx) -> bool {
    if ctx.control.rate.load(Ordering::Relaxed) == 0 {
        return true;
    }
    loop {
        if ctx.cancel.load(Ordering::Relaxed) || Instant::now() >= ctx.deadline {
            return false;
        }
        let tokens = ctx.control.tokens.load(Ordering::Relaxed);
        if tokens > 0 {
            if ctx
                .control
                .tokens
                .compare_exchange(tokens, tokens - 1, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                return true;
            }
            continue;
        }
        // 先登记唤醒再复查：避免「复查后刚好补令牌」导致错过通知
        let notified = ctx.control.token_wake.notified();
        if ctx.control.tokens.load(Ordering::Relaxed) > 0 {
            continue;
        }
        // 20ms 兜底：即使错过一次通知，也不会让限速请求多等一整秒
        let _ = tokio::time::timeout(Duration::from_millis(20), notified).await;
    }
}

/// 轻量随机数（xorshift64*）：用于思考时间分布与 VU 起始相位抖动
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed | 1)
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// 标准正态（Box-Muller），不引入额外依赖
    fn next_normal(&mut self) -> f64 {
        let u1 = self.next_f64().max(1e-12);
        let u2 = self.next_f64();
        (-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos()
    }
}

/// 预热曲线：linear / step / spike（8.7）
fn ramp_delay_ms(config: &StressConfig, index: u32, workers: u32) -> u64 {
    let total = config.ramp_up_sec as f64 * 1000.0;
    if workers <= 1 || total <= 0.0 {
        return 0;
    }
    let ratio = index as f64 / (workers - 1) as f64;
    let factor = match config.ramp_up_mode.trim() {
        // 5 个梯队：每批同时进场并停留，形成阶梯
        "step" => {
            let steps = 5.0;
            (ratio * steps).floor() / (steps - 1.0)
        }
        // 后段集中进场，前期几乎不压
        "spike" => ratio * ratio,
        _ => ratio,
    };
    (total * factor.clamp(0.0, 1.0)) as u64
}

/// 每次请求后的思考时间（8.7）
fn think_delay(config: &StressConfig, rng: &mut Rng) -> Option<Duration> {
    let base = config.think_time_ms;
    let mode = config.think_time_mode.trim();
    let ms = match mode {
        "uniform" => {
            let min = if config.think_time_min_ms > 0 {
                config.think_time_min_ms
            } else {
                base
            };
            let max = if config.think_time_max_ms > min {
                config.think_time_max_ms
            } else {
                min
            };
            if max == min {
                min
            } else {
                min + rng.next_u64() % (max - min + 1)
            }
        }
        "normal" => {
            let mean = if config.think_time_mean_ms > 0 {
                config.think_time_mean_ms
            } else {
                base
            } as f64;
            if mean <= 0.0 {
                0
            } else {
                let std = (if config.think_time_std_ms > 0 {
                    config.think_time_std_ms as f64
                } else {
                    mean * 0.3
                })
                .max(1.0);
                (mean + std * rng.next_normal()).max(0.0) as u64
            }
        }
        "lognormal" => {
            let mean = if config.think_time_mean_ms > 0 {
                config.think_time_mean_ms
            } else {
                base
            } as f64;
            if mean <= 0.0 {
                0
            } else {
                let std = (if config.think_time_std_ms > 0 {
                    config.think_time_std_ms as f64
                } else {
                    mean * 0.3
                })
                .max(1.0);
                // 由均值/标准差反推对数正态参数
                let sigma = (1.0 + (std * std) / (mean * mean)).ln().sqrt();
                let mu = mean.ln() - sigma * sigma / 2.0;
                (mu + sigma * rng.next_normal()).exp().max(0.0) as u64
            }
        }
        _ => base,
    };
    // 最多 1 小时，避免误配置把 worker 永久睡住
    let ms = ms.min(3_600_000);
    if ms == 0 {
        None
    } else {
        Some(Duration::from_millis(ms))
    }
}

/// 按权重随机选一个接口：单接口直接返回 0，避免无意义抽样
fn pick_request(ctx: &RunCtx, rng: &mut Rng) -> usize {
    if ctx.requests.len() <= 1 {
        return 0;
    }
    let mut point = rng.next_f64() * ctx.total_weight;
    for (index, req) in ctx.requests.iter().enumerate() {
        if req.weight <= 0.0 {
            continue;
        }
        point -= req.weight;
        if point < 0.0 {
            return index;
        }
    }
    // 浮点误差兜底：取最后一个有权重的接口
    ctx.requests
        .iter()
        .rposition(|req| req.weight > 0.0)
        .unwrap_or(0)
}

async fn worker(ctx: Arc<RunCtx>, index: u32, workers: u32) {
    let mut rng = Rng::new(
        (index as u64 + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15)
            ^ std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|value| value.as_nanos() as u64)
                .unwrap_or(1),
    );
    let mut remaining = ramp_delay_ms(&ctx.config, index, workers);
    // 每 VU 起始相位随机：把并发进场摊平，避免同一毫秒集中打满
    if ctx.config.vu_start_jitter_ms > 0 {
        remaining += rng.next_u64() % (ctx.config.vu_start_jitter_ms + 1);
    }
    while remaining > 0 {
        if ctx.cancel.load(Ordering::Relaxed) {
            return;
        }
        let step = remaining.min(200);
        tokio::time::sleep(Duration::from_millis(step)).await;
        remaining -= step;
    }

    let mut seq: u64 = 0;
    loop {
        if ctx.cancel.load(Ordering::Relaxed) || Instant::now() >= ctx.deadline {
            break;
        }

        // 阶梯加压：未进入生效区间的 worker 等待。先建 notified() 再复查，
        // 避免「检查完刚好切换并发」时白等一整秒；1 秒兜底只是防御。
        if index >= ctx.control.active.load(Ordering::Relaxed) {
            let notified = ctx.control.wake.notified();
            if index < ctx.control.active.load(Ordering::Relaxed) {
                continue;
            }
            let _ = tokio::time::timeout(Duration::from_secs(1), notified).await;
            continue;
        }

        if !take_token(&ctx).await {
            break;
        }

        // 采集开关每请求读一次，auto 降档后立即生效
        let capture = Capture::from_u8(ctx.capture.load(Ordering::Relaxed));
        // 多接口按权重分发：每个 VU 每轮独立抽样，长稳压测下占比收敛到配置值
        let req_index = pick_request(&ctx, &mut rng);
        let req = &ctx.requests[req_index];
        ctx.metrics.concurrent.fetch_add(1, Ordering::Relaxed);
        let started = Instant::now();
        let mut attempt = send_once(&ctx, req, capture, index).await;
        // 401/403 刷新后重试：两次请求都产生流量与耗时，
        // 报告里要把第一次的响应字节和发送字节补回来，否则流量统计偏小
        let mut retried_bytes = 0u64;
        let mut sent_factor = 1u64;
        // Token 池：401/403 时先尝试刷新（或切下一条 token），再重试一次
        if matches!(attempt.status, 401 | 403) && !attempt.picks.is_empty() {
            if let Some(pools) = ctx.pools.as_ref() {
                let refreshable = pools.http_refresh_enabled(&attempt.picks);
                if pools
                    .refresh_after_unauthorized(&ctx.client, &ctx.vars, &attempt.picks)
                    .await
                {
                    retried_bytes = attempt.bytes;
                    sent_factor = 2;
                    attempt = send_once(&ctx, req, capture, index).await;
                } else if refreshable {
                    ctx.metrics
                        .bump_error_type(crate::services::pool::E_POOL_REFRESH_FAILED);
                }
            }
        }
        let latency_ms = started.elapsed().as_millis() as u64;
        ctx.metrics.concurrent.fetch_sub(1, Ordering::Relaxed);
        ctx.metrics.record(
            req.method.as_str(),
            &attempt.url,
            ctx.start.elapsed().as_millis() as i64,
            latency_ms,
            attempt.ttfb_ms.min(latency_ms),
            attempt.body_ms,
            attempt.status,
            attempt.bytes.saturating_add(retried_bytes),
            req.req_bytes.saturating_mul(sent_factor),
            attempt.ok,
            attempt.error.as_deref(),
            req_index,
        );

        let error_type = if attempt.ok {
            None
        } else if attempt.status > 0 {
            Some("HTTP".to_string())
        } else {
            Some(classify_error(attempt.error.as_deref().unwrap_or("")).to_string())
        };
        // 明细：off 完全不构造；sample 每 SAMPLE_EVERY 个请求留一条（确定性，可复现）
        let should_record = match capture {
            Capture::Off => false,
            Capture::Full => true,
            Capture::Sample => seq.is_multiple_of(SAMPLE_EVERY),
        };
        seq = seq.wrapping_add(1);
        // 慢请求 TopN：与逐请求明细解耦，定位模式下只保留轻量字段
        if ctx.slow_limit > 0 {
            let ttfb_ms = attempt.ttfb_ms.min(latency_ms);
            if (ctx.slow_threshold_ms == 0 || latency_ms >= ctx.slow_threshold_ms)
                && ctx
                    .slow_tx
                    .try_send(SlowRequest {
                        elapsed_ms: ctx.start.elapsed().as_millis() as i64,
                        method: req.method.as_str().to_string(),
                        url: attempt.url.clone(),
                        status: attempt.status as i64,
                        latency_ms: latency_ms as i64,
                        ttfb_ms: ttfb_ms as i64,
                        body_ms: latency_ms.saturating_sub(ttfb_ms) as i64,
                    })
                    .is_err()
            {
                ctx.metrics.slow_dropped.fetch_add(1, Ordering::Relaxed);
            }
        }
        if should_record {
            let ttfb_ms = attempt.ttfb_ms.min(latency_ms);
            let _ = ctx.records.send(ReportRequestRecord {
                elapsed_ms: ctx.start.elapsed().as_millis() as i64,
                time_ms: now_ms() - latency_ms as i64,
                method: req.method.as_str().to_string(),
                url: attempt.url.clone(),
                status: attempt.status as i64,
                latency_ms: latency_ms as i64,
                ttfb_ms: ttfb_ms as i64,
                bytes: attempt.bytes as i64,
                success: attempt.ok,
                body: attempt
                    .body
                    .as_deref()
                    .map(|text| clip(text, MAX_RECORD_BODY)),
                params: if attempt.params.is_empty() {
                    None
                } else {
                    Some(attempt.params.clone())
                },
                headers: if attempt.headers.is_empty() {
                    None
                } else {
                    Some(attempt.headers.clone())
                },
                response_headers: attempt.response_headers.clone(),
                response_body: attempt.response_body.clone(),
                error_type: error_type.clone(),
                error_message: attempt
                    .error
                    .as_deref()
                    .map(|message| clip(message, MAX_RECORD_ERROR)),
            });
        }

        if !attempt.ok {
            ctx.metrics
                .bump_error_type(error_type.as_deref().unwrap_or("NETWORK"));
            let item = ReportError {
                elapsed_sec: ctx.start.elapsed().as_secs() as i64,
                error_type: error_type.unwrap_or_else(|| "NETWORK".to_string()),
                error_message: attempt.error.clone().unwrap_or_default(),
                request_url: attempt.url,
                response_status: if attempt.status > 0 {
                    Some(attempt.status as i64)
                } else {
                    None
                },
                response_body: attempt.snippet,
            };
            if ctx.errors.try_send(item).is_err() {
                ctx.metrics.errors_dropped.fetch_add(1, Ordering::Relaxed);
            }
        }

        if let Some(delay) = think_delay(&ctx.config, &mut rng) {
            tokio::time::sleep(delay).await;
        }
    }
}

/// 压测执行结果：命令层与套件 `kind=stress` 用例共用
pub struct StressExecOutcome {
    pub report_id: Option<i64>,
    pub report: StressReport,
    pub message: Option<String>,
}

pub(crate) struct PreparedStress {
    ctx: RunCtx,
    pub(crate) name: String,
    stages: Vec<StressStage>,
    pub(crate) peak: u32,
    total_sec: u64,
    rate: u32,
    target_rps: f64,
    config_snapshot: String,
    stop_rate: Option<f64>,
    stop_min: u32,
    concurrent: i64,
    /// 渲染后的目标 URL（注册表展示与同目标判定）
    pub(crate) url_text: String,
    pub(crate) target_key: String,
    errors_rx: mpsc::Receiver<ReportError>,
    records_rx: mpsc::UnboundedReceiver<ReportRequestRecord>,
    slow_rx: mpsc::Receiver<SlowRequest>,
}

impl PreparedStress {
    /// 任务登记后才拿到注册表取消位：替换成同一个 Arc，停止按钮才能停掉引擎
    pub(crate) fn replace_cancel(&mut self, cancel: Arc<AtomicBool>) {
        self.ctx.cancel = cancel;
    }
}

/// 归一化本次运行：阶梯阶段、峰值并发、总时长
///
/// 固定并发上限按来源区分：快速压测 2h，压测计划 24h；阶梯单阶段固定 1h，
/// 总时长可到 24h。超过 2h 的启动需要 `long_run_ack` 显式确认。
fn plan_run(config: &StressConfig) -> (Vec<StressStage>, u32, u64) {
    let stages: Vec<StressStage> = config
        .stages
        .iter()
        .filter(|item| item.duration_sec > 0)
        .map(|item| StressStage {
            concurrent: item.concurrent.clamp(1, MAX_CONCURRENT),
            duration_sec: item.duration_sec.clamp(1, MAX_STAGE_SEC),
        })
        .collect();
    let peak = if stages.is_empty() {
        config.concurrent.clamp(1, MAX_CONCURRENT)
    } else {
        stages.iter().map(|item| item.concurrent).max().unwrap_or(1)
    };
    let total_sec = if stages.is_empty() {
        let max = if config.plan_id.is_some() {
            MAX_FIXED_PLAN_SEC
        } else {
            MAX_FIXED_QUICK_SEC
        };
        config.duration_sec.clamp(1, max) as u64
    } else {
        stages
            .iter()
            .map(|item| item.duration_sec as u64)
            .sum::<u64>()
            .clamp(1, MAX_FIXED_PLAN_SEC as u64)
    };
    (stages, peak, total_sec)
}

/// 同目标判定键：method + scheme://host:port + path（query 不参与）
fn target_key_of(method: &str, url: &reqwest::Url) -> String {
    let mut key = format!(
        "{} {}://{}",
        method.trim().to_uppercase(),
        url.scheme(),
        url.host_str().unwrap_or_default()
    );
    if let Some(port) = url.port_or_known_default() {
        key.push_str(&format!(":{port}"));
    }
    key.push_str(url.path());
    key
}

/// 采集预设 → (明细档位, TopN 条数)
///
/// - 容量：明细交给 auto 定档，只统计均值，不保留逐请求明细；
/// - 定位：关闭 5 万条全量明细，改为保留 TopN 慢请求（阈值可配，0 = 全量候选）；
/// - 自动：沿用旧行为，`topN > 0` 时额外保留慢请求。
fn capture_plan(config: &StressConfig) -> (RecordDetailMode, u32) {
    match config.capture_mode {
        CapturePreset::Capacity => (RecordDetailMode::Auto, 0),
        CapturePreset::Locate => (
            RecordDetailMode::Off,
            config.top_n.unwrap_or(DEFAULT_TOP_N).min(MAX_TOP_N),
        ),
        CapturePreset::Auto => (
            config.record_detail,
            config.top_n.unwrap_or(0).min(MAX_TOP_N),
        ),
    }
}

/// 解析后的环境（id 为空表示未绑定任何环境）
struct ResolvedEnv {
    id: Option<i64>,
    name: Option<String>,
}

/// 环境解析优先级：显式 envId → 配置/计划/请求所属项目的激活环境
///
/// 显式指定的环境不存在时直接报错，避免静默回退到另一个环境导致压错目标。
fn resolve_env(conn: &rusqlite::Connection, config: &StressConfig) -> Result<ResolvedEnv, String> {
    if let Some(env_id) = config.env_id {
        let name: Option<String> = conn
            .query_row(
                "SELECT name FROM environments WHERE id = ?1",
                params![env_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(err)?;
        return match name {
            Some(name) => Ok(ResolvedEnv {
                id: Some(env_id),
                name: Some(name),
            }),
            None => Err(format!(
                "所选环境不存在或已删除（envId={env_id}），请在运行栏重新选择环境"
            )),
        };
    }

    // 无显式环境：按计划 → 请求逐级找出所属项目
    let mut project_id = config.project_id;
    if project_id.is_none() {
        if let Some(plan_id) = config.plan_id {
            project_id = conn
                .query_row(
                    "SELECT project_id FROM test_plans WHERE id = ?1",
                    params![plan_id],
                    |row| row.get(0),
                )
                .optional()
                .map_err(err)?;
        }
    }
    if project_id.is_none() {
        if let Some(request_id) = config.request_id {
            project_id = conn
                .query_row(
                    "SELECT c.project_id FROM requests r
                     JOIN collections c ON c.id = r.collection_id WHERE r.id = ?1",
                    params![request_id],
                    |row| row.get(0),
                )
                .optional()
                .map_err(err)?;
        }
    }
    let Some(project_id) = project_id else {
        return Ok(ResolvedEnv {
            id: None,
            name: None,
        });
    };

    let active: Option<(i64, String)> = conn
        .query_row(
            "SELECT id, name FROM environments
             WHERE project_id = ?1 AND is_active = 1 ORDER BY id ASC LIMIT 1",
            params![project_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(err)?;
    Ok(match active {
        Some((id, name)) => ResolvedEnv {
            id: Some(id),
            name: Some(name),
        },
        None => ResolvedEnv {
            id: None,
            name: None,
        },
    })
}

/// 扫描渲染后文本中残留的 `{{变量}}` 名称（去重、保序）
fn find_unresolved(text: &str) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("{{") {
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else {
            break;
        };
        let name = after[..end].trim();
        if !name.is_empty() && !names.iter().any(|item| item == name) {
            names.push(name.to_string());
        }
        rest = &after[end + 2..];
    }
    names
}

fn collect_unresolved_into(target: &mut Vec<String>, text: &str) {
    for name in find_unresolved(text) {
        if !target.iter().any(|item| item == &name) {
            target.push(name);
        }
    }
}

/// 启动前检查 URL / 参数 / 请求头 / 请求体 / 认证中是否有未解析变量
///
/// `build_url` 也做同样校验（运行期零分配路径），这里提前给出可读错误，
/// 并把缺失变量名、当前环境一次性告诉用户。
/// `provided` 是运行期由池注入的变量名（`*` 展开列名 + 显式 target），不判为缺失。
fn check_unresolved(
    config: &StressConfig,
    vars: &HashMap<String, String>,
    provided: &[String],
    env_name: Option<&str>,
) -> Result<(), String> {
    let mut missing: Vec<String> = Vec::new();
    for request in effective_requests(config) {
        collect_unresolved_into(&mut missing, &render(request.url.trim(), vars));
        for item in &request.params {
            if !item.enabled {
                continue;
            }
            collect_unresolved_into(&mut missing, &render(item.key.trim(), vars));
            collect_unresolved_into(&mut missing, &render(&item.value, vars));
        }
        for item in &request.headers {
            if !item.enabled {
                continue;
            }
            collect_unresolved_into(&mut missing, &render(item.key.trim(), vars));
            collect_unresolved_into(&mut missing, &render(&item.value, vars));
        }
        if let Some(body) = &request.body {
            collect_unresolved_into(&mut missing, &render(body, vars));
        }
        if let Some(auth) = &request.auth {
            for value in [
                &auth.token,
                &auth.username,
                &auth.password,
                &auth.key,
                &auth.value,
                &auth.token_url,
                &auth.client_id,
                &auth.client_secret,
                &auth.scope,
            ] {
                collect_unresolved_into(&mut missing, &render(value, vars));
            }
        }
    }
    // 池运行期注入的变量（自动列 + 显式 target）不应在启动前被判定为缺失
    missing.retain(|name| !provided.iter().any(|target| target == name));
    if missing.is_empty() {
        return Ok(());
    }
    let env_label = match env_name {
        Some(name) => format!("环境「{name}」"),
        None => "当前未绑定环境".to_string(),
    };
    // `os:` / `secret:` 前缀来自专用占位符，分开说明，避免用户去环境里找同名普通变量
    let mut plain: Vec<&str> = Vec::new();
    let mut os_names: Vec<&str> = Vec::new();
    let mut secret_names: Vec<&str> = Vec::new();
    for name in &missing {
        if let Some(rest) = name.strip_prefix("os:") {
            os_names.push(rest);
        } else if let Some(rest) = name.strip_prefix("secret:") {
            secret_names.push(rest);
        } else {
            plain.push(name.as_str());
        }
    }
    let mut detail: Vec<String> = Vec::new();
    if !plain.is_empty() {
        detail.push(plain.join("、"));
    }
    if !os_names.is_empty() {
        detail.push(format!(
            "{}（系统变量，需在「设置 → 压测 → OS 变量」加入白名单且本机有值）",
            os_names.join("、")
        ));
    }
    if !secret_names.is_empty() {
        detail.push(format!(
            "{}（密钥，需在当前环境里配置该 secret 变量）",
            secret_names.join("、")
        ));
    }
    Err(format!(
        "{env_label} 缺少变量：{}。请补充变量或切换环境后重试",
        detail.join("；")
    ))
}

/// 校验池绑定：池名、列选择、同池轮换模式、同池重复注入与跨池同名注入
fn validate_pool_bindings(
    bindings: &[PoolBinding],
    columns: &HashMap<String, Vec<String>>,
) -> Result<(), String> {
    use crate::services::pool::{
        E_POOL_BINDING_INVALID as INVALID, E_POOL_VAR_CONFLICT as CONFLICT,
    };
    let mut modes: HashMap<&str, &str> = HashMap::new();
    // 变量名 → 注入它的池：跨池同名会静默覆盖，必须拦下
    let mut owners: HashMap<String, String> = HashMap::new();
    // 池 → 已注入变量名：同池多条绑定重复注入同一个变量
    let mut injected: HashMap<String, Vec<String>> = HashMap::new();
    for binding in bindings {
        let pool = binding.pool.trim();
        if pool.is_empty() {
            return Err(format!("{INVALID}: 池绑定缺少池名"));
        }
        let column = binding.column.trim();
        let target = binding.target.trim();
        let pool_columns = columns.get(pool);
        if column == "*" {
            if pool_columns.map(|list| list.is_empty()).unwrap_or(true) {
                return Err(format!(
                    "{INVALID}: 池「{pool}」没有列定义，不能使用「全部列」绑定"
                ));
            }
        } else if column.is_empty() {
            if target.is_empty() {
                return Err(format!("{INVALID}: 池「{pool}」的绑定缺少目标变量"));
            }
        } else {
            if target.is_empty() {
                return Err(format!(
                    "{INVALID}: 池「{pool}」的绑定列「{column}」缺少目标变量"
                ));
            }
            let exists = pool_columns
                .map(|list| list.iter().any(|item| item == column))
                .unwrap_or(false);
            if !exists {
                return Err(format!("{INVALID}: 池「{pool}」不存在列「{column}」"));
            }
        }
        let mode = binding_mode(binding);
        if let Some(previous) = modes.get(pool) {
            if *previous != mode {
                return Err(format!("{INVALID}: 池「{pool}」的多条绑定轮换模式必须一致"));
            }
        } else {
            modes.insert(pool, mode);
        }
        let names: Vec<String> = if column == "*" {
            pool_columns.cloned().unwrap_or_default()
        } else {
            vec![target.to_string()]
        };
        for name in names {
            if name.trim().is_empty() {
                continue;
            }
            let same_pool = injected.entry(pool.to_string()).or_default();
            if same_pool.iter().any(|item| item == &name) {
                return Err(format!(
                    "{INVALID}: 池「{pool}」的绑定重复注入变量「{name}」"
                ));
            }
            same_pool.push(name.clone());
            if let Some(previous) = owners.insert(name.clone(), pool.to_string()) {
                return Err(format!(
                    "{CONFLICT}: 池「{previous}」和池「{pool}」都注入变量「{name}」，后绑定的会静默覆盖前者；请改名或只保留一个绑定"
                ));
            }
        }
    }
    Ok(())
}

/// 构建运行期池
///
/// 单机：按 `poolBindings` 从本机库读取；
/// 集群：以主节点下发的 `poolSlices` 为准，不再访问本机池；
/// `relay` 非空时 token 刷新统一走主节点单飞。
fn build_pool_runtime(
    state: &AppState,
    config: &StressConfig,
    relay: Option<Arc<dyn PoolRefreshRelay>>,
) -> Result<Option<Arc<PoolRuntime>>, String> {
    let mut bindings = config.pool_bindings.clone();
    if !config.pool_slices.is_empty() {
        let mut items: HashMap<String, Vec<PoolItem>> = HashMap::new();
        let mut offsets: HashMap<String, usize> = HashMap::new();
        let mut refresh = HashMap::new();
        let mut columns: HashMap<String, Vec<String>> = HashMap::new();
        for slice in &config.pool_slices {
            if slice.items.is_empty() {
                return Err(format!(
                    "{}: 池「{}」的节点分片为空",
                    crate::services::pool::E_POOL_SHARD_EMPTY,
                    slice.name
                ));
            }
            items.insert(slice.name.clone(), slice.items.clone());
            offsets.insert(slice.name.clone(), slice.start_index);
            columns.insert(slice.name.clone(), slice.columns.clone());
            if let Some(config) = slice.refresh.clone() {
                refresh.insert(slice.name.clone(), config);
            }
        }
        if bindings.is_empty() {
            bindings = config
                .pool_slices
                .iter()
                .map(|slice| PoolBinding {
                    pool: slice.name.clone(),
                    target: slice.name.clone(),
                    mode: "per_vu".to_string(),
                    refresh_on_401: !slice.items.is_empty(),
                    // 有列定义时自动注入全部列，否则沿用旧行为取 value
                    column: if slice.columns.is_empty() {
                        String::new()
                    } else {
                        "*".to_string()
                    },
                })
                .collect();
        }
        if bindings.is_empty() {
            return Ok(None);
        }
        validate_pool_bindings(&bindings, &columns)?;
        return Ok(Some(Arc::new(
            PoolRuntime::new(bindings, items, columns, refresh)
                .with_offsets(offsets)
                .with_relay(relay),
        )));
    }
    if bindings.is_empty() {
        return Ok(None);
    }
    let conn = state.pool.get().map_err(err)?;
    let items = crate::services::pool::items_for_bindings(&conn, &bindings)?;
    // 列定义用于 "*" 自动注入、显式列校验与刷新列选择
    let mut columns: HashMap<String, Vec<String>> = HashMap::new();
    for binding in &bindings {
        let name = binding.pool.trim();
        if name.is_empty() || columns.contains_key(name) {
            continue;
        }
        columns.insert(
            name.to_string(),
            crate::services::pool::pool_columns(&conn, name)?,
        );
    }
    validate_pool_bindings(&bindings, &columns)?;
    for binding in &bindings {
        if items
            .get(binding.pool.trim())
            .map(|list| list.is_empty())
            .unwrap_or(true)
        {
            return Err(format!(
                "{}: 池「{}」没有可用条目",
                crate::services::pool::E_POOL_EMPTY,
                binding.pool
            ));
        }
    }
    let names: Vec<String> = bindings
        .iter()
        .map(|binding| binding.pool.trim().to_string())
        .collect();
    let refresh = crate::services::pool::pool_refresh_configs(&conn, &names)?;
    let runtime = PoolRuntime::new(bindings, items, columns, refresh).with_relay(relay);
    Ok(Some(Arc::new(runtime)))
}

/// 校验配置、渲染模板、构建客户端并准备 Cookie 快照；不依赖 Tauri 事件
///
/// `cancel` 由任务注册表发放：调用方先建取消位，任务登记成功后共用同一个 Arc。
pub(crate) fn prepare_stress(
    state: &AppState,
    config: StressConfig,
    cancel: Arc<AtomicBool>,
) -> Result<PreparedStress, String> {
    prepare_stress_with_relay(state, config, cancel, None)
}

/// 变量来源清单的一条记录
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VarSourceItem {
    /// 变量名（不含花括号）
    pub name: String,
    /// user / secret / os
    pub kind: String,
    /// env / runtime / pool / os / missing
    pub source: String,
    /// 具体说明：环境名、池名、白名单状态
    pub detail: String,
    /// 当前配置下能否取到值
    pub ok: bool,
}

/// 变量来源清单：本次配置引用到的每个占位符，标明「是什么、从哪来、能不能用」
///
/// 只读命令：不写库、不下发、不返回任何变量值。对应界面上的「本次变量」面板，
/// 让用户在开跑前就能分清普通变量、密钥与系统变量，而不是等跑不起来再猜。
#[tauri::command]
pub fn stress_var_sources(
    state: State<'_, AppState>,
    config: StressConfig,
) -> Result<Vec<VarSourceItem>, String> {
    let conn = state.pool.get().map_err(err)?;
    let resolved_env = resolve_env(&conn, &config)?;
    let (env_vars, env_secrets) = match resolved_env.id {
        Some(env_id) => crate::commands::http::load_vars_with_secrets(&conn, env_id)?,
        None => (HashMap::new(), std::collections::HashSet::new()),
    };
    // 池会注入的变量名 → 池名：`*` 按列定义展开，其余用显式 target
    let mut pool_targets: HashMap<String, String> = HashMap::new();
    for binding in &config.pool_bindings {
        let pool = binding.pool.trim();
        if pool.is_empty() {
            continue;
        }
        if binding.column.trim() == "*" {
            if let Ok(columns) = crate::services::pool::pool_columns(&conn, pool) {
                for name in columns {
                    pool_targets.insert(name, pool.to_string());
                }
            }
        } else if !binding.target.trim().is_empty() {
            pool_targets.insert(binding.target.trim().to_string(), pool.to_string());
        }
    }
    drop(conn);

    let texts = crate::util::config_texts(&config);
    let classified = crate::util::required_vars_in_texts(texts.iter().map(String::as_str));
    let os_allow = state.settings().stress_os_env_allow;
    let env_label = resolved_env
        .name
        .clone()
        .unwrap_or_else(|| "未指定环境".to_string());
    let mut items: Vec<VarSourceItem> = Vec::new();

    for name in classified.user {
        let item = if let Some(pool) = pool_targets.get(&name) {
            VarSourceItem {
                name: name.clone(),
                kind: "user".to_string(),
                source: "pool".to_string(),
                detail: format!("数据池「{pool}」"),
                ok: true,
            }
        } else if config.vars.contains_key(&name) {
            VarSourceItem {
                name: name.clone(),
                kind: "user".to_string(),
                source: "runtime".to_string(),
                detail: "运行期变量".to_string(),
                ok: true,
            }
        } else if env_vars.contains_key(&name) {
            VarSourceItem {
                name: name.clone(),
                kind: "user".to_string(),
                source: "env".to_string(),
                detail: format!("环境「{env_label}」"),
                ok: true,
            }
        } else {
            VarSourceItem {
                name: name.clone(),
                kind: "user".to_string(),
                source: "missing".to_string(),
                detail: "环境变量与数据池里都没有".to_string(),
                ok: false,
            }
        };
        items.push(item);
    }

    for name in classified.secret {
        let item = if let Some(pool) = pool_targets.get(&name) {
            VarSourceItem {
                name: name.clone(),
                kind: "secret".to_string(),
                source: "pool".to_string(),
                detail: format!("数据池「{pool}」"),
                ok: true,
            }
        } else if env_secrets.contains(&name) {
            VarSourceItem {
                name: name.clone(),
                kind: "secret".to_string(),
                source: "env".to_string(),
                detail: format!("环境「{env_label}」的密钥"),
                ok: true,
            }
        } else {
            VarSourceItem {
                name: name.clone(),
                kind: "secret".to_string(),
                source: "missing".to_string(),
                detail: "当前环境里没有这个密钥".to_string(),
                ok: false,
            }
        };
        items.push(item);
    }

    for name in classified.os {
        let allowed = crate::util::os_var_allowed(&os_allow, &name);
        let has = allowed && std::env::var(&name).is_ok();
        let detail = if !allowed {
            "未加入白名单（设置 → 压测 → OS 变量）"
        } else if !has {
            "白名单已加，但本机没有该变量"
        } else {
            "本机系统变量"
        };
        items.push(VarSourceItem {
            name,
            kind: "os".to_string(),
            source: "os".to_string(),
            detail: detail.to_string(),
            ok: has,
        });
    }

    // 取不到值的排前面，展开面板时第一眼就能看到要处理什么
    items.sort_by_key(|item| item.ok);
    Ok(items)
}

/// 与 `prepare_stress` 相同，但可注入集群刷新中继：
/// 主节点本地引擎与从节点都通过它把 token 刷新交给同一个单飞入口（8.10.5）。
pub(crate) fn prepare_stress_with_relay(
    state: &AppState,
    config: StressConfig,
    cancel: Arc<AtomicBool>,
    relay: Option<Arc<dyn PoolRefreshRelay>>,
) -> Result<PreparedStress, String> {
    let conn = state.pool.get().map_err(err)?;
    // 环境优先级：显式 envId → 计划/请求所属项目的激活环境（4.3）
    let resolved_env = resolve_env(&conn, &config)?;
    // 开始压测瞬间读取最近一次本机能力检测结果，写入报告快照（1.7.5）
    let machine_capacity = crate::commands::capacity::latest_result(&conn)
        .ok()
        .flatten();
    let mut vars = match resolved_env.id {
        Some(env_id) => crate::commands::http::load_vars(&conn, env_id)?,
        None => HashMap::new(),
    };
    drop(conn);
    // 运行期变量优先于环境变量
    for (key, value) in &config.vars {
        if !key.trim().is_empty() {
            vars.insert(key.clone(), value.clone());
        }
    }
    // OS 环境变量：只注入白名单内的变量（{{os:NAME}}）
    // 双键写入：专用键供 {{os:NAME}} 取值，裸名供 {{NAME}} 沿用原优先级
    let os_allow = state.settings().stress_os_env_allow;
    if !os_allow.trim().is_empty() {
        let classified =
            crate::util::required_vars_in_json(&serde_json::to_value(&config).unwrap_or_default());
        for name in classified.os {
            if !crate::util::os_var_allowed(&os_allow, &name) {
                continue;
            }
            if let Ok(value) = std::env::var(&name) {
                vars.insert(crate::util::os_var_key(&name), value.clone());
                vars.insert(name, value);
            }
        }
    }
    // 参数 / Token 池：无池时为 None，运行期不产生额外开销
    let pools = build_pool_runtime(state, &config, relay)?;
    // 预检白名单：池运行期自动注入的列名 + 显式 target
    let provided = pools
        .as_ref()
        .map(|runtime| runtime.injected_names())
        .unwrap_or_default();
    // 变量未解析时直接阻止启动，避免整轮压测全是同一个错误（4.4 / 4.5）
    check_unresolved(&config, &vars, &provided, resolved_env.name.as_deref())?;

    // Cookie 只按目标 URL 取一次：压测过程中不写回 Cookie 库，避免高并发下的写竞争
    let cookie_enabled = config.cookie_enabled.unwrap_or(true);

    // 多接口归一：未配置 requests 时退回单接口字段（旧计划兼容）
    let defs = effective_requests(&config);
    let raw_weights: Vec<f64> = defs.iter().map(|item| item.weight.unwrap_or(0.0)).collect();
    let normalized_weights = normalize_request_weights(&raw_weights);
    let mut requests: Vec<PreparedRequest> = Vec::with_capacity(defs.len());
    for (index, def) in defs.iter().enumerate() {
        let method = reqwest::Method::from_bytes(def.method.trim().to_uppercase().as_bytes())
            .map_err(|_| format!("不支持的请求方法：{}", def.method))?;
        let auth = def
            .auth
            .clone()
            .map(|auth| render_auth(&auth, &vars))
            .unwrap_or_default();
        let prepared_url = build_url_for(&def.url, &def.params, &vars, &auth)?;
        let user_has_cookie = def
            .headers
            .iter()
            .any(|item| item.enabled && item.key.trim().eq_ignore_ascii_case("cookie"));
        let cookie = if cookie_enabled && !user_has_cookie {
            state
                .pool
                .get()
                .ok()
                .map(|conn| crate::commands::cookie::cookies_for_url(&conn, &prepared_url))
                .filter(|items| !items.is_empty())
                .map(|items| items.join("; "))
        } else {
            None
        };
        let req_bytes =
            estimate_request_bytes_for(&def.url, &def.headers, def.body.as_deref(), &method);
        let weight = normalized_weights.get(index).copied().unwrap_or(0.0);
        let mut name = def.name.trim().to_string();
        if name.is_empty() {
            name = def.url.clone();
        }
        requests.push(PreparedRequest {
            request_id: def.request_id,
            name,
            method,
            url_template: def.url.clone(),
            url_text: prepared_url.to_string(),
            headers: def.headers.clone(),
            params: def.params.clone(),
            body_type: def.body_type.clone(),
            body: def.body.clone(),
            raw_auth: def.auth.clone(),
            auth,
            cookie,
            req_bytes,
            weight,
        });
    }
    let total_weight: f64 = requests.iter().map(|item| item.weight).sum();
    let primary = requests
        .first()
        .ok_or_else(|| "没有可压测的接口".to_string())?;
    let primary_url = reqwest::Url::parse(&primary.url_text)
        .map_err(|_| format!("URL 无法解析：{}", primary.url_text))?;

    // 固定并发或阶梯加压
    let (stages, peak, total_sec) = plan_run(&config);
    let target_key = target_key_of(primary.method.as_str(), &primary_url);
    // 同主机不同路径的并发压测只警告不拦截（1.3.2）：同机施压会争抢连接与带宽
    let same_host_warning = {
        let others = state.stress_jobs.same_host_others(&target_key);
        if others.is_empty() {
            None
        } else {
            let list = others
                .iter()
                .map(|(id, name)| format!("#{id}（{name}）"))
                .collect::<Vec<_>>()
                .join("、");
            Some(format!(
                "同一主机上已有任务 {list} 在施压，同机并发会互相争抢连接与带宽；\
这份结果不能当作单实例容量结论"
            ))
        }
    };
    // 并发总和超过检测饱和并发的 80% 时追加警告，不硬拦（1.3.1）
    let capacity_warning = machine_capacity.as_ref().and_then(|item| {
        if item.saturation_concurrent == 0 {
            return None;
        }
        let running: u32 = state
            .stress_jobs
            .snapshot()
            .iter()
            .map(|job| job.concurrent)
            .sum();
        let total_target = running.saturating_add(peak);
        let threshold = (item.saturation_concurrent as f64 * 0.8).round() as u32;
        if total_target > threshold {
            Some(format!(
                "本次并发总和 {total_target} 已超过本机检测饱和并发 {} 的 80%（{threshold}），结果可能无法解释",
                item.saturation_concurrent
            ))
        } else {
            None
        }
    });
    if total_sec > LONG_RUN_ACK_SEC && !config.long_run_ack {
        return Err(format!(
            "本次压测时长 {total_sec} 秒超过 2 小时，请先确认「已知悉同机施压风险」；\
建议为长稳压测准备独立的施压机"
        ));
    }

    // 多接口轮次没有单一名称：优先用户命名，其次用「首个接口 + N 个接口」表达
    let name = config
        .name
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| {
            if requests.len() > 1 {
                format!("{} 等 {} 个接口", primary.name, requests.len())
            } else {
                primary.url_template.clone()
            }
        });
    let rate = config
        .target_rps
        .map(|value| value.max(0.0).round() as u32)
        .unwrap_or(0);
    let target_rps = rate as f64;

    // 明细采集定档：容量预设等价 auto；定位预设默认全量 + TopN
    let (record_detail, top_n) = capture_plan(&config);
    let capture_mode = match record_detail {
        RecordDetailMode::Full => Capture::Full,
        RecordDetailMode::Sample => Capture::Sample,
        RecordDetailMode::Off => Capture::Off,
        RecordDetailMode::Auto => {
            // 无目标 RPS 时按 100ms 参考 RT 估算：QPS ≈ 并发 × 10
            let predicted = if rate > 0 {
                rate as f64
            } else {
                peak as f64 * 10.0
            };
            if predicted >= AUTO_OFF_QPS {
                Capture::Off
            } else if predicted >= AUTO_SAMPLE_QPS {
                Capture::Sample
            } else {
                Capture::Full
            }
        }
    };
    let capture = Arc::new(AtomicU8::new(capture_mode as u8));

    let metrics = Arc::new(Metrics::new_with_interfaces(requests.len()));
    let control = Arc::new(Control::new(
        if stages.is_empty() {
            peak
        } else {
            stages[0].concurrent
        },
        rate,
    ));
    // 按当前配置构建客户端：连接超时、重定向策略可单独覆盖，代理与客户端证书沿用应用设置
    // 空闲连接上限按压测峰值并发放大，普通请求客户端不受影响
    let client = state.client_with_options(
        config.insecure,
        config.connect_timeout_ms,
        config.follow_redirects,
        Some(peak as usize),
    )?;
    let stop_rate = config.stop_error_rate.filter(|value| *value > 0.0);
    let stop_min = config.stop_min_requests.unwrap_or(100).max(1);
    let start = Instant::now();
    let deadline = start + Duration::from_secs(total_sec);
    let concurrent = peak as i64;

    let (errors_tx, errors_rx) = mpsc::channel::<ReportError>(1024);
    let (records_tx, records_rx) = mpsc::unbounded_channel::<ReportRequestRecord>();
    // 慢请求队列有界：阈值=0 且高 QPS 时，消费端每秒才取一次，
    // 无界队列会把整段压测的慢请求全堆在内存里
    let (slow_tx, slow_rx) = mpsc::channel::<SlowRequest>(4096);
    let slow_threshold_ms = config.slow_threshold_ms.unwrap_or(0);

    // 报告记录每个接口的最终目标与权重：前端据此把 URL 汇总「翻译」成接口视图
    let request_targets = requests
        .iter()
        .map(|item| {
            json!({
                "requestId": item.request_id,
                "name": item.name,
                "method": item.method.as_str(),
                "url": item.url_text,
                "urlTemplate": item.url_template,
                "weight": item.weight,
            })
        })
        .collect::<Vec<_>>();

    let config_snapshot = json!({
        "mode": if stages.is_empty() { "fixed" } else { "stages" },
        "method": primary.method.as_str(),
        // 报告要展示「实际压测的目标」：存渲染后的完整 URL（含变量替换与拼接的查询参数），
        // 而不是 {{base_url}}/xxx 这样的模板原文；模板原文单独存 urlTemplate（3.5.1）
        "url": primary.url_text.clone(),
        "urlTemplate": primary.url_template.clone(),
        "requestCount": requests.len(),
        "target": {
            "requestId": primary.request_id,
            "method": primary.method.as_str(),
            "urlTemplate": primary.url_template.clone(),
            "resolvedUrl": primary.url_text.clone(),
            "capturedAt": now_ms(),
        },
        "headers": primary.headers.iter().filter(|h| h.enabled && !h.key.trim().is_empty()).count(),
        "params": primary.params.iter().filter(|p| p.enabled && !p.key.trim().is_empty()).count(),
        "bodyType": primary.body_type.clone(),
        // 解析后的环境与名称：换机器/换库后仍能看出当时用的是哪个环境（2.6.3）
        "envId": resolved_env.id,
        "envName": resolved_env.name.clone(),
        "projectId": config.project_id,
        "planId": config.plan_id,
        // 只记录认证方式与开关，绝不写入密钥值
        "authKind": if primary.auth.kind.trim().is_empty() { "none" } else { primary.auth.kind.as_str() },
        "apiKeyIn": primary.auth.add_to.clone(),
        "cookieEnabled": cookie_enabled,
        "runtimeVars": config.vars.len(),
        "timeoutMs": config.timeout_ms,
        "connectTimeoutMs": config.connect_timeout_ms,
        "followRedirects": config.follow_redirects,
        "stopErrorRate": config.stop_error_rate,
        "stopMinRequests": config.stop_min_requests,
        "thinkTimeMs": config.think_time_ms,
        "rampUpSec": config.ramp_up_sec,
        "targetRps": target_rps,
        "stages": stages,
        "sla": config.sla,
        // 明细采集：请求模式与实际生效模式（auto 可能已降档）
        "recordDetail": capture_mode.label(),
        "recordDetailRequested": record_detail.label(),
        "captureMode": config.capture_mode.label(),
        "topN": top_n,
        "slowThresholdMs": slow_threshold_ms,
        "baselineReportId": config.baseline_report_id,
        "machineCapacity": machine_capacity.as_ref().map(|item| json!({
            "probedAt": item.probed_at,
            "maxQps": item.max_qps,
            "cores": item.cores,
            "cpuPeak": item.cpu_peak,
            "saturationConcurrent": item.saturation_concurrent,
            "reachedLimit": item.reached_limit,
        })),
        "capacityWarning": capacity_warning,
        "sameHostWarning": same_host_warning,
        "agent": collect_agent_info(),
    });
    // 配置形态版本与批次号单独插入：json! 嵌套层数已接近宏递归上限（2.2.1 / 1.3.3 / 6.2）
    let mut config_snapshot = config_snapshot;
    if let Some(map) = config_snapshot.as_object_mut() {
        map.insert("schemaVersion".to_string(), json!(2));
        map.insert("runGroupId".to_string(), json!(config.run_group_id));
        // 嵌套层数已接近 json! 递归上限，接口清单单独插入
        map.insert("requests".to_string(), json!(request_targets));
    }
    let config_snapshot = config_snapshot.to_string();

    let url_text = if requests.len() > 1 {
        format!("{} 等 {} 个接口", primary.url_text, requests.len())
    } else {
        primary.url_text.clone()
    };

    let ctx = RunCtx {
        client,
        config,
        vars,
        pools,
        requests,
        total_weight,
        metrics,
        control,
        start,
        deadline,
        cancel,
        errors: errors_tx,
        records: records_tx,
        slow_tx,
        slow_limit: top_n as usize,
        slow_threshold_ms,
        capture,
    };

    Ok(PreparedStress {
        ctx,
        name,
        stages,
        peak,
        total_sec,
        rate,
        target_rps,
        config_snapshot,
        stop_rate,
        stop_min,
        concurrent,
        url_text,
        target_key,
        errors_rx,
        records_rx,
        slow_rx,
    })
}

/// 每秒外送采样：集群模式注入，单机模式不传
pub(crate) type EngineSampleSink = Arc<dyn Fn(ClusterSamplePayload) + Send + Sync>;

/// 引擎运行选项
///
/// 单机压测使用默认值；集群模式传入统一起跑时刻与采样外送回调。
#[derive(Default)]
pub(crate) struct EngineOptions {
    /// 统一起跑时刻（epoch 毫秒）：到点后再建 worker 与 ticker
    pub start_at_ms: Option<i64>,
    /// 每秒采样外送：主节点合并本地与从节点曲线
    pub sink: Option<EngineSampleSink>,
}

/// 不落库的引擎结果：单机报告包装与集群合并共用
#[allow(dead_code)] // W4 合并模块会读取 sparse
pub(crate) struct EngineOutcome {
    pub report: StressReport,
    pub samples: Vec<ReportSample>,
    pub errors: Vec<ReportError>,
    pub requests: Vec<ReportRequestRecord>,
    pub slow_requests: Vec<SlowRequest>,
    pub stages: Vec<StageSummary>,
    pub status_codes: Vec<StatusCount>,
    pub histogram: Vec<HistogramBucket>,
    pub body_histogram: Vec<HistogramBucket>,
    pub error_types: Vec<ErrorTypeCount>,
    pub config_snapshot: String,
    pub sparse: EngineSparseMetrics,
    /// 按 URL 聚合的统计（已脱敏）
    pub url_stats: Vec<UrlStatRow>,
    /// URL 统计是否完整（键未溢出）
    pub url_stats_complete: bool,
    /// 按接口（配置 requests 下标）的每秒曲线
    pub interface_samples: Vec<crate::models::InterfaceSamplePoint>,
    pub message: Option<String>,
}

/// 合并用逐毫秒桶与原始计数：百分位必须从桶重算，不能对分位数取平均
#[allow(dead_code)] // W4 合并模块读取
pub(crate) struct EngineSparseMetrics {
    pub total: u64,
    pub success: u64,
    pub errors: u64,
    /// 有响应的请求数（TTFB / 响应体统计分母）
    pub responses: u64,
    pub bytes: u64,
    pub bytes_sent: u64,
    pub sum_rt_ms: u64,
    pub sum_ttfb_ms: u64,
    /// 仅响应请求的响应体耗时累加
    pub sum_body_ms: u64,
    /// APDEX 分子计数：只含成功请求
    pub apdex_satisfied: u64,
    pub apdex_tolerating: u64,
    pub min_rt: f64,
    pub max_rt: f64,
    pub latency: Vec<(u32, u64)>,
    pub ttfb: Vec<(u32, u64)>,
    pub body: Vec<(u32, u64)>,
    pub status_codes: Vec<(u16, u64)>,
    pub error_types: Vec<(String, i64)>,
    pub errors_dropped: u64,
    pub records_dropped: u64,
}

/// 执行压测并落库；进度通过回调交给调用方（桌面端转 Tauri 事件）
async fn drive_stress(
    prepared: PreparedStress,
    pool: DbPool,
    job_id: u64,
    progress: Option<&(dyn Fn(StressProgress) + Send + Sync)>,
    watch: Option<&Arc<AtomicBool>>,
) -> StressExecOutcome {
    let outcome = run_engine(prepared, job_id, progress, watch, EngineOptions::default()).await;
    let (report_id, save_message) = match save_report(
        &pool,
        ReportPayload {
            report: &outcome.report,
            samples: &outcome.samples,
            errors: &outcome.errors,
            requests: &outcome.requests,
            stages: &outcome.stages,
            status_codes: &outcome.status_codes,
            histogram: &outcome.histogram,
            body_histogram: &outcome.body_histogram,
            slow_requests: &outcome.slow_requests,
            error_types: &outcome.error_types,
            url_stats: &outcome.url_stats,
            interface_samples: &outcome.interface_samples,
            config: &outcome.config_snapshot,
        },
    ) {
        Ok(id) => (Some(id), None),
        Err(error) => (None, Some(format!("报告保存失败：{error}"))),
    };
    StressExecOutcome {
        report_id,
        report: outcome.report,
        message: save_message.or(outcome.message),
    }
}

/// 只执行压测、不落库；集群模式通过 `options` 对齐起跑时刻并外送每秒采样
pub(crate) async fn run_engine(
    prepared: PreparedStress,
    job_id: u64,
    progress: Option<&(dyn Fn(StressProgress) + Send + Sync)>,
    watch: Option<&Arc<AtomicBool>>,
    options: EngineOptions,
) -> EngineOutcome {
    let PreparedStress {
        mut ctx,
        name,
        stages,
        peak,
        total_sec,
        rate,
        target_rps,
        config_snapshot,
        stop_rate,
        stop_min,
        concurrent,
        url_text: _,
        target_key: _,
        mut errors_rx,
        mut records_rx,
        mut slow_rx,
    } = prepared;

    // 统一起跑：主从两侧都在 atMs 之后才建 worker，采样时间轴才能对齐。
    // 先睡到目标时刻，再用当前 Instant 重算起点与截止时间。
    if let Some(at_ms) = options.start_at_ms {
        let wait = at_ms.saturating_sub(now_ms());
        if wait > 0 {
            tokio::time::sleep(Duration::from_millis(wait as u64)).await;
        }
        ctx.start = Instant::now();
        ctx.deadline = ctx.start + Duration::from_secs(total_sec);
    }
    let sink = options.sink;
    let ctx = Arc::new(ctx);
    let start = ctx.start;
    let deadline = ctx.deadline;
    let cancel = ctx.cancel.clone();
    let metrics = ctx.metrics.clone();
    let control = ctx.control.clone();

    // 限速令牌按 100ms 平滑补充：原先每秒一次性补满会让请求在秒初扎堆，
    // 高 QPS 时形成明显锯齿；上限 1.5 秒量，避免进程卡顿后一次性爆发。
    let mut refill_task = if rate > 0 {
        let control = control.clone();
        let cancel = cancel.clone();
        Some(tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_millis(100));
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            ticker.tick().await;
            let mut last = Instant::now();
            // 小数令牌累计：rate=10.5 时不能每 tick 都向下取整成 10
            let mut carry = 0.0f64;
            let cap = (rate as f64 * 1.5).max(1.0);
            loop {
                ticker.tick().await;
                if cancel.load(Ordering::Relaxed) || Instant::now() >= deadline {
                    break;
                }
                let now = Instant::now();
                let dt = now.duration_since(last).as_secs_f64().clamp(0.001, 1.0);
                last = now;
                let want = rate as f64 * dt + carry;
                let whole = want.floor();
                carry = want - whole;
                if whole >= 1.0 {
                    let add = whole as u64;
                    // 新版工具链把 fetch_update 标记为 deprecated，旧版没有 try_update，
                    // 这里保留兼容名并在语句级别忽略告警
                    #[allow(deprecated)]
                    let _ = control.tokens.fetch_update(
                        Ordering::Relaxed,
                        Ordering::Relaxed,
                        |current| Some(((current as f64 + add as f64).min(cap)) as u64),
                    );
                    // 唤醒在 take_token 里等待的 worker
                    control.token_wake.notify_waiters();
                }
            }
        }))
    } else {
        None
    };

    let mut tasks = JoinSet::new();
    for index in 0..peak {
        tasks.spawn(worker(ctx.clone(), index, peak));
    }

    let mut samples: Vec<ReportSample> = Vec::new();
    // 按接口的每秒采样点：与全局 samples 同时间轴，供报告画接口曲线
    let mut interface_samples: Vec<crate::models::InterfaceSamplePoint> = Vec::new();
    let mut errors: Vec<ReportError> = Vec::new();
    let mut requests: Vec<ReportRequestRecord> = Vec::new();
    let mut slow_requests: Vec<SlowRequest> = Vec::new();
    // 自动降档轨迹：报告里记录每次降档的时间点与当时 QPS（1.6.3）
    let mut downgrade_trail: Vec<serde_json::Value> = Vec::new();
    // 慢请求候选缓冲上限：定位模式低并发，留 8 倍余量避免每 tick 排序
    let slow_cap = ctx.slow_limit.saturating_mul(8).max(64);
    let mut prev_total = 0u64;
    let mut prev_success = 0u64;
    let mut prev_errors = 0u64;
    let mut prev_bytes = 0u64;
    // 集群采样外送：累计耗时与逐毫秒桶都需要按采样间隔取增量
    let mut prev_sum_rt = 0u64;
    let mut prev_sum_ttfb = 0u64;
    let mut prev_sum_body = 0u64;
    let mut prev_responses = 0u64;
    let mut prev_latency = vec![0u64; metrics.buckets.len()];
    let mut prev_body = vec![0u64; metrics.body_buckets.len()];
    // 各接口累计值快照：采样时取差值，得到本区间各接口 QPS
    let mut prev_interfaces = vec![(0u64, 0u64, 0u64, 0u64); metrics.interface_counters.len()];
    // 阶梯汇总：按阶段累积精确的请求数、总耗时与延迟直方图
    let mut stage_accums: Vec<StageAccum> =
        (0..stages.len()).map(|_| StageAccum::default()).collect();
    let mut last_stage_index = -1i64;
    let mut left_sec = total_sec;
    let mut auto_stop: Option<String> = None;
    // 已下发到前端的采样点数量（进度事件只发增量）
    let mut emitted = 0usize;
    let mut notice: Option<String> = None;
    let mut qps_window: VecDeque<f64> = VecDeque::new();
    let mut monitor = ResourceMonitor::new();
    // 长压测降低采样频率：> 2h 每 2s、> 6h 每 5s 一个采样点
    let sample_step: u64 = if total_sec > SAMPLE_DECIMATE_5S_SEC {
        5
    } else if total_sec > SAMPLE_DECIMATE_2S_SEC {
        2
    } else {
        1
    };

    let mut ticker = tokio::time::interval(Duration::from_millis(1000));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    // 采样间隔必须与被统计的计数区间一致：记录上一次**实际采样**的时刻，
    // 否则 tick 漂移或降频采样时 qps 会被算成两倍（用相邻 tick 差当时钟的经典错误）
    let mut last_sample_at = Instant::now();
    let mut last_sample_sec = 0u64;

    loop {
        ticker.tick().await;
        // 套件运行被停止时同步结束压测，最迟 1 秒内生效
        if let Some(flag) = watch {
            if flag.load(Ordering::SeqCst) {
                cancel.store(true, Ordering::SeqCst);
            }
        }
        left_sec = left_sec.saturating_sub(1);

        while let Ok(item) = errors_rx.try_recv() {
            if errors.len() < MAX_ERRORS {
                errors.push(item);
            } else {
                metrics.errors_dropped.fetch_add(1, Ordering::Relaxed);
            }
        }
        while let Ok(item) = records_rx.try_recv() {
            if requests.len() < MAX_REQUEST_RECORDS {
                requests.push(item);
            } else {
                metrics.records_dropped.fetch_add(1, Ordering::Relaxed);
            }
        }
        while let Ok(item) = slow_rx.try_recv() {
            if slow_requests.len() < slow_cap {
                slow_requests.push(item);
            }
        }
        if slow_requests.len() >= slow_cap {
            // 只留最慢的 N 条，避免阈值=0 时无限堆积
            slow_requests.sort_by_key(|item| std::cmp::Reverse(item.latency_ms));
            slow_requests.truncate(ctx.slow_limit.max(1));
        }

        let elapsed = start.elapsed().as_secs();
        let total = metrics.count();
        let success = metrics.success.load(Ordering::Relaxed);
        let failure = metrics.errors.load(Ordering::Relaxed);
        let bytes = metrics.bytes.load(Ordering::Relaxed);

        // 阶梯加压：按时间切换生效并发；变化时唤醒等待中的 worker
        let (active, stage_index) = stage_at(&stages, elapsed, peak);
        let previous_active = control.active.swap(active, Ordering::Relaxed);
        control.stage.store(stage_index, Ordering::Relaxed);
        if previous_active != active {
            control.wake.notify_waiters();
        }

        monitor.sample();

        let stage_changed = stage_index != last_stage_index;
        // 阶梯切换时强制补一个采样点：否则 2s/5s 降频采样会把跨阶段的一整段
        // 请求全部算到下一个阶段，阶段报告的 QPS / P95 就会串档
        if elapsed > 0 && (elapsed.saturating_sub(last_sample_sec) >= sample_step || stage_changed)
        {
            // QPS 按真实采样间隔归一：高负载下 ticker 会漂移，直接取相邻计数差会高估
            let now_sample = Instant::now();
            let dt = now_sample
                .duration_since(last_sample_at)
                .as_secs_f64()
                .clamp(0.001, 60.0);
            // 本段请求实际发生在 [上次采样, 本次采样) 之间，按区间起点归属阶段
            let acc_stage_index = stage_at(&stages, last_sample_sec, peak).1;
            last_sample_at = now_sample;
            last_sample_sec = elapsed;

            let delta_total = total.saturating_sub(prev_total);
            let delta_success = success.saturating_sub(prev_success);
            let delta_errors = failure.saturating_sub(prev_errors);
            let delta_bytes = bytes.saturating_sub(prev_bytes);
            let sum_rt = metrics.sum_ms.load(Ordering::Relaxed);
            let delta_rt = sum_rt.saturating_sub(prev_sum_rt);
            prev_sum_rt = sum_rt;
            let sum_ttfb = metrics.sum_ttfb.load(Ordering::Relaxed);
            let delta_ttfb = sum_ttfb.saturating_sub(prev_sum_ttfb);
            prev_sum_ttfb = sum_ttfb;
            let sum_body = metrics.sum_body.load(Ordering::Relaxed);
            let delta_body = sum_body.saturating_sub(prev_sum_body);
            prev_sum_body = sum_body;
            let responses = metrics.responses.load(Ordering::Relaxed);
            let delta_responses = responses.saturating_sub(prev_responses);
            prev_responses = responses;

            let latency_buckets = bucket_delta(&metrics.buckets, &mut prev_latency);
            let body_buckets = bucket_delta(&metrics.body_buckets, &mut prev_body);

            let qps = delta_total as f64 / dt;
            // 曲线按「本采样区间」统计，而不是启动以来的累计值：
            // 累计值画出来只会是一条收敛曲线，阶梯压测的阶段对比会完全失真
            let avg_rt = if delta_total > 0 {
                delta_rt as f64 / delta_total as f64
            } else {
                0.0
            };
            let ttfb_avg = if delta_responses > 0 {
                delta_ttfb as f64 / delta_responses as f64
            } else {
                0.0
            };
            let body_avg = if delta_responses > 0 {
                delta_body as f64 / delta_responses as f64
            } else {
                0.0
            };
            samples.push(ReportSample {
                elapsed_sec: elapsed,
                concurrent: metrics.concurrent.load(Ordering::Relaxed),
                qps,
                avg_rt,
                p50_rt: percentile_sparse(&latency_buckets, delta_total, 0.5),
                p95_rt: percentile_sparse(&latency_buckets, delta_total, 0.95),
                p99_rt: percentile_sparse(&latency_buckets, delta_total, 0.99),
                ttfb_avg,
                body_avg,
                body_p95: percentile_sparse(&body_buckets, delta_responses, 0.95),
                bytes: delta_bytes,
                success_count: delta_success,
                error_count: delta_errors,
                response_count: delta_responses,
            });

            // 接口维度：本区间各接口增量，QPS 同样按真实采样间隔归一
            let iface_now = metrics.interface_snapshot();
            let mut iface_points: Vec<crate::models::InterfaceSample> = Vec::new();
            for (iface_index, current) in iface_now.iter().enumerate() {
                let prev = prev_interfaces
                    .get(iface_index)
                    .copied()
                    .unwrap_or((0, 0, 0, 0));
                let d_total = current.0.saturating_sub(prev.0);
                if d_total == 0 && current.3 == prev.3 {
                    continue;
                }
                let d_sum = current.3.saturating_sub(prev.3);
                iface_points.push(crate::models::InterfaceSample {
                    index: iface_index as u32,
                    qps: d_total as f64 / dt,
                    success_count: current.1.saturating_sub(prev.1),
                    error_count: current.2.saturating_sub(prev.2),
                    sum_rt_ms: d_sum,
                    avg_rt: if d_total > 0 {
                        d_sum as f64 / d_total as f64
                    } else {
                        0.0
                    },
                });
            }
            prev_interfaces = iface_now;
            for point in &iface_points {
                interface_samples.push(crate::models::InterfaceSamplePoint {
                    elapsed_sec: elapsed,
                    index: point.index,
                    qps: point.qps,
                    success_count: point.success_count,
                    error_count: point.error_count,
                    avg_rt: point.avg_rt,
                });
            }

            // 阶梯汇总：用区间增量累加到所属阶段，阶段内 P95 由真实直方图重算
            if let Some(accum) = stage_accums.get_mut(acc_stage_index.max(0) as usize) {
                accum.requests += delta_total;
                accum.errors += delta_errors;
                accum.sum_rt += delta_rt;
                for &(index, count) in &latency_buckets {
                    *accum.latency.entry(index).or_insert(0) += count;
                }
            }

            // 集群模式外送本秒增量：QPS / 计数 / 耗时累加 / 稀疏直方图
            if let Some(sink) = &sink {
                sink(ClusterSamplePayload {
                    elapsed_sec: elapsed,
                    concurrent: metrics.concurrent.load(Ordering::Relaxed),
                    qps,
                    avg_rt,
                    ttfb_avg,
                    body_avg,
                    bytes: delta_bytes,
                    success_count: delta_success,
                    error_count: delta_errors,
                    sum_rt_ms: delta_rt,
                    sum_ttfb_ms: delta_ttfb,
                    response_count: delta_responses,
                    sum_body_ms: delta_body,
                    latency_buckets,
                    body_buckets,
                    interfaces: iface_points,
                    // 本机资源由调用方（从节点 sink）补齐，单机路径与主节点引擎保持 None
                    host: None,
                });
            }

            prev_total = total;
            prev_success = success;
            prev_errors = failure;
            prev_bytes = bytes;

            // auto 定档：连续 N 个采样点超阈值后降档，只降不升
            qps_window.push_back(qps);
            while qps_window.len() > AUTO_DOWNGRADE_WINDOW {
                qps_window.pop_front();
            }
            if qps_window.len() == AUTO_DOWNGRADE_WINDOW {
                let lowest = qps_window.iter().cloned().fold(f64::MAX, f64::min);
                let current = Capture::from_u8(ctx.capture.load(Ordering::Relaxed));
                if current != Capture::Off && lowest >= AUTO_OFF_QPS {
                    ctx.capture.store(Capture::Off as u8, Ordering::Relaxed);
                    notice = Some(format!(
                        "实测 QPS 已达 {:.0}，已自动关闭逐请求明细（聚合指标不受影响）",
                        lowest
                    ));
                    downgrade_trail.push(json!({
                        "atSec": elapsed,
                        "from": current.label(),
                        "to": "off",
                        "qps": (lowest * 10.0).round() / 10.0,
                    }));
                    log::info!("压测自动降档：capture=off，qps={lowest:.0}");
                } else if current == Capture::Full && lowest >= AUTO_SAMPLE_QPS {
                    ctx.capture.store(Capture::Sample as u8, Ordering::Relaxed);
                    notice = Some(format!("实测 QPS 已达 {:.0}，明细已切换为 1% 采样", lowest));
                    downgrade_trail.push(json!({
                        "atSec": elapsed,
                        "from": current.label(),
                        "to": "sample",
                        "qps": (lowest * 10.0).round() / 10.0,
                    }));
                    log::info!("压测自动降档：capture=sample，qps={lowest:.0}");
                }
                qps_window.clear();
            }
        }
        last_stage_index = stage_index;

        if let Some(sink) = progress {
            // 只下发增量采样点，避免长压测下每秒 clone 全量序列
            let delta: Vec<ReportSample> = samples[emitted.min(samples.len())..].to_vec();
            emitted = samples.len();
            sink(StressProgress {
                job_id,
                name: name.clone(),
                running: true,
                elapsed_sec: elapsed,
                total,
                success,
                errors: failure,
                rps: if elapsed > 0 {
                    total as f64 / elapsed as f64
                } else {
                    0.0
                },
                avg_rt: metrics.avg(),
                p95_rt: metrics.percentile(0.95),
                p99_rt: metrics.percentile(0.99),
                concurrent: metrics.concurrent.load(Ordering::Relaxed),
                stage_index,
                active_concurrent: active,
                status_codes: metrics.status_codes(),
                samples: delta,
                record_mode: Capture::from_u8(ctx.capture.load(Ordering::Relaxed))
                    .label()
                    .to_string(),
                notice: notice.take(),
            });
        }

        // 自动停止判定：错误率达到阈值且请求数足够时提前结束
        if auto_stop.is_none() {
            if let Some(limit) = stop_rate {
                if total >= stop_min as u64 && total > 0 {
                    let error_rate = failure as f64 / total as f64 * 100.0;
                    if error_rate > limit {
                        auto_stop = Some(format!(
                            "错误率 {:.1}% 超过阈值 {:.1}%，已自动停止",
                            error_rate, limit
                        ));
                        cancel.store(true, Ordering::SeqCst);
                    }
                }
            }
        }

        let stopped = cancel.load(Ordering::SeqCst);
        if stopped || Instant::now() >= deadline || left_sec == 0 {
            break;
        }
    }

    // 报告时长按「停止发压的时刻」计算，不把等待在途请求的 drain 时间算进去，
    // 否则固定 60s 的压测可能显示成 63-65s，平均 QPS 也会被稀释
    let run_end = Instant::now();
    let run_end_ms = now_ms();

    if let Some(task) = refill_task.take() {
        task.abort();
    }

    // 等待在途请求结束，最多等 5 秒
    let wait_all = async { while tasks.join_next().await.is_some() {} };
    if tokio::time::timeout(Duration::from_secs(5), wait_all)
        .await
        .is_err()
    {
        tasks.abort_all();
    }

    while let Ok(item) = errors_rx.try_recv() {
        if errors.len() < MAX_ERRORS {
            errors.push(item);
        }
    }
    while let Ok(item) = records_rx.try_recv() {
        if requests.len() < MAX_REQUEST_RECORDS {
            requests.push(item);
        }
    }
    while let Ok(item) = slow_rx.try_recv() {
        if slow_requests.len() < slow_cap {
            slow_requests.push(item);
        }
    }
    // 最终 TopN：按总耗时降序取前 N 条
    slow_requests.sort_by_key(|item| std::cmp::Reverse(item.latency_ms));
    slow_requests.truncate(ctx.slow_limit);

    let elapsed = run_end.duration_since(start).as_secs_f64().max(0.001);
    let total = metrics.count();
    let success = metrics.success.load(Ordering::Relaxed);
    let failure = metrics.errors.load(Ordering::Relaxed);
    let status = if cancel.load(Ordering::SeqCst) {
        "stopped"
    } else {
        "completed"
    };
    let end_time = run_end_ms;
    let start_time = end_time - (elapsed * 1000.0) as i64;
    // 极短压测（<1s）还没有采样点时用总量/耗时兜底，避免峰值 QPS 显示 0
    let max_qps = samples
        .iter()
        .map(|s| s.qps)
        .fold(0.0, f64::max)
        .max(if total > 0 {
            total as f64 / elapsed
        } else {
            0.0
        });

    let histogram = metrics.histogram();
    let body_histogram = metrics.body_histogram();
    let status_codes = metrics.status_codes();
    let stage_summaries = summarize_stages(&stages, &samples, &stage_accums);
    let error_types = metrics.error_types();
    // URL 聚合与明细采集解耦：这里拿到最终（已脱敏）的逐地址统计
    let (url_stats, url_stats_complete) = metrics.url_stats();

    // 运行期指标并入配置快照：避免为统计项新增表列，读取集中在 ReportDetail.config
    let config_snapshot = with_runtime_metrics(
        &config_snapshot,
        json!({
            "bytesSent": metrics.bytes_sent.load(Ordering::Relaxed),
            "errorsDropped": metrics.errors_dropped.load(Ordering::Relaxed),
            "recordsDropped": metrics.records_dropped.load(Ordering::Relaxed),
            "slowDropped": metrics.slow_dropped.load(Ordering::Relaxed),
            // URL 键溢出时页面与导出要标注「统计不完整」
            "urlStatsComplete": url_stats_complete,
            "recordMode": Capture::from_u8(ctx.capture.load(Ordering::Relaxed)).label(),
            // 自动降档轨迹：事后能解释「明细为何缺失」
            "downgradeTrail": downgrade_trail,
            "slowRequestCount": slow_requests.len(),
            "cpuPeakCores": (monitor.cpu_peak * 10.0).round() / 10.0,
            "memPeakMb": monitor.mem_peak_mb,
            "sampleIntervalSec": sample_step,
        }),
    );
    // 峰值专题与集群报告同一口径：报告页 / 导出 / AI 都读 config.metricsPeaks
    let metrics_peaks = crate::services::cluster::merge::build_peaks(&samples, start_time);
    let mut config_snapshot = with_metrics_peaks(&config_snapshot, &metrics_peaks);

    let report = StressReport {
        id: 0,
        plan_id: ctx.config.plan_id,
        run_group_id: ctx.config.run_group_id,
        plan_name: name,
        title: None,
        request_id: ctx.config.request_id,
        total_requests: total as i64,
        success_count: success as i64,
        error_count: failure as i64,
        avg_rt: metrics.avg(),
        min_rt: metrics.min(),
        max_rt: metrics.max(),
        p50_rt: metrics.percentile(0.5),
        p75_rt: metrics.percentile(0.75),
        p90_rt: metrics.percentile(0.9),
        p95_rt: metrics.percentile(0.95),
        p99_rt: metrics.percentile(0.99),
        p999_rt: metrics.percentile(0.999),
        stddev_rt: metrics.stddev(),
        ttfb_avg: metrics.ttfb_avg(),
        ttfb_p95: metrics.ttfb_percentile(0.95),
        apdex: metrics.apdex(),
        bytes_total: metrics.bytes.load(Ordering::Relaxed) as i64,
        max_qps,
        avg_qps: if elapsed > 0.0 {
            total as f64 / elapsed
        } else {
            0.0
        },
        target_rps,
        error_rate: if total > 0 {
            failure as f64 / total as f64 * 100.0
        } else {
            0.0
        },
        concurrency: concurrent,
        status: status.to_string(),
        is_cluster: false,
        peak_qps: metrics_peaks.qps.value,
        start_time: Some(start_time),
        end_time: Some(end_time),
        created_at: end_time,
    };

    // 合并用原始桶：百分位按桶重算，不能对各节点分位数取平均
    let sparse = EngineSparseMetrics {
        total,
        success,
        errors: failure,
        responses: metrics.responses.load(Ordering::Relaxed),
        bytes: metrics.bytes.load(Ordering::Relaxed),
        bytes_sent: metrics.bytes_sent.load(Ordering::Relaxed),
        sum_rt_ms: metrics.sum_ms.load(Ordering::Relaxed),
        sum_ttfb_ms: metrics.sum_ttfb.load(Ordering::Relaxed),
        sum_body_ms: metrics.sum_body.load(Ordering::Relaxed),
        apdex_satisfied: metrics.apdex_satisfied.load(Ordering::Relaxed),
        apdex_tolerating: metrics.apdex_tolerating.load(Ordering::Relaxed),
        min_rt: metrics.min(),
        max_rt: metrics.max(),
        latency: sparse_buckets(&metrics.buckets),
        ttfb: sparse_buckets(&metrics.ttfb_buckets),
        body: sparse_buckets(&metrics.body_buckets),
        status_codes: status_codes
            .iter()
            .map(|item| (item.code.max(0) as u16, item.count.max(0) as u64))
            .collect(),
        error_types: error_types
            .iter()
            .map(|item| (item.error_type.clone(), item.count))
            .collect(),
        errors_dropped: metrics.errors_dropped.load(Ordering::Relaxed),
        records_dropped: metrics.records_dropped.load(Ordering::Relaxed),
    };

    // 池运行统计：只写池名、条目数与使用计数，不含任何明文（8.8）
    if let Some(pools) = ctx.pools.as_ref() {
        if let Ok(mut snapshot) = serde_json::from_str::<serde_json::Value>(&config_snapshot) {
            if let Some(map) = snapshot.as_object_mut() {
                map.insert("pools".to_string(), pools.report_json());
                config_snapshot = snapshot.to_string();
            }
        }
    }

    EngineOutcome {
        report,
        samples,
        errors,
        requests,
        slow_requests,
        stages: stage_summaries,
        status_codes,
        histogram,
        body_histogram,
        error_types,
        config_snapshot,
        sparse,
        url_stats,
        url_stats_complete,
        interface_samples: decimate_interface_samples(interface_samples),
        message: auto_stop,
    }
}

/// 接口曲线抽稀：每个接口最多保留约 2 万点
///
/// 长压测（24h）本身已按 2s/5s 降频采样，这里只防御极端情况下的表体积，
/// 常规时长（≤5.5h、1s 采样）不会丢点。
pub(crate) fn decimate_interface_samples(
    points: Vec<crate::models::InterfaceSamplePoint>,
) -> Vec<crate::models::InterfaceSamplePoint> {
    const MAX_POINTS: usize = 20_000;
    use std::collections::BTreeMap;
    let mut groups: BTreeMap<u32, Vec<crate::models::InterfaceSamplePoint>> = BTreeMap::new();
    for point in points {
        groups.entry(point.index).or_default().push(point);
    }
    let mut out = Vec::new();
    for (_, items) in groups {
        let step = (items.len() / MAX_POINTS).max(1);
        out.extend(items.into_iter().step_by(step));
    }
    out.sort_by_key(|point| (point.elapsed_sec, point.index));
    out
}

/// 全量桶转稀疏形式（只保留非零桶）
fn sparse_buckets(buckets: &[AtomicU64]) -> Vec<(u32, u64)> {    buckets
        .iter()
        .enumerate()
        .filter_map(|(index, bucket)| {
            let count = bucket.load(Ordering::Relaxed);
            (count > 0).then_some((index as u32, count))
        })
        .collect()
}

/// 按采样间隔取桶增量：集群每秒只传本秒新增的毫秒桶
fn bucket_delta(buckets: &[AtomicU64], previous: &mut [u64]) -> Vec<(u32, u64)> {
    let mut items = Vec::new();
    for (index, bucket) in buckets.iter().enumerate() {
        let value = bucket.load(Ordering::Relaxed);
        if value > previous[index] {
            items.push((index as u32, value - previous[index]));
            previous[index] = value;
        }
    }
    items
}

/// 本机并发预算与来源（auto / manual、CPU/内存/fd/能力检测）
#[tauri::command]
pub fn stress_budget_get(state: State<'_, AppState>) -> crate::state::ConcurrencyBudget {
    state.budget()
}

/// 调整预算模式；manual 时必须同时给出上限（1..=100000）
#[tauri::command]
pub fn stress_budget_set(
    state: State<'_, AppState>,
    mode: String,
    manual_limit: Option<i64>,
) -> Result<crate::state::ConcurrencyBudget, String> {
    state.set_budget(&mode, manual_limit)
}

#[tauri::command]
pub async fn stress_start(
    state: State<'_, AppState>,
    app: AppHandle,
    config: StressConfig,
) -> Result<u64, String> {
    if state.capacity.is_running() {
        return Err(
            "本机能力检测正在运行，检测期间不能启动压测；请等待检测结束或先取消检测".to_string(),
        );
    }
    let cancel = Arc::new(AtomicBool::new(false));
    let prepared = prepare_stress(state.inner(), config.clone(), cancel)?;
    let (max_jobs, max_total) = state.stress_limits();
    let draft = StressJobDraft {
        name: prepared.name.clone(),
        target: prepared.url_text.clone(),
        target_key: prepared.target_key.clone(),
        plan_id: config.plan_id,
        request_id: config.request_id,
        concurrent: prepared.peak,
    };
    // 任务数 / 并发之和 / 同目标三项校验在同一把锁内完成
    let (job_id, job_cancel, overlaps) =
        state
            .stress_jobs
            .try_reserve(draft, max_jobs, max_total, config.ignore_overlap)?;
    let mut prepared = prepared;
    // 停止按钮走注册表：必须把注册表的取消位换成引擎实际监听的那一个
    prepared.ctx.cancel = job_cancel;
    if !overlaps.is_empty() {
        prepared.config_snapshot = with_overlap(&prepared.config_snapshot, &overlaps);
    }
    let pool = state.pool.clone();
    let registry = state.stress_jobs.clone();
    tauri::async_runtime::spawn(async move {
        let progress_app = app.clone();
        // 每秒 QPS 同步回注册表，主窗口面板无需订阅事件也能显示实时速率
        let progress_registry = registry.clone();
        let sink = move |progress: StressProgress| {
            progress_registry.set_qps(job_id, progress.rps);
            let _ = progress_app.emit("stress:progress", &progress);
        };
        let outcome = drive_stress(prepared, pool, job_id, Some(&sink), None).await;
        // 报告已落库，任务从注册表移除；窗口仍可通过 done 事件展示结果
        registry.finish(job_id);
        let elapsed_sec = outcome
            .report
            .end_time
            .unwrap_or(0)
            .saturating_sub(outcome.report.start_time.unwrap_or(0))
            as u64
            / 1000;
        let _ = app.emit(
            "stress:done",
            &StressDone {
                job_id,
                report_id: outcome.report_id,
                name: outcome.report.plan_name.clone(),
                status: outcome.report.status.clone(),
                total: outcome.report.total_requests.max(0) as u64,
                success: outcome.report.success_count.max(0) as u64,
                errors: outcome.report.error_count.max(0) as u64,
                elapsed_sec,
                message: outcome.message,
            },
        );
    });

    Ok(job_id)
}

/// 套件内执行压测用例：与手工压测共用任务注册表，结束（含出错）后释放
pub async fn run_stress_case(
    state: &AppState,
    config: StressConfig,
    watch: Option<&Arc<AtomicBool>>,
) -> Result<StressExecOutcome, String> {
    if state.capacity.is_running() {
        return Err("本机能力检测正在运行，检测期间不能启动压测".to_string());
    }
    let cancel = Arc::new(AtomicBool::new(false));
    let prepared = prepare_stress(state, config.clone(), cancel)?;
    let (max_jobs, max_total) = state.stress_limits();
    let draft = StressJobDraft {
        name: prepared.name.clone(),
        target: prepared.url_text.clone(),
        target_key: prepared.target_key.clone(),
        plan_id: config.plan_id,
        request_id: config.request_id,
        concurrent: prepared.peak,
    };
    let (job_id, job_cancel, overlaps) = state
        .stress_jobs
        .try_reserve(draft, max_jobs, max_total, config.ignore_overlap)
        .map_err(|error| format!("压测任务无法启动：{error}"))?;
    let mut prepared = prepared;
    // 套件停止与手工停止共用注册表取消位
    prepared.ctx.cancel = job_cancel;
    if !overlaps.is_empty() {
        prepared.config_snapshot = with_overlap(&prepared.config_snapshot, &overlaps);
    }
    let pool = state.pool.clone();
    let outcome = drive_stress(prepared, pool, job_id, None, watch).await;
    state.stress_jobs.finish(job_id);
    Ok(outcome)
}

/// 阶梯阶段的精确累加器：请求数、错误数、总耗时与延迟直方图
#[derive(Default)]
struct StageAccum {
    requests: u64,
    errors: u64,
    sum_rt: u64,
    latency: BTreeMap<u32, u64>,
}

/// 从稀疏桶计算分位数（桶内线性插值）
fn percentile_from_iter(items: impl Iterator<Item = (u32, u64)>, total: u64, p: f64) -> f64 {
    if total == 0 {
        return 0.0;
    }
    let target = ((total as f64) * p).ceil() as u64;
    let mut acc = 0u64;
    let mut last = 0u32;
    for (index, count) in items {
        if count == 0 {
            continue;
        }
        last = index;
        let before = acc;
        acc += count;
        if acc >= target {
            let rank = target.saturating_sub(before) as f64;
            let offset = ((rank - 0.5) / count as f64).clamp(0.0, 1.0);
            return index as f64 + offset;
        }
    }
    last as f64
}

fn percentile_sparse(items: &[(u32, u64)], total: u64, p: f64) -> f64 {
    percentile_from_iter(
        items.iter().map(|(index, count)| (*index, *count)),
        total,
        p,
    )
}

/// 按阶段把采样归并成阶段性汇总
///
/// 优先使用引擎内按阶段累积的真实直方图（P95 精确、QPS 按阶段实际覆盖秒数）；
/// 没有累积数据时退回采样窗口估算，保证旧报告仍可展示。
fn summarize_stages(
    stages: &[StressStage],
    samples: &[ReportSample],
    accums: &[StageAccum],
) -> Vec<StageSummary> {
    if stages.is_empty() {
        return Vec::new();
    }
    let mut result = Vec::new();
    let mut start_sec = 0u64;
    for (index, stage) in stages.iter().enumerate() {
        let end_sec = start_sec + stage.duration_sec as u64;
        let window: Vec<&ReportSample> = samples
            .iter()
            .filter(|s| s.elapsed_sec > start_sec && s.elapsed_sec <= end_sec)
            .collect();
        let covered_sec = window
            .iter()
            .map(|s| s.elapsed_sec)
            .max()
            .unwrap_or(end_sec)
            .min(end_sec)
            .saturating_sub(start_sec)
            .max(1);
        let (requests, errors, avg_rt, p95_rt) = match accums.get(index) {
            Some(accum) if accum.requests > 0 => (
                accum.requests as i64,
                accum.errors as i64,
                accum.sum_rt as f64 / accum.requests as f64,
                percentile_from_iter(
                    accum.latency.iter().map(|(key, count)| (*key, *count)),
                    accum.requests,
                    0.95,
                ),
            ),
            _ => {
                let requests: i64 = window
                    .iter()
                    .map(|s| (s.success_count + s.error_count) as i64)
                    .sum();
                let errors: i64 = window.iter().map(|s| s.error_count as i64).sum();
                let avg_rt = if window.is_empty() {
                    0.0
                } else {
                    window.iter().map(|s| s.avg_rt).sum::<f64>() / window.len() as f64
                };
                (
                    requests,
                    errors,
                    avg_rt,
                    window.iter().map(|s| s.p95_rt).fold(0.0, f64::max),
                )
            }
        };
        result.push(StageSummary {
            index: index as i64,
            concurrent: stage.concurrent as i64,
            duration_sec: stage.duration_sec as i64,
            requests,
            errors,
            // 分母是阶段实际覆盖的秒数，不是采样点个数：
            // 2s/5s 降频采样时用采样点数会把 QPS 放大 2/5 倍
            avg_qps: requests as f64 / covered_sec as f64,
            avg_rt,
            p95_rt,
        });
        start_sec = end_sec;
    }
    result
}

/// 只停止指定任务；窗口 / 主面板的停止入口都走这里
#[tauri::command]
pub fn stress_stop(state: State<'_, AppState>, job_id: u64) -> Result<(), String> {
    if state.stress_jobs.cancel(job_id) {
        Ok(())
    } else {
        Err(format!("压测任务 #{job_id} 不存在或已结束"))
    }
}

/// 全部停止（退出确认后使用），返回被取消的任务数
#[tauri::command]
pub fn stress_stop_all(state: State<'_, AppState>) -> Result<usize, String> {
    Ok(state.stress_jobs.cancel_all())
}

/// 运行中任务快照：主窗口面板与窗口恢复共用，不依赖任何窗口是否打开
#[tauri::command]
pub fn stress_jobs(state: State<'_, AppState>) -> Vec<crate::state::StressJobSummary> {
    state.stress_jobs.snapshot()
}

/// 打开独立压测窗口（1.2.1）
///
/// 绑定任务的窗口 label 固定为 `stress-job-<jobId>`，重复打开时聚焦已有窗口；
/// 只有计划没有任务时用 `stress-plan-<planId>`；都没有时用时间戳序号。
/// 窗口只是视图：关闭不影响任务，重新打开时按 jobId 恢复进度。
#[tauri::command]
pub fn stress_open_window(
    app: AppHandle,
    plan_id: Option<i64>,
    job_id: Option<u64>,
    env_id: Option<i64>,
) -> Result<String, String> {
    open_stress_window(&app, plan_id, job_id, env_id)
}

/// 供命令与单实例回调共用的建窗实现
pub fn open_stress_window(
    app: &AppHandle,
    plan_id: Option<i64>,
    job_id: Option<u64>,
    env_id: Option<i64>,
) -> Result<String, String> {
    let state = app.state::<AppState>();
    let label = match (job_id, plan_id) {
        (Some(id), _) => state
            .stress_jobs
            .window_label(id)
            .unwrap_or_else(|| format!("stress-job-{id}")),
        (None, Some(id)) => format!("stress-plan-{id}"),
        (None, None) => format!("stress-{}", now_ms()),
    };
    let mut url = String::from("index.html#/stress?newWindow=1");
    if let Some(id) = plan_id {
        url.push_str(&format!("&planId={id}"));
    }
    if let Some(id) = job_id {
        url.push_str(&format!("&jobId={id}"));
    }
    if let Some(id) = env_id {
        url.push_str(&format!("&envId={id}"));
    }

    // 窗口标题优先取运行中任务名，其次取计划名
    let title = job_id
        .and_then(|id| {
            state
                .stress_jobs
                .snapshot()
                .into_iter()
                .find(|job| job.id == id)
                .map(|job| job.name)
        })
        .or_else(|| {
            plan_id.and_then(|id| {
                state.pool.get().ok().and_then(|conn| {
                    conn.query_row("SELECT name FROM test_plans WHERE id = ?1", [id], |row| {
                        row.get::<_, String>(0)
                    })
                    .optional()
                    .ok()
                    .flatten()
                })
            })
        })
        .map(|name| format!("压测 · {name}"))
        .unwrap_or_else(|| "压测".to_string());

    if let Some(id) = job_id {
        state.stress_jobs.set_window_label(id, &label);
    }
    // 已有窗口直接聚焦，避免重复建窗
    if let Some(window) = app.get_webview_window(&label) {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
        return Ok(label);
    }

    // create_window 会阻塞等待主事件循环，放到独立线程 + 二次检查（同 quick.rs）
    let handle = app.clone();
    let thread_label = label.clone();
    std::thread::spawn(move || {
        if let Some(window) = handle.get_webview_window(&thread_label) {
            let _ = window.show();
            let _ = window.unminimize();
            let _ = window.set_focus();
            return;
        }
        match WebviewWindowBuilder::new(&handle, &thread_label, WebviewUrl::App(url.into()))
            .title(title)
            .inner_size(1280.0, 860.0)
            .min_inner_size(960.0, 640.0)
            .build()
        {
            Ok(window) => {
                let _ = window.set_focus();
            }
            Err(error) => log::warn!("创建压测窗口失败：{error}"),
        }
    });
    Ok(label)
}

/// 把运行期指标合并进配置快照 JSON
fn with_runtime_metrics(snapshot: &str, metrics: serde_json::Value) -> String {
    let mut value: serde_json::Value = serde_json::from_str(snapshot).unwrap_or_else(|_| json!({}));
    if let Some(map) = value.as_object_mut() {
        map.insert("metrics".to_string(), metrics);
    }
    value.to_string()
}

/// 写入单机峰值专题（与集群 config.cluster.peaks 同结构）
fn with_metrics_peaks(
    snapshot: &str,
    peaks: &crate::services::cluster::protocol::ReportPeaks,
) -> String {
    let mut value: serde_json::Value = serde_json::from_str(snapshot).unwrap_or_else(|_| json!({}));
    if let Some(map) = value.as_object_mut() {
        map.insert(
            "metricsPeaks".to_string(),
            serde_json::to_value(peaks).unwrap_or(serde_json::Value::Null),
        );
    }
    value.to_string()
}

/// 同目标叠加压测留痕：报告里必须能看出这份结果不能当单实例容量结论
fn with_overlap(snapshot: &str, job_ids: &[u64]) -> String {
    let mut value: serde_json::Value = serde_json::from_str(snapshot).unwrap_or_else(|_| json!({}));
    if let Some(map) = value.as_object_mut() {
        map.insert(
            "overlap".to_string(),
            json!({ "ignored": true, "with": job_ids }),
        );
    }
    value.to_string()
}

/// 报告落库入参：单机压测与集群合并共用同一套表结构
pub(crate) struct ReportPayload<'a> {
    pub report: &'a StressReport,
    pub samples: &'a [ReportSample],
    pub errors: &'a [ReportError],
    pub requests: &'a [ReportRequestRecord],
    pub stages: &'a [StageSummary],
    pub status_codes: &'a [StatusCount],
    pub histogram: &'a [HistogramBucket],
    /// 响应体读取耗时直方图（body = latency - ttfb）
    pub body_histogram: &'a [HistogramBucket],
    /// TopN 慢请求
    pub slow_requests: &'a [crate::models::SlowRequest],
    pub error_types: &'a [ErrorTypeCount],
    /// 按 URL 聚合的统计（已脱敏）
    pub url_stats: &'a [UrlStatRow],
    /// 按接口的每秒采样（旧调用方传空切片）
    pub interface_samples: &'a [crate::models::InterfaceSamplePoint],
    pub config: &'a str,
}

/// 批量插入：SQLite 单条语句变量数有上限，按 500 行一组生成多值 VALUES
///
/// 逐条 execute 在事务里虽然能复用语句，但长压测的请求明细可达 5 万行，
/// 单行往返仍会明显拖长报告落库时间；多值插入把往返次数降到百分之一。
fn batch_insert(
    tx: &rusqlite::Transaction<'_>,
    prefix: &str,
    columns: usize,
    rows: &[Vec<rusqlite::types::Value>],
) -> Result<(), String> {
    const BATCH_ROWS: usize = 500;
    let row_sql = format!(
        "({})",
        std::iter::repeat_n("?", columns)
            .collect::<Vec<_>>()
            .join(",")
    );
    for chunk in rows.chunks(BATCH_ROWS) {
        let placeholders = std::iter::repeat_n(row_sql.as_str(), chunk.len())
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!("{prefix} VALUES {placeholders}");
        let mut flat = Vec::with_capacity(chunk.len() * columns);
        for row in chunk {
            flat.extend(row.iter().cloned());
        }
        tx.execute(&sql, rusqlite::params_from_iter(flat.iter()))
            .map_err(err)?;
    }
    Ok(())
}

pub(crate) fn save_report(pool: &DbPool, payload: ReportPayload<'_>) -> Result<i64, String> {
    let report = payload.report;
    let mut conn = pool.get().map_err(err)?;
    let tx = conn.transaction().map_err(err)?;

    let stages_text = serde_json::to_string(payload.stages).map_err(err)?;
    let codes_text = serde_json::to_string(payload.status_codes).map_err(err)?;
    let histogram_text = serde_json::to_string(payload.histogram).map_err(err)?;
    let body_histogram_text = serde_json::to_string(payload.body_histogram).map_err(err)?;
    let slow_requests_text = serde_json::to_string(payload.slow_requests).map_err(err)?;
    let types_text = serde_json::to_string(payload.error_types).map_err(err)?;

    tx.execute(
        "INSERT INTO test_reports
            (plan_id, plan_name, request_id, total_requests, success_count, error_count,
             avg_rt, min_rt, max_rt, p50_rt, p75_rt, p90_rt, p95_rt, p99_rt, p999_rt,
             stddev_rt, ttfb_avg, ttfb_p95, apdex, bytes_total, max_qps, avg_qps, target_rps,
             error_rate, concurrency, status, config_json, stages_json, codes_json,
             histogram_json, error_types_json, start_time, end_time, created_at,
             body_histogram_json, slow_requests_json, run_group_id, is_cluster, peak_qps)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14,
                 ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28,
                 ?29, ?30, ?31, ?32, ?33, ?34, ?35, ?36, ?37, ?38, ?39)",
        params![
            report.plan_id,
            report.plan_name,
            report.request_id,
            report.total_requests,
            report.success_count,
            report.error_count,
            report.avg_rt,
            report.min_rt,
            report.max_rt,
            report.p50_rt,
            report.p75_rt,
            report.p90_rt,
            report.p95_rt,
            report.p99_rt,
            report.p999_rt,
            report.stddev_rt,
            report.ttfb_avg,
            report.ttfb_p95,
            report.apdex,
            report.bytes_total,
            report.max_qps,
            report.avg_qps,
            report.target_rps,
            report.error_rate,
            report.concurrency,
            report.status,
            payload.config,
            stages_text,
            codes_text,
            histogram_text,
            types_text,
            report.start_time,
            report.end_time,
            report.created_at,
            body_histogram_text,
            slow_requests_text,
            report.run_group_id,
            report.is_cluster,
            report.peak_qps,
        ],
    )
    .map_err(err)?;

    let report_id = tx.last_insert_rowid();

    {
        use rusqlite::types::Value as SqlValue;
        let rows: Vec<Vec<SqlValue>> = payload
            .samples
            .iter()
            .map(|sample| {
                vec![
                    SqlValue::Integer(report_id),
                    SqlValue::Integer(sample.elapsed_sec as i64),
                    SqlValue::Integer(sample.concurrent as i64),
                    SqlValue::Real(sample.qps),
                    SqlValue::Real(sample.avg_rt),
                    SqlValue::Real(sample.p50_rt),
                    SqlValue::Real(sample.p95_rt),
                    SqlValue::Real(sample.p99_rt),
                    SqlValue::Real(sample.ttfb_avg),
                    SqlValue::Real(sample.body_avg),
                    SqlValue::Real(sample.body_p95),
                    SqlValue::Integer(sample.bytes as i64),
                    SqlValue::Integer(sample.success_count as i64),
                    SqlValue::Integer(sample.error_count as i64),
                    SqlValue::Integer(sample.response_count as i64),
                ]
            })
            .collect();
        batch_insert(
            &tx,
            "INSERT INTO report_samples
                (report_id, elapsed_sec, concurrent, qps, avg_rt, p50_rt, p95_rt, p99_rt,
                 ttfb_avg, body_avg, body_p95, bytes, success_count, error_count, response_count)",
            15,
            &rows,
        )?;
    }

    {
        use rusqlite::types::Value as SqlValue;
        let rows: Vec<Vec<SqlValue>> = payload
            .errors
            .iter()
            .map(|item| {
                vec![
                    SqlValue::Integer(report_id),
                    SqlValue::Integer(item.elapsed_sec),
                    SqlValue::Text(item.error_type.clone()),
                    SqlValue::Text(item.error_message.clone()),
                    SqlValue::Text(item.request_url.clone()),
                    item.response_status
                        .map(SqlValue::Integer)
                        .unwrap_or(SqlValue::Null),
                    SqlValue::Text(item.response_body.clone()),
                ]
            })
            .collect();
        batch_insert(
            &tx,
            "INSERT INTO report_errors
                (report_id, elapsed_sec, error_type, error_message, request_url,
                 response_status, response_body)",
            7,
            &rows,
        )?;
    }

    // 兼容旧库：逐请求明细表可能尚未创建，缺失时在此补建，避免整份报告保存失败
    let _ = tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS report_requests (
            id            INTEGER PRIMARY KEY AUTOINCREMENT,
            report_id     INTEGER NOT NULL REFERENCES test_reports(id) ON DELETE CASCADE,
            elapsed_ms    INTEGER DEFAULT 0,
            time_ms       INTEGER DEFAULT 0,
            method        TEXT,
            url           TEXT,
            status        INTEGER DEFAULT 0,
            latency_ms    INTEGER DEFAULT 0,
            ttfb_ms       INTEGER DEFAULT 0,
            bytes         INTEGER DEFAULT 0,
            success       INTEGER DEFAULT 1,
            body          TEXT,
            params        TEXT,
            req_headers   TEXT,
            res_headers   TEXT,
            res_body      TEXT,
            error_type    TEXT,
            error_message TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_report_requests_report ON report_requests(report_id, id);",
    );

    let _ = tx.execute_batch("SAVEPOINT req_rows");
    let detail_result = {
        use rusqlite::types::Value as SqlValue;
        let text = |value: &Option<String>| {
            value
                .as_ref()
                .map(|item| SqlValue::Text(item.clone()))
                .unwrap_or(SqlValue::Null)
        };
        let rows: Vec<Vec<SqlValue>> = payload
            .requests
            .iter()
            .map(|item| {
                vec![
                    SqlValue::Integer(report_id),
                    SqlValue::Integer(item.elapsed_ms),
                    SqlValue::Integer(item.time_ms),
                    SqlValue::Text(item.method.clone()),
                    SqlValue::Text(item.url.clone()),
                    SqlValue::Integer(item.status),
                    SqlValue::Integer(item.latency_ms),
                    SqlValue::Integer(item.ttfb_ms),
                    SqlValue::Integer(item.bytes),
                    SqlValue::Integer(item.success as i64),
                    text(&item.body),
                    text(&item.params),
                    text(&item.headers),
                    text(&item.response_headers),
                    text(&item.response_body),
                    text(&item.error_type),
                    text(&item.error_message),
                ]
            })
            .collect();
        batch_insert(
            &tx,
            "INSERT INTO report_requests
                (report_id, elapsed_ms, time_ms, method, url, status, latency_ms, ttfb_ms,
                 bytes, success, body, params, req_headers, res_headers, res_body,
                 error_type, error_message)",
            17,
            &rows,
        )
    };
    // 明细写入失败只回滚这一段，报告主体照常提交
    if detail_result.is_ok() {
        let _ = tx.execute_batch("RELEASE req_rows");
    } else {
        log::warn!("压测请求明细写入失败：{:?}", detail_result.as_ref().err());
        let _ = tx.execute_batch("ROLLBACK TO req_rows; RELEASE req_rows");
    }

    // URL 统计：旧库可能没有这张表，先补建再批量写入；失败只影响该段
    let _ = tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS report_url_stats (
            id             INTEGER PRIMARY KEY AUTOINCREMENT,
            report_id      INTEGER NOT NULL REFERENCES test_reports(id) ON DELETE CASCADE,
            method         TEXT    NOT NULL DEFAULT 'GET',
            url            TEXT    NOT NULL,
            status_code    INTEGER NOT NULL DEFAULT 0,
            requests       INTEGER NOT NULL DEFAULT 0,
            success_count  INTEGER NOT NULL DEFAULT 0,
            error_count    INTEGER NOT NULL DEFAULT 0,
            sum_ms         REAL    NOT NULL DEFAULT 0,
            max_ms         REAL    NOT NULL DEFAULT 0,
            max_at_ms      INTEGER,
            bytes          INTEGER NOT NULL DEFAULT 0,
            error_message  TEXT,
            max_request_id TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_url_stats_report
            ON report_url_stats(report_id, status_code);",
    );
    let _ = tx.execute_batch("SAVEPOINT url_rows");
    let url_result = {
        use rusqlite::types::Value as SqlValue;
        let text = |value: &Option<String>| {
            value
                .as_ref()
                .map(|item| SqlValue::Text(item.clone()))
                .unwrap_or(SqlValue::Null)
        };
        let rows: Vec<Vec<SqlValue>> = payload
            .url_stats
            .iter()
            .map(|item| {
                vec![
                    SqlValue::Integer(report_id),
                    SqlValue::Text(item.method.clone()),
                    SqlValue::Text(item.url.clone()),
                    SqlValue::Integer(item.status_code),
                    SqlValue::Integer(item.requests),
                    SqlValue::Integer(item.success_count),
                    SqlValue::Integer(item.error_count),
                    SqlValue::Real(item.sum_ms),
                    SqlValue::Real(item.max_ms),
                    item.max_at_ms
                        .map(SqlValue::Integer)
                        .unwrap_or(SqlValue::Null),
                    SqlValue::Integer(item.bytes),
                    text(&item.error_message),
                    text(&item.max_request_id),
                ]
            })
            .collect();
        batch_insert(
            &tx,
            "INSERT INTO report_url_stats
                (report_id, method, url, status_code, requests, success_count, error_count,
                 sum_ms, max_ms, max_at_ms, bytes, error_message, max_request_id)",
            13,
            &rows,
        )
    };
    if url_result.is_ok() {
        let _ = tx.execute_batch("RELEASE url_rows");
    } else {
        log::warn!("URL 统计写入失败：{:?}", url_result.as_ref().err());
        let _ = tx.execute_batch("ROLLBACK TO url_rows; RELEASE url_rows");
    }

    // 接口曲线采样：旧库先补建表，失败只影响该段，不拖垮整份报告
    let _ = tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS report_interface_samples (
            id               INTEGER PRIMARY KEY AUTOINCREMENT,
            report_id        INTEGER NOT NULL REFERENCES test_reports(id) ON DELETE CASCADE,
            elapsed_sec      INTEGER NOT NULL,
            interface_index  INTEGER NOT NULL,
            qps              REAL    NOT NULL DEFAULT 0,
            success_count    INTEGER NOT NULL DEFAULT 0,
            error_count      INTEGER NOT NULL DEFAULT 0,
            avg_rt           REAL    NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS idx_interface_samples_report
            ON report_interface_samples(report_id, interface_index, elapsed_sec);",
    );
    let _ = tx.execute_batch("SAVEPOINT iface_rows");
    let iface_result = {
        use rusqlite::types::Value as SqlValue;
        let rows: Vec<Vec<SqlValue>> = payload
            .interface_samples
            .iter()
            .map(|item| {
                vec![
                    SqlValue::Integer(report_id),
                    SqlValue::Integer(item.elapsed_sec as i64),
                    SqlValue::Integer(item.index as i64),
                    SqlValue::Real(item.qps),
                    SqlValue::Integer(item.success_count as i64),
                    SqlValue::Integer(item.error_count as i64),
                    SqlValue::Real(item.avg_rt),
                ]
            })
            .collect();
        batch_insert(
            &tx,
            "INSERT INTO report_interface_samples
                (report_id, elapsed_sec, interface_index, qps, success_count, error_count, avg_rt)",
            7,
            &rows,
        )
    };
    if iface_result.is_ok() {
        let _ = tx.execute_batch("RELEASE iface_rows");
    } else {
        log::warn!("接口曲线写入失败：{:?}", iface_result.as_ref().err());
        let _ = tx.execute_batch("ROLLBACK TO iface_rows; RELEASE iface_rows");
    }

    tx.commit().map_err(err)?;
    Ok(report_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::KeyValueItem;

    fn kv(key: &str, value: &str) -> KeyValueItem {
        KeyValueItem {
            key: key.to_string(),
            value: value.to_string(),
            enabled: true,
        }
    }

    #[test]
    fn normalize_request_weights_handles_blank_and_zero() {
        // 全部未设置：平均分配
        let equal = normalize_request_weights(&[0.0, 0.0]);
        assert_eq!(equal, vec![0.5, 0.5]);
        // 有正权重：未设置按 1 参与，总和为 1
        let mixed = normalize_request_weights(&[3.0, 0.0]);
        assert_eq!(mixed, vec![0.75, 0.25]);
        // 空列表不 panic
        assert!(normalize_request_weights(&[]).is_empty());
    }

    #[test]
    fn effective_requests_falls_back_to_single_definition() {
        let config: StressConfig =
            serde_json::from_str(r#"{"url":"http://127.0.0.1/x","method":"POST"}"#).unwrap();
        let defs = effective_requests(&config);
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].url, "http://127.0.0.1/x");
        assert_eq!(defs[0].method, "POST");
        assert_eq!(defs[0].weight, Some(1.0));
    }

    #[test]
    fn effective_requests_keeps_multi_definition() {
        let config: StressConfig = serde_json::from_str(
            r#"{
                "url": "http://127.0.0.1/a",
                "requests": [
                    {"name": "a", "method": "GET", "url": "http://127.0.0.1/a", "weight": 3},
                    {"name": "b", "method": "POST", "url": "http://127.0.0.1/b"}
                ]
            }"#,
        )
        .unwrap();
        let defs = effective_requests(&config);
        assert_eq!(defs.len(), 2);
        assert_eq!(defs[1].method, "POST");
        assert_eq!(defs[0].weight, Some(3.0));
    }

    #[test]
    fn ramp_curve_modes_shape_delay() {
        let mut config: StressConfig =
            serde_json::from_str(r#"{"url":"http://127.0.0.1/x","rampUpSec":10}"#).unwrap();
        config.ramp_up_mode = "linear".to_string();
        assert_eq!(ramp_delay_ms(&config, 0, 11), 0);
        assert_eq!(ramp_delay_ms(&config, 10, 11), 10_000);
        config.ramp_up_mode = "spike".to_string();
        assert!(ramp_delay_ms(&config, 5, 11) < 5_000);
        config.ramp_up_mode = "step".to_string();
        assert_eq!(ramp_delay_ms(&config, 0, 11), 0);
        assert_eq!(ramp_delay_ms(&config, 10, 11), 10_000);
    }

    #[test]
    fn think_time_distributions_stay_in_bounds() {
        let mut config: StressConfig =
            serde_json::from_str(r#"{"url":"http://127.0.0.1/x","thinkTimeMs":100}"#).unwrap();
        let mut rng = Rng::new(42);
        config.think_time_mode = "uniform".to_string();
        config.think_time_min_ms = 200;
        config.think_time_max_ms = 300;
        for _ in 0..100 {
            let ms = think_delay(&config, &mut rng).unwrap().as_millis() as u64;
            assert!((200..=300).contains(&ms));
        }
        config.think_time_mode = "lognormal".to_string();
        config.think_time_mean_ms = 500;
        config.think_time_std_ms = 100;
        let ms = think_delay(&config, &mut rng).unwrap().as_millis() as u64;
        assert!(ms > 0 && ms < 60_000);
        config.think_time_mode = "fixed".to_string();
        config.think_time_ms = 0;
        assert!(think_delay(&config, &mut rng).is_none());
    }

    #[test]
    fn capture_maps_from_atomic_u8() {
        assert_eq!(Capture::from_u8(0), Capture::Off);
        assert_eq!(Capture::from_u8(1), Capture::Sample);
        assert_eq!(Capture::from_u8(2), Capture::Full);
        // 越界按 full 处理，避免脏值把采集关掉
        assert_eq!(Capture::from_u8(9), Capture::Full);
        assert_eq!(Capture::Off.label(), "off");
    }

    #[test]
    fn record_detail_defaults_to_auto_and_parses_lowercase() {
        let config: StressConfig = serde_json::from_str(r#"{"url":"http://127.0.0.1/x"}"#).unwrap();
        assert_eq!(config.record_detail, RecordDetailMode::Auto);

        let config: StressConfig =
            serde_json::from_str(r#"{"url":"http://127.0.0.1/x","recordDetail":"off"}"#).unwrap();
        assert_eq!(config.record_detail, RecordDetailMode::Off);

        let config: StressConfig =
            serde_json::from_str(r#"{"url":"http://127.0.0.1/x","recordDetail":"sample"}"#)
                .unwrap();
        assert_eq!(config.record_detail, RecordDetailMode::Sample);
    }

    #[test]
    fn estimate_request_bytes_covers_line_headers_and_body() {
        let mut config: StressConfig =
            serde_json::from_str(r#"{"url":"http://127.0.0.1/x","body":"payload"}"#).unwrap();
        config.method = "POST".to_string();
        config.body_type = "text".to_string();
        config.headers = vec![kv("X-Trace", "abc")];

        let without = estimate_request_bytes(
            &StressConfig {
                body: None,
                headers: Vec::new(),
                ..serde_json::from_str(r#"{"url":"http://127.0.0.1/x"}"#).unwrap()
            },
            &reqwest::Method::POST,
        );
        let with = estimate_request_bytes(&config, &reqwest::Method::POST);
        // 多了一个头（key + value + ": \r\n"）与 7 字节 body
        assert_eq!(
            with - without,
            ("X-Trace".len() + "abc".len() + 4 + 7) as u64
        );
    }

    #[test]
    fn runtime_metrics_are_merged_into_snapshot() {
        let snapshot = r#"{"mode":"fixed","recordDetail":"off"}"#;
        let merged =
            with_runtime_metrics(snapshot, json!({"bytesSent": 1024, "recordsDropped": 3}));
        let value: serde_json::Value = serde_json::from_str(&merged).unwrap();
        assert_eq!(value["mode"], "fixed");
        assert_eq!(value["metrics"]["bytesSent"], 1024);
        assert_eq!(value["metrics"]["recordsDropped"], 3);
    }

    #[test]
    fn find_unresolved_collects_unique_names() {
        assert_eq!(
            find_unresolved("{{base_url}}/api/{{id}}?token={{base_url}}"),
            vec!["base_url".to_string(), "id".to_string()]
        );
        assert!(find_unresolved("http://127.0.0.1/api").is_empty());
        // 未闭合的占位符按原文保留，不误判为变量
        assert!(find_unresolved("{{base_url").is_empty());
    }

    #[test]
    fn check_unresolved_blocks_startup_with_env_name() {
        let config: StressConfig = serde_json::from_str(
            r#"{"url":"{{base_url}}/api","headers":[{"key":"X-Token","value":"{{token}}","enabled":true}]}"#,
        )
        .unwrap();
        let error = check_unresolved(&config, &HashMap::new(), &[], Some("测试环境")).unwrap_err();
        assert!(error.contains("测试环境"));
        assert!(error.contains("base_url"));
        assert!(error.contains("token"));

        let mut vars = HashMap::new();
        vars.insert("base_url".to_string(), "http://127.0.0.1".to_string());
        vars.insert("token".to_string(), "abc".to_string());
        assert!(check_unresolved(&config, &vars, &[], Some("测试环境")).is_ok());
        // 池自动注入的变量名进入白名单后不再判缺失
        let provided = vec!["token".to_string(), "base_url".to_string()];
        assert!(check_unresolved(&config, &HashMap::new(), &provided, Some("测试环境")).is_ok());
    }

    /// 逐请求轮换语义：1000 次请求 / 100 条 token ⇒ 每条正好命中 10 次
    #[tokio::test]
    async fn per_request_binding_rotates_evenly_over_requests() {
        let bindings = vec![PoolBinding {
            pool: "tokens".to_string(),
            target: "token".to_string(),
            mode: "per_request".to_string(),
            ..Default::default()
        }];
        let items: Vec<PoolItem> = (0..100)
            .map(|index| PoolItem {
                id: index as i64 + 1,
                value: format!("t{index}"),
                account: None,
                weight: 1.0,
                expires_at: 0,
                data: HashMap::new(),
            })
            .collect();
        let runtime = PoolRuntime::new(
            bindings,
            HashMap::from([("tokens".to_string(), items)]),
            HashMap::new(),
            HashMap::new(),
        );
        let client = reqwest::Client::new();
        let base: HashMap<String, String> = HashMap::new();
        let mut counts: HashMap<String, usize> = HashMap::new();
        for _ in 0..1000 {
            let overlay = runtime
                .overlay(&client, &base, 1)
                .await
                .expect("逐请求绑定应产生覆盖层");
            let value = overlay.vars.get("token").cloned().unwrap_or_default();
            *counts.entry(value).or_insert(0) += 1;
        }
        assert_eq!(counts.len(), 100);
        assert!(
            counts.values().all(|count| *count == 10),
            "逐请求轮换应当均匀覆盖每个条目：{counts:?}"
        );
    }

    /// 跨池注入同名变量会静默覆盖，必须在启动前拦下
    #[test]
    fn pool_bindings_reject_cross_pool_target_conflict() {
        let columns = HashMap::from([
            ("tokens".to_string(), vec!["token".to_string()]),
            ("accounts".to_string(), vec!["account".to_string()]),
        ]);
        let bindings = vec![
            PoolBinding {
                pool: "tokens".to_string(),
                target: "credential".to_string(),
                ..Default::default()
            },
            PoolBinding {
                pool: "accounts".to_string(),
                target: "credential".to_string(),
                ..Default::default()
            },
        ];
        let error = validate_pool_bindings(&bindings, &columns).unwrap_err();
        assert!(
            error.starts_with("POOL_VAR_CONFLICT"),
            "应报同名冲突：{error}"
        );
        assert!(error.contains("credential"), "{error}");
    }

    /// 同一个池的多列注入不同变量是正常用法
    #[test]
    fn pool_bindings_allow_multiple_columns_of_same_pool() {
        let columns = HashMap::from([(
            "tokens".to_string(),
            vec!["token".to_string(), "account".to_string()],
        )]);
        let bindings = vec![
            PoolBinding {
                pool: "tokens".to_string(),
                target: "token".to_string(),
                column: "token".to_string(),
                ..Default::default()
            },
            PoolBinding {
                pool: "tokens".to_string(),
                target: "account".to_string(),
                column: "account".to_string(),
                ..Default::default()
            },
        ];
        assert!(validate_pool_bindings(&bindings, &columns).is_ok());
    }

    /// 401 换条的手位只能是整体平移，不能接管轮换（否则 per_request 退化成 per_vu）
    #[tokio::test]
    async fn unauthorized_shift_keeps_per_request_rotation() {
        let runtime = PoolRuntime::new(
            vec![PoolBinding {
                pool: "tokens".to_string(),
                target: "token".to_string(),
                mode: "per_request".to_string(),
                ..Default::default()
            }],
            HashMap::from([(
                "tokens".to_string(),
                (0..10)
                    .map(|index| PoolItem {
                        id: index as i64 + 1,
                        value: format!("t{index}"),
                        account: None,
                        weight: 1.0,
                        expires_at: 0,
                        data: HashMap::new(),
                    })
                    .collect::<Vec<_>>(),
            )]),
            HashMap::new(),
            HashMap::new(),
        );
        // 模拟一次 401 换条：手位 +1
        runtime.refresh.lock().insert("tokens".to_string(), 1);
        let client = reqwest::Client::new();
        let base: HashMap<String, String> = HashMap::new();
        let mut counts: HashMap<String, usize> = HashMap::new();
        for _ in 0..100 {
            let overlay = runtime
                .overlay(&client, &base, 3)
                .await
                .expect("应有覆盖层");
            let value = overlay.vars.get("token").cloned().unwrap_or_default();
            *counts.entry(value).or_insert(0) += 1;
        }
        assert_eq!(counts.len(), 10, "401 之后仍应轮询到全部条目：{counts:?}");
        assert!(
            counts.values().all(|count| *count == 10),
            "逐请求轮换不应被手位打断：{counts:?}"
        );
    }

    /// random 模式用无锁递增 + splitmix64 打散，高并发下不能集中在少数条目
    #[tokio::test]
    async fn random_mode_spreads_over_items() {
        let runtime = PoolRuntime::new(
            vec![PoolBinding {
                pool: "tokens".to_string(),
                target: "token".to_string(),
                mode: "random".to_string(),
                ..Default::default()
            }],
            HashMap::from([(
                "tokens".to_string(),
                (0..20)
                    .map(|index| PoolItem {
                        id: index as i64 + 1,
                        value: format!("t{index}"),
                        account: None,
                        weight: 1.0,
                        expires_at: 0,
                        data: HashMap::new(),
                    })
                    .collect::<Vec<_>>(),
            )]),
            HashMap::new(),
            HashMap::new(),
        );
        let client = reqwest::Client::new();
        let base: HashMap<String, String> = HashMap::new();
        let mut counts: HashMap<String, usize> = HashMap::new();
        for _ in 0..400 {
            let overlay = runtime
                .overlay(&client, &base, 1)
                .await
                .expect("应有覆盖层");
            let value = overlay.vars.get("token").cloned().unwrap_or_default();
            *counts.entry(value).or_insert(0) += 1;
        }
        assert_eq!(counts.len(), 20, "随机模式应覆盖全部条目：{counts:?}");
    }

    #[test]
    fn target_key_ignores_query_but_keeps_path() {
        let a = reqwest::Url::parse("http://127.0.0.1:8080/api/orders?id=1").unwrap();
        let b = reqwest::Url::parse("http://127.0.0.1:8080/api/orders?id=2").unwrap();
        let c = reqwest::Url::parse("http://127.0.0.1:8080/api/users").unwrap();
        let key_a = target_key_of("get", &a);
        assert_eq!(key_a, target_key_of("GET", &b));
        assert_ne!(key_a, target_key_of("GET", &c));
        assert!(key_a.ends_with("/api/orders"));
    }

    #[test]
    fn capture_plan_modes_match_product_rules() {
        let capacity: StressConfig =
            serde_json::from_str(r#"{"url":"http://127.0.0.1/x","captureMode":"capacity"}"#)
                .unwrap();
        assert_eq!(capture_plan(&capacity), (RecordDetailMode::Auto, 0));

        let locate: StressConfig =
            serde_json::from_str(r#"{"url":"http://127.0.0.1/x","captureMode":"locate"}"#).unwrap();
        assert_eq!(capture_plan(&locate), (RecordDetailMode::Off, 200));

        let mut custom = locate.clone();
        custom.top_n = Some(50);
        assert_eq!(capture_plan(&custom), (RecordDetailMode::Off, 50));
    }

    #[test]
    fn sparse_buckets_keeps_only_non_zero() {
        let buckets = [
            AtomicU64::new(0),
            AtomicU64::new(3),
            AtomicU64::new(0),
            AtomicU64::new(7),
        ];
        assert_eq!(sparse_buckets(&buckets), vec![(1, 3), (3, 7)]);
        assert!(sparse_buckets(&[]).is_empty());
    }

    #[test]
    fn bucket_delta_returns_increments_between_samples() {
        let buckets = [
            AtomicU64::new(0),
            AtomicU64::new(5),
            AtomicU64::new(0),
            AtomicU64::new(9),
        ];
        let mut previous = vec![0u64; buckets.len()];

        // 首次发送：累积桶整体视作增量
        assert_eq!(bucket_delta(&buckets, &mut previous), vec![(1, 5), (3, 9)]);
        assert_eq!(previous, vec![0, 5, 0, 9]);

        // 本秒只新增 2 / 1，旧桶不重复发送
        buckets[2].store(2, Ordering::Relaxed);
        buckets[3].store(10, Ordering::Relaxed);
        assert_eq!(bucket_delta(&buckets, &mut previous), vec![(2, 2), (3, 1)]);
        assert_eq!(previous, vec![0, 5, 2, 10]);

        // 桶没有变化时不产生空增量
        assert!(bucket_delta(&buckets, &mut previous).is_empty());
    }

    #[test]
    fn url_stats_aggregate_without_request_detail() {
        let metrics = Metrics::new();
        metrics.record(
            "GET",
            "https://api.test/a?x=1",
            100,
            10,
            5,
            5,
            200,
            100,
            10,
            true,
            None,
            0,
        );
        metrics.record(
            "GET",
            "https://api.test/a?x=1",
            300,
            30,
            12,
            18,
            200,
            50,
            10,
            true,
            None,
            0,
        );
        metrics.record(
            "POST",
            "https://api.test/b",
            500,
            50,
            20,
            30,
            500,
            20,
            10,
            false,
            Some("HTTP 500"),
            0,
        );

        let (rows, complete) = metrics.url_stats();
        assert!(complete);
        assert_eq!(rows.len(), 2);
        // 请求数多的 URL 排在前
        assert_eq!(rows[0].url, "https://api.test/a?x=1");
        assert_eq!(rows[0].requests, 2);
        assert_eq!(rows[0].success_count, 2);
        assert_eq!(rows[0].max_ms, 30.0);
        assert_eq!(rows[0].max_at_ms, Some(300));
        assert_eq!(rows[0].bytes, 150);
        let failed = rows.iter().find(|row| row.url.ends_with("/b")).unwrap();
        assert_eq!(failed.status_code, 500);
        assert_eq!(failed.error_count, 1);
        assert_eq!(failed.error_message.as_deref(), Some("HTTP 500"));
    }

    #[test]
    fn url_stats_sanitizes_sensitive_query_and_error() {
        assert_eq!(
            sanitize_url("https://a.com/p?token=abcdef&page=1"),
            "https://a.com/p?token=******&page=1"
        );
        assert_eq!(
            sanitize_url("https://a.com/p?access_token=abcdef&x=1#frag"),
            "https://a.com/p?access_token=******&x=1#frag"
        );
        // 参数名大小写不敏感，常见变体也要覆盖
        assert_eq!(
            sanitize_url("https://a.com/p?API_KEY=abcdef&client_secret=xyz&keep=1"),
            "https://a.com/p?API_KEY=******&client_secret=******&keep=1"
        );
        let masked = sanitize_error("Authorization: Bearer abcdef123456");
        assert!(!masked.contains("abcdef123456"));
        assert!(masked.contains("******"));
        assert_eq!(
            sanitize_error(r#"{"token":"abcdef","user":"demo"}"#),
            r#"{"token":"******","user":"demo"}"#
        );
        assert_eq!(
            sanitize_error("password=hunter2; retry=1"),
            "password=******; retry=1"
        );
        assert_eq!(
            sanitize_error("invalid key sk-1234567890"),
            "invalid key ******"
        );
    }

    #[test]
    fn url_stats_overflow_folds_into_other_and_marks_incomplete() {
        let metrics = Metrics::new();
        // 直接把键计数拉到上限，模拟第 2001 个不同 URL
        metrics
            .url_keys
            .store(URL_STATS_MAX_KEYS as u64, Ordering::Relaxed);
        metrics.record(
            "GET",
            "https://a.test/new",
            10,
            1,
            0,
            1,
            200,
            1,
            1,
            true,
            None,
            0,
        );
        let (rows, complete) = metrics.url_stats();
        assert!(!complete);
        assert!(rows.iter().any(|row| row.url == URL_OTHER));
    }

    #[test]
    fn url_stats_truncates_long_url() {
        let metrics = Metrics::new();
        let url = format!("https://a.test/{}", "x".repeat(MAX_URL_LEN + 100));
        metrics.record("GET", &url, 10, 1, 0, 1, 200, 1, 1, true, None, 0);
        let (rows, _) = metrics.url_stats();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].url.chars().count(), MAX_URL_LEN);
    }

    #[test]
    fn metrics_exclude_failed_requests_from_ttfb_and_body() {
        let metrics = Metrics::new();
        // 成功响应：100ms 总耗时，其中 TTFB 60ms、响应体 40ms
        metrics.record(
            "GET",
            "https://a.test/ok",
            0,
            100,
            60,
            40,
            200,
            1024,
            10,
            true,
            None,
            0,
        );
        // 连接失败：没有响应，不能按 0 计入 TTFB，也不能把 50ms 全算成响应体
        metrics.record(
            "GET",
            "https://a.test/fail",
            0,
            50,
            0,
            0,
            0,
            0,
            10,
            false,
            Some("timeout"),
            0,
        );
        assert_eq!(metrics.count(), 2);
        assert_eq!(metrics.responses.load(Ordering::Relaxed), 1);
        assert!((metrics.ttfb_avg() - 60.0).abs() < 1e-9);
        assert_eq!(metrics.sum_body.load(Ordering::Relaxed), 40);
        // 延迟统计仍然包含失败请求的真实耗时
        assert!((metrics.avg() - 75.0).abs() < 1e-9);
        assert_eq!(metrics.status_codes().len(), 1);
    }

    #[test]
    fn percentile_interpolates_inside_bucket() {
        let items = vec![(10u32, 10u64), (20u32, 10u64)];
        // 前 10 个落在 10ms 桶：P50 位于桶内，应返回 10.x 而不是 10.0 / 11.0
        let p50 = percentile_sparse(&items, 20, 0.5);
        assert!((10.0..11.0).contains(&p50), "p50={p50}");
        let p95 = percentile_sparse(&items, 20, 0.95);
        assert!((20.0..21.0).contains(&p95), "p95={p95}");
    }

    #[test]
    fn stage_summary_uses_exact_accumulator() {
        let stages = vec![
            StressStage {
                concurrent: 10,
                duration_sec: 10,
            },
            StressStage {
                concurrent: 20,
                duration_sec: 10,
            },
        ];
        let mut accums = vec![StageAccum::default(), StageAccum::default()];
        accums[0].requests = 100;
        accums[0].errors = 2;
        accums[0].sum_rt = 2_000;
        accums[0].latency.insert(20, 90);
        accums[0].latency.insert(40, 10);
        let summaries = summarize_stages(&stages, &[], &accums);
        assert_eq!(summaries.len(), 2);
        // 100 个请求 / 10 秒 = 10 QPS，而不是按采样点个数算
        assert!((summaries[0].avg_qps - 10.0).abs() < 1e-9);
        assert!((summaries[0].avg_rt - 20.0).abs() < 1e-9);
        assert_eq!(summaries[0].requests, 100);
        assert_eq!(summaries[0].errors, 2);
        // 95 分位落在 40ms 桶内（90 < 95 ≤ 100）
        assert!((40.0..41.0).contains(&summaries[0].p95_rt));
        assert_eq!(summaries[1].requests, 0);
        assert_eq!(summaries[1].avg_qps, 0.0);
    }
}

/// 12.5 无池 5,000 QPS 回归基线
///
/// 固定工作负载：本机 loopback HTTP 服务 + `send_once` 无池快路径（GET、2 个参数、8 个头、
/// 模板变量已解析），64 个并发任务连续发送，预热 1s 后取 3 个 1s 窗口的中位数。
/// 基线文件记录机器指纹与 QPS；同指纹下低于基线 95% 判定回归。
/// 重新记录：`ASTROMAN_PERF_BASELINE=update cargo test --lib perf_no_pool_5000_qps -- --ignored`
#[cfg(test)]
mod perf_tests {
    use super::*;
    use serde::{Deserialize, Serialize};
    use std::io::{Read, Write};
    use std::net::{SocketAddr, TcpListener, TcpStream};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::Arc;
    use std::thread;
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    const WORKERS: usize = 64;
    const WARMUP: Duration = Duration::from_secs(1);
    const WINDOW: Duration = Duration::from_secs(1);
    const WINDOWS: usize = 3;
    const TARGET_QPS: f64 = 5_000.0;

    #[derive(Debug, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Baseline {
        version: u32,
        fingerprint: String,
        median_qps: f64,
        recorded_at_unix: u64,
        workload: String,
    }

    fn baseline_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("perf")
            .join("no_pool_5000_baseline.json")
    }

    /// 机器指纹：OS / 架构 / 核心数 / debug 或 release
    fn fingerprint() -> String {
        format!(
            "{}-{}-{}-{}",
            std::env::consts::OS,
            std::env::consts::ARCH,
            std::thread::available_parallelism()
                .map(|value| value.get())
                .unwrap_or(0),
            if cfg!(debug_assertions) {
                "debug"
            } else {
                "release"
            }
        )
    }

    fn spawn_loopback_server() -> (SocketAddr, Arc<AtomicBool>) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("绑定本地压测目标失败");
        listener.set_nonblocking(true).expect("设置非阻塞失败");
        let addr = listener.local_addr().expect("读取本地端口失败");
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        thread::spawn(move || {
            while !flag.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        thread::spawn(move || serve_keep_alive(stream));
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2));
                    }
                    Err(_) => break,
                }
            }
        });
        (addr, stop)
    }

    /// 极简 keep-alive HTTP/1.1：读完整请求头（GET 无体），固定返回 200
    fn serve_keep_alive(mut stream: TcpStream) {
        stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
        let mut buffer: Vec<u8> = Vec::with_capacity(2048);
        let mut chunk = [0u8; 4096];
        loop {
            let header_end = loop {
                if let Some(pos) = buffer.windows(4).position(|item| item == b"\r\n\r\n") {
                    break pos + 4;
                }
                match stream.read(&mut chunk) {
                    Ok(0) => return,
                    Ok(read) => buffer.extend_from_slice(&chunk[..read]),
                    Err(_) => return,
                }
            };
            let head = String::from_utf8_lossy(&buffer[..header_end]).to_ascii_lowercase();
            let body_len = head
                .lines()
                .find_map(|line| line.strip_prefix("content-length:"))
                .and_then(|value| value.trim().parse::<usize>().ok())
                .unwrap_or(0);
            while buffer.len() < header_end + body_len {
                match stream.read(&mut chunk) {
                    Ok(0) => return,
                    Ok(read) => buffer.extend_from_slice(&chunk[..read]),
                    Err(_) => return,
                }
            }
            buffer.drain(..header_end + body_len);
            if stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 2\r\nConnection: keep-alive\r\n\r\nok",
                )
                .is_err()
            {
                return;
            }
        }
    }

    fn perf_config(addr: SocketAddr) -> StressConfig {
        serde_json::from_value(serde_json::json!({
            "method": "GET",
            "url": format!("http://{addr}/api/v1/{{{{tenant}}}}/items"),
            "params": [
                { "key": "page", "value": "{{page}}", "enabled": true },
                { "key": "size", "value": "{{size}}", "enabled": true }
            ],
            "headers": [
                { "key": "Accept", "value": "application/json", "enabled": true },
                { "key": "X-Tenant", "value": "{{tenant}}", "enabled": true },
                { "key": "X-Trace", "value": "{{trace}}", "enabled": true },
                { "key": "X-Client", "value": "astroman-perf", "enabled": true },
                { "key": "X-Scenario", "value": "no-pool-5000", "enabled": true },
                { "key": "Cache-Control", "value": "no-cache", "enabled": true },
                { "key": "Pragma", "value": "no-cache", "enabled": true },
                { "key": "User-Agent", "value": "AstroMan/perf", "enabled": true }
            ],
            "bodyType": "none",
            "timeoutMs": 5000,
            "concurrent": 64,
            "durationSec": 10
        }))
        .expect("构造压测配置失败")
    }

    #[test]
    #[ignore = "性能基准：URL 聚合单请求开销，手动执行"]
    fn perf_url_aggregation_overhead() {
        const ITERATIONS: usize = 1_000_000;
        let url = "https://api.test/v1/orders?page=1&size=20&tenant=acme";
        let off = Metrics::new_with_url_stats(false);
        let on = Metrics::new_with_url_stats(true);
        let measure = |metrics: &Metrics| {
            let started = Instant::now();
            for index in 0..ITERATIONS {
                metrics.record(
                    "GET",
                    url,
                    index as i64,
                    20,
                    10,
                    10,
                    200,
                    1024,
                    500,
                    true,
                    None,
                    0,
                );
            }
            started.elapsed().as_nanos() as f64 / ITERATIONS as f64
        };
        // 预热，避免首次分配与正则初始化计入结果
        let _ = measure(&off);
        let _ = measure(&on);
        let off_ns = measure(&off).min(measure(&off));
        let on_ns = measure(&on).min(measure(&on));
        let added = (on_ns - off_ns).max(0.0);
        println!(
            "perf_url_aggregation_overhead: off={off_ns:.1}ns on={on_ns:.1}ns added={added:.1}ns/req, 10k QPS 估算 CPU 占比 {:.3}%",
            added * 10_000.0 / 1_000_000_000.0 * 100.0
        );
        assert!(
            added < 2_000.0,
            "每次请求 URL 聚合新增 {added:.0}ns，超过 2µs 预算"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 8)]
    #[ignore = "性能基线：无池 5,000 QPS 回归，手动执行"]
    async fn perf_no_pool_5000_qps() {
        let (addr, server_stop) = spawn_loopback_server();
        let mut vars = HashMap::new();
        vars.insert("tenant".to_string(), "acme".to_string());
        vars.insert("page".to_string(), "1".to_string());
        vars.insert("size".to_string(), "20".to_string());
        vars.insert("trace".to_string(), "trace-1".to_string());

        let client = reqwest::Client::builder()
            .pool_max_idle_per_host(WORKERS)
            .build()
            .expect("构建压测客户端失败");
        let (errors_tx, _errors_rx) = mpsc::channel::<ReportError>(1024);
        let (records_tx, _records_rx) = mpsc::unbounded_channel::<ReportRequestRecord>();
        let (slow_tx, _slow_rx) = mpsc::channel::<SlowRequest>(4096);
        let start = Instant::now();
        let config = perf_config(addr);
        let requests = vec![PreparedRequest {
            request_id: config.request_id,
            name: config.name.clone().unwrap_or_default(),
            method: reqwest::Method::GET,
            url_template: config.url.clone(),
            url_text: config.url.clone(),
            headers: config.headers.clone(),
            params: config.params.clone(),
            body_type: config.body_type.clone(),
            body: config.body.clone(),
            raw_auth: config.auth.clone(),
            auth: RequestAuth::default(),
            cookie: None,
            req_bytes: 0,
            weight: 1.0,
        }];
        let ctx = RunCtx {
            client,
            config,
            vars,
            pools: None,
            requests,
            total_weight: 1.0,
            metrics: Arc::new(Metrics::new()),
            control: Arc::new(Control::new(WORKERS as u32, 0)),
            start,
            deadline: start + Duration::from_secs(30),
            cancel: Arc::new(AtomicBool::new(false)),
            errors: errors_tx,
            records: records_tx,
            slow_tx,
            slow_limit: 0,
            slow_threshold_ms: 0,
            capture: Arc::new(AtomicU8::new(Capture::Off as u8)),
        };
        let ctx = Arc::new(ctx);
        let stop = Arc::new(AtomicBool::new(false));
        let success = Arc::new(AtomicU64::new(0));

        let mut workers = Vec::with_capacity(WORKERS);
        for _ in 0..WORKERS {
            let ctx = ctx.clone();
            let stop = stop.clone();
            let success = success.clone();
            workers.push(tokio::spawn(async move {
                while !stop.load(Ordering::Relaxed) {
                    let attempt = send_once(&ctx, &ctx.requests[0], Capture::Off, 0).await;
                    if attempt.ok {
                        success.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }));
        }

        tokio::time::sleep(WARMUP).await;
        let mut windows = Vec::with_capacity(WINDOWS);
        for _ in 0..WINDOWS {
            let before = success.load(Ordering::Relaxed);
            tokio::time::sleep(WINDOW).await;
            windows.push(success.load(Ordering::Relaxed) - before);
        }
        stop.store(true, Ordering::Relaxed);
        for worker in workers {
            let _ = worker.await;
        }
        server_stop.store(true, Ordering::Relaxed);

        windows.sort_unstable();
        let median_qps = windows[WINDOWS / 2] as f64;
        println!(
            "无池 5000 QPS 基线：指纹 {}，窗口 {:?}，中位数 {:.0} QPS",
            fingerprint(),
            windows,
            median_qps
        );

        let path = baseline_path();
        if std::env::var("ASTROMAN_PERF_BASELINE").ok().as_deref() == Some("update") {
            let baseline = Baseline {
                version: 1,
                fingerprint: fingerprint(),
                median_qps,
                recorded_at_unix: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|value| value.as_secs())
                    .unwrap_or(0),
                workload: format!(
                    "GET /api/v1/{{{{tenant}}}}/items + 2 params + 8 headers, {WORKERS} workers, {WINDOWS}x{}s windows",
                    WINDOW.as_secs()
                ),
            };
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).expect("创建基线目录失败");
            }
            std::fs::write(
                &path,
                serde_json::to_string_pretty(&baseline).expect("序列化基线失败"),
            )
            .expect("写入基线失败");
            println!("已更新基线：{}", path.display());
            return;
        }

        assert!(
            median_qps >= TARGET_QPS,
            "未达到 5,000 QPS 目标：{median_qps:.0}"
        );
        match std::fs::read_to_string(&path) {
            Ok(text) => {
                let baseline: Baseline = serde_json::from_str(&text).expect("基线文件格式错误");
                if baseline.fingerprint == fingerprint() {
                    let floor = baseline.median_qps * 0.95;
                    assert!(
                        median_qps >= floor,
                        "无池 5000 QPS 回归超过 5%：当前 {median_qps:.0}，基线 {:.0}，下限 {floor:.0}",
                        baseline.median_qps
                    );
                    println!("基线对比通过：{median_qps:.0} >= {floor:.0}");
                } else {
                    println!(
                        "基线指纹不一致（当前 {} / 基线 {}），跳过 5% 门禁",
                        fingerprint(),
                        baseline.fingerprint
                    );
                }
            }
            Err(_) => println!(
                "未找到基线文件 {}，仅校验 5,000 QPS 下限；用 ASTROMAN_PERF_BASELINE=update 生成",
                path.display()
            ),
        }
    }
}
