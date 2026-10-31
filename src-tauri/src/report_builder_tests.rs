//! report_builder 的测试：直接写库造数，不依赖网络与 Python

use super::*;
use rusqlite::params;
use std::path::PathBuf;

fn temp_state(tag: &str) -> (PathBuf, AppState) {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let root = std::env::temp_dir().join(format!(
        "astroman-report-{tag}-{}-{stamp}",
        std::process::id()
    ));
    let state = AppState::headless(root.join("data"), root.join("config")).unwrap();
    (root, state)
}

fn seed_suite(state: &AppState, name: &str) -> i64 {
    let conn = state.pool.get().unwrap();
    let now = now_ms();
    conn.execute(
        "INSERT INTO projects (name, created_at, updated_at) VALUES ('p', ?1, ?1)",
        params![now],
    )
    .unwrap();
    let project_id = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO test_suites (project_id, name, config_json, created_at, updated_at)
         VALUES (?1, ?2, '{}', ?3, ?3)",
        params![project_id, name, now],
    )
    .unwrap();
    conn.last_insert_rowid()
}

struct SeedCase<'a> {
    name: &'a str,
    status: &'a str,
    duration_ms: i64,
    error: Option<&'a str>,
    logs: Option<&'a str>,
}

/// 直接写库造一次已结束的运行，返回 runId
fn seed_run(state: &AppState, suite_id: i64, run_status: &str, cases: &[SeedCase<'_>]) -> i64 {
    let conn = state.pool.get().unwrap();
    let now = now_ms();
    let passed = cases.iter().filter(|item| item.status == "passed").count() as i64;
    let failed = cases.iter().filter(|item| item.status == "failed").count() as i64;
    let skipped = cases.iter().filter(|item| item.status == "skipped").count() as i64;
    let errored = cases.iter().filter(|item| item.status == "error").count() as i64;
    conn.execute(
        "INSERT INTO test_runs
            (suite_id, env_name, trigger, status, total, passed, failed, skipped, errored,
             duration_ms, summary_json, app_version, python_version, started_at, finished_at, created_at)
         VALUES (?1, 'dev', 'ui', ?2, ?3, ?4, ?5, ?6, ?7, 1234,
                 '{\"pythonPath\":\"python3.11\"}', '1.0.8', '3.11.4', ?8, ?9, ?8)",
        params![
            suite_id,
            run_status,
            cases.len() as i64,
            passed,
            failed,
            skipped,
            errored,
            now,
            now + 1234
        ],
    )
    .unwrap();
    let run_id = conn.last_insert_rowid();
    for item in cases {
        conn.execute(
            "INSERT INTO test_run_cases
                (run_id, name, kind, status, attempts, duration_ms, http_duration_ms,
                 python_duration_ms, request_snapshot, response_snapshot, assertions_total,
                 assertions_failed, error, logs, python_version, script_hash, started_at, finished_at)
             VALUES (?1, ?2, 'request', ?3, 1, ?4, ?5, 0,
                     '{\"method\":\"GET\",\"url\":\"http://127.0.0.1/x\"}',
                     '{\"status\":200}', 2, ?6, ?7, ?8, ?9, ?10, ?11, ?11)",
            params![
                run_id,
                item.name,
                item.status,
                item.duration_ms,
                item.duration_ms / 2,
                if item.status == "failed" { 1 } else { 0 },
                item.error,
                item.logs,
                item.logs.map(|_| "3.11.4"),
                item.logs.map(|_| "sha256:abc"),
                now
            ],
        )
        .unwrap();
        let run_case_id = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO test_run_assertions (run_case_id, name, passed, message, duration_ms)
             VALUES (?1, '状态码', 1, NULL, 3)",
            params![run_case_id],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO test_run_assertions (run_case_id, name, passed, message, duration_ms)
             VALUES (?1, 'orderId', ?2, ?3, 2)",
            params![
                run_case_id,
                item.status != "failed",
                if item.status != "failed" {
                    None
                } else {
                    Some("期望 42，实际 7")
                }
            ],
        )
        .unwrap();
    }
    run_id
}

