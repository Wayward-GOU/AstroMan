//! 集群采样合并与报告构造（W4）
//!
//! 合并规则（第八章 8.3）：
//!
//! - **QPS 相加**、并发相加；均值按请求数加权（累加耗时 / 总请求数）；
//! - **百分位不取平均**：把各节点逐毫秒直方图桶合并后重算；
//! - 错误率按总请求数合并，状态码 / 错误类型逐项相加；
//! - 失联节点没有 `done`，只用它失联前上报的每秒采样统计到失联时刻；
//! - 时钟误差超过 200ms 的节点，其每秒采样丢弃并计入 `droppedSamples`，
//!   最终总量仍按 `done` 计入（计数不受时间轴影响）。
//!
//! 合并结果写入既有 `test_reports`，集群元数据放 `config_json.cluster`，
//! 报告分析、导出、HTML/Excel 全部复用，不新增第二套报告结构。

use std::collections::BTreeMap;

use serde_json::json;

use super::protocol::{
    ClusterReportMeta, ClusterReportNode, DonePayload, NodeSeries, PeakMetric, ReportPeaks,
    SampleHealth, SamplePayload, SeriesPoint, CLOCK_DROP_UNCERTAINTY_MS, PROTOCOL_VERSION,
};
use crate::commands::stress::EngineOutcome;
use crate::models::{ErrorTypeCount, HistogramBucket, ReportSample, StatusCount, StressReport};

/// APDEX 的 T 值（毫秒），与单机引擎保持一致
const APDEX_T: f64 = 500.0;

/// 合并输入的一个节点：主节点与从节点同构
pub(crate) struct MergeNode {
    pub name: String,
    pub role: String,
    pub addr: String,
    pub weight: f64,
    pub concurrent: u32,
    pub status: String,
    pub clock_offset_ms: f64,
    pub clock_uncertainty_ms: f64,
    /// 每秒采样：丢失节点只有这部分数据
    pub samples: Vec<SamplePayload>,
    /// 正常结束 / 被停止的节点才有
    pub done: Option<DonePayload>,
}

/// 合并入参
pub(crate) struct MergeRequest {
    pub plan_id: Option<i64>,
    pub plan_name: String,
    pub request_id: Option<i64>,
    pub run_group_id: Option<i64>,
    pub total_concurrent: u32,
    /// 任务级时间兜底：所有节点都没有 done 时使用
    pub started_at: i64,
    pub finished_at: i64,
    pub incomplete: bool,
    pub note: Option<String>,
    /// 主节点本地引擎的配置快照；合并后把集群元数据插进 `cluster` 字段
    pub config_snapshot: String,
    pub nodes: Vec<MergeNode>,
}

/// 合并结果：字段与单机引擎输出一一对应，直接交给 `save_report`
pub(crate) struct MergedReport {
    pub report: StressReport,
    pub samples: Vec<ReportSample>,
    pub status_codes: Vec<StatusCount>,
    pub histogram: Vec<HistogramBucket>,
    pub body_histogram: Vec<HistogramBucket>,
    pub error_types: Vec<ErrorTypeCount>,
    pub meta: ClusterReportMeta,
    pub config_snapshot: String,
}

/// 每秒累加器：曲线按秒合并，QPS 相加、耗时累加
#[derive(Default)]
struct SecondAccumulator {
    qps: f64,
    concurrent: u32,
    success: u64,
    errors: u64,
    bytes: u64,
    sum_rt: u64,
    sum_ttfb: u64,
    body_weighted: f64,
    latency: BTreeMap<u32, u64>,
}

impl SecondAccumulator {
    fn absorb(&mut self, sample: &SamplePayload) {
        let count = sample.success_count + sample.error_count;
        self.qps += sample.qps;
        self.concurrent = self.concurrent.saturating_add(sample.concurrent);
        self.success += sample.success_count;
        self.errors += sample.error_count;
        self.bytes += sample.bytes;
        self.sum_rt += sample.sum_rt_ms;
        self.sum_ttfb += sample.sum_ttfb_ms;
        self.body_weighted += sample.body_avg * count as f64;
        for (ms, count) in &sample.latency_buckets {
            *self.latency.entry(*ms).or_insert(0) += *count;
        }
    }

    fn count(&self) -> u64 {
        self.success + self.errors
    }
}

