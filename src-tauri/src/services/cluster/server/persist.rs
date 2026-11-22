//! 集群任务的持久化：任务、节点、报告采样曲线
//!
//! 拆出 `mod.rs` 只为让会话状态机保持在可读范围；这里都是无状态函数，直接吃连接池。

use super::*;

/* ---------- 持久化 ---------- */

pub(super) struct WorkerRow {
    pub(super) name: String,
    pub(super) addr: String,
    pub(super) role: String,
    pub(super) weight: f64,
    pub(super) concurrent: u32,
    pub(super) status: String,
    pub(super) qps: f64,
    pub(super) cpu: Option<f64>,
    pub(super) mem_mb: Option<u64>,
    pub(super) clock_offset_ms: Option<f64>,
    pub(super) clock_uncertainty_ms: Option<f64>,
    pub(super) dropped_samples: u64,
    pub(super) joined_at: i64,
    pub(super) last_seen_at: Option<i64>,
    pub(super) error: Option<String>,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn insert_job(
    pool: &DbPool,
    created_at: i64,
    master_node: &str,
    plan_id: Option<i64>,
    plan_name: &str,
    port: u16,
    config_json: &str,
    pair_expires_at: i64,
    total_concurrent: u32,
) -> Result<i64, String> {
    let conn = pool.get().map_err(err)?;
    conn.execute(
        "INSERT INTO cluster_jobs (created_at, status, master_node, plan_id, plan_name, port,
            config_json, pair_code_expires_at, total_concurrent)
         VALUES (?1, 'pairing', ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            created_at,
            master_node,
            plan_id,
            plan_name,
            port,
            config_json,
            pair_expires_at,
            total_concurrent
        ],
    )
    .map_err(err)?;
    Ok(conn.last_insert_rowid())
}

pub(super) fn upsert_worker(pool: &DbPool, job_id: i64, node: &NodeState) -> Result<(), String> {
    update_worker(
        pool,
        job_id,
        &WorkerRow {
            name: node.name.clone(),
            addr: node.addr.clone(),
            role: node.role.clone(),
            weight: node.weight,
            concurrent: node.concurrent,
            status: node.status.clone(),
            qps: node.qps,
            cpu: node.cpu,
            mem_mb: node.mem_mb,
            clock_offset_ms: node.clock_offset_ms,
            clock_uncertainty_ms: node.clock_uncertainty_ms,
            dropped_samples: node.dropped_samples,
            joined_at: node.joined_at,
            last_seen_at: node.last_seen_at,
            error: node.error.clone(),
        },
    )
}

pub(super) fn update_worker(pool: &DbPool, job_id: i64, node: &WorkerRow) -> Result<(), String> {
    let conn = pool.get().map_err(err)?;
    conn.execute(
        "INSERT INTO cluster_workers (job_id, node_name, addr, role, weight, concurrent, status,
            qps, cpu, mem, clock_offset_ms, clock_uncertainty_ms, dropped_samples, joined_at,
            last_seen_at, error)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
         ON CONFLICT(job_id, node_name) DO UPDATE SET
            addr = excluded.addr,
            role = excluded.role,
            weight = excluded.weight,
            concurrent = excluded.concurrent,
            status = excluded.status,
            qps = excluded.qps,
            cpu = excluded.cpu,
            mem = excluded.mem,
            clock_offset_ms = excluded.clock_offset_ms,
            clock_uncertainty_ms = excluded.clock_uncertainty_ms,
            dropped_samples = excluded.dropped_samples,
            last_seen_at = excluded.last_seen_at,
            error = excluded.error",
        params![
            job_id,
            node.name,
            node.addr,
            node.role,
            node.weight,
            node.concurrent,
            node.status,
            node.qps,
            // 表里这些列是 NOT NULL DEFAULT 0：未采集到就直接写 0，不写 NULL
            node.cpu.unwrap_or(0.0),
            node.mem_mb.map(|value| value as i64).unwrap_or(0),
            node.clock_offset_ms.unwrap_or(0.0),
            node.clock_uncertainty_ms.unwrap_or(0.0),
            node.dropped_samples as i64,
            node.joined_at,
            node.last_seen_at,
            node.error
        ],
    )
    .map_err(err)?;
    Ok(())
}

pub(super) fn update_job(
    pool: &DbPool,
    job_id: i64,
    status: &str,
    message: Option<&str>,
    incomplete: bool,
    finished_at: Option<i64>,
) -> Result<(), String> {
    let conn = pool.get().map_err(err)?;
    conn.execute(
        "UPDATE cluster_jobs SET status = ?2, message = ?3, incomplete = ?4,
            finished_at = COALESCE(?5, finished_at) WHERE id = ?1",
        params![job_id, status, message, incomplete, finished_at],
    )
    .map_err(err)?;
    Ok(())
}

