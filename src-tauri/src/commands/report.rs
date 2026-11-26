use crate::models::{
    ErrorTypeCount, HistogramBucket, ReclaimResult, ReportDetail, ReportError, ReportRequestRecord,
    ReportRequestsPage, ReportSample, StageSummary, StatusCount, StressReport, UrlStat,
};
use crate::state::AppState;
use crate::util::{err, now_ms};
use rusqlite::{params, Row};
use rust_xlsxwriter::{Format, Workbook, Worksheet};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use tauri::State;

const REPORT_COLUMNS: &str =
    "id, plan_name, request_id, total_requests, success_count, error_count,
    avg_rt, min_rt, max_rt, p50_rt, p75_rt, p90_rt, p95_rt, p99_rt, p999_rt,
    stddev_rt, ttfb_avg, ttfb_p95, apdex, bytes_total, max_qps, avg_qps, target_rps,
    error_rate, concurrency, status, start_time, end_time, created_at,
    plan_id, run_group_id, report_title";

/// 详情查询在基础列之后追加的 JSON 列
const JSON_COLUMNS: &str =
    "config_json, stages_json, codes_json, histogram_json, error_types_json, scenario_text, summary_text, slow_requests_json, body_histogram_json";

fn map_report(row: &Row<'_>) -> rusqlite::Result<StressReport> {
    Ok(StressReport {
        id: row.get(0)?,
        plan_name: row.get(1)?,
        request_id: row.get(2)?,
        total_requests: row.get(3)?,
        success_count: row.get(4)?,
        error_count: row.get(5)?,
        avg_rt: row.get(6)?,
        min_rt: row.get(7)?,
        max_rt: row.get(8)?,
        p50_rt: row.get(9)?,
        p75_rt: row.get(10)?,
        p90_rt: row.get(11)?,
        p95_rt: row.get(12)?,
        p99_rt: row.get(13)?,
        p999_rt: row.get(14)?,
        stddev_rt: row.get(15)?,
        ttfb_avg: row.get(16)?,
        ttfb_p95: row.get(17)?,
        apdex: row.get(18)?,
        bytes_total: row.get(19)?,
        max_qps: row.get(20)?,
        avg_qps: row.get(21)?,
        target_rps: row.get(22)?,
        error_rate: row.get(23)?,
        concurrency: row.get(24)?,
        status: row.get(25)?,
        is_cluster: false,
        peak_qps: 0.0,
        start_time: row.get(26)?,
        end_time: row.get(27)?,
        created_at: row.get(28)?,
        plan_id: row.get(29)?,
        run_group_id: row.get(30)?,
        title: row
            .get::<_, Option<String>>(31)?
            .filter(|text| !text.trim().is_empty()),
    })
}

fn parse_list<T: serde::de::DeserializeOwned>(raw: Option<String>) -> Vec<T> {
    raw.and_then(|text| serde_json::from_str::<Vec<T>>(&text).ok())
        .unwrap_or_default()
}

fn parse_value(raw: Option<String>) -> Option<Value> {
    raw.and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .filter(|value| !value.is_null())
}

/// 从配置快照读取峰值 QPS：集群优先 config.cluster.peaks，单机读 config.metricsPeaks
fn peak_qps_config(raw: Option<&str>) -> f64 {
    raw.and_then(|text| serde_json::from_str::<Value>(text).ok())
        .and_then(|value| {
            let peaks = value
                .get("cluster")
                .and_then(|cluster| cluster.get("peaks"))
                .or_else(|| value.get("metricsPeaks"));
            peaks
                .and_then(|peaks| peaks.get("qps"))
                .and_then(|qps| qps.get("value"))
                .and_then(|value| value.as_f64())
        })
        .unwrap_or(0.0)
}

/// 报告配置里的 mode 为 cluster 时说明来自集群压测
fn is_cluster_config(raw: Option<&str>) -> bool {
    raw.and_then(|text| serde_json::from_str::<Value>(text).ok())
        .and_then(|value| {
            value
                .get("mode")
                .and_then(|mode| mode.as_str())
                .map(|mode| mode == "cluster")
        })
        .unwrap_or(false)
}

/// 新报告不再把节点曲线写进 config_json，读取详情时从 cluster_report_samples 拼回，
/// 前端与导出继续使用原有的 config.cluster.nodeSeries 结构。
fn attach_cluster_node_series(
    conn: &rusqlite::Connection,
    id: i64,
    config: &mut Value,
) -> Result<(), String> {
    let Some(cluster) = config
        .get_mut("cluster")
        .and_then(|value| value.as_object_mut())
    else {
        return Ok(());
    };
    let already = cluster
        .get("nodeSeries")
        .and_then(|value| value.as_array())
        .is_some_and(|items| !items.is_empty());
    if already {
        return Ok(());
    }
    let mut stmt = conn
        .prepare(
            "SELECT node, role, elapsed_sec, qps, concurrent, p95_rt, error_count,
                    cpu, host_cpu_percent, mem_mb, net_rx_kbps, net_tx_kbps
             FROM cluster_report_samples WHERE report_id = ?1
             ORDER BY node ASC, elapsed_sec ASC",
        )
        .map_err(err)?;
    let mut rows = stmt.query(params![id]).map_err(err)?;
    let mut series: Vec<(String, String, Vec<Value>)> = Vec::new();
    while let Some(row) = rows.next().map_err(err)? {
        let node: String = row.get(0).map_err(err)?;
        let role: String = row.get(1).map_err(err)?;
        let point = serde_json::json!({
            "sec": row.get::<_, i64>(2).map_err(err)?.max(0),
            "qps": row.get::<_, f64>(3).map_err(err)?,
            "concurrent": row.get::<_, i64>(4).map_err(err)?.max(0),
            "p95": row.get::<_, f64>(5).map_err(err)?,
            "errors": row.get::<_, i64>(6).map_err(err)?.max(0),
            "cpu": row.get::<_, Option<f64>>(7).map_err(err)?.unwrap_or(0.0),
            "hostCpuPercent": row.get::<_, Option<f64>>(8).map_err(err)?.unwrap_or(0.0),
            "memMb": row.get::<_, Option<i64>>(9).map_err(err)?.unwrap_or(0),
            "netRxKbps": row.get::<_, Option<f64>>(10).map_err(err)?.unwrap_or(0.0),
            "netTxKbps": row.get::<_, Option<f64>>(11).map_err(err)?.unwrap_or(0.0),
        });
        match series.last_mut() {
            Some((last_node, _, points)) if last_node == &node => points.push(point),
            _ => series.push((node, role, vec![point])),
        }
    }
    let items: Vec<Value> = series
        .into_iter()
        .map(|(node, role, points)| {
            serde_json::json!({ "node": node, "role": role, "points": points })
        })
        .collect();
    cluster.insert("nodeSeries".to_string(), Value::Array(items));
    Ok(())
}

