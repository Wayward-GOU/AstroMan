//! G-06 异地备份目标：本地目录 / WebDAV
//!
//! 与普通备份的差别：
//! - 上传前先用 [backup_crypto] 把快照加密成 `.astroman.bak.enc`，**口令不落库**，
//!   由用户每次上传 / 恢复时输入；库被拷走也解不开备份；
//! - WebDAV 密码走 secrets 模块（`AAD_BACKUP`），列表接口只回 `hasPassword`；
//! - 远端保留 7 份或 90 天，超出部分删除，避免把网盘塞满；
//! - 恢复沿用 `backup::mark_restore` + 重启替换，绝不在连接池存活时动主库文件。

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::backup::{self, BackupItem, BackupKind};
use crate::secret;
use crate::services::backup_crypto;
use crate::state::DbPool;
use crate::util::{err, now_ms};

pub const KIND_LOCAL: &str = "local";
pub const KIND_WEBDAV: &str = "webdav";

/// 远端保留口径：最多 7 份，且不超过 90 天
const KEEP_COPIES: usize = 7;
const KEEP_DAYS_MS: i64 = 90 * 24 * 3600 * 1000;
/// WebDAV 上传失败后的重试次数（共 3 次尝试）
const UPLOAD_RETRIES: usize = 2;
const PROPFIND_BODY: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<d:propfind xmlns:d="DAV:"><d:prop><d:getcontentlength/><d:getlastmodified/><d:resourcetype/></d:prop></d:propfind>"#;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupTarget {
    pub id: i64,
    pub name: String,
    pub kind: String,
    pub kind_label: String,
    pub url: String,
    pub username: String,
    pub dir: String,
    pub enabled: bool,
    /// 只告诉前端「有没有存过密码」，绝不返回密文或明文
    pub has_password: bool,
    pub last_run_at: Option<i64>,
    pub last_status: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupTargetInput {
    pub id: Option<i64>,
    pub name: String,
    pub kind: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub username: String,
    /// 空 / 缺省表示保留原密码；只有显式传入新值才重新加密
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default)]
    pub dir: String,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_enabled() -> bool {
    true
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetTestReport {
    pub ok: bool,
    pub detail: String,
    pub checked_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteBackup {
    pub name: String,
    pub size: u64,
    pub modified_at: Option<i64>,
    pub kind: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupUploadResult {
    pub target_id: i64,
    pub target_name: String,
    pub kind: String,
    pub remote_name: String,
    /// 本地加密副本路径（WebDAV 上传后也会保留一份，便于排障）
    pub encrypted_path: String,
    pub encrypted_size: u64,
    pub uploaded_at: i64,
    pub detail: String,
    pub snapshot: BackupItem,
}

struct TargetRow {
    id: i64,
    name: String,
    kind: String,
    url: String,
    username: String,
    password_enc: String,
    dir: String,
    enabled: bool,
    last_run_at: Option<i64>,
    last_status: Option<String>,
    created_at: i64,
}

impl TargetRow {
    fn to_public(&self) -> BackupTarget {
        BackupTarget {
            id: self.id,
            name: self.name.clone(),
            kind: self.kind.clone(),
            kind_label: kind_label(&self.kind).to_string(),
            url: self.url.clone(),
            username: self.username.clone(),
            dir: self.dir.clone(),
            enabled: self.enabled,
            has_password: !self.password_enc.is_empty(),
            last_run_at: self.last_run_at,
            last_status: self.last_status.clone(),
            created_at: self.created_at,
        }
    }
}

fn kind_label(kind: &str) -> &'static str {
    match kind {
        KIND_WEBDAV => "WebDAV",
        _ => "本地目录",
    }
}

fn select_columns() -> &'static str {
    "id, name, kind, url, username, password_enc, dir, enabled, last_run_at, last_status, created_at"
}

fn read_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<TargetRow> {
    Ok(TargetRow {
        id: row.get(0)?,
        name: row.get(1)?,
        kind: row.get(2)?,
        url: row.get(3)?,
        username: row.get(4)?,
        password_enc: row.get(5)?,
        dir: row.get(6)?,
        enabled: row.get::<_, i64>(7)? == 1,
        last_run_at: row.get(8)?,
        last_status: row.get(9)?,
        created_at: row.get(10)?,
    })
}

pub fn list(conn: &Connection) -> Result<Vec<BackupTarget>, String> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {} FROM backup_targets ORDER BY id ASC",
            select_columns()
        ))
        .map_err(err)?;
    let rows = stmt
        .query_map([], read_row)
        .map_err(err)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(err)?;
    Ok(rows.iter().map(TargetRow::to_public).collect())
}

