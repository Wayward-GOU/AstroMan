//! 集群历史：主节点任务记录与从节点参与记录
//!
//! 主节点历史直接复用 `cluster_jobs` / `cluster_workers`，这里只提供查询；
//! 从节点在 `cluster_slave_history` 里记录每次配对与执行，供本机回看。

use rusqlite::params;
use serde::Serialize;

use crate::state::DbPool;

/// 主节点任务列表项
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClusterJobSummary {
    pub id: i64,
    pub created_at: i64,
    pub finished_at: Option<i64>,
    pub status: String,
    pub plan_name: String,
    pub master_node: String,
    pub total_concurrent: u32,
    pub node_count: i64,
    pub avg_qps: Option<f64>,
    pub report_id: Option<i64>,
    pub incomplete: bool,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClusterJobPage {
    pub total: i64,
    pub items: Vec<ClusterJobSummary>,
}

/// 任务详情里的节点记录
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClusterWorkerRow {
    pub node_name: String,
    pub addr: String,
    pub role: String,
    pub weight: f64,
    pub concurrent: u32,
    pub status: String,
    pub qps: f64,
    pub cpu: Option<f64>,
    pub mem_mb: Option<u64>,
    pub clock_offset_ms: f64,
    pub clock_uncertainty_ms: f64,
    pub dropped_samples: i64,
    pub joined_at: i64,
    pub last_seen_at: Option<i64>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClusterJobDetail {
    pub job: ClusterJobSummary,
    pub nodes: Vec<ClusterWorkerRow>,
}

/// 从节点参与记录
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClusterSlaveHistoryItem {
    pub id: i64,
    pub created_at: i64,
    pub master_addr: String,
    pub master_node: String,
    pub master_job_id: Option<i64>,
    pub node_name: String,
    pub plan_name: String,
    pub join_mode: String,
    pub concurrent: u32,
    pub total_concurrent: u32,
    pub status: String,
    pub qps_avg: f64,
    pub cpu_peak: f64,
    pub mem_peak_mb: u64,
    /// 整机 CPU 使用率峰值（%）
    pub host_cpu_peak: f64,
    /// 网卡收发合计峰值（KB/s）
    pub net_peak_kbps: f64,
    pub started_at: Option<i64>,
    pub ended_at: Option<i64>,
    pub message: Option<String>,
}

/// 从节点历史的一次状态快照：只在状态变化时写库
#[derive(Debug, Clone)]
pub struct SlaveHistorySync {
    pub id: i64,
    pub master_node: String,
    pub master_job_id: Option<i64>,
    pub node_name: String,
    pub plan_name: String,
    pub concurrent: u32,
    pub total_concurrent: u32,
    pub status: String,
    pub qps_avg: f64,
    pub cpu_peak: f64,
    pub mem_peak_mb: u64,
    pub host_cpu_peak: f64,
    pub net_peak_kbps: f64,
    pub started_at: Option<i64>,
    pub ended_at: Option<i64>,
    pub message: Option<String>,
}

const JOB_SELECT: &str = "\
SELECT j.id, j.created_at, j.finished_at, j.status, j.plan_name, j.master_node,
       j.total_concurrent, j.report_id, j.incomplete, j.message,
       (SELECT COUNT(*) FROM cluster_workers w WHERE w.job_id = j.id) AS node_count,
       (SELECT r.avg_qps FROM test_reports r WHERE r.id = j.report_id) AS avg_qps
  FROM cluster_jobs j";

/// 主节点任务分页；`status` 与 `keyword` 传空串表示不过滤
pub fn list_jobs(
    pool: &DbPool,
    page: u32,
    page_size: u32,
    status: &str,
    keyword: &str,
) -> Result<ClusterJobPage, String> {
    let conn = pool.get().map_err(|error| error.to_string())?;
    let like = format!("%{}%", keyword.trim());
    let total: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM cluster_jobs
              WHERE (?1 = '' OR status = ?1)
                AND (?2 = '' OR plan_name LIKE ?2 OR master_node LIKE ?2)",
            params![status, like],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    let offset = page.saturating_sub(1) as i64 * page_size as i64;
    let mut statement = conn
        .prepare(&format!(
            "{JOB_SELECT}
              WHERE (?1 = '' OR j.status = ?1)
                AND (?2 = '' OR j.plan_name LIKE ?2 OR j.master_node LIKE ?2)
              ORDER BY j.created_at DESC
              LIMIT ?3 OFFSET ?4"
        ))
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(
            params![status, like, page_size as i64, offset],
            map_job_summary,
        )
        .map_err(|error| error.to_string())?;
    let mut items = Vec::new();
    for row in rows {
        items.push(row.map_err(|error| error.to_string())?);
    }
    Ok(ClusterJobPage { total, items })
}

fn map_job_summary(row: &rusqlite::Row<'_>) -> rusqlite::Result<ClusterJobSummary> {
    Ok(ClusterJobSummary {
        id: row.get(0)?,
        created_at: row.get(1)?,
        finished_at: row.get(2)?,
        status: row.get(3)?,
        plan_name: row.get(4)?,
        master_node: row.get(5)?,
        total_concurrent: row.get::<_, i64>(6)? as u32,
        report_id: row.get(7)?,
        incomplete: row.get::<_, i64>(8)? != 0,
        message: row.get(9)?,
        node_count: row.get(10)?,
        avg_qps: row.get(11)?,
    })
}

pub fn job_detail(pool: &DbPool, job_id: i64) -> Result<ClusterJobDetail, String> {
    let conn = pool.get().map_err(|error| error.to_string())?;
    let job = conn
        .query_row(
            &format!("{JOB_SELECT} WHERE j.id = ?1"),
            params![job_id],
            map_job_summary,
        )
        .map_err(|error| format!("任务不存在或已清理：{error}"))?;
    let mut statement = conn
        .prepare(
            "SELECT node_name, addr, role, weight, concurrent, status, qps, cpu, mem,
                    clock_offset_ms, clock_uncertainty_ms, dropped_samples,
                    joined_at, last_seen_at, error
               FROM cluster_workers
              WHERE job_id = ?1
              ORDER BY id",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(params![job_id], |row| {
            Ok(ClusterWorkerRow {
                node_name: row.get(0)?,
                addr: row.get(1)?,
                role: row.get(2)?,
                weight: row.get(3)?,
                concurrent: row.get::<_, i64>(4)? as u32,
                status: row.get(5)?,
                qps: row.get(6)?,
                cpu: row.get::<_, Option<f64>>(7)?.filter(|value| *value > 0.0),
                mem_mb: row.get::<_, Option<i64>>(8)?.map(|value| value as u64),
                clock_offset_ms: row.get(9)?,
                clock_uncertainty_ms: row.get(10)?,
                dropped_samples: row.get(11)?,
                joined_at: row.get(12)?,
                last_seen_at: row.get(13)?,
                error: row.get(14)?,
            })
        })
        .map_err(|error| error.to_string())?;
    let mut nodes = Vec::new();
    for row in rows {
        nodes.push(row.map_err(|error| error.to_string())?);
    }
    Ok(ClusterJobDetail { job, nodes })
}

/// 清理已结束的历史；`keep_days` 为 0 表示清空全部已结束任务
pub fn clear_jobs(pool: &DbPool, keep_days: i64) -> Result<u32, String> {
    let conn = pool.get().map_err(|error| error.to_string())?;
    let cutoff = if keep_days <= 0 {
        i64::MAX
    } else {
        crate::util::now_ms() - keep_days * 86_400_000
    };
    let removed = conn
        .execute(
            "DELETE FROM cluster_jobs
              WHERE status IN ('finished', 'failed', 'idle') AND created_at < ?1",
            params![cutoff],
        )
        .map_err(|error| error.to_string())?;
    Ok(removed as u32)
}

/* ---------- 从节点历史 ---------- */

/// 新建一条从节点参与记录，返回主键
pub fn insert_slave_history(pool: &DbPool, master_addr: &str, join_mode: &str) -> Option<i64> {
    let conn = pool.get().ok()?;
    conn.execute(
        "INSERT INTO cluster_slave_history (created_at, master_addr, join_mode, status)
         VALUES (?1, ?2, ?3, 'connecting')",
        params![crate::util::now_ms(), master_addr, join_mode],
    )
    .ok()?;
    Some(conn.last_insert_rowid())
}

/// 状态变化时覆盖写；失败只记日志，不影响协议主流程
pub fn sync_slave_history(pool: &DbPool, sync: &SlaveHistorySync) {
    let Ok(conn) = pool.get() else { return };
    if let Err(error) = conn.execute(
        "UPDATE cluster_slave_history
            SET master_node = ?2, master_job_id = ?3, node_name = ?4, plan_name = ?5,
                concurrent = ?6, total_concurrent = ?7, status = ?8, qps_avg = ?9,
                cpu_peak = ?10, mem_peak_mb = ?11, started_at = ?12, ended_at = ?13,
                message = ?14, host_cpu_peak = ?15, net_peak_kbps = ?16
          WHERE id = ?1",
        params![
            sync.id,
            sync.master_node,
            sync.master_job_id,
            sync.node_name,
            sync.plan_name,
            sync.concurrent,
            sync.total_concurrent,
            sync.status,
            sync.qps_avg,
            sync.cpu_peak,
            sync.mem_peak_mb as i64,
            sync.started_at,
            sync.ended_at,
            sync.message,
            sync.host_cpu_peak,
            sync.net_peak_kbps
        ],
    ) {
        log::warn!("写入从节点历史失败：{error}");
    }
}

pub fn list_slave_history(
    pool: &DbPool,
    limit: u32,
    offset: u32,
) -> Result<Vec<ClusterSlaveHistoryItem>, String> {
    let conn = pool.get().map_err(|error| error.to_string())?;
    let mut statement = conn
        .prepare(
            "SELECT id, created_at, master_addr, master_node, master_job_id, node_name,
                    plan_name, join_mode, concurrent, total_concurrent, status, qps_avg,
                    cpu_peak, mem_peak_mb, started_at, ended_at, message,
                    host_cpu_peak, net_peak_kbps
               FROM cluster_slave_history
              ORDER BY created_at DESC
              LIMIT ?1 OFFSET ?2",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(params![limit as i64, offset as i64], |row| {
            Ok(ClusterSlaveHistoryItem {
                id: row.get(0)?,
                created_at: row.get(1)?,
                master_addr: row.get(2)?,
                master_node: row.get(3)?,
                master_job_id: row.get(4)?,
                node_name: row.get(5)?,
                plan_name: row.get(6)?,
                join_mode: row.get(7)?,
                concurrent: row.get::<_, i64>(8)? as u32,
                total_concurrent: row.get::<_, i64>(9)? as u32,
                status: row.get(10)?,
                qps_avg: row.get(11)?,
                cpu_peak: row.get(12)?,
                mem_peak_mb: row.get::<_, i64>(13)? as u64,
                started_at: row.get(14)?,
                ended_at: row.get(15)?,
                message: row.get(16)?,
                host_cpu_peak: row.get::<_, Option<f64>>(17)?.unwrap_or(0.0),
                net_peak_kbps: row.get::<_, Option<f64>>(18)?.unwrap_or(0.0),
            })
        })
        .map_err(|error| error.to_string())?;
    let mut items = Vec::new();
    for row in rows {
        items.push(row.map_err(|error| error.to_string())?);
    }
    Ok(items)
}

pub fn clear_slave_history(pool: &DbPool) -> Result<u32, String> {
    let conn = pool.get().map_err(|error| error.to_string())?;
    let removed = conn
        .execute("DELETE FROM cluster_slave_history", [])
        .map_err(|error| error.to_string())?;
    Ok(removed as u32)
}
