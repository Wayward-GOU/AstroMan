//! 集群设备缓存与权重建议（R9）
//!
//! 从节点上报的 `machine_capacity` 结果按稳定 `device_id` 落库，
//! 跨任务复用；准备阶段用它生成默认权重（容量归一化）。

use rusqlite::params;

use super::protocol::{ClusterDevice, DeviceCapability};
use crate::state::DbPool;
use crate::util::{err, now_ms};

pub const MIN_SUGGESTED_WEIGHT: f64 = 0.1;
pub const MAX_SUGGESTED_WEIGHT: f64 = 10.0;

pub fn upsert_device(
    pool: &DbPool,
    device_id: &str,
    addr: &str,
    capability: &DeviceCapability,
) -> Result<(), String> {
    let id = device_id.trim();
    if id.is_empty() {
        return Ok(());
    }
    let conn = pool.get().map_err(err)?;
    conn.execute(
        "INSERT INTO cluster_devices
            (node_id, node_name, last_addr, last_seen_at, os, cores,
             capacity_concurrent, capacity_qps, probed_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT(node_id) DO UPDATE SET
            node_name = excluded.node_name,
            last_addr = excluded.last_addr,
            last_seen_at = excluded.last_seen_at,
            os = excluded.os,
            cores = excluded.cores,
            capacity_concurrent = excluded.capacity_concurrent,
            capacity_qps = excluded.capacity_qps,
            probed_at = excluded.probed_at",
        params![
            id,
            capability.node_name.trim(),
            addr,
            now_ms(),
            capability.os,
            capability.cores as i64,
            capability.capacity_concurrent as i64,
            capability.capacity_qps,
            capability.probed_at,
        ],
    )
    .map_err(err)?;
    Ok(())
}

/// 记录一次连接但没有能力数据：只刷新地址与时间，不覆盖已有检测结果
pub fn touch_device(
    pool: &DbPool,
    device_id: &str,
    node_name: &str,
    addr: &str,
) -> Result<(), String> {
    let id = device_id.trim();
    if id.is_empty() {
        return Ok(());
    }
    let conn = pool.get().map_err(err)?;
    conn.execute(
        "INSERT INTO cluster_devices
            (node_id, node_name, last_addr, last_seen_at)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(node_id) DO UPDATE SET
            node_name = excluded.node_name,
            last_addr = excluded.last_addr,
            last_seen_at = excluded.last_seen_at",
        params![id, node_name.trim(), addr, now_ms()],
    )
    .map_err(err)?;
    Ok(())
}

pub fn list_devices(pool: &DbPool, limit: usize) -> Result<Vec<ClusterDevice>, String> {
    let conn = pool.get().map_err(err)?;
    let mut stmt = conn
        .prepare(
            "SELECT node_id, node_name, last_addr, last_seen_at, os, cores,
                    capacity_concurrent, capacity_qps, probed_at
             FROM cluster_devices ORDER BY last_seen_at DESC LIMIT ?1",
        )
        .map_err(err)?;
    let items = stmt
        .query_map(params![limit.clamp(1, 200) as i64], |row| {
            Ok(ClusterDevice {
                node_id: row.get(0)?,
                node_name: row.get(1)?,
                last_addr: row.get(2)?,
                last_seen_at: row.get(3)?,
                os: row.get(4)?,
                cores: row.get::<_, i64>(5)? as u32,
                capacity_concurrent: row.get::<_, i64>(6)? as u32,
                capacity_qps: row.get(7)?,
                probed_at: row.get(8)?,
                online: false,
            })
        })
        .map_err(err)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(err)?;
    Ok(items)
}

/// 容量归一化：以参与节点中的最小正容量为 1，封顶 10；没有容量的节点返回 None
pub fn capacity_weight(capacity: Option<u32>, floor: Option<u32>) -> Option<f64> {
    let capacity = capacity.filter(|value| *value > 0)?;
    let floor = floor.filter(|value| *value > 0).unwrap_or(capacity);
    Some((capacity as f64 / floor as f64).clamp(MIN_SUGGESTED_WEIGHT, MAX_SUGGESTED_WEIGHT))
}

/// 一组节点的建议权重：按最小正容量归一化，缺能力数据的节点保持 None
pub fn suggest_weights(capacities: &[(String, Option<u32>)]) -> Vec<(String, Option<f64>)> {
    let floor = capacities
        .iter()
        .filter_map(|(_, capacity)| capacity.filter(|value| *value > 0))
        .min();
    capacities
        .iter()
        .map(|(id, capacity)| (id.clone(), capacity_weight(*capacity, floor)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capacity_weight_is_relative_and_clamped() {
        assert_eq!(capacity_weight(Some(100), Some(100)), Some(1.0));
        assert_eq!(capacity_weight(Some(200), Some(100)), Some(2.0));
        assert_eq!(
            capacity_weight(Some(10), Some(100)),
            Some(MIN_SUGGESTED_WEIGHT)
        );
        assert_eq!(
            capacity_weight(Some(10_000), Some(100)),
            Some(MAX_SUGGESTED_WEIGHT)
        );
        assert_eq!(capacity_weight(None, Some(100)), None);
        assert_eq!(capacity_weight(Some(0), Some(100)), None);
    }

    #[test]
    fn suggest_weights_skip_nodes_without_capacity() {
        let input = vec![
            ("a".to_string(), Some(100)),
            ("b".to_string(), Some(200)),
            ("c".to_string(), None),
        ];
        let weights = suggest_weights(&input);
        assert_eq!(weights[0].1, Some(1.0));
        assert_eq!(weights[1].1, Some(2.0));
        assert_eq!(weights[2].1, None);
    }
}
