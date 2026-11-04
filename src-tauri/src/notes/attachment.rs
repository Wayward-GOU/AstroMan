//! 记事本图片附件：按内容去重、关联、取图、另存与孤儿清理。

use base64::Engine;
use rusqlite::{params, OptionalExtension};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::state::DbPool;
use crate::util::{err, now_ms};

/// 单张图片上限 8MB
pub const MAX_BYTES: usize = 8 * 1024 * 1024;
/// 单条待办最多 5 张
pub const MAX_PER_NOTE: i64 = 5;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteFile {
    pub id: i64,
    pub file_name: String,
    pub mime: String,
    pub size: i64,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileData {
    pub mime: String,
    pub base64: String,
}

/// 解码两种入口：文件路径（文件选择器）或 base64（粘贴 / 拖拽）
pub fn decode_input(
    file_name: Option<String>,
    mime: Option<String>,
    base64_data: Option<String>,
    path: Option<String>,
) -> Result<(Vec<u8>, String, String), String> {
    let (bytes, default_name) = if let Some(path) = path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let bytes =
            std::fs::read(path).map_err(|error| format!("读取图片失败（{path}）：{error}"))?;
        let name = std::path::Path::new(path)
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_string();
        (bytes, name)
    } else if let Some(data) = base64_data.as_deref().filter(|value| !value.is_empty()) {
        let payload = data
            .strip_prefix("data:")
            .and_then(|rest| rest.split_once(',').map(|(_, body)| body))
            .unwrap_or(data);
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(payload)
            .map_err(|error| format!("图片 base64 解码失败：{error}"))?;
        (bytes, file_name.clone().unwrap_or_default())
    } else {
        return Err("没有可读取的图片：请粘贴、拖拽或选择文件".into());
    };

    if bytes.len() > MAX_BYTES {
        return Err("图片超过 8MB，请压缩或重新截图后再试".into());
    }
    let sniffed = sniff_mime(&bytes).ok_or_else(|| {
        "不支持的图片格式：只接受 PNG / JPEG / WebP（按文件内容校验，不信任扩展名）".to_string()
    })?;
    // 声明类型与真实类型不一致时以真实类型为准
    let _ = mime;
    Ok((bytes, sniffed.to_string(), default_name))
}

pub fn sniff_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]) {
        return Some("image/png");
    }
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some("image/jpeg");
    }
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Some("image/webp");
    }
    None
}

pub fn add(
    pool: &DbPool,
    note_id: i64,
    file_name: Option<String>,
    mime: Option<String>,
    base64_data: Option<String>,
    path: Option<String>,
) -> Result<NoteFile, String> {
    let (bytes, sniffed, default_name) = decode_input(file_name, mime, base64_data, path)?;
    let conn = pool.get().map_err(err)?;
    let exists: Option<String> = conn
        .query_row(
            "SELECT status FROM notes WHERE id = ?1",
            params![note_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(err)?;
    match exists.as_deref() {
        Some("deleted") | None => return Err("记录不存在或已删除".into()),
        _ => {}
    }
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM note_file_links WHERE note_id = ?1",
            params![note_id],
            |row| row.get(0),
        )
        .map_err(err)?;
    if count >= MAX_PER_NOTE {
        return Err(format!("单条记录最多 {MAX_PER_NOTE} 张图片"));
    }

    let sha = hex_sha256(&bytes);
    let existing: Option<i64> = conn
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
            conn.execute(
                "INSERT INTO note_files (sha256, file_name, mime, size, data, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    sha,
                    default_name,
                    sniffed,
                    bytes.len() as i64,
                    bytes,
                    now_ms()
                ],
            )
            .map_err(err)?;
            conn.last_insert_rowid()
        }
    };

    conn.execute(
        "INSERT OR IGNORE INTO note_file_links (note_id, file_id, created_at)
         VALUES (?1, ?2, ?3)",
        params![note_id, file_id, now_ms()],
    )
    .map_err(err)?;
    file_record(&conn, file_id)
}