pub(crate) fn merge(request: MergeRequest) -> MergedReport {
    let mut seconds: BTreeMap<u64, SecondAccumulator> = BTreeMap::new();
    let mut latency: BTreeMap<u32, u64> = BTreeMap::new();
    let mut ttfb: BTreeMap<u32, u64> = BTreeMap::new();
    let mut body: BTreeMap<u32, u64> = BTreeMap::new();
    let mut codes: BTreeMap<u16, u64> = BTreeMap::new();
    let mut error_kinds: BTreeMap<String, i64> = BTreeMap::new();

    let mut total = 0u64;
    let mut success = 0u64;
    let mut errors = 0u64;
    let mut bytes = 0u64;
    let mut bytes_sent = 0u64;
    let mut errors_dropped = 0u64;
    let mut records_dropped = 0u64;
    let mut sum_rt = 0u64;
    let mut sum_ttfb = 0u64;
    let mut cpu_peak = 0.0f64;
    let mut mem_peak = 0u64;
    let mut stopped = false;
    let mut dropped_samples = 0u64;
    let mut started: Option<i64> = None;
    let mut ended: Option<i64> = None;
    let mut node_meta: Vec<ClusterReportNode> = Vec::with_capacity(request.nodes.len());
    let mut node_requests: Vec<(String, u64)> = Vec::with_capacity(request.nodes.len());

    for node in &request.nodes {
        // 时钟误差过大时，每秒采样落在错误的时间轴上，只能丢弃曲线数据
        let trusted = node.clock_uncertainty_ms <= CLOCK_DROP_UNCERTAINTY_MS;
        if trusted {
            for sample in &node.samples {
                seconds
                    .entry(sample.elapsed_sec)
                    .or_default()
                    .absorb(sample);
            }
        } else {
            dropped_samples += node.samples.len() as u64;
        }

        let node_start = node.done.as_ref().map(|done| done.start_ms).or_else(|| {
            node.samples
                .first()
                .map(|sample| request.started_at + sample.elapsed_sec as i64 * 1000)
        });
        let node_end = node.done.as_ref().map(|done| done.end_ms).or_else(|| {
            node.samples
                .last()
                .map(|sample| request.started_at + sample.elapsed_sec as i64 * 1000)
        });

        if let Some(done) = &node.done {
            total += done.total;
            success += done.success;
            errors += done.errors;
            bytes += done.bytes;
            bytes_sent += done.bytes_sent;
            errors_dropped += done.errors_dropped;
            records_dropped += done.records_dropped;
            sum_rt += done.sum_rt_ms;
            sum_ttfb += done.sum_ttfb_ms;
            for (ms, count) in &done.latency_buckets {
                *latency.entry(*ms).or_insert(0) += *count;
            }
            for (ms, count) in &done.ttfb_buckets {
                *ttfb.entry(*ms).or_insert(0) += *count;
            }
            for (ms, count) in &done.body_buckets {
                *body.entry(*ms).or_insert(0) += *count;
            }
            for (code, count) in &done.status_codes {
                *codes.entry(*code).or_insert(0) += *count;
            }
            for (kind, count) in &done.error_types {
                *error_kinds.entry(kind.clone()).or_insert(0) += *count;
            }
            cpu_peak = cpu_peak.max(done.cpu_peak);
            mem_peak = mem_peak.max(done.mem_peak_mb);
            stopped |= done.stopped;
            dropped_samples += done.dropped_samples;
            started = Some(started.map_or(done.start_ms, |value| value.min(done.start_ms)));
            ended = Some(ended.map_or(done.end_ms, |value| value.max(done.end_ms)));
        } else if trusted {
            // 失联节点：只统计到最后一个上报的采样
            let mut node_total = 0u64;
            for sample in &node.samples {
                node_total += sample.sum_rt_ms;
                total += sample.success_count + sample.error_count;
                success += sample.success_count;
                errors += sample.error_count;
                bytes += sample.bytes;
                sum_ttfb += sample.sum_ttfb_ms;
                for (ms, count) in &sample.latency_buckets {
                    *latency.entry(*ms).or_insert(0) += *count;
                }
            }
            sum_rt += node_total;
        }

        node_meta.push(ClusterReportNode {
            name: node.name.clone(),
            role: node.role.clone(),
            addr: node.addr.clone(),
            weight: node.weight,
            concurrent: node.concurrent,
            status: node.status.clone(),
            qps_avg: node_qps(node),
            cpu_peak: node.done.as_ref().map(|done| done.cpu_peak).unwrap_or(0.0),
            mem_peak_mb: node.done.as_ref().map(|done| done.mem_peak_mb).unwrap_or(0),
            clock_offset_ms: node.clock_offset_ms,
            clock_uncertainty_ms: node.clock_uncertainty_ms,
            dropped_samples: if trusted {
                node.done
                    .as_ref()
                    .map(|done| done.dropped_samples)
                    .unwrap_or(0)
            } else {
                node.samples.len() as u64
                    + node
                        .done
                        .as_ref()
                        .map(|done| done.dropped_samples)
                        .unwrap_or(0)
            },
            start_ms: node_start.unwrap_or(request.started_at),
            end_ms: node_end.unwrap_or(request.finished_at),
            peak_qps: node.samples.iter().map(|sample| sample.qps).fold(0.0, f64::max),
            peak_concurrent: node
                .samples
                .iter()
                .map(|sample| sample.concurrent)
                .max()
                .unwrap_or(node.concurrent),
            share: 0.0,
            health: node_health(node, node_start, node_end),
        });
        node_requests.push((
            node.name.clone(),
            node.samples
                .iter()
                .map(|sample| sample.success_count + sample.error_count)
                .sum::<u64>(),
        ));
    }

    // 贡献占比：基于各节点累计请求数
    for node in &mut node_meta {
        let requests = node_requests
            .iter()
            .find(|(name, _)| name == &node.name)
            .map(|(_, count)| *count)
            .unwrap_or(0);
        node.share = if total > 0 {
            requests as f64 / total as f64
        } else {
            0.0
        };
    }

    let samples = seconds
        .into_iter()
        .map(|(elapsed_sec, second)| {
            let count = second.count();
            ReportSample {
                elapsed_sec,
                concurrent: second.concurrent,
                qps: second.qps,
                avg_rt: ratio(second.sum_rt, count),
                p50_rt: percentile(&second.latency, count, 0.5),
                p95_rt: percentile(&second.latency, count, 0.95),
                p99_rt: percentile(&second.latency, count, 0.99),
                ttfb_avg: ratio(second.sum_ttfb, count),
                body_avg: if count > 0 {
                    second.body_weighted / count as f64
                } else {
                    0.0
                },
                // 从节点每秒只上报总耗时桶：逐秒 body P95 无数据，用 0 占位
                body_p95: 0.0,
                bytes: second.bytes,
                success_count: second.success,
                error_count: second.errors,
            }
        })
        .collect::<Vec<_>>();

    let start_ms = started.unwrap_or(request.started_at);
    let end_ms = ended.unwrap_or(request.finished_at).max(start_ms);
    let elapsed_sec = ((end_ms - start_ms) as f64 / 1000.0).max(0.001);
    let max_qps = samples.iter().map(|sample| sample.qps).fold(0.0, f64::max);
    let avg_rt = ratio(sum_rt, total);
    let ttfb_avg = ratio(sum_ttfb, total);
    let min_rt = latency
        .keys()
        .next()
        .map(|value| *value as f64)
        .unwrap_or(0.0);
    let max_rt = latency
        .keys()
        .next_back()
        .map(|value| *value as f64)
        .unwrap_or(0.0);

    let peaks = build_peaks(&samples, start_ms);
    let report = StressReport {
        id: 0,
        plan_id: request.plan_id,
        run_group_id: request.run_group_id,
        plan_name: request.plan_name.clone(),
        request_id: request.request_id,
        total_requests: total as i64,
        success_count: success as i64,
        error_count: errors as i64,
        avg_rt,
        min_rt,
        max_rt,
        p50_rt: percentile(&latency, total, 0.5),
        p75_rt: percentile(&latency, total, 0.75),
        p90_rt: percentile(&latency, total, 0.9),
        p95_rt: percentile(&latency, total, 0.95),
        p99_rt: percentile(&latency, total, 0.99),
        p999_rt: percentile(&latency, total, 0.999),
        stddev_rt: stddev(&latency, total, avg_rt),
        ttfb_avg,
        ttfb_p95: percentile(&ttfb, total, 0.95),
        apdex: apdex(&latency, total),
        bytes_total: bytes as i64,
        max_qps,
        avg_qps: total as f64 / elapsed_sec,
        target_rps: 0.0,
        error_rate: if total > 0 {
            errors as f64 / total as f64 * 100.0
        } else {
            0.0
        },
        concurrency: request.total_concurrent as i64,
        status: if stopped { "stopped" } else { "completed" }.to_string(),
        is_cluster: true,
        peak_qps: peaks.qps.value,
        start_time: Some(start_ms),
        end_time: Some(end_ms),
        created_at: end_ms,
    };

    let status_codes = sorted_codes(&codes);
    let error_types = sorted_error_types(&error_kinds);
    let latency_sparse: Vec<(u32, u64)> = latency.into_iter().collect();
    let body_sparse: Vec<(u32, u64)> = body.into_iter().collect();
    let histogram = crate::commands::stress::histogram_from_sparse(&latency_sparse);
    let body_histogram = crate::commands::stress::histogram_from_sparse(&body_sparse);

    let master_node = request
        .nodes
        .iter()
        .find(|node| node.role == "master")
        .map(|node| node.name.clone())
        .unwrap_or_else(|| "master".to_string());
    let node_series = build_node_series(&request.nodes);
    let meta = ClusterReportMeta {
        protocol_version: PROTOCOL_VERSION,
        master_node,
        total_concurrent: request.total_concurrent,
        incomplete: request.incomplete,
        started_at: start_ms,
        finished_at: end_ms,
        dropped_samples,
        nodes: node_meta,
        peaks: Some(peaks),
        node_series,
        note: request.note.clone(),
    };

    let mut config: serde_json::Value =
        serde_json::from_str(&request.config_snapshot).unwrap_or_else(|_| json!({}));
    if let Some(map) = config.as_object_mut() {
        map.insert("mode".to_string(), json!("cluster"));
        // 池统计：合并所有节点回传的计数；旧节点没有回传时保留主节点快照并标注范围
        let pools = merged_pools(&request.nodes);
        if !pools.is_empty() {
            map.insert("pools".to_string(), json!(pools));
        } else if let Some(pools) = map.get_mut("pools").and_then(|value| value.as_array_mut()) {
            for pool in pools {
                if let Some(pool) = pool.as_object_mut() {
                    pool.insert("nodeScope".to_string(), json!("master"));
                }
            }
        }
        map.insert(
            "cluster".to_string(),
            serde_json::to_value(&meta).unwrap_or_else(|_| json!({})),
        );
        // 运行期指标沿用单机位置：导出与报告详情不用改读取路径
        let metrics = map.entry("metrics").or_insert_with(|| json!({}));
        if let Some(metrics) = metrics.as_object_mut() {
            metrics.insert("bytesSent".to_string(), json!(bytes_sent));
            metrics.insert("errorsDropped".to_string(), json!(errors_dropped));
            metrics.insert("recordsDropped".to_string(), json!(records_dropped));
            metrics.insert("clusterNodes".to_string(), json!(meta.nodes.len()));
        }
    }

    MergedReport {
        report,
        samples,
        status_codes,
        histogram,
        body_histogram,
        error_types,
        meta,
        config_snapshot: config.to_string(),
    }
}