pub fn save(conn: &Connection, input: BackupTargetInput) -> Result<BackupTarget, String> {
    let name = input.name.trim();
    if name.is_empty() {
        return Err("请填写备份目标名称".to_string());
    }
    let kind = match input.kind.trim() {
        KIND_LOCAL => KIND_LOCAL,
        KIND_WEBDAV => KIND_WEBDAV,
        other => return Err(format!("不支持的备份目标类型：{other}")),
    };
    let url = input.url.trim().trim_end_matches('/').to_string();
    let dir = normalize_dir(&input.dir);
    if kind == KIND_LOCAL {
        if dir.is_empty() {
            return Err("本地备份目录不能为空".to_string());
        }
    } else if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err("WebDAV 地址必须以 http:// 或 https:// 开头".to_string());
    }

    let current = input.id.unwrap_or(0);
    let duplicate: Option<i64> = conn
        .query_row(
            "SELECT id FROM backup_targets WHERE name = ?1 AND id <> ?2",
            params![name, current],
            |row| row.get(0),
        )
        .optional()
        .map_err(err)?;
    if duplicate.is_some() {
        return Err("同名备份目标已存在".to_string());
    }

    let password = input.password.as_deref().map(str::trim).unwrap_or("");
    let password_enc = if password.is_empty() {
        None
    } else {
        Some(secret::encrypt(secret::AAD_BACKUP, password)?)
    };

    let id = match input.id {
        Some(id) => {
            let changed = conn
                .execute(
                    "UPDATE backup_targets
                     SET name = ?1, kind = ?2, url = ?3, username = ?4, dir = ?5,
                         enabled = ?6, password_enc = COALESCE(?7, password_enc)
                     WHERE id = ?8",
                    params![
                        name,
                        kind,
                        url,
                        input.username.trim(),
                        dir,
                        input.enabled as i64,
                        password_enc,
                        id
                    ],
                )
                .map_err(err)?;
            if changed == 0 {
                return Err("备份目标不存在，可能已被删除".to_string());
            }
            id
        }
        None => {
            conn.execute(
                "INSERT INTO backup_targets
                    (name, kind, url, username, password_enc, dir, enabled, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    name,
                    kind,
                    url,
                    input.username.trim(),
                    password_enc.unwrap_or_default(),
                    dir,
                    input.enabled as i64,
                    now_ms()
                ],
            )
            .map_err(err)?;
            conn.last_insert_rowid()
        }
    };

    load(conn, id).map(|row| row.to_public())
}

pub fn delete(conn: &Connection, id: i64) -> Result<(), String> {
    let changed = conn
        .execute("DELETE FROM backup_targets WHERE id = ?1", params![id])
        .map_err(err)?;
    if changed == 0 {
        return Err("备份目标不存在，可能已被删除".to_string());
    }
    Ok(())
}

fn load(conn: &Connection, id: i64) -> Result<TargetRow, String> {
    conn.query_row(
        &format!(
            "SELECT {} FROM backup_targets WHERE id = ?1",
            select_columns()
        ),
        params![id],
        read_row,
    )
    .optional()
    .map_err(err)?
    .ok_or_else(|| "备份目标不存在，可能已被删除".to_string())
}

/// 连通性检查：本地目录建目录并写探针；WebDAV 发 PROPFIND
pub fn test(conn: &Connection, id: i64) -> Result<TargetTestReport, String> {
    let target = load(conn, id)?;
    let (ok, detail) = if target.kind == KIND_LOCAL {
        local_probe(&target.dir)
    } else {
        webdav_probe(&target)
    };
    Ok(TargetTestReport {
        ok,
        detail,
        checked_at: now_ms(),
    })
}

fn local_probe(dir: &str) -> (bool, String) {
    let path = PathBuf::from(dir);
    if let Err(error) = std::fs::create_dir_all(&path) {
        return (false, format!("无法创建目录：{error}"));
    }
    let probe = path.join(".astroman-probe");
    match std::fs::File::create(&probe).and_then(|mut file| file.write_all(b"ok")) {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            (true, format!("目录可写：{}", path.display()))
        }
        Err(error) => (false, format!("目录不可写：{error}")),
    }
}