/// 读取按 URL 聚合的统计：表中按状态码分行，这里按 (method, url) 再合并
///
/// 旧报告没有这张表或没有数据时返回空数组，不报错；URL 与错误信息在读取时
/// 再脱敏一次，避免旧数据绕过写入期掩码。
fn load_url_stats(conn: &rusqlite::Connection, id: i64) -> Vec<UrlStat> {
    #[derive(Default)]
    struct Accum {
        requests: i64,
        success: i64,
        errors: i64,
        sum_ms: f64,
        max_ms: f64,
        max_at_ms: Option<i64>,
        bytes: i64,
        error_message: Option<String>,
        codes: BTreeMap<i64, i64>,
    }

    let mut stmt = match conn.prepare(
        "SELECT method, url, status_code, requests, success_count, error_count,
                sum_ms, max_ms, max_at_ms, bytes, error_message
         FROM report_url_stats WHERE report_id = ?1 ORDER BY id ASC",
    ) {
        Ok(stmt) => stmt,
        Err(_) => return Vec::new(),
    };
    let rows = match stmt.query_map(params![id], |row| {
        Ok((
            row.get::<_, Option<String>>(0)?
                .unwrap_or_else(|| "GET".to_string()),
            row.get::<_, Option<String>>(1)?.unwrap_or_default(),
            row.get::<_, i64>(2).unwrap_or(0),
            row.get::<_, i64>(3).unwrap_or(0),
            row.get::<_, i64>(4).unwrap_or(0),
            row.get::<_, i64>(5).unwrap_or(0),
            row.get::<_, f64>(6).unwrap_or(0.0),
            row.get::<_, f64>(7).unwrap_or(0.0),
            row.get::<_, Option<i64>>(8).unwrap_or(None),
            row.get::<_, i64>(9).unwrap_or(0),
            row.get::<_, Option<String>>(10).unwrap_or(None),
        ))
    }) {
        Ok(rows) => rows,
        Err(_) => return Vec::new(),
    };

    let mut merged: BTreeMap<(String, String), Accum> = BTreeMap::new();
    for row in rows.flatten() {
        let (
            method,
            url,
            status,
            requests,
            success,
            errors,
            sum_ms,
            max_ms,
            max_at_ms,
            bytes,
            message,
        ) = row;
        let entry = merged.entry((method, url)).or_default();
        entry.requests += requests;
        entry.success += success;
        entry.errors += errors;
        entry.sum_ms += sum_ms;
        entry.bytes = entry.bytes.saturating_add(bytes);
        if status > 0 {
            *entry.codes.entry(status).or_insert(0) += requests;
        }
        if max_ms > entry.max_ms {
            entry.max_ms = max_ms;
            entry.max_at_ms = max_at_ms;
        }
        if entry.error_message.is_none() {
            if let Some(message) = message.filter(|text| !text.trim().is_empty()) {
                entry.error_message = Some(crate::commands::stress::sanitize_error(&message));
            }
        }
    }

    let mut items: Vec<UrlStat> = merged
        .into_iter()
        .map(|((method, url), accum)| {
            let requests = accum.requests.max(0);
            let status_code = accum
                .codes
                .iter()
                .max_by(|a, b| a.1.cmp(b.1).then(b.0.cmp(a.0)))
                .map(|(code, _)| *code)
                .unwrap_or(0);
            let status_summary = accum
                .codes
                .iter()
                .map(|(code, count)| format!("{code}×{count}"))
                .collect::<Vec<_>>()
                .join(", ");
            UrlStat {
                method,
                url: crate::commands::stress::sanitize_url(&url),
                status_code,
                requests,
                success_count: accum.success,
                error_count: accum.errors,
                avg_ms: if requests > 0 {
                    accum.sum_ms / requests as f64
                } else {
                    0.0
                },
                max_ms: accum.max_ms,
                max_at_ms: accum.max_at_ms,
                total_hours: accum.sum_ms / 3_600_000.0,
                avg_kb: if requests > 0 {
                    accum.bytes as f64 / requests as f64 / 1024.0
                } else {
                    0.0
                },
                total_mb: accum.bytes as f64 / 1024.0 / 1024.0,
                error_message: accum.error_message,
                status_summary,
            }
        })
        .collect();
    items.sort_by(|a, b| b.requests.cmp(&a.requests).then_with(|| a.url.cmp(&b.url)));
    items
}

/// 施压机明细：集群取 config.cluster.nodes；单机合成 1 行本机数据
///
/// 单机行完全来自本报告已落库的聚合值与配置快照，不推断未采集字段：
/// 缺少 bytesSent 时出站带宽为 None，页面与导出显示「—」。
fn build_load_generators(
    config: Option<&Value>,
    report: &StressReport,
) -> Vec<crate::services::cluster::protocol::ClusterReportNode> {
    use crate::services::cluster::protocol::{ClusterReportNode, SampleHealth};

    if let Some(nodes) = config
        .and_then(|value| value.pointer("/cluster/nodes"))
        .and_then(|value| value.as_array())
    {
        return nodes
            .iter()
            .filter_map(|node| serde_json::from_value::<ClusterReportNode>(node.clone()).ok())
            .collect();
    }

    let metrics = config.and_then(|value| value.get("metrics"));
    let agent = config.and_then(|value| value.get("agent"));
    let start = report.start_time.unwrap_or(0);
    let end = report.end_time.unwrap_or(0);
    let duration = (end - start).max(0) as f64 / 1000.0;
    let total = report.total_requests.max(0) as u64;
    let success = report.success_count.max(0) as u64;
    let errors = report.error_count.max(0) as u64;
    let bytes_sent = metrics
        .and_then(|value| value.get("bytesSent"))
        .and_then(|value| value.as_u64());
    vec![ClusterReportNode {
        name: "本机".to_string(),
        role: "master".to_string(),
        addr: agent
            .and_then(|value| value.get("host"))
            .and_then(|value| value.as_str())
            .unwrap_or("—")
            .to_string(),
        weight: 1.0,
        concurrent: report.concurrency.max(0) as u32,
        status: report.status.clone(),
        qps_avg: report.avg_qps,
        cpu_peak: metrics
            .and_then(|value| value.get("cpuPeakCores"))
            .and_then(|value| value.as_f64())
            .unwrap_or(0.0),
        mem_peak_mb: metrics
            .and_then(|value| value.get("memPeakMb"))
            .and_then(|value| value.as_u64())
            .unwrap_or(0),
        host_cpu_peak: 0.0,
        host_mem_peak_percent: 0.0,
        net_peak_kbps: 0.0,
        clock_offset_ms: 0.0,
        clock_uncertainty_ms: 0.0,
        clock_drift_ms: 0.0,
        dropped_samples: 0,
        start_ms: start,
        end_ms: end,
        peak_qps: report.peak_qps.max(report.max_qps),
        peak_concurrent: report.concurrency.max(0) as u32,
        share: 1.0,
        health: SampleHealth::default(),
        total_requests: Some(total),
        success_count: Some(success),
        error_count: Some(errors),
        avg_rt_ms: (total > 0).then_some(report.avg_rt),
        tps: (duration > 0.0).then_some(total as f64 / duration),
        egress_avg_kbps: if duration > 0.0 {
            bytes_sent.map(|bytes| bytes as f64 / duration / 1024.0)
        } else {
            None
        },
    }]
}