/// 合并各节点回传的池统计：同池按键累加，条目按下标累加，并记录贡献节点
///
/// `items` 取各节点最大值而不是求和：`per_vu` 模式下每个节点都持有完整池，
/// 求和会把池容量放大 N 倍。`used / requests / refreshed` 是各节点实际用量，求和。
fn merged_pools(nodes: &[MergeNode]) -> Vec<serde_json::Value> {
    #[derive(Default)]
    struct Agg {
        items: i64,
        used: i64,
        requests: i64,
        refreshed: i64,
        unauthorized: i64,
        invalid: i64,
        cooling: i64,
        exhausted: bool,
        per_item: BTreeMap<i64, i64>,
        nodes: Vec<String>,
        by_node: BTreeMap<String, [i64; 3]>,
    }

    let mut merged: BTreeMap<String, Agg> = BTreeMap::new();
    for node in nodes {
        let Some(done) = &node.done else {
            continue;
        };
        for pool in &done.pools {
            let Some(name) = pool.get("name").and_then(|value| value.as_str()) else {
                continue;
            };
            let name = name.trim();
            if name.is_empty() {
                continue;
            }
            let agg = merged.entry(name.to_string()).or_default();
            if !agg.nodes.iter().any(|item| item == &node.name) {
                agg.nodes.push(node.name.clone());
            }
            let count = |key: &str| pool.get(key).and_then(|value| value.as_i64()).unwrap_or(0);
            agg.items = agg.items.max(count("items"));
            agg.used += count("used");
            agg.requests += count("requests");
            agg.refreshed += count("refreshed");
            agg.unauthorized += count("unauthorized");
            agg.invalid += count("invalid");
            agg.cooling += count("cooling");
            let node_stats = agg.by_node.entry(node.name.clone()).or_insert([0; 3]);
            node_stats[0] += count("used");
            node_stats[1] += count("requests");
            node_stats[2] += count("refreshed");
            if pool
                .get("exhausted")
                .and_then(|value| value.as_bool())
                .unwrap_or(false)
            {
                agg.exhausted = true;
            }
            if let Some(items) = pool.get("perItem").and_then(|value| value.as_array()) {
                for item in items {
                    let Some(index) = item.get("index").and_then(|value| value.as_i64()) else {
                        continue;
                    };
                    let requests = item
                        .get("requests")
                        .and_then(|value| value.as_i64())
                        .unwrap_or(0);
                    *agg.per_item.entry(index).or_insert(0) += requests;
                }
            }
        }
    }

    merged
        .into_iter()
        .map(|(name, agg)| {
            let per_item: Vec<serde_json::Value> = agg
                .per_item
                .into_iter()
                .map(|(index, requests)| json!({ "index": index, "requests": requests }))
                .collect();
            let by_node: Vec<serde_json::Value> = agg
                .by_node
                .into_iter()
                .map(|(name, stats)| {
                    json!({
                        "name": name,
                        "used": stats[0],
                        "requests": stats[1],
                        "refreshed": stats[2],
                    })
                })
                .collect();
            json!({
                "name": name,
                "items": agg.items,
                "used": agg.used,
                "requests": agg.requests,
                "refreshed": agg.refreshed,
                "unauthorized": agg.unauthorized,
                "invalid": agg.invalid,
                "cooling": agg.cooling,
                "exhausted": agg.exhausted,
                "perItem": per_item,
                "nodeScope": "cluster",
                "nodes": agg.nodes,
                "byNode": by_node,
            })
        })
        .collect()
}

