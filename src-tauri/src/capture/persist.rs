//! 抓包会话归档（R7）。
//!
//! 代理线程只做 `try_send`，落库与 body 存储都在独立 writer 线程里完成。
//! 队列满时丢弃归档、只累加计数，绝不阻塞转发。

use super::store::{BodyPayload, FlowRecord, HeaderEntry};
use crate::docs::masking::{is_sensitive_name, SecretSet};
use crate::state::DbPool;
use base64::Engine as _;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, RecvTimeoutError, SyncSender};
use std::sync::Arc;
use std::time::Duration;

pub const QUEUE_CAPACITY: usize = 4096;
pub const BATCH_SIZE: usize = 200;
pub const INLINE_BODY_LIMIT: usize = 8 * 1024;
pub const TEXT_BODY_LIMIT: usize = 256 * 1024;
pub const MAX_SESSIONS: usize = 20;
pub const MAX_TOTAL_BYTES: u64 = 1024 * 1024 * 1024;
const FLUSH_INTERVAL: Duration = Duration::from_secs(2);
const PREVIEW_BINARY_LIMIT: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ArchiveMode {
    #[default]
    Memory,
    ArchiveMeta,
    ArchiveFull,
}

impl ArchiveMode {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value.trim().to_ascii_lowercase().as_str() {
            "memory" | "" => Ok(Self::Memory),
            "archive-meta" => Ok(Self::ArchiveMeta),
            "archive-full" => Ok(Self::ArchiveFull),
            other => Err(format!("E_ARG: 未知的归档模式 {other}")),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Memory => "memory",
            Self::ArchiveMeta => "archive-meta",
            Self::ArchiveFull => "archive-full",
        }
    }

    pub fn stores_bodies(self) -> bool {
        self == Self::ArchiveFull
    }

    pub fn is_archive(self) -> bool {
        self != Self::Memory
    }
}

pub enum ArchiveMessage {
    Flow(Box<FlowRecord>),
    Finish,
}

#[derive(Clone)]
pub struct ArchiveSender {
    tx: SyncSender<ArchiveMessage>,
    dropped: Arc<AtomicU64>,
}

impl ArchiveSender {
    /// 不阻塞转发：队列满时只累加丢弃计数。
    pub fn send(&self, record: &FlowRecord) {
        let message = ArchiveMessage::Flow(Box::new(record.clone()));
        if let Err(std::sync::mpsc::TrySendError::Full(_)) = self.tx.try_send(message) {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }
}

#[derive(Debug, Default, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveStats {
    pub persisted: u64,
    pub dropped: u64,
    pub body_bytes: u64,
    pub errors: u64,
}

pub struct ArchiveWriter {
    sender: ArchiveSender,
    tx: SyncSender<ArchiveMessage>,
    join: Option<std::thread::JoinHandle<ArchiveStats>>,
}

impl ArchiveWriter {
    pub fn start(
        pool: DbPool,
        data_dir: PathBuf,
        session_id: i64,
        mode: ArchiveMode,
        exclude: Vec<String>,
    ) -> Result<Self, String> {
        let (tx, rx) = sync_channel::<ArchiveMessage>(QUEUE_CAPACITY);
        let dropped = Arc::new(AtomicU64::new(0));
        let sender = ArchiveSender {
            tx: tx.clone(),
            dropped: dropped.clone(),
        };
        let join = std::thread::Builder::new()
            .name(format!("capture-archive-{session_id}"))
            .spawn(move || run_writer(pool, data_dir, session_id, mode, exclude, rx, dropped))
            .map_err(|error| format!("E_THREAD: 启动归档线程失败 {error}"))?;
        Ok(Self {
            sender,
            tx,
            join: Some(join),
        })
    }

    pub fn sender(&self) -> ArchiveSender {
        self.sender.clone()
    }

    /// 先投递 Finish 再 join，保证最后一批落盘。
    pub fn finish(mut self) -> ArchiveStats {
        let _ = self.tx.send(ArchiveMessage::Finish);
        match self.join.take() {
            Some(join) => join.join().unwrap_or_else(|_| ArchiveStats::default()),
            None => ArchiveStats::default(),
        }
    }
}

fn run_writer(
    pool: DbPool,
    data_dir: PathBuf,
    session_id: i64,
    mode: ArchiveMode,
    exclude: Vec<String>,
    rx: Receiver<ArchiveMessage>,
    dropped: Arc<AtomicU64>,
) -> ArchiveStats {
    let mut stats = ArchiveStats::default();
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(error) => {
            log::error!("归档会话 {session_id} 获取数据库连接失败：{error}");
            while let Ok(message) = rx.recv() {
                if matches!(message, ArchiveMessage::Finish) {
                    break;
                }
            }
            stats.dropped = dropped.load(Ordering::Relaxed);
            return stats;
        }
    };
    let root = body_root(&data_dir);
    let mut batch: Vec<FlowRecord> = Vec::with_capacity(BATCH_SIZE);
    let mut flush = |batch: &mut Vec<FlowRecord>, stats: &mut ArchiveStats| {
        if batch.is_empty() {
            return;
        }
        match flush_batch(&mut conn, &root, session_id, mode, batch, stats) {
            Ok(()) => {}
            Err(error) => {
                stats.errors += 1;
                log::warn!("归档会话 {session_id} 批量写入失败：{error}");
            }
        }
        batch.clear();
    };

    loop {
        match rx.recv_timeout(FLUSH_INTERVAL) {
            Ok(ArchiveMessage::Flow(record)) => {
                if !excluded(&record, &exclude) {
                    batch.push(*record);
                }
                if batch.len() >= BATCH_SIZE {
                    flush(&mut batch, &mut stats);
                }
            }
            Ok(ArchiveMessage::Finish) => {
                flush(&mut batch, &mut stats);
                break;
            }
            Err(RecvTimeoutError::Timeout) => flush(&mut batch, &mut stats),
            Err(RecvTimeoutError::Disconnected) => {
                flush(&mut batch, &mut stats);
                break;
            }
        }
    }

    stats.dropped = dropped.load(Ordering::Relaxed);
    let flow_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM capture_flows WHERE session_id = ?1",
            params![session_id],
            |row| row.get(0),
        )
        .unwrap_or(stats.persisted as i64);
    if let Err(error) = conn.execute(
        "UPDATE capture_sessions SET ended_at = ?2, flow_count = ?3, dropped = ?4 WHERE id = ?1",
        params![
            session_id,
            crate::util::now_ms(),
            flow_count,
            stats.dropped as i64
        ],
    ) {
        log::warn!("归档会话 {session_id} 收尾失败：{error}");
    }
    stats.persisted = flow_count.max(0) as u64;
    stats
}

