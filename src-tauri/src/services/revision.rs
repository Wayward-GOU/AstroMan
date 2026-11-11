//! 请求版本历史：保存即快照、字段级 diff、实体合并回滚
//!
//! 技术方案 4.1：
//! - 快照只存请求定义（auth 保持库内密文），不存运行结果；
//! - 保留最近 50 条（硬上限），随请求删除级联清理；
//! - 正文超过 1 MB / 实体超过 256 KB 时截断并标记，截断版本禁止回滚；
//! - 回滚走正常保存链路（生成新版本），实体按合并语义保留用户后填的说明与隐藏项。

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::entity::merge::merge;
use crate::entity::model::EntityDoc;
use crate::models::{HttpRequestDetail, KeyValueItem, RequestAuth};
use crate::services::request::{find_request, save_detail};
use crate::util::{err, now_ms};

const MAX_BODY_BYTES: usize = 1024 * 1024;
const MAX_ENTITY_BYTES: usize = 256 * 1024;
const RETAIN_VERSIONS: i64 = 50;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RequestSnapshot {
    pub name: String,
    pub method: String,
    /// 请求类型；老快照没有该字段，按 http 处理
    #[serde(default = "snapshot_protocol")]
    pub protocol: String,
    pub url: String,
    pub headers: String,
    pub params: String,
    pub body_type: String,
    pub body: String,
    pub body_meta: String,
    pub pre_script: String,
    pub post_script: String,
    pub assertions: String,
    /// 数据库中的密文，回滚后仍走现有解密链路
    pub auth: String,
    pub request_entity: Option<String>,
    pub response_entity: Option<String>,
}