fn webdav_probe(target: &TargetRow) -> (bool, String) {
    let url = join_url(&target.url, &target.dir, "");
    let password = match target_password(target) {
        Ok(value) => value,
        Err(error) => return (false, error),
    };
    let result = tauri::async_runtime::block_on(async move {
        let client = webdav_client()?;
        let response = client
            .request(reqwest::Method::from_bytes(b"PROPFIND").unwrap(), &url)
            .header("Depth", "0")
            .header("Content-Type", "application/xml; charset=utf-8")
            .basic_auth(&target.username, Some(&password))
            .body(PROPFIND_BODY)
            .send()
            .await
            .map_err(|e| format!("连接失败：{e}"))?;
        let status = response.status();
        if status.as_u16() == 207 || status.is_success() {
            Ok(format!("WebDAV 可访问（{status}）"))
        } else if status.as_u16() == 401 || status.as_u16() == 403 {
            Err(format!("认证失败（{status}），请检查用户名 / 密码"))
        } else if status.as_u16() == 404 {
            Err(format!("目录不存在（{status}）：{url}"))
        } else {
            Err(format!("WebDAV 返回 {status}"))
        }
    });
    match result {
        Ok(detail) => (true, detail),
        Err(error) => (false, error),
    }
}

/// 上传一份加密快照：建快照 → 加密 → 推送 → 远端保留策略 → 记录状态
pub fn upload(
    pool: &DbPool,
    data_dir: &Path,
    target_id: i64,
    passphrase: &str,
) -> Result<BackupUploadResult, String> {
    let target = {
        let conn = pool.get().map_err(err)?;
        load(&conn, target_id)?
    };
    if !target.enabled {
        return Err("备份目标已停用，请先在设置中启用".to_string());
    }

    let snapshot = backup::create(pool, data_dir, BackupKind::Manual)?;
    let encrypted_dir = backup::backup_dir(data_dir).join("encrypted");
    std::fs::create_dir_all(&encrypted_dir).map_err(|e| format!("创建加密备份目录失败：{e}"))?;

    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S-%3f");
    let remote_name = format!(
        "{}-{}{}",
        slug(&target.name),
        stamp,
        backup_crypto::EXTENSION
    );
    let encrypted_path = encrypted_dir.join(&remote_name);
    backup_crypto::encrypt_file(Path::new(&snapshot.path), &encrypted_path, passphrase)?;
    let encrypted_size = std::fs::metadata(&encrypted_path)
        .map(|meta| meta.len())
        .unwrap_or(0);

    let pushed = match target.kind.as_str() {
        KIND_LOCAL => local_push(&target, &encrypted_path, &remote_name),
        _ => webdav_push(&target, &encrypted_path, &remote_name),
    };

    // 本地加密副本也按 7 份保留，避免上传目录无限增长
    prune_local(&encrypted_dir);

    let (status, detail) = match &pushed {
        Ok(detail) => ("ok".to_string(), detail.clone()),
        Err(error) => ("fail".to_string(), error.clone()),
    };
    {
        let conn = pool.get().map_err(err)?;
        conn.execute(
            "UPDATE backup_targets SET last_run_at = ?1, last_status = ?2 WHERE id = ?3",
            params![
                now_ms(),
                truncate(&format!("{status}: {detail}"), 300),
                target.id
            ],
        )
        .map_err(err)?;
        let _ = crate::services::audit::write(
            &conn,
            "backup.upload",
            Some(&target.name),
            if pushed.is_ok() { "ok" } else { "fail" },
            Some(&json!({
                "remoteName": remote_name,
                "encryptedSize": encrypted_size,
                "detail": detail,
            })),
            None,
        );
    }

    pushed?;
    Ok(BackupUploadResult {
        target_id: target.id,
        target_name: target.name.clone(),
        kind: target.kind.clone(),
        remote_name,
        encrypted_path: encrypted_path.to_string_lossy().to_string(),
        encrypted_size,
        uploaded_at: now_ms(),
        detail,
        snapshot,
    })
}

fn local_push(target: &TargetRow, source: &Path, name: &str) -> Result<String, String> {
    let dir = PathBuf::from(&target.dir);
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建备份目录失败：{e}"))?;
    let target_path = dir.join(name);
    std::fs::copy(source, &target_path).map_err(|e| format!("写入备份目录失败：{e}"))?;
    prune_local(&dir);
    Ok(format!("已写入 {}", target_path.display()))
}

