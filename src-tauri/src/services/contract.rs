//! 契约快照与变更检测
//!
//! 快照来源与 OpenAPI 导出共用 `services::openapi::build_openapi_spec`，生成后统一
//! `normalize_spec`，保证同一项目重复生成 diff 为空（技术方案 4.2）。
//! 每项目只保留最近 20 条基线（技术方案 5.1）。

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use serde_json::Value;

use crate::contract::{diff_specs, normalize_spec, render_html, render_markdown, ContractDiff};
use crate::services::openapi::build_openapi_spec;
use crate::util::{err, now_ms};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContractSnapshot {
    pub id: i64,
    pub project_id: i64,
    pub label: String,
    pub request_count: i64,
    pub source: String,
    pub digest: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContractCheckResult {
    pub baseline: ContractSnapshot,
    pub diff: ContractDiff,
    pub generated_at: i64,
}

fn map_snapshot(row: &rusqlite::Row<'_>) -> rusqlite::Result<ContractSnapshot> {
    Ok(ContractSnapshot {
        id: row.get(0)?,
        project_id: row.get(1)?,
        label: row.get(2)?,
        request_count: row.get(3)?,
        source: row.get(4)?,
        digest: row.get(5)?,
        created_at: row.get(6)?,
    })
}

const SNAPSHOT_COLUMNS: &str = "id, project_id, label, request_count, source, digest, created_at";
/// 每项目保留最近 20 条基线（技术方案 5.1），超出即删最旧，避免快照表无限增长。
const RETAIN_SNAPSHOTS: i64 = 20;

fn request_count(conn: &Connection, project_id: i64) -> Result<i64, String> {
    conn.query_row(
        "SELECT COUNT(*) FROM requests r
           JOIN collections c ON c.id = r.collection_id
          WHERE c.project_id = ?1",
        params![project_id],
        |row| row.get(0),
    )
    .map_err(err)
}

pub(crate) fn create_snapshot(
    conn: &Connection,
    project_id: i64,
    label: &str,
) -> Result<ContractSnapshot, String> {
    let spec = normalize_spec(&build_openapi_spec(conn, project_id)?);
    let spec_json = serde_json::to_string(&spec).map_err(err)?;
    let digest = crate::commands::tools::sha256_hex(spec_json.as_bytes());
    let count = request_count(conn, project_id)?;
    let label = label.trim();
    let label = if label.is_empty() {
        format!("快照 {}", now_ms())
    } else {
        label.to_string()
    };
    let now = now_ms();
    conn.execute(
        "INSERT INTO contract_snapshots
           (project_id, label, spec_json, request_count, source, digest, created_at)
         VALUES (?1, ?2, ?3, ?4, 'project', ?5, ?6)",
        params![project_id, label, spec_json, count, digest, now],
    )
    .map_err(err)?;
    let id = conn.last_insert_rowid();
    conn.execute(
        &format!(
            "DELETE FROM contract_snapshots
              WHERE project_id = ?1
                AND id NOT IN (
                    SELECT id FROM contract_snapshots
                     WHERE project_id = ?1 ORDER BY id DESC LIMIT {RETAIN_SNAPSHOTS}
                )"
        ),
        params![project_id],
    )
    .map_err(err)?;
    Ok(ContractSnapshot {
        id,
        project_id,
        label,
        request_count: count,
        source: "project".to_string(),
        digest,
        created_at: now,
    })
}

pub(crate) fn list_snapshots(
    conn: &Connection,
    project_id: i64,
) -> Result<Vec<ContractSnapshot>, String> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {SNAPSHOT_COLUMNS} FROM contract_snapshots
              WHERE project_id = ?1 ORDER BY id DESC"
        ))
        .map_err(err)?;
    let rows = stmt
        .query_map(params![project_id], map_snapshot)
        .map_err(err)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(err)?;
    Ok(rows)
}

