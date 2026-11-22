//! 主节点会话的单元测试：分配、状态机、持久化、清理策略

use super::*;

#[test]
fn pool_slice_size_limit_rejects_oversized() {
    assert!(ensure_pool_slice_bytes(MAX_POOL_SLICE_BYTES, "从机A").is_ok());
    let message = ensure_pool_slice_bytes(MAX_POOL_SLICE_BYTES + 1, "从机A").unwrap_err();
    assert!(message.starts_with(crate::services::pool::E_POOL_SLICE_TOO_LARGE));
    assert!(message.contains("从机A"));
    assert!(message.contains("64 MB"));
    assert!(message.contains("缩小池条目或增加节点"));
}

#[test]
fn pair_code_is_fixed_width_digits() {
    for _ in 0..20 {
        let code = new_pair_code();
        assert_eq!(code.len(), PAIR_CODE_DIGITS);
        assert!(code.chars().all(|ch| ch.is_ascii_digit()));
    }
}

#[test]
fn pair_code_ttl_defaults_and_bounds() {
    assert_eq!(resolve_pair_code_ttl(None).unwrap(), PAIR_CODE_TTL_SEC);
    assert_eq!(resolve_pair_code_ttl(Some(900)).unwrap(), 900);
    assert!(resolve_pair_code_ttl(Some(30)).is_err());
    assert!(resolve_pair_code_ttl(Some(7200)).is_err());
    assert_eq!(resolve_pair_code_ttl(Some(60)).unwrap(), 60);
    assert_eq!(resolve_pair_code_ttl(Some(3600)).unwrap(), 3600);
}

#[test]
fn clock_metrics_follow_ntp_single_sample() {
    // 从节点比主节点快 5 分钟，RTT 40ms
    let (offset, uncertainty) = clock_metrics(1_000, 301_020, 1_040);
    assert!((offset - 300_000.0).abs() < 0.01);
    assert!((uncertainty - 20.0).abs() < 0.01);
}

#[test]
fn allocation_sums_to_total_and_keeps_minimum_one() {
    let participants = vec![
        ("master".to_string(), 1.0),
        ("a".to_string(), 1.0),
        ("b".to_string(), 1.0),
    ];
    let weights = participants.clone();
    let caps = participants
        .iter()
        .map(|(id, _)| (id.clone(), 100u32))
        .collect::<Vec<_>>();
    let limits = participants
        .iter()
        .map(|(id, _)| (id.clone(), 1u32, 100u32))
        .collect::<Vec<_>>();
    let allocation =
        allocate_concurrency_capped(16, &participants, &weights, &caps, &limits).unwrap();
    assert_eq!(allocation.iter().map(|(_, count)| count).sum::<u32>(), 16);
    assert!(allocation.iter().all(|(_, count)| *count >= 1));
    assert_eq!(allocation.len(), 3);

    let two = vec![("master".to_string(), 1.0), ("a".to_string(), 1.0)];
    let two_caps = vec![("master".to_string(), 100u32), ("a".to_string(), 100u32)];
    let two_limits = vec![
        ("master".to_string(), 1u32, 100u32),
        ("a".to_string(), 1u32, 100u32),
    ];
    let allocation = allocate_concurrency_capped(16, &two, &two, &two_caps, &two_limits).unwrap();
    assert_eq!(
        allocation,
        vec![("master".to_string(), 8), ("a".to_string(), 8)]
    );

    let skewed = vec![
        ("master".to_string(), 2.0),
        ("a".to_string(), 1.0),
        ("b".to_string(), 1.0),
    ];
    let skewed_caps = skewed
        .iter()
        .map(|(id, _)| (id.clone(), 100u32))
        .collect::<Vec<_>>();
    let skewed_limits = skewed
        .iter()
        .map(|(id, _)| (id.clone(), 1u32, 100u32))
        .collect::<Vec<_>>();
    let allocation =
        allocate_concurrency_capped(10, &skewed, &skewed, &skewed_caps, &skewed_limits).unwrap();
    assert_eq!(allocation.iter().map(|(_, count)| count).sum::<u32>(), 10);
    assert_eq!(allocation[0].1, 4);
}

#[test]
fn allocation_rejects_more_nodes_than_concurrency() {
    let participants = vec![("a".to_string(), 1.0), ("b".to_string(), 1.0)];
    let caps = vec![("a".to_string(), 100u32), ("b".to_string(), 100u32)];
    let limits = vec![
        ("a".to_string(), 1u32, 100u32),
        ("b".to_string(), 1u32, 100u32),
    ];
    assert!(allocate_concurrency_capped(1, &participants, &participants, &caps, &limits).is_err());
}

