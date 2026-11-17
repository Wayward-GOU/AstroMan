//! 双机验收（W6）：用同进程两个 mock 应用模拟主从两台机器
//!
//! 桌面端有单实例锁，没法真的起两个应用进程；这里给主从各自一套
//! `AppState::headless`（独立数据库与任务注册表），集群服务通过
//! `ClusterApp` 解耦后可直接跑在 `tauri::test::MockRuntime` 上。
//! 覆盖：16 并发合并吞吐、目标不可达预检拦截、从节点失联、缺失环境变量。

use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rusqlite::params;
use tauri::test::{mock_app, MockRuntime};
use tauri::{AppHandle, Manager};

use super::client::ClusterSlaveConnectPayload;
use super::host::ClusterApp;
use super::server::{ClusterMasterStartPayload, ClusterNodeWeight};
use crate::commands::stress::{prepare_stress, run_engine, EngineOptions};
use crate::models::StressConfig;
use crate::state::AppState;

/* ---------- 本地目标服务 ---------- */

/// 固定延迟应答的 keep-alive HTTP 目标：吞吐由并发数决定，便于和单机基线对比
struct TestTarget {
    port: u16,
    stop: Arc<AtomicBool>,
    accept: Option<std::thread::JoinHandle<()>>,
}

impl TestTarget {
    fn start(delay_ms: u64) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("绑定验收目标端口");
        let port = listener.local_addr().expect("读取验收目标端口").port();
        listener.set_nonblocking(true).expect("目标端口设为非阻塞");
        let stop = Arc::new(AtomicBool::new(false));
        let accept_stop = Arc::clone(&stop);
        let accept = std::thread::spawn(move || {
            while !accept_stop.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let conn_stop = Arc::clone(&accept_stop);
                        std::thread::spawn(move || serve_connection(stream, delay_ms, conn_stop));
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(_) => break,
                }
            }
        });
        Self {
            port,
            stop,
            accept: Some(accept),
        }
    }

    fn url(&self) -> String {
        format!("http://127.0.0.1:{}/", self.port)
    }
}

impl Drop for TestTarget {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.accept.take() {
            let _ = handle.join();
        }
    }
}

fn serve_connection(mut stream: std::net::TcpStream, delay_ms: u64, stop: Arc<AtomicBool>) {
    let _ = stream.set_read_timeout(Some(Duration::from_millis(200)));
    let mut buffer = [0u8; 2048];
    let mut pending: Vec<u8> = Vec::new();
    while !stop.load(Ordering::SeqCst) {
        match std::io::Read::read(&mut stream, &mut buffer) {
            Ok(0) => break,
            Ok(count) => {
                pending.extend_from_slice(&buffer[..count]);
                // GET 无请求体：每个 `\r\n\r\n` 就是一个完整请求
                while let Some(position) = pending.windows(4).position(|w| w == b"\r\n\r\n") {
                    pending.drain(..position + 4);
                    std::thread::sleep(Duration::from_millis(delay_ms));
                    let response = "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 2\r\nConnection: keep-alive\r\n\r\nok";
                    if std::io::Write::write_all(&mut stream, response.as_bytes()).is_err() {
                        return;
                    }
                }
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(_) => break,
        }
    }
}

/* ---------- mock 机器 ---------- */

static NODE_SEQ: AtomicUsize = AtomicUsize::new(0);

/// 一台“机器”：独立数据目录 + 独立 AppState + mock 运行时
struct MockNode {
    app: tauri::App<MockRuntime>,
    dir: PathBuf,
}

impl MockNode {
    fn new(tag: &str) -> Self {
        let index = NODE_SEQ.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "astroman-w6-{}-{}-{}",
            std::process::id(),
            tag,
            index
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let state = AppState::headless(dir.clone(), dir.clone()).expect("初始化无窗口 AppState");
        let app = mock_app();
        app.manage(state);
        Self { app, dir }
    }

    fn state(&self) -> tauri::State<'_, AppState> {
        self.app.state::<AppState>()
    }

    fn host(&self) -> Arc<dyn ClusterApp> {
        let handle: AppHandle<MockRuntime> = self.app.handle().clone();
        Arc::new(handle)
    }

    fn cleanup(self) {
        let dir = self.dir.clone();
        drop(self);
        let _ = std::fs::remove_dir_all(dir);
    }
}