/// 主节点本地引擎结果转 `DonePayload`：时间按本地时钟（偏移 0）直接使用
pub(crate) fn done_from_outcome(outcome: &EngineOutcome) -> DonePayload {
    let report = &outcome.report;
    let end_ms = report.end_time.unwrap_or(report.created_at);
    let start_ms = report.start_time.unwrap_or(end_ms);
    let snapshot = serde_json::from_str::<serde_json::Value>(&outcome.config_snapshot)
        .unwrap_or_else(|_| json!({}));
    let runtime = snapshot.get("metrics").cloned().unwrap_or_else(|| json!({}));
    DonePayload {
        start_ms,
        end_ms,
        total: outcome.sparse.total,
        success: outcome.sparse.success,
        errors: outcome.sparse.errors,
        bytes: outcome.sparse.bytes,
        bytes_sent: outcome.sparse.bytes_sent,
        sum_rt_ms: outcome.sparse.sum_rt_ms,
        sum_ttfb_ms: outcome.sparse.sum_ttfb_ms,
        min_rt: outcome.sparse.min_rt,
        max_rt: outcome.sparse.max_rt,
        latency_buckets: outcome.sparse.latency.clone(),
        ttfb_buckets: outcome.sparse.ttfb.clone(),
        body_buckets: outcome.sparse.body.clone(),
        status_codes: outcome.sparse.status_codes.clone(),
        error_types: outcome.sparse.error_types.clone(),
        cpu_peak: runtime
            .get("cpuPeakCores")
            .and_then(|value| value.as_f64())
            .unwrap_or(0.0),
        mem_peak_mb: runtime
            .get("memPeakMb")
            .and_then(|value| value.as_u64())
            .unwrap_or(0),
        stopped: report.status == "stopped",
        errors_dropped: outcome.sparse.errors_dropped,
        records_dropped: outcome.sparse.records_dropped,
        dropped_samples: 0,
        pools: snapshot
            .get("pools")
            .and_then(|value| value.as_array())
            .cloned()
            .unwrap_or_default(),
    }
}

/// 节点平均 QPS：优先用 done 的总量 / 时长，失联节点用采样窗口估算
fn node_qps(node: &MergeNode) -> f64 {
    if let Some(done) = &node.done {
        let elapsed = (done.end_ms - done.start_ms).max(0) as f64 / 1000.0;
        if elapsed > 0.0 {
            return done.total as f64 / elapsed;
        }
    }
    let Some(first) = node.samples.first() else {
        return 0.0;
    };
    let last = node.samples.last().unwrap_or(first);
    let span = (last.elapsed_sec.saturating_sub(first.elapsed_sec) + 1) as f64;
    let requests: u64 = node
        .samples
        .iter()
        .map(|sample| sample.success_count + sample.error_count)
        .sum();
    requests as f64 / span
}

/// 采集健康度快照：报告只关心「是否完整」，实时态由主节点会话维护
fn node_health(node: &MergeNode, start_ms: Option<i64>, end_ms: Option<i64>) -> SampleHealth {
    let received = node.samples.len() as u64;
    let lost = node
        .done
        .as_ref()
        .map(|done| done.dropped_samples)
        .unwrap_or(0);
    let expected = received.saturating_add(lost);
    let state = if node.done.is_some() {
        if lost > 0 {
            "gap"
        } else {
            "done"
        }
    } else {
        "lost"
    };
    SampleHealth {
        state: state.to_string(),
        first_sample_ms: start_ms,
        last_sample_ms: end_ms,
        expected,
        received,
        lost,
        gap_sec: if node.done.is_some() { 0 } else { 1 },
        message: if node.done.is_none() {
            Some("节点失联，曲线只到最后一个采样".to_string())
        } else if lost > 0 {
            Some(format!("丢失 {lost} 个采样点"))
        } else {
            None
        },
    }
}