/// 默认基线：项目最近一次手工创建的快照（不自动把导出当基线）。
pub(crate) fn latest_snapshot_id(conn: &Connection, project_id: i64) -> Result<i64, String> {
    conn.query_row(
        "SELECT id FROM contract_snapshots WHERE project_id = ?1 ORDER BY id DESC LIMIT 1",
        params![project_id],
        |row| row.get(0),
    )
    .optional()
    .map_err(err)?
    .ok_or_else(|| "E_CONTRACT_BASELINE: 项目还没有契约快照，请先生成一个基线".to_string())
}

fn find_snapshot(
    conn: &Connection,
    project_id: i64,
    baseline_id: i64,
) -> Result<ContractSnapshot, String> {
    conn.query_row(
        &format!(
            "SELECT {SNAPSHOT_COLUMNS} FROM contract_snapshots
              WHERE id = ?1 AND project_id = ?2"
        ),
        params![baseline_id, project_id],
        map_snapshot,
    )
    .optional()
    .map_err(err)?
    .ok_or_else(|| "E_CONTRACT_BASELINE: 基线快照不存在或不属于该项目".to_string())
}

pub(crate) fn check_contract(
    conn: &Connection,
    project_id: i64,
    baseline_id: i64,
) -> Result<ContractCheckResult, String> {
    let baseline = find_snapshot(conn, project_id, baseline_id)?;
    let spec_json: String = conn
        .query_row(
            "SELECT spec_json FROM contract_snapshots WHERE id = ?1",
            params![baseline_id],
            |row| row.get(0),
        )
        .map_err(err)?;
    let base_spec: Value = serde_json::from_str(&spec_json)
        .map_err(|e| format!("E_CONTRACT_SNAPSHOT: 基线快照损坏：{e}"))?;
    let current = normalize_spec(&build_openapi_spec(conn, project_id)?);
    let diff = diff_specs(&base_spec, &current);
    Ok(ContractCheckResult {
        baseline,
        diff,
        generated_at: now_ms(),
    })
}

