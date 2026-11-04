//! 记事本领域层：类型、CRUD、筛选、校验、重复时间推进与软删除清理。
//!
//! 这一层不依赖 Tauri 窗口，只依赖 `DbPool` 与 `chrono`，方便单测。
//! IPC 薄封装在 `commands/notes.rs`，窗口 / 通知在 `quick.rs` / `reminder.rs`。

use chrono::{
    Datelike, Duration, Local, LocalResult, NaiveDate, NaiveDateTime, TimeZone, Timelike,
};
use rusqlite::{params, params_from_iter, Connection, OptionalExtension, Row};
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::HashSet;
use std::fmt;

use crate::state::DbPool;
use crate::util::{err, now_ms};

pub mod attachment;
pub mod quick;
pub mod reminder;
pub mod vision;

#[cfg(test)]
mod tests;

/// app_meta 里的设置键
pub const SETTINGS_KEY: &str = "notes.settings";
/// 软删除保留天数
pub const TRASH_DAYS: i64 = 30;
/// 单条标题上限
const TITLE_LIMIT: usize = 200;
/// 正文上限
const CONTENT_LIMIT: usize = 200_000;
/// 标签上限
const TAG_LIMIT: usize = 20;
const TAG_CHAR_LIMIT: usize = 32;
/// 提前量上限：7 天
const LEAD_MAX: i64 = 10_080;
/// 列表默认 / 最大条数
const LIST_LIMIT_DEFAULT: i64 = 200;
const LIST_LIMIT_MAX: i64 = 500;

// ---------------------------------------------------------------------------
// 类型
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NotesFilter {
    pub scope: String,
    pub keyword: Option<String>,
    pub tag: Option<String>,
    pub kind: Option<String>,
    pub limit: Option<i64>,
}

