use crate::state::{AppState, DbPool};
use crate::util::err;

mod seed;

/// 数据库结构版本（PRAGMA user_version）
///
/// 1 = v1.0.7；2 = v1.0.8（耗时分解、响应体类型、按请求网络配置、app_meta）
/// 3 = 按 Body 类型保留草稿（requests.body_meta）
/// 4 = 请求级凭据字段加密（无新列）+ 补齐 schema.sql 缺失的 requests.net_json
/// 5 = 记事本（notes.done_at / deleted_at / deleted_from_status、note_files、ai_providers.supports_vision）
/// 6 = 记事本类型（notes.kind：todo / note / idea / reference）
/// 7 = 运行记录瘦身（请求 / 响应体积与截断标记）
/// 8 = 模型上下文窗口覆盖（ai_providers.context_window）
/// 9 = 接口实体（requests.request_entity / response_entity）
/// 10 = 资产闭环（请求版本历史 request_revisions、契约快照 contract_snapshots）
/// 11 = AI 治理（审计表 audit_logs、生产环境标记 environments.is_prod）
/// 12 = 协议广度（requests.protocol、GraphQL schema 表 graphql_schemas）
/// 13 = AI 专项（ai_providers.native_tools / 单价 / 币种；设置项随 settings.json 落盘）
/// 14 = AI 写操作回滚（request_revisions.trace_id）
/// 15 = 压测 v1.1.1（plan 的 request_id 可空且 SET NULL、机器容量表、
///      报告分组 / 响应体耗时直方图 / TopN 慢请求、采样响应体耗时）
/// 16 = 分布式压测一期（cluster_jobs / cluster_workers，报告仍复用 test_reports）
/// 17 = 压测增量落库与外部指标（stress_job_samples / stress_job_meta、
///      test_reports.external_metrics_json）
/// 18 = 抓包会话归档（capture_sessions / capture_flows / capture_bodies）
/// 19 = 分布式压测二期三期（cluster_devices、cluster_jobs.orchestration_json）
pub const DB_VERSION: i64 = 19;

/// 每条连接都要应用的 SQLite 参数（P-06）。
///
/// - `cache_size = -16000`：16MB page cache，换大库查询命中率；
/// - `synchronous = NORMAL`：WAL 下足够安全，避免每次提交都 fsync；
/// - `mmap_size`：只在 64 位构建启用，32 位地址空间不足，开了反而更容易失败。
pub fn connection_pragmas() -> String {
    let mut sql = String::from(
        "PRAGMA journal_mode = WAL;
         PRAGMA foreign_keys = ON;
         PRAGMA busy_timeout = 5000;
         PRAGMA cache_size = -16000;
         PRAGMA synchronous = NORMAL;",
    );
    #[cfg(target_pointer_width = "64")]
    sql.push_str(" PRAGMA mmap_size = 134217728;");
    sql
}

/// 打开库时发现库版本高于本程序：说明用户降级了应用。
/// 直接返回错误而不是继续写，避免旧代码用旧结构写坏新数据。
const E_SCHEMA_FUTURE: &str = "E_SCHEMA: 数据库由更高版本的 AstroMan 创建，请升级应用；\
如已降级，请从「设置 → 数据备份」恢复对应版本的备份";

/// 主库与 WAL 的合计字节数，用来计算"这次清理到底释放了多少空间"
pub fn total_size(state: &AppState) -> u64 {
    let db = std::fs::metadata(&state.db_path)
        .map(|meta| meta.len())
        .unwrap_or(0);
    let wal = std::fs::metadata(state.db_path.with_extension("db-wal"))
        .map(|meta| meta.len())
        .unwrap_or(0);
    db + wal
}

/// 删除数据后回收磁盘空间
///
/// `DELETE` 只把页标成空闲，文件本身不会缩小；WAL 模式下空闲页还会继续留在日志里。
/// 因此固定按「截断 WAL → VACUUM 重写主库 → 再截断 WAL」执行：VACUUM 产生的新页会先写进
/// WAL，前后各截断一次才能让两个文件都真正变小。
///
/// 返回 (压缩前字节, 压缩后字节)；失败原样返回，由调用方决定提示方式。
pub fn reclaim_space(state: &AppState) -> Result<(u64, u64), String> {
    let before = total_size(state);
    let conn = state.pool.get().map_err(err)?;
    conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
        .map_err(|error| format!("截断 WAL 失败：{error}"))?;
    conn.execute_batch("VACUUM;")
        .map_err(|error| format!("压缩数据库失败：{error}"))?;
    // VACUUM 的写入都经过 WAL，不收尾的话文件大小等于没变
    conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
        .map_err(|error| format!("压缩后截断 WAL 失败：{error}"))?;
    Ok((before, total_size(state)))
}

/// 清理命令统一用这个：压测运行中跳过压缩，其余情况拿重任务锁后回收空间
///
/// 返回 (释放字节, 失败原因)。数据删除本身已经提交，压缩失败不影响删除结果。
pub fn compact_after_delete(state: &AppState) -> (u64, Option<String>) {
    if !state.stress_jobs.is_empty() {
        return (
            0,
            Some("压测正在运行，已跳过压缩；请压测结束后重新清理".into()),
        );
    }
    let _guard = state.heavy_ops.lock();
    match reclaim_space(state) {
        Ok((before, after)) => (before.saturating_sub(after), None),
        Err(error) => (0, Some(error)),
    }
}

pub fn init(pool: &DbPool) -> Result<(), String> {
    {
        let conn = pool.get().map_err(err)?;
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .map_err(err)?;
        if version > DB_VERSION {
            return Err(E_SCHEMA_FUTURE.to_string());
        }
    }

    let conn = pool.get().map_err(err)?;
    conn.execute_batch(include_str!("schema.sql"))
        .map_err(err)?;
    drop(conn);

    migrate(pool)?;
    verify_core_tables(pool)?;
    recover_interrupted_runs(pool)?;
    recover_interrupted_stress_jobs(pool)?;
    seed_database(pool)
}

