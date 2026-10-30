//! 测试运行报告生成（HTML / JSON / JUnit / summary）
//!
//! 单点在 Rust 生成（文档 7.1、7.2），CLI 与桌面端共用同一份产物：
//!
//! ```text
//! <data_dir>/test-reports/<runId>/
//! ├─ report.html
//! ├─ report.json
//! ├─ junit.xml
//! ├─ summary.txt
//! ├─ logs/<runCaseId>.log
//! └─ files/...            # 附件副本（路径相对本次运行目录）
//! ```

use crate::models::{RunDetail, TestRun, TestRunAssertion, TestRunCase};
use crate::state::AppState;
use crate::test_runner::load_run_detail;
use crate::util::{err, now_ms};
use chrono::{Local, TimeZone};
use rusqlite::{params, OptionalExtension};
use serde::Serialize;
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

pub const REPORT_SCHEMA_VERSION: &str = "1.0";
/// 每个套件默认保留的报告份数（文档 7.6）
pub const DEFAULT_KEEP_REPORTS: usize = 50;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestRunReport {
    pub run_id: i64,
    pub dir: String,
    pub relative_dir: Option<String>,
    pub files: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactEntry {
    pub id: i64,
    pub run_case_id: Option<i64>,
    pub kind: String,
    pub name: String,
    pub path: String,
    /// 相对 `test-runs/<runId>` 的路径，报告页 `files/` 链接直接用这个值
    pub relative_path: Option<String>,
    pub size_bytes: i64,
    pub exists: bool,
}

/// 压测用例嵌入报告的核心指标
#[derive(Debug, Clone)]
struct StressBrief {
    id: i64,
    plan_name: String,
    total: i64,
    success: i64,
    errors: i64,
    p95: f64,
    p99: f64,
    avg_qps: f64,
    error_rate: f64,
    concurrency: i64,
    status: String,
}

fn load_stress_briefs(
    conn: &rusqlite::Connection,
    detail: &RunDetail,
) -> HashMap<i64, StressBrief> {
    let mut out = HashMap::new();
    for case in &detail.cases {
        let Some(id) = case.report_id else { continue };
        if out.contains_key(&id) {
            continue;
        }
        let item = conn.query_row(
            "SELECT id, plan_name, total_requests, success_count, error_count, p95_rt, p99_rt,
                    avg_qps, error_rate, concurrency, status
             FROM test_reports WHERE id = ?1",
            params![id],
            |row| {
                Ok(StressBrief {
                    id: row.get(0)?,
                    plan_name: row.get(1)?,
                    total: row.get(2)?,
                    success: row.get(3)?,
                    errors: row.get(4)?,
                    p95: row.get(5)?,
                    p99: row.get(6)?,
                    avg_qps: row.get(7)?,
                    error_rate: row.get(8)?,
                    concurrency: row.get(9)?,
                    status: row.get(10)?,
                })
            },
        );
        if let Ok(item) = item {
            out.insert(id, item);
        }
    }
    out
}

/// 生成报告；`formats` 为空时输出全部四种产物
pub fn generate(
    state: &AppState,
    run_id: i64,
    formats: &[String],
    out_dir: Option<&Path>,
) -> Result<TestRunReport, String> {
    let formats = normalize_formats(formats)?;
    let (detail, suite_name, stress) = {
        let conn = state.pool.get().map_err(err)?;
        let detail = load_run_detail(&conn, run_id)?;
        let suite_name: Option<String> = conn
            .query_row(
                "SELECT name FROM test_suites WHERE id = ?1",
                params![detail.run.suite_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(err)?;
        let stress = load_stress_briefs(&conn, &detail);
        (detail, suite_name, stress)
    };
    let suite_name = suite_name.unwrap_or_else(|| format!("套件 #{}", detail.run.suite_id));

    let dir = match out_dir {
        Some(path) => path.to_path_buf(),
        None => state.data_dir.join("test-reports").join(run_id.to_string()),
    };
    std::fs::create_dir_all(&dir)
        .map_err(|error| format!("创建报告目录失败（{}）：{error}", dir.display()))?;

    let artifacts = list_artifacts(state, run_id)?;
    let files = copy_attachments(state, &dir, run_id, &artifacts)?;
    write_case_logs(&dir, &detail)?;

    let mut written = Vec::new();
    if formats.iter().any(|item| item == "html") {
        let html = render_html(&detail, &suite_name, &artifacts, &stress);
        write_text(&dir.join("report.html"), &html)?;
        written.push("report.html".to_string());
    }
    if formats.iter().any(|item| item == "json") {
        let json = render_json(&detail, &suite_name, &artifacts, &stress);
        write_text(
            &dir.join("report.json"),
            &serde_json::to_string_pretty(&json).map_err(err)?,
        )?;
        written.push("report.json".to_string());
    }
    if formats.iter().any(|item| item == "junit") {
        let xml = render_junit(&detail, &suite_name);
        write_text(&dir.join("junit.xml"), &xml)?;
        written.push("junit.xml".to_string());
    }
    if formats.iter().any(|item| item == "summary") {
        let text = render_summary(&detail, &suite_name);
        write_text(&dir.join("summary.txt"), &text)?;
        written.push("summary.txt".to_string());
    }
    written.extend(files);

    let relative_dir = dir
        .strip_prefix(&state.data_dir)
        .ok()
        .map(|path| path.to_string_lossy().replace('\\', "/"));
    {
        let conn = state.pool.get().map_err(err)?;
        conn.execute(
            "UPDATE test_runs SET report_dir = ?1 WHERE id = ?2",
            params![relative_dir, run_id],
        )
        .map_err(err)?;
        let _ = prune_reports(&conn, &state.data_dir, detail.run.suite_id, DEFAULT_KEEP_REPORTS);
    }

    Ok(TestRunReport {
        run_id,
        dir: dir.display().to_string(),
        relative_dir,
        files: written,
    })
}

fn normalize_formats(formats: &[String]) -> Result<Vec<String>, String> {
    if formats.is_empty() {
        return Ok(vec![
            "html".to_string(),
            "json".to_string(),
            "junit".to_string(),
            "summary".to_string(),
        ]);
    }
    let mut out = Vec::new();
    for item in formats {
        for part in item.split(',') {
            let name = part.trim().to_ascii_lowercase();
            if name.is_empty() {
                continue;
            }
            if !matches!(name.as_str(), "html" | "json" | "junit" | "summary") {
                return Err(format!("不支持的报告格式：{name}（可选 html/json/junit/summary）"));
            }
            if !out.contains(&name) {
                out.push(name);
            }
        }
    }
    Ok(out)
}

fn write_text(path: &Path, text: &str) -> Result<(), String> {
    std::fs::write(path, text).map_err(|error| format!("写入 {} 失败：{error}", path.display()))
}

fn write_case_logs(dir: &Path, detail: &RunDetail) -> Result<(), String> {
    let logs_dir = dir.join("logs");
    std::fs::create_dir_all(&logs_dir)
        .map_err(|error| format!("创建日志目录失败：{error}"))?;
    for case in &detail.cases {
        let Some(logs) = case.logs.as_deref() else {
            continue;
        };
        if logs.trim().is_empty() {
            continue;
        }
        let _ = std::fs::write(logs_dir.join(format!("{}.log", case.id)), logs);
    }
    Ok(())
}

/// 把附件复制到报告目录的 `files/` 下，返回相对文件名
fn copy_attachments(
    state: &AppState,
    dir: &Path,
    run_id: i64,
    artifacts: &[ArtifactEntry],
) -> Result<Vec<String>, String> {
    let run_root = state.data_dir.join("test-runs").join(run_id.to_string());
    let mut copied = Vec::new();
    for item in artifacts {
        if !item.exists {
            continue;
        }
        let source = PathBuf::from(&item.path);
        let relative = item
            .relative_path
            .as_ref()
            .map(PathBuf::from)
            .or_else(|| {
                source
                    .strip_prefix(&run_root)
                    .ok()
                    .map(|path| path.to_path_buf())
            })
            .unwrap_or_else(|| PathBuf::from(source.file_name().unwrap_or_default()));
        let target = dir.join("files").join(&relative);
        if let Some(parent) = target.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if std::fs::copy(&source, &target).is_ok() {
            copied.push(format!("files/{}", relative.to_string_lossy().replace('\\', "/")));
        }
    }
    Ok(copied)
}

fn artifact_path(data_dir: &Path, run_id: i64, raw: &str) -> PathBuf {
    let path = Path::new(raw);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        data_dir.join("test-runs").join(run_id.to_string()).join(path)
    }
}

/// 读取某次运行的附件索引（报告页与 CLI 共用）
pub fn list_artifacts(state: &AppState, run_id: i64) -> Result<Vec<ArtifactEntry>, String> {
    let conn = state.pool.get().map_err(err)?;
    let mut stmt = conn
        .prepare(
            "SELECT id, run_case_id, kind, name, path, size_bytes
             FROM test_artifacts WHERE run_id = ?1 ORDER BY id ASC",
        )
        .map_err(err)?;
    let rows = stmt
        .query_map(params![run_id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, Option<i64>>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, i64>(5)?,
            ))
        })
        .map_err(err)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(err)?;
    Ok(rows
        .into_iter()
        .map(|(id, run_case_id, kind, name, path, size_bytes)| {
            let resolved = artifact_path(&state.data_dir, run_id, &path);
            let relative_path = resolved
                .strip_prefix(state.data_dir.join("test-runs").join(run_id.to_string()))
                .ok()
                .map(|item| item.to_string_lossy().replace('\\', "/"))
                .or_else(|| {
                    Path::new(&path)
                        .is_relative()
                        .then(|| path.replace('\\', "/"))
                });
            ArtifactEntry {
                id,
                run_case_id,
                kind,
                name,
                path: resolved.display().to_string(),
                relative_path,
                size_bytes,
                exists: resolved.is_file(),
            }
        })
        .collect())
}