impl Default for NotesFilter {
    fn default() -> Self {
        Self {
            scope: "today".to_string(),
            keyword: None,
            tag: None,
            kind: None,
            limit: None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteCounts {
    pub open: i64,
    /// 今天到期且未完成，不含逾期
    pub due_today: i64,
    /// 已过截止时间且未完成
    pub overdue: i64,
    pub done: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteSummary {
    pub id: i64,
    pub title: String,
    pub excerpt: String,
    pub status: String,
    pub kind: String,
    pub priority: i64,
    pub tags: Vec<String>,
    pub due_at: Option<i64>,
    pub remind_at: Option<i64>,
    pub remind_lead_min: i64,
    pub repeat: String,
    pub snooze_until: Option<i64>,
    pub pinned: bool,
    pub done_at: Option<i64>,
    pub updated_at: i64,
    pub attachment_count: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteDetail {
    #[serde(flatten)]
    pub summary: NoteSummary,
    pub content: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotesListResult {
    pub items: Vec<NoteSummary>,
    pub counts: NoteCounts,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TagCount {
    pub name: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToggleResult {
    pub note: NoteDetail,
    /// 重复条目顺延后的下次截止时间
    pub rolled_to: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateFromImageResult {
    pub note_ids: Vec<i64>,
    pub file_id: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupResult {
    pub deleted_notes: i64,
    pub freed_bytes: i64,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct NoteInput {
    pub title: String,
    pub content: String,
    pub kind: Option<String>,
    pub priority: Option<i64>,
    pub tags: Option<Vec<String>>,
    pub due_at: Option<i64>,
    pub remind_lead_min: Option<i64>,
    pub repeat: Option<String>,
    pub pinned: Option<bool>,
}

/// 部分更新：字段缺省 = 不动；`dueAt: null` = 清空截止。
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct NotePatch {
    pub title: Option<String>,
    pub content: Option<String>,
    pub kind: Option<String>,
    pub priority: Option<i64>,
    pub tags: Option<Vec<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub due_at: Option<Option<i64>>,
    pub remind_lead_min: Option<i64>,
    pub repeat: Option<String>,
    pub pinned: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NotesSettings {
    pub notify_enabled: bool,
    pub quiet_hours_enabled: bool,
    pub quiet_start: String,
    pub quiet_end: String,
    pub pause_until: i64,
    pub global_shortcut: bool,
    pub vision_provider_id: Option<i64>,
}

impl Default for NotesSettings {
    fn default() -> Self {
        Self {
            notify_enabled: true,
            quiet_hours_enabled: false,
            quiet_start: "22:00".to_string(),
            quiet_end: "08:00".to_string(),
            pause_until: 0,
            global_shortcut: true,
            vision_provider_id: None,
        }
    }
}

/// 调度器一轮里命中的条目
#[derive(Debug, Clone)]
pub struct DueNote {
    pub id: i64,
    pub title: String,
    pub due_at: Option<i64>,
}

/// 只对已删除且过期的条目做物理清理
pub fn cleanup_deleted(pool: &DbPool) -> Result<CleanupResult, String> {
    let conn = pool.get().map_err(err)?;
    let cutoff = now_ms() - TRASH_DAYS * 24 * 60 * 60 * 1000;
    let deleted_notes = conn
        .execute(
            "DELETE FROM notes WHERE status = 'deleted' AND deleted_at IS NOT NULL AND deleted_at < ?1",
            params![cutoff],
        )
        .map_err(err)? as i64;
    drop(conn);
    let freed_bytes = attachment::cleanup_orphans(pool)?;
    Ok(CleanupResult {
        deleted_notes,
        freed_bytes,
    })
}

// ---------------------------------------------------------------------------
// 设置
// ---------------------------------------------------------------------------

pub fn get_settings(pool: &DbPool) -> NotesSettings {
    let Ok(conn) = pool.get() else {
        return NotesSettings::default();
    };
    let raw = crate::db::meta_get(&conn, SETTINGS_KEY);
    parse_settings(raw.as_deref())
}

fn parse_settings(raw: Option<&str>) -> NotesSettings {
    let Some(text) = raw else {
        return NotesSettings::default();
    };
    let mut settings: NotesSettings = serde_json::from_str(text).unwrap_or_default();
    settings.quiet_start = normalize_hhmm(&settings.quiet_start).unwrap_or_else(|| "22:00".into());
    settings.quiet_end = normalize_hhmm(&settings.quiet_end).unwrap_or_else(|| "08:00".into());
    settings
}

pub fn set_settings(pool: &DbPool, settings: &NotesSettings) -> Result<NotesSettings, String> {
    let mut next = settings.clone();
    next.quiet_start = normalize_hhmm(&next.quiet_start).unwrap_or_else(|| "22:00".into());
    next.quiet_end = normalize_hhmm(&next.quiet_end).unwrap_or_else(|| "08:00".into());
    let raw = serde_json::to_string(&next).map_err(err)?;
    let conn = pool.get().map_err(err)?;
    crate::db::meta_set(&conn, SETTINGS_KEY, &raw)?;
    Ok(next)
}

fn normalize_hhmm(value: &str) -> Option<String> {
    let (hour, minute) = value.trim().split_once(':')?;
    let hour: u32 = hour.parse().ok()?;
    let minute: u32 = minute.parse().ok()?;
    if hour > 23 || minute > 59 {
        return None;
    }
    Some(format!("{hour:02}:{minute:02}"))
}

/// 当前是否处于免打扰时段（支持跨零点）
pub fn is_quiet_now(settings: &NotesSettings, now: i64) -> bool {
    if !settings.quiet_hours_enabled {
        return false;
    }
    let Some(start) = parse_minutes(&settings.quiet_start) else {
        return false;
    };
    let Some(end) = parse_minutes(&settings.quiet_end) else {
        return false;
    };
    if start == end {
        return false;
    }
    let Some(dt) = Local.timestamp_millis_opt(now).single() else {
        return false;
    };
    let current = dt.hour() as i64 * 60 + dt.minute() as i64;
    if start <= end {
        current >= start && current < end
    } else {
        current >= start || current < end
    }
}

fn parse_minutes(value: &str) -> Option<i64> {
    let (hour, minute) = value.trim().split_once(':')?;
    Some(hour.parse::<i64>().ok()? * 60 + minute.parse::<i64>().ok()?)
}

pub fn pause_active(settings: &NotesSettings, now: i64) -> bool {
    settings.pause_until > now
}

// ---------------------------------------------------------------------------
// CRUD
// ---------------------------------------------------------------------------

pub fn list(pool: &DbPool, filter: &NotesFilter) -> Result<NotesListResult, String> {
    let conn = pool.get().map_err(err)?;
    let now = now_ms();
    let limit = filter
        .limit
        .unwrap_or(LIST_LIMIT_DEFAULT)
        .clamp(1, LIST_LIMIT_MAX);

    let mut where_sql = String::from("n.status = ?");
    let mut values: Vec<rusqlite::types::Value> = vec![rusqlite::types::Value::Text(
        if filter.scope == "done" {
            "done"
        } else {
            "open"
        }
        .to_string(),
    )];

    match filter.scope.as_str() {
        "today" | "week" => {
            let boundary = if filter.scope == "today" {
                start_of_tomorrow(now)
            } else {
                start_of_next_week(now)
            };
            where_sql.push_str(" AND (n.due_at IS NULL OR n.due_at < ?)");
            values.push(boundary.into());
        }
        _ => {}
    }

    if let Some(keyword) = filter
        .keyword
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
    {
        where_sql.push_str(" AND (n.title LIKE ? ESCAPE '\\' OR n.content LIKE ? ESCAPE '\\')");
        let pattern = format!("%{}%", escape_like(keyword));
        values.push(pattern.clone().into());
        values.push(pattern.into());
    }

    if let Some(kind) = filter
        .kind
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
    {
        where_sql.push_str(" AND n.kind = ?");
        values.push(normalize_kind(Some(kind)).into());
    }

    if let Some(tag) = filter
        .tag
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
    {
        where_sql.push_str(" AND instr(',' || n.tags || ',', ',' || ? || ',') > 0");
        values.push(tag.to_string().into());
    }

    let order = if filter.scope == "done" {
        "n.done_at DESC, n.updated_at DESC"
    } else {
        "n.pinned DESC, n.priority DESC,
         CASE WHEN n.due_at IS NULL THEN 1 ELSE 0 END,
         n.due_at ASC, n.updated_at DESC"
    };
    let sql = format!(
        "SELECT {SUMMARY_COLUMNS}
         FROM notes n
         LEFT JOIN (
           SELECT note_id, COUNT(*) AS c FROM note_file_links GROUP BY note_id
         ) links ON links.note_id = n.id
         WHERE {where_sql}
         ORDER BY {order}
         LIMIT ?"
    );
    values.push(limit.into());

    let mut stmt = conn.prepare(&sql).map_err(err)?;
    let rows = stmt
        .query_map(params_from_iter(values.iter()), row_to_summary)
        .map_err(err)?;
    let mut items = Vec::new();
    for item in rows {
        items.push(item.map_err(err)?);
    }
    drop(stmt);

    let counts = counts_with(&conn, now)?;
    Ok(NotesListResult { items, counts })
}

pub fn counts(pool: &DbPool) -> Result<NoteCounts, String> {
    let conn = pool.get().map_err(err)?;
    counts_with(&conn, now_ms())
}

fn counts_with(conn: &Connection, now: i64) -> Result<NoteCounts, String> {
    let open: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM notes WHERE status = 'open'",
            [],
            |row| row.get(0),
        )
        .map_err(err)?;
    let done: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM notes WHERE status = 'done'",
            [],
            |row| row.get(0),
        )
        .map_err(err)?;
    let today = start_of_today(now);
    let tomorrow = start_of_tomorrow(now);
    let due_today: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM notes
             WHERE status = 'open' AND due_at IS NOT NULL AND due_at >= ?1 AND due_at < ?2",
            params![today, tomorrow],
            |row| row.get(0),
        )
        .map_err(err)?;
    let overdue: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM notes
             WHERE status = 'open' AND due_at IS NOT NULL AND due_at < ?1",
            params![now],
            |row| row.get(0),
        )
        .map_err(err)?;
    Ok(NoteCounts {
        open,
        due_today,
        overdue,
        done,
    })
}

pub fn get(pool: &DbPool, id: i64) -> Result<NoteDetail, String> {
    let conn = pool.get().map_err(err)?;
    let sql = format!(
        "SELECT {DETAIL_COLUMNS}
         FROM notes n
         LEFT JOIN (
           SELECT note_id, COUNT(*) AS c FROM note_file_links GROUP BY note_id
         ) links ON links.note_id = n.id
         WHERE n.id = ?1 AND n.status <> 'deleted'"
    );
    conn.query_row(&sql, params![id], row_to_detail)
        .optional()
        .map_err(err)?
        .ok_or_else(|| "记录不存在或已删除".to_string())
}

/// 校验并规范化后的待办输入，供单条创建与批量创建共用
struct PreparedNote {
    title: String,
    content: String,
    kind: String,
    priority: i64,
    tags: Vec<String>,
    due_at: Option<i64>,
    remind_at: Option<i64>,
    lead: i64,
    repeat: String,
    pinned: bool,
}

fn prepare_note(input: &NoteInput) -> Result<PreparedNote, String> {
    let mut title = input.title.trim().to_string();
    let content = input.content.trim_end().to_string();
    if title.is_empty() {
        title = first_line(&content);
    }
    if title.is_empty() && content.trim().is_empty() {
        return Err("E_NOTES_EMPTY: 标题和正文不能同时为空".into());
    }
    validate_content(&content)?;
    title = clip(&title, TITLE_LIMIT);

    let priority = input.priority.unwrap_or(1).clamp(0, 2);
    let tags = normalize_tags(input.tags.clone().unwrap_or_default());
    let repeat = normalize_repeat(input.repeat.as_deref());
    let lead = if input.due_at.is_some() {
        input.remind_lead_min.unwrap_or(0).clamp(0, LEAD_MAX)
    } else {
        0
    };
    let due_at = match input.due_at {
        Some(value) => Some(validate_due(value)?),
        None => None,
    };
    let repeat = if due_at.is_none() {
        "none".into()
    } else {
        repeat
    };
    let remind_at = due_at.map(|due| due - lead * 60_000);
    Ok(PreparedNote {
        title,
        content,
        kind: normalize_kind(input.kind.as_deref()),
        priority,
        tags,
        due_at,
        remind_at,
        lead,
        repeat,
        pinned: input.pinned.unwrap_or(false),
    })
}

fn insert_prepared(conn: &Connection, note: &PreparedNote, now: i64) -> Result<i64, String> {
    conn.execute(
        "INSERT INTO notes
           (title, content, kind, status, priority, tags, due_at, remind_at,
            remind_lead_min, remind_repeat, snooze_until, notified_at, pinned,
            created_at, updated_at, done_at, deleted_at, deleted_from_status)
         VALUES (?1, ?2, ?3, 'open', ?4, ?5, ?6, ?7, ?8, ?9, NULL, NULL, ?10, ?11, ?11, NULL, NULL, NULL)",
        params![
            note.title,
            note.content,
            note.kind,
            note.priority,
            note.tags.join(","),
            note.due_at,
            note.remind_at,
            note.lead,
            note.repeat,
            note.pinned as i64,
            now
        ],
    )
    .map_err(err)?;
    Ok(conn.last_insert_rowid())
}

pub fn create(pool: &DbPool, input: NoteInput) -> Result<NoteDetail, String> {
    let prepared = prepare_note(&input)?;
    let conn = pool.get().map_err(err)?;
    let id = insert_prepared(&conn, &prepared, now_ms())?;
    drop(conn);
    get(pool, id)
}

/// 截图识别确认后批量建待办：N 条记录 + 一张图 + N 条关联，同一事务提交
pub fn create_from_image(
    pool: &DbPool,
    file_name: Option<String>,
    mime: Option<String>,
    base64_data: Option<String>,
    path: Option<String>,
    tasks: Vec<NoteInput>,
) -> Result<CreateFromImageResult, String> {
    if tasks.is_empty() || tasks.len() > 50 {
        return Err("一次最多生成 50 条待办".into());
    }
    let prepared: Vec<PreparedNote> = tasks.iter().map(prepare_note).collect::<Result<_, _>>()?;
    let (bytes, sniffed, default_name) =
        attachment::decode_input(file_name, mime, base64_data, path)?;
    let sha = attachment::hex_sha256(&bytes);
    let size = bytes.len() as i64;
    let now = now_ms();

    let conn = pool.get().map_err(err)?;
    let tx = conn.unchecked_transaction().map_err(err)?;
    let mut note_ids = Vec::with_capacity(prepared.len());
    for note in &prepared {
        note_ids.push(insert_prepared(&tx, note, now)?);
    }
    let existing: Option<i64> = tx
        .query_row(
            "SELECT id FROM note_files WHERE sha256 = ?1",
            params![sha],
            |row| row.get(0),
        )
        .optional()
        .map_err(err)?;
    let file_id = match existing {
        Some(id) => id,
        None => {
            tx.execute(
                "INSERT INTO note_files (sha256, file_name, mime, size, data, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![sha, default_name, sniffed, size, bytes, now],
            )
            .map_err(err)?;
            tx.last_insert_rowid()
        }
    };
    for note_id in &note_ids {
        tx.execute(
            "INSERT OR IGNORE INTO note_file_links (note_id, file_id, created_at)
             VALUES (?1, ?2, ?3)",
            params![note_id, file_id, now],
        )
        .map_err(err)?;
    }
    tx.commit().map_err(err)?;
    Ok(CreateFromImageResult { note_ids, file_id })
}

pub fn update(pool: &DbPool, id: i64, patch: NotePatch) -> Result<NoteDetail, String> {
    let conn = pool.get().map_err(err)?;
    let raw = load_raw(&conn, id)?.ok_or_else(|| "记录不存在或已删除".to_string())?;
    let now = now_ms();

    let mut title = match patch.title {
        Some(value) => value.trim().to_string(),
        None => raw.title.clone(),
    };
    let content = match patch.content {
        Some(value) => value.trim_end().to_string(),
        None => raw.content.clone(),
    };
    if title.is_empty() {
        title = first_line(&content);
    }
    if title.is_empty() && content.trim().is_empty() {
        return Err("E_NOTES_EMPTY: 标题和正文不能同时为空".into());
    }
    validate_content(&content)?;
    title = clip(&title, TITLE_LIMIT);

    let priority = patch
        .priority
        .map(|value| value.clamp(0, 2))
        .unwrap_or(raw.priority);
    let kind = match patch
        .kind
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        Some(value) => normalize_kind(Some(value)),
        None => raw.kind.clone(),
    };
    let tags = match patch.tags {
        Some(value) => normalize_tags(value),
        None => raw.tags,
    };
    let pinned = patch.pinned.unwrap_or(raw.pinned);

    let next_due = match patch.due_at {
        None => raw.due_at,
        Some(None) => None,
        Some(Some(value)) => Some(validate_due(value)?),
    };
    let next_repeat = match patch.repeat {
        Some(value) => normalize_repeat(Some(&value)),
        None => raw.repeat.clone(),
    };
    let next_lead = match patch.remind_lead_min {
        Some(value) => value.clamp(0, LEAD_MAX),
        None => raw.lead_min,
    };
    let schedule_changed =
        next_due != raw.due_at || next_lead != raw.lead_min || next_repeat != raw.repeat;

    let repeat = if next_due.is_none() {
        "none".to_string()
    } else {
        next_repeat
    };
    let lead = if next_due.is_none() { 0 } else { next_lead };
    let remind_at = next_due.map(|due| due - lead * 60_000);

    let (snooze_until, notified_at) = if schedule_changed {
        (None, None)
    } else {
        (raw.snooze_until, raw.notified_at)
    };

    conn.execute(
        "UPDATE notes SET title = ?1, content = ?2, priority = ?3, tags = ?4,
            due_at = ?5, remind_at = ?6, remind_lead_min = ?7, remind_repeat = ?8,
            snooze_until = ?9, notified_at = ?10, pinned = ?11, updated_at = ?12,
            kind = ?14
         WHERE id = ?13 AND status <> 'deleted'",
        params![
            title,
            content,
            priority,
            tags.join(","),
            next_due,
            remind_at,
            lead,
            repeat,
            snooze_until,
            notified_at,
            pinned as i64,
            now,
            id,
            kind
        ],
    )
    .map_err(err)?;
    drop(conn);
    get(pool, id)
}

pub fn toggle(pool: &DbPool, id: i64, done: bool) -> Result<ToggleResult, String> {
    let conn = pool.get().map_err(err)?;
    let raw = load_raw(&conn, id)?.ok_or_else(|| "记录不存在或已删除".to_string())?;
    let now = now_ms();

    if !done {
        if raw.status == "done" {
            conn.execute(
                "UPDATE notes SET status = 'open', done_at = NULL, updated_at = ?1
                 WHERE id = ?2 AND status = 'done'",
                params![now, id],
            )
            .map_err(err)?;
        }
        drop(conn);
        return Ok(ToggleResult {
            note: get(pool, id)?,
            rolled_to: None,
        });
    }

    if raw.status == "done" {
        drop(conn);
        return Ok(ToggleResult {
            note: get(pool, id)?,
            rolled_to: None,
        });
    }

    let mut rolled_to = None;
    if raw.repeat != "none" {
        // 只认 remind_at > now：说明本次提醒已触发，调度器已把周期推进到下一次。
        // 不能把 snooze_until 非空当作「已触发」，逾期条目在首次 tick 前稍后也会写它。
        let (next_due, next_remind) = if raw.remind_at.unwrap_or(now) > now {
            (raw.due_at, raw.remind_at.unwrap_or(now))
        } else {
            let anchor = raw.due_at.unwrap_or(raw.remind_at.unwrap_or(now));
            let next = advance_until_future(anchor, &raw.repeat, now);
            let due = raw.due_at.map(|_| next);
            let remind = due
                .map(|value| value - raw.lead_min * 60_000)
                .unwrap_or(next);
            (due, remind)
        };
        conn.execute(
            "UPDATE notes SET done_at = ?1, due_at = ?2, remind_at = ?3,
                status = 'open', notified_at = NULL, snooze_until = NULL, updated_at = ?1
             WHERE id = ?4 AND status = 'open'",
            params![now, next_due, next_remind, id],
        )
        .map_err(err)?;
        rolled_to = next_due;
    } else {
        conn.execute(
            "UPDATE notes SET status = 'done', done_at = ?1, notified_at = ?1,
                snooze_until = NULL, updated_at = ?1
             WHERE id = ?2 AND status = 'open'",
            params![now, id],
        )
        .map_err(err)?;
    }
    drop(conn);
    Ok(ToggleResult {
        note: get(pool, id)?,
        rolled_to,
    })
}

pub fn snooze(pool: &DbPool, id: i64, minutes: i64) -> Result<NoteDetail, String> {
    let minutes = minutes.clamp(1, 1440);
    let now = now_ms();
    let conn = pool.get().map_err(err)?;
    let raw = load_raw(&conn, id)?.ok_or_else(|| "记录不存在或已删除".to_string())?;
    if raw.status != "open" {
        return Err("只有未完成的记录可以稍后提醒".into());
    }
    if raw.due_at.is_none() || raw.remind_at.is_none() {
        return Err("该记录没有截止时间".into());
    }
    let remind_at = raw.remind_at.unwrap_or(now);
    // 一次性条目取 max，避免提前点铃铛把唯一一次提醒消耗掉；
    // 重复条目的 remind_at 已指向下一周期，取 max 会让「稍后」变成空操作。
    let target = now + minutes * 60_000;
    let snooze_until = if raw.repeat == "none" {
        target.max(remind_at)
    } else {
        target
    };
    conn.execute(
        "UPDATE notes SET snooze_until = ?1, notified_at = NULL, updated_at = ?2
         WHERE id = ?3 AND status = 'open'",
        params![snooze_until, now, id],
    )
    .map_err(err)?;
    drop(conn);
    get(pool, id)
}

pub fn delete(pool: &DbPool, id: i64) -> Result<(), String> {
    let conn = pool.get().map_err(err)?;
    let raw = load_raw(&conn, id)?.ok_or_else(|| "记录不存在或已删除".to_string())?;
    if raw.status == "deleted" {
        return Ok(());
    }
    conn.execute(
        "UPDATE notes SET status = 'deleted', deleted_at = ?1, deleted_from_status = ?2,
            updated_at = ?1
         WHERE id = ?3",
        params![now_ms(), raw.status, id],
    )
    .map_err(err)?;
    Ok(())
}

pub fn restore(pool: &DbPool, id: i64) -> Result<NoteDetail, String> {
    let conn = pool.get().map_err(err)?;
    let raw = load_raw(&conn, id)?.ok_or_else(|| "记录不存在".to_string())?;
    if raw.status != "deleted" {
        drop(conn);
        return get(pool, id);
    }
    let restored = match raw.deleted_from_status.as_deref() {
        Some("done") => "done",
        _ => "open",
    };
    conn.execute(
        "UPDATE notes SET status = ?1, deleted_at = NULL, deleted_from_status = NULL,
            updated_at = ?2
         WHERE id = ?3",
        params![restored, now_ms(), id],
    )
    .map_err(err)?;
    drop(conn);
    get(pool, id)
}

pub fn clear_done(pool: &DbPool) -> Result<i64, String> {
    let conn = pool.get().map_err(err)?;
    let now = now_ms();
    let changed = conn
        .execute(
            "UPDATE notes SET status = 'deleted', deleted_at = ?1,
                deleted_from_status = 'done', updated_at = ?1
             WHERE status = 'done'",
            params![now],
        )
        .map_err(err)?;
    Ok(changed as i64)
}

pub fn tags(pool: &DbPool) -> Result<Vec<TagCount>, String> {
    let conn = pool.get().map_err(err)?;
    let mut stmt = conn
        .prepare("SELECT tags FROM notes WHERE status <> 'deleted' AND tags <> ''")
        .map_err(err)?;
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(err)?;
    let mut counter: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
    let mut seen: HashSet<String> = HashSet::new();
    for row in rows {
        let text = row.map_err(err)?;
        seen.clear();
        for tag in text.split(',').map(str::trim).filter(|tag| !tag.is_empty()) {
            if !seen.insert(tag.to_string()) {
                continue;
            }
            *counter.entry(tag.to_string()).or_insert(0) += 1;
        }
    }
    let mut list: Vec<TagCount> = counter
        .into_iter()
        .map(|(name, count)| TagCount { name, count })
        .collect();
    list.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.name.cmp(&b.name)));
    Ok(list)
}

// ---------------------------------------------------------------------------
// 提醒调度
// ---------------------------------------------------------------------------

pub fn due_notes(pool: &DbPool, now: i64) -> Result<Vec<DueNote>, String> {
    let conn = pool.get().map_err(err)?;
    let mut stmt = conn
        .prepare(
            "SELECT id, title, due_at
             FROM notes
             WHERE status = 'open'
               AND remind_at IS NOT NULL
               AND (
                 (snooze_until IS NOT NULL AND snooze_until <= ?1)
                 OR (snooze_until IS NULL AND remind_at <= ?1)
               )
               AND (COALESCE(remind_repeat, 'none') <> 'none' OR notified_at IS NULL)
             ORDER BY COALESCE(snooze_until, remind_at) ASC
             LIMIT 200",
        )
        .map_err(err)?;
    let rows = stmt
        .query_map(params![now], |row| {
            Ok(DueNote {
                id: row.get(0)?,
                title: row.get(1)?,
                due_at: row.get(2)?,
            })
        })
        .map_err(err)?;
    let mut list = Vec::new();
    for row in rows {
        list.push(row.map_err(err)?);
    }
    Ok(list)
}

/// `mark_fired` 读取的一条提醒状态：status / due_at / remind_at / lead / snooze / repeat
type FiredRow = (String, Option<i64>, Option<i64>, i64, Option<i64>, String);

/// 先标记、后通知：写库成功后才由调用方发通知。
pub fn mark_fired(pool: &DbPool, batch: &[DueNote], now: i64) -> Result<(), String> {
    let conn = pool.get().map_err(err)?;
    let tx = conn.unchecked_transaction().map_err(err)?;
    for note in batch {
        let current: Option<FiredRow> = tx
            .query_row(
                "SELECT status, due_at, remind_at, remind_lead_min, snooze_until, remind_repeat
                 FROM notes WHERE id = ?1",
                params![note.id],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                    ))
                },
            )
            .optional()
            .map_err(err)?;
        let Some((status, due_at, remind_at, lead_min, snooze_until, repeat)) = current else {
            continue;
        };
        if status != "open" {
            continue;
        }
        // 本轮查询后用户又点了稍后：把下一次让给新时间
        if let Some(snooze) = snooze_until {
            if snooze > now {
                continue;
            }
        }
        let Some(remind_at) = remind_at else {
            continue;
        };

        if repeat != "none" && remind_at <= now {
            let anchor = due_at.unwrap_or(remind_at);
            let next_anchor = advance_until_future(anchor, &repeat, now);
            let next_due = due_at.map(|_| next_anchor);
            let next_remind = next_due
                .map(|value| value - lead_min * 60_000)
                .unwrap_or(next_anchor);
            tx.execute(
                "UPDATE notes SET notified_at = ?1, snooze_until = NULL,
                    due_at = ?2, remind_at = ?3
                 WHERE id = ?4 AND status = 'open'",
                params![now, next_due, next_remind, note.id],
            )
            .map_err(err)?;
        } else {
            tx.execute(
                "UPDATE notes SET notified_at = ?1, snooze_until = NULL
                 WHERE id = ?2 AND status = 'open'",
                params![now, note.id],
            )
            .map_err(err)?;
        }
    }
    tx.commit().map_err(err)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// 重复规则
// ---------------------------------------------------------------------------

/// 按一次重复规则推进；非法规则返回 None
pub fn advance_once(ms: i64, repeat: &str) -> Option<i64> {
    let dt = Local.timestamp_millis_opt(ms).single()?;
    let naive = dt.naive_local();
    let date = naive.date();
    let time = naive.time();
    let next_date = match repeat {
        "daily" => date + Duration::days(1),
        "weekdays" => {
            let mut next = date + Duration::days(1);
            while matches!(next.weekday(), chrono::Weekday::Sat | chrono::Weekday::Sun) {
                next += Duration::days(1);
            }
            next
        }
        "weekly" => date + Duration::days(7),
        "monthly" => next_month_clamped(date),
        _ => return None,
    };
    local_millis(next_date.and_time(time))
}

/// 推进到严格晚于 `now` 的下一个周期，最多 1000 次防脏数据死循环
pub fn advance_until_future(ms: i64, repeat: &str, now: i64) -> i64 {
    let mut current = ms;
    for _ in 0..1000 {
        match advance_once(current, repeat) {
            Some(next) if next > now => return next,
            Some(next) => current = next,
            None => break,
        }
    }
    current
}

fn next_month_clamped(date: NaiveDate) -> NaiveDate {
    let (mut year, mut month) = (date.year(), date.month());
    if month == 12 {
        year += 1;
        month = 1;
    } else {
        month += 1;
    }
    let first_next = if month == 12 {
        NaiveDate::from_ymd_opt(year + 1, 1, 1)
    } else {
        NaiveDate::from_ymd_opt(year, month + 1, 1)
    }
    .unwrap_or_else(|| NaiveDate::from_ymd_opt(year, month, 1).expect("valid month"));
    let last_day = (first_next - Duration::days(1)).day();
    NaiveDate::from_ymd_opt(year, month, date.day().min(last_day))
        .unwrap_or_else(|| NaiveDate::from_ymd_opt(year, month, 1).expect("valid month"))
}

pub(crate) fn local_millis(naive: NaiveDateTime) -> Option<i64> {
    match Local.from_local_datetime(&naive) {
        LocalResult::Single(dt) => Some(dt.timestamp_millis()),
        LocalResult::Ambiguous(earlier, _) => Some(earlier.timestamp_millis()),
        // 夏令时跳过的本地时刻：顺延一小时再取
        LocalResult::None => Local
            .from_local_datetime(&(naive + Duration::hours(1)))
            .earliest()
            .map(|dt| dt.timestamp_millis()),
    }
}

pub fn start_of_today(now: i64) -> i64 {
    let dt = Local
        .timestamp_millis_opt(now)
        .single()
        .unwrap_or_else(Local::now);
    let naive = dt.date_naive().and_hms_opt(0, 0, 0).unwrap_or_else(|| {
        NaiveDate::from_ymd_opt(1970, 1, 1)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap()
    });
    local_millis(naive).unwrap_or(now)
}

pub fn start_of_tomorrow(now: i64) -> i64 {
    let today = start_of_today(now);
    let dt = Local
        .timestamp_millis_opt(today)
        .single()
        .unwrap_or_else(Local::now);
    local_millis(
        (dt.date_naive() + Duration::days(1))
            .and_hms_opt(0, 0, 0)
            .unwrap(),
    )
    .unwrap_or(now)
}

pub fn start_of_next_week(now: i64) -> i64 {
    let today = start_of_today(now);
    let dt = Local
        .timestamp_millis_opt(today)
        .single()
        .unwrap_or_else(Local::now);
    let days = 7 - dt.weekday().num_days_from_monday() as i64;
    local_millis(
        (dt.date_naive() + Duration::days(days))
            .and_hms_opt(0, 0, 0)
            .unwrap(),
    )
    .unwrap_or(now)
}

// ---------------------------------------------------------------------------
// 行映射与校验
// ---------------------------------------------------------------------------

const SUMMARY_COLUMNS: &str = "n.id, n.title, n.content, n.status, n.priority, n.tags,
    n.due_at, n.remind_at, n.remind_lead_min, n.remind_repeat, n.snooze_until,
    n.pinned, n.done_at, n.updated_at, COALESCE(links.c, 0), n.kind";

const DETAIL_COLUMNS: &str = "n.id, n.title, n.content, n.status, n.priority, n.tags,
    n.due_at, n.remind_at, n.remind_lead_min, n.remind_repeat, n.snooze_until,
    n.pinned, n.done_at, n.updated_at, COALESCE(links.c, 0), n.kind, n.created_at";

fn row_to_summary(row: &Row) -> rusqlite::Result<NoteSummary> {
    Ok(NoteSummary {
        id: row.get(0)?,
        title: row.get(1)?,
        excerpt: clip(&row.get::<_, String>(2)?, 120),
        status: row.get(3)?,
        priority: row.get(4)?,
        tags: split_tags(&row.get::<_, String>(5)?),
        due_at: row.get(6)?,
        remind_at: row.get(7)?,
        remind_lead_min: row.get(8)?,
        repeat: row.get(9)?,
        snooze_until: row.get(10)?,
        pinned: row.get::<_, i64>(11)? != 0,
        done_at: row.get(12)?,
        updated_at: row.get(13)?,
        attachment_count: row.get(14)?,
        kind: row.get(15)?,
    })
}

fn row_to_detail(row: &Row) -> rusqlite::Result<NoteDetail> {
    Ok(NoteDetail {
        summary: row_to_summary(row)?,
        // DETAIL_COLUMNS 与 SUMMARY_COLUMNS 前 16 列一致，正文在第 3 列，
        // created_at 追加在最后（第 17 列）。
        content: row.get(2)?,
        created_at: row.get(16)?,
    })
}

#[derive(Debug, Clone)]
struct RawNote {
    title: String,
    content: String,
    status: String,
    kind: String,
    priority: i64,
    tags: Vec<String>,
    due_at: Option<i64>,
    remind_at: Option<i64>,
    lead_min: i64,
    repeat: String,
    snooze_until: Option<i64>,
    notified_at: Option<i64>,
    pinned: bool,
    deleted_from_status: Option<String>,
}

fn load_raw(conn: &Connection, id: i64) -> Result<Option<RawNote>, String> {
    conn.query_row(
        "SELECT title, content, status, priority, tags, due_at, remind_at,
                remind_lead_min, remind_repeat, snooze_until, notified_at, pinned,
                deleted_from_status, kind
         FROM notes WHERE id = ?1",
        params![id],
        |row| {
            Ok(RawNote {
                title: row.get(0)?,
                content: row.get(1)?,
                status: row.get(2)?,
                priority: row.get(3)?,
                tags: split_tags(&row.get::<_, String>(4)?),
                due_at: row.get(5)?,
                remind_at: row.get(6)?,
                lead_min: row.get(7)?,
                repeat: row.get(8)?,
                snooze_until: row.get(9)?,
                notified_at: row.get(10)?,
                pinned: row.get::<_, i64>(11)? != 0,
                deleted_from_status: row.get(12)?,
                kind: row.get(13)?,
            })
        },
    )
    .optional()
    .map_err(err)
}

fn validate_content(content: &str) -> Result<(), String> {
    if content.chars().count() > CONTENT_LIMIT {
        return Err(format!(
            "E_NOTES_TOO_LONG: 正文不能超过 {CONTENT_LIMIT} 字符"
        ));
    }
    Ok(())
}

fn validate_due(value: i64) -> Result<i64, String> {
    let min = NaiveDate::from_ymd_opt(2000, 1, 1)
        .and_then(|date| date.and_hms_opt(0, 0, 0))
        .and_then(local_millis)
        .unwrap_or(946656000000);
    let max = NaiveDate::from_ymd_opt(2100, 1, 1)
        .and_then(|date| date.and_hms_opt(0, 0, 0))
        .and_then(local_millis)
        .unwrap_or(4102444800000);
    if value < min || value > max {
        return Err("截止时间超出允许范围（2000-01-01 至 2100-01-01）".into());
    }
    Ok(value)
}

fn normalize_repeat(value: Option<&str>) -> String {
    match value.unwrap_or("none").trim() {
        "daily" | "weekdays" | "weekly" | "monthly" => value.unwrap_or("none").trim().to_string(),
        _ => "none".to_string(),
    }
}

/// 记录类型：待办 / 笔记 / 灵感 / 资料；未知值一律按待办处理
pub(crate) fn normalize_kind(value: Option<&str>) -> String {
    match value.unwrap_or("todo").trim() {
        "note" => "note",
        "idea" => "idea",
        "reference" => "reference",
        _ => "todo",
    }
    .to_string()
}

pub(crate) fn normalize_tags(values: Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for value in values {
        for tag in value.split([',', '，']) {
            let tag = tag.trim().trim_start_matches(['#', '＃']).trim();
            if tag.is_empty() {
                continue;
            }
            let tag = clip(tag, TAG_CHAR_LIMIT);
            if !seen.insert(tag.clone()) {
                continue;
            }
            out.push(tag);
            if out.len() >= TAG_LIMIT {
                return out;
            }
        }
    }
    out
}

fn split_tags(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|tag| !tag.is_empty())
        .map(str::to_string)
        .collect()
}

fn first_line(content: &str) -> String {
    content
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(|line| clip(line, TITLE_LIMIT))
        .unwrap_or_default()
}

fn clip(text: &str, limit: usize) -> String {
    text.chars().take(limit).collect()
}

fn escape_like(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

/// 区分「字段缺省」与「显式 null」：缺省 = 不动，null = 清空
fn double_option<'de, D>(deserializer: D) -> Result<Option<Option<i64>>, D::Error>
where
    D: Deserializer<'de>,
{
    struct DoubleOptionVisitor;

    impl<'de> serde::de::Visitor<'de> for DoubleOptionVisitor {
        type Value = Option<Option<i64>>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("一个时间戳、null 或缺失")
        }

        fn visit_none<E>(self) -> Result<Self::Value, E> {
            Ok(Some(None))
        }

        fn visit_unit<E>(self) -> Result<Self::Value, E> {
            Ok(Some(None))
        }

        fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
        where
            D: Deserializer<'de>,
        {
            Deserialize::deserialize(deserializer).map(Some)
        }
    }

    deserializer.deserialize_option(DoubleOptionVisitor)
}

#[cfg(test)]
pub(crate) fn test_pool(tag: &str) -> (std::path::PathBuf, DbPool) {
    use r2d2_sqlite::SqliteConnectionManager;
    let dir = crate::util::test_dir(tag);
    let path = dir.join("notes.db");
    let manager = SqliteConnectionManager::file(&path).with_init(|conn| {
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA foreign_keys = ON;
             PRAGMA busy_timeout = 5000;",
        )
    });
    let pool = r2d2::Pool::builder()
        .max_size(4)
        .build(manager)
        .expect("build pool");
    crate::db::init(&pool).expect("init db");
    (dir, pool)
}