fn load_detail(conn: &rusqlite::Connection, id: i64) -> Result<ReportDetail, String> {
    let sql = format!("SELECT {REPORT_COLUMNS}, {JSON_COLUMNS} FROM test_reports WHERE id = ?1");
    let (
        report,
        config_text,
        stages_text,
        codes_text,
        histogram_text,
        types_text,
        scenario_text,
        summary_text,
        slow_requests_text,
        body_histogram_text,
    ) = conn
        .query_row(&sql, params![id], |row| {
            Ok((
                map_report(row)?,
                row.get::<_, Option<String>>(32)?,
                row.get::<_, Option<String>>(33)?,
                row.get::<_, Option<String>>(34)?,
                row.get::<_, Option<String>>(35)?,
                row.get::<_, Option<String>>(36)?,
                row.get::<_, Option<String>>(37)?,
                row.get::<_, Option<String>>(38)?,
                row.get::<_, Option<String>>(39)?,
                row.get::<_, Option<String>>(40)?,
            ))
        })
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => "报告不存在".to_string(),
            other => err(other),
        })?;

    let mut report = report;
    report.is_cluster = is_cluster_config(config_text.as_deref());
    report.peak_qps = peak_qps_config(config_text.as_deref());

    let mut sample_stmt = conn
        .prepare(
            "SELECT elapsed_sec, concurrent, qps, avg_rt, p50_rt, p95_rt, p99_rt,
                    ttfb_avg, body_avg, body_p95, bytes, success_count, error_count,
                    response_count
             FROM report_samples WHERE report_id = ?1 ORDER BY elapsed_sec ASC",
        )
        .map_err(err)?;
    let samples = sample_stmt
        .query_map(params![id], |row| {
            Ok(ReportSample {
                elapsed_sec: row.get::<_, i64>(0)?.max(0) as u64,
                concurrent: row.get::<_, i64>(1)?.max(0) as u32,
                qps: row.get(2)?,
                avg_rt: row.get(3)?,
                p50_rt: row.get(4)?,
                p95_rt: row.get(5)?,
                p99_rt: row.get(6)?,
                ttfb_avg: row.get(7)?,
                body_avg: row.get(8)?,
                body_p95: row.get(9)?,
                bytes: row.get::<_, i64>(10)?.max(0) as u64,
                success_count: row.get::<_, i64>(11)?.max(0) as u64,
                error_count: row.get::<_, i64>(12)?.max(0) as u64,
                response_count: row.get::<_, i64>(13)?.max(0) as u64,
            })
        })
        .map_err(err)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(err)?;

    let mut error_stmt = conn
        .prepare(
            "SELECT elapsed_sec, error_type, error_message, request_url, response_status, response_body
             FROM report_errors WHERE report_id = ?1 ORDER BY id ASC LIMIT 500",
        )
        .map_err(err)?;
    let errors = error_stmt
        .query_map(params![id], |row| {
            Ok(ReportError {
                elapsed_sec: row.get::<_, Option<i64>>(0)?.unwrap_or(0),
                error_type: row
                    .get::<_, Option<String>>(1)?
                    .unwrap_or_else(|| "UNKNOWN".to_string()),
                error_message: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                request_url: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                response_status: row.get(4)?,
                response_body: row.get::<_, Option<String>>(5)?.unwrap_or_default(),
            })
        })
        .map_err(err)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(err)?;

    // 新报告的节点曲线不在 config_json 里，读取时从 cluster_report_samples 拼回；
    // 旧报告已内嵌 nodeSeries 的保持原样，避免重复查询。
    let mut config = parse_value(config_text);
    if let Some(value) = config.as_mut() {
        if let Err(error) = attach_cluster_node_series(conn, id, value) {
            log::warn!("加载集群节点曲线失败：{error}");
        }
    }

    let url_stats = load_url_stats(conn, id);
    if url_stats.is_empty() {
        log::info!("报告 #{id} 没有 URL 统计（旧报告或未采集），页面与导出显示「未记录」");
    }
    // 施压机明细：集群从节点表读取，单机合成 1 行本机
    let load_generators = build_load_generators(config.as_ref(), &report);
    // 旧报告没有 urlStatsComplete 标记：有数据即视为完整，无数据按「未记录」展示
    let url_stats_complete = config
        .as_ref()
        .and_then(|value| value.pointer("/metrics/urlStatsComplete"))
        .and_then(|value| value.as_bool())
        .unwrap_or(!url_stats.is_empty());

    Ok(ReportDetail {
        report,
        samples,
        errors,
        status_codes: parse_list::<StatusCount>(codes_text),
        error_types: parse_list::<ErrorTypeCount>(types_text),
        histogram: parse_list::<HistogramBucket>(histogram_text),
        body_histogram: parse_list::<HistogramBucket>(body_histogram_text),
        stages: parse_list::<StageSummary>(stages_text),
        slow_requests: parse_list::<crate::models::SlowRequest>(slow_requests_text),
        url_stats,
        url_stats_complete,
        load_generators,
        config,
        scenario_text: scenario_text.filter(|text| !text.trim().is_empty()),
        summary_text: summary_text.filter(|text| !text.trim().is_empty()),
    })
}

/// 保存报告标题、场景描述与总结（传空串等同清空/回退）
#[tauri::command]
pub fn report_update_texts(
    state: State<'_, AppState>,
    id: i64,
    title: Option<String>,
    scenario_text: Option<String>,
    summary_text: Option<String>,
) -> Result<(), String> {
    let conn = state.pool.get().map_err(err)?;
    let title = title
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty());
    let affected = conn
        .execute(
            "UPDATE test_reports
             SET report_title = ?2, scenario_text = ?3, summary_text = ?4
             WHERE id = ?1",
            params![
                id,
                title,
                scenario_text.map(|text| text.trim().to_string()),
                summary_text.map(|text| text.trim().to_string())
            ],
        )
        .map_err(err)?;
    if affected == 0 {
        return Err("报告不存在".to_string());
    }
    Ok(())
}

/// 清理某份报告的逐请求明细，保留全部汇总与采样数据
///
/// 明细往往是容量主体（单份报告可达十万级、上百 MB），删完必须压缩，否则文件大小不变。
#[tauri::command]
pub fn report_clear_details(state: State<'_, AppState>, id: i64) -> Result<ReclaimResult, String> {
    let removed = {
        let conn = state.pool.get().map_err(err)?;
        conn.execute(
            "DELETE FROM report_requests WHERE report_id = ?1",
            params![id],
        )
        .map_err(err)?
    };
    let (freed_bytes, compact_error) = if removed > 0 {
        crate::db::compact_after_delete(&state)
    } else {
        (0, None)
    };
    Ok(ReclaimResult {
        removed: removed as u64,
        freed_bytes,
        compact_error,
    })
}

#[tauri::command]
pub fn report_list(
    state: State<'_, AppState>,
    limit: Option<u32>,
) -> Result<Vec<StressReport>, String> {
    let limit = limit.unwrap_or(100).clamp(1, 500) as i64;
    let conn = state.pool.get().map_err(err)?;
    // 列表只需要汇总字段：is_cluster / peak_qps 已在写入时物化，
    // 这里不再读取 config_json，避免为列表反序列化节点序列与池条目明细。
    let sql = format!(
        "SELECT {REPORT_COLUMNS}, COALESCE(is_cluster, 0), COALESCE(peak_qps, 0)
         FROM test_reports ORDER BY id DESC LIMIT ?1"
    );
    let mut stmt = conn.prepare(&sql).map_err(err)?;
    let rows = stmt
        .query_map(params![limit], |row| {
            let mut report = map_report(row)?;
            report.is_cluster = row.get::<_, i64>(32)? != 0;
            report.peak_qps = row.get::<_, f64>(33)?;
            Ok(report)
        })
        .map_err(err)?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(err)
}

#[tauri::command]
pub fn report_get(state: State<'_, AppState>, id: i64) -> Result<ReportDetail, String> {
    let conn = state.pool.get().map_err(err)?;
    load_detail(&conn, id)
}

/// 节点级曲线的一个采样点
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportNodePoint {
    pub sec: u64,
    pub qps: f64,
    pub concurrent: u32,
    pub p95: f64,
    pub errors: u64,
}

/// 一份报告内某个节点的曲线
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportNodeSeriesItem {
    pub report_id: i64,
    pub node: String,
    pub role: String,
    pub points: Vec<ReportNodePoint>,
}

