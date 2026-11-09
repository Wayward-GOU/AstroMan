use crate::models::{
    ErrorTypeCount, HistogramBucket, ReportDetail, ReportError, ReportRequestRecord,
    ReportRequestsPage, ReportSample, StageSummary, StatusCount, StressReport,
};
use crate::state::AppState;
use crate::util::{err, now_ms};
use rusqlite::{params, Row};
use serde::Serialize;
use serde_json::Value;
use tauri::State;

const REPORT_COLUMNS: &str =
    "id, plan_name, request_id, total_requests, success_count, error_count,
    avg_rt, min_rt, max_rt, p50_rt, p75_rt, p90_rt, p95_rt, p99_rt, p999_rt,
    stddev_rt, ttfb_avg, ttfb_p95, apdex, bytes_total, max_qps, avg_qps, target_rps,
    error_rate, concurrency, status, start_time, end_time, created_at";

/// 详情查询在基础列之后追加的 JSON 列
const JSON_COLUMNS: &str =
    "config_json, stages_json, codes_json, histogram_json, error_types_json, scenario_text, summary_text";

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
    let (
        report,
        config_text,
        stages_text,
        codes_text,
        histogram_text,
        types_text,
        scenario_text,
        summary_text,
    ) = conn
        .query_row(&sql, params![id], |row| {
            Ok((
                map_report(row)?,
                row.get::<_, Option<String>>(29)?,
                row.get::<_, Option<String>>(30)?,
                row.get::<_, Option<String>>(31)?,
                row.get::<_, Option<String>>(32)?,
                row.get::<_, Option<String>>(33)?,
                row.get::<_, Option<String>>(34)?,
                row.get::<_, Option<String>>(35)?,
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
        scenario_text: scenario_text.filter(|text| !text.trim().is_empty()),
        summary_text: summary_text.filter(|text| !text.trim().is_empty()),
    })
}

/// 保存报告的场景描述与总结（传空串等同清空）
#[tauri::command]
pub fn report_update_texts(
    state: State<'_, AppState>,
    id: i64,
    scenario_text: Option<String>,
    summary_text: Option<String>,
) -> Result<(), String> {
    let conn = state.pool.get().map_err(err)?;
    let affected = conn
        .execute(
            "UPDATE test_reports SET scenario_text = ?2, summary_text = ?3 WHERE id = ?1",
            params![
                id,
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
#[tauri::command]
pub fn report_clear_details(state: State<'_, AppState>, id: i64) -> Result<u64, String> {
    let conn = state.pool.get().map_err(err)?;
    let removed = conn
        .execute(
            "DELETE FROM report_requests WHERE report_id = ?1",
            params![id],
        )
        .map_err(err)?;
    Ok(removed as u64)
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