fn snapshot_protocol() -> String {
    "http".to_string()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RevisionBrief {
    pub id: i64,
    pub request_id: i64,
    pub revision: i64,
    pub source: String,
    pub truncated: bool,
    pub created_at: i64,
    /// AI 写操作所属的提问轮次；普通保存为 None
    pub trace_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RevisionView {
    pub name: String,
    pub method: String,
    pub url: String,
    pub headers: Vec<KeyValueItem>,
    pub params: Vec<KeyValueItem>,
    pub body_type: String,
    pub body: String,
    pub body_meta: String,
    pub pre_script: String,
    pub post_script: String,
    pub assertions: String,
    /// 只展示认证类型，不回传任何凭据明文
    pub auth_kind: String,
    pub auth_configured: bool,
    pub request_entity: Option<String>,
    pub response_entity: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RevisionDetail {
    pub brief: RevisionBrief,
    pub snapshot: RevisionView,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RevisionChange {
    pub field: String,
    /// added / removed / changed / type_changed / required_changed / description_changed
    pub kind: String,
    /// changed / warning
    pub level: String,
    pub before: String,
    pub after: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RevisionDiff {
    pub from: RevisionBrief,
    pub to: RevisionBrief,
    pub changes: Vec<RevisionChange>,
}

fn truncate_utf8(text: &str, limit: usize) -> String {
    if text.len() <= limit {
        return text.to_string();
    }
    let mut end = limit;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_string()
}

fn read_snapshot(conn: &Connection, request_id: i64) -> Result<Option<RequestSnapshot>, String> {
    conn.query_row(
        "SELECT name, method, COALESCE(protocol, 'http'), url,
                COALESCE(headers, '[]'), COALESCE(params, '[]'),
                COALESCE(body_type, 'none'), COALESCE(body, ''),
                COALESCE(body_meta, ''), COALESCE(pre_script, ''),
                COALESCE(post_script, ''), COALESCE(assertions, ''),
                COALESCE(auth, '{\"kind\":\"none\"}'),
                request_entity, response_entity
           FROM requests WHERE id = ?1",
        params![request_id],
        |row| {
            Ok(RequestSnapshot {
                name: row.get(0)?,
                method: row.get(1)?,
                protocol: row.get(2)?,
                url: row.get(3)?,
                headers: row.get(4)?,
                params: row.get(5)?,
                body_type: row.get(6)?,
                body: row.get(7)?,
                body_meta: row.get(8)?,
                pre_script: row.get(9)?,
                post_script: row.get(10)?,
                assertions: row.get(11)?,
                auth: row.get(12)?,
                request_entity: row.get(13)?,
                response_entity: row.get(14)?,
            })
        },
    )
    .optional()
    .map_err(err)
}

/// 保存成功后追加快照（自带事务）；请求已被删除时静默跳过。
pub(crate) fn record(conn: &Connection, request_id: i64, source: &str) -> Result<(), String> {
    let tx = conn.unchecked_transaction().map_err(err)?;
    record_in_tx(&tx, request_id, source, None)?;
    tx.commit().map_err(err)
}

/// AI 写操作执行前的目标快照：`source = 'ai'`，按 trace_id 聚合即可一键回滚。
///
/// 必须在实际写入之前调用；同一轮对同一请求多次写入时，回滚取该轮最早的一条。
pub(crate) fn record_before_ai(
    conn: &Connection,
    request_id: i64,
    trace_id: &str,
) -> Result<(), String> {
    let trace = trace_id.trim();
    if trace.is_empty() {
        return Err("E_TRACE_EMPTY: trace_id 不能为空".to_string());
    }
    let tx = conn.unchecked_transaction().map_err(err)?;
    record_in_tx(&tx, request_id, "ai", Some(trace))?;
    tx.commit().map_err(err)
}

/// 在调用方事务内追加快照，由调用方负责提交（导入等批量写路径复用）。
pub(crate) fn record_in_tx(
    conn: &Connection,
    request_id: i64,
    source: &str,
    trace_id: Option<&str>,
) -> Result<(), String> {
    let Some(mut snapshot) = read_snapshot(conn, request_id)? else {
        return Ok(());
    };
    let mut truncated = false;
    if snapshot.body.len() > MAX_BODY_BYTES {
        snapshot.body = truncate_utf8(&snapshot.body, MAX_BODY_BYTES);
        truncated = true;
    }
    if let Some(entity) = snapshot.request_entity.as_mut() {
        if entity.len() > MAX_ENTITY_BYTES {
            *entity = truncate_utf8(entity, MAX_ENTITY_BYTES);
            truncated = true;
        }
    }
    if let Some(entity) = snapshot.response_entity.as_mut() {
        if entity.len() > MAX_ENTITY_BYTES {
            *entity = truncate_utf8(entity, MAX_ENTITY_BYTES);
            truncated = true;
        }
    }
    let snapshot_json = serde_json::to_string(&snapshot).map_err(err)?;
    let now = now_ms();
    let revision: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(revision), 0) + 1 FROM request_revisions WHERE request_id = ?1",
            params![request_id],
            |row| row.get(0),
        )
        .map_err(err)?;
    conn.execute(
        "INSERT INTO request_revisions
           (request_id, revision, snapshot_json, source, truncated, created_at, trace_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            request_id,
            revision,
            snapshot_json,
            source,
            truncated,
            now,
            trace_id
        ],
    )
    .map_err(err)?;
    // 保留最近 50 条硬上限：自动保存会把版本写得很快，超出即删除最旧的；
    // 不单独按时间保留，避免频繁保存时版本表无限累积。
    conn.execute(
        &format!(
            "DELETE FROM request_revisions
              WHERE request_id = ?1
                AND revision NOT IN (
                    SELECT revision FROM request_revisions
                     WHERE request_id = ?1 ORDER BY revision DESC LIMIT {RETAIN_VERSIONS}
                )"
        ),
        params![request_id],
    )
    .map_err(err)?;
    Ok(())
}

const BRIEF_COLUMNS: &str = "id, request_id, revision, source, truncated, created_at, trace_id";

fn map_brief(row: &rusqlite::Row<'_>) -> rusqlite::Result<RevisionBrief> {
    let truncated: i64 = row.get(4)?;
    Ok(RevisionBrief {
        id: row.get(0)?,
        request_id: row.get(1)?,
        revision: row.get(2)?,
        source: row.get(3)?,
        truncated: truncated != 0,
        created_at: row.get(5)?,
        trace_id: row.get(6)?,
    })
}

pub(crate) fn list_revisions(
    conn: &Connection,
    request_id: i64,
    limit: Option<i64>,
) -> Result<Vec<RevisionBrief>, String> {
    let limit = limit.unwrap_or(RETAIN_VERSIONS).clamp(1, 200);
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {BRIEF_COLUMNS} FROM request_revisions
              WHERE request_id = ?1 ORDER BY revision DESC LIMIT ?2"
        ))
        .map_err(err)?;
    let rows = stmt
        .query_map(params![request_id, limit], map_brief)
        .map_err(err)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(err)?;
    Ok(rows)
}

fn load_revision(
    conn: &Connection,
    request_id: i64,
    revision: i64,
) -> Result<(RevisionBrief, RequestSnapshot), String> {
    let (brief, snapshot_json) = conn
        .query_row(
            &format!(
                "SELECT {BRIEF_COLUMNS}, snapshot_json FROM request_revisions
                  WHERE request_id = ?1 AND revision = ?2"
            ),
            params![request_id, revision],
            |row| Ok((map_brief(row)?, row.get::<_, String>(7)?)),
        )
        .optional()
        .map_err(err)?
        .ok_or_else(|| format!("E_REVISION_NOT_FOUND: 版本 {revision} 不存在"))?;
    let snapshot: RequestSnapshot = serde_json::from_str(&snapshot_json)
        .map_err(|e| format!("E_REVISION_PARSE: 版本 {revision} 数据损坏：{e}"))?;
    Ok((brief, snapshot))
}

fn parse_items(raw: &str) -> Vec<KeyValueItem> {
    serde_json::from_str(raw).unwrap_or_default()
}

fn decrypt_auth(raw: &str) -> RequestAuth {
    let plain = crate::secret::decrypt_auth_json(raw);
    serde_json::from_str(&plain).unwrap_or_default()
}

fn auth_configured(auth: &RequestAuth) -> bool {
    !auth.token.trim().is_empty()
        || !auth.password.trim().is_empty()
        || (!auth.key.trim().is_empty() && !auth.value.trim().is_empty())
        || !auth.client_secret.trim().is_empty()
}

pub(crate) fn get_revision(
    conn: &Connection,
    request_id: i64,
    revision: i64,
) -> Result<RevisionDetail, String> {
    let (brief, snapshot) = load_revision(conn, request_id, revision)?;
    let auth = decrypt_auth(&snapshot.auth);
    let configured = auth_configured(&auth);
    Ok(RevisionDetail {
        brief,
        snapshot: RevisionView {
            name: snapshot.name,
            method: snapshot.method,
            url: snapshot.url,
            headers: parse_items(&snapshot.headers),
            params: parse_items(&snapshot.params),
            body_type: snapshot.body_type,
            body: snapshot.body,
            body_meta: snapshot.body_meta,
            pre_script: snapshot.pre_script,
            post_script: snapshot.post_script,
            assertions: snapshot.assertions,
            auth_kind: auth.kind,
            auth_configured: configured,
            request_entity: snapshot.request_entity,
            response_entity: snapshot.response_entity,
        },
    })
}

// ---------- 字段级 diff ----------

fn clip(text: &str) -> String {
    let mut out: String = text.chars().take(200).collect();
    if text.chars().count() > 200 {
        out.push('…');
    }
    out
}

fn sensitive_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    [
        "authorization",
        "cookie",
        "token",
        "secret",
        "password",
        "api-key",
        "apikey",
    ]
    .iter()
    .any(|needle| key.contains(needle))
}