/// 关键表存在性校验：缺表说明库损坏或结构被外力改过，交给恢复流程处理
fn verify_core_tables(pool: &DbPool) -> Result<(), String> {
    let conn = pool.get().map_err(err)?;
    let mut missing = Vec::new();
    for table in ["projects", "collections", "requests", "environments"] {
        let exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                [table],
                |row| row.get(0),
            )
            .map_err(err)?;
        if exists == 0 {
            missing.push(table);
        }
    }
    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "E_CORRUPT: 数据库缺少核心表（{}），可能已损坏，请在启动提示中从备份恢复",
            missing.join("、")
        ))
    }
}

/// 上次异常退出（崩溃 / 强杀）遗留的 queued / running 运行标记为 error，
/// 避免界面永远显示「运行中」；用例级结果已落库的保持不变
fn recover_interrupted_runs(pool: &DbPool) -> Result<(), String> {
    let conn = pool.get().map_err(err)?;
    conn.execute(
        "UPDATE test_runs SET status = 'error', finished_at = ?1
         WHERE status IN ('queued', 'running')",
        rusqlite::params![crate::util::now_ms()],
    )
    .map_err(err)?;
    Ok(())
}

/// v17 / R3：崩溃恢复。运行期每秒把采样追加进 `stress_job_samples`，
/// 正常结束由 `save_report` 在同一事务里标记 `saved` 并删除临时样本；
/// 启动时看到 `running` / `failed` 的任务头，就用已落盘的样本生成「不完整」报告。
///
/// 不自动续跑：续跑会产生时间轴断裂的报告，误导性大于价值。
fn recover_interrupted_stress_jobs(pool: &DbPool) -> Result<(), String> {
    let conn = pool.get().map_err(err)?;

    // 报告已提交、只差清理任务头：直接收尾，不重复生成报告
    conn.execute(
        "DELETE FROM stress_job_samples
         WHERE job_id IN (SELECT job_id FROM stress_job_meta WHERE status = 'saved')",
        [],
    )
    .map_err(err)?;
    conn.execute("DELETE FROM stress_job_meta WHERE status = 'saved'", [])
        .map_err(err)?;

    type PendingStressJob = (i64, Option<i64>, Option<String>, String, i64);
    let jobs: Vec<PendingStressJob> = {
        let mut stmt = conn
            .prepare(
                "SELECT job_id, plan_id, plan_name, config_json, started_at
                 FROM stress_job_meta WHERE status IN ('running', 'failed')",
            )
            .map_err(err)?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, Option<i64>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            })
            .map_err(err)?;
        rows.filter_map(Result::ok).collect()
    };

    for (job_id, plan_id, plan_name, config_json, started_at) in jobs {
        let samples = match load_job_samples(&conn, job_id) {
            Ok(samples) => samples,
            Err(error) => {
                log::warn!("读取中断压测 #{} 的采样失败：{error}", job_id);
                continue;
            }
        };
        if samples.is_empty() {
            // 启动后立刻被强杀：没有任何可用数据，清掉任务头即可
            let _ = conn.execute(
                "DELETE FROM stress_job_meta WHERE job_id = ?1",
                rusqlite::params![job_id],
            );
            continue;
        }
        if let Err(error) = build_partial_report(
            &conn,
            plan_id,
            plan_name.as_deref(),
            &config_json,
            started_at,
            &samples,
        ) {
            // 保留现场，下次启动再试；宁可不生成，也不要写出半条坏报告
            log::warn!("恢复中断压测 #{} 失败：{error}", job_id);
            continue;
        }
        let _ = conn.execute(
            "DELETE FROM stress_job_samples WHERE job_id = ?1",
            rusqlite::params![job_id],
        );
        let _ = conn.execute(
            "DELETE FROM stress_job_meta WHERE job_id = ?1",
            rusqlite::params![job_id],
        );
        log::info!(
            "已把中断压测 #{} 恢复为不完整报告（{} 个采样点）",
            job_id,
            samples.len()
        );
    }
    Ok(())
}

fn load_job_samples(
    conn: &rusqlite::Connection,
    job_id: i64,
) -> Result<Vec<crate::models::ReportSample>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT elapsed_sec, concurrent, qps, avg_rt, p50_rt, p95_rt, p99_rt,
                    ttfb_avg, body_avg, body_p95, bytes, success_count, error_count
             FROM stress_job_samples WHERE job_id = ?1 ORDER BY elapsed_sec ASC",
        )
        .map_err(err)?;
    let samples = stmt
        .query_map(rusqlite::params![job_id], |row| {
            Ok(crate::models::ReportSample {
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
            })
        })
        .map_err(err)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(err)?;
    Ok(samples)
}