fn excluded(record: &FlowRecord, exclude: &[String]) -> bool {
    let host = record.brief.host.to_ascii_lowercase();
    exclude.iter().any(|pattern| {
        let pattern = pattern.trim().trim_start_matches("*.").to_ascii_lowercase();
        !pattern.is_empty() && (host == pattern || host.ends_with(&format!(".{pattern}")))
    })
}

fn flush_batch(
    conn: &mut rusqlite::Connection,
    root: &Path,
    session_id: i64,
    mode: ArchiveMode,
    batch: &mut [FlowRecord],
    stats: &mut ArchiveStats,
) -> Result<(), String> {
    let tx = conn
        .transaction()
        .map_err(|error| format!("E_ARCHIVE: 开启事务失败 {error}"))?;
    for record in batch.iter() {
        let mut secrets = SecretSet::default();
        for header in record
            .request_headers
            .iter()
            .chain(record.response_headers.iter())
        {
            collect_header_secrets(&mut secrets, header);
        }
        let request_headers = mask_headers(&record.request_headers, &secrets);
        let response_headers = mask_headers(&record.response_headers, &secrets);
        let (request_body_sha, response_body_sha) = if mode.stores_bodies() {
            (
                store_body(&tx, root, &record.request_body, &secrets, stats)?,
                store_body(&tx, root, &record.response_body, &secrets, stats)?,
            )
        } else {
            (None, None)
        };
        let brief = &record.brief;
        tx.execute(
            "INSERT INTO capture_flows
                (session_id, flow_id, started_at, method, scheme, host, path, http_version,
                 status, duration_ms, request_size, response_size, content_type, client_addr,
                 error, mapped, modified, passthrough, request_headers_json,
                 response_headers_json, request_body_sha, response_body_sha)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15,
                     ?16, ?17, ?18, ?19, ?20, ?21, ?22)",
            params![
                session_id,
                brief.id as i64,
                brief.started_at,
                brief.method,
                brief.scheme,
                brief.host,
                brief.path,
                brief.http_version,
                brief.status.map(i64::from),
                brief.duration_ms,
                brief.request_size as i64,
                brief.response_size as i64,
                brief.content_type,
                brief.client_addr,
                brief.error,
                i64::from(brief.mapped),
                i64::from(brief.modified),
                i64::from(brief.passthrough),
                serde_json::to_string(&request_headers).unwrap_or_else(|_| "[]".into()),
                serde_json::to_string(&response_headers).unwrap_or_else(|_| "[]".into()),
                request_body_sha,
                response_body_sha,
            ],
        )
        .map_err(|error| format!("E_ARCHIVE: 写入流量失败 {error}"))?;
    }
    tx.commit()
        .map_err(|error| format!("E_ARCHIVE: 提交失败 {error}"))?;
    stats.persisted += batch.len() as u64;
    Ok(())
}

fn collect_header_secrets(set: &mut SecretSet, header: &HeaderEntry) {
    if !is_sensitive_name(&header.name) {
        return;
    }
    let value = header.value.trim();
    set.add(value);
    for scheme in ["bearer ", "basic ", "token "] {
        if value.to_ascii_lowercase().starts_with(scheme) {
            set.add(value[scheme.len()..].trim());
        }
    }
    for part in value.split(';') {
        if let Some((name, value)) = part.split_once('=') {
            if is_sensitive_name(name.trim()) {
                set.add(value.trim().trim_matches('"'));
            }
        }
    }
}

fn mask_headers(headers: &[HeaderEntry], secrets: &SecretSet) -> Vec<HeaderEntry> {
    headers
        .iter()
        .map(|header| HeaderEntry {
            name: header.name.clone(),
            value: if is_sensitive_name(&header.name) {
                crate::secret::MASK.to_string()
            } else {
                secrets.mask_text(&header.value)
            },
        })
        .collect()
}

fn store_body(
    conn: &rusqlite::Connection,
    root: &Path,
    body: &BodyPayload,
    secrets: &SecretSet,
    stats: &mut ArchiveStats,
) -> Result<Option<String>, String> {
    if body.size == 0 && body.text.is_none() && body.base64.is_none() {
        return Ok(None);
    }
    let mut binary = body.binary;
    let mut truncated = body.truncated;
    let bytes = if let Some(text) = &body.text {
        if text.is_empty() {
            return Ok(None);
        }
        let masked = if body.content_type.to_ascii_lowercase().contains("json") {
            secrets.mask_json(text)
        } else {
            secrets.mask_text(text)
        };
        if masked.len() > TEXT_BODY_LIMIT {
            truncated = true;
            masked.into_bytes()[..TEXT_BODY_LIMIT].to_vec()
        } else {
            masked.into_bytes()
        }
    } else if let Some(encoded) = &body.base64 {
        match base64::engine::general_purpose::STANDARD.decode(encoded) {
            Ok(bytes) => {
                binary = true;
                bytes
            }
            Err(_) => return Ok(None),
        }
    } else {
        return Ok(None);
    };
    if bytes.is_empty() {
        return Ok(None);
    }
    let sha = sha_hex(&bytes);
    if conn
        .query_row(
            "SELECT 1 FROM capture_bodies WHERE sha256 = ?1",
            params![sha],
            |_| Ok(()),
        )
        .optional()
        .map_err(|error| format!("E_ARCHIVE: 查询 body 失败 {error}"))?
        .is_some()
    {
        return Ok(Some(sha));
    }

    let inline = !binary && bytes.len() <= INLINE_BODY_LIMIT;
    let (storage, inline_text, file_path) = if inline {
        (
            "blob",
            Some(String::from_utf8_lossy(&bytes).to_string()),
            None,
        )
    } else {
        let relative = body_path(&sha, binary);
        let target = root.join(relative.replace('/', std::path::MAIN_SEPARATOR_STR));
        if !target.exists() {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|error| format!("E_ARCHIVE: 创建 body 目录失败 {error}"))?;
            }
            std::fs::write(&target, &bytes)
                .map_err(|error| format!("E_ARCHIVE: 写入 body 文件失败 {error}"))?;
        }
        ("file", None, Some(relative))
    };
    conn.execute(
        "INSERT OR IGNORE INTO capture_bodies
            (sha256, size, truncated, binary, content_type, storage, inline_text, file_path)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            sha,
            bytes.len() as i64,
            i64::from(truncated),
            i64::from(binary),
            body.content_type,
            storage,
            inline_text,
            file_path,
        ],
    )
    .map_err(|error| format!("E_ARCHIVE: 写入 body 记录失败 {error}"))?;
    stats.body_bytes = stats.body_bytes.saturating_add(bytes.len() as u64);
    Ok(Some(sha))
}

