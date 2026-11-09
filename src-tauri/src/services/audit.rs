//! 审计日志（P-07 / AI-01）。
//!
//! 记录「谁在什么时候对什么做了什么」：文档导出、证书安装、AI 写操作确认等。
//! 只追加不修改，默认保留 90 天；详情存 JSON，方便前端展开，也方便诊断包带走。

use rusqlite::{params, Connection, Row};
use serde_json::Value;

use crate::util::{err, now_ms};

/// 保留天数：超过即清理，避免审计表无限增长
pub const RETENTION_DAYS: i64 = 90;
/// 单条详情上限：防止把整份文档塞进审计表
const MAX_DETAIL_CHARS: usize = 8 * 1024;

/// 写入一条审计。失败只记日志，不影响主业务流程。
pub fn write(
    conn: &Connection,
    action: &str,
    target: Option<&str>,
    result: &str,
    detail: Option<&Value>,
    trace_id: Option<&str>,
) -> Result<(), String> {
    let detail = detail.map(|value| {
        let text = value.to_string();
        if text.chars().count() > MAX_DETAIL_CHARS {
            let truncated: String = text.chars().take(MAX_DETAIL_CHARS).collect();
            format!("{truncated}…(truncated)")
        } else {
            text
        }
    });
    conn.execute(
        "INSERT INTO audit_logs (created_at, action, target, result, detail, trace_id, actor)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'local')",
        params![now_ms(), action, target, result, detail, trace_id],
    )
    .map_err(err)?;
    Ok(())
}

/// 清理超过保留期的审计，返回删除条数
pub fn cleanup(conn: &Connection, keep_days: i64) -> Result<usize, String> {
    let cutoff = now_ms() - keep_days.max(1) * 24 * 60 * 60 * 1000;
    conn.execute(
        "DELETE FROM audit_logs WHERE created_at < ?1",
        params![cutoff],
    )
    .map_err(err)
}

/// 最近审计，按时间倒序；`action` 为空表示不过滤
pub fn list(
    conn: &Connection,
    action: Option<&str>,
    limit: i64,
    offset: i64,
) -> Result<Vec<crate::models::AuditLog>, String> {
    let action = action.map(str::trim).filter(|value| !value.is_empty());
    let mut stmt = conn
        .prepare(
            "SELECT id, created_at, action, target, result, detail, trace_id, actor
             FROM audit_logs
             WHERE (?1 IS NULL OR action = ?1)
             ORDER BY created_at DESC, id DESC
             LIMIT ?2 OFFSET ?3",
        )
        .map_err(err)?;
    let rows = stmt
        .query_map(params![action, limit.clamp(1, 500), offset.max(0)], map_row)
        .map_err(err)?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(err)
}

fn map_row(row: &Row<'_>) -> rusqlite::Result<crate::models::AuditLog> {
    let detail: Option<String> = row.get(5)?;
    Ok(crate::models::AuditLog {
        id: row.get(0)?,
        created_at: row.get(1)?,
        action: row.get(2)?,
        target: row.get(3)?,
        result: row.get(4)?,
        detail: detail
            .map(|text| serde_json::from_str(&text).unwrap_or(serde_json::Value::String(text))),
        trace_id: row.get(6)?,
        actor: row.get(7)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn memory_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(include_str!("../db/schema.sql"))
            .unwrap();
        conn
    }

    #[test]
    fn writes_and_reads_back_json_detail() {
        let conn = memory_db();
        let detail = serde_json::json!({ "format": "markdown", "requests": 3 });
        write(
            &conn,
            "docs_export",
            Some("/tmp/doc.md"),
            "ok",
            Some(&detail),
            None,
        )
        .unwrap();
        let list = list(&conn, Some("docs_export"), 10, 0).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].action, "docs_export");
        assert_eq!(list[0].result, "ok");
        assert_eq!(list[0].detail.as_ref().unwrap()["requests"], 3);
        assert_eq!(list[0].actor, "local");
    }

    #[test]
    fn filters_by_action_and_orders_newest_first() {
        let conn = memory_db();
        write(&conn, "docs_export", None, "ok", None, None).unwrap();
        write(&conn, "ca_install", None, "ok", None, None).unwrap();
        write(&conn, "docs_export", None, "failed", None, None).unwrap();
        let items = list(&conn, Some("docs_export"), 10, 0).unwrap();
        assert_eq!(items.len(), 2);
        assert!(items[0].id > items[1].id);
        assert_eq!(list(&conn, None, 10, 0).unwrap().len(), 3);
    }

    #[test]
    fn cleanup_removes_expired_entries() {
        let conn = memory_db();
        conn.execute(
            "INSERT INTO audit_logs (created_at, action, result, actor)
             VALUES (0, 'old', 'ok', 'local')",
            [],
        )
        .unwrap();
        write(&conn, "new", None, "ok", None, None).unwrap();
        let removed = cleanup(&conn, RETENTION_DAYS).unwrap();
        assert_eq!(removed, 1);
        assert_eq!(list(&conn, None, 10, 0).unwrap().len(), 1);
    }

    #[test]
    fn oversized_detail_is_truncated() {
        let conn = memory_db();
        let huge = serde_json::json!({ "text": "x".repeat(MAX_DETAIL_CHARS * 2) });
        write(&conn, "big", None, "ok", Some(&huge), None).unwrap();
        let raw: String = conn
            .query_row("SELECT detail FROM audit_logs LIMIT 1", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert!(raw.ends_with("(truncated)"));
        assert!(raw.len() < MAX_DETAIL_CHARS + 64);
    }
}