/// 把附件复制到用户选择的位置（下载）
pub fn export_artifact(
    state: &AppState,
    artifact_id: i64,
    target: &str,
) -> Result<(), String> {
    let conn = state.pool.get().map_err(err)?;
    let (run_id, path): (i64, String) = conn
        .query_row(
            "SELECT run_id, path FROM test_artifacts WHERE id = ?1",
            params![artifact_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|error| match error {
            rusqlite::Error::QueryReturnedNoRows => "附件不存在".to_string(),
            other => err(other),
        })?;
    let source = artifact_path(&state.data_dir, run_id, &path);
    if !source.is_file() {
        return Err(format!("附件文件不存在：{}", source.display()));
    }
    if let Some(parent) = Path::new(target).parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(err)?;
        }
    }
    std::fs::copy(&source, target).map_err(|error| format!("导出附件失败：{error}"))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// 渲染
// ---------------------------------------------------------------------------

fn fmt_time(ms: Option<i64>) -> String {
    let Some(ms) = ms else {
        return "-".to_string();
    };
    match Local.timestamp_millis_opt(ms).single() {
        Some(time) => time.format("%Y-%m-%d %H:%M:%S").to_string(),
        None => ms.to_string(),
    }
}

fn fmt_duration(ms: i64) -> String {
    let ms = ms.max(0);
    if ms < 1000 {
        format!("{ms} ms")
    } else if ms < 60_000 {
        format!("{:.2} s", ms as f64 / 1000.0)
    } else {
        let seconds = ms / 1000;
        format!("{} 分 {} 秒", seconds / 60, seconds % 60)
    }
}