fn sha_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn body_root(data_dir: &Path) -> PathBuf {
    data_dir.join("capture").join("bodies")
}

fn body_path(sha: &str, binary: bool) -> String {
    let extension = if binary { "bin" } else { "txt" };
    format!("{}/{sha}.{extension}", &sha[..2])
}

pub fn create_session(
    pool: &DbPool,
    config: &super::CaptureConfig,
    bind_addr: &str,
    port: u16,
) -> Result<i64, String> {
    let conn = pool
        .get()
        .map_err(|error| format!("E_ARCHIVE: 获取数据库连接失败 {error}"))?;
    let name = config
        .archive_name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("会话 {}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S")));
    let filter = json!({
        "ignorePatterns": config.ignore_patterns,
        "sslHosts": config.ssl_hosts,
        "archiveExclude": config.archive_exclude,
    });
    conn.execute(
        "INSERT INTO capture_sessions
            (name, mode, bind_addr, port, started_at, filter_json, app_version)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            name,
            config.archive_mode.as_str(),
            bind_addr,
            i64::from(port),
            crate::util::now_ms(),
            filter.to_string(),
            env!("CARGO_PKG_VERSION"),
        ],
    )
    .map_err(|error| format!("E_ARCHIVE: 创建会话失败 {error}"))?;
    Ok(conn.last_insert_rowid())
}

pub fn delete_session(pool: &DbPool, data_dir: &Path, id: i64) -> Result<(), String> {
    let mut conn = pool
        .get()
        .map_err(|error| format!("E_ARCHIVE: 获取数据库连接失败 {error}"))?;
    delete_session_rows(&mut conn, &body_root(data_dir), id)?;
    cleanup_orphans(&mut conn, &body_root(data_dir))?;
    Ok(())
}

fn delete_session_rows(
    conn: &mut rusqlite::Connection,
    root: &Path,
    id: i64,
) -> Result<(), String> {
    let shas = session_body_shas(conn, id)?;
    let tx = conn
        .transaction()
        .map_err(|error| format!("E_ARCHIVE: 开启事务失败 {error}"))?;
    tx.execute(
        "DELETE FROM capture_flows WHERE session_id = ?1",
        params![id],
    )
    .map_err(|error| format!("E_ARCHIVE: 删除流量失败 {error}"))?;
    tx.execute("DELETE FROM capture_sessions WHERE id = ?1", params![id])
        .map_err(|error| format!("E_ARCHIVE: 删除会话失败 {error}"))?;
    tx.commit()
        .map_err(|error| format!("E_ARCHIVE: 提交失败 {error}"))?;
    for sha in shas {
        if !body_referenced(conn, &sha)? {
            remove_body(conn, root, &sha)?;
        }
    }
    Ok(())
}

fn downgrade_session(conn: &mut rusqlite::Connection, root: &Path, id: i64) -> Result<(), String> {
    let shas = session_body_shas(conn, id)?;
    let tx = conn
        .transaction()
        .map_err(|error| format!("E_ARCHIVE: 开启事务失败 {error}"))?;
    tx.execute(
        "UPDATE capture_flows SET request_body_sha = NULL, response_body_sha = NULL
         WHERE session_id = ?1",
        params![id],
    )
    .map_err(|error| format!("E_ARCHIVE: 降级会话失败 {error}"))?;
    tx.execute(
        "UPDATE capture_sessions SET mode = 'archive-meta' WHERE id = ?1",
        params![id],
    )
    .map_err(|error| format!("E_ARCHIVE: 降级会话失败 {error}"))?;
    tx.commit()
        .map_err(|error| format!("E_ARCHIVE: 提交失败 {error}"))?;
    for sha in shas {
        if !body_referenced(conn, &sha)? {
            remove_body(conn, root, &sha)?;
        }
    }
    Ok(())
}

fn session_body_shas(conn: &rusqlite::Connection, id: i64) -> Result<Vec<String>, String> {
    let mut statement = conn
        .prepare(
            "SELECT request_body_sha FROM capture_flows
             WHERE session_id = ?1 AND request_body_sha IS NOT NULL
             UNION
             SELECT response_body_sha FROM capture_flows
             WHERE session_id = ?1 AND response_body_sha IS NOT NULL",
        )
        .map_err(|error| format!("E_ARCHIVE: 查询 body 失败 {error}"))?;
    let rows = statement
        .query_map(params![id], |row| row.get::<_, String>(0))
        .map_err(|error| format!("E_ARCHIVE: 查询 body 失败 {error}"))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("E_ARCHIVE: 查询 body 失败 {error}"))
}

fn body_referenced(conn: &rusqlite::Connection, sha: &str) -> Result<bool, String> {
    conn.query_row(
        "SELECT 1 FROM capture_flows
         WHERE request_body_sha = ?1 OR response_body_sha = ?1 LIMIT 1",
        params![sha],
        |_| Ok(()),
    )
    .optional()
    .map(|found| found.is_some())
    .map_err(|error| format!("E_ARCHIVE: 查询 body 引用失败 {error}"))
}

fn remove_body(conn: &rusqlite::Connection, root: &Path, sha: &str) -> Result<(), String> {
    let file: Option<String> = conn
        .query_row(
            "SELECT file_path FROM capture_bodies WHERE sha256 = ?1",
            params![sha],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| format!("E_ARCHIVE: 查询 body 失败 {error}"))?
        .flatten();
    conn.execute("DELETE FROM capture_bodies WHERE sha256 = ?1", params![sha])
        .map_err(|error| format!("E_ARCHIVE: 删除 body 记录失败 {error}"))?;
    if let Some(file) = file {
        let _ = std::fs::remove_file(root.join(file.replace('/', std::path::MAIN_SEPARATOR_STR)));
    }
    Ok(())
}

