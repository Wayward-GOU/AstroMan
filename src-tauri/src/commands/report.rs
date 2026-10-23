use crate::models::{
    ErrorTypeCount, HistogramBucket, ReportDetail, ReportError, ReportSample, StageSummary,
    StatusCount, StressReport,
};
use crate::state::AppState;
use crate::util::{err, now_ms};
use rusqlite::{params, Row};
use serde::Serialize;
use serde_json::Value;
use tauri::State;

const REPORT_COLUMNS: &str = "id, plan_name, request_id, total_requests, success_count, error_count,
    avg_rt, min_rt, max_rt, p50_rt, p75_rt, p90_rt, p95_rt, p99_rt, p999_rt,
    stddev_rt, ttfb_avg, ttfb_p95, apdex, bytes_total, max_qps, avg_qps, target_rps,
    error_rate, concurrency, status, start_time, end_time, created_at";

/// 详情查询在基础列之后追加的 JSON 列
const JSON_COLUMNS: &str =
    "config_json, stages_json, codes_json, histogram_json, error_types_json";

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
        start_time: row.get(26)?,
        end_time: row.get(27)?,
        created_at: row.get(28)?,
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

fn load_detail(conn: &rusqlite::Connection, id: i64) -> Result<ReportDetail, String> {
    let sql = format!("SELECT {REPORT_COLUMNS}, {JSON_COLUMNS} FROM test_reports WHERE id = ?1");
    let (report, config_text, stages_text, codes_text, histogram_text, types_text) = conn
        .query_row(&sql, params![id], |row| {
            Ok((
                map_report(row)?,
                row.get::<_, Option<String>>(29)?,
                row.get::<_, Option<String>>(30)?,
                row.get::<_, Option<String>>(31)?,
                row.get::<_, Option<String>>(32)?,
                row.get::<_, Option<String>>(33)?,
            ))
        })
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => "报告不存在".to_string(),
            other => err(other),
        })?;

    let mut sample_stmt = conn
        .prepare(
            "SELECT elapsed_sec, concurrent, qps, avg_rt, p50_rt, p95_rt, p99_rt,
                    ttfb_avg, bytes, success_count, error_count
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
                bytes: row.get::<_, i64>(8)?.max(0) as u64,
                success_count: row.get::<_, i64>(9)?.max(0) as u64,
                error_count: row.get::<_, i64>(10)?.max(0) as u64,
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

    Ok(ReportDetail {
        report,
        samples,
        errors,
        status_codes: parse_list::<StatusCount>(codes_text),
        error_types: parse_list::<ErrorTypeCount>(types_text),
        histogram: parse_list::<HistogramBucket>(histogram_text),
        stages: parse_list::<StageSummary>(stages_text),
        config: parse_value(config_text),
    })
}

#[tauri::command]
pub fn report_list(
    state: State<'_, AppState>,
    limit: Option<u32>,
) -> Result<Vec<StressReport>, String> {
    let limit = limit.unwrap_or(100).clamp(1, 500) as i64;
    let conn = state.pool.get().map_err(err)?;
    let sql = format!("SELECT {REPORT_COLUMNS} FROM test_reports ORDER BY id DESC LIMIT ?1");
    let mut stmt = conn.prepare(&sql).map_err(err)?;
    let rows = stmt.query_map(params![limit], map_report).map_err(err)?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(err)
}

#[tauri::command]
pub fn report_get(state: State<'_, AppState>, id: i64) -> Result<ReportDetail, String> {
    let conn = state.pool.get().map_err(err)?;
    load_detail(&conn, id)
}

#[tauri::command]
pub fn report_delete(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    let conn = state.pool.get().map_err(err)?;
    conn.execute("DELETE FROM test_reports WHERE id = ?1", params![id])
        .map_err(err)?;
    Ok(())
}

/// 导出为可离线归档的 JSON 报告文件
#[tauri::command]
pub fn report_export(state: State<'_, AppState>, id: i64, path: String) -> Result<(), String> {
    let conn = state.pool.get().map_err(err)?;
    let detail = load_detail(&conn, id)?;

    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct ExportBundle {
        format: String,
        version: u32,
        exported_at: i64,
        #[serde(flatten)]
        detail: ReportDetail,
    }

    let bundle = ExportBundle {
        format: "finkmeter.report".to_string(),
        version: 1,
        exported_at: now_ms(),
        detail,
    };

    let json = serde_json::to_string_pretty(&bundle).map_err(err)?;
    std::fs::write(&path, json).map_err(|e| format!("写入文件失败：{e}"))?;
    Ok(())
}