/// 跨报告按节点查询曲线（v19）：不传报告时取最近 20 份
#[tauri::command]
pub fn report_node_series(
    state: State<'_, AppState>,
    report_ids: Option<Vec<i64>>,
    node: Option<String>,
) -> Result<Vec<ReportNodeSeriesItem>, String> {
    let conn = state.pool.get().map_err(err)?;
    let ids: Vec<i64> = report_ids
        .unwrap_or_default()
        .into_iter()
        .filter(|id| *id > 0)
        .take(100)
        .collect();
    let mut sql = String::from(
        "SELECT report_id, node, role, elapsed_sec, qps, concurrent, p95_rt, error_count
         FROM cluster_report_samples WHERE 1 = 1",
    );
    let mut args: Vec<rusqlite::types::Value> = Vec::new();
    if ids.is_empty() {
        sql.push_str(" AND report_id IN (SELECT id FROM test_reports ORDER BY id DESC LIMIT 20)");
    } else {
        let marks = vec!["?"; ids.len()].join(",");
        sql.push_str(&format!(" AND report_id IN ({marks})"));
        args.extend(ids.into_iter().map(rusqlite::types::Value::Integer));
    }
    if let Some(node) = node.filter(|value| !value.trim().is_empty()) {
        sql.push_str(" AND node = ?");
        args.push(rusqlite::types::Value::Text(node.trim().to_string()));
    }
    sql.push_str(" ORDER BY report_id DESC, node ASC, elapsed_sec ASC");
    let mut stmt = conn.prepare(&sql).map_err(err)?;
    let rows = stmt
        .query_map(rusqlite::params_from_iter(args.iter()), |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                ReportNodePoint {
                    sec: row.get::<_, i64>(3)?.max(0) as u64,
                    qps: row.get(4)?,
                    concurrent: row.get(5)?,
                    p95: row.get(6)?,
                    errors: row.get::<_, i64>(7)?.max(0) as u64,
                },
            ))
        })
        .map_err(err)?;
    let mut result: Vec<ReportNodeSeriesItem> = Vec::new();
    for row in rows {
        let (report_id, node, role, point) = row.map_err(err)?;
        match result
            .iter_mut()
            .find(|item| item.report_id == report_id && item.node == node)
        {
            Some(item) => item.points.push(point),
            None => result.push(ReportNodeSeriesItem {
                report_id,
                node,
                role,
                points: vec![point],
            }),
        }
    }
    Ok(result)
}

#[tauri::command]
pub fn report_delete(state: State<'_, AppState>, id: i64) -> Result<ReclaimResult, String> {
    let removed = {
        let conn = state.pool.get().map_err(err)?;
        conn.execute("DELETE FROM test_reports WHERE id = ?1", params![id])
            .map_err(err)?
    };
    // 级联删除的明细会留下大量空闲页，不压缩的话库里空间不会回来
    let (freed_bytes, compact_error) = if removed > 0 {
        crate::db::compact_after_delete(&state)
    } else {
        (0, None)
    };
    Ok(ReclaimResult {
        removed: removed as u64,
        freed_bytes,
        compact_error,
    })
}

/// 导出为可离线归档的 JSON 报告文件
#[tauri::command]
pub fn report_export(state: State<'_, AppState>, id: i64, path: String) -> Result<(), String> {
    let conn = state.pool.get().map_err(err)?;
    let detail = load_detail(&conn, id)?;
    let requests = load_requests(&conn, id, 50_000, 0)?;

    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct ExportBundle {
        format: String,
        version: u32,
        exported_at: i64,
        #[serde(flatten)]
        detail: ReportDetail,
        requests: Vec<ReportRequestRecord>,
    }

    let bundle = ExportBundle {
        format: "astroman.report".to_string(),
        version: 2,
        exported_at: now_ms(),
        detail,
        requests,
    };

    let json = serde_json::to_string_pretty(&bundle).map_err(err)?;
    std::fs::write(&path, json).map_err(|e| format!("写入文件失败：{e}"))?;
    Ok(())
}

fn map_request(row: &Row<'_>) -> rusqlite::Result<ReportRequestRecord> {
    Ok(ReportRequestRecord {
        elapsed_ms: row.get(0)?,
        time_ms: row.get(1)?,
        method: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
        url: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
        status: row.get(4)?,
        latency_ms: row.get(5)?,
        ttfb_ms: row.get(6)?,
        bytes: row.get(7)?,
        success: row.get::<_, i64>(8)? != 0,
        body: row.get(9)?,
        params: row.get(10)?,
        headers: row.get(11)?,
        response_headers: row.get(12)?,
        response_body: row.get(13)?,
        error_type: row.get(14)?,
        error_message: row.get(15)?,
    })
}

fn load_requests(
    conn: &rusqlite::Connection,
    id: i64,
    limit: i64,
    offset: i64,
) -> Result<Vec<ReportRequestRecord>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT elapsed_ms, time_ms, method, url, status, latency_ms, ttfb_ms, bytes,
                    success, body, params, req_headers, res_headers, res_body,
                    error_type, error_message
             FROM report_requests WHERE report_id = ?1 ORDER BY id ASC LIMIT ?2 OFFSET ?3",
        )
        .map_err(err)?;
    let rows = stmt
        .query_map(params![id, limit, offset], map_request)
        .map_err(err)?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(err)
}

/// 分页查询报告中的逐请求明细
#[tauri::command]
pub fn report_requests(
    state: State<'_, AppState>,
    id: i64,
    page: Option<u32>,
    page_size: Option<u32>,
    success: Option<bool>,
) -> Result<ReportRequestsPage, String> {
    let page = page.unwrap_or(1).max(1) as i64;
    let size = page_size.unwrap_or(50).clamp(1, 100_000) as i64;
    let conn = state.pool.get().map_err(err)?;
    let filter = match success {
        Some(true) => " AND success = 1",
        Some(false) => " AND success = 0",
        None => "",
    };

    let total: i64 = conn
        .query_row(
            &format!("SELECT COUNT(*) FROM report_requests WHERE report_id = ?1{filter}"),
            params![id],
            |row| row.get(0),
        )
        .map_err(err)?;

    let mut stmt = conn
        .prepare(&format!(
            "SELECT elapsed_ms, time_ms, method, url, status, latency_ms, ttfb_ms, bytes,
                    success, body, params, req_headers, res_headers, res_body,
                    error_type, error_message
             FROM report_requests WHERE report_id = ?1{filter}
             ORDER BY id ASC LIMIT ?2 OFFSET ?3"
        ))
        .map_err(err)?;
    let items = stmt
        .query_map(params![id, size, (page - 1) * size], map_request)
        .map_err(err)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(err)?;

    Ok(ReportRequestsPage { total, items })
}

/* ---------- 报告 Excel 后端生成 ---------- */

const REQUEST_SHEET: &str = "请求明细";
const SAMPLE_SHEET: &str = "采样明细";

/// 前端构建好的工作表（除请求明细外）。请求明细由后端直接从库里逐行写，
/// 避免 5 万条含请求体/响应体的明细经 IPC 传一遍。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportSheetInput {
    pub name: String,
    pub rows: Vec<Vec<Value>>,
}

fn clean_sheet_name(name: &str, index: usize) -> String {
    let cleaned: String = name
        .chars()
        .filter(|value| !matches!(value, '\\' | '/' | '?' | '*' | '[' | ']' | ':'))
        .take(31)
        .collect();
    if cleaned.trim().is_empty() {
        format!("Sheet{}", index + 1)
    } else {
        cleaned
    }
}

fn clip_text(value: &str, max: usize) -> String {
    let mut chars = value.chars();
    let text: String = chars.by_ref().take(max).collect();
    if chars.next().is_some() {
        format!("{text}…")
    } else {
        text
    }
}

fn format_ms(ms: i64) -> String {
    use chrono::TimeZone;
    if ms <= 0 {
        return "—".to_string();
    }
    chrono::Local
        .timestamp_millis_opt(ms)
        .single()
        .map(|time| time.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_else(|| "—".to_string())
}

fn json_text(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(text)) => text.clone(),
        Some(other) => other.to_string(),
        None => String::new(),
    }
}

