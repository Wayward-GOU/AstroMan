//! 日志落盘与滚动（G-15）
//!
//! 三件事：
//! 1. 写到 `数据目录/logs/astroman-YYYYMMDD.log`，按天滚动、单文件 5MB 上限、保留 7 天；
//! 2. 写盘前统一脱敏：运行期收集到的敏感值（环境变量里的 secret、AI Key）一律替换；
//! 3. stdout 仍然保留一份，`tauri dev` 下照旧能看到。
//!
//! 脱敏为什么放在写入口而不是调用点：调用点太多（请求、报告、AI、诊断包），
//! 只要漏一处就会把 token 写进长期保存的文件里；写入口是唯一收口。

use std::collections::HashSet;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use parking_lot::RwLock;

/// 单文件上限
const MAX_FILE_BYTES: u64 = 5 * 1024 * 1024;
/// 保留天数
const KEEP_DAYS: usize = 7;

static SECRETS: RwLock<Option<HashSet<String>>> = RwLock::new(None);

/// 登记一个需要脱敏的敏感值（长度 < 4 的不登记，避免把正常文本打码成筛子）
pub fn register_secret(value: &str) {
    let trimmed = value.trim();
    if trimmed.chars().count() < 4 {
        return;
    }
    let mut guard = SECRETS.write();
    let set = guard.get_or_insert_with(HashSet::new);
    // 上限保护：极端情况下不让集合无限增长（长跑压测会反复注册同一个值）
    if set.len() < 512 {
        set.insert(trimmed.to_string());
    }
}

pub fn clear_secrets() {
    if let Some(set) = SECRETS.write().as_mut() {
        set.clear();
    }
}

/// 把文本里出现的敏感值替换为掩码
pub fn redact(text: &str) -> String {
    let guard = SECRETS.read();
    let Some(set) = guard.as_ref() else {
        return text.to_string();
    };
    if set.is_empty() || text.is_empty() {
        return text.to_string();
    }
    let mut out = text.to_string();
    for secret in set.iter() {
        if out.contains(secret.as_str()) {
            out = out.replace(secret.as_str(), "••••••");
        }
    }
    out
}

/// 按天 + 按大小滚动的写入器
pub struct RollingWriter {
    dir: PathBuf,
    date: String,
    written: u64,
    file: Option<std::fs::File>,
}

impl RollingWriter {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            date: String::new(),
            written: 0,
            file: None,
        }
    }

    fn ensure(&mut self) -> io::Result<()> {
        let today = chrono::Local::now().format("%Y%m%d").to_string();
        let need_new = self.file.is_none() || self.date != today || self.written >= MAX_FILE_BYTES;
        if !need_new {
            return Ok(());
        }
        std::fs::create_dir_all(&self.dir)?;
        self.date = today.clone();
        let path = self.dir.join(format!("astroman-{today}.log"));
        // 单文件超限：追加序号，不覆盖当天已有内容
        let path = if self.written >= MAX_FILE_BYTES && path.exists() {
            let stamp = chrono::Local::now().format("%H%M%S").to_string();
            self.dir.join(format!("astroman-{today}-{stamp}.log"))
        } else {
            path
        };
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        self.written = file.metadata().map(|m| m.len()).unwrap_or(0);
        self.file = Some(file);
        prune(&self.dir, KEEP_DAYS);
        Ok(())
    }
}

impl Write for RollingWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if let Err(error) = self.ensure() {
            // 日志失败不能影响业务：退化为只写 stdout
            eprintln!("日志文件不可用（{error}），本条仅输出到控制台");
            let _ = io::stdout().write_all(buf);
            return Ok(buf.len());
        }
        let text = String::from_utf8_lossy(buf).into_owned();
        let redacted = redact(&text);
        let bytes = redacted.as_bytes();
        if let Some(file) = self.file.as_mut() {
            file.write_all(bytes)?;
            self.written += bytes.len() as u64;
        }
        let _ = io::stdout().write_all(bytes);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if let Some(file) = self.file.as_mut() {
            file.flush()?;
        }
        io::stdout().flush()
    }
}

/// 只保留最近 N 天的日志文件
fn prune(dir: &Path, keep: usize) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<(std::time::SystemTime, PathBuf)> = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if path.extension().and_then(|v| v.to_str()) != Some("log") {
                return None;
            }
            let modified = entry.metadata().ok()?.modified().ok()?;
            Some((modified, path))
        })
        .collect();
    if files.len() <= keep {
        return;
    }
    files.sort_by_key(|(time, _)| *time);
    for (_, path) in files.iter().take(files.len() - keep) {
        let _ = std::fs::remove_file(path);
    }
}

/// 给诊断包用：最近 N 行日志
pub fn tail(dir: &Path, lines: usize) -> String {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return String::new();
    };
    let mut files: Vec<(std::time::SystemTime, PathBuf)> = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if path.extension().and_then(|v| v.to_str()) != Some("log") {
                return None;
            }
            let modified = entry.metadata().ok()?.modified().ok()?;
            Some((modified, path))
        })
        .collect();
    files.sort_by_key(|(time, _)| *time);
    let mut buffer = String::new();
    for (_, path) in files.iter().rev() {
        if let Ok(text) = std::fs::read_to_string(path) {
            buffer.push_str(&text);
        }
        if buffer.lines().count() >= lines {
            break;
        }
    }
    let all: Vec<&str> = buffer.lines().collect();
    let start = all.len().saturating_sub(lines);
    all[start..].join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_registered_secrets() {
        clear_secrets();
        register_secret("sk-super-secret-token");
        let text = redact("Authorization: Bearer sk-super-secret-token 结束");
        assert!(!text.contains("sk-super-secret-token"));
        assert!(text.contains("••••••"));
        clear_secrets();
    }

    #[test]
    fn short_values_are_not_registered() {
        clear_secrets();
        register_secret("abc");
        assert_eq!(redact("abc"), "abc");
        clear_secrets();
    }
}