pub(crate) fn report_text(
    conn: &Connection,
    project_id: i64,
    baseline_id: i64,
    format: &str,
) -> Result<String, String> {
    let result = check_contract(conn, project_id, baseline_id)?;
    let label = format!("{} (#{})", result.baseline.label, result.baseline.id);
    match format.trim().to_ascii_lowercase().as_str() {
        "markdown" | "md" => Ok(render_markdown(&result.diff, &label)),
        "html" => Ok(render_html(&result.diff, &label)),
        other => Err(format!(
            "E_CONTRACT_FORMAT: 不支持的报告格式 {other}（markdown / html）"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::entity::model::{EntityDoc, EntityNode};
    use r2d2_sqlite::SqliteConnectionManager;

    fn pool(tag: &str) -> crate::state::DbPool {
        let dir = crate::util::test_dir(tag);
        let manager = SqliteConnectionManager::file(dir.join("astroman.db"));
        let pool = r2d2::Pool::builder().max_size(2).build(manager).unwrap();
        db::init(&pool).unwrap();
        pool
    }

    fn entity(children: Vec<EntityNode>) -> String {
        let root = EntityNode {
            children,
            ..EntityNode::new_root("object")
        };
        EntityDoc::new(root, 0).encode().unwrap()
    }

    fn seed(conn: &Connection, response_entity: &str) -> i64 {
        let now = now_ms();
        conn.execute(
            "INSERT INTO projects (name, created_at, updated_at) VALUES ('P', ?1, ?1)",
            params![now],
        )
        .unwrap();
        let project = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO collections (project_id, name, created_at, updated_at)
             VALUES (?1, 'C', ?2, ?2)",
            params![project, now],
        )
        .unwrap();
        let collection = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO requests
               (collection_id, name, method, url, response_entity, created_at, updated_at)
             VALUES (?1, '详情', 'GET', 'https://api.example.com/users/{id}', ?2, ?3, ?3)",
            params![collection, response_entity, now],
        )
        .unwrap();
        project
    }

    #[test]
    fn snapshot_list_and_self_check_are_clean() {
        let pool = pool("contract-snapshot");
        let conn = pool.get().unwrap();
        let mut name = EntityNode {
            name: "name".into(),
            type_name: "string".into(),
            description: "昵称".into(),
            ..EntityNode::default()
        };
        name.required = true;
        let project = seed(&conn, &entity(vec![name]));

        let snapshot = create_snapshot(&conn, project, "v1.0.0").unwrap();
        assert_eq!(snapshot.request_count, 1);
        assert!(!snapshot.digest.is_empty());
        let listed = list_snapshots(&conn, project).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].label, "v1.0.0");

        let result = check_contract(&conn, project, snapshot.id).unwrap();
        assert!(result.diff.breaking.is_empty());
        assert!(result.diff.compatible.is_empty());
        assert_eq!(result.diff.summary.operations, 1);
    }

    #[test]
    fn removed_response_field_is_breaking_after_snapshot() {
        let pool = pool("contract-diff");
        let conn = pool.get().unwrap();
        let name = EntityNode {
            name: "name".into(),
            type_name: "string".into(),
            ..EntityNode::default()
        };
        let email = EntityNode {
            name: "email".into(),
            type_name: "string".into(),
            ..EntityNode::default()
        };
        let project = seed(&conn, &entity(vec![name, email]));
        let snapshot = create_snapshot(&conn, project, "baseline").unwrap();

        // 用户删除 email 实体字段后，契约应判破坏
        let only_name = EntityNode {
            name: "name".into(),
            type_name: "string".into(),
            ..EntityNode::default()
        };
        conn.execute(
            "UPDATE requests SET response_entity = ?1",
            params![entity(vec![only_name])],
        )
        .unwrap();

        let result = check_contract(&conn, project, snapshot.id).unwrap();
        assert_eq!(result.diff.summary.breaking, 1);
        assert_eq!(result.diff.breaking[0].kind, "removed_response_field");

        let report = report_text(&conn, project, snapshot.id, "markdown").unwrap();
        assert!(report.contains("破坏性变更"));
        assert!(report.contains("email"));
        let html = report_text(&conn, project, snapshot.id, "html").unwrap();
        assert!(html.contains("<table>"));
        assert!(report_text(&conn, project, snapshot.id, "pdf").is_err());
    }

    #[test]
    fn baseline_from_other_project_is_rejected() {
        let pool = pool("contract-other");
        let conn = pool.get().unwrap();
        let project = seed(&conn, &entity(vec![]));
        let snapshot = create_snapshot(&conn, project, "b").unwrap();
        assert!(check_contract(&conn, project + 1, snapshot.id).is_err());
        assert!(create_snapshot(&conn, 999, "x").is_err());
    }

    #[test]
    fn snapshot_retention_keeps_latest_twenty_per_project() {
        let pool = pool("contract-retention");
        let conn = pool.get().unwrap();
        let project = seed(&conn, &entity(vec![]));
        let mut last = 0;
        for index in 0..25 {
            last = create_snapshot(&conn, project, &format!("v{index}"))
                .unwrap()
                .id;
        }
        let listed = list_snapshots(&conn, project).unwrap();
        assert_eq!(listed.len(), 20);
        assert_eq!(listed[0].id, last);
        assert_eq!(listed[0].label, "v24");
        assert_eq!(listed[19].label, "v5");
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM contract_snapshots WHERE project_id = ?1",
                params![project],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            20
        );
    }

    #[test]
    fn consecutive_snapshots_produce_empty_diff() {
        let pool = pool("contract-stable");
        let conn = pool.get().unwrap();
        let name = EntityNode {
            name: "name".into(),
            type_name: "string".into(),
            description: "昵称".into(),
            ..EntityNode::default()
        };
        let project = seed(&conn, &entity(vec![name]));

        let first = create_snapshot(&conn, project, "v1").unwrap();
        let second = create_snapshot(&conn, project, "v2").unwrap();
        assert_eq!(first.digest, second.digest);

        let result = check_contract(&conn, project, first.id).unwrap();
        assert!(
            result.diff.breaking.is_empty(),
            "{:?}",
            result.diff.breaking
        );
        assert!(
            result.diff.compatible.is_empty(),
            "{:?}",
            result.diff.compatible
        );
        assert!(
            result.diff.warnings.is_empty(),
            "{:?}",
            result.diff.warnings
        );
        assert!(result.diff.schema_missing.is_empty());
        assert_eq!(result.diff.summary.operations, 1);
    }
}