pub(super) fn update_job_finished(
    pool: &DbPool,
    job_id: i64,
    message: &str,
    incomplete: bool,
    report_id: Option<i64>,
) -> Result<(), String> {
    let conn = pool.get().map_err(err)?;
    conn.execute(
        "UPDATE cluster_jobs SET status = 'finished', message = ?2, incomplete = ?3,
            report_id = ?4, finished_at = ?5 WHERE id = ?1",
        params![job_id, message, incomplete, report_id, now_ms()],
    )
    .map_err(err)?;
    Ok(())
}

pub(super) fn update_job_start(pool: &DbPool, job_id: i64, at_ms: i64) -> Result<(), String> {
    let conn = pool.get().map_err(err)?;
    conn.execute(
        "UPDATE cluster_jobs SET status = 'running', start_at_ms = ?2 WHERE id = ?1",
        params![job_id, at_ms],
    )
    .map_err(err)?;
    Ok(())
}

pub(super) fn update_pair_expiry(
    pool: &DbPool,
    job_id: i64,
    expires_at: i64,
) -> Result<(), String> {
    let conn = pool.get().map_err(err)?;
    conn.execute(
        "UPDATE cluster_jobs SET pair_code_expires_at = ?2 WHERE id = ?1",
        params![job_id, expires_at],
    )
    .map_err(err)?;
    Ok(())
}

pub(super) fn update_weights(
    pool: &DbPool,
    job_id: i64,
    weights: &HashMap<String, f64>,
) -> Result<(), String> {
    let conn = pool.get().map_err(err)?;
    let current: Option<String> = conn
        .query_row(
            "SELECT config_json FROM cluster_jobs WHERE id = ?1",
            params![job_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(err)?;
    let text = match current {
        Some(text) => text,
        None => return Ok(()),
    };
    let mut value: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
    if !value.is_object() {
        value = serde_json::json!({});
    }
    value["weights"] = serde_json::to_value(weights).map_err(err)?;
    conn.execute(
        "UPDATE cluster_jobs SET config_json = ?2 WHERE id = ?1",
        params![job_id, value.to_string()],
    )
    .map_err(err)?;
    Ok(())
}

/// 启动时把上次异常退出遗留的任务标记为失败，避免列表长期显示运行中
pub fn recover_stale_jobs(pool: &DbPool) -> Result<usize, String> {
    let conn = pool.get().map_err(err)?;
    let now = now_ms();
    let jobs = conn
        .execute(
            "UPDATE cluster_jobs SET status = 'failed', message = '应用退出导致任务中断',
                finished_at = ?1, incomplete = 1
             WHERE status IN ('pairing', 'preparing', 'ready', 'running')",
            params![now],
        )
        .map_err(err)?;
    conn.execute(
        "UPDATE cluster_workers SET status = 'lost', error = '应用退出导致连接中断'
         WHERE status NOT IN ('done', 'lost', 'left', 'rejected')",
        [],
    )
    .map_err(err)?;
    Ok(jobs)
}

/// 把合并后的节点级曲线落库（v19），供跨报告按节点查询
pub(super) fn save_cluster_samples(
    db: &DbPool,
    report_id: i64,
    series: &[crate::services::cluster::protocol::NodeSeries],
) -> Result<(), String> {
    if series.is_empty() {
        return Ok(());
    }
    let mut conn = db.get().map_err(err)?;
    let tx = conn.transaction().map_err(err)?;
    {
        let mut stmt = tx
            .prepare(
                "INSERT INTO cluster_report_samples
                 (report_id, node, role, elapsed_sec, concurrent, qps, p95_rt, error_count,
                  cpu, host_cpu_percent, mem_mb, net_rx_kbps, net_tx_kbps)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            )
            .map_err(err)?;
        for node in series {
            for point in &node.points {
                stmt.execute(rusqlite::params![
                    report_id,
                    node.node,
                    node.role,
                    point.sec as i64,
                    point.concurrent as i64,
                    point.qps,
                    point.p95,
                    point.errors as i64,
                    point.cpu,
                    point.host_cpu_percent,
                    point.mem_mb as i64,
                    point.net_rx_kbps,
                    point.net_tx_kbps,
                ])
                .map_err(err)?;
            }
        }
    }
    tx.commit().map_err(err)?;
    Ok(())
}