fn webdav_push(target: &TargetRow, source: &Path, name: &str) -> Result<String, String> {
    let password = target_password(target)?;
    let url = join_url(&target.url, &target.dir, name);
    let mut last_error = String::new();
    for attempt in 0..=UPLOAD_RETRIES {
        match put_file(target, &password, &url, source) {
            Ok(()) => {
                // 保留策略失败不影响「上传成功」，只记日志
                if let Err(error) =
                    webdav_prune(&target.url, &target.dir, &target.username, &password)
                {
                    log::warn!("清理远端旧备份失败：{error}");
                }
                return Ok(format!("已上传 {name}"));
            }
            Err(error) => {
                last_error = error;
                if attempt < UPLOAD_RETRIES {
                    std::thread::sleep(Duration::from_millis(800 * (attempt as u64 + 1)));
                }
            }
        }
    }
    Err(last_error)
}

fn put_file(target: &TargetRow, password: &str, url: &str, source: &Path) -> Result<(), String> {
    let username = target.username.clone();
    let password = password.to_string();
    let url = url.to_string();
    let source = source.to_path_buf();
    tauri::async_runtime::block_on(async move {
        let client = webdav_client()?;
        // tokio 文件句柄可直接流式作为请求体，避免整包读进内存
        let file = tokio::fs::File::open(&source)
            .await
            .map_err(|e| format!("读取加密备份失败：{e}"))?;
        let response = client
            .put(&url)
            .basic_auth(&username, Some(&password))
            .header("Content-Type", "application/octet-stream")
            .body(reqwest::Body::from(file))
            .send()
            .await
            .map_err(|e| format!("上传失败：{e}"))?;
        let status = response.status();
        if status.is_success() {
            Ok(())
        } else if status.as_u16() == 401 || status.as_u16() == 403 {
            Err(format!("WebDAV 认证失败（{status}）"))
        } else {
            Err(format!("上传失败（{status}）"))
        }
    })
}

pub fn remote_list(conn: &Connection, target_id: i64) -> Result<Vec<RemoteBackup>, String> {
    let target = load(conn, target_id)?;
    let mut items = if target.kind == KIND_LOCAL {
        local_entries(&target.dir)?
    } else {
        let password = target_password(&target)?;
        webdav_entries(&target, &password)?
    };
    items.sort_by(|a, b| {
        b.modified_at
            .unwrap_or(0)
            .cmp(&a.modified_at.unwrap_or(0))
            .then_with(|| b.name.cmp(&a.name))
    });
    Ok(items)
}

fn local_entries(dir: &str) -> Result<Vec<RemoteBackup>, String> {
    let path = PathBuf::from(dir);
    let entries = match std::fs::read_dir(&path) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("读取备份目录失败：{error}")),
    };
    let mut items = Vec::new();
    for entry in entries.flatten() {
        let file_path = entry.path();
        if !is_package(&file_path) {
            continue;
        }
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        items.push(RemoteBackup {
            name: file_path
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_default(),
            size: meta.len(),
            modified_at: meta
                .modified()
                .ok()
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as i64),
            kind: KIND_LOCAL.to_string(),
        });
    }
    Ok(items)
}

fn webdav_entries(target: &TargetRow, password: &str) -> Result<Vec<RemoteBackup>, String> {
    let url = join_url(&target.url, &target.dir, "");
    let username = target.username.clone();
    let password = password.to_string();
    let xml = tauri::async_runtime::block_on(async move {
        let client = webdav_client()?;
        let response = client
            .request(reqwest::Method::from_bytes(b"PROPFIND").unwrap(), &url)
            .header("Depth", "1")
            .header("Content-Type", "application/xml; charset=utf-8")
            .basic_auth(&username, Some(&password))
            .body(PROPFIND_BODY)
            .send()
            .await
            .map_err(|e| format!("读取远端目录失败：{e}"))?;
        let status = response.status();
        if !(status.as_u16() == 207 || status.is_success()) {
            return Err(format!("读取远端目录失败（{status}）"));
        }
        response
            .text()
            .await
            .map_err(|e| format!("读取远端目录响应失败：{e}"))
    })?;

    let mut items = Vec::new();
    for block in response_blocks(&xml) {
        let Some(href) = href_of(block) else {
            continue;
        };
        let href = href.trim();
        if href.ends_with('/') {
            continue;
        }
        let name = percent_decode(href.rsplit('/').next().unwrap_or_default());
        if !name.ends_with(backup_crypto::EXTENSION) {
            continue;
        }
        let size = length_of(block)
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(0);
        let modified_at = modified_of(block)
            .and_then(|value| chrono::DateTime::parse_from_rfc2822(value.trim()).ok())
            .map(|time| time.timestamp_millis());
        items.push(RemoteBackup {
            name,
            size,
            modified_at,
            kind: KIND_WEBDAV.to_string(),
        });
    }
    Ok(items)
}