/// 保留最近 20 个会话 / 1GB；full 会话先降级再删除，最后清掉孤儿 body 与文件。
pub fn cleanup(pool: &DbPool, data_dir: &Path) -> Result<usize, String> {
    let mut conn = pool
        .get()
        .map_err(|error| format!("E_ARCHIVE: 获取数据库连接失败 {error}"))?;
    let root = body_root(data_dir);
    // 上次异常退出留下的会话：按已落库的条数收尾，避免永远显示“进行中”
    if let Err(error) = conn.execute(
        "UPDATE capture_sessions
         SET ended_at = COALESCE(ended_at, started_at),
             flow_count = (SELECT COUNT(*) FROM capture_flows f WHERE f.session_id = capture_sessions.id)
         WHERE ended_at IS NULL",
        [],
    ) {
        log::warn!("收尾残留抓包会话失败：{error}");
    }

    let sessions: Vec<(i64, String)> = {
        let mut statement = conn
            .prepare("SELECT id, mode FROM capture_sessions ORDER BY started_at DESC, id DESC")
            .map_err(|error| format!("E_ARCHIVE: 查询会话失败 {error}"))?;
        let rows = statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|error| format!("E_ARCHIVE: 查询会话失败 {error}"))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("E_ARCHIVE: 查询会话失败 {error}"))?
    };

    let mut removed = 0usize;
    for (id, _) in sessions.iter().skip(MAX_SESSIONS) {
        delete_session_rows(&mut conn, &root, *id)?;
        removed += 1;
    }
    loop {
        if total_body_bytes(&conn)? <= MAX_TOTAL_BYTES {
            break;
        }
        let oldest: Option<(i64, String)> = conn
            .query_row(
                "SELECT id, mode FROM capture_sessions ORDER BY started_at ASC, id ASC LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(|error| format!("E_ARCHIVE: 查询会话失败 {error}"))?;
        let Some((id, mode)) = oldest else {
            break;
        };
        if mode == ArchiveMode::ArchiveFull.as_str() {
            downgrade_session(&mut conn, &root, id)?;
        } else {
            delete_session_rows(&mut conn, &root, id)?;
            removed += 1;
        }
    }
    cleanup_orphans(&mut conn, &root)?;
    Ok(removed)
}

fn total_body_bytes(conn: &rusqlite::Connection) -> Result<u64, String> {
    conn.query_row(
        "SELECT COALESCE(SUM(b.size), 0) FROM capture_bodies b
         WHERE EXISTS (
             SELECT 1 FROM capture_flows f
             WHERE f.request_body_sha = b.sha256 OR f.response_body_sha = b.sha256
         )",
        [],
        |row| row.get::<_, i64>(0),
    )
    .map(|value| value.max(0) as u64)
    .map_err(|error| format!("E_ARCHIVE: 统计体积失败 {error}"))
}

fn cleanup_orphans(conn: &mut rusqlite::Connection, root: &Path) -> Result<(), String> {
    let orphan_files: Vec<String> = {
        let mut statement = conn
            .prepare(
                "SELECT file_path FROM capture_bodies b WHERE file_path IS NOT NULL AND NOT EXISTS (
                     SELECT 1 FROM capture_flows f
                     WHERE f.request_body_sha = b.sha256 OR f.response_body_sha = b.sha256
                 )",
            )
            .map_err(|error| format!("E_ARCHIVE: 查询孤儿 body 失败 {error}"))?;
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|error| format!("E_ARCHIVE: 查询孤儿 body 失败 {error}"))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("E_ARCHIVE: 查询孤儿 body 失败 {error}"))?
    };
    conn.execute(
        "DELETE FROM capture_bodies WHERE NOT EXISTS (
             SELECT 1 FROM capture_flows f
             WHERE f.request_body_sha = capture_bodies.sha256
                OR f.response_body_sha = capture_bodies.sha256
         )",
        [],
    )
    .map_err(|error| format!("E_ARCHIVE: 清理孤儿 body 失败 {error}"))?;
    let mut known: HashSet<String> = HashSet::new();
    {
        let mut statement = conn
            .prepare("SELECT file_path FROM capture_bodies WHERE file_path IS NOT NULL")
            .map_err(|error| format!("E_ARCHIVE: 查询 body 文件失败 {error}"))?;
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|error| format!("E_ARCHIVE: 查询 body 文件失败 {error}"))?;
        for row in rows {
            known.insert(
                row.map_err(|error| format!("E_ARCHIVE: 查询 body 文件失败 {error}"))?
                    .replace('/', std::path::MAIN_SEPARATOR_STR),
            );
        }
    }
    for file in orphan_files {
        let _ = std::fs::remove_file(root.join(file.replace('/', std::path::MAIN_SEPARATOR_STR)));
    }
    prune_unreferenced_files(root, root, &known);
    Ok(())
}