fn shown_value(key: &str, value: &str) -> String {
    if sensitive_key(key) && !value.is_empty() {
        "••••••".to_string()
    } else {
        clip(value)
    }
}

fn keyed_items(items: &[KeyValueItem], lowercase: bool) -> BTreeMap<String, KeyValueItem> {
    items
        .iter()
        .map(|item| {
            let key = item.key.trim().to_string();
            let key = if lowercase {
                key.to_ascii_lowercase()
            } else {
                key
            };
            (key, item.clone())
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn push_change(
    changes: &mut Vec<RevisionChange>,
    field: String,
    kind: &str,
    level: &str,
    before: String,
    after: String,
) {
    changes.push(RevisionChange {
        field,
        kind: kind.to_string(),
        level: level.to_string(),
        before,
        after,
    });
}

fn compare_items(
    changes: &mut Vec<RevisionChange>,
    prefix: &str,
    base: &[KeyValueItem],
    cur: &[KeyValueItem],
    lowercase: bool,
) {
    let base_items = keyed_items(base, lowercase);
    let cur_items = keyed_items(cur, lowercase);
    let all: BTreeSet<&String> = base_items.keys().chain(cur_items.keys()).collect();
    for key in all {
        let field = format!("{prefix}.{key}");
        match (base_items.get(key), cur_items.get(key)) {
            (Some(_), None) => push_change(
                changes,
                field,
                "removed",
                "changed",
                shown_value(key, &base_items[key].value),
                String::new(),
            ),
            (None, Some(item)) => push_change(
                changes,
                field,
                "added",
                "changed",
                String::new(),
                shown_value(key, &item.value),
            ),
            (Some(before), Some(after)) => {
                if before.value != after.value || before.enabled != after.enabled {
                    push_change(
                        changes,
                        field,
                        "changed",
                        "changed",
                        shown_value(key, &before.value),
                        shown_value(key, &after.value),
                    );
                }
            }
            (None, None) => {}
        }
    }
}

fn entity_fields(raw: Option<&str>) -> BTreeMap<String, (String, bool, String)> {
    let Some(doc) = crate::entity::decode_lenient(raw) else {
        return BTreeMap::new();
    };
    doc.flatten()
        .into_iter()
        .filter(|field| !field.stale)
        .map(|field| {
            (
                field.path,
                (field.type_name, field.required, field.description),
            )
        })
        .collect()
}

fn compare_entities(
    changes: &mut Vec<RevisionChange>,
    prefix: &str,
    base: Option<&str>,
    cur: Option<&str>,
) {
    let base_fields = entity_fields(base);
    let cur_fields = entity_fields(cur);
    let all: BTreeSet<&String> = base_fields.keys().chain(cur_fields.keys()).collect();
    for path in all {
        let field = format!("{prefix}.{path}");
        match (base_fields.get(path), cur_fields.get(path)) {
            (Some((kind, required, description)), None) => push_change(
                changes,
                field,
                "removed",
                "changed",
                format!("{kind}{}", if *required { " · 必填" } else { "" }),
                description.to_string(),
            ),
            (None, Some((kind, required, _))) => push_change(
                changes,
                field,
                "added",
                "changed",
                String::new(),
                format!("{kind}{}", if *required { " · 必填" } else { "" }),
            ),
            (
                Some((base_kind, base_required, base_desc)),
                Some((cur_kind, cur_required, cur_desc)),
            ) => {
                if base_kind != cur_kind {
                    push_change(
                        changes,
                        field.clone(),
                        "type_changed",
                        "changed",
                        base_kind.clone(),
                        cur_kind.clone(),
                    );
                }
                if base_required != cur_required {
                    push_change(
                        changes,
                        field.clone(),
                        "required_changed",
                        "changed",
                        if *base_required { "必填" } else { "可选" }.to_string(),
                        if *cur_required { "必填" } else { "可选" }.to_string(),
                    );
                }
                if base_desc != cur_desc {
                    push_change(
                        changes,
                        field,
                        "description_changed",
                        "warning",
                        clip(base_desc),
                        clip(cur_desc),
                    );
                }
            }
            (None, None) => {}
        }
    }
}

pub(crate) fn diff_revisions(
    conn: &Connection,
    request_id: i64,
    from: i64,
    to: i64,
) -> Result<RevisionDiff, String> {
    let (from_brief, base) = load_revision(conn, request_id, from)?;
    let (to_brief, cur) = load_revision(conn, request_id, to)?;
    let mut changes = Vec::new();
    for (field, before, after) in [
        ("name", base.name.as_str(), cur.name.as_str()),
        ("method", base.method.as_str(), cur.method.as_str()),
        ("url", base.url.as_str(), cur.url.as_str()),
        ("bodyType", base.body_type.as_str(), cur.body_type.as_str()),
        ("body", base.body.as_str(), cur.body.as_str()),
        (
            "preScript",
            base.pre_script.as_str(),
            cur.pre_script.as_str(),
        ),
        (
            "postScript",
            base.post_script.as_str(),
            cur.post_script.as_str(),
        ),
        (
            "assertions",
            base.assertions.as_str(),
            cur.assertions.as_str(),
        ),
    ] {
        if before != after {
            push_change(
                &mut changes,
                field.to_string(),
                "changed",
                "changed",
                clip(before),
                clip(after),
            );
        }
    }
    compare_items(
        &mut changes,
        "headers",
        &parse_items(&base.headers),
        &parse_items(&cur.headers),
        true,
    );
    compare_items(
        &mut changes,
        "params",
        &parse_items(&base.params),
        &parse_items(&cur.params),
        false,
    );
    let base_auth = decrypt_auth(&base.auth);
    let cur_auth = decrypt_auth(&cur.auth);
    if base_auth.kind != cur_auth.kind {
        push_change(
            &mut changes,
            "auth.kind".to_string(),
            "changed",
            "changed",
            base_auth.kind.clone(),
            cur_auth.kind.clone(),
        );
    } else if auth_configured(&base_auth) != auth_configured(&cur_auth) {
        push_change(
            &mut changes,
            "auth.credentials".to_string(),
            "changed",
            "changed",
            if auth_configured(&base_auth) {
                "已配置"
            } else {
                "未配置"
            }
            .to_string(),
            if auth_configured(&cur_auth) {
                "已配置"
            } else {
                "未配置"
            }
            .to_string(),
        );
    }
    compare_entities(
        &mut changes,
        "requestEntity",
        base.request_entity.as_deref(),
        cur.request_entity.as_deref(),
    );
    compare_entities(
        &mut changes,
        "responseEntity",
        base.response_entity.as_deref(),
        cur.response_entity.as_deref(),
    );
    changes.sort_by(|left, right| left.field.cmp(&right.field));
    Ok(RevisionDiff {
        from: from_brief,
        to: to_brief,
        changes,
    })
}

// ---------- 回滚 ----------

/// 实体回滚：结构取快照，当前 manual 说明 / 手工字段 / 隐藏路径优先
fn restore_entity(
    snapshot_raw: Option<&str>,
    current_raw: Option<&str>,
    now: i64,
) -> Result<Option<String>, String> {
    let Some(snapshot_raw) = snapshot_raw else {
        return Ok(None);
    };
    let snapshot = match EntityDoc::decode(snapshot_raw) {
        Ok(doc) => doc,
        // 截断 / 损坏的快照不参与回滚，保留当前实体更安全
        Err(_) => return Ok(current_raw.map(str::to_string)),
    };
    let current = current_raw.and_then(|raw| EntityDoc::decode(raw).ok());
    let merged = match current {
        Some(current) => merge(Some(current), snapshot.root, false, now).doc,
        None => EntityDoc::new(snapshot.root, now),
    };
    Ok(Some(merged.encode()?))
}

pub(crate) fn restore_revision(
    conn: &Connection,
    request_id: i64,
    revision: i64,
) -> Result<HttpRequestDetail, String> {
    let (brief, snapshot) = load_revision(conn, request_id, revision)?;
    if brief.truncated {
        return Err("E_REVISION_TRUNCATED: 该版本正文不完整，不能回滚".to_string());
    }
    let current = find_request(conn, request_id)?;
    let now = now_ms();
    let next = HttpRequestDetail {
        id: Some(request_id),
        collection_id: current.collection_id,
        name: snapshot.name,
        method: snapshot.method,
        protocol: snapshot.protocol,
        url: snapshot.url,
        headers: parse_items(&snapshot.headers),
        params: parse_items(&snapshot.params),
        body_type: snapshot.body_type,
        body: snapshot.body,
        body_meta: snapshot.body_meta,
        pre_script: snapshot.pre_script,
        post_script: snapshot.post_script,
        assertions: snapshot.assertions,
        auth: decrypt_auth(&snapshot.auth),
        request_entity: restore_entity(
            snapshot.request_entity.as_deref(),
            current.request_entity.as_deref(),
            now,
        )?,
        response_entity: restore_entity(
            snapshot.response_entity.as_deref(),
            current.response_entity.as_deref(),
            now,
        )?,
    };
    save_detail(conn, &next, "restore", true)
}

// ---------- AI 写操作回滚（AI-09） ----------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceRollbackItem {
    pub request_id: i64,
    pub revision: i64,
    pub name: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceRollbackSkip {
    pub request_id: i64,
    pub revision: i64,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceRollbackResult {
    pub trace_id: String,
    pub restored: Vec<TraceRollbackItem>,
    pub skipped: Vec<TraceRollbackSkip>,
}

/// 某轮提问关联的 AI 前置快照（只读展示 / 回放用）
pub(crate) fn list_by_trace(
    conn: &Connection,
    trace_id: &str,
    limit: Option<i64>,
) -> Result<Vec<RevisionBrief>, String> {
    let trace = trace_id.trim();
    if trace.is_empty() {
        return Err("E_TRACE_EMPTY: trace_id 不能为空".to_string());
    }
    let limit = limit.unwrap_or(200).clamp(1, 500);
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {BRIEF_COLUMNS} FROM request_revisions
              WHERE trace_id = ?1 ORDER BY id ASC LIMIT ?2"
        ))
        .map_err(err)?;
    let rows = stmt
        .query_map(params![trace, limit], map_brief)
        .map_err(err)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(err)?;
    Ok(rows)
}

/// 按 trace_id 聚合回滚：每个请求取该轮最早的前置快照，走正常保存链路生成新版本。
///
/// 删除类操作没有可回滚对象（请求删除后版本级联清理），会记入 skipped 而不是静默忽略。
pub(crate) fn rollback_trace(
    conn: &Connection,
    trace_id: &str,
) -> Result<TraceRollbackResult, String> {
    let trace = trace_id.trim();
    if trace.is_empty() {
        return Err("E_TRACE_EMPTY: trace_id 不能为空".to_string());
    }
    let mut stmt = conn
        .prepare(
            "SELECT request_id, MIN(revision) FROM request_revisions
              WHERE trace_id = ?1 GROUP BY request_id ORDER BY request_id",
        )
        .map_err(err)?;
    let groups = stmt
        .query_map(params![trace], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
        })
        .map_err(err)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(err)?;
    if groups.is_empty() {
        return Err("E_TRACE_NOT_FOUND: 没有找到该轮 AI 写操作的回滚快照".to_string());
    }
    let mut restored = Vec::new();
    let mut skipped = Vec::new();
    for (request_id, revision) in groups {
        match restore_revision(conn, request_id, revision) {
            Ok(detail) => restored.push(TraceRollbackItem {
                request_id,
                revision,
                name: detail.name,
            }),
            Err(reason) => skipped.push(TraceRollbackSkip {
                request_id,
                revision,
                reason,
            }),
        }
    }
    Ok(TraceRollbackResult {
        trace_id: trace.to_string(),
        restored,
        skipped,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::entity::model::EntityNode;
    use r2d2_sqlite::SqliteConnectionManager;

    fn pool(tag: &str) -> crate::state::DbPool {
        crate::secret::install_for_test();
        let dir = crate::util::test_dir(tag);
        let manager = SqliteConnectionManager::file(dir.join("astroman.db"));
        let pool = r2d2::Pool::builder().max_size(2).build(manager).unwrap();
        db::init(&pool).unwrap();
        pool
    }

    fn seed(conn: &Connection) -> i64 {
        conn.execute(
            "INSERT INTO projects (name, created_at, updated_at) VALUES ('P', 0, 0)",
            [],
        )
        .unwrap();
        let project = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO collections (project_id, name, created_at, updated_at)
             VALUES (?1, 'C', 0, 0)",
            params![project],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    fn detail(collection: i64, name: &str, url: &str) -> HttpRequestDetail {
        HttpRequestDetail {
            id: None,
            collection_id: collection,
            name: name.into(),
            method: "POST".into(),
            protocol: "http".into(),
            url: url.into(),
            headers: Vec::new(),
            params: Vec::new(),
            body_type: "json".into(),
            body: "{}".into(),
            body_meta: String::new(),
            pre_script: String::new(),
            post_script: String::new(),
            assertions: String::new(),
            auth: RequestAuth {
                kind: "none".into(),
                ..RequestAuth::default()
            },
            request_entity: None,
            response_entity: None,
        }
    }

    fn node(name: &str, type_name: &str) -> EntityNode {
        EntityNode {
            name: name.into(),
            type_name: type_name.into(),
            ..EntityNode::default()
        }
    }

    fn entity(children: Vec<EntityNode>, hidden: Vec<String>) -> String {
        let root = EntityNode {
            children,
            ..EntityNode::new_root("object")
        };
        let mut doc = EntityDoc::new(root, 0);
        doc.hidden_paths = hidden;
        doc.encode().unwrap()
    }

    fn change<'a>(diff: &'a RevisionDiff, field: &str) -> &'a RevisionChange {
        diff.changes
            .iter()
            .find(|item| item.field == field)
            .unwrap_or_else(|| panic!("diff 缺少字段 {field}: {:?}", diff.changes))
    }

    #[test]
    fn ai_snapshots_roll_back_by_trace() {
        let pool = pool("revision-trace");
        let conn = pool.get().unwrap();
        let collection = seed(&conn);

        let mut request = detail(collection, "R1", "https://a/1");
        let id = save_detail(&conn, &request, "save", false)
            .unwrap()
            .id
            .unwrap();
        request.id = Some(id);

        // 同一轮 AI 连续改两次：先记前置快照再写
        record_before_ai(&conn, id, "trace-1").unwrap();
        request.name = "AI-1".into();
        save_detail(&conn, &request, "ai", false).unwrap();
        record_before_ai(&conn, id, "trace-1").unwrap();
        request.name = "AI-2".into();
        request.url = "https://a/2".into();
        save_detail(&conn, &request, "ai", false).unwrap();

        let trace = list_by_trace(&conn, "trace-1", None).unwrap();
        assert_eq!(trace.len(), 2);
        assert!(trace.iter().all(|item| item.source == "ai"));
        assert_eq!(trace[0].trace_id.as_deref(), Some("trace-1"));

        let result = rollback_trace(&conn, "trace-1").unwrap();
        assert_eq!(result.restored.len(), 1);
        assert!(result.skipped.is_empty());
        assert_eq!(result.restored[0].revision, trace[0].revision);

        let current = find_request(&conn, id).unwrap();
        assert_eq!(current.name, "R1");
        assert_eq!(current.url, "https://a/1");
        // 回滚走正常保存链路：新增 restore 版本，不带 trace_id
        let list = list_revisions(&conn, id, None).unwrap();
        assert_eq!(list[0].source, "restore");
        assert!(list[0].trace_id.is_none());
        assert!(rollback_trace(&conn, "trace-missing").is_err());
    }

    #[test]
    fn saves_create_revisions_and_restore_goes_through_save() {
        let pool = pool("revision-flow");
        let conn = pool.get().unwrap();
        let collection = seed(&conn);

        let mut request = detail(collection, "R1", "https://a/1");
        let saved = save_detail(&conn, &request, "save", false).unwrap();
        let id = saved.id.unwrap();
        request.id = Some(id);
        request.name = "R2".into();
        save_detail(&conn, &request, "save", false).unwrap();
        request.url = "https://a/3".into();
        save_detail(&conn, &request, "save", false).unwrap();

        let list = list_revisions(&conn, id, None).unwrap();
        assert_eq!(list.len(), 3);
        assert_eq!(
            list.iter().map(|item| item.revision).collect::<Vec<_>>(),
            vec![3, 2, 1]
        );
        assert!(list.iter().all(|item| item.source == "save"));
        assert_eq!(get_revision(&conn, id, 1).unwrap().snapshot.name, "R1");

        let diff = diff_revisions(&conn, id, 1, 3).unwrap();
        assert_eq!(change(&diff, "name").kind, "changed");
        assert_eq!(change(&diff, "url").after, "https://a/3");
        assert!(diff_revisions(&conn, id, 1, 99).is_err());

        let restored = restore_revision(&conn, id, 1).unwrap();
        assert_eq!(restored.name, "R1");
        assert_eq!(restored.url, "https://a/1");
        // 回滚本身再生成一条版本，天然可再退
        let list = list_revisions(&conn, id, None).unwrap();
        assert_eq!(list[0].revision, 4);
        assert_eq!(list[0].source, "restore");
    }

    #[test]
    fn diff_masks_credentials_and_compares_entities() {
        let pool = pool("revision-diff");
        let conn = pool.get().unwrap();
        let collection = seed(&conn);

        let mut id_value = node("id", "number");
        id_value.description = "ID".into();
        let mut request = detail(collection, "R", "https://a/1");
        request.headers = vec![KeyValueItem {
            key: "Authorization".into(),
            value: "Bearer abc".into(),
            enabled: true,
        }];
        request.params = vec![KeyValueItem {
            key: "q".into(),
            value: "1".into(),
            enabled: true,
        }];
        request.request_entity = Some(entity(vec![id_value], Vec::new()));
        let saved = save_detail(&conn, &request, "save", false).unwrap();
        let id = saved.id.unwrap();

        request.id = Some(id);
        request.headers[0].value = "Bearer def".into();
        request.params = vec![KeyValueItem {
            key: "page".into(),
            value: "2".into(),
            enabled: true,
        }];
        let mut id_value = node("id", "string");
        id_value.description = "编号".into();
        request.request_entity = Some(entity(vec![id_value, node("name", "string")], Vec::new()));
        save_detail(&conn, &request, "save", false).unwrap();

        let diff = diff_revisions(&conn, id, 1, 2).unwrap();
        let header = change(&diff, "headers.authorization");
        assert_eq!(header.kind, "changed");
        assert_eq!(header.before, "••••••");
        assert_eq!(header.after, "••••••");
        assert_eq!(change(&diff, "params.q").kind, "removed");
        assert_eq!(change(&diff, "params.page").kind, "added");
        assert_eq!(change(&diff, "requestEntity.id").kind, "type_changed");
        assert_eq!(change(&diff, "requestEntity.name").kind, "added");
    }

    #[test]
    fn restore_keeps_manual_descriptions_hidden_paths_and_manual_fields() {
        let pool = pool("revision-restore-entity");
        let conn = pool.get().unwrap();
        let collection = seed(&conn);

        let mut name = node("name", "string");
        name.description = "旧说明".into();
        let mut request = detail(collection, "R", "https://a/1");
        request.request_entity = Some(entity(vec![name, node("legacy", "string")], Vec::new()));
        let saved = save_detail(&conn, &request, "save", false).unwrap();
        let id = saved.id.unwrap();

        let mut name = node("name", "string");
        name.description = "用户后填".into();
        let mut extra = node("extra", "string");
        extra.manual = true;
        request.id = Some(id);
        request.request_entity = Some(entity(vec![name, extra], vec!["legacy".to_string()]));
        save_detail(&conn, &request, "save", false).unwrap();

        let restored = restore_revision(&conn, id, 1).unwrap();
        let doc = EntityDoc::decode(restored.request_entity.as_deref().unwrap()).unwrap();
        let fields = doc.flatten();
        let paths: Vec<&str> = fields.iter().map(|field| field.path.as_str()).collect();
        assert!(paths.contains(&"name"), "快照字段应回滚: {paths:?}");
        assert!(paths.contains(&"extra"), "手工字段必须保留: {paths:?}");
        assert!(!paths.contains(&"legacy"), "被隐藏的字段不回滚: {paths:?}");
        assert_eq!(
            fields
                .iter()
                .find(|field| field.path == "name")
                .unwrap()
                .description,
            "用户后填"
        );
        assert!(doc.hidden_paths.contains(&"legacy".to_string()));
    }

    #[test]
    fn retention_keeps_latest_fifty_versions() {
        let pool = pool("revision-retention");
        let conn = pool.get().unwrap();
        let collection = seed(&conn);
        let request = detail(collection, "R", "https://a/1");
        let saved = save_detail(&conn, &request, "save", false).unwrap();
        let id = saved.id.unwrap();
        for _ in 0..55 {
            record(&conn, id, "save").unwrap();
        }
        let list = list_revisions(&conn, id, Some(200)).unwrap();
        assert_eq!(list.len(), 50);
        assert_eq!(list[0].revision, 56);
        assert_eq!(list.last().unwrap().revision, 7);
    }

    #[test]
    fn oversized_body_is_truncated_and_cannot_be_restored() {
        let pool = pool("revision-truncate");
        let conn = pool.get().unwrap();
        let collection = seed(&conn);
        let mut request = detail(collection, "R", "https://a/1");
        request.body = "a".repeat(MAX_BODY_BYTES + 16);
        let saved = save_detail(&conn, &request, "save", false).unwrap();
        let id = saved.id.unwrap();

        let brief = &list_revisions(&conn, id, None).unwrap()[0];
        assert!(brief.truncated);
        let view = get_revision(&conn, id, 1).unwrap();
        assert_eq!(view.snapshot.body.len(), MAX_BODY_BYTES);
        assert!(restore_revision(&conn, id, 1)
            .unwrap_err()
            .contains("E_REVISION_TRUNCATED"));
    }
}