/// 每秒采样里的 P95（毫秒）
fn sample_p95(sample: &SamplePayload) -> f64 {
    let total: u64 = sample.latency_buckets.iter().map(|(_, count)| *count).sum();
    if total == 0 {
        return 0.0;
    }
    let target = ((total as f64) * 0.95).ceil() as u64;
    let mut seen = 0u64;
    for (ms, count) in &sample.latency_buckets {
        seen += *count;
        if seen >= target {
            return *ms as f64;
        }
    }
    sample
        .latency_buckets
        .last()
        .map(|(ms, _)| *ms as f64)
        .unwrap_or(sample.avg_rt)
}

/// 峰值指标专题：峰值、出现时刻、平台期
///
/// 单机压测也复用该函数，保证「报告页 / 导出 / AI」与集群同一口径。
pub(crate) fn build_peaks(samples: &[ReportSample], start_ms: i64) -> ReportPeaks {
    fn metric(start_ms: i64, values: impl Iterator<Item = (usize, u64, f64)>) -> (usize, PeakMetric) {
        let mut best = (0usize, 0u64, 0.0f64);
        for (index, second, value) in values {
            if value > best.2 {
                best = (index, second, value);
            }
        }
        (
            best.0,
            PeakMetric {
                value: best.2,
                at_ms: start_ms + best.1.saturating_mul(1000) as i64,
                second: best.1,
                held_sec: 0,
                top_node: None,
            },
        )
    }
    let qps = metric(
        start_ms,
        samples
            .iter()
            .enumerate()
            .map(|(index, sample)| (index, sample.elapsed_sec, sample.qps)),
    );
    let concurrent = metric(
        start_ms,
        samples
            .iter()
            .enumerate()
            .map(|(index, sample)| (index, sample.elapsed_sec, sample.concurrent as f64)),
    );
    let bandwidth = metric(
        start_ms,
        samples
            .iter()
            .enumerate()
            .map(|(index, sample)| (index, sample.elapsed_sec, sample.bytes as f64)),
    );
    let p95 = metric(
        start_ms,
        samples
            .iter()
            .enumerate()
            .map(|(index, sample)| (index, sample.elapsed_sec, sample.p95_rt)),
    );
    let p99 = metric(
        start_ms,
        samples
            .iter()
            .enumerate()
            .map(|(index, sample)| (index, sample.elapsed_sec, sample.p99_rt)),
    );
    let error_rate = metric(start_ms, samples.iter().enumerate().map(|(index, sample)| {
        let total = sample.success_count + sample.error_count;
        (
            index,
            sample.elapsed_sec,
            if total > 0 {
                sample.error_count as f64 / total as f64
            } else {
                0.0
            },
        )
    }));

    // 平台期：从峰值点向两侧扩展，统计 ≥ 峰值 98% 的连续秒数
    fn hold(
        samples: &[ReportSample],
        index: usize,
        peak: f64,
        pick: impl Fn(&ReportSample) -> f64,
    ) -> u64 {
        if peak <= 0.0 || samples.is_empty() {
            return 0;
        }
        let threshold = peak * 0.98;
        let mut count = 1u64;
        let mut left = index;
        while left > 0 && pick(&samples[left - 1]) >= threshold {
            count += 1;
            left -= 1;
        }
        let mut right = index;
        while right + 1 < samples.len() && pick(&samples[right + 1]) >= threshold {
            count += 1;
            right += 1;
        }
        count
    }
    let mut qps_peak = qps.1;
    qps_peak.held_sec = hold(samples, qps.0, qps_peak.value, |sample| sample.qps);
    let mut concurrent_peak = concurrent.1;
    concurrent_peak.held_sec = hold(samples, concurrent.0, concurrent_peak.value, |sample| {
        sample.concurrent as f64
    });
    let mut bandwidth_peak = bandwidth.1;
    bandwidth_peak.held_sec = hold(samples, bandwidth.0, bandwidth_peak.value, |sample| {
        sample.bytes as f64
    });
    let mut p95_peak = p95.1;
    p95_peak.held_sec = hold(samples, p95.0, p95_peak.value, |sample| sample.p95_rt);
    let mut p99_peak = p99.1;
    p99_peak.held_sec = hold(samples, p99.0, p99_peak.value, |sample| sample.p99_rt);
    let mut error_peak = error_rate.1;
    error_peak.held_sec = hold(samples, error_rate.0, error_peak.value, |sample| {
        let total = sample.success_count + sample.error_count;
        if total > 0 {
            sample.error_count as f64 / total as f64
        } else {
            0.0
        }
    });

    ReportPeaks {
        qps: qps_peak,
        concurrent: concurrent_peak,
        bandwidth: bandwidth_peak,
        p95: p95_peak,
        p99: p99_peak,
        error_rate: error_peak,
    }
}

/// 节点级序列（抽稀到最多约 1800 点）
fn build_node_series(nodes: &[MergeNode]) -> Vec<NodeSeries> {
    const MAX_POINTS: usize = 1800;
    nodes
        .iter()
        .filter(|node| !node.samples.is_empty())
        .map(|node| {
            let step = (node.samples.len() / MAX_POINTS).max(1);
            let points = node
                .samples
                .iter()
                .step_by(step)
                .map(|sample| SeriesPoint {
                    sec: sample.elapsed_sec,
                    qps: sample.qps,
                    concurrent: sample.concurrent,
                    p95: sample_p95(sample),
                    errors: sample.error_count,
                })
                .collect();
            NodeSeries {
                node: node.name.clone(),
                role: node.role.clone(),
                points,
            }
        })
        .collect()
}

fn ratio(sum: u64, count: u64) -> f64 {
    if count == 0 {
        0.0
    } else {
        sum as f64 / count as f64
    }
}