fn status_label(status: &str) -> &'static str {
    match status {
        "passed" => "通过",
        "failed" => "失败",
        "skipped" => "跳过",
        "cancelled" => "已取消",
        "error" => "错误",
        "running" => "运行中",
        "queued" => "排队中",
        _ => "未知",
    }
}

fn clip(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max).collect();
    out.push_str("…（已截断）");
    out
}

fn escape_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

/// XML 1.0 不允许的控制字符直接剔除，避免 JUnit 解析失败
fn escape_xml(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\u{9}' | '\u{a}' | '\u{d}' => out.push(ch),
            ch if (ch as u32) < 0x20 => {}
            _ => out.push(ch),
        }
    }
    out
}

fn snapshot_value(raw: Option<&str>) -> Value {
    match raw.map(str::trim).filter(|text| !text.is_empty()) {
        Some(text) => serde_json::from_str::<Value>(text).unwrap_or_else(|_| Value::String(text.to_string())),
        None => Value::Null,
    }
}

fn assertions_for<'a>(
    detail: &'a RunDetail,
    case: &TestRunCase,
) -> Vec<&'a TestRunAssertion> {
    detail
        .assertions
        .iter()
        .filter(|item| item.run_case_id == case.id)
        .collect()
}

fn case_artifacts<'a>(artifacts: &'a [ArtifactEntry], case: &TestRunCase) -> Vec<&'a ArtifactEntry> {
    artifacts
        .iter()
        .filter(|item| item.run_case_id == Some(case.id))
        .collect()
}

/// 报告页里附件的相对链接（统一用 `/` 分隔）
fn artifact_href(item: &ArtifactEntry) -> String {
    item.relative_path
        .clone()
        .or_else(|| {
            Path::new(&item.path)
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
        })
        .unwrap_or_else(|| item.name.clone())
}

fn run_assertions_total(detail: &RunDetail) -> (i64, i64) {
    let total = detail.cases.iter().map(|case| case.assertions_total).sum();
    let failed = detail.cases.iter().map(|case| case.assertions_failed).sum();
    (total, failed)
}

fn summary_field(run: &TestRun, key: &str) -> Option<String> {
    let raw = run.summary_json.as_deref()?;
    let value: Value = serde_json::from_str(raw).ok()?;
    let value = value.get(key)?;
    match value {
        Value::Null => None,
        Value::String(text) => Some(text.clone()),
        other => Some(other.to_string()),
    }
}

fn python_path(run: &TestRun) -> Option<String> {
    summary_field(run, "pythonPath")
}

fn render_json(
    detail: &RunDetail,
    suite_name: &str,
    artifacts: &[ArtifactEntry],
    stress: &HashMap<i64, StressBrief>,
) -> Value {
    let run = &detail.run;
    let (total_assertions, failed_assertions) = run_assertions_total(detail);
    let cases: Vec<Value> = detail
        .cases
        .iter()
        .map(|case| {
            let assertions: Vec<Value> = assertions_for(detail, case)
                .into_iter()
                .map(|item| {
                    json!({
                        "name": item.name,
                        "passed": item.passed,
                        "message": item.message,
                    })
                })
                .collect();
            let mut python = Map::new();
            if let Some(version) = case.python_version.as_deref().filter(|text| !text.is_empty()) {
                python.insert("version".into(), Value::String(version.to_string()));
            }
            if let Some(hash) = case.script_hash.as_deref().filter(|text| !text.is_empty()) {
                python.insert("scriptHash".into(), Value::String(hash.to_string()));
            }
            json!({
                "id": case.id,
                "caseId": case.case_id,
                "name": case.name,
                "kind": case.kind,
                "status": case.status,
                "attempts": case.attempts,
                "durationMs": case.duration_ms,
                "httpDurationMs": case.http_duration_ms,
                "pythonDurationMs": case.python_duration_ms,
                "request": snapshot_value(case.request_snapshot.as_deref()),
                "response": snapshot_value(case.response_snapshot.as_deref()),
                "assertions": assertions,
                "python": if python.is_empty() { Value::Null } else { Value::Object(python) },
                "error": case.error,
                "logs": case.logs,
                "artifacts": case_artifacts(artifacts, case)
                    .iter()
                    .map(|item| item.name.clone())
                    .collect::<Vec<_>>(),
                "stressReportId": case.report_id,
                "stress": case.report_id.and_then(|id| stress.get(&id)).map(stress_json),
            })
        })
        .collect();

    let artifacts_json: Vec<Value> = artifacts
        .iter()
        .map(|item| {
            json!({
                "id": item.id,
                "runCaseId": item.run_case_id,
                "kind": item.kind,
                "name": item.name,
                "path": item.path,
                "sizeBytes": item.size_bytes,
                "exists": item.exists,
            })
        })
        .collect();

    json!({
        "schemaVersion": REPORT_SCHEMA_VERSION,
        "generatedAt": now_ms(),
        "run": {
            "id": run.id,
            "suiteId": run.suite_id,
            "suite": suite_name,
            "env": run.env_name,
            "trigger": run.trigger,
            "status": run.status,
            "startedAt": run.started_at,
            "finishedAt": run.finished_at,
            "durationMs": run.duration_ms,
            "total": run.total,
            "passed": run.passed,
            "failed": run.failed,
            "skipped": run.skipped,
            "errored": run.errored,
            "assertionsTotal": total_assertions,
            "assertionsFailed": failed_assertions,
        },
        "environment": {
            "appVersion": run.app_version,
            "pythonVersion": run.python_version,
            "pythonPath": python_path(run),
            "summary": run.summary_json.as_deref().and_then(|text| serde_json::from_str::<Value>(text).ok()),
        },
        "cases": cases,
        "stress": stress.values().map(stress_json).collect::<Vec<_>>(),
        "artifacts": artifacts_json,
    })
}