/// 保留策略：只留最新 7 份、且不超过 90 天
fn prune_local(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<(PathBuf, i64)> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| is_package(path))
        .map(|path| {
            let modified = std::fs::metadata(&path)
                .ok()
                .and_then(|meta| meta.modified().ok())
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);
            (path, modified)
        })
        .collect();
    files.sort_by_key(|(_, modified)| std::cmp::Reverse(*modified));
    let now = now_ms();
    for (rank, (path, modified)) in files.into_iter().enumerate() {
        let expired = now - modified > KEEP_DAYS_MS;
        if rank >= KEEP_COPIES || expired {
            let _ = std::fs::remove_file(path);
        }
    }
}

fn webdav_prune(base: &str, dir: &str, username: &str, password: &str) -> Result<(), String> {
    let url = join_url(base, dir, "");
    let request_user = username.to_string();
    let request_password = password.to_string();
    let xml = tauri::async_runtime::block_on(async move {
        let client = webdav_client()?;
        let response = client
            .request(reqwest::Method::from_bytes(b"PROPFIND").unwrap(), &url)
            .header("Depth", "1")
            .header("Content-Type", "application/xml; charset=utf-8")
            .basic_auth(&request_user, Some(&request_password))
            .body(PROPFIND_BODY)
            .send()
            .await
            .map_err(|e| format!("读取远端目录失败：{e}"))?;
        if !response.status().is_success() && response.status().as_u16() != 207 {
            return Err(format!("读取远端目录失败（{}）", response.status()));
        }
        response
            .text()
            .await
            .map_err(|e| format!("读取远端目录响应失败：{e}"))
    })?;

    let mut files: Vec<(String, i64)> = Vec::new();
    for block in response_blocks(&xml) {
        let Some(href) = href_of(block) else {
            continue;
        };
        let href = href.trim();
        if href.ends_with('/') {
            continue;
        }
        let name = percent_decode(href.rsplit('/').next().unwrap_or_default());
        if !name.ends_with(backup_crypto::EXTENSION) {
            continue;
        }
        let modified = modified_of(block)
            .and_then(|value| chrono::DateTime::parse_from_rfc2822(value.trim()).ok())
            .map(|time| time.timestamp_millis())
            .unwrap_or(0);
        files.push((name, modified));
    }
    files.sort_by_key(|(_, modified)| std::cmp::Reverse(*modified));
    let now = now_ms();
    let username = username.to_string();
    let password = password.to_string();
    for (rank, (name, modified)) in files.into_iter().enumerate() {
        if rank < KEEP_COPIES && now - modified <= KEEP_DAYS_MS {
            continue;
        }
        let delete_url = join_url(base, dir, &name);
        let user = username.clone();
        let pass = password.clone();
        let _ = tauri::async_runtime::block_on(async {
            let client = webdav_client()?;
            let _ = client
                .delete(&delete_url)
                .basic_auth(&user, Some(&pass))
                .send()
                .await;
            Ok::<(), String>(())
        });
    }
    Ok(())
}

/// 从远端恢复：下载 / 拷贝 → 解密 → 校验 → 写待恢复标记（重启后替换）
pub fn restore_from_target(
    pool: &DbPool,
    data_dir: &Path,
    config_file: &Path,
    target_id: i64,
    remote_name: &str,
    passphrase: &str,
) -> Result<BackupItem, String> {
    let name = safe_remote_name(remote_name)?;
    let target = {
        let conn = pool.get().map_err(err)?;
        load(&conn, target_id)?
    };

    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S-%3f");
    let download_dir = backup::backup_dir(data_dir).join("downloads");
    std::fs::create_dir_all(&download_dir).map_err(|e| format!("创建恢复目录失败：{e}"))?;
    let downloaded = download_dir.join(format!("restore-{stamp}{}", backup_crypto::EXTENSION));

    if target.kind == KIND_LOCAL {
        let source = PathBuf::from(&target.dir).join(&name);
        if !source.exists() {
            return Err("远端备份不存在，可能已被清理".to_string());
        }
        std::fs::copy(&source, &downloaded).map_err(|e| format!("读取远端备份失败：{e}"))?;
    } else {
        let password = target_password(&target)?;
        let url = join_url(&target.url, &target.dir, &name);
        if let Err(error) = download_file(&target.username, &password, &url, &downloaded) {
            let _ = std::fs::remove_file(&downloaded);
            return Err(error);
        }
    }

    let restored = backup::backup_dir(data_dir).join(format!("manual-restore-{stamp}.db"));
    if !backup_crypto::is_encrypted(&downloaded) {
        let _ = std::fs::remove_file(&downloaded);
        return Err("E_CRYPTO: 远端文件不是 AstroMan 加密备份".to_string());
    }
    if let Err(error) = backup_crypto::decrypt_file(&downloaded, &restored, passphrase) {
        let _ = std::fs::remove_file(&downloaded);
        return Err(error);
    }
    let _ = std::fs::remove_file(&downloaded);

    let item = match backup::validate(&restored) {
        Ok(item) => item,
        Err(error) => {
            let _ = std::fs::remove_file(&restored);
            return Err(error);
        }
    };
    if let Some(parent) = config_file.parent() {
        // 配置目录通常已在启动时建好；这里兜底，避免首次恢复时标记写不进去
        std::fs::create_dir_all(parent).map_err(|e| format!("创建配置目录失败：{e}"))?;
    }
    backup::mark_restore(config_file, Path::new(&item.path))?;

    if let Ok(conn) = pool.get() {
        let _ = crate::services::audit::write(
            &conn,
            "backup.restore",
            Some(&target.name),
            "ok",
            Some(&json!({ "remoteName": name, "schemaVersion": item.schema_version })),
            None,
        );
    }
    log::info!("已排队从异地备份恢复：{} / {name}", target.name);
    Ok(item)
}