/// 按桶重算分位：目标名次 ceil(total * p)，累加到该桶即为分位值
fn percentile(buckets: &BTreeMap<u32, u64>, total: u64, p: f64) -> f64 {
    if total == 0 {
        return 0.0;
    }
    let target = ((total as f64) * p).ceil() as u64;
    let mut acc = 0u64;
    for (ms, count) in buckets {
        acc += count;
        if acc >= target {
            return *ms as f64;
        }
    }
    buckets
        .keys()
        .next_back()
        .map(|value| *value as f64)
        .unwrap_or(0.0)
}

/// 按毫秒桶中值计算样本标准差（n-1），与单机引擎口径一致
fn stddev(buckets: &BTreeMap<u32, u64>, total: u64, mean: f64) -> f64 {
    if total < 2 {
        return 0.0;
    }
    let mut acc = 0.0;
    for (ms, count) in buckets {
        let diff = *ms as f64 - mean;
        acc += *count as f64 * diff * diff;
    }
    (acc / (total - 1) as f64).sqrt()
}

fn apdex(buckets: &BTreeMap<u32, u64>, total: u64) -> f64 {
    if total == 0 {
        return 0.0;
    }
    let mut satisfied = 0u64;
    let mut tolerating = 0u64;
    for (ms, count) in buckets {
        let value = *ms as f64;
        if value <= APDEX_T {
            satisfied += count;
        } else if value <= APDEX_T * 4.0 {
            tolerating += count;
        }
    }
    (satisfied as f64 + tolerating as f64 / 2.0) / total as f64
}

fn sorted_codes(codes: &BTreeMap<u16, u64>) -> Vec<StatusCount> {
    let mut items: Vec<StatusCount> = codes
        .iter()
        .map(|(code, count)| StatusCount {
            code: *code as i64,
            count: *count as i64,
        })
        .collect();
    items.sort_by(|a, b| b.count.cmp(&a.count).then(a.code.cmp(&b.code)));
    items
}