fn prune_unreferenced_files(root: &Path, dir: &Path, known: &HashSet<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            prune_unreferenced_files(root, &path, known);
            if std::fs::read_dir(&path)
                .map(|mut items| items.next().is_none())
                .unwrap_or(false)
            {
                let _ = std::fs::remove_dir(&path);
            }
            continue;
        }
        let Ok(relative) = path.strip_prefix(root) else {
            continue;
        };
        if !known.contains(&relative.to_string_lossy().to_string()) {
            let _ = std::fs::remove_file(&path);
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummary {
    pub id: i64,
    pub name: String,
    pub mode: String,
    pub bind_addr: String,
    pub port: u16,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub flow_count: i64,
    pub dropped: i64,
    pub bytes: i64,
    pub app_version: Option<String>,
}

pub fn list_sessions(pool: &DbPool, limit: usize) -> Result<Vec<SessionSummary>, String> {
    let conn = pool
        .get()
        .map_err(|error| format!("E_ARCHIVE: 获取数据库连接失败 {error}"))?;
    let limit = limit.clamp(1, 200);
    let mut statement = conn
        .prepare(
            "SELECT s.id, s.name, s.mode, s.bind_addr, s.port, s.started_at, s.ended_at,
                    s.flow_count, s.dropped, s.app_version,
                    COALESCE((
                        SELECT SUM(b.size) FROM capture_bodies b
                        WHERE b.sha256 IN (
                            SELECT request_body_sha FROM capture_flows f
                            WHERE f.session_id = s.id AND request_body_sha IS NOT NULL
                            UNION
                            SELECT response_body_sha FROM capture_flows f
                            WHERE f.session_id = s.id AND response_body_sha IS NOT NULL
                        )
                    ), 0)
             FROM capture_sessions s
             ORDER BY s.started_at DESC, s.id DESC
             LIMIT ?1",
        )
        .map_err(|error| format!("E_ARCHIVE: 查询会话失败 {error}"))?;
    let rows = statement
        .query_map(params![limit as i64], |row| {
            Ok(SessionSummary {
                id: row.get(0)?,
                name: row.get(1)?,
                mode: row.get(2)?,
                bind_addr: row.get(3)?,
                port: row.get::<_, i64>(4)? as u16,
                started_at: row.get(5)?,
                ended_at: row.get(6)?,
                flow_count: row.get(7)?,
                dropped: row.get(8)?,
                app_version: row.get(9)?,
                bytes: row.get(10)?,
            })
        })
        .map_err(|error| format!("E_ARCHIVE: 查询会话失败 {error}"))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("E_ARCHIVE: 查询会话失败 {error}"))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchivedFlowBrief {
    pub id: u64,
    pub started_at: i64,
    pub method: String,
    pub scheme: String,
    pub host: String,
    pub path: String,
    pub http_version: String,
    pub status: Option<u16>,
    pub duration_ms: i64,
    pub request_size: u64,
    pub response_size: u64,
    pub content_type: String,
    pub client_addr: String,
    pub error: Option<String>,
    pub mapped: bool,
    pub modified: bool,
    pub passthrough: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionFlows {
    pub total: i64,
    pub items: Vec<ArchivedFlowBrief>,
}

pub fn session_flows(
    pool: &DbPool,
    session_id: i64,
    keyword: Option<&str>,
    method: Option<&str>,
    status: Option<&str>,
    offset: usize,
    limit: usize,
) -> Result<SessionFlows, String> {
    let conn = pool
        .get()
        .map_err(|error| format!("E_ARCHIVE: 获取数据库连接失败 {error}"))?;
    let mut where_sql = String::from(" WHERE session_id = ?1");
    let mut values: Vec<rusqlite::types::Value> = vec![rusqlite::types::Value::Integer(session_id)];
    if let Some(keyword) = keyword.map(str::trim).filter(|value| !value.is_empty()) {
        where_sql.push_str(&format!(
            " AND (host LIKE ?{} OR path LIKE ?{})",
            values.len() + 1,
            values.len() + 1
        ));
        values.push(rusqlite::types::Value::Text(format!("%{keyword}%")));
    }
    if let Some(method) = method.map(str::trim).filter(|value| !value.is_empty()) {
        where_sql.push_str(&format!(" AND method = ?{}", values.len() + 1));
        values.push(rusqlite::types::Value::Text(method.to_ascii_uppercase()));
    }
    if let Some(status) = status.map(str::trim).filter(|value| !value.is_empty()) {
        match status {
            "error" => where_sql.push_str(" AND status IS NULL"),
            "2xx" | "3xx" | "4xx" | "5xx" => {
                let base: i64 = match status {
                    "2xx" => 200,
                    "3xx" => 300,
                    "4xx" => 400,
                    _ => 500,
                };
                where_sql.push_str(&format!(
                    " AND status >= ?{0} AND status < ?{1}",
                    values.len() + 1,
                    values.len() + 2
                ));
                values.push(rusqlite::types::Value::Integer(base));
                values.push(rusqlite::types::Value::Integer(base + 100));
            }
            other => {
                let Ok(code) = other.parse::<i64>() else {
                    return Err(format!("E_ARG: 未知的状态过滤 {other}"));
                };
                where_sql.push_str(&format!(" AND status = ?{}", values.len() + 1));
                values.push(rusqlite::types::Value::Integer(code));
            }
        }
    }

    let total: i64 = conn
        .query_row(
            &format!("SELECT COUNT(*) FROM capture_flows{where_sql}"),
            rusqlite::params_from_iter(values.iter()),
            |row| row.get(0),
        )
        .map_err(|error| format!("E_ARCHIVE: 查询流量失败 {error}"))?;

    let limit = limit.clamp(1, 500);
    let offset = offset as i64;
    values.push(rusqlite::types::Value::Integer(limit as i64));
    values.push(rusqlite::types::Value::Integer(offset));
    let sql = format!(
        "SELECT flow_id, started_at, method, scheme, host, path, http_version, status,
                duration_ms, request_size, response_size, content_type, client_addr, error,
                mapped, modified, passthrough
         FROM capture_flows{where_sql}
         ORDER BY started_at ASC, id ASC
         LIMIT ?{} OFFSET ?{}",
        values.len() - 1,
        values.len()
    );
    let mut statement = conn
        .prepare(&sql)
        .map_err(|error| format!("E_ARCHIVE: 查询流量失败 {error}"))?;
    let rows = statement
        .query_map(rusqlite::params_from_iter(values.iter()), |row| {
            Ok(ArchivedFlowBrief {
                id: row.get::<_, i64>(0)? as u64,
                started_at: row.get(1)?,
                method: row.get(2)?,
                scheme: row.get(3)?,
                host: row.get(4)?,
                path: row.get(5)?,
                http_version: row.get(6)?,
                status: row.get::<_, Option<i64>>(7)?.map(|value| value as u16),
                duration_ms: row.get(8)?,
                request_size: row.get::<_, i64>(9)? as u64,
                response_size: row.get::<_, i64>(10)? as u64,
                content_type: row.get(11)?,
                client_addr: row.get(12)?,
                error: row.get(13)?,
                mapped: row.get::<_, i64>(14)? != 0,
                modified: row.get::<_, i64>(15)? != 0,
                passthrough: row.get::<_, i64>(16)? != 0,
            })
        })
        .map_err(|error| format!("E_ARCHIVE: 查询流量失败 {error}"))?;
    let items = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("E_ARCHIVE: 查询流量失败 {error}"))?;
    Ok(SessionFlows { total, items })
}

pub fn session_flow_detail(
    pool: &DbPool,
    data_dir: &Path,
    session_id: i64,
    flow_id: u64,
) -> Result<super::store::FlowDetail, String> {
    let conn = pool
        .get()
        .map_err(|error| format!("E_ARCHIVE: 获取数据库连接失败 {error}"))?;
    let row = conn
        .query_row(
            "SELECT flow_id, started_at, method, scheme, host, path, http_version, status,
                    duration_ms, request_size, response_size, content_type, client_addr, error,
                    mapped, modified, passthrough, request_headers_json, response_headers_json,
                    request_body_sha, response_body_sha
             FROM capture_flows WHERE session_id = ?1 AND flow_id = ?2",
            params![session_id, flow_id as i64],
            |row| {
                Ok((
                    super::store::FlowBrief {
                        id: row.get::<_, i64>(0)? as u64,
                        started_at: row.get(1)?,
                        method: row.get(2)?,
                        scheme: row.get(3)?,
                        host: row.get(4)?,
                        path: row.get(5)?,
                        http_version: row.get(6)?,
                        status: row.get::<_, Option<i64>>(7)?.map(|value| value as u16),
                        duration_ms: row.get(8)?,
                        request_size: row.get::<_, i64>(9)? as u64,
                        response_size: row.get::<_, i64>(10)? as u64,
                        content_type: row.get(11)?,
                        client_addr: row.get(12)?,
                        error: row.get(13)?,
                        mapped: row.get::<_, i64>(14)? != 0,
                        modified: row.get::<_, i64>(15)? != 0,
                        remote: None,
                        passthrough: row.get::<_, i64>(16)? != 0,
                    },
                    row.get::<_, Option<String>>(17)?,
                    row.get::<_, Option<String>>(18)?,
                    row.get::<_, Option<String>>(19)?,
                    row.get::<_, Option<String>>(20)?,
                ))
            },
        )
        .optional()
        .map_err(|error| format!("E_ARCHIVE: 查询流量失败 {error}"))?
        .ok_or_else(|| format!("E_FLOW_NOT_FOUND: 会话 {session_id} 中流量 #{flow_id} 不存在"))?;
    let (brief, request_headers, response_headers, request_sha, response_sha) = row;
    Ok(super::store::FlowDetail {
        brief,
        request_headers: parse_headers(request_headers.as_deref()),
        response_headers: parse_headers(response_headers.as_deref()),
        request_body: load_body(&conn, data_dir, request_sha.as_deref())?,
        response_body: load_body(&conn, data_dir, response_sha.as_deref())?,
    })
}

fn parse_headers(raw: Option<&str>) -> Vec<HeaderEntry> {
    raw.and_then(|raw| serde_json::from_str::<Vec<HeaderEntry>>(raw).ok())
        .unwrap_or_default()
}

type BodyRow = (
    i64,
    i64,
    i64,
    String,
    String,
    Option<String>,
    Option<String>,
);

fn load_body(
    conn: &rusqlite::Connection,
    data_dir: &Path,
    sha: Option<&str>,
) -> Result<BodyPayload, String> {
    let Some(sha) = sha else {
        return Ok(BodyPayload::default());
    };
    let row: Option<BodyRow> = conn
        .query_row(
            "SELECT size, truncated, binary, content_type, storage, inline_text, file_path
             FROM capture_bodies WHERE sha256 = ?1",
            params![sha],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                ))
            },
        )
        .optional()
        .map_err(|error| format!("E_ARCHIVE: 查询 body 失败 {error}"))?;
    let Some((size, truncated, binary, content_type, storage, inline, file)) = row else {
        return Ok(BodyPayload::default());
    };
    let mut body = BodyPayload {
        size: size.max(0) as u64,
        truncated: truncated != 0,
        binary: binary != 0,
        text: None,
        base64: None,
        content_type,
    };
    let bytes = if storage == "blob" {
        inline.map(String::into_bytes)
    } else {
        file.map(|file| {
            std::fs::read(
                body_root(data_dir).join(file.replace('/', std::path::MAIN_SEPARATOR_STR)),
            )
        })
        .transpose()
        .map_err(|error| format!("E_ARCHIVE: 读取 body 失败 {error}"))?
    };
    match bytes {
        Some(bytes) if binary != 0 => {
            if bytes.len() <= PREVIEW_BINARY_LIMIT {
                body.base64 = Some(base64::engine::general_purpose::STANDARD.encode(bytes));
            }
        }
        Some(bytes) => body.text = Some(String::from_utf8_lossy(&bytes).to_string()),
        None => {}
    }
    Ok(body)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompareSide {
    pub id: i64,
    pub name: String,
    pub started_at: i64,
    pub flow_count: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompareItem {
    pub method: String,
    pub host: String,
    pub path: String,
    pub a_count: i64,
    pub b_count: i64,
    pub a_avg_ms: f64,
    pub b_avg_ms: f64,
    pub delta_pct: Option<f64>,
    pub a_error_rate: f64,
    pub b_error_rate: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompareOnly {
    pub method: String,
    pub host: String,
    pub path: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionCompare {
    pub a: CompareSide,
    pub b: CompareSide,
    pub items: Vec<CompareItem>,
    pub only_a: Vec<CompareOnly>,
    pub only_b: Vec<CompareOnly>,
}

struct CompareRow {
    method: String,
    host: String,
    path: String,
    count: i64,
    avg_ms: f64,
    errors: i64,
}

fn compare_side(
    conn: &rusqlite::Connection,
    id: i64,
) -> Result<(CompareSide, Vec<CompareRow>), String> {
    let side = conn
        .query_row(
            "SELECT id, name, started_at, flow_count FROM capture_sessions WHERE id = ?1",
            params![id],
            |row| {
                Ok(CompareSide {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    started_at: row.get(2)?,
                    flow_count: row.get(3)?,
                })
            },
        )
        .optional()
        .map_err(|error| format!("E_ARCHIVE: 查询会话失败 {error}"))?
        .ok_or_else(|| format!("E_ARCHIVE: 会话 {id} 不存在"))?;
    let mut statement = conn
        .prepare(
            "SELECT method, host, path, COUNT(*), AVG(duration_ms), SUM(status IS NULL)
             FROM capture_flows WHERE session_id = ?1
             GROUP BY method, host, path",
        )
        .map_err(|error| format!("E_ARCHIVE: 对比会话失败 {error}"))?;
    let rows = statement
        .query_map(params![id], |row| {
            Ok(CompareRow {
                method: row.get(0)?,
                host: row.get(1)?,
                path: row.get(2)?,
                count: row.get(3)?,
                avg_ms: row.get(4)?,
                errors: row.get(5)?,
            })
        })
        .map_err(|error| format!("E_ARCHIVE: 对比会话失败 {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("E_ARCHIVE: 对比会话失败 {error}"))?;
    Ok((side, rows))
}

pub fn compare_sessions(pool: &DbPool, a: i64, b: i64) -> Result<SessionCompare, String> {
    if a == b {
        return Err("E_ARG: 请选择两个不同的会话".into());
    }
    let conn = pool
        .get()
        .map_err(|error| format!("E_ARCHIVE: 获取数据库连接失败 {error}"))?;
    let (side_a, rows_a) = compare_side(&conn, a)?;
    let (side_b, rows_b) = compare_side(&conn, b)?;
    let mut index_b: std::collections::HashMap<(String, String, String), CompareRow> = rows_b
        .into_iter()
        .map(|row| {
            (
                (row.method.clone(), row.host.clone(), row.path.clone()),
                row,
            )
        })
        .collect();
    let mut items = Vec::new();
    let mut only_a = Vec::new();
    for row in rows_a {
        let key = (row.method.clone(), row.host.clone(), row.path.clone());
        match index_b.remove(&key) {
            Some(other) => {
                let delta_pct = if row.avg_ms > 0.0 {
                    Some((other.avg_ms - row.avg_ms) / row.avg_ms * 100.0)
                } else {
                    None
                };
                items.push(CompareItem {
                    method: row.method,
                    host: row.host,
                    path: row.path,
                    a_count: row.count,
                    b_count: other.count,
                    a_avg_ms: row.avg_ms,
                    b_avg_ms: other.avg_ms,
                    delta_pct,
                    a_error_rate: if row.count > 0 {
                        row.errors as f64 / row.count as f64
                    } else {
                        0.0
                    },
                    b_error_rate: if other.count > 0 {
                        other.errors as f64 / other.count as f64
                    } else {
                        0.0
                    },
                });
            }
            None => only_a.push(CompareOnly {
                method: row.method,
                host: row.host,
                path: row.path,
                count: row.count,
            }),
        }
    }
    let only_b = index_b
        .into_values()
        .map(|row| CompareOnly {
            method: row.method,
            host: row.host,
            path: row.path,
            count: row.count,
        })
        .collect();
    items.sort_by(|left, right| {
        right
            .delta_pct
            .unwrap_or(0.0)
            .abs()
            .partial_cmp(&left.delta_pct.unwrap_or(0.0).abs())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(SessionCompare {
        a: side_a,
        b: side_b,
        items,
        only_a,
        only_b,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::store::FlowBrief;
    use crate::db;
    use crate::state::DbPool;
    use r2d2::Pool;
    use r2d2_sqlite::SqliteConnectionManager;

    fn test_pool(tag: &str) -> (PathBuf, DbPool) {
        let dir = crate::util::test_dir(&format!("capture-persist-{tag}"));
        let manager = SqliteConnectionManager::file(dir.join("astroman.db"))
            .with_init(|conn| conn.execute_batch(&db::connection_pragmas()));
        let pool = Pool::builder().max_size(2).build(manager).unwrap();
        db::init(&pool).unwrap();
        (dir, pool)
    }

    fn flow(id: u64, host: &str, path: &str, body: &str) -> FlowRecord {
        FlowRecord {
            brief: FlowBrief {
                id,
                started_at: 1_700_000_000_000 + id as i64,
                method: "GET".into(),
                scheme: "https".into(),
                host: host.into(),
                path: path.into(),
                http_version: "HTTP/1.1".into(),
                status: Some(200),
                duration_ms: 12,
                request_size: 0,
                response_size: body.len() as u64,
                content_type: "text/plain".into(),
                client_addr: "127.0.0.1:1".into(),
                error: None,
                mapped: false,
                modified: false,
                remote: None,
                passthrough: false,
            },
            request_headers: vec![HeaderEntry {
                name: "Authorization".into(),
                value: "Bearer super-secret".into(),
            }],
            response_headers: Vec::new(),
            request_body: BodyPayload::default(),
            response_body: BodyPayload {
                size: body.len() as u64,
                truncated: false,
                binary: false,
                text: Some(body.to_string()),
                base64: None,
                content_type: "text/plain".into(),
            },
        }
    }

    fn session(pool: &DbPool, name: &str, mode: ArchiveMode) -> i64 {
        let conn = pool.get().unwrap();
        conn.execute(
            "INSERT INTO capture_sessions (name, mode, bind_addr, port, started_at)
             VALUES (?1, ?2, '', 0, 1)",
            params![name, mode.as_str()],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    #[test]
    fn archive_writes_session_and_flows_and_masks_secrets() {
        let (dir, pool) = test_pool("write");
        let id = session(&pool, "s1", ArchiveMode::ArchiveMeta);
        let writer = ArchiveWriter::start(
            pool.clone(),
            dir.clone(),
            id,
            ArchiveMode::ArchiveMeta,
            Vec::new(),
        )
        .unwrap();
        writer.sender().send(&flow(1, "a.test", "/x", "hello"));
        let stats = writer.finish();
        assert_eq!(stats.persisted, 1);
        let conn = pool.get().unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM capture_flows WHERE session_id = ?1",
                params![id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
        let headers: String = conn
            .query_row(
                "SELECT request_headers_json FROM capture_flows WHERE session_id = ?1",
                params![id],
                |row| row.get(0),
            )
            .unwrap();
        assert!(headers.contains(crate::secret::MASK));
        assert!(!headers.contains("super-secret"));
        let bodies: i64 = conn
            .query_row("SELECT COUNT(*) FROM capture_bodies", [], |row| row.get(0))
            .unwrap();
        assert_eq!(bodies, 0, "archive-meta 不存 body");
    }

    #[test]
    fn archive_dedups_identical_bodies() {
        let (dir, pool) = test_pool("dedup");
        let id = session(&pool, "s1", ArchiveMode::ArchiveFull);
        let writer = ArchiveWriter::start(
            pool.clone(),
            dir.clone(),
            id,
            ArchiveMode::ArchiveFull,
            Vec::new(),
        )
        .unwrap();
        for flow_id in 1..=2 {
            writer
                .sender()
                .send(&flow(flow_id, "a.test", "/same", "same-body"));
        }
        writer.finish();
        let conn = pool.get().unwrap();
        let bodies: i64 = conn
            .query_row("SELECT COUNT(*) FROM capture_bodies", [], |row| row.get(0))
            .unwrap();
        assert_eq!(bodies, 1);
        let linked: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM capture_flows WHERE response_body_sha IS NOT NULL",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(linked, 2);
    }

    #[test]
    fn queue_full_drops_without_blocking() {
        let (tx, _rx) = sync_channel(1);
        let dropped = Arc::new(AtomicU64::new(0));
        let sender = ArchiveSender {
            tx,
            dropped: dropped.clone(),
        };
        let record = flow(1, "a.test", "/x", "body");
        sender.send(&record);
        sender.send(&record);
        sender.send(&record);
        assert_eq!(dropped.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn exclusion_skips_host() {
        let record = flow(1, "login.example.com", "/x", "body");
        assert!(excluded(&record, &["example.com".into()]));
        assert!(!excluded(&record, &["other.com".into()]));
    }

    #[test]
    fn retention_deletes_oldest_and_orphan_bodies() {
        let (dir, pool) = test_pool("retention");
        let mut ids = Vec::new();
        for index in 0..(MAX_SESSIONS + 3) {
            let id = session(&pool, &format!("s{index}"), ArchiveMode::ArchiveFull);
            ids.push(id);
            let writer = ArchiveWriter::start(
                pool.clone(),
                dir.clone(),
                id,
                ArchiveMode::ArchiveFull,
                Vec::new(),
            )
            .unwrap();
            writer.sender().send(&flow(
                index as u64 + 1,
                "a.test",
                "/x",
                &format!("body-{index}"),
            ));
            writer.finish();
        }
        cleanup(&pool, &dir).unwrap();
        let conn = pool.get().unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM capture_sessions", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count as usize, MAX_SESSIONS);
        let oldest: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM capture_sessions WHERE id = ?1",
                params![ids[0]],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(oldest, 0);
        let bodies: i64 = conn
            .query_row("SELECT COUNT(*) FROM capture_bodies", [], |row| row.get(0))
            .unwrap();
        assert_eq!(bodies as usize, MAX_SESSIONS);
    }

    #[test]
    fn queries_filters_detail_and_compare() {
        let (dir, pool) = test_pool("queries");
        let first = session(&pool, "first", ArchiveMode::ArchiveFull);
        let writer = ArchiveWriter::start(
            pool.clone(),
            dir.clone(),
            first,
            ArchiveMode::ArchiveFull,
            Vec::new(),
        )
        .unwrap();
        writer.sender().send(&flow(1, "a.test", "/one", "hello"));
        writer.sender().send(&flow(2, "a.test", "/two", "world"));
        writer.finish();

        let sessions = list_sessions(&pool, 10).unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].flow_count, 2);
        assert!(sessions[0].ended_at.is_some());

        let filtered =
            session_flows(&pool, first, Some("one"), Some("GET"), Some("2xx"), 0, 50).unwrap();
        assert_eq!(filtered.total, 1);
        assert_eq!(filtered.items[0].path, "/one");

        let detail = session_flow_detail(&pool, &dir, first, 1).unwrap();
        assert_eq!(detail.response_body.text.as_deref(), Some("hello"));
        assert_eq!(
            detail.request_headers[0].value,
            crate::secret::MASK,
            "归档详情里 Authorization 已脱敏"
        );

        let second = session(&pool, "second", ArchiveMode::ArchiveFull);
        let writer = ArchiveWriter::start(
            pool.clone(),
            dir.clone(),
            second,
            ArchiveMode::ArchiveFull,
            Vec::new(),
        )
        .unwrap();
        writer.sender().send(&flow(1, "a.test", "/one", "hello"));
        writer.sender().send(&flow(2, "a.test", "/three", "new"));
        writer.finish();
        pool.get()
            .unwrap()
            .execute(
                "UPDATE capture_flows SET duration_ms = 24 WHERE session_id = ?1",
                params![second],
            )
            .unwrap();
        let compare = compare_sessions(&pool, first, second).unwrap();
        assert_eq!(compare.items.len(), 1);
        assert_eq!(compare.items[0].path, "/one");
        assert!((compare.items[0].delta_pct.unwrap() - 100.0).abs() < 0.01);
        assert_eq!(compare.only_a.len(), 1);
        assert_eq!(compare.only_b.len(), 1);
    }

    #[test]
    fn large_body_file_is_removed_with_session() {
        let (dir, pool) = test_pool("large-body");
        let id = session(&pool, "large", ArchiveMode::ArchiveFull);
        let writer = ArchiveWriter::start(
            pool.clone(),
            dir.clone(),
            id,
            ArchiveMode::ArchiveFull,
            Vec::new(),
        )
        .unwrap();
        writer
            .sender()
            .send(&flow(1, "a.test", "/big", &"x".repeat(9_000)));
        writer.finish();
        let (sha, file): (String, String) = {
            let conn = pool.get().unwrap();
            conn.query_row(
                "SELECT sha256, file_path FROM capture_bodies WHERE file_path IS NOT NULL",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap()
        };
        let path = body_root(&dir).join(file.replace('/', std::path::MAIN_SEPARATOR_STR));
        assert!(path.exists());
        delete_session(&pool, &dir, id).unwrap();
        assert!(!path.exists(), "删除会话后 body 文件应消失");
        let conn = pool.get().unwrap();
        let rows: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM capture_bodies WHERE sha256 = ?1",
                params![sha],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(rows, 0);
    }

    #[test]
    fn memory_mode_never_touches_tables() {
        let (dir, pool) = test_pool("memory");
        let store = super::super::store::CaptureStore::new();
        store.record(flow(1, "a.test", "/x", "body"));
        assert!(store.dropped() == 0);
        assert!(
            pool.get()
                .unwrap()
                .query_row("SELECT COUNT(*) FROM capture_sessions", [], |row| row
                    .get::<_, i64>(0))
                .unwrap()
                == 0
        );
        assert!(dir.exists());
    }
}