fn download_file(username: &str, password: &str, url: &str, target: &Path) -> Result<(), String> {
    let username = username.to_string();
    let password = password.to_string();
    let url = url.to_string();
    let target = target.to_path_buf();
    tauri::async_runtime::block_on(async move {
        let client = webdav_client()?;
        let response = client
            .get(&url)
            .basic_auth(&username, Some(&password))
            .send()
            .await
            .map_err(|e| format!("下载备份失败：{e}"))?;
        let status = response.status();
        if status.as_u16() == 404 {
            return Err("远端备份不存在，可能已被清理".to_string());
        }
        if !status.is_success() {
            return Err(format!("下载备份失败（{status}）"));
        }
        let part = target.with_file_name(format!(
            "{}.part",
            target
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_else(|| "download".to_string())
        ));
        let mut file = tokio::fs::File::create(&part)
            .await
            .map_err(|e| format!("创建临时文件失败：{e}"))?;
        use futures_util::StreamExt;
        use tokio::io::AsyncWriteExt;
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| format!("下载中断：{e}"))?;
            if let Err(error) = file.write_all(&chunk).await {
                let _ = std::fs::remove_file(&part);
                return Err(format!("写入备份失败：{error}"));
            }
        }
        file.flush()
            .await
            .map_err(|e| format!("写入备份失败：{e}"))?;
        drop(file);
        std::fs::rename(&part, &target).map_err(|e| format!("保存备份失败：{e}"))?;
        Ok(())
    })
}

fn webdav_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|e| format!("创建 WebDAV 客户端失败：{e}"))
}

fn target_password(target: &TargetRow) -> Result<String, String> {
    if target.password_enc.trim().is_empty() {
        return Ok(String::new());
    }
    let password = secret::decrypt(secret::AAD_BACKUP, &target.password_enc);
    if password.is_empty() {
        return Err("无法解密 WebDAV 密码，请到设置中重新填写".to_string());
    }
    Ok(password)
}

fn normalize_dir(dir: &str) -> String {
    dir.trim().replace('\\', "/").trim_matches('/').to_string()
}

fn join_url(base: &str, dir: &str, name: &str) -> String {
    let mut url = base.trim().trim_end_matches('/').to_string();
    let dir = normalize_dir(dir);
    if !dir.is_empty() {
        url.push('/');
        url.push_str(&dir);
    }
    if !name.is_empty() {
        url.push('/');
        url.push_str(name);
    }
    url
}

/// 远端文件名只允许最后一段，防止 `../` 跳出备份目录
fn safe_remote_name(name: &str) -> Result<String, String> {
    let trimmed = name.trim();
    if trimmed.is_empty()
        || trimmed.contains("..")
        || trimmed.contains('/')
        || trimmed.contains('\\')
        || !trimmed.ends_with(backup_crypto::EXTENSION)
    {
        return Err("备份文件名不合法".to_string());
    }
    Ok(trimmed.to_string())
}

fn slug(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let trimmed = cleaned.trim_matches('-');
    if trimmed.is_empty() {
        "astroman".to_string()
    } else {
        trimmed.chars().take(40).collect()
    }
}

fn is_package(path: &Path) -> bool {
    path.file_name()
        .map(|name| name.to_string_lossy().ends_with(backup_crypto::EXTENSION))
        .unwrap_or(false)
}