fn sorted_error_types(kinds: &BTreeMap<String, i64>) -> Vec<ErrorTypeCount> {
    let mut items: Vec<ErrorTypeCount> = kinds
        .iter()
        .map(|(kind, count)| ErrorTypeCount {
            error_type: kind.clone(),
            count: *count,
        })
        .collect();
    items.sort_by(|a, b| b.count.cmp(&a.count).then(a.error_type.cmp(&b.error_type)));
    items
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(sec: u64, qps: f64, count: u64, latency: Vec<(u32, u64)>) -> SamplePayload {
        SamplePayload {
            elapsed_sec: sec,
            concurrent: 8,
            qps,
            avg_rt: 20.0,
            ttfb_avg: 10.0,
            body_avg: 5.0,
            bytes: count * 100,
            success_count: count,
            error_count: 0,
            sum_rt_ms: count * 20,
            sum_ttfb_ms: count * 10,
            latency_buckets: latency,
        }
    }

    fn done(total: u64, buckets: Vec<(u32, u64)>) -> DonePayload {
        DonePayload {
            start_ms: 1_000,
            end_ms: 11_000,
            total,
            success: total,
            errors: 0,
            bytes: total * 100,
            bytes_sent: total * 10,
            sum_rt_ms: total * 20,
            sum_ttfb_ms: total * 10,
            min_rt: 1.0,
            max_rt: 100.0,
            latency_buckets: buckets,
            ttfb_buckets: vec![(10, total)],
            body_buckets: vec![(5, total)],
            status_codes: vec![(200, total)],
            error_types: vec![],
            cpu_peak: 0.5,
            mem_peak_mb: 128,
            stopped: false,
            errors_dropped: 0,
            records_dropped: 0,
            dropped_samples: 0,
            pools: Vec::new(),
        }
    }

    fn node(id: &str, samples: Vec<SamplePayload>, done: Option<DonePayload>) -> MergeNode {
        MergeNode {
            name: id.to_string(),
            role: "slave".to_string(),
            addr: "127.0.0.1:1".to_string(),
            weight: 1.0,
            concurrent: 8,
            status: if done.is_some() { "done" } else { "lost" }.to_string(),
            clock_offset_ms: 0.0,
            clock_uncertainty_ms: 5.0,
            samples,
            done,
        }
    }

    fn request(nodes: Vec<MergeNode>) -> MergeRequest {
        MergeRequest {
            plan_id: None,
            plan_name: "merged".to_string(),
            request_id: None,
            run_group_id: None,
            total_concurrent: 16,
            started_at: 1_000,
            finished_at: 11_000,
            incomplete: false,
            note: None,
            config_snapshot: "{}".to_string(),
            nodes,
        }
    }

    #[test]
    fn merge_sums_qps_and_recomputes_percentile_from_buckets() {
        let left = done(100, vec![(10, 99), (900, 1)]);
        let right = done(900, vec![(10, 1), (20, 899)]);
        let merged = merge(request(vec![
            node("a", vec![sample(1, 10.0, 10, vec![(10, 10)])], Some(left)),
            node("b", vec![sample(1, 30.0, 30, vec![(20, 30)])], Some(right)),
        ]));

        assert_eq!(merged.report.total_requests, 1_000);
        // 均值按请求数加权，而不是两个节点均值的简单平均
        assert!((merged.report.avg_rt - 20.0).abs() < 0.01);
        // 合并后 P50/P95 都落在 20ms：两个节点各自 P95 是 10 和 20，取平均会得到 15
        assert_eq!(merged.report.p50_rt, 20.0);
        assert_eq!(merged.report.p95_rt, 20.0);
        // 第一秒 QPS 相加
        assert_eq!(merged.samples.len(), 1);
        assert!((merged.samples[0].qps - 40.0).abs() < 0.01);
        assert!((merged.report.max_qps - 40.0).abs() < 0.01);
        // 集群元数据写进配置快照
        let config: serde_json::Value = serde_json::from_str(&merged.config_snapshot).unwrap();
        assert_eq!(config["mode"], "cluster");
        assert_eq!(config["cluster"]["totalConcurrent"], 16);
        assert_eq!(merged.meta.nodes.len(), 2);
    }

    #[test]
    fn lost_node_counts_only_samples_until_lost() {
        let lost = node(
            "b",
            vec![
                sample(1, 10.0, 10, vec![(10, 10)]),
                sample(2, 10.0, 10, vec![(10, 10)]),
            ],
            None,
        );
        let merged = merge(MergeRequest {
            incomplete: true,
            ..request(vec![
                node(
                    "a",
                    vec![sample(1, 10.0, 10, vec![(20, 10)])],
                    Some(done(100, vec![(20, 100)])),
                ),
                lost,
            ])
        });
        // 主节点 100 + 失联节点失联前 20
        assert_eq!(merged.report.total_requests, 120);
        assert!(merged.meta.incomplete);
        assert_eq!(merged.meta.nodes[1].status, "lost");
        assert!((merged.meta.nodes[1].qps_avg - 10.0).abs() < 0.01);
        // 失联节点仍计入每秒曲线
        assert!((merged.samples[0].qps - 20.0).abs() < 0.01);
    }

    #[test]
    fn high_clock_uncertainty_drops_per_second_samples() {
        let mut skewed = node(
            "b",
            vec![sample(1, 10.0, 10, vec![(10, 10)])],
            Some(done(50, vec![(10, 50)])),
        );
        skewed.clock_uncertainty_ms = CLOCK_DROP_UNCERTAINTY_MS + 1.0;
        let merged = merge(request(vec![
            node(
                "a",
                vec![sample(1, 10.0, 10, vec![(20, 10)])],
                Some(done(50, vec![(20, 50)])),
            ),
            skewed,
        ]));
        // 曲线只含主节点，采样被丢弃并计数
        assert_eq!(merged.samples.len(), 1);
        assert!((merged.samples[0].qps - 10.0).abs() < 0.01);
        // 总量仍按 done 合并（计数不受时间轴影响）
        assert_eq!(merged.report.total_requests, 100);
        assert_eq!(merged.meta.dropped_samples, 1);
        assert_eq!(merged.meta.nodes[1].dropped_samples, 1);
    }

    #[test]
    fn merged_report_persists_into_existing_report_tables() {
        use crate::commands::stress::ReportPayload;
        use crate::models::{ReportError, ReportRequestRecord, SlowRequest, StageSummary};

        let (dir, pool) = crate::notes::test_pool("cluster-merge-report");
        let merged = merge(request(vec![
            node(
                "master",
                vec![sample(1, 10.0, 10, vec![(10, 10)])],
                Some(done(100, vec![(10, 100)])),
            ),
            node(
                "slave",
                vec![sample(1, 10.0, 10, vec![(20, 10)])],
                Some(done(100, vec![(20, 100)])),
            ),
        ]));
        let errors: Vec<ReportError> = Vec::new();
        let requests: Vec<ReportRequestRecord> = Vec::new();
        let slow: Vec<SlowRequest> = Vec::new();
        let stages: Vec<StageSummary> = Vec::new();
        let report_id = crate::commands::stress::save_report(
            &pool,
            ReportPayload {
                report: &merged.report,
                samples: &merged.samples,
                errors: &errors,
                requests: &requests,
                stages: &stages,
                status_codes: &merged.status_codes,
                histogram: &merged.histogram,
                body_histogram: &merged.body_histogram,
                slow_requests: &slow,
                error_types: &merged.error_types,
                config: &merged.config_snapshot,
            },
        )
        .unwrap();

        let conn = pool.get().unwrap();
        let (total, config): (i64, String) = conn
            .query_row(
                "SELECT total_requests, config_json FROM test_reports WHERE id = ?1",
                rusqlite::params![report_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(total, 200);
        let config: serde_json::Value = serde_json::from_str(&config).unwrap();
        assert_eq!(config["mode"], "cluster");
        assert_eq!(config["cluster"]["nodes"].as_array().unwrap().len(), 2);
        let samples: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM report_samples WHERE report_id = ?1",
                rusqlite::params![report_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(samples, merged.samples.len() as i64);
        drop(conn);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn master_outcome_converts_to_done_payload() {
        let outcome = EngineOutcome {
            report: StressReport {
                id: 0,
                plan_id: None,
                run_group_id: None,
                plan_name: "local".to_string(),
                request_id: None,
                peak_qps: 0.0,
                total_requests: 10,
                success_count: 9,
                error_count: 1,
                avg_rt: 20.0,
                min_rt: 1.0,
                max_rt: 100.0,
                p50_rt: 10.0,
                p75_rt: 20.0,
                p90_rt: 30.0,
                p95_rt: 40.0,
                p99_rt: 50.0,
                p999_rt: 60.0,
                stddev_rt: 1.0,
                ttfb_avg: 10.0,
                ttfb_p95: 20.0,
                apdex: 1.0,
                bytes_total: 1000,
                max_qps: 5.0,
                avg_qps: 1.0,
                target_rps: 0.0,
                error_rate: 10.0,
                concurrency: 8,
                status: "stopped".to_string(),
                is_cluster: true,
                start_time: Some(5_000),
                end_time: Some(9_000),
                created_at: 9_000,
            },
            samples: vec![],
            errors: vec![],
            requests: vec![],
            slow_requests: vec![],
            stages: vec![],
            status_codes: vec![],
            histogram: vec![],
            body_histogram: vec![],
            error_types: vec![],
            config_snapshot: r#"{"metrics":{"cpuPeakCores":1.5,"memPeakMb":256}}"#.to_string(),
            sparse: crate::commands::stress::EngineSparseMetrics {
                total: 10,
                success: 9,
                errors: 1,
                bytes: 1000,
                bytes_sent: 100,
                sum_rt_ms: 200,
                sum_ttfb_ms: 100,
                min_rt: 1.0,
                max_rt: 100.0,
                latency: vec![(10, 10)],
                ttfb: vec![(10, 10)],
                body: vec![(5, 10)],
                status_codes: vec![(200, 9), (500, 1)],
                error_types: vec![("timeout".to_string(), 1)],
                errors_dropped: 2,
                records_dropped: 3,
            },
            message: None,
        };
        let payload = done_from_outcome(&outcome);
        assert_eq!(payload.start_ms, 5_000);
        assert_eq!(payload.end_ms, 9_000);
        assert!(payload.stopped);
        assert_eq!(payload.cpu_peak, 1.5);
        assert_eq!(payload.mem_peak_mb, 256);
        assert_eq!(payload.total, 10);
        assert_eq!(payload.latency_buckets, vec![(10, 10)]);
    }

    #[test]
    fn build_peaks_reports_peak_time_and_plateau() {
        let sample = |sec: u64, qps: f64, concurrent: u32| ReportSample {
            elapsed_sec: sec,
            concurrent,
            qps,
            avg_rt: 20.0,
            p50_rt: 10.0,
            p95_rt: 50.0,
            p99_rt: 100.0,
            ttfb_avg: 5.0,
            body_avg: 15.0,
            body_p95: 40.0,
            bytes: 1_000,
            success_count: 10,
            error_count: 0,
        };
        let samples = vec![
            sample(1, 10.0, 10),
            sample(2, 100.0, 20),
            sample(3, 99.0, 20),
            sample(4, 98.0, 20),
            sample(5, 5.0, 10),
        ];
        let peaks = build_peaks(&samples, 1_000);
        assert!((peaks.qps.value - 100.0).abs() < f64::EPSILON);
        assert_eq!(peaks.qps.second, 2);
        assert_eq!(peaks.qps.at_ms, 3_000);
        // 峰值点及其两侧 ≥ 98% 的连续区间：100 / 99 / 98 共 3 秒
        assert_eq!(peaks.qps.held_sec, 3);
        assert_eq!(peaks.concurrent.value, 20.0);
        assert_eq!(peaks.concurrent.held_sec, 3);
    }

    #[test]
    fn merges_pool_stats_across_nodes() {
        let mut master = done(10, vec![(20, 10)]);
        master.pools = vec![json!({
            "name": "tokens",
            "items": 100,
            "used": 10,
            "requests": 40,
            "refreshed": 1,
            "unauthorized": 0,
            "invalid": 0,
            "cooling": 0,
            "exhausted": false,
            "perItem": [{ "index": 1, "requests": 10 }],
        })];
        let mut slave = done(10, vec![(20, 10)]);
        slave.pools = vec![json!({
            "name": "tokens",
            "items": 100,
            "used": 6,
            "requests": 30,
            "refreshed": 2,
            "unauthorized": 1,
            "invalid": 0,
            "cooling": 0,
            "exhausted": true,
            "perItem": [{ "index": 1, "requests": 5 }],
        })];
        let mut master = node("master", vec![], Some(master));
        master.role = "master".to_string();
        let merged = merge(request(vec![master, node("slave-a", vec![], Some(slave))]));
        let config: serde_json::Value = serde_json::from_str(&merged.config_snapshot).unwrap();
        let pool = &config["pools"][0];
        assert_eq!(pool["name"], "tokens");
        // 条目取最大值（per_vu 下每节点持有完整池），用量求和
        assert_eq!(pool["items"], 100);
        assert_eq!(pool["used"], 16);
        assert_eq!(pool["requests"], 70);
        assert_eq!(pool["refreshed"], 3);
        assert_eq!(pool["unauthorized"], 1);
        assert_eq!(pool["exhausted"], true);
        assert_eq!(pool["nodeScope"], "cluster");
        assert_eq!(pool["byNode"].as_array().unwrap().len(), 2);
        assert_eq!(pool["perItem"][0]["requests"], 15);
    }
}

/// 12.5 性能验收（手动执行，默认测试集跳过）
///
/// `cargo test --lib perf_node_series_24h_16_nodes -- --ignored --nocapture`
#[cfg(test)]
mod perf_tests {
    use super::*;
    use std::time::Instant;

    #[test]
    #[ignore = "性能验收，需手动执行"]
    fn perf_node_series_24h_16_nodes() {
        // 24 小时 = 86,400 秒；16 个节点，每个节点每秒一条采样
        let nodes = (0..16u32)
            .map(|index| MergeNode {
                name: format!("node-{index:02}"),
                role: if index == 0 { "master".into() } else { "slave".into() },
                addr: format!("192.168.1.{}:17801", index + 10),
                weight: 1.0,
                concurrent: 100,
                status: "done".into(),
                clock_offset_ms: 0.0,
                clock_uncertainty_ms: 0.0,
                samples: (0..86_400u64)
                    .map(|sec| SamplePayload {
                        elapsed_sec: sec,
                        concurrent: 100,
                        qps: 100.0,
                        avg_rt: 20.0,
                        ttfb_avg: 10.0,
                        body_avg: 5.0,
                        bytes: 10_000,
                        success_count: 100,
                        error_count: 0,
                        sum_rt_ms: 2_000,
                        sum_ttfb_ms: 1_000,
                        latency_buckets: Vec::new(),
                    })
                    .collect(),
                done: None,
            })
            .collect::<Vec<_>>();

        let started = Instant::now();
        let series = build_node_series(&nodes);
        let elapsed = started.elapsed();
        let rows: usize = series.iter().map(|node| node.points.len()).sum();
        println!("perf_node_series_24h_16_nodes: {rows} points in {elapsed:?}");
        assert_eq!(series.len(), 16);
        assert!(rows <= 50_000, "节点序列 {rows} 行超过 50k 目标");
    }
}