#[test]
fn even_mode_splits_with_remainder() {
    let participants = vec![
        ("master".to_string(), 8.0),
        ("a".to_string(), 4.0),
        ("b".to_string(), 2.0),
    ];
    let weights = resolve_weights(&participants, &[], "even").unwrap();
    assert!(weights
        .iter()
        .all(|(_, weight)| (*weight - 1.0).abs() < f64::EPSILON));
    let caps = vec![
        ("master".to_string(), 100),
        ("a".to_string(), 100),
        ("b".to_string(), 100),
    ];
    let limits = vec![
        ("master".to_string(), 1, 100),
        ("a".to_string(), 1, 100),
        ("b".to_string(), 1, 100),
    ];
    let allocation =
        allocate_concurrency_capped(10, &participants, &weights, &caps, &limits).unwrap();
    assert_eq!(allocation.iter().map(|(_, count)| count).sum::<u32>(), 10);
    assert_eq!(
        allocation,
        vec![
            ("master".to_string(), 4),
            ("a".to_string(), 3),
            ("b".to_string(), 3),
        ]
    );
}

#[test]
fn allocation_respects_node_max_and_reports_cap_shortage() {
    let participants = vec![("a".to_string(), 1.0), ("b".to_string(), 1.0)];
    let weights = resolve_weights(&participants, &[], "even").unwrap();
    let caps = vec![("a".to_string(), 8), ("b".to_string(), 8)];
    let limits = vec![("a".to_string(), 1, 2), ("b".to_string(), 1, 8)];
    let allocation =
        allocate_concurrency_capped(10, &participants, &weights, &caps, &limits).unwrap();
    assert_eq!(allocation, vec![("a".to_string(), 2), ("b".to_string(), 8)]);

    let tight = vec![("a".to_string(), 1, 2), ("b".to_string(), 1, 3)];
    let error =
        allocate_concurrency_capped(10, &participants, &weights, &caps, &tight).unwrap_err();
    assert!(error.contains("上限之和"), "错误应说明上限不足：{error}");
}

#[test]
fn allocation_rejects_min_above_total() {
    let participants = vec![("a".to_string(), 1.0), ("b".to_string(), 1.0)];
    let weights = resolve_weights(&participants, &[], "even").unwrap();
    let caps = vec![("a".to_string(), 32), ("b".to_string(), 32)];
    let limits = vec![("a".to_string(), 6, 8), ("b".to_string(), 6, 8)];
    let error =
        allocate_concurrency_capped(8, &participants, &weights, &caps, &limits).unwrap_err();
    assert!(
        error.contains("最小并发之和"),
        "错误应说明下限过高：{error}"
    );
}

#[test]
fn allocation_mode_defaults_and_rejects_unknown() {
    assert_eq!(normalize_allocation_mode(None, false).unwrap(), "auto");
    assert_eq!(normalize_allocation_mode(None, true).unwrap(), "custom");
    assert_eq!(
        normalize_allocation_mode(Some(" EVEN "), false).unwrap(),
        "even"
    );
    assert!(normalize_allocation_mode(Some("random"), false).is_err());
}

#[test]
fn required_vars_cover_headers_body_auth_and_skip_disabled() {
    let config: StressConfig = serde_json::from_str(
        r#"{
            "url":"{{base_url}}/api/{{id}}",
            "headers":[
                {"key":"X-Token","value":"{{token}}","enabled":true},
                {"key":"X-Off","value":"{{hidden}}","enabled":false}
            ],
            "body":"{\"user\":\"{{user}}\"}",
            "auth":{"kind":"bearer","token":"{{token}}"}
        }"#,
    )
    .unwrap();
    let names = collect_required_vars(&config);
    assert!(names.contains(&"base_url".to_string()));
    assert!(names.contains(&"id".to_string()));
    assert!(names.contains(&"token".to_string()));
    assert!(names.contains(&"user".to_string()));
    assert!(!names.contains(&"hidden".to_string()));
    assert_eq!(names.iter().filter(|name| *name == "token").count(), 1);
}