fn truncate(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let shortened: String = text.chars().take(limit).collect();
    format!("{shortened}…")
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).unwrap_or("");
            if let Ok(byte) = u8::from_str_radix(hex, 16) {
                out.push(byte);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn response_blocks(xml: &str) -> Vec<&str> {
    block_regex()
        .captures_iter(xml)
        .filter_map(|captures| captures.get(1).map(|matched| matched.as_str()))
        .collect()
}

const HREF_PATTERN: &str =
    r"(?i)<(?:[a-z0-9.-]+:)?href[^>]*>\s*([^<]+?)\s*</(?:[a-z0-9.-]+:)?href>";
const LENGTH_PATTERN: &str = r"(?i)<(?:[a-z0-9.-]+:)?getcontentlength[^>]*>\s*(\d+)\s*</";
const MODIFIED_PATTERN: &str = r"(?i)<(?:[a-z0-9.-]+:)?getlastmodified[^>]*>\s*([^<]+?)\s*</";

fn block_regex() -> &'static regex::Regex {
    static BLOCK: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    BLOCK.get_or_init(|| {
        regex::Regex::new(
            r"(?is)<(?:[a-z0-9.-]+:)?response\b[^>]*>(.*?)</(?:[a-z0-9.-]+:)?response>",
        )
        .expect("response block regex")
    })
}

fn capture_with(
    slot: &'static std::sync::OnceLock<regex::Regex>,
    pattern: &'static str,
    text: &str,
) -> Option<String> {
    let regex = slot.get_or_init(|| regex::Regex::new(pattern).expect("propfind regex"));
    regex
        .captures(text)
        .and_then(|captures| captures.get(1))
        .map(|matched| matched.as_str().to_string())
}

fn href_of(text: &str) -> Option<String> {
    static HREF: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    capture_with(&HREF, HREF_PATTERN, text)
}

fn length_of(text: &str) -> Option<String> {
    static LENGTH: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    capture_with(&LENGTH, LENGTH_PATTERN, text)
}