fn stress_json(item: &StressBrief) -> Value {
    json!({
        "reportId": item.id,
        "plan": item.plan_name,
        "status": item.status,
        "totalRequests": item.total,
        "success": item.success,
        "errors": item.errors,
        "p95Ms": item.p95,
        "p99Ms": item.p99,
        "avgRps": item.avg_qps,
        "errorRate": item.error_rate,
        "concurrency": item.concurrency,
    })
}

fn render_summary(detail: &RunDetail, suite_name: &str) -> String {
    let run = &detail.run;
    let (total_assertions, failed_assertions) = run_assertions_total(detail);
    let mut out = String::new();
    let _ = writeln!(out, "AstroMan 测试运行 #{}", run.id);
    let _ = writeln!(out, "套件：{suite_name}");
    let _ = writeln!(out, "环境：{}", run.env_name.as_deref().unwrap_or("-"));
    let _ = writeln!(out, "触发：{}", run.trigger);
    let _ = writeln!(
        out,
        "状态：{}（{}）",
        run.status,
        status_label(&run.status)
    );
    let _ = writeln!(
        out,
        "时间：{} ~ {}",
        fmt_time(run.started_at),
        fmt_time(run.finished_at)
    );
    let _ = writeln!(out, "耗时：{}", fmt_duration(run.duration_ms));
    let _ = writeln!(
        out,
        "用例：共 {}，通过 {}，失败 {}，跳过 {}，错误 {}",
        run.total, run.passed, run.failed, run.skipped, run.errored
    );
    let _ = writeln!(
        out,
        "断言：共 {total_assertions}，失败 {failed_assertions}"
    );
    if let Some(version) = run.python_version.as_deref() {
        let _ = writeln!(out, "Python：{version}");
    }
    if let Some(path) = python_path(run) {
        let _ = writeln!(out, "解释器：{path}");
    }
    let failures: Vec<&TestRunCase> = detail
        .cases
        .iter()
        .filter(|case| matches!(case.status.as_str(), "failed" | "error"))
        .collect();
    if failures.is_empty() {
        let _ = writeln!(out, "失败用例：无");
    } else {
        let _ = writeln!(out, "失败用例：");
        for case in failures {
            let _ = writeln!(
                out,
                "  - [{}] {}：{}",
                case.status,
                case.name,
                clip(case.error.as_deref().unwrap_or(""), 300)
            );
        }
    }
    out
}