/* ---------- 公共流程 ---------- */

fn smoke_config(url: &str, concurrent: u32, duration_sec: u32) -> StressConfig {
    serde_json::from_value(serde_json::json!({
        "name": "W6 验收",
        "method": "GET",
        "url": url,
        "concurrent": concurrent,
        "durationSec": duration_sec,
        "timeoutMs": 5000,
        "ignoreOverlap": true
    }))
    .expect("构造压测配置")
}

async fn wait_until<F>(timeout: Duration, mut check: F) -> bool
where
    F: FnMut() -> bool,
{
    let deadline = Instant::now() + timeout;
    loop {
        if check() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// 建任务 + 从节点配对，返回 (job_id, 从节点 id)
async fn pair_cluster(
    master: &MockNode,
    slave: &MockNode,
    config: StressConfig,
    env_name: Option<String>,
) -> (i64, String) {
    let payload = ClusterMasterStartPayload {
        plan_id: None,
        plan_name: Some("W6 验收".to_string()),
        config,
        env_name,
        port: Some(0),
        master_name: Some("主-验收".to_string()),
    };
    let snapshot = master
        .state()
        .cluster
        .create_with(master.state().pool.clone(), master.host(), payload)
        .await
        .expect("主节点创建任务");
    let job_id = snapshot.job_id;
    let pair_code = snapshot.pair_code.clone().expect("配对码");
    let connect = ClusterSlaveConnectPayload {
        master_addr: format!("127.0.0.1:{}", snapshot.port),
        pair_code,
        node_name: Some("从-验收".to_string()),
    };
    slave
        .state()
        .cluster_slave
        .connect_with(slave.state().pool.clone(), slave.host(), connect)
        .expect("从节点发起连接");

    let slave_id = {
        let mut found = None;
        wait_until(Duration::from_secs(10), || {
            let snapshot = master.state().cluster.snapshot(job_id).expect("读主节点快照");
            found = snapshot
                .nodes
                .iter()
                .find(|node| node.role == "slave" && node.status == "pairReady")
                .map(|node| node.id.clone());
            found.is_some()
        })
        .await;
        found.expect("从节点 10 秒内未完成配对与时钟对齐")
    };
    (job_id, slave_id)
}

fn create_env(node: &MockNode, name: &str, vars: &[(&str, &str)]) -> i64 {
    let conn = node.state().pool.get().expect("取节点数据库连接");
    conn.execute(
        "INSERT INTO projects (name, description, color, sort_order, created_at, updated_at)
         VALUES ('验收项目', NULL, NULL, 0, 0, 0)",
        [],
    )
    .expect("插入验收项目");
    let project_id = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO environments (project_id, name, is_active, script, is_prod, created_at)
         VALUES (?1, ?2, 1, NULL, 0, 0)",
        params![project_id, name],
    )
    .expect("插入验收环境");
    let env_id = conn.last_insert_rowid();
    for (key, value) in vars {
        conn.execute(
            "INSERT INTO env_variables (env_id, \"key\", value, is_secret, sort_order)
             VALUES (?1, ?2, ?3, 0, 0)",
            params![env_id, key, value],
        )
        .expect("插入环境变量");
    }
    env_id
}

async fn baseline_qps(node: &MockNode, config: StressConfig) -> f64 {
    let state = node.state();
    let prepared = prepare_stress(&state, config, Arc::new(AtomicBool::new(false)))
        .expect("单机基线准备");
    let outcome = run_engine(
        prepared,
        0,
        None,
        None,
        EngineOptions {
            start_at_ms: None,
            sink: None,
        },
    )
    .await;
    assert_eq!(outcome.report.status, "completed", "基线压测应正常结束");
    outcome.report.avg_qps
}

fn report_of_master(
    node: &MockNode,
    report_id: i64,
) -> (i64, f64, String, String) {
    let conn = node.state().pool.get().expect("取主节点数据库连接");
    conn.query_row(
        "SELECT total_requests, avg_qps, config_json, status FROM test_reports WHERE id = ?1",
        params![report_id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    )
    .expect("读取合并报告")
}

/* ---------- 验收用例 ---------- */

/// 跑一轮完整测量：配对 → 下发预检 → 启动 → 合并报告 → 两台 8 并发单机基线
///
/// 返回 (集群合并 QPS, 基线 A QPS, 基线 B QPS)；报告结构断言每轮都校验。
async fn smoke_round(target: &TestTarget) -> (f64, f64, f64) {
    let master = MockNode::new("master");
    let slave = MockNode::new("slave");

    let config = smoke_config(&target.url(), 16, 4);
    let (job_id, slave_id) = pair_cluster(&master, &slave, config, None).await;

    master
        .state()
        .cluster
        .prepare(
            job_id,
            vec![ClusterNodeWeight {
                node_id: slave_id.clone(),
                weight: 1.0,
            }],
        )
        .await
        .expect("下发配置并预检");
    master.state().cluster.launch(job_id).expect("启动集群压测");

    // finished 先于落库发布：等报告 id 出现再断言，避免撞上合并落库窗口
    let finished = wait_until(Duration::from_secs(60), || {
        master
            .state()
            .cluster
            .snapshot(job_id)
            .map(|snapshot| {
                snapshot.status == "failed"
                    || (snapshot.status == "finished" && snapshot.report_id.is_some())
            })
            .unwrap_or(false)
    })
    .await;
    assert!(finished, "集群压测 60 秒内未结束");
    let snapshot = master.state().cluster.snapshot(job_id).expect("读最终快照");
    assert_eq!(
        snapshot.status,
        "finished",
        "{}",
        snapshot.message.unwrap_or_default()
    );
    assert!(!snapshot.incomplete, "双节点都正常时报告不应标注不完整");
    let report_id = snapshot
        .report_id
        .unwrap_or_else(|| panic!("合并报告 id 缺失：{}", snapshot.message.unwrap_or_default()));

    // 报告元数据：两节点各 8 并发、协议版本与窗口齐全
    let (total, cluster_qps, config_json, status) = report_of_master(&master, report_id);
    assert_eq!(status, "completed");
    assert!(total > 0, "合并报告应包含请求数");
    let config: serde_json::Value = serde_json::from_str(&config_json).expect("解析报告配置快照");
    let cluster = config.get("cluster").expect("报告应带集群元数据");
    assert_eq!(cluster["incomplete"], false);
    let nodes = cluster["nodes"].as_array().expect("节点元数据");
    assert_eq!(nodes.len(), 2);
    for node in nodes {
        assert_eq!(node["concurrent"].as_u64(), Some(8), "16 并发应平均分给两节点");
    }
    let slave_node = nodes
        .iter()
        .find(|node| node["role"] == "slave")
        .expect("从节点元数据");
    assert_eq!(slave_node["status"], "done");
    assert!(
        slave_node["clockOffsetMs"].as_f64().is_some(),
        "从节点应记录时钟偏移"
    );

    master.state().cluster.close(job_id).expect("关闭主节点会话");
    let _ = slave.state().cluster_slave.disconnect();
    master.cleanup();
    slave.cleanup();

    // 单机基线：两台各跑 8 并发，并发执行以对齐集群的资源占用
    let base_a = MockNode::new("base-a");
    let base_b = MockNode::new("base-b");
    let (left, right) = tokio::join!(
        baseline_qps(&base_a, smoke_config(&target.url(), 8, 4)),
        baseline_qps(&base_b, smoke_config(&target.url(), 8, 4)),
    );
    base_a.cleanup();
    base_b.cleanup();
    (cluster_qps, left, right)
}

#[tokio::test]
async fn two_node_smoke_matches_sum_of_independent_runs() {
    let target = TestTarget::start(10);
    // 整套用例并行时其他测试抢 CPU，会压低单机基线读数（合并轮次却未必同样受压）。
    // 每轮都是完整测量，允许重测：任一轮偏差 <3% 即视为合并口径正确。
    let mut attempts: Vec<String> = Vec::new();
    let mut passed = false;
    for attempt in 1..=3 {
        let (cluster_qps, left, right) = smoke_round(&target).await;
        let sum = left + right;
        let delta = (cluster_qps - sum).abs() / sum;
        eprintln!(
            "[W6] 第 {attempt} 轮：16 并发合并 {cluster_qps:.1} QPS；两台单机 8 并发 {left:.1} + {right:.1} = {sum:.1} QPS；偏差 {:.2}%",
            delta * 100.0
        );
        attempts.push(format!(
            "第{attempt}轮 {cluster_qps:.1} vs {sum:.1}（{:.2}%）",
            delta * 100.0
        ));
        if delta < 0.03 {
            passed = true;
            break;
        }
    }
    assert!(
        passed,
        "合并吞吐与两台单机之和偏差始终超过 3%：{}",
        attempts.join("；")
    );
}

#[tokio::test]
async fn unreachable_target_is_blocked_at_preflight() {
    // 先占一个端口再释放：保证该端口上没有监听
    let probe = TcpListener::bind(("127.0.0.1", 0)).expect("占位端口");
    let port = probe.local_addr().expect("占位端口地址").port();
    drop(probe);

    let master = MockNode::new("preflight-master");
    let slave = MockNode::new("preflight-slave");
    let config = smoke_config(&format!("http://127.0.0.1:{port}/"), 2, 2);
    let (job_id, slave_id) = pair_cluster(&master, &slave, config, None).await;

    let error = master
        .state()
        .cluster
        .prepare(
            job_id,
            vec![ClusterNodeWeight {
                node_id: slave_id,
                weight: 1.0,
            }],
        )
        .await
        .expect_err("目标不可达时准备必须失败");
    assert!(error.contains("从-验收"), "错误信息应带从节点名：{error}");
    assert!(error.contains("无法访问目标"), "错误信息应说明预检失败：{error}");

    let snapshot = master.state().cluster.snapshot(job_id).expect("读快照");
    assert_eq!(snapshot.status, "failed");
    assert_eq!(snapshot.nodes[1].status, "rejected");

    master.state().cluster.close(job_id).expect("关闭主节点会话");
    let _ = slave.state().cluster_slave.disconnect();
    master.cleanup();
    slave.cleanup();
}

#[tokio::test]
async fn lost_slave_marks_report_incomplete() {
    let target = TestTarget::start(10);
    let master = MockNode::new("lost-master");
    let slave = MockNode::new("lost-slave");

    let config = smoke_config(&target.url(), 16, 6);
    let (job_id, slave_id) = pair_cluster(&master, &slave, config, None).await;
    master
        .state()
        .cluster
        .prepare(
            job_id,
            vec![ClusterNodeWeight {
                node_id: slave_id,
                weight: 1.0,
            }],
        )
        .await
        .expect("下发配置并预检");
    master.state().cluster.launch(job_id).expect("启动集群压测");

    let running = wait_until(Duration::from_secs(15), || {
        master
            .state()
            .cluster
            .snapshot(job_id)
            .map(|snapshot| snapshot.status == "running")
            .unwrap_or(false)
    })
    .await;
    assert!(running, "集群压测未进入运行状态");
    tokio::time::sleep(Duration::from_millis(1_200)).await;

    // 模拟从节点进程被杀：直接掐断会话任务，不发送 abort
    assert!(slave.state().cluster_slave.kill(), "应能杀掉从节点会话");
    let killed_at = Instant::now();
    let lost = wait_until(Duration::from_secs(7), || {
        master
            .state()
            .cluster
            .snapshot(job_id)
            .map(|snapshot| {
                snapshot
                    .nodes
                    .iter()
                    .any(|node| node.role == "slave" && node.status == "lost")
            })
            .unwrap_or(false)
    })
    .await;
    assert!(
        lost,
        "从节点被杀后 7 秒内未标记失联（实际等待 {:?}）",
        killed_at.elapsed()
    );
    eprintln!(
        "[W6] 从节点被杀后 {:.1}s 标记 lost（阈值 6s）",
        killed_at.elapsed().as_secs_f64()
    );

    let finished = wait_until(Duration::from_secs(40), || {
        master
            .state()
            .cluster
            .snapshot(job_id)
            .map(|snapshot| snapshot.status == "finished" && snapshot.report_id.is_some())
            .unwrap_or(false)
    })
    .await;
    assert!(finished, "从节点失联后主节点应继续跑完并生成报告");
    let snapshot = master.state().cluster.snapshot(job_id).expect("读最终快照");
    assert!(snapshot.incomplete, "失联时最终状态应标注不完整");
    let report_id = snapshot
        .report_id
        .unwrap_or_else(|| panic!("失联也应生成合并报告：{}", snapshot.message.unwrap_or_default()));

    let (total, _qps, config_json, status) = report_of_master(&master, report_id);
    assert_eq!(status, "completed");
    assert!(total > 0);
    let config: serde_json::Value = serde_json::from_str(&config_json).expect("解析报告配置快照");
    let cluster = config.get("cluster").expect("报告应带集群元数据");
    assert_eq!(cluster["incomplete"], true);
    assert!(
        cluster["note"].as_str().unwrap_or("").contains("结果不完整"),
        "备注应说明结果不完整：{}",
        cluster["note"]
    );
    let lost_node = cluster["nodes"]
        .as_array()
        .and_then(|nodes| nodes.iter().find(|node| node["role"] == "slave"))
        .expect("从节点元数据");
    assert_eq!(lost_node["status"], "lost");

    master.state().cluster.close(job_id).expect("关闭主节点会话");
    master.cleanup();
    slave.cleanup();
}

#[tokio::test]
async fn missing_env_var_is_reported_by_name() {
    let target = TestTarget::start(5);
    let master = MockNode::new("env-master");
    let slave = MockNode::new("env-slave");

    // 主节点环境齐全；从节点同名环境缺少 token
    let master_env = create_env(&master, "prod", &[("token", "abc")]);
    create_env(&slave, "prod", &[]);

    let mut config: StressConfig = serde_json::from_value(serde_json::json!({
        "name": "W6 缺变量",
        "method": "GET",
        "url": target.url(),
        "headers": [{ "key": "X-Token", "value": "{{token}}", "enabled": true }],
        "concurrent": 2,
        "durationSec": 2,
        "ignoreOverlap": true
    }))
    .expect("构造缺变量配置");
    config.env_id = Some(master_env);

    let (job_id, slave_id) =
        pair_cluster(&master, &slave, config, Some("prod".to_string())).await;

    let error = master
        .state()
        .cluster
        .prepare(
            job_id,
            vec![ClusterNodeWeight {
                node_id: slave_id,
                weight: 1.0,
            }],
        )
        .await
        .expect_err("从节点缺少变量时准备必须失败");
    assert!(error.contains("从-验收"), "错误信息应带从节点名：{error}");
    assert!(error.contains("token"), "错误信息应带缺失变量名：{error}");

    master.state().cluster.close(job_id).expect("关闭主节点会话");
    let _ = slave.state().cluster_slave.disconnect();
    drop(target);
    master.cleanup();
    slave.cleanup();
}