pub fn list(pool: &DbPool, note_id: i64) -> Result<Vec<NoteFile>, String> {
    let conn = pool.get().map_err(err)?;
    let mut stmt = conn
        .prepare(
            "SELECT f.id, f.file_name, f.mime, f.size, f.created_at
             FROM note_files f
             JOIN note_file_links l ON l.file_id = f.id
             WHERE l.note_id = ?1
             ORDER BY l.created_at ASC, f.id ASC",
        )
        .map_err(err)?;
    let rows = stmt
        .query_map(params![note_id], |row| {
            Ok(NoteFile {
                id: row.get(0)?,
                file_name: row.get(1)?,
                mime: row.get(2)?,
                size: row.get(3)?,
                created_at: row.get(4)?,
            })
        })
        .map_err(err)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(err)?);
    }
    Ok(out)
}

pub fn data(pool: &DbPool, file_id: i64) -> Result<FileData, String> {
    let conn = pool.get().map_err(err)?;
    let row: Option<(String, Vec<u8>)> = conn
        .query_row(
            "SELECT f.mime, f.data FROM note_files f
             WHERE f.id = ?1
               AND EXISTS (SELECT 1 FROM note_file_links l WHERE l.file_id = f.id)",
            params![file_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(err)?;
    let (mime, bytes) = row.ok_or_else(|| "图片不存在或已被清理".to_string())?;
    Ok(FileData {
        mime,
        base64: base64::engine::general_purpose::STANDARD.encode(bytes),
    })
}

pub fn save(pool: &DbPool, file_id: i64, path: &str) -> Result<(), String> {
    if path.trim().is_empty() {
        return Err("E_ARG: 保存路径不能为空".into());
    }
    let data = data(pool, file_id)?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data.base64)
        .map_err(|error| format!("图片解码失败：{error}"))?;
    std::fs::write(path, bytes).map_err(|error| format!("保存图片失败（{path}）：{error}"))
}

pub fn remove(pool: &DbPool, note_id: i64, file_id: i64) -> Result<(), String> {
    let conn = pool.get().map_err(err)?;
    conn.execute(
        "DELETE FROM note_file_links WHERE note_id = ?1 AND file_id = ?2",
        params![note_id, file_id],
    )
    .map_err(err)?;
    let left: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM note_file_links WHERE file_id = ?1",
            params![file_id],
            |row| row.get(0),
        )
        .map_err(err)?;
    if left == 0 {
        conn.execute("DELETE FROM note_files WHERE id = ?1", params![file_id])
            .map_err(err)?;
    }
    Ok(())
}

/// 清理没有任何关联的图片，返回释放的字节数
pub fn cleanup_orphans(pool: &DbPool) -> Result<i64, String> {
    let conn = pool.get().map_err(err)?;
    let freed: i64 = conn
        .query_row(
            "SELECT COALESCE(SUM(size), 0) FROM note_files
             WHERE id NOT IN (SELECT file_id FROM note_file_links)",
            [],
            |row| row.get(0),
        )
        .map_err(err)?;
    if freed > 0 {
        conn.execute(
            "DELETE FROM note_files WHERE id NOT IN (SELECT file_id FROM note_file_links)",
            [],
        )
        .map_err(err)?;
    }
    Ok(freed)
}

fn file_record(conn: &rusqlite::Connection, file_id: i64) -> Result<NoteFile, String> {
    conn.query_row(
        "SELECT id, file_name, mime, size, created_at FROM note_files WHERE id = ?1",
        params![file_id],
        |row| {
            Ok(NoteFile {
                id: row.get(0)?,
                file_name: row.get(1)?,
                mime: row.get(2)?,
                size: row.get(3)?,
                created_at: row.get(4)?,
            })
        },
    )
    .map_err(err)
}

pub(crate) fn hex_sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sniffs_supported_images() {
        assert_eq!(
            sniff_mime(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0]),
            Some("image/png")
        );
        assert_eq!(sniff_mime(&[0xFF, 0xD8, 0xFF, 0xE0]), Some("image/jpeg"));
        let mut webp = b"RIFFxxxxWEBP".to_vec();
        webp.push(0);
        assert_eq!(sniff_mime(&webp), Some("image/webp"));
        assert_eq!(sniff_mime(b"not an image"), None);
    }
}