fn modified_of(text: &str) -> Option<String> {
    static MODIFIED: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    capture_with(&MODIFIED, MODIFIED_PATTERN, text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use r2d2_sqlite::SqliteConnectionManager;

    fn temp_state(tag: &str) -> (PathBuf, DbPool, PathBuf) {
        let dir = crate::util::test_dir(&format!("backup-target-{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let manager = SqliteConnectionManager::file(dir.join(crate::state::DB_FILE));
        let pool = r2d2::Pool::builder().max_size(2).build(manager).unwrap();
        let conn = pool.get().unwrap();
        conn.execute_batch(
            "CREATE TABLE demo (id INTEGER PRIMARY KEY, name TEXT);
             INSERT INTO demo (name) VALUES ('a'), ('b');
             PRAGMA user_version = 2;",
        )
        .unwrap();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS backup_targets (
                id            INTEGER PRIMARY KEY AUTOINCREMENT,
                name          TEXT    NOT NULL,
                kind          TEXT    NOT NULL,
                url           TEXT    NOT NULL DEFAULT '',
                username      TEXT    NOT NULL DEFAULT '',
                password_enc  TEXT    NOT NULL DEFAULT '',
                dir           TEXT    NOT NULL DEFAULT '',
                enabled       INTEGER NOT NULL DEFAULT 1,
                last_run_at   INTEGER,
                last_status   TEXT,
                created_at    INTEGER NOT NULL
            );
            CREATE UNIQUE INDEX IF NOT EXISTS idx_backup_target_name
                ON backup_targets(name);
            CREATE TABLE IF NOT EXISTS audit_logs (
                id         INTEGER PRIMARY KEY AUTOINCREMENT,
                created_at INTEGER NOT NULL,
                action     TEXT NOT NULL,
                target     TEXT,
                result     TEXT NOT NULL DEFAULT 'ok',
                detail     TEXT,
                trace_id   TEXT,
                actor      TEXT NOT NULL DEFAULT 'local'
            );
            CREATE TABLE IF NOT EXISTS app_meta (
                key TEXT PRIMARY KEY, value TEXT NOT NULL DEFAULT ''
            );",
        )
        .unwrap();
        let config = dir.join("config").join("storage.json");
        (dir, pool, config)
    }

    fn save_local(conn: &Connection, dir: &Path, name: &str) -> BackupTarget {
        save(
            conn,
            BackupTargetInput {
                id: None,
                name: name.to_string(),
                kind: KIND_LOCAL.to_string(),
                url: String::new(),
                username: String::new(),
                password: None,
                dir: dir.to_string_lossy().to_string(),
                enabled: true,
            },
        )
        .unwrap()
    }

    #[test]
    fn save_list_delete_and_duplicate_names() {
        let (dir, pool, _) = temp_state("crud");
        let conn = pool.get().unwrap();
        let remote = dir.join("remote");
        let saved = save_local(&conn, &remote, "我的网盘");
        assert_eq!(saved.name, "我的网盘");
        assert!(!saved.has_password);

        let duplicate = save(
            &conn,
            BackupTargetInput {
                id: None,
                name: "我的网盘".to_string(),
                kind: KIND_LOCAL.to_string(),
                url: String::new(),
                username: String::new(),
                password: None,
                dir: remote.to_string_lossy().to_string(),
                enabled: true,
            },
        )
        .unwrap_err();
        assert!(duplicate.contains("同名"));

        assert_eq!(list(&conn).unwrap().len(), 1);
        delete(&conn, saved.id).unwrap();
        assert!(list(&conn).unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn webdav_requires_http_url() {
        let (dir, pool, _) = temp_state("webdav-url");
        let conn = pool.get().unwrap();
        let error = save(
            &conn,
            BackupTargetInput {
                id: None,
                name: "nas".to_string(),
                kind: KIND_WEBDAV.to_string(),
                url: "nas.local/dav".to_string(),
                username: "user".to_string(),
                password: None,
                dir: String::new(),
                enabled: true,
            },
        )
        .unwrap_err();
        assert!(error.contains("http"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn local_upload_writes_encrypted_package_and_restore_marks_pending() {
        let (dir, pool, config) = temp_state("upload");
        let remote = dir.join("remote");
        let target = {
            let conn = pool.get().unwrap();
            save_local(&conn, &remote, "本地目录")
        };
        let passphrase = "correct horse battery";

        let result = upload(&pool, &dir, target.id, passphrase).unwrap();
        assert!(result.remote_name.ends_with(backup_crypto::EXTENSION));
        assert!(result.encrypted_size > 0);
        let remote_file = remote.join(&result.remote_name);
        assert!(remote_file.exists(), "应推送到目标目录");
        assert!(backup_crypto::is_encrypted(&remote_file));

        let conn = pool.get().unwrap();
        let listed = remote_list(&conn, target.id).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name, result.remote_name);
        let saved = list(&conn).unwrap().remove(0);
        assert_eq!(
            saved.last_status.as_deref().map(|v| v.starts_with("ok")),
            Some(true)
        );

        // 口令错误：不解密、不写标记
        let error = restore_from_target(
            &pool,
            &dir,
            &config,
            target.id,
            &result.remote_name,
            "wrong passphrase",
        )
        .unwrap_err();
        assert!(error.starts_with("E_CRYPTO"), "实际：{error}");
        assert!(!config.parent().unwrap().join("restore.pending").exists());

        let item = restore_from_target(
            &pool,
            &dir,
            &config,
            target.id,
            &result.remote_name,
            passphrase,
        )
        .unwrap();
        assert!(Path::new(&item.path).exists());
        assert!(config.parent().unwrap().join("restore.pending").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn remote_name_cannot_escape_directory() {
        assert!(safe_remote_name("../evil.astroman.bak.enc").is_err());
        assert!(safe_remote_name("sub/dir.astroman.bak.enc").is_err());
        assert!(safe_remote_name("ok.astroman.bak.enc").is_ok());
        assert!(safe_remote_name("ok.db").is_err());
    }

    #[test]
    fn propfind_entries_are_parsed() {
        let xml = r#"<?xml version="1.0"?>
<d:multistatus xmlns:d="DAV:">
  <d:response>
    <d:href>/dav/backups/</d:href>
    <d:propstat><d:prop><d:resourcetype><d:collection/></d:resourcetype></d:prop></d:propstat>
  </d:response>
  <d:response>
    <d:href>/dav/backups/nas-2026.astroman.bak.enc</d:href>
    <d:propstat><d:prop>
      <d:getcontentlength>1024</d:getcontentlength>
      <d:getlastmodified>Fri, 08 Nov 2026 10:00:00 GMT</d:getlastmodified>
    </d:prop></d:propstat>
  </d:response>
</d:multistatus>"#;
        let blocks = response_blocks(xml);
        assert_eq!(blocks.len(), 2);
        assert_eq!(
            href_of(blocks[1]).as_deref(),
            Some("/dav/backups/nas-2026.astroman.bak.enc")
        );
        assert_eq!(length_of(blocks[1]).as_deref(), Some("1024"));
        assert!(modified_of(blocks[1]).is_some());
    }
}