/// 用已落盘采样拼一份「不完整」报告：状态 `interrupted`，配置快照标记 `partial`。
/// 分位与均值只能按每秒样本加权回算，因此不参与基线对比，健康结论由前端强制 fail。
fn build_partial_report(
    conn: &rusqlite::Connection,
    plan_id: Option<i64>,
    plan_name: Option<&str>,
    config_json: &str,
    started_at: i64,
    samples: &[crate::models::ReportSample],
) -> Result<(), String> {
    use crate::models::ReportSample;

    let requests: i64 = samples
        .iter()
        .map(|s| (s.success_count + s.error_count) as i64)
        .sum();
    let success: i64 = samples.iter().map(|s| s.success_count as i64).sum();
    let errors: i64 = samples.iter().map(|s| s.error_count as i64).sum();
    let bytes: i64 = samples.iter().map(|s| s.bytes as i64).sum();
    let last_sec = samples.iter().map(|s| s.elapsed_sec).max().unwrap_or(0);
    let max_qps = samples.iter().map(|s| s.qps).fold(0.0, f64::max);
    let avg_qps = if last_sec > 0 {
        requests as f64 / last_sec as f64
    } else {
        0.0
    };
    let weighted = |get: fn(&ReportSample) -> f64| -> f64 {
        if requests > 0 {
            samples
                .iter()
                .map(|s| get(s) * (s.success_count + s.error_count) as f64)
                .sum::<f64>()
                / requests as f64
        } else {
            0.0
        }
    };
    let min_rt = samples
        .iter()
        .map(|s| s.avg_rt)
        .reduce(f64::min)
        .unwrap_or(0.0);
    let max_rt = samples
        .iter()
        .map(|s| s.avg_rt)
        .reduce(f64::max)
        .unwrap_or(0.0);
    let concurrency = samples.last().map(|s| s.concurrent as i64).unwrap_or(0);

    let mut config: serde_json::Value =
        serde_json::from_str(config_json).unwrap_or_else(|_| serde_json::json!({}));
    if let Some(map) = config.as_object_mut() {
        map.insert("partial".to_string(), serde_json::json!(true));
        map.insert(
            "partialReason".to_string(),
            serde_json::json!("运行中断，已恢复已落盘采样（未续跑）"),
        );
        map.insert(
            "restoredAt".to_string(),
            serde_json::json!(crate::util::now_ms()),
        );
        map.insert("dataCutoffSec".to_string(), serde_json::json!(last_sec));
        map.insert(
            "restoredSamples".to_string(),
            serde_json::json!(samples.len()),
        );
    }

    let end_time = started_at + (last_sec as i64) * 1000;
    conn.execute(
        "INSERT INTO test_reports
            (plan_id, plan_name, total_requests, success_count, error_count,
             avg_rt, min_rt, max_rt, p50_rt, p75_rt, p90_rt, p95_rt, p99_rt, p999_rt,
             stddev_rt, ttfb_avg, ttfb_p95, apdex, bytes_total, max_qps, avg_qps,
             target_rps, error_rate, concurrency, status, config_json, start_time, end_time, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14,
                 ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, 'interrupted',
                 ?25, ?26, ?27, ?28)",
        rusqlite::params![
            plan_id,
            plan_name.unwrap_or("压测（运行中断）"),
            requests,
            success,
            errors,
            weighted(|s| s.avg_rt),
            min_rt,
            max_rt,
            weighted(|s| s.p50_rt),
            weighted(|s| s.p95_rt),
            weighted(|s| s.p95_rt),
            weighted(|s| s.p95_rt),
            weighted(|s| s.p99_rt),
            weighted(|s| s.p99_rt),
            0.0,
            weighted(|s| s.ttfb_avg),
            weighted(|s| s.ttfb_avg),
            0.0,
            bytes,
            max_qps,
            avg_qps,
            0.0,
            if requests > 0 {
                errors as f64 / requests as f64 * 100.0
            } else {
                0.0
            },
            concurrency,
            config.to_string(),
            started_at,
            end_time,
            crate::util::now_ms(),
        ],
    )
    .map_err(err)?;
    let report_id = conn.last_insert_rowid();

    let mut stmt = conn
        .prepare(
            "INSERT INTO report_samples
                (report_id, elapsed_sec, concurrent, qps, avg_rt, p50_rt, p95_rt, p99_rt,
                 ttfb_avg, body_avg, body_p95, bytes, success_count, error_count)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        )
        .map_err(err)?;
    for sample in samples {
        stmt.execute(rusqlite::params![
            report_id,
            sample.elapsed_sec as i64,
            sample.concurrent as i64,
            sample.qps,
            sample.avg_rt,
            sample.p50_rt,
            sample.p95_rt,
            sample.p99_rt,
            sample.ttfb_avg,
            sample.body_avg,
            sample.body_p95,
            sample.bytes as i64,
            sample.success_count as i64,
            sample.error_count as i64,
        ])
        .map_err(err)?;
    }
    Ok(())
}