fn write_artifact(state: &AppState, run_id: i64, relative: &str, data: &[u8]) -> i64 {
    let run_root = state.data_dir.join("test-runs").join(run_id.to_string());
    let file = run_root.join(relative);
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, data).unwrap();
    let conn = state.pool.get().unwrap();
    conn.execute(
        "INSERT INTO test_artifacts (run_id, run_case_id, kind, name, path, size_bytes, created_at)
         VALUES (?1, NULL, 'attachment', ?2, ?3, ?4, ?5)",
        params![run_id, relative, relative, data.len() as i64, now_ms()],
    )
    .unwrap();
    conn.last_insert_rowid()
}

#[test]
fn generates_all_formats_with_failure_focus_and_attachment_links() {
    let (root, state) = temp_state("all");
    let suite_id = seed_suite(&state, "订单套件");
    let run_id = seed_run(
        &state,
        suite_id,
        "failed",
        &[
            SeedCase {
                name: "下单成功",
                status: "passed",
                duration_ms: 80,
                error: None,
                logs: Some("ok"),
            },
            SeedCase {
                name: "取消订单",
                status: "failed",
                duration_ms: 120,
                error: Some("断言失败：orderId"),
                logs: Some("boom <tag>"),
            },
            SeedCase {
                name: "退款",
                status: "skipped",
                duration_ms: 0,
                error: Some("DEPENDENCY_FAILED"),
                logs: None,
            },
        ],
    );
    write_artifact(&state, run_id, "shots/a.png", b"png-bytes");

    let report = generate(&state, run_id, &[], None).unwrap();
    assert_eq!(report.run_id, run_id);
    let dir = PathBuf::from(&report.dir);
    assert!(dir.join("report.html").is_file());
    assert!(dir.join("report.json").is_file());
    assert!(dir.join("junit.xml").is_file());
    assert!(dir.join("summary.txt").is_file());
    assert!(dir.join("files/shots/a.png").is_file());
    assert_eq!(
        std::fs::read(dir.join("files/shots/a.png")).unwrap(),
        b"png-bytes"
    );
    assert!(report.files.iter().any(|item| item == "files/shots/a.png"));
    assert_eq!(
        report.relative_dir.as_deref(),
        Some(format!("test-reports/{run_id}").as_str())
    );

    let html = std::fs::read_to_string(dir.join("report.html")).unwrap();
    assert!(html.contains("失败聚焦"));
    assert!(html.contains("取消订单"));
    assert!(html.contains("断言失败：orderId"));
    assert!(html.contains("href=\"files/shots/a.png\""));
    // 日志里的 HTML 片段必须被转义
    assert!(html.contains("boom &lt;tag&gt;"));

    let json: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("report.json")).unwrap()).unwrap();
    assert_eq!(json["schemaVersion"], REPORT_SCHEMA_VERSION);
    assert_eq!(json["run"]["status"], "failed");
    assert_eq!(json["run"]["assertionsTotal"], 6);
    assert_eq!(json["run"]["assertionsFailed"], 1);
    assert_eq!(json["environment"]["pythonVersion"], "3.11.4");
    assert_eq!(json["environment"]["pythonPath"], "python3.11");
    assert_eq!(json["cases"][1]["status"], "failed");
    assert_eq!(json["cases"][1]["request"]["method"], "GET");
    assert_eq!(json["cases"][1]["artifacts"].as_array().unwrap().len(), 0);
    assert_eq!(json["artifacts"][0]["name"], "shots/a.png");

    let junit = std::fs::read_to_string(dir.join("junit.xml")).unwrap();
    assert!(junit.contains("tests=\"3\""));
    assert!(junit.contains("<failure message=\"orderId\" type=\"AssertionError\">"));
    assert!(junit.contains("<skipped message=\"DEPENDENCY_FAILED\"/>"));
    assert!(junit.contains("<property name=\"pythonPath\" value=\"python3.11\"/>"));
    assert!(!junit.contains("<tag>"));

    let summary = std::fs::read_to_string(dir.join("summary.txt")).unwrap();
    assert!(summary.contains("AstroMan 测试运行"));
    assert!(summary.contains("取消订单"));

    let conn = state.pool.get().unwrap();
    let stored: Option<String> = conn
        .query_row(
            "SELECT report_dir FROM test_runs WHERE id = ?1",
            params![run_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        stored.as_deref(),
        Some(format!("test-reports/{run_id}").as_str())
    );
    drop(conn);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn supports_partial_formats_and_rejects_unknown() {
    let (root, state) = temp_state("partial");
    let suite_id = seed_suite(&state, "s");
    let run_id = seed_run(
        &state,
        suite_id,
        "passed",
        &[SeedCase {
            name: "a",
            status: "passed",
            duration_ms: 10,
            error: None,
            logs: None,
        }],
    );
    let report = generate(&state, run_id, &["JUNIT".to_string()], None).unwrap();
    let dir = PathBuf::from(&report.dir);
    assert!(dir.join("junit.xml").is_file());
    assert!(!dir.join("report.html").exists());

    let error = generate(&state, run_id, &["pdf".to_string()], None).unwrap_err();
    assert!(error.contains("不支持的报告格式"));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn export_artifact_copies_file_and_reports_missing() {
    let (root, state) = temp_state("export");
    let suite_id = seed_suite(&state, "s");
    let run_id = seed_run(
        &state,
        suite_id,
        "passed",
        &[SeedCase {
            name: "a",
            status: "passed",
            duration_ms: 10,
            error: None,
            logs: None,
        }],
    );
    let artifact_id = write_artifact(&state, run_id, "out/report.csv", b"a,b\n1,2\n");

    let target = root.join("downloads/report.csv");
    export_artifact(&state, artifact_id, target.to_str().unwrap()).unwrap();
    assert_eq!(std::fs::read(&target).unwrap(), b"a,b\n1,2\n");

    // 原文件丢失时给出明确错误
    std::fs::remove_file(
        state
            .data_dir
            .join("test-runs")
            .join(run_id.to_string())
            .join("out/report.csv"),
    )
    .unwrap();
    let error = export_artifact(&state, artifact_id, target.to_str().unwrap()).unwrap_err();
    assert!(error.contains("附件文件不存在"));

    let entries = list_artifacts(&state, run_id).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].relative_path.as_deref(), Some("out/report.csv"));
    assert!(!entries[0].exists);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn prune_keeps_recent_reports_and_removes_old_files() {
    let (root, state) = temp_state("prune");
    let suite_id = seed_suite(&state, "s");
    let mut run_ids = Vec::new();
    for index in 0..3 {
        let run_id = seed_run(
            &state,
            suite_id,
            "passed",
            &[SeedCase {
                name: "a",
                status: "passed",
                duration_ms: 10,
                error: None,
                logs: None,
            }],
        );
        write_artifact(&state, run_id, &format!("f{index}.txt"), b"x");
        generate(&state, run_id, &["json".to_string()], None).unwrap();
        run_ids.push(run_id);
    }
    let conn = state.pool.get().unwrap();
    let removed = prune_reports(&conn, &state.data_dir, suite_id, 1).unwrap();
    assert_eq!(removed, 2);
    assert!(state
        .data_dir
        .join("test-reports")
        .join(run_ids[2].to_string())
        .join("report.json")
        .is_file());
    assert!(!state
        .data_dir
        .join("test-reports")
        .join(run_ids[0].to_string())
        .exists());
    assert!(!state
        .data_dir
        .join("test-runs")
        .join(run_ids[0].to_string())
        .exists());
    let artifacts: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM test_artifacts WHERE run_id = ?1",
            params![run_ids[0]],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(artifacts, 0);
    drop(conn);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn cleanup_orphans_removes_unknown_run_dirs() {
    let (root, state) = temp_state("orphan");
    let suite_id = seed_suite(&state, "s");
    let run_id = seed_run(
        &state,
        suite_id,
        "passed",
        &[SeedCase {
            name: "a",
            status: "passed",
            duration_ms: 10,
            error: None,
            logs: None,
        }],
    );
    let orphan = state.data_dir.join("test-runs").join("999999");
    std::fs::create_dir_all(&orphan).unwrap();
    let keep = state.data_dir.join("test-runs").join(run_id.to_string());
    std::fs::create_dir_all(&keep).unwrap();

    let conn = state.pool.get().unwrap();
    cleanup_orphans(&conn, &state.data_dir);
    assert!(!orphan.exists());
    assert!(keep.exists());
    drop(conn);
    let _ = std::fs::remove_dir_all(&root);
}
