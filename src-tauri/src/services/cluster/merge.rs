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
    ClusterReportMeta, ClusterReportNode, DonePayload, Orchestration, SamplePayload,
    CLOCK_DROP_UNCERTAINTY_MS, PROTOCOL_VERSION,
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
    /// 计划级编排：写入报告元数据，便于还原阶段边界
    pub orchestration: Option<Orchestration>,
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
        });
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
        start_time: Some(start_ms),
        end_time: Some(end_ms),
        created_at: end_ms,
        external_metrics: None,
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
    let meta = ClusterReportMeta {
        protocol_version: PROTOCOL_VERSION,
        master_node,
        total_concurrent: request.total_concurrent,
        incomplete: request.incomplete,
        started_at: start_ms,
        finished_at: end_ms,
        dropped_samples,
        nodes: node_meta,
        note: request.note.clone(),
        orchestration: request.orchestration.clone(),
    };

    let mut config: serde_json::Value =
        serde_json::from_str(&request.config_snapshot).unwrap_or_else(|_| json!({}));
    if let Some(map) = config.as_object_mut() {
        map.insert("mode".to_string(), json!("cluster"));
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

/// 主节点本地引擎结果转 `DonePayload`：时间按本地时钟（偏移 0）直接使用
pub(crate) fn done_from_outcome(outcome: &EngineOutcome) -> DonePayload {
    let report = &outcome.report;
    let end_ms = report.end_time.unwrap_or(report.created_at);
    let start_ms = report.start_time.unwrap_or(end_ms);
    let runtime = serde_json::from_str::<serde_json::Value>(&outcome.config_snapshot)
        .ok()
        .and_then(|value| value.get("metrics").cloned())
        .unwrap_or_else(|| json!({}));
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
            orchestration: None,
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
                job_id: None,
                samples: &merged.samples,
                worker_states: &[],
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
                start_time: Some(5_000),
                end_time: Some(9_000),
                created_at: 9_000,
                external_metrics: None,
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
            worker_states: vec![],
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
}