/// 存量表补列清单：(表, 列, 列定义)
///
/// 改成结构化清单而不是整条 SQL，是为了在执行前先 `PRAGMA table_info` 判存：
/// 老代码用 `let _ = conn.execute(...)` 吞掉所有错误，真实的「表不存在 / 库只读 /
/// 磁盘满」也被一并吞掉，用户只会看到功能莫名失效（文档 4.0.3 / 7.2）。
const COLUMN_MIGRATIONS: &[(&str, &str, &str)] = &[
    ("test_reports", "p75_rt", "REAL DEFAULT 0"),
    ("test_reports", "p90_rt", "REAL DEFAULT 0"),
    ("test_reports", "p999_rt", "REAL DEFAULT 0"),
    ("test_reports", "stddev_rt", "REAL DEFAULT 0"),
    ("test_reports", "ttfb_avg", "REAL DEFAULT 0"),
    ("test_reports", "ttfb_p95", "REAL DEFAULT 0"),
    ("test_reports", "apdex", "REAL DEFAULT 0"),
    ("test_reports", "bytes_total", "INTEGER DEFAULT 0"),
    ("test_reports", "target_rps", "REAL DEFAULT 0"),
    ("test_reports", "config_json", "TEXT"),
    ("test_reports", "stages_json", "TEXT"),
    ("test_reports", "codes_json", "TEXT"),
    ("test_reports", "histogram_json", "TEXT"),
    ("test_reports", "error_types_json", "TEXT"),
    // v1.0.8 压测报告：可编辑的场景描述与总结
    ("test_reports", "scenario_text", "TEXT"),
    ("test_reports", "summary_text", "TEXT"),
    // v17：外部监控指标序列（R5），列表查询不带该列，详情读取
    ("test_reports", "external_metrics_json", "TEXT"),
    // v17：并发透视的每秒 worker 状态分布（R8），仅详情读取
    ("test_reports", "worker_states_json", "TEXT"),
    ("report_samples", "p50_rt", "REAL DEFAULT 0"),
    ("report_samples", "ttfb_avg", "REAL DEFAULT 0"),
    ("report_samples", "bytes", "INTEGER DEFAULT 0"),
    ("report_requests", "body", "TEXT"),
    ("report_requests", "params", "TEXT"),
    ("report_requests", "req_headers", "TEXT"),
    ("report_requests", "res_headers", "TEXT"),
    ("report_requests", "res_body", "TEXT"),
    ("notes", "deleted_from_status", "TEXT"),
    ("test_plans", "config_json", "TEXT"),
    ("test_plans", "updated_at", "INTEGER DEFAULT 0"),
    ("ai_providers", "kind", "TEXT NOT NULL DEFAULT 'openai'"),
    ("ai_messages", "reasoning", "TEXT"),
    ("ai_usage_logs", "request_preview", "TEXT"),
    ("ai_providers", "max_tokens", "INTEGER DEFAULT 0"),
    ("ai_providers", "timeout_secs", "INTEGER DEFAULT 300"),
    ("ai_sessions", "pinned", "INTEGER NOT NULL DEFAULT 0"),
    ("ai_sessions", "summary", "TEXT"),
    ("ai_sessions", "summary_until", "INTEGER DEFAULT 0"),
    ("ai_messages", "token_count", "INTEGER DEFAULT 0"),
    ("ai_tool_logs", "round", "INTEGER DEFAULT 0"),
    ("ai_tool_logs", "trace_id", "TEXT"),
    ("ai_tool_logs", "timeout", "INTEGER DEFAULT 0"),
    // W5：AI 写操作执行前的快照关联同一次提问，用于按 trace_id 聚合回滚
    ("request_revisions", "trace_id", "TEXT"),
    // v1.0.8：执行记录来源（manual | batch | suite | cli），老数据默认手工
    (
        "request_history",
        "origin",
        "TEXT NOT NULL DEFAULT 'manual'",
    ),
    ("request_runs", "origin", "TEXT NOT NULL DEFAULT 'manual'"),
    // v1.1.0：运行记录列表只返回摘要，体积与截断标记用于详情提示
    (
        "request_runs",
        "request_body_size",
        "INTEGER NOT NULL DEFAULT 0",
    ),
    (
        "request_runs",
        "response_body_size",
        "INTEGER NOT NULL DEFAULT 0",
    ),
    (
        "request_runs",
        "request_body_truncated",
        "INTEGER NOT NULL DEFAULT 0",
    ),
    (
        "request_runs",
        "response_body_truncated",
        "INTEGER NOT NULL DEFAULT 0",
    ),
    // v19：计划级编排定义（cluster_jobs 在 v16 建表，老库需补列）
    ("cluster_jobs", "orchestration_json", "TEXT"),
    // v1.0.8：耗时分解、响应体类型、按请求网络配置
    ("request_history", "timings_json", "TEXT"),
    ("request_history", "body_kind", "TEXT"),
    ("request_history", "body_file", "TEXT"),
    ("requests", "net_json", "TEXT"),
    // 各 Body 类型的草稿，切换类型时不丢失内容
    ("requests", "body_meta", "TEXT"),
    // v1.0.9：记事本软删除、完成时间与截图识别标记
    ("notes", "done_at", "INTEGER"),
    ("notes", "deleted_at", "INTEGER"),
    ("notes", "deleted_from_status", "TEXT"),
    (
        "ai_providers",
        "supports_vision",
        "INTEGER NOT NULL DEFAULT 0",
    ),
    // v1.1.1：用户显式覆盖模型上下文窗口，0 = 自动识别
    (
        "ai_providers",
        "context_window",
        "INTEGER NOT NULL DEFAULT 0",
    ),
    // v1.1.2：接口实体（请求 / 返回字段树，结构 JSON，不含取值）
    ("requests", "request_entity", "TEXT"),
    ("requests", "response_entity", "TEXT"),
    // v1.1.3：环境是否生产（AI-01 生产环境强制确认）
    ("environments", "is_prod", "INTEGER NOT NULL DEFAULT 0"),
    ("environments", "script", "TEXT"),
    ("requests", "auth", "TEXT"),
    // v12：请求类型（http | graphql | sse | websocket），与发送时的 HTTP 版本协议无关
    ("requests", "protocol", "TEXT NOT NULL DEFAULT 'http'"),
    // v13：AI 专项——原生工具开关（-1 自动 / 0 关闭 / 1 开启）、单价覆盖与币种
    (
        "ai_providers",
        "native_tools",
        "INTEGER NOT NULL DEFAULT -1",
    ),
    ("ai_providers", "input_price", "REAL NOT NULL DEFAULT 0"),
    ("ai_providers", "output_price", "REAL NOT NULL DEFAULT 0"),
    ("ai_providers", "currency", "TEXT NOT NULL DEFAULT 'USD'"),
    // v15：压测报告分组、响应体耗时直方图与 TopN 慢请求
    ("test_reports", "run_group_id", "INTEGER"),
    ("test_reports", "body_histogram_json", "TEXT"),
    ("test_reports", "slow_requests_json", "TEXT"),
    ("report_samples", "body_avg", "REAL DEFAULT 0"),
    ("report_samples", "body_p95", "REAL DEFAULT 0"),
];