fn render_junit(detail: &RunDetail, suite_name: &str) -> String {
    let run = &detail.run;
    let failures = run.failed.max(0);
    let errors = run.errored.max(0);
    let skipped = run.skipped.max(0);
    let time = run.duration_ms.max(0) as f64 / 1000.0;
    let mut out = String::with_capacity(8192);
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    let _ = writeln!(
        out,
        "<testsuites name=\"{}\" tests=\"{}\" failures=\"{failures}\" errors=\"{errors}\" skipped=\"{skipped}\" time=\"{time:.3}\">",
        escape_xml(suite_name),
        run.total
    );
    let _ = writeln!(
        out,
        "  <testsuite name=\"{}\" tests=\"{}\" failures=\"{failures}\" errors=\"{errors}\" skipped=\"{skipped}\" time=\"{time:.3}\">",
        escape_xml(suite_name),
        run.total
    );
    out.push_str("    <properties>\n");
    let _ = writeln!(out, "      <property name=\"runId\" value=\"{}\"/>", run.id);
    let _ = writeln!(
        out,
        "      <property name=\"env\" value=\"{}\"/>",
        escape_xml(run.env_name.as_deref().unwrap_or(""))
    );
    let _ = writeln!(
        out,
        "      <property name=\"trigger\" value=\"{}\"/>",
        escape_xml(&run.trigger)
    );
    if let Some(version) = run.app_version.as_deref() {
        let _ = writeln!(
            out,
            "      <property name=\"appVersion\" value=\"{}\"/>",
            escape_xml(version)
        );
    }
    if let Some(version) = run.python_version.as_deref() {
        let _ = writeln!(
            out,
            "      <property name=\"python\" value=\"{}\"/>",
            escape_xml(version)
        );
    }
    if let Some(path) = python_path(run) {
        let _ = writeln!(
            out,
            "      <property name=\"pythonPath\" value=\"{}\"/>",
            escape_xml(&path)
        );
    }
    out.push_str("    </properties>\n");

    for case in &detail.cases {
        let case_time = case.duration_ms.max(0) as f64 / 1000.0;
        let _ = writeln!(
            out,
            "    <testcase classname=\"{}\" name=\"{}\" time=\"{case_time:.3}\">",
            escape_xml(suite_name),
            escape_xml(&case.name)
        );
        match case.status.as_str() {
            "passed" => {}
            "skipped" => {
                let message = case.error.as_deref().unwrap_or("已跳过");
                let _ = writeln!(
                    out,
                    "      <skipped message=\"{}\"/>",
                    escape_xml(message)
                );
            }
            "error" => {
                let message = case.error.as_deref().unwrap_or("执行错误");
                let _ = writeln!(
                    out,
                    "      <error message=\"{}\" type=\"RuntimeError\">{}</error>",
                    escape_xml(&clip(message, 200)),
                    escape_xml(message)
                );
            }
            _ => {
                let failed: Vec<&TestRunAssertion> = assertions_for(detail, case)
                    .into_iter()
                    .filter(|item| !item.passed)
                    .collect();
                let message = failed
                    .first()
                    .map(|item| item.name.clone())
                    .or_else(|| case.error.clone())
                    .unwrap_or_else(|| "断言未通过".to_string());
                let detail_text = if failed.is_empty() {
                    case.error.clone().unwrap_or_default()
                } else {
                    failed
                        .iter()
                        .map(|item| {
                            format!(
                                "{}：{}",
                                item.name,
                                item.message.clone().unwrap_or_default()
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                };
                let _ = writeln!(
                    out,
                    "      <failure message=\"{}\" type=\"AssertionError\">{}</failure>",
                    escape_xml(&clip(&message, 200)),
                    escape_xml(&detail_text)
                );
            }
        }
        if let Some(logs) = case.logs.as_deref().filter(|text| !text.trim().is_empty()) {
            let _ = writeln!(
                out,
                "      <system-out>{}</system-out>",
                escape_xml(logs)
            );
        }
        out.push_str("    </testcase>\n");
    }
    out.push_str("  </testsuite>\n</testsuites>\n");
    out
}

const HTML_CSS: &str = r#"
:root { color-scheme: light; }
* { box-sizing: border-box; }
body { margin: 0; background: #f4f5f7; color: #1f2329; font: 14px/1.6 -apple-system, "Segoe UI", "PingFang SC", "Microsoft YaHei", sans-serif; }
.wrap { max-width: 1180px; margin: 0 auto; padding: 24px; }
.hero { background: #fff; border-radius: 8px; padding: 20px 24px; display: flex; justify-content: space-between; align-items: flex-start; gap: 16px; box-shadow: 0 1px 3px rgba(0,0,0,.06); }
.hero h1 { margin: 0 0 6px; font-size: 22px; }
.hero p { margin: 0; color: #646a73; }
.badge { padding: 4px 14px; border-radius: 999px; font-weight: 600; font-size: 13px; white-space: nowrap; }
.st-passed { background: #e8f7ee; color: #1a7f37; }
.st-failed { background: #fdecec; color: #c62828; }
.st-error { background: #fff4e5; color: #b26a00; }
.st-skipped { background: #eef0f3; color: #646a73; }
.st-cancelled { background: #eef0f3; color: #646a73; }
.cards { display: grid; grid-template-columns: repeat(auto-fit, minmax(150px, 1fr)); gap: 12px; margin-top: 16px; }
.card { background: #fff; border-radius: 8px; padding: 14px 16px; box-shadow: 0 1px 3px rgba(0,0,0,.06); }
.card .label { color: #646a73; font-size: 12px; }
.card .value { font-size: 24px; font-weight: 600; margin-top: 2px; }
.card .value.fail { color: #c62828; }
.card .value.err { color: #b26a00; }
.card .value.ok { color: #1a7f37; }
.meta { background: #fff; border-radius: 8px; padding: 14px 18px; margin-top: 16px; display: grid; grid-template-columns: repeat(auto-fit, minmax(220px, 1fr)); gap: 8px 24px; box-shadow: 0 1px 3px rgba(0,0,0,.06); }
.meta div { color: #646a73; word-break: break-all; }
.meta b { color: #1f2329; font-weight: 600; }
section.block { margin-top: 22px; }
section.block > h2 { font-size: 16px; margin: 0 0 10px; }
.fail-card { background: #fff; border-left: 3px solid #c62828; border-radius: 6px; padding: 14px 16px; margin-bottom: 10px; box-shadow: 0 1px 3px rgba(0,0,0,.06); }
.fail-card h3 { margin: 0 0 6px; font-size: 15px; }
.fail-card .err { margin: 0 0 8px; color: #c62828; white-space: pre-wrap; word-break: break-word; }
.filters { display: flex; gap: 8px; align-items: center; margin-bottom: 10px; flex-wrap: wrap; }
.filters button { border: 1px solid #d9dce0; background: #fff; border-radius: 6px; padding: 4px 12px; cursor: pointer; font-size: 13px; }
.filters button.active { border-color: #1668dc; color: #1668dc; background: #eef4ff; }
.filters input { flex: 1; min-width: 180px; border: 1px solid #d9dce0; border-radius: 6px; padding: 5px 10px; font-size: 13px; outline: none; }
details.case { background: #fff; border-radius: 6px; margin-bottom: 8px; box-shadow: 0 1px 3px rgba(0,0,0,.05); }
details.case > summary { list-style: none; display: flex; gap: 12px; align-items: center; padding: 10px 14px; cursor: pointer; }
details.case > summary::-webkit-details-marker { display: none; }
details.case[open] > summary { border-bottom: 1px solid #eef0f3; }
.pill { padding: 1px 10px; border-radius: 999px; font-size: 12px; font-weight: 600; }
.case .name { font-weight: 600; flex: 1; word-break: break-word; }
.case .kind, .case .dur, .case .asserts { color: #646a73; font-size: 12px; white-space: nowrap; }
.case .body { padding: 12px 16px 16px; }
.case h4 { margin: 14px 0 6px; font-size: 13px; color: #646a73; }
.stress-summary { margin-bottom: 6px; padding: 8px 10px; border-radius: 6px; background: #f7f8fa; border: 1px solid #eef0f3; font-size: 12px; line-height: 1.7; }
pre { background: #f7f8fa; border: 1px solid #eef0f3; border-radius: 6px; padding: 10px 12px; margin: 0; overflow: auto; max-height: 360px; font-size: 12px; line-height: 1.5; white-space: pre-wrap; word-break: break-word; }
table { border-collapse: collapse; width: 100%; font-size: 13px; }
th, td { border-bottom: 1px solid #eef0f3; padding: 6px 8px; text-align: left; vertical-align: top; }
th { color: #646a73; font-weight: 500; }
td.pass { color: #1a7f37; }
td.fail { color: #c62828; }
ul.artifacts { margin: 0; padding-left: 18px; }
ul.artifacts a { color: #1668dc; text-decoration: none; }
footer { margin: 26px 0 10px; color: #8f959e; font-size: 12px; text-align: center; }
"#;

const HTML_SCRIPT: &str = r#"
(function () {
  var buttons = document.querySelectorAll('[data-filter]');
  var list = document.getElementById('case-list');
  var query = document.getElementById('case-query');
  var cases = Array.prototype.slice.call(list.querySelectorAll('details.case'));
  var current = 'all';
  function apply() {
    var text = (query.value || '').toLowerCase();
    cases.forEach(function (item) {
      var statusOk = current === 'all' || item.getAttribute('data-status') === current;
      var textOk = !text || item.getAttribute('data-name').indexOf(text) >= 0;
      item.style.display = statusOk && textOk ? '' : 'none';
    });
  }
  buttons.forEach(function (button) {
    button.addEventListener('click', function () {
      buttons.forEach(function (item) { item.classList.remove('active'); });
      button.classList.add('active');
      current = button.getAttribute('data-filter');
      apply();
    });
  });
  query.addEventListener('input', apply);
})();
"#;

fn render_html(
    detail: &RunDetail,
    suite_name: &str,
    artifacts: &[ArtifactEntry],
    stress: &HashMap<i64, StressBrief>,
) -> String {
    let run = &detail.run;
    let (total_assertions, failed_assertions) = run_assertions_total(detail);
    let pass_rate = if run.total > 0 {
        run.passed as f64 * 100.0 / run.total as f64
    } else {
        0.0
    };
    let mut out = String::with_capacity(64 * 1024 + detail.cases.len() * 1024);
    out.push_str("<!doctype html>\n<html lang=\"zh-CN\">\n<head>\n<meta charset=\"utf-8\">\n");
    out.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    let _ = writeln!(
        out,
        "<title>测试报告 · {} · #{}</title>",
        escape_html(suite_name),
        run.id
    );
    out.push_str("<style>");
    out.push_str(HTML_CSS);
    out.push_str("</style>\n</head>\n<body>\n<div class=\"wrap\">\n");

    // 概览
    out.push_str("<header class=\"hero\">\n<div>\n");
    let _ = writeln!(out, "<h1>{}</h1>", escape_html(suite_name));
    let _ = writeln!(
        out,
        "<p>运行 #{} · 环境 {} · 触发 {} · {} ~ {}</p>",
        run.id,
        escape_html(run.env_name.as_deref().unwrap_or("-")),
        escape_html(&run.trigger),
        fmt_time(run.started_at),
        fmt_time(run.finished_at)
    );
    out.push_str("</div>\n");
    let _ = writeln!(
        out,
        "<span class=\"badge st-{}\">{} · {}</span>",
        escape_html(&run.status),
        escape_html(status_label(&run.status)),
        fmt_duration(run.duration_ms)
    );
    out.push_str("</header>\n");

    // 指标卡
    out.push_str("<section class=\"cards\">\n");
    let cards = [
        ("总用例", run.total.to_string(), ""),
        ("通过", run.passed.to_string(), "ok"),
        ("失败", run.failed.to_string(), "fail"),
        ("跳过", run.skipped.to_string(), ""),
        ("错误", run.errored.to_string(), "err"),
        ("通过率", format!("{pass_rate:.1}%"), if run.failed + run.errored > 0 { "fail" } else { "ok" }),
        (
            "断言",
            format!("{}/{}", total_assertions - failed_assertions, total_assertions),
            if failed_assertions > 0 { "fail" } else { "" },
        ),
        ("总耗时", fmt_duration(run.duration_ms), ""),
    ];
    for (label, value, class) in cards {
        let _ = writeln!(
            out,
            "<div class=\"card\"><div class=\"label\">{label}</div><div class=\"value {class}\">{}</div></div>",
            escape_html(&value)
        );
    }
    out.push_str("</section>\n");

    // 环境信息
    out.push_str("<section class=\"meta\">\n");
    let _ = writeln!(out, "<div>运行 ID：<b>{}</b></div>", run.id);
    let _ = writeln!(
        out,
        "<div>应用版本：<b>{}</b></div>",
        escape_html(run.app_version.as_deref().unwrap_or("-"))
    );
    let _ = writeln!(
        out,
        "<div>Python：<b>{}</b></div>",
        escape_html(run.python_version.as_deref().unwrap_or("未使用"))
    );
    let _ = writeln!(
        out,
        "<div>解释器：<b>{}</b></div>",
        escape_html(python_path(run).as_deref().unwrap_or("-"))
    );
    let _ = writeln!(
        out,
        "<div>开始：<b>{}</b></div>",
        fmt_time(run.started_at)
    );
    let _ = writeln!(
        out,
        "<div>结束：<b>{}</b></div>",
        fmt_time(run.finished_at)
    );
    out.push_str("</section>\n");

    // 失败聚焦
    let failures: Vec<&TestRunCase> = detail
        .cases
        .iter()
        .filter(|case| matches!(case.status.as_str(), "failed" | "error"))
        .collect();
    if !failures.is_empty() {
        let _ = writeln!(
            out,
            "<section class=\"block\"><h2>失败聚焦（{}）</h2>",
            failures.len()
        );
        for case in failures {
            out.push_str("<div class=\"fail-card\">\n");
            let _ = writeln!(
                out,
                "<h3><span class=\"pill st-{}\">{}</span> {}</h3>",
                escape_html(&case.status),
                escape_html(status_label(&case.status)),
                escape_html(&case.name)
            );
            if let Some(error) = case.error.as_deref().filter(|text| !text.trim().is_empty()) {
                let _ = writeln!(out, "<p class=\"err\">{}</p>", escape_html(&clip(error, 1200)));
            }
            let failed: Vec<&TestRunAssertion> = assertions_for(detail, case)
                .into_iter()
                .filter(|item| !item.passed)
                .collect();
            if !failed.is_empty() {
                out.push_str("<table><thead><tr><th>断言</th><th>实际</th></tr></thead><tbody>\n");
                for item in failed {
                    let _ = writeln!(
                        out,
                        "<tr><td>{}</td><td class=\"fail\">{}</td></tr>",
                        escape_html(&item.name),
                        escape_html(item.message.as_deref().unwrap_or(""))
                    );
                }
                out.push_str("</tbody></table>\n");
            }
            if let Some(response) = case.response_snapshot.as_deref() {
                let _ = writeln!(
                    out,
                    "<details><summary>响应片段</summary><pre>{}</pre></details>",
                    escape_html(&clip(response, 2000))
                );
            }
            out.push_str("</div>\n");
        }
        out.push_str("</section>\n");
    }

    // 用例列表
    let _ = writeln!(
        out,
        "<section class=\"block\"><h2>用例（{}）</h2>",
        detail.cases.len()
    );
    out.push_str("<div class=\"filters\">\n");
    out.push_str("<button data-filter=\"all\" class=\"active\">全部</button>\n");
    for status in ["passed", "failed", "error", "skipped"] {
        let count = detail.cases.iter().filter(|case| case.status == status).count();
        if count == 0 {
            continue;
        }
        let _ = writeln!(
            out,
            "<button data-filter=\"{status}\">{} {count}</button>",
            status_label(status)
        );
    }
    out.push_str("<input id=\"case-query\" placeholder=\"按用例名搜索\">\n");
    out.push_str("</div>\n<div id=\"case-list\">\n");

    for case in &detail.cases {
        let name_attr = escape_html(&case.name.to_ascii_lowercase());
        let _ = writeln!(
            out,
            "<details class=\"case\" data-status=\"{}\" data-name=\"{name_attr}\">",
            escape_html(&case.status)
        );
        let _ = writeln!(
            out,
            "<summary><span class=\"pill st-{}\">{}</span><span class=\"name\">{}</span><span class=\"kind\">{}</span><span class=\"asserts\">断言 {}/{}</span><span class=\"dur\">{}</span></summary>",
            escape_html(&case.status),
            escape_html(status_label(&case.status)),
            escape_html(&case.name),
            escape_html(&case.kind),
            case.assertions_total - case.assertions_failed,
            case.assertions_total,
            fmt_duration(case.duration_ms)
        );
        out.push_str("<div class=\"body\">\n");
        let _ = writeln!(
            out,
            "<div class=\"meta\"><div>尝试：<b>{}</b></div><div>总耗时：<b>{}</b></div><div>HTTP：<b>{}</b></div><div>Python：<b>{}</b></div></div>",
            case.attempts,
            fmt_duration(case.duration_ms),
            fmt_duration(case.http_duration_ms),
            fmt_duration(case.python_duration_ms)
        );
        if let Some(error) = case.error.as_deref().filter(|text| !text.trim().is_empty()) {
            let _ = writeln!(
                out,
                "<h4>错误</h4><pre style=\"border-color:#f3c1c1\">{}</pre>",
                escape_html(&clip(error, 4000))
            );
        }
        let case_assertions = assertions_for(detail, case);
        if !case_assertions.is_empty() {
            out.push_str("<h4>断言</h4><table><thead><tr><th>名称</th><th>结果</th><th>说明</th></tr></thead><tbody>\n");
            for item in case_assertions {
                let _ = writeln!(
                    out,
                    "<tr><td>{}</td><td class=\"{}\">{}</td><td>{}</td></tr>",
                    escape_html(&item.name),
                    if item.passed { "pass" } else { "fail" },
                    if item.passed { "通过" } else { "失败" },
                    escape_html(item.message.as_deref().unwrap_or(""))
                );
            }
            out.push_str("</tbody></table>\n");
        }
        if let Some(request) = case.request_snapshot.as_deref().filter(|text| !text.trim().is_empty()) {
            let _ = writeln!(
                out,
                "<h4>请求</h4><pre>{}</pre>",
                escape_html(&clip(request, 4000))
            );
        }
        if let Some(response) = case.response_snapshot.as_deref().filter(|text| !text.trim().is_empty()) {
            let _ = writeln!(
                out,
                "<h4>响应</h4><pre>{}</pre>",
                escape_html(&clip(response, 6000))
            );
        }
        if let Some(logs) = case.logs.as_deref().filter(|text| !text.trim().is_empty()) {
            let _ = writeln!(
                out,
                "<h4>日志</h4><pre>{}</pre>",
                escape_html(&clip(logs, 8000))
            );
        }
        let case_files = case_artifacts(artifacts, case);
        if !case_files.is_empty() {
            out.push_str("<h4>附件</h4><ul class=\"artifacts\">\n");
            for item in case_files {
                let _ = writeln!(
                    out,
                    "<li><a href=\"files/{}\" target=\"_blank\">{}</a>（{} 字节）</li>",
                    escape_html(&artifact_href(item)),
                    escape_html(&item.name),
                    item.size_bytes
                );
            }
            out.push_str("</ul>\n");
        }
        if let Some(version) = case.python_version.as_deref().filter(|text| !text.is_empty()) {
            let _ = writeln!(
                out,
                "<h4>Python</h4><div>版本 {} · 脚本 {}</div>",
                escape_html(version),
                escape_html(case.script_hash.as_deref().unwrap_or("-"))
            );
        }
        if let Some(report_id) = case.report_id {
            let _ = write!(out, "<h4>压测报告</h4>");
            if let Some(item) = stress.get(&report_id) {
                let _ = write!(
                    out,
                    "<div class=\"stress-summary\">报告 #{} · {} · 请求 {}（成功 {}，错误 {}）· 并发 {} · 平均 RPS {:.1} · P95 {:.0}ms · P99 {:.0}ms · 错误率 {:.2}%</div>",
                    item.id,
                    escape_html(&item.status),
                    item.total,
                    item.success,
                    item.errors,
                    item.concurrency,
                    item.avg_qps,
                    item.p95,
                    item.p99,
                    item.error_rate
                );
            }
            let _ = write!(
                out,
                "<div>压测报告 #{report_id}，可在报告页查看详情</div>"
            );
        }
        out.push_str("</div>\n</details>\n");
    }
    out.push_str("</div></section>\n");

    // 产物
    if !artifacts.is_empty() {
        let _ = writeln!(
            out,
            "<section class=\"block\"><h2>附件与产物（{}）</h2><ul class=\"artifacts\">",
            artifacts.len()
        );
        for item in artifacts {
            let _ = writeln!(
                out,
                "<li><a href=\"files/{}\" target=\"_blank\">{}</a>{}</li>",
                escape_html(&artifact_href(item)),
                escape_html(&item.name),
                if item.exists { String::new() } else { "（原文件已丢失）".to_string() }
            );
        }
        out.push_str("</ul></section>\n");
    }

    let _ = writeln!(
        out,
        "<footer>AstroMan · 报告生成于 {} · 报告格式 v{}</footer>",
        fmt_time(Some(now_ms())),
        REPORT_SCHEMA_VERSION
    );
    out.push_str("</div>\n<script>");
    out.push_str(HTML_SCRIPT);
    out.push_str("</script>\n</body>\n</html>\n");
    out
}

// ---------------------------------------------------------------------------
// 保留与清理
// ---------------------------------------------------------------------------

/// 每个套件只保留最近 N 份报告，超限删除目录与附件索引（文档 7.6）
pub fn prune_reports(
    conn: &rusqlite::Connection,
    data_dir: &Path,
    suite_id: i64,
    keep: usize,
) -> Result<usize, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, report_dir FROM test_runs
             WHERE suite_id = ?1 AND report_dir IS NOT NULL
             ORDER BY id DESC LIMIT -1 OFFSET ?2",
        )
        .map_err(err)?;
    let stale: Vec<(i64, String)> = stmt
        .query_map(params![suite_id, keep as i64], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .map_err(err)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(err)?;
    if stale.is_empty() {
        return Ok(0);
    }
    for (run_id, report_dir) in &stale {
        let dir = if Path::new(report_dir).is_absolute() {
            PathBuf::from(report_dir)
        } else {
            data_dir.join(report_dir)
        };
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(
            data_dir.join("test-runs").join(run_id.to_string()),
        );
        conn.execute(
            "UPDATE test_runs SET report_dir = NULL WHERE id = ?1",
            params![run_id],
        )
        .map_err(err)?;
        conn.execute(
            "DELETE FROM test_artifacts WHERE run_id = ?1",
            params![run_id],
        )
        .map_err(err)?;
    }
    Ok(stale.len())
}

/// 启动时清理无主目录：库里不存在的 runId 目录（崩溃残留）
pub fn cleanup_orphans(conn: &rusqlite::Connection, data_dir: &Path) {
    for folder in ["test-reports", "test-runs"] {
        let root = data_dir.join(folder);
        let Ok(entries) = std::fs::read_dir(&root) else {
            continue;
        };
        for entry in entries.flatten() {
            if !entry.path().is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            let Ok(run_id) = name.parse::<i64>() else {
                continue;
            };
            let exists: bool = conn
                .query_row(
                    "SELECT COUNT(*) FROM test_runs WHERE id = ?1",
                    params![run_id],
                    |row| row.get::<_, i64>(0).map(|count| count > 0),
                )
                .unwrap_or(true);
            if !exists {
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
    }
}

#[cfg(test)]
#[path = "report_builder_tests.rs"]
mod tests;