/// 与前端 pairsText 同口径：支持 [[k,v]] 与 [{key,value}] 两种头信息形态
fn pairs_text(raw: Option<String>) -> String {
    let Some(raw) = raw else {
        return String::new();
    };
    let Ok(Value::Array(items)) = serde_json::from_str::<Value>(&raw) else {
        return String::new();
    };
    items
        .iter()
        .filter_map(|item| match item {
            Value::Array(pair) => Some(format!(
                "{}: {}",
                json_text(pair.first()),
                json_text(pair.get(1))
            )),
            Value::Object(map) => Some(format!(
                "{}: {}",
                json_text(map.get("key")),
                json_text(map.get("value"))
            )),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn write_cell(
    worksheet: &mut Worksheet,
    row: u32,
    col: u16,
    cell: &Value,
    header: &Format,
) -> Result<(), String> {
    let result = match cell {
        Value::Null => Ok(()),
        Value::Bool(value) => worksheet.write_boolean(row, col, *value).map(|_| ()),
        Value::Number(number) => match number.as_f64() {
            Some(value) => worksheet.write_number(row, col, value).map(|_| ()),
            None => worksheet
                .write_string(row, col, number.to_string())
                .map(|_| ()),
        },
        Value::String(text) => {
            if row == 0 {
                worksheet
                    .write_string_with_format(row, col, text.as_str(), header)
                    .map(|_| ())
            } else {
                worksheet.write_string(row, col, text.clone()).map(|_| ())
            }
        }
        other => worksheet
            .write_string(row, col, other.to_string())
            .map(|_| ()),
    };
    result.map_err(|error| format!("写入工作表失败：{error}"))
}

fn write_rows(
    worksheet: &mut Worksheet,
    rows: &[Vec<Value>],
    header: &Format,
) -> Result<(), String> {
    for (row_index, row) in rows.iter().enumerate() {
        let row_num = u32::try_from(row_index).map_err(|_| "行数超出 Excel 上限".to_string())?;
        for (col_index, cell) in row.iter().enumerate() {
            let col = u16::try_from(col_index).map_err(|_| "列数超出 Excel 上限".to_string())?;
            write_cell(worksheet, row_num, col, cell, header)?;
        }
    }
    if !rows.is_empty() {
        worksheet
            .set_freeze_panes(1, 0)
            .map_err(|error| format!("冻结首行失败：{error}"))?;
    }
    Ok(())
}

const REQUEST_HEADERS: [&str; 16] = [
    "时间(s)",
    "请求时间",
    "方法",
    "URL",
    "请求体",
    "查询参数",
    "请求头",
    "响应头",
    "响应体",
    "状态码",
    "耗时(ms)",
    "TTFB(ms)",
    "响应大小(B)",
    "结果",
    "错误类型",
    "错误信息",
];

fn write_request_sheet(
    conn: &rusqlite::Connection,
    report_id: i64,
    worksheet: &mut Worksheet,
    header: &Format,
) -> Result<(), String> {
    for (index, name) in REQUEST_HEADERS.iter().enumerate() {
        worksheet
            .write_string_with_format(0, index as u16, *name, header)
            .map_err(|error| format!("写入表头失败：{error}"))?;
    }
    let widths: [f64; 16] = [
        10.0, 20.0, 8.0, 60.0, 40.0, 24.0, 40.0, 40.0, 50.0, 8.0, 10.0, 10.0, 12.0, 8.0, 16.0, 50.0,
    ];
    for (col, width) in widths.iter().enumerate() {
        worksheet
            .set_column_width(col as u16, *width)
            .map_err(|error| format!("设置列宽失败：{error}"))?;
    }

    let mut stmt = conn
        .prepare(
            "SELECT elapsed_ms, time_ms, method, url, status, latency_ms, ttfb_ms, bytes,
                    success, body, params, req_headers, res_headers, res_body,
                    error_type, error_message
             FROM report_requests WHERE report_id = ?1 ORDER BY id ASC",
        )
        .map_err(err)?;
    let mut rows = stmt.query(params![report_id]).map_err(err)?;
    let mut row_index = 1u32;
    while let Some(row) = rows.next().map_err(err)? {
        if row_index >= 1_048_576 {
            break;
        }
        let elapsed_ms: i64 = row.get(0).map_err(err)?;
        let time_ms: i64 = row.get(1).map_err(err)?;
        let method: String = row
            .get::<_, Option<String>>(2)
            .map_err(err)?
            .unwrap_or_default();
        let url: String = row
            .get::<_, Option<String>>(3)
            .map_err(err)?
            .unwrap_or_default();
        let status: Option<i64> = row.get(4).map_err(err)?;
        let latency_ms: i64 = row.get(5).map_err(err)?;
        let ttfb_ms: i64 = row.get(6).map_err(err)?;
        let bytes: i64 = row.get(7).map_err(err)?;
        let success: i64 = row.get(8).map_err(err)?;
        let body: Option<String> = row.get(9).map_err(err)?;
        let params: Option<String> = row.get(10).map_err(err)?;
        let req_headers: Option<String> = row.get(11).map_err(err)?;
        let res_headers: Option<String> = row.get(12).map_err(err)?;
        let res_body: Option<String> = row.get(13).map_err(err)?;
        let error_type: Option<String> = row.get(14).map_err(err)?;
        let error_message: Option<String> = row.get(15).map_err(err)?;

        worksheet
            .write_number(
                row_index,
                0,
                (elapsed_ms as f64 / 1000.0 * 1000.0).round() / 1000.0,
            )
            .map_err(|error| format!("写入请求明细失败：{error}"))?;
        worksheet
            .write_string(row_index, 1, format_ms(time_ms))
            .map_err(|error| format!("写入请求明细失败：{error}"))?;
        worksheet
            .write_string(row_index, 2, method)
            .map_err(|error| format!("写入请求明细失败：{error}"))?;
        worksheet
            .write_string(row_index, 3, url)
            .map_err(|error| format!("写入请求明细失败：{error}"))?;
        worksheet
            .write_string(row_index, 4, clip_text(&body.unwrap_or_default(), 1000))
            .map_err(|error| format!("写入请求明细失败：{error}"))?;
        worksheet
            .write_string(row_index, 5, clip_text(&params.unwrap_or_default(), 1000))
            .map_err(|error| format!("写入请求明细失败：{error}"))?;
        worksheet
            .write_string(row_index, 6, clip_text(&pairs_text(req_headers), 1500))
            .map_err(|error| format!("写入请求明细失败：{error}"))?;
        worksheet
            .write_string(row_index, 7, clip_text(&pairs_text(res_headers), 1500))
            .map_err(|error| format!("写入请求明细失败：{error}"))?;
        worksheet
            .write_string(row_index, 8, clip_text(&res_body.unwrap_or_default(), 2000))
            .map_err(|error| format!("写入请求明细失败：{error}"))?;
        if let Some(status) = status.filter(|value| *value != 0) {
            worksheet
                .write_number(row_index, 9, status as f64)
                .map_err(|error| format!("写入请求明细失败：{error}"))?;
        }
        worksheet
            .write_number(row_index, 10, latency_ms as f64)
            .map_err(|error| format!("写入请求明细失败：{error}"))?;
        worksheet
            .write_number(row_index, 11, ttfb_ms as f64)
            .map_err(|error| format!("写入请求明细失败：{error}"))?;
        worksheet
            .write_number(row_index, 12, bytes as f64)
            .map_err(|error| format!("写入请求明细失败：{error}"))?;
        worksheet
            .write_string(row_index, 13, if success != 0 { "成功" } else { "失败" })
            .map_err(|error| format!("写入请求明细失败：{error}"))?;
        worksheet
            .write_string(row_index, 14, error_type.unwrap_or_default())
            .map_err(|error| format!("写入请求明细失败：{error}"))?;
        worksheet
            .write_string(
                row_index,
                15,
                clip_text(&error_message.unwrap_or_default(), 500),
            )
            .map_err(|error| format!("写入请求明细失败：{error}"))?;
        row_index += 1;
    }
    worksheet
        .set_freeze_panes(1, 0)
        .map_err(|error| format!("冻结首行失败：{error}"))?;
    Ok(())
}

const SAMPLE_HEADERS: [&str; 13] = [
    "时间(s)",
    "并发",
    "QPS",
    "平均 RT(ms)",
    "P50(ms)",
    "P95(ms)",
    "P99(ms)",
    "TTFB(ms)",
    "Body 平均(ms)",
    "Body P95(ms)",
    "流量(B)",
    "成功数",
    "失败数",
];

fn write_sample_sheet(
    conn: &rusqlite::Connection,
    report_id: i64,
    worksheet: &mut Worksheet,
    header: &Format,
) -> Result<(), String> {
    for (index, name) in SAMPLE_HEADERS.iter().enumerate() {
        worksheet
            .write_string_with_format(0, index as u16, *name, header)
            .map_err(|error| format!("写入表头失败：{error}"))?;
    }
    let widths: [f64; 13] = [
        10.0, 8.0, 10.0, 12.0, 10.0, 10.0, 10.0, 10.0, 12.0, 12.0, 12.0, 10.0, 10.0,
    ];
    for (col, width) in widths.iter().enumerate() {
        worksheet
            .set_column_width(col as u16, *width)
            .map_err(|error| format!("设置列宽失败：{error}"))?;
    }

    let mut stmt = conn
        .prepare(
            "SELECT elapsed_sec, concurrent, qps, avg_rt, p50_rt, p95_rt, p99_rt,
                    ttfb_avg, body_avg, body_p95, bytes, success_count, error_count,
                    response_count
             FROM report_samples WHERE report_id = ?1 ORDER BY elapsed_sec ASC",
        )
        .map_err(err)?;
    let mut rows = stmt.query(params![report_id]).map_err(err)?;
    let mut row_index = 1u32;
    while let Some(row) = rows.next().map_err(err)? {
        if row_index >= 1_048_576 {
            break;
        }
        let numbers: [f64; 10] = [
            row.get::<_, Option<i64>>(0).map_err(err)?.unwrap_or(0) as f64,
            row.get::<_, Option<i64>>(1).map_err(err)?.unwrap_or(0) as f64,
            row.get::<_, Option<f64>>(2).map_err(err)?.unwrap_or(0.0),
            row.get::<_, Option<f64>>(3).map_err(err)?.unwrap_or(0.0),
            row.get::<_, Option<f64>>(4).map_err(err)?.unwrap_or(0.0),
            row.get::<_, Option<f64>>(5).map_err(err)?.unwrap_or(0.0),
            row.get::<_, Option<f64>>(6).map_err(err)?.unwrap_or(0.0),
            row.get::<_, Option<f64>>(7).map_err(err)?.unwrap_or(0.0),
            row.get::<_, Option<f64>>(8).map_err(err)?.unwrap_or(0.0),
            row.get::<_, Option<f64>>(9).map_err(err)?.unwrap_or(0.0),
        ];
        for (col, value) in numbers.iter().enumerate() {
            worksheet
                .write_number(row_index, col as u16, *value)
                .map_err(|error| format!("写入采样明细失败：{error}"))?;
        }
        let tail: [f64; 3] = [
            row.get::<_, Option<i64>>(10).map_err(err)?.unwrap_or(0) as f64,
            row.get::<_, Option<i64>>(11).map_err(err)?.unwrap_or(0) as f64,
            row.get::<_, Option<i64>>(12).map_err(err)?.unwrap_or(0) as f64,
        ];
        for (offset, value) in tail.iter().enumerate() {
            worksheet
                .write_number(row_index, 10 + offset as u16, *value)
                .map_err(|error| format!("写入采样明细失败：{error}"))?;
        }
        row_index += 1;
    }
    worksheet
        .set_freeze_panes(1, 0)
        .map_err(|error| format!("冻结首行失败：{error}"))?;
    Ok(())
}

fn write_report_workbook(
    conn: &rusqlite::Connection,
    report_id: i64,
    path: &std::path::Path,
    sheets: Vec<ReportSheetInput>,
) -> Result<(), String> {
    let mut workbook = Workbook::new();
    let header = Format::new().set_bold();
    for (index, sheet) in sheets.into_iter().enumerate() {
        let name = clean_sheet_name(&sheet.name, index);
        let worksheet = workbook.add_worksheet();
        worksheet
            .set_name(&name)
            .map_err(|error| format!("写入工作表失败：{error}"))?;
        if sheet.name == REQUEST_SHEET && sheet.rows.is_empty() {
            write_request_sheet(conn, report_id, worksheet, &header)?;
        } else if sheet.name == SAMPLE_SHEET && sheet.rows.is_empty() {
            write_sample_sheet(conn, report_id, worksheet, &header)?;
        } else {
            write_rows(worksheet, &sheet.rows, &header)?;
        }
    }
    workbook
        .save(path)
        .map_err(|error| format!("保存 Excel 失败：{error}"))?;
    Ok(())
}

/// Excel 报告：其余工作表由前端传行数据，请求明细在 Rust 侧流式写入。
#[tauri::command]
pub fn report_export_xlsx(
    state: State<'_, AppState>,
    id: i64,
    path: String,
    sheets: Vec<ReportSheetInput>,
) -> Result<(), String> {
    let conn = state.pool.get().map_err(err)?;
    let exists: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM test_reports WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )
        .map_err(err)?;
    if exists == 0 {
        return Err("报告不存在".to_string());
    }
    let path = crate::services::paths::prepare_write(&path)?;
    write_report_workbook(&conn, id, &path, sheets)
}

/// 将前端生成的报告内容（HTML 文本或 XLSX 字节）写入指定路径。
#[tauri::command]
pub fn report_write_file(path: String, data: Vec<u8>) -> Result<(), String> {
    let path = crate::services::paths::prepare_write(&path)?;
    std::fs::write(&path, data).map_err(|e| format!("写入文件失败（{}）：{e}", path.display()))?;
    Ok(())
}

/* ---------- 明细 CSV 流式导出 ---------- */

/// CSV 字段转义：含逗号、引号、换行时加引号并转义内部引号
fn csv_field(value: &str) -> String {
    if value.contains(',') || value.contains('"') || value.contains('\n') || value.contains('\r') {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

/// 毫秒时间戳转本地时间字符串
fn csv_time(ms: i64) -> String {
    use chrono::TimeZone;
    chrono::Local
        .timestamp_millis_opt(ms)
        .single()
        .map(|time| time.format("%Y-%m-%d %H:%M:%S%.3f").to_string())
        .unwrap_or_default()
}

const REQUEST_CSV_HEADER: [&str; 16] = [
    "时间(s)",
    "绝对时间",
    "方法",
    "URL",
    "状态码",
    "耗时(ms)",
    "TTFB(ms)",
    "响应字节",
    "结果",
    "错误类型",
    "错误信息",
    "请求体",
    "查询参数",
    "请求头",
    "响应头",
    "响应体",
];

const ERROR_CSV_HEADER: [&str; 6] = [
    "时间(s)",
    "错误类型",
    "状态码",
    "错误信息",
    "请求地址",
    "响应体",
];

/// 明细 / 错误 CSV 导出：后端逐行流式写盘，内存占用与明细条数无关
///
/// 不复用 `report_write_file`：那条通道要求前端把整个文件字节经 IPC 传入，
/// 几十万行明细会直接把内存打爆。这里先写临时文件，成功后原子替换。
#[tauri::command]
pub fn report_export_detail_csv(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: i64,
    kind: String,
    path: String,
) -> Result<u64, String> {
    use std::io::Write;

    let conn = state.pool.get().map_err(err)?;
    let (total, query): (i64, &str) = match kind.as_str() {
        "requests" => (
            conn.query_row(
                "SELECT COUNT(*) FROM report_requests WHERE report_id = ?1",
                params![id],
                |row| row.get(0),
            )
            .map_err(err)?,
            "SELECT elapsed_ms, time_ms, method, url, status, latency_ms, ttfb_ms, bytes,
                    success, error_type, error_message, body, params, req_headers,
                    res_headers, res_body
             FROM report_requests WHERE report_id = ?1 ORDER BY id ASC",
        ),
        "errors" => (
            conn.query_row(
                "SELECT COUNT(*) FROM report_errors WHERE report_id = ?1",
                params![id],
                |row| row.get(0),
            )
            .map_err(err)?,
            "SELECT elapsed_sec, error_type, response_status, error_message, request_url,
                    response_body
             FROM report_errors WHERE report_id = ?1 ORDER BY id ASC",
        ),
        other => return Err(format!("未知的导出类型：{other}")),
    };

    let target = std::path::PathBuf::from(&path);
    if let Some(parent) = target.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|e| format!("创建目录失败：{e}"))?;
        }
    }
    let temp = target.with_extension("csv.part");

    let mut done: u64 = 0;
    let write_result = (|| -> Result<(), String> {
        let file = std::fs::File::create(&temp).map_err(|e| format!("创建文件失败：{e}"))?;
        let mut writer = std::io::BufWriter::with_capacity(64 * 1024, file);
        // UTF-8 BOM：Excel 直接双击打开时中文不乱码
        writer
            .write_all("\u{feff}".as_bytes())
            .map_err(|e| format!("写入文件失败：{e}"))?;

        let header: &[&str] = if kind == "requests" {
            &REQUEST_CSV_HEADER
        } else {
            &ERROR_CSV_HEADER
        };
        let header_line = header
            .iter()
            .map(|name| csv_field(name))
            .collect::<Vec<_>>()
            .join(",");
        writeln!(writer, "{header_line}").map_err(|e| format!("写入文件失败：{e}"))?;

        let mut stmt = conn.prepare(query).map_err(err)?;
        if kind == "requests" {
            let mut rows = stmt.query(params![id]).map_err(err)?;
            while let Some(row) = rows.next().map_err(err)? {
                let elapsed_ms: i64 = row.get(0).unwrap_or(0);
                let time_ms: i64 = row.get(1).unwrap_or(0);
                let method: String = row.get(2).unwrap_or_default();
                let url: String = row.get(3).unwrap_or_default();
                let status: i64 = row.get(4).unwrap_or(0);
                let latency: i64 = row.get(5).unwrap_or(0);
                let ttfb: i64 = row.get(6).unwrap_or(0);
                let bytes: i64 = row.get(7).unwrap_or(0);
                let success: i64 = row.get(8).unwrap_or(0);
                let error_type: String = row.get(9).unwrap_or_default();
                let error_message: String = row.get(10).unwrap_or_default();
                let body: String = row.get(11).unwrap_or_default();
                let params_text: String = row.get(12).unwrap_or_default();
                let req_headers: String = row.get(13).unwrap_or_default();
                let res_headers: String = row.get(14).unwrap_or_default();
                let res_body: String = row.get(15).unwrap_or_default();
                let fields = vec![
                    format!("{:.3}", elapsed_ms as f64 / 1000.0),
                    csv_time(time_ms),
                    method,
                    url,
                    status.to_string(),
                    latency.to_string(),
                    ttfb.to_string(),
                    bytes.to_string(),
                    if success != 0 {
                        "成功".to_string()
                    } else {
                        "失败".to_string()
                    },
                    error_type,
                    error_message,
                    body,
                    params_text,
                    req_headers,
                    res_headers,
                    res_body,
                ];
                let line = fields
                    .iter()
                    .map(|value| csv_field(value))
                    .collect::<Vec<_>>()
                    .join(",");
                writeln!(writer, "{line}").map_err(|e| format!("写入文件失败：{e}"))?;
                done += 1;
                if done.is_multiple_of(5000) {
                    let _ = tauri::Emitter::emit(
                        &app,
                        "report:export-progress",
                        serde_json::json!({ "done": done, "total": total, "kind": kind }),
                    );
                }
            }
        } else {
            let mut rows = stmt.query(params![id]).map_err(err)?;
            while let Some(row) = rows.next().map_err(err)? {
                let elapsed_sec: i64 = row.get(0).unwrap_or(0);
                let error_type: String = row.get(1).unwrap_or_default();
                let status: Option<i64> = row.get(2).unwrap_or(None);
                let message: String = row.get(3).unwrap_or_default();
                let url: String = row.get(4).unwrap_or_default();
                let body: String = row.get(5).unwrap_or_default();
                let fields = [
                    elapsed_sec.to_string(),
                    error_type,
                    status.map(|value| value.to_string()).unwrap_or_default(),
                    message,
                    url,
                    body,
                ];
                let line = fields
                    .iter()
                    .map(|value| csv_field(value))
                    .collect::<Vec<_>>()
                    .join(",");
                writeln!(writer, "{line}").map_err(|e| format!("写入文件失败：{e}"))?;
                done += 1;
                if done.is_multiple_of(5000) {
                    let _ = tauri::Emitter::emit(
                        &app,
                        "report:export-progress",
                        serde_json::json!({ "done": done, "total": total, "kind": kind }),
                    );
                }
            }
        }
        writer.flush().map_err(|e| format!("写入文件失败：{e}"))?;
        Ok(())
    })();

    if let Err(error) = write_result {
        let _ = std::fs::remove_file(&temp);
        return Err(error);
    }
    // Windows 下 rename 不覆盖已存在文件，先删除目标
    let _ = std::fs::remove_file(&target);
    std::fs::rename(&temp, &target).map_err(|e| {
        let _ = std::fs::remove_file(&temp);
        format!("保存文件失败：{e}")
    })?;
    Ok(done)
}

#[cfg(test)]
mod tests {
    use super::*;
    use calamine::{open_workbook_auto, Reader};

    #[test]
    fn cluster_node_series_is_attached_from_table_when_config_lacks_it() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE cluster_report_samples (
                 report_id INTEGER, node TEXT, role TEXT, elapsed_sec INTEGER,
                 qps REAL, concurrent INTEGER, p95_rt REAL, error_count INTEGER,
                 cpu REAL, host_cpu_percent REAL, mem_mb INTEGER,
                 net_rx_kbps REAL, net_tx_kbps REAL
             );
             INSERT INTO cluster_report_samples VALUES
                 (1, 'master-a', 'master', 1, 100, 10, 20, 0, 1.5, 50, 256, 10, 20),
                 (1, 'master-a', 'master', 2, 110, 10, 22, 1, 1.6, 51, 257, 11, 21);",
        )
        .unwrap();
        let mut config = serde_json::json!({ "cluster": { "nodes": [] } });
        attach_cluster_node_series(&conn, 1, &mut config).unwrap();
        let series = config["cluster"]["nodeSeries"].as_array().unwrap();
        assert_eq!(series.len(), 1);
        assert_eq!(series[0]["node"], "master-a");
        assert_eq!(series[0]["points"].as_array().unwrap().len(), 2);
        assert_eq!(series[0]["points"][0]["hostCpuPercent"], 50.0);
    }

    #[test]
    fn url_stats_merge_status_rows_and_derive_metrics() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE report_url_stats (
                 id INTEGER PRIMARY KEY AUTOINCREMENT, report_id INTEGER, method TEXT, url TEXT,
                 status_code INTEGER, requests INTEGER, success_count INTEGER, error_count INTEGER,
                 sum_ms REAL, max_ms REAL, max_at_ms INTEGER, bytes INTEGER, error_message TEXT,
                 max_request_id TEXT
             );
             INSERT INTO report_url_stats
                 (report_id, method, url, status_code, requests, success_count, error_count,
                  sum_ms, max_ms, max_at_ms, bytes, error_message)
             VALUES
                 (1, 'GET', 'https://a.test/p?token=abc', 200, 3, 3, 0, 300, 120, 500, 3072, NULL),
                 (1, 'GET', 'https://a.test/p?token=abc', 500, 1, 0, 1, 400, 400, 800, 1024, 'HTTP 500');",
        )
        .unwrap();

        let rows = load_url_stats(&conn, 1);
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        // URL 读取时再次脱敏
        assert_eq!(row.url, "https://a.test/p?token=******");
        assert_eq!(row.requests, 4);
        assert_eq!(row.success_count, 3);
        assert_eq!(row.error_count, 1);
        assert_eq!(row.max_ms, 400.0);
        assert_eq!(row.max_at_ms, Some(800));
        assert_eq!(row.status_code, 200);
        assert_eq!(row.status_summary, "200×3, 500×1");
        assert!((row.avg_ms - 175.0).abs() < 0.001);
        assert!((row.avg_kb - 1.0).abs() < 0.001);
        assert!((row.total_mb - 4096.0 / 1024.0 / 1024.0).abs() < 1e-9);
        assert_eq!(row.error_message.as_deref(), Some("HTTP 500"));
    }

    #[test]
    fn load_url_stats_returns_empty_when_table_missing() {
        // 旧库没有 report_url_stats 时读取详情不能报错，页面与导出显示「未记录」
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        assert!(load_url_stats(&conn, 1).is_empty());
    }

    #[test]
    fn load_detail_reads_report_title_and_falls_back_when_empty() {
        let (dir, pool) = crate::notes::test_pool("report-title");
        let conn = pool.get().unwrap();
        conn.execute(
            "INSERT INTO test_reports (id, plan_name, created_at) VALUES (1, 'plan', 1)",
            [],
        )
        .unwrap();

        let detail = load_detail(&conn, 1).unwrap();
        assert_eq!(detail.report.title, None);

        conn.execute(
            "UPDATE test_reports SET report_title = '自定义标题' WHERE id = 1",
            [],
        )
        .unwrap();
        let detail = load_detail(&conn, 1).unwrap();
        assert_eq!(detail.report.title.as_deref(), Some("自定义标题"));

        // 只有空白字符视同未设置，展示层回退到 plan_name
        conn.execute(
            "UPDATE test_reports SET report_title = '   ' WHERE id = 1",
            [],
        )
        .unwrap();
        let detail = load_detail(&conn, 1).unwrap();
        assert_eq!(detail.report.title, None);
        drop(conn);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn single_machine_load_generator_row_uses_reported_totals() {
        let report = StressReport {
            id: 1,
            plan_id: None,
            run_group_id: None,
            plan_name: "single".to_string(),
            title: None,
            request_id: None,
            total_requests: 100,
            success_count: 95,
            error_count: 5,
            avg_rt: 20.0,
            min_rt: 1.0,
            max_rt: 100.0,
            p50_rt: 18.0,
            p75_rt: 22.0,
            p90_rt: 30.0,
            p95_rt: 40.0,
            p99_rt: 80.0,
            p999_rt: 120.0,
            stddev_rt: 5.0,
            ttfb_avg: 10.0,
            ttfb_p95: 20.0,
            apdex: 0.9,
            bytes_total: 102400,
            max_qps: 10.0,
            avg_qps: 9.0,
            target_rps: 0.0,
            error_rate: 5.0,
            concurrency: 8,
            status: "completed".to_string(),
            is_cluster: false,
            peak_qps: 12.0,
            start_time: Some(1_000),
            end_time: Some(11_000),
            created_at: 11_000,
        };
        let config = serde_json::json!({
            "agent": { "host": "bench-01" },
            "metrics": { "bytesSent": 102_400 },
        });
        let rows = build_load_generators(Some(&config), &report);
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!(row.name, "本机");
        assert_eq!(row.addr, "bench-01");
        assert_eq!(row.total_requests, Some(100));
        assert_eq!(row.success_count, Some(95));
        assert_eq!(row.error_count, Some(5));
        assert_eq!(row.avg_rt_ms, Some(20.0));
        assert_eq!(row.tps, Some(10.0));
        assert!((row.egress_avg_kbps.unwrap() - 10.0).abs() < 1e-9);
    }

    #[test]
    fn xlsx_export_fills_sample_sheet_from_table() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE test_reports (id INTEGER PRIMARY KEY);
             CREATE TABLE report_samples (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 report_id INTEGER NOT NULL, elapsed_sec INTEGER NOT NULL,
                 concurrent INTEGER DEFAULT 0, qps REAL DEFAULT 0, avg_rt REAL DEFAULT 0,
                 p50_rt REAL DEFAULT 0, p95_rt REAL DEFAULT 0, p99_rt REAL DEFAULT 0,
                 ttfb_avg REAL DEFAULT 0, body_avg REAL DEFAULT 0, body_p95 REAL DEFAULT 0,
                 bytes INTEGER DEFAULT 0, success_count INTEGER DEFAULT 0,
                 error_count INTEGER DEFAULT 0, response_count INTEGER DEFAULT 0
             );
             INSERT INTO test_reports (id) VALUES (1);
             INSERT INTO report_samples
                 (report_id, elapsed_sec, concurrent, qps, avg_rt, p50_rt, p95_rt, p99_rt,
                  ttfb_avg, body_avg, body_p95, bytes, success_count, error_count, response_count)
             VALUES (1, 1, 10, 100, 20, 18, 40, 80, 12, 8, 10, 1024, 100, 1, 100),
                    (1, 2, 10, 110, 21, 19, 41, 81, 13, 9, 11, 2048, 110, 0, 110);",
        )
        .unwrap();

        let sheets = vec![ReportSheetInput {
            name: SAMPLE_SHEET.to_string(),
            rows: Vec::new(),
        }];
        let path = std::env::temp_dir().join(format!(
            "astroman-report-samples-{}-{}.xlsx",
            std::process::id(),
            now_ms()
        ));
        write_report_workbook(&conn, 1, &path, sheets).unwrap();

        let mut workbook = open_workbook_auto(&path).unwrap();
        let range = workbook.worksheet_range(SAMPLE_SHEET).unwrap();
        assert_eq!(range.get_size().0, 3);
        assert_eq!(
            range.get_value((0, 0)).map(|cell| cell.to_string()),
            Some("时间(s)".to_string())
        );
        assert_eq!(
            range.get_value((1, 0)).map(|cell| cell.to_string()),
            Some("1".to_string())
        );

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn xlsx_export_writes_provided_sheets_and_request_rows() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE test_reports (id INTEGER PRIMARY KEY);
             CREATE TABLE report_requests (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 report_id INTEGER, elapsed_ms INTEGER, time_ms INTEGER,
                 method TEXT, url TEXT, status INTEGER, latency_ms INTEGER,
                 ttfb_ms INTEGER, bytes INTEGER, success INTEGER, body TEXT,
                 params TEXT, req_headers TEXT, res_headers TEXT, res_body TEXT,
                 error_type TEXT, error_message TEXT
             );
             INSERT INTO test_reports (id) VALUES (1);
             INSERT INTO report_requests
                 (report_id, elapsed_ms, time_ms, method, url, status, latency_ms, ttfb_ms,
                  bytes, success, body, params, req_headers, res_headers, res_body,
                  error_type, error_message)
             VALUES (1, 1500, 1700000000000, 'GET', 'http://example.com/api', 200, 20, 10,
                     100, 1, '', '', '[[\"a\",\"b\"]]', '[]', 'ok', NULL, NULL);",
        )
        .unwrap();

        let sheets = vec![
            ReportSheetInput {
                name: "概览".to_string(),
                rows: vec![
                    vec![Value::String("标题".to_string())],
                    vec![Value::Number(1.into())],
                ],
            },
            ReportSheetInput {
                name: REQUEST_SHEET.to_string(),
                rows: Vec::new(),
            },
        ];
        let path = std::env::temp_dir().join(format!(
            "astroman-report-xlsx-{}-{}.xlsx",
            std::process::id(),
            now_ms()
        ));
        write_report_workbook(&conn, 1, &path, sheets).unwrap();

        let mut workbook = open_workbook_auto(&path).unwrap();
        let names = workbook.sheet_names().to_vec();
        assert_eq!(names, vec!["概览", "请求明细"]);
        let range = workbook.worksheet_range(REQUEST_SHEET).unwrap();
        assert_eq!(range.get_size().0, 2);
        assert_eq!(
            range.get_value((0, 0)).map(|cell| cell.to_string()),
            Some("时间(s)".to_string())
        );

        let _ = std::fs::remove_file(&path);
    }
}