#[test]
fn shared_config_rejects_phase_one_gaps() {
    let mut config: StressConfig = serde_json::from_str(r#"{"url":"http://127.0.0.1/x"}"#).unwrap();
    config.vars.insert("token".to_string(), "abc".to_string());
    assert!(shared_config(&config).unwrap_err().contains("运行期变量"));

    let mut config: StressConfig = serde_json::from_str(r#"{"url":"http://127.0.0.1/x"}"#).unwrap();
    config.stages = vec![serde_json::from_str(r#"{"concurrent":1,"durationSec":5}"#).unwrap()];
    assert!(shared_config(&config).unwrap_err().contains("阶梯"));

    let mut config: StressConfig = serde_json::from_str(
        r#"{"url":"http://127.0.0.1/x","auth":{"kind":"bearer","token":"plain-secret"}}"#,
    )
    .unwrap();
    assert!(shared_config(&config).unwrap_err().contains("明文"));

    config.auth.as_mut().unwrap().token = "{{token}}".to_string();
    let (value, _) = shared_config(&config).unwrap();
    assert!(value.get("auth").is_some());
}

#[test]
fn shared_config_clears_env_and_keeps_template() {
    let config: StressConfig = serde_json::from_str(
        r#"{"url":"{{base_url}}/x","envId":7,"headers":[{"key":"A","value":"{{token}}","enabled":true}]}"#,
    )
    .unwrap();
    let (value, names) = shared_config(&config).unwrap();
    assert!(value.get("envId").is_none() || value["envId"].is_null());
    assert_eq!(value["url"], "{{base_url}}/x");
    assert!(names.contains(&"base_url".to_string()));
    assert!(names.contains(&"token".to_string()));
}

#[test]
fn finished_job_records_report_and_incomplete_flag() {
    let (dir, pool) = crate::notes::test_pool("cluster-job-finish");
    let job_id = insert_job(
        &pool,
        1_000,
        "master",
        Some(7),
        "计划",
        17801,
        "{}",
        2_000,
        16,
    )
    .unwrap();
    update_job_finished(&pool, job_id, "合并报告 #9 已生成", true, Some(9)).unwrap();
    let conn = pool.get().unwrap();
    let (status, report_id, incomplete, message, finished): (
        String,
        Option<i64>,
        i64,
        String,
        Option<i64>,
    ) = conn
        .query_row(
            "SELECT status, report_id, incomplete, message, finished_at
             FROM cluster_jobs WHERE id = ?1",
            params![job_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(status, "finished");
    assert_eq!(report_id, Some(9));
    assert_eq!(incomplete, 1);
    assert_eq!(message, "合并报告 #9 已生成");
    assert!(finished.is_some());
    drop(conn);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn nodes_settled_requires_master_done() {
    let mut state = SessionState {
        status: "running".to_string(),
        start_at_ms: None,
        report_id: None,
        incomplete: false,
        message: None,
        pair_code: None,
        nodes: vec![NodeState::master("m".to_string(), String::new(), 1)],
        next_node: 1,
        join_requests: HashMap::new(),
        pair_failures: HashMap::new(),
        pair_fail_total: 0,
    };
    assert!(!nodes_settled(&state));
    state.nodes[0].status = "done".to_string();
    assert!(nodes_settled(&state));
    state
        .nodes
        .push(NodeState::master("s".to_string(), String::new(), 1));
    state.nodes[1].status = "lost".to_string();
    assert!(nodes_settled(&state));
}

#[test]
fn pair_failures_rate_limit_and_rotate_code() {
    let now = 1_700_000_000_000;
    let mut state = SessionState {
        status: "pairing".to_string(),
        start_at_ms: None,
        report_id: None,
        incomplete: false,
        message: None,
        pair_code: Some(PairCode {
            code: "123456".to_string(),
            expires_at: now + 300_000,
        }),
        nodes: Vec::new(),
        next_node: 1,
        join_requests: HashMap::new(),
        pair_failures: HashMap::new(),
        pair_fail_total: 0,
    };
    // 同一来源连续失败：前 4 次正常提示，第 5 次触发窗口限速
    for _ in 0..4 {
        let message = register_pair_failure(&mut state, "10.0.0.2", now, 300);
        assert_eq!(message, "配对码不正确");
    }
    let message = register_pair_failure(&mut state, "10.0.0.2", now, 300);
    assert_eq!(message, "配对尝试过于频繁，请稍后再试");
    // 换个来源继续试：累计达到上限后当前码作废并换新
    for _ in 0..4 {
        let message = register_pair_failure(&mut state, "10.0.0.3", now, 300);
        assert_eq!(message, "配对码不正确");
    }
    let message = register_pair_failure(&mut state, "10.0.0.3", now, 300);
    assert!(message.contains("已刷新"), "{message}");
    let code = state.pair_code.as_ref().unwrap().code.clone();
    assert_ne!(code, "123456");
    assert_eq!(state.pair_fail_total, 0);
    assert!(state.pair_failures.is_empty());
}

#[test]
fn resolve_weights_uses_default_and_rejects_unknown() {
    let participants = vec![("master".to_string(), 1.0), ("a".to_string(), 1.0)];
    let resolved = resolve_weights(
        &participants,
        &[ClusterNodeWeight {
            node_id: "a".to_string(),
            weight: 2.0,
            min_concurrent: None,
            max_concurrent: None,
        }],
        "custom",
    )
    .unwrap();
    assert_eq!(resolved[0].1, 1.0);
    assert_eq!(resolved[1].1, 2.0);

    assert!(resolve_weights(
        &participants,
        &[ClusterNodeWeight {
            node_id: "ghost".to_string(),
            weight: 1.0,
            min_concurrent: None,
            max_concurrent: None,
        }],
        "custom",
    )
    .is_err());
}

#[test]
fn clock_recheck_records_drift_without_touching_offset() {
    let mut node = NodeState::master("s".to_string(), String::new(), 1);
    node.clock_offset_ms = Some(120.0);
    // 60 秒后再测，偏移变成 180ms：漂移 60ms，但配对偏移必须保持不动
    apply_clock_recheck(&mut node, 180.0, 1_700_000_060_000);
    assert_eq!(node.clock_offset_ms, Some(120.0), "复测不能改配对偏移");
    assert!((node.clock_drift_ms - 60.0).abs() < 1e-9);
    assert_eq!(node.clock_recheck_at, Some(1_700_000_060_000));
}

#[test]
fn clock_recheck_without_baseline_keeps_zero_drift() {
    let mut node = NodeState::master("s".to_string(), String::new(), 1);
    apply_clock_recheck(&mut node, 50.0, 1_700_000_000_000);
    assert_eq!(node.clock_drift_ms, 0.0, "没配对过就没有基准，不能当成漂移");
    assert!(node.clock_recheck_at.is_some());
}

/// 12.5 性能验收（手动执行，默认测试集跳过）
///
/// `cargo test --lib perf_save_cluster_samples -- --ignored --nocapture`
#[cfg(test)]
mod perf_tests {
    use super::*;
    use r2d2_sqlite::SqliteConnectionManager;
    use std::time::Instant;

    #[test]
    #[ignore = "性能验收，需手动执行"]
    fn perf_save_cluster_samples_24h_16_nodes() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|value| value.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!(
            "astroman-perf-series-{}-{stamp}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let manager = SqliteConnectionManager::file(dir.join("series.db")).with_init(|conn| {
            conn.execute_batch(
                "PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL; PRAGMA foreign_keys = ON;",
            )
        });
        let db: crate::state::DbPool = r2d2::Pool::builder().max_size(2).build(manager).unwrap();
        {
            let conn = db.get().unwrap();
            conn.execute_batch(
                "CREATE TABLE test_reports (id INTEGER PRIMARY KEY);
             INSERT INTO test_reports (id) VALUES (1);
             CREATE TABLE cluster_report_samples (
                 id          INTEGER PRIMARY KEY AUTOINCREMENT,
                 report_id   INTEGER NOT NULL REFERENCES test_reports(id) ON DELETE CASCADE,
                 node        TEXT    NOT NULL,
                 role        TEXT    NOT NULL DEFAULT 'slave',
                 elapsed_sec INTEGER NOT NULL,
                 concurrent  INTEGER DEFAULT 0,
                 qps         REAL    DEFAULT 0,
                 p95_rt      REAL    DEFAULT 0,
                 error_count INTEGER DEFAULT 0
             );
             CREATE INDEX idx_cluster_samples_report
                 ON cluster_report_samples(report_id, node, elapsed_sec);",
            )
            .unwrap();
        }

        // 24h 抽稀后的上限：16 节点 × 1800 点 = 28,800 行
        let series = (0..16u32)
            .map(|index| crate::services::cluster::protocol::NodeSeries {
                node: format!("node-{index:02}"),
                role: if index == 0 {
                    "master".into()
                } else {
                    "slave".into()
                },
                points: (0..1_800u64)
                    .map(|point| crate::services::cluster::protocol::SeriesPoint {
                        sec: point * 48,
                        qps: 100.0,
                        concurrent: 100,
                        p95: 20.0,
                        errors: 0,
                        ..Default::default()
                    })
                    .collect(),
            })
            .collect::<Vec<_>>();

        let started = Instant::now();
        save_cluster_samples(&db, 1, &series).unwrap();
        let elapsed = started.elapsed();
        let rows: usize = series.iter().map(|node| node.points.len()).sum();
        println!("perf_save_cluster_samples: {rows} rows in {elapsed:?}");
        assert!(rows <= 50_000, "节点序列 {rows} 行超过 50k 目标");
        assert!(
            elapsed < std::time::Duration::from_millis(2_000),
            "写入 {rows} 行耗时 {elapsed:?}，超过 2s 目标"
        );
        drop(db);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