fn column_exists(conn: &rusqlite::Connection, table: &str, column: &str) -> Result<bool, String> {
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(err)?;
    let mut rows = stmt.query([]).map_err(err)?;
    while let Some(row) = rows.next().map_err(err)? {
        let name: String = row.get(1).map_err(err)?;
        if name.eq_ignore_ascii_case(column) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// 判存式补列：只有「列已存在」被忽略，其它错误一律上报
pub fn add_column_if_missing(
    conn: &rusqlite::Connection,
    table: &str,
    column: &str,
    ddl: &str,
) -> Result<bool, String> {
    if column_exists(conn, table, column)? {
        return Ok(false);
    }
    conn.execute(
        &format!("ALTER TABLE {table} ADD COLUMN {column} {ddl}"),
        [],
    )
    .map_err(|e| format!("E_SCHEMA: 为 {table} 添加列 {column} 失败：{e}"))?;
    Ok(true)
}

/// v15：重建 test_plans，把 request_id 改为可空并改用 `ON DELETE SET NULL`
///
/// SQLite 不支持修改已有外键动作，只能整表重建。重建期间必须先关闭外键：
/// 否则 `DROP TABLE test_plans` 会按旧的 CASCADE 把 test_cases 的用例关联一并删掉。
/// 计划在接口被删除后保留，界面回退展示 URL 模板。
fn rebuild_test_plans_v15(conn: &rusqlite::Connection) -> Result<(), String> {
    let fk_was_on = conn
        .query_row("PRAGMA foreign_keys", [], |row| row.get::<_, i64>(0))
        .map(|value| value != 0)
        .unwrap_or(true);
    conn.execute_batch("PRAGMA foreign_keys = OFF")
        .map_err(err)?;
    let result = (|| -> Result<(), String> {
        let tx = conn.unchecked_transaction().map_err(err)?;
        tx.execute_batch(
            "DROP TABLE IF EXISTS test_plans_v15;
             CREATE TABLE test_plans_v15 (
                 id              INTEGER PRIMARY KEY AUTOINCREMENT,
                 project_id      INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
                 request_id      INTEGER REFERENCES requests(id) ON DELETE SET NULL,
                 name            TEXT NOT NULL,
                 concurrent      INTEGER DEFAULT 10,
                 duration_sec    INTEGER DEFAULT 60,
                 ramp_up_sec     INTEGER DEFAULT 10,
                 think_time_ms   INTEGER DEFAULT 0,
                 timeout_ms      INTEGER DEFAULT 30000,
                 params_config   TEXT,
                 config_json     TEXT,
                 created_at      INTEGER NOT NULL,
                 updated_at      INTEGER NOT NULL DEFAULT 0
             );
             INSERT INTO test_plans_v15
                 (id, project_id, request_id, name, concurrent, duration_sec, ramp_up_sec,
                  think_time_ms, timeout_ms, params_config, config_json, created_at, updated_at)
             SELECT id, project_id, request_id, name, concurrent, duration_sec, ramp_up_sec,
                    think_time_ms, timeout_ms, params_config, config_json, created_at, updated_at
             FROM test_plans;
             DROP TABLE test_plans;
             ALTER TABLE test_plans_v15 RENAME TO test_plans;",
        )
        .map_err(err)?;
        tx.commit().map_err(err)?;
        Ok(())
    })();
    if fk_was_on {
        let _ = conn.execute_batch("PRAGMA foreign_keys = ON");
    }
    result?;
    // 重建后做一次外键体检：只记日志，历史脏数据不应该阻断升级
    if fk_was_on {
        if let Ok(mut stmt) = conn.prepare("PRAGMA foreign_key_check") {
            if let Ok(rows) = stmt.query_map([], |row| row.get::<_, String>(0)) {
                let dirty = rows.filter_map(Result::ok).count();
                if dirty > 0 {
                    log::warn!("数据库存在 {dirty} 条外键不一致记录（升级不阻断）");
                }
            }
        }
    }
    Ok(())
}

/// 老版本数据库补列 + 补索引 + 版本号推进，整体包在一个事务里
fn migrate(pool: &DbPool) -> Result<(), String> {
    let conn = pool.get().map_err(err)?;
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(err)?;

    // v15 需要整表重建 test_plans：重建必须关掉外键且不能包在外层事务里
    if version > 0 && version < 15 {
        rebuild_test_plans_v15(&conn)?;
    }

    {
        let tx = conn.unchecked_transaction().map_err(err)?;
        for (table, column, ddl) in COLUMN_MIGRATIONS {
            add_column_if_missing(&tx, table, column, ddl)?;
        }
        // 依赖 updated_at 的索引必须在补列之后创建，否则老库会在建索引时报「no such column」
        tx.execute(
            "CREATE INDEX IF NOT EXISTS idx_plans_project ON test_plans(project_id, updated_at DESC)",
            [],
        )
        .map_err(err)?;
        // 依赖新增列的索引必须放这里：schema.sql 先于补列执行，老库直接建会报 no such column
        tx.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_history_origin ON request_history(origin, created_at DESC);
             CREATE INDEX IF NOT EXISTS idx_runs_origin ON request_runs(origin, created_at DESC);
             CREATE INDEX IF NOT EXISTS idx_notes_status_done ON notes(status, done_at DESC);
             CREATE INDEX IF NOT EXISTS idx_revisions_trace ON request_revisions(trace_id, id);
             CREATE INDEX IF NOT EXISTS idx_reports_run_group ON test_reports(run_group_id, id DESC);",
        )
        .map_err(err)?;
        if version < 6 {
            // 旧库的 kind 一直是占位值：有截止时间的视为待办，纯文字仍留在笔记。
            // 未知值统一收敛到待办，避免前端筛选漏数据。
            tx.execute_batch(
                "UPDATE notes SET kind = 'todo' WHERE kind = 'note' AND due_at IS NOT NULL;
                 UPDATE notes SET kind = 'todo'
                 WHERE kind NOT IN ('todo', 'note', 'idea', 'reference');",
            )
            .map_err(err)?;
        }
        if version < DB_VERSION {
            tx.execute_batch(&format!("PRAGMA user_version = {DB_VERSION}"))
                .map_err(err)?;
        }
        tx.commit().map_err(err)?;
    }

    if version > 0 && version < DB_VERSION {
        log::info!("数据库已从结构版本 {version} 升级到 {DB_VERSION}");
    }
    Ok(())
}

/// 读取 app_meta 中的一项（表不存在时返回 None，便于迁移早期调用）
pub fn meta_get(conn: &rusqlite::Connection, key: &str) -> Option<String> {
    conn.query_row("SELECT value FROM app_meta WHERE key = ?1", [key], |row| {
        row.get::<_, Option<String>>(0)
    })
    .ok()
    .flatten()
}

pub fn meta_set(conn: &rusqlite::Connection, key: &str, value: &str) -> Result<(), String> {
    conn.execute(
        "INSERT INTO app_meta (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        rusqlite::params![key, value],
    )
    .map_err(err)?;
    Ok(())
}

/// 诊断包与备份列表用：各表行数
pub fn table_counts(conn: &rusqlite::Connection) -> Vec<(String, i64)> {
    let mut names = Vec::new();
    if let Ok(mut stmt) = conn.prepare(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
    ) {
        if let Ok(rows) = stmt.query_map([], |row| row.get::<_, String>(0)) {
            names = rows.filter_map(Result::ok).collect();
        }
    }
    names
        .into_iter()
        .map(|name| {
            let count: i64 = conn
                .query_row(&format!("SELECT COUNT(*) FROM \"{name}\""), [], |row| {
                    row.get(0)
                })
                .unwrap_or(0);
            (name, count)
        })
        .collect()
}

/// 首次启动写入示例数据；已有数据库则按需补齐示例场景
fn seed_database(pool: &DbPool) -> Result<(), String> {
    let conn = pool.get().map_err(err)?;

    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM projects", [], |row| row.get(0))
        .map_err(err)?;

    if count == 0 {
        seed::seed(&conn)?;
    } else {
        seed::upgrade_sample(&conn)?;
    }
    // 示例测试套件幂等补齐：新库与老库升级都能看到可直接运行的回归例子
    seed::ensure_sample_suite(&conn)
}

#[cfg(test)]
mod tests {
    use super::*;
    use r2d2_sqlite::SqliteConnectionManager;
    use rusqlite::params;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_pool(tag: &str) -> (PathBuf, DbPool) {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir =
            std::env::temp_dir().join(format!("astroman-db-{tag}-{}-{stamp}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let manager = SqliteConnectionManager::file(dir.join("astroman.db"))
            .with_init(|conn| conn.execute_batch(&connection_pragmas()));
        let pool = r2d2::Pool::builder().max_size(2).build(manager).unwrap();
        (dir, pool)
    }

    #[test]
    fn crash_recovery_builds_partial_report() {
        let (dir, pool) = temp_pool("recover-stress");
        init(&pool).unwrap();
        {
            let conn = pool.get().unwrap();
            conn.execute(
                "INSERT INTO stress_job_meta
                    (job_id, plan_id, plan_name, config_json, started_at, status)
                 VALUES (42, NULL, 'crash', '{\"mode\":\"fixed\"}', 1000, 'running')",
                [],
            )
            .unwrap();
            for sec in 1..=5 {
                conn.execute(
                    "INSERT INTO stress_job_samples
                        (job_id, elapsed_sec, concurrent, qps, avg_rt, p50_rt, p95_rt, p99_rt,
                         ttfb_avg, body_avg, body_p95, bytes, success_count, error_count, created_at)
                     VALUES (42, ?1, 10, 100, 20, 18, 40, 60, 8, 5, 9, 1000, 9, 1, 1000)",
                    params![sec],
                )
                .unwrap();
            }
        }
        recover_interrupted_stress_jobs(&pool).unwrap();

        let conn = pool.get().unwrap();
        let (status, total, config_text): (String, i64, String) = conn
            .query_row(
                "SELECT status, total_requests, config_json FROM test_reports
                 WHERE plan_name = 'crash'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(status, "interrupted");
        assert_eq!(total, 50);
        let config: serde_json::Value = serde_json::from_str(&config_text).unwrap();
        assert_eq!(config["partial"], true);
        assert_eq!(config["dataCutoffSec"], 5);

        let samples: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM report_samples
                 WHERE report_id = (SELECT id FROM test_reports WHERE plan_name = 'crash')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(samples, 5);

        let leftovers: i64 = conn
            .query_row(
                "SELECT (SELECT COUNT(*) FROM stress_job_meta WHERE job_id = 42)
                        + (SELECT COUNT(*) FROM stress_job_samples WHERE job_id = 42)",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(leftovers, 0, "恢复后必须删除临时行");
        drop(conn);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn saved_job_is_cleaned_without_duplicate_report() {
        let (dir, pool) = temp_pool("recover-saved");
        init(&pool).unwrap();
        {
            let conn = pool.get().unwrap();
            conn.execute(
                "INSERT INTO stress_job_meta
                    (job_id, plan_id, plan_name, config_json, started_at, status)
                 VALUES (7, NULL, 'saved', '{}', 1000, 'saved')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO stress_job_samples
                    (job_id, elapsed_sec, concurrent, qps, created_at)
                 VALUES (7, 1, 1, 1, 1000)",
                [],
            )
            .unwrap();
        }
        recover_interrupted_stress_jobs(&pool).unwrap();
        let conn = pool.get().unwrap();
        let reports: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM test_reports WHERE plan_name = 'saved'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(reports, 0, "已保存的任务不应再生成报告");
        let rows: i64 = conn
            .query_row("SELECT COUNT(*) FROM stress_job_samples", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(rows, 0);
        drop(conn);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn connection_pragmas_apply_cache_and_synchronous() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(&connection_pragmas()).unwrap();
        let cache: i64 = conn
            .query_row("PRAGMA cache_size", [], |row| row.get(0))
            .unwrap();
        let synchronous: i64 = conn
            .query_row("PRAGMA synchronous", [], |row| row.get(0))
            .unwrap();
        assert_eq!(cache, -16000);
        // NORMAL = 1；WAL 下不需要每次提交都 fsync
        assert_eq!(synchronous, 1);
    }

    fn has_column(pool: &DbPool, table: &str, column: &str) -> bool {
        let conn = pool.get().unwrap();
        let mut stmt = conn
            .prepare(&format!("PRAGMA table_info({table})"))
            .unwrap();
        let found = stmt
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .filter_map(Result::ok)
            .any(|name| name == column);
        found
    }

    #[test]
    fn init_is_idempotent_and_creates_test_tables() {
        let (dir, pool) = temp_pool("init");
        init(&pool).unwrap();
        // 重复执行不应报错（新建表幂等 + ALTER 已存在则忽略）
        init(&pool).unwrap();

        let conn = pool.get().unwrap();
        for table in [
            "test_suites",
            "test_cases",
            "test_datasets",
            "test_runs",
            "test_run_cases",
            "test_run_assertions",
            "test_artifacts",
            "notes",
        ] {
            let count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                    [table],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(count, 1, "缺少表 {table}");
        }

        assert!(has_column(&pool, "request_history", "origin"));
        assert!(has_column(&pool, "request_runs", "origin"));
        for column in [
            "request_body_size",
            "response_body_size",
            "request_body_truncated",
            "response_body_truncated",
        ] {
            assert!(
                has_column(&pool, "request_runs", column),
                "运行记录缺少列 {column}"
            );
        }
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, DB_VERSION);
        drop(conn);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn bootstrap_encrypts_legacy_request_auth_and_is_idempotent() {
        let (dir, pool) = temp_pool("auth-migrate");
        init(&pool).unwrap();
        let now = crate::util::now_ms();
        let conn = pool.get().unwrap();
        conn.execute(
            "INSERT INTO projects (name, created_at, updated_at) VALUES ('p', ?1, ?1)",
            params![now],
        )
        .unwrap();
        let project_id = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO collections (project_id, name, created_at, updated_at)
             VALUES (?1, 'c', ?2, ?2)",
            params![project_id, now],
        )
        .unwrap();
        let collection_id = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO requests (collection_id, name, method, url, auth, created_at, updated_at)
             VALUES (?1, 'r', 'GET', 'https://example.com', ?2, ?3, ?3)",
            params![
                collection_id,
                r#"{"kind":"bearer","token":"tk-legacy","username":"alice"}"#,
                now
            ],
        )
        .unwrap();
        let request_id = conn.last_insert_rowid();
        drop(conn);

        // 启动时 bootstrap 会把历史明文认证就地加密
        let store = crate::secret::SecretStore::bootstrap(&dir, &pool).unwrap();
        let conn = pool.get().unwrap();
        let stored: String = conn
            .query_row(
                "SELECT auth FROM requests WHERE id = ?1",
                params![request_id],
                |row| row.get(0),
            )
            .unwrap();
        assert!(stored.contains("enc:v1:"), "历史明文认证必须被加密");
        assert!(!stored.contains("tk-legacy"), "token 不能以明文留在库里");
        assert!(stored.contains("alice"), "非敏感字段保持明文");
        drop(conn);

        // 幂等：再次迁移不应继续套加密
        assert_eq!(store.encrypt_plaintext(&pool).unwrap(), 0);
        assert!(store.decrypt_auth(&stored).contains("tk-legacy"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn seeds_sample_suite_with_runnable_cases() {
        let (dir, pool) = temp_pool("sample-suite");
        init(&pool).unwrap();
        let conn = pool.get().unwrap();
        let suite_id: i64 = conn
            .query_row(
                "SELECT id FROM test_suites WHERE name = '示例回归（httpbin）'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let cases: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM test_cases WHERE suite_id = ?1 AND enabled = 1",
                params![suite_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(cases, 3, "示例套件应包含 3 个可直接运行的用例");
        let with_assertions: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM test_cases WHERE suite_id = ?1
                   AND assertions_json IS NOT NULL AND assertions_json != '[]'",
                params![suite_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(with_assertions, 3);
        drop(conn);

        // 重复初始化不应重复插入
        init(&pool).unwrap();
        let conn = pool.get().unwrap();
        let suites: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM test_suites WHERE name = '示例回归（httpbin）'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(suites, 1);
        drop(conn);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn creates_contract_and_revision_tables_with_cascade() {
        let (dir, pool) = temp_pool("w1-tables");
        init(&pool).unwrap();
        // 重复初始化保持幂等
        init(&pool).unwrap();

        // v11 起追加 AI 治理表；这里只保证资产闭环所需的 v10 不会被回退
        const { assert!(DB_VERSION >= 10, "W1 资产闭环需要 v10 及以上") };
        let conn = pool.get().unwrap();
        for table in ["request_revisions", "contract_snapshots"] {
            let count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                    [table],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(count, 1, "缺少表 {table}");
        }

        // 请求被删除时，版本历史随之级联清理
        let now = crate::util::now_ms();
        conn.execute(
            "INSERT INTO projects (name, created_at, updated_at) VALUES ('p', ?1, ?1)",
            params![now],
        )
        .unwrap();
        let project_id = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO collections (project_id, name, created_at, updated_at)
             VALUES (?1, 'c', ?2, ?2)",
            params![project_id, now],
        )
        .unwrap();
        let collection_id = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO requests (collection_id, name, method, url, created_at, updated_at)
             VALUES (?1, 'r', 'GET', 'https://example.com', ?2, ?2)",
            params![collection_id, now],
        )
        .unwrap();
        let request_id = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO request_revisions
               (request_id, revision, snapshot_json, source, created_at, truncated)
             VALUES (?1, 1, '{}', 'save', ?2, 0)",
            params![request_id, now],
        )
        .unwrap();
        conn.execute("DELETE FROM requests WHERE id = ?1", params![request_id])
            .unwrap();
        let left: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM request_revisions WHERE request_id = ?1",
                params![request_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(left, 0, "删除请求必须级联删除版本历史");
        drop(conn);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn upgrades_v14_plans_to_set_null_and_adds_capacity_table() {
        let (dir, pool) = temp_pool("v15");
        {
            let conn = pool.get().unwrap();
            // 模拟 v14：plan.request_id 非空且 CASCADE；同时存在引用计划的套件用例
            conn.execute_batch(
                "PRAGMA foreign_keys = OFF;
                 CREATE TABLE projects (
                     id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL,
                     description TEXT, color TEXT, sort_order INTEGER DEFAULT 0,
                     created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL);
                 CREATE TABLE collections (
                     id INTEGER PRIMARY KEY AUTOINCREMENT,
                     project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
                     parent_id INTEGER, name TEXT NOT NULL, sort_order INTEGER DEFAULT 0,
                     created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL);
                 CREATE TABLE requests (
                     id INTEGER PRIMARY KEY AUTOINCREMENT,
                     collection_id INTEGER NOT NULL REFERENCES collections(id) ON DELETE CASCADE,
                     name TEXT NOT NULL, method TEXT NOT NULL DEFAULT 'GET', url TEXT NOT NULL,
                     created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL);
                 CREATE TABLE test_plans (
                     id INTEGER PRIMARY KEY AUTOINCREMENT,
                     project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
                     request_id INTEGER NOT NULL REFERENCES requests(id) ON DELETE CASCADE,
                     name TEXT NOT NULL, concurrent INTEGER DEFAULT 10,
                     duration_sec INTEGER DEFAULT 60, ramp_up_sec INTEGER DEFAULT 10,
                     think_time_ms INTEGER DEFAULT 0, timeout_ms INTEGER DEFAULT 30000,
                     params_config TEXT, config_json TEXT,
                     created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL DEFAULT 0);
                 CREATE TABLE test_cases (
                     id INTEGER PRIMARY KEY AUTOINCREMENT, suite_id INTEGER NOT NULL,
                     name TEXT NOT NULL, sort_order INTEGER DEFAULT 0,
                     plan_id INTEGER REFERENCES test_plans(id) ON DELETE SET NULL);
                 INSERT INTO projects (name, created_at, updated_at) VALUES ('p', 1, 1);
                 INSERT INTO collections (project_id, name, created_at, updated_at)
                 VALUES (1, 'c', 1, 1);
                 INSERT INTO requests (collection_id, name, url, created_at, updated_at)
                 VALUES (1, 'r', 'http://127.0.0.1/x', 1, 1);
                 INSERT INTO test_plans (project_id, request_id, name, created_at)
                 VALUES (1, 1, '计划', 1);
                 INSERT INTO test_cases (suite_id, name, plan_id) VALUES (1, '用例', 1);
                 PRAGMA user_version = 14;",
            )
            .unwrap();
        }
        init(&pool).unwrap();
        let conn = pool.get().unwrap();

        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, DB_VERSION);

        // 删除接口后计划保留，request_id 置空
        conn.execute("DELETE FROM requests WHERE id = 1", [])
            .unwrap();
        let (plans, request_id): (i64, Option<i64>) = conn
            .query_row(
                "SELECT COUNT(*), MAX(request_id) FROM test_plans WHERE id = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(plans, 1, "删除接口不能连计划一起删掉");
        assert_eq!(request_id, None);
        // 用例对计划的关联不能被重建过程误删
        let plan_ref: Option<i64> = conn
            .query_row("SELECT plan_id FROM test_cases WHERE id = 1", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(plan_ref, Some(1));

        for (table, column) in [
            ("test_reports", "run_group_id"),
            ("test_reports", "body_histogram_json"),
            ("test_reports", "slow_requests_json"),
            ("report_samples", "body_avg"),
            ("report_samples", "body_p95"),
        ] {
            assert!(has_column(&pool, table, column), "缺少列 {table}.{column}");
        }
        let capacity: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'machine_capacity'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(capacity, 1);
        drop(conn);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn creates_cluster_tables_and_cascades_workers() {
        let (dir, pool) = temp_pool("cluster-v16");
        init(&pool).unwrap();
        // 重复初始化保持幂等
        init(&pool).unwrap();
        let conn = pool.get().unwrap();
        for table in ["cluster_jobs", "cluster_workers"] {
            let count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                    [table],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(count, 1, "缺少表 {table}");
        }
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, DB_VERSION);

        let now = crate::util::now_ms();
        conn.execute(
            "INSERT INTO cluster_jobs (created_at, status, master_node, plan_name, total_concurrent)
             VALUES (?1, 'pairing', 'master', '集群计划', 8)",
            params![now],
        )
        .unwrap();
        let job_id = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO cluster_workers
               (job_id, node_name, role, weight, concurrent, status, joined_at)
             VALUES (?1, 'slave-1', 'slave', 1, 4, 'ready', ?2)",
            params![job_id, now],
        )
        .unwrap();
        // 任务删除后节点记录随之级联清理
        conn.execute("DELETE FROM cluster_jobs WHERE id = ?1", params![job_id])
            .unwrap();
        let left: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM cluster_workers WHERE job_id = ?1",
                params![job_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(left, 0);
        drop(conn);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn upgrades_legacy_db_without_origin_columns() {
        let (dir, pool) = temp_pool("legacy");
        {
            // 模拟 v1.0.7 老库：存在历史表但没有 origin 列
            let conn = pool.get().unwrap();
            conn.execute_batch(
                "CREATE TABLE request_history (
                     id INTEGER PRIMARY KEY AUTOINCREMENT,
                     request_id INTEGER, request_name TEXT, method TEXT, url TEXT,
                     status_code INTEGER, duration_ms INTEGER, created_at INTEGER NOT NULL);
                 CREATE TABLE request_runs (
                     id INTEGER PRIMARY KEY AUTOINCREMENT,
                     request_id INTEGER NOT NULL, method TEXT NOT NULL, url TEXT NOT NULL,
                     headers TEXT, params TEXT, body_type TEXT DEFAULT 'none', body TEXT,
                     response_status INTEGER DEFAULT 0, response_status_text TEXT,
                     response_headers TEXT, response_body TEXT,
                     duration_ms INTEGER DEFAULT 0, size_bytes INTEGER DEFAULT 0,
                     error TEXT, created_at INTEGER NOT NULL);
                 INSERT INTO request_history (request_name, method, url, created_at)
                 VALUES ('旧记录', 'GET', 'http://localhost', 1);",
            )
            .unwrap();
        }
        init(&pool).unwrap();
        let conn = pool.get().unwrap();
        let origin: String = conn
            .query_row(
                "SELECT origin FROM request_history WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(origin, "manual");
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, DB_VERSION);
        drop(conn);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
