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

/// 把文本里出现的敏感值替换为掩码。
///
/// 除了运行期登记的密钥，还要按「字段名」兜底：Authorization / Cookie /
/// token / api_key / password 这类内容经常从请求头、URL、错误信息里顺带进日志，
/// 不可能每个调用点都记得登记。
pub fn redact(text: &str) -> String {
    if text.is_empty() {
        return String::new();
    }
    let mut out = text.to_string();
    {
        let guard = SECRETS.read();
        if let Some(set) = guard.as_ref() {
            for secret in set.iter() {
                if out.contains(secret.as_str()) {
                    out = out.replace(secret.as_str(), "••••••");
                }
            }
        }
    }
    apply_patterns(&out)
}

fn apply_patterns(text: &str) -> String {
    use std::sync::OnceLock;
    static PATTERNS: OnceLock<Vec<(regex::Regex, &'static str)>> = OnceLock::new();
    let patterns = PATTERNS.get_or_init(|| {
        vec![
            (
                regex::Regex::new(
                    r#"(?i)(authorization\s*[:=]\s*)(?:bearer\s+)?[^\s",}\]]+"#,
                )
                .unwrap(),
                "${1}••••••",
            ),
            (
                regex::Regex::new(r#"(?i)(cookie\s*[:=]\s*)[^\r\n"]+"#).unwrap(),
                "${1}••••••",
            ),
            (
                regex::Regex::new(
                    r#"(?i)((?:access[_-]?token|refresh[_-]?token|api[_-]?key|apikey|secret|password|passwd|pwd|token)\s*["']?\s*[:=]\s*["']?)([^"'\s,}&\]}]+)"#,
                )
                .unwrap(),
                "${1}••••••",
            ),
            (
                regex::Regex::new(r"(?i)(https?://[^:/\s@]+:)([^/\s]+)(@)").unwrap(),
                "${1}••••••${3}",
            ),
        ]
    });
    let mut out = text.to_string();
    for (pattern, replacement) in patterns {
        out = pattern.replace_all(&out, *replacement).into_owned();
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

/// 结构化日志行：一行一个 JSON，方便 grep 与诊断包解析。
///
/// 脱敏在序列化之前做，避免密钥里带引号时转义后匹配不到。
pub fn format_json(buf: &mut dyn Write, record: &log::Record<'_>) -> io::Result<()> {
    let entry = serde_json::json!({
        "ts": chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        "level": record.level().as_str().to_ascii_lowercase(),
        "target": record.target(),
        "msg": redact(&record.args().to_string()),
    });
    writeln!(buf, "{entry}")
}

/// 记录 panic 到独立文件并保留默认 stderr 输出。
///
/// 不经过 log 宏：panic 可能发生在持有日志锁的时刻，再走一遍日志会死锁。
pub fn install_panic_hook(data_dir: &Path) {
    let dir = data_dir.join("logs");
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let payload = info
            .payload()
            .downcast_ref::<&str>()
            .map(|value| value.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "未知 panic".to_string());
        let location = info
            .location()
            .map(|value| format!("{}:{}:{}", value.file(), value.line(), value.column()))
            .unwrap_or_else(|| "unknown".to_string());
        let thread = std::thread::current();
        let line = panic_line(thread.name().unwrap_or("unnamed"), &location, &payload);
        if let Err(error) = append_panic(&dir, &line) {
            eprintln!("panic 日志写入失败（{error}）");
        }
        // 保留默认行为：dev 下仍能在控制台看到完整堆栈
        previous(info);
    }));
}

fn panic_line(thread: &str, location: &str, payload: &str) -> String {
    let entry = serde_json::json!({
        "ts": chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        "level": "panic",
        "thread": thread,
        "location": location,
        "msg": redact(payload),
    });
    format!("{entry}\n")
}

fn append_panic(dir: &Path, line: &str) -> io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let day = chrono::Local::now().format("%Y%m%d").to_string();
    let path = dir.join(format!("panic-{day}.log"));
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    file.write_all(line.as_bytes())
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

    #[test]
    fn redacts_headers_and_url_credentials() {
        clear_secrets();
        let text = "Authorization: Bearer abc.def.ghi\nCookie: sid=topsecret; theme=light\n\
https://alice:p@ssw0rd@example.com/v1?token=xyz&q=1";
        let masked = redact(text);
        assert!(!masked.contains("abc.def.ghi"));
        assert!(!masked.contains("topsecret"));
        assert!(!masked.contains("p@ssw0rd"));
        assert!(!masked.contains("token=xyz"));
        assert!(masked.contains("Authorization: ••••••"));
        assert!(masked.contains("https://alice:••••••@example.com"));
    }

    #[test]
    fn redacts_json_fields() {
        clear_secrets();
        let masked = redact(r#"{"api_key":"sk-live-123","password":"hunter2","note":"ok"}"#);
        assert!(!masked.contains("sk-live-123"));
        assert!(!masked.contains("hunter2"));
        assert!(masked.contains("\"note\":\"ok\""));
    }

    #[test]
    fn json_lines_are_parseable_and_redacted() {
        clear_secrets();
        register_secret("sk-json-secret");
        let record = log::Record::builder()
            .args(format_args!("发送失败 token=sk-json-secret"))
            .level(log::Level::Warn)
            .target("astroman::test")
            .build();
        let mut buffer: Vec<u8> = Vec::new();
        format_json(&mut buffer, &record).unwrap();
        let text = String::from_utf8(buffer).unwrap();
        assert!(!text.contains("sk-json-secret"));
        let value: serde_json::Value = serde_json::from_str(text.trim()).unwrap();
        assert_eq!(value["level"], "warn");
        assert_eq!(value["target"], "astroman::test");
        assert!(value["ts"].as_str().unwrap().contains('T'));
        clear_secrets();
    }

    #[test]
    fn panic_line_contains_thread_and_location() {
        let line = panic_line("main", "src/lib.rs:10:2", "boom");
        let value: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(value["level"], "panic");
        assert_eq!(value["thread"], "main");
        assert_eq!(value["location"], "src/lib.rs:10:2");
        assert_eq!(value["msg"], "boom");
    }
}
