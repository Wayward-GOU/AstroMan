//! 本机大模型 token 用量看板（移植自独立工具 TokenPanel）。
//!
//! 口径：只解析各客户端自己写在盘上的**真实 usage**，不做任何估算。
//!   - WorkBuddy / Claude：`providerData.usage`（camelCase）+ `providerData.rawUsage`（Anthropic 原始字段）
//!   - Codex：`type=token_usage_record`（精确）+ `event_msg/token_count`（汇总，降级用）
//!
//! 扫描是增量的：按文件记录已消费的字节偏移，只读新增部分；文件被截断则从头重解析。
//! 同一条 messageId 可能同时出现在多个文件里，按 id 去重，精确记录优先于汇总记录。

use parking_lot::Mutex;
use serde::Serialize;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::Instant;
use tauri::State;

/* ------------------------------------------------------------------ */
/* 对外结构                                                            */
/* ------------------------------------------------------------------ */

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageRecord {
    pub id: String,
    pub ts: i64,
    pub source: String,
    pub model: String,
    pub workspace: String,
    pub workspace_path: String,
    pub session_id: String,
    pub input: i64,
    pub output: i64,
    pub cached_read: i64,
    pub cache_write: i64,
    pub reasoning: i64,
    pub total: i64,
    pub credit: f64,
    /// 该次调用费用（美元）：代理行走账面值，日志行按价目表估算
    pub cost_usd: f64,
    /// 费用来源：`billing` 账面 / `estimate` 估算 / `none` 无价目
    pub cost_kind: String,
    /// 供应商或中转名（只有 CC Switch 代理行有）
    pub provider: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceStat {
    pub found: bool,
    pub path: String,
    pub files: usize,
    pub requests: usize,
    pub tokens: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanStats {
    pub scanned_at: i64,
    pub duration_ms: u64,
    pub files: usize,
    pub parse_errors: usize,
    pub deduped: usize,
    /// 全量费用（美元），与所选时间窗口无关
    pub cost_usd: f64,
    /// 没有价目、未计入费用的记录数
    pub unpriced: usize,
    pub sources: HashMap<String, SourceStat>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanResult {
    pub records: Vec<UsageRecord>,
    pub stats: ScanStats,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RootInfo {
    pub source: String,
    pub dir: String,
    pub exists: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientProbe {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub status: String,
    pub source: Option<String>,
    pub paths: Vec<String>,
    pub requests: usize,
    pub tokens: i64,
    pub data_mb: Option<f64>,
    pub detail: String,
}

/* ------------------------------------------------------------------ */
/* 扫描状态                                                            */
/* ------------------------------------------------------------------ */

struct FileState {
    source: String,
    offset: u64,
    records: Vec<Parsed>,
    carried: String,
    strong: bool,
    ctx: ParseCtx,
}

#[derive(Clone, Default)]
struct ParseCtx {
    model: Option<String>,
    cwd: Option<String>,
    session_id: Option<String>,
}

struct Parsed {
    rec: UsageRecord,
    weak: bool,
}

pub struct TokenUsageState {
    files: Mutex<HashMap<PathBuf, FileState>>,
    /// 抓包侧的用量记账文件（`<数据目录>/capture/llm-usage.jsonl`）
    capture_path: PathBuf,
}

impl TokenUsageState {
    pub fn new(capture_path: PathBuf) -> Self {
        Self {
            files: Mutex::new(HashMap::new()),
            capture_path,
        }
    }
}

/* ------------------------------------------------------------------ */
/* 数据根目录                                                          */
/* ------------------------------------------------------------------ */

const SOURCE_ORDER: [&str; 6] = [
    "workbuddy",
    "codex",
    "claude",
    "chatgpt",
    "ccswitch",
    "capture",
];

fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
        .unwrap_or_default()
}

fn data_roots() -> Vec<(String, PathBuf)> {
    let home = home_dir();
    vec![
        (
            "workbuddy".to_string(),
            home.join(".workbuddy").join("projects"),
        ),
        ("codex".to_string(), home.join(".codex").join("sessions")),
        (
            "codex".to_string(),
            home.join(".codex").join("archived_sessions"),
        ),
        ("claude".to_string(), home.join(".claude").join("projects")),
    ]
}

/* ------------------------------------------------------------------ */
/* CC Switch 本地记账库                                                */
/* ------------------------------------------------------------------ */

/// cc-switch 的库里，这些 `app_type` 对应的日志我们已经自己逐文件解析过，
/// 再用代理表里的行会和现有记录重复计数，必须排除。
const CCSWITCH_COVERED_APPS: [&str; 4] = ["workbuddy", "codex", "claude", "claude-code"];

#[derive(Debug, Clone, Default)]
struct Price {
    input: f64,
    output: f64,
    cache_read: f64,
    cache_write: f64,
}

#[derive(Default)]
struct CcSwitchData {
    records: Vec<UsageRecord>,
    stat: Option<SourceStat>,
    pricing: HashMap<String, Price>,
}

fn ccswitch_db_path() -> PathBuf {
    home_dir().join(".cc-switch").join("cc-switch.db")
}

/// 只读打开：cc-switch 可能正在写，保持只读 + 短超时，失败就让调用方降级
fn open_ccswitch(path: &Path) -> Result<rusqlite::Connection, String> {
    use rusqlite::OpenFlags;
    let conn = rusqlite::Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_URI
            | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| format!("打开失败：{e}"))?;
    let _ = conn.busy_timeout(std::time::Duration::from_millis(800));
    Ok(conn)
}

fn load_pricing(conn: &rusqlite::Connection) -> HashMap<String, Price> {
    let mut out = HashMap::new();
    let Ok(mut stmt) = conn.prepare(
        "SELECT model_id,
                CAST(input_cost_per_million AS TEXT),
                CAST(output_cost_per_million AS TEXT),
                CAST(cache_read_cost_per_million AS TEXT),
                CAST(cache_creation_cost_per_million AS TEXT)
         FROM model_pricing",
    ) else {
        return out;
    };
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1).unwrap_or_default(),
            r.get::<_, String>(2).unwrap_or_default(),
            r.get::<_, String>(3).unwrap_or_default(),
            r.get::<_, String>(4).unwrap_or_default(),
        ))
    });
    let Ok(rows) = rows else { return out };
    for row in rows.flatten() {
        let (id, input, output, cr, cw) = row;
        out.insert(
            id.trim().to_lowercase(),
            Price {
                input: input.trim().parse().unwrap_or(0.0),
                output: output.trim().parse().unwrap_or(0.0),
                cache_read: cr.trim().parse().unwrap_or(0.0),
                cache_write: cw.trim().parse().unwrap_or(0.0),
            },
        );
    }
    out
}

/// 从 cc-switch 库里取「我们解析不到的来源」的代理流水（目前是 Claude 桌面版等）
fn read_ccswitch() -> CcSwitchData {
    let path = ccswitch_db_path();
    let mut data = CcSwitchData::default();
    if !path.is_file() {
        return data;
    }
    let mut stat = empty_stat(true, path.display().to_string());
    stat.files = 1;

    let conn = match open_ccswitch(&path) {
        Ok(c) => c,
        Err(_) => {
            data.stat = Some(stat);
            return data;
        }
    };
    data.pricing = load_pricing(&conn);

    // 供应商名：老版本没有 providers 表，读不到就留空
    let mut providers: HashMap<String, String> = HashMap::new();
    if let Ok(mut stmt) = conn.prepare("SELECT id, name FROM providers") {
        if let Ok(rows) = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1).unwrap_or_default(),
            ))
        }) {
            for (id, name) in rows.flatten() {
                providers.insert(id, name);
            }
        }
    }

    let sql = "SELECT request_id, app_type, model, provider_id, input_tokens, output_tokens,
                      cache_read_tokens, cache_creation_tokens, CAST(total_cost_usd AS TEXT),
                      created_at, session_id
               FROM proxy_request_logs
               WHERE COALESCE(data_source, 'proxy') = 'proxy'";
    let Ok(mut stmt) = conn.prepare(sql) else {
        data.stat = Some(stat);
        return data;
    };
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0).unwrap_or_default(),
            r.get::<_, String>(1).unwrap_or_default(),
            r.get::<_, String>(2).unwrap_or_default(),
            r.get::<_, Option<String>>(3).unwrap_or_default(),
            r.get::<_, i64>(4).unwrap_or(0),
            r.get::<_, i64>(5).unwrap_or(0),
            r.get::<_, i64>(6).unwrap_or(0),
            r.get::<_, i64>(7).unwrap_or(0),
            r.get::<_, String>(8).unwrap_or_default(),
            r.get::<_, i64>(9).unwrap_or(0),
            r.get::<_, Option<String>>(10).unwrap_or_default(),
        ))
    });
    let Ok(rows) = rows else {
        data.stat = Some(stat);
        return data;
    };

    for row in rows.flatten() {
        let (
            request_id,
            app_type,
            model,
            provider_id,
            input_tokens,
            output_tokens,
            cache_read,
            cache_write,
            cost,
            created_at,
            session_id,
        ) = row;
        if CCSWITCH_COVERED_APPS.contains(&app_type.as_str()) {
            continue;
        }
        let provider = provider_id
            .and_then(|id| providers.get(&id).cloned())
            .unwrap_or_default();
        // cc-switch 的 input 不含缓存，这里换成与本地日志一致的「输入含缓存」口径
        let input = input_tokens + cache_read + cache_write;
        let total = input + output_tokens;
        let ts = if created_at > 1_000_000_000_000 {
            created_at
        } else {
            created_at.saturating_mul(1000)
        };
        if ts <= 0 {
            continue;
        }
        data.records.push(UsageRecord {
            id: format!("ccswitch:{request_id}"),
            ts,
            source: "ccswitch".to_string(),
            model: if model.trim().is_empty() {
                "(未知模型)".to_string()
            } else {
                model
            },
            workspace: if provider.is_empty() {
                "CC Switch 代理".to_string()
            } else {
                provider.clone()
            },
            workspace_path: path.display().to_string(),
            session_id: session_id.unwrap_or_else(|| format!("ccswitch:{app_type}")),
            input,
            output: output_tokens,
            cached_read: cache_read,
            cache_write,
            reasoning: 0,
            total,
            credit: 0.0,
            cost_usd: cost.trim().parse().unwrap_or(0.0),
            // 代理流水是 cc-switch 自己算过的账面值，不再按价目表重算
            cost_kind: "billing".to_string(),
            provider,
        });
    }

    data.records.sort_by_key(|r| r.ts);
    stat.requests = data.records.len();
    stat.tokens = data.records.iter().map(|r| r.total).sum();
    data.stat = Some(stat);
    data
}

/// 抓包旁路记账（`capture/llm-usage.jsonl`）。
///
/// 每行一条 assistant 消息，字段来自客户端直连时的真实响应 `usage`，
/// 与解析 session 日志得到的数字同源同口径，因此可以直接合并。
/// 费用走 `apply_estimate`（价目表来自 cc-switch），标 `estimate`。
fn read_capture(path: &Path) -> (Vec<UsageRecord>, SourceStat) {
    let exists = path.is_file();
    let mut stat = empty_stat(exists, path.display().to_string());
    let mut records = Vec::new();
    if !exists {
        return (records, stat);
    }
    stat.files = 1;

    let Ok(text) = fs::read_to_string(path) else {
        return (records, stat);
    };
    let mut ids: HashSet<String> = HashSet::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let input = as_i64(as_num(get(&value, &["input"])));
        let output = as_i64(as_num(get(&value, &["output"])));
        let cache_read = as_i64(as_num(get(&value, &["cacheRead"])));
        let cache_write = as_i64(as_num(get(&value, &["cacheWrite"])));
        let total = input + cache_read + cache_write + output;
        if total <= 0 {
            continue;
        }
        let raw_id = get(&value, &["id"])
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string();
        if raw_id.is_empty() {
            continue;
        }
        let id = format!("capture:{raw_id}");
        if !ids.insert(id.clone()) {
            continue;
        }
        let host = get(&value, &["host"])
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let model = get(&value, &["model"])
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("(未知模型)")
            .to_string();
        let client = get(&value, &["client"])
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let ts = as_i64(as_num(get(&value, &["ts"])));
        if ts <= 0 {
            continue;
        }
        records.push(UsageRecord {
            id,
            ts,
            source: "capture".to_string(),
            model,
            workspace: if host.is_empty() {
                "抓包".to_string()
            } else {
                host.clone()
            },
            workspace_path: path.display().to_string(),
            session_id: format!("capture:{host}"),
            // 与本地日志一致：input 含缓存
            input: input + cache_read + cache_write,
            output,
            cached_read: cache_read,
            cache_write,
            reasoning: 0,
            total,
            credit: 0.0,
            cost_usd: 0.0,
            cost_kind: String::new(),
            provider: if client.is_empty() {
                "抓包观测".to_string()
            } else {
                client
            },
        });
    }

    records.sort_by_key(|r| r.ts);
    stat.requests = records.len();
    stat.tokens = records.iter().map(|r| r.total).sum();
    (records, stat)
}

/// 按价目表估算：输入（不含缓存）/ 缓存读 / 缓存写 / 输出分别计价
fn estimate_cost(rec: &UsageRecord, price: &Price) -> f64 {
    let uncached = (rec.input - rec.cached_read - rec.cache_write).max(0) as f64;
    (uncached * price.input
        + rec.cached_read as f64 * price.cache_read
        + rec.cache_write as f64 * price.cache_write
        + rec.output as f64 * price.output)
        / 1_000_000.0
}

/// 给没有账面值的记录补估算费用，返回无价目的条数
fn apply_estimate(records: &mut [UsageRecord], pricing: &HashMap<String, Price>) -> usize {
    let mut unpriced = 0usize;
    for rec in records.iter_mut() {
        if rec.cost_kind == "billing" {
            continue;
        }
        match pricing.get(&rec.model.trim().to_lowercase()) {
            Some(price) => {
                rec.cost_usd = estimate_cost(rec, price);
                rec.cost_kind = "estimate".to_string();
            }
            None => {
                rec.cost_usd = 0.0;
                rec.cost_kind = "none".to_string();
                unpriced += 1;
            }
        }
    }
    unpriced
}

/* ------------------------------------------------------------------ */
/* 解析                                                                */
/* ------------------------------------------------------------------ */

fn as_num(v: Option<&Value>) -> f64 {
    match v {
        Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0),
        Some(Value::String(s)) => s.trim().parse::<f64>().unwrap_or(0.0),
        _ => 0.0,
    }
}

/// 取嵌套字段：`get(v, &["a","b"])`。纯数字键按数组下标处理
/// （AntD/OpenAI 的 `inputTokensDetails[0].cached_tokens` 就属于这种形态）
fn get<'a>(v: &'a Value, path: &[&str]) -> Option<&'a Value> {
    let mut cur = v;
    for key in path {
        cur = match cur {
            Value::Array(items) => items.get(key.parse::<usize>().ok()?)?,
            Value::Object(map) => map.get(*key)?,
            _ => return None,
        };
    }
    Some(cur)
}

fn as_i64(n: f64) -> i64 {
    if n.is_finite() {
        n.round() as i64
    } else {
        0
    }
}

/// 时间戳兼容：ISO 字符串 / 秒 / 毫秒
fn normalize_ts(v: Option<&Value>) -> i64 {
    match v {
        Some(Value::String(s)) => parse_iso_ms(s).unwrap_or(0),
        Some(Value::Number(n)) => {
            let raw = n.as_f64().unwrap_or(0.0);
            if raw <= 0.0 {
                0
            } else if raw > 1e11 {
                raw as i64
            } else {
                (raw * 1000.0) as i64
            }
        }
        _ => 0,
    }
}

/// 极简 ISO8601 → epoch 毫秒（只处理日志里实际出现的 `2026-11-27T02:30:00.123Z` 形态）
fn parse_iso_ms(s: &str) -> Option<i64> {
    let t = s.trim();
    if t.len() < 19 {
        return None;
    }
    let num = |a: usize, b: usize| t.get(a..b)?.parse::<i64>().ok();
    let (y, mo, d) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
    let (h, mi, se) = (num(11, 13)?, num(14, 16)?, num(17, 19)?);
    let ms = t
        .get(20..23)
        .and_then(|m| m.parse::<i64>().ok())
        .filter(|_| t.as_bytes().get(19) == Some(&b'.'))
        .unwrap_or(0);
    let mut secs = days_from_civil(y, mo, d) * 86_400 + h * 3600 + mi * 60 + se;
    // 时区：Z / +08:00 / -05:00，缺省按本地时区外的 UTC 处理
    if let Some(idx) = t.rfind(['Z', '+', '-']).filter(|i| *i >= 19) {
        if t.as_bytes()[idx] == b'-' || t.as_bytes()[idx] == b'+' {
            let sign = if t.as_bytes()[idx] == b'-' { -1 } else { 1 };
            let hh = t
                .get(idx + 1..idx + 3)
                .and_then(|x| x.parse::<i64>().ok())
                .unwrap_or(0);
            let mm = t
                .get(idx + 4..idx + 6)
                .and_then(|x| x.parse::<i64>().ok())
                .unwrap_or(0);
            secs -= sign * (hh * 3600 + mm * 60);
        }
    }
    Some(secs * 1000 + ms)
}

/// Howard Hinnant 的 civil→days 算法，避免引 chrono 的解析依赖
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn workspace_of(p: Option<&str>) -> (String, String) {
    let raw = match p {
        Some(v) if !v.trim().is_empty() => v.trim(),
        _ => return ("(未知工作空间)".to_string(), String::new()),
    };
    let clean = raw.trim_end_matches(['/', '\\']).to_string();
    let name = clean
        .rsplit(['/', '\\'])
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or(&clean)
        .to_string();
    (name, clean)
}

/// WorkBuddy / CodeBuddy / Claude 桌面端日志
fn parse_workbuddy(
    obj: &Value,
    ctx: &ParseCtx,
    fallback_session: &str,
    source: &str,
) -> Option<Parsed> {
    let usage = get(obj, &["providerData", "usage"])
        .or_else(|| get(obj, &["message", "usage"]))
        .filter(|v| v.is_object())?;

    let raw = get(obj, &["providerData", "rawUsage"])
        .filter(|v| v.is_object())
        .cloned()
        .unwrap_or(Value::Null);

    // Anthropic 风格：usage.input_tokens 不含缓存，需自行补齐
    let anthropic_style =
        usage.get("inputTokens").is_none() && usage.get("cache_read_input_tokens").is_some();

    let cached_tokens = as_num(get(usage, &["inputTokensDetails", "0", "cached_tokens"]));
    let cache_read = if cached_tokens != 0.0 {
        cached_tokens
    } else {
        let cand = [
            as_num(raw.get("cache_read_input_tokens")),
            as_num(raw.get("prompt_cache_hit_tokens")),
            as_num(usage.get("cache_read_input_tokens")),
        ];
        cand.into_iter().find(|v| *v != 0.0).unwrap_or(0.0)
    };
    let cache_write = [
        as_num(raw.get("cache_creation_input_tokens")),
        as_num(raw.get("prompt_cache_write_tokens")),
        as_num(usage.get("cache_creation_input_tokens")),
    ]
    .into_iter()
    .find(|v| *v != 0.0)
    .unwrap_or(0.0);

    let raw_input = {
        let a = as_num(usage.get("inputTokens"));
        if a != 0.0 {
            a
        } else {
            as_num(usage.get("input_tokens"))
        }
    };
    let input = if anthropic_style {
        raw_input + cache_read + cache_write
    } else {
        raw_input
    };
    let output = {
        let a = as_num(usage.get("outputTokens"));
        if a != 0.0 {
            a
        } else {
            as_num(usage.get("output_tokens"))
        }
    };
    let total = {
        let explicit = {
            let a = as_num(usage.get("totalTokens"));
            if a != 0.0 {
                a
            } else {
                as_num(usage.get("total_tokens"))
            }
        };
        if explicit != 0.0 {
            explicit
        } else {
            input + output
        }
    };
    let reasoning = {
        let a = as_num(get(
            usage,
            &["outputTokensDetails", "0", "reasoning_tokens"],
        ));
        if a != 0.0 {
            a
        } else {
            as_num(raw.get("completion_thinking_tokens"))
        }
    };
    let credit = as_num(raw.get("credit"));

    let ts = normalize_ts(obj.get("timestamp"));
    if ts == 0 {
        return None;
    }

    let (ws_name, ws_path) = workspace_of(obj.get("cwd").and_then(|v| v.as_str()));
    let session_id = obj
        .get("sessionId")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| fallback_session.to_string());
    let id = get(obj, &["providerData", "messageId"])
        .or_else(|| obj.get("id"))
        .and_then(|v| match v {
            Value::String(s) => Some(s.clone()),
            Value::Number(n) => Some(n.to_string()),
            _ => None,
        })
        .unwrap_or_else(|| format!("{session_id}:{ts}:{}", as_i64(total)));

    let model = get(obj, &["providerData", "model"])
        .or_else(|| get(obj, &["message", "model"]))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .or_else(|| ctx.model.clone())
        .unwrap_or_else(|| "(未知模型)".to_string());

    Some(Parsed {
        rec: UsageRecord {
            id,
            ts,
            source: source.to_string(),
            model,
            workspace: ws_name,
            workspace_path: ws_path,
            session_id,
            input: as_i64(input),
            output: as_i64(output),
            cached_read: as_i64(cache_read),
            cache_write: as_i64(cache_write),
            reasoning: as_i64(reasoning),
            total: as_i64(total),
            credit,
            cost_usd: 0.0,
            cost_kind: "none".to_string(),
            provider: String::new(),
        },
        weak: false,
    })
}

/// Codex / ChatGPT Work 会话日志
fn parse_codex(
    obj: &Value,
    ctx: &mut ParseCtx,
    fallback_session: &str,
    source: &str,
) -> Option<Parsed> {
    let ty = obj.get("type").and_then(|v| v.as_str()).unwrap_or("");
    let pl = obj.get("payload").cloned().unwrap_or(Value::Null);

    match ty {
        "session_meta" => {
            if let Some(cwd) = pl.get("cwd").and_then(|v| v.as_str()) {
                ctx.cwd = Some(cwd.to_string());
            }
            let sid = pl
                .get("session_id")
                .or_else(|| pl.get("id"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| fallback_session.to_string());
            ctx.session_id = Some(sid);
            None
        }
        "turn_context" => {
            if let Some(m) = pl.get("model").and_then(|v| v.as_str()) {
                ctx.model = Some(m.to_string());
            }
            if let Some(cwd) = pl.get("cwd").and_then(|v| v.as_str()) {
                ctx.cwd = Some(cwd.to_string());
            }
            None
        }
        "token_usage_record" => {
            let usage = pl
                .get("usage")
                .or_else(|| pl.get("turn_token_usage"))
                .filter(|v| v.is_object())?;
            let input = as_num(usage.get("input_tokens"));
            let output = as_num(usage.get("output_tokens"));
            let explicit = as_num(usage.get("total_tokens"));
            let total = if explicit != 0.0 {
                explicit
            } else {
                input + output
            };
            let ts = normalize_ts(obj.get("timestamp"));
            if ts == 0 {
                return None;
            }
            let (ws_name, ws_path) = workspace_of(ctx.cwd.as_deref());
            let session_id = pl
                .get("session_id")
                .or_else(|| pl.get("thread_id"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .or_else(|| ctx.session_id.clone())
                .unwrap_or_else(|| fallback_session.to_string());
            let id = pl
                .get("response_id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .or_else(|| {
                    obj.get("ordinal")
                        .and_then(|v| v.as_i64())
                        .map(|o| format!("{session_id}:{o}"))
                })
                .unwrap_or_else(|| format!("{session_id}:{ts}"));
            Some(Parsed {
                rec: UsageRecord {
                    id,
                    ts,
                    source: source.to_string(),
                    model: ctx
                        .model
                        .clone()
                        .unwrap_or_else(|| "(未知模型)".to_string()),
                    workspace: ws_name,
                    workspace_path: ws_path,
                    session_id,
                    input: as_i64(input),
                    output: as_i64(output),
                    cached_read: as_i64(as_num(usage.get("cached_input_tokens"))),
                    cache_write: as_i64(as_num(usage.get("cache_write_input_tokens"))),
                    reasoning: as_i64(as_num(usage.get("reasoning_output_tokens"))),
                    total: as_i64(total),
                    credit: 0.0,
                    cost_usd: 0.0,
                    cost_kind: "none".to_string(),
                    provider: String::new(),
                },
                weak: false,
            })
        }
        "event_msg" => {
            if pl.get("type").and_then(|v| v.as_str()) != Some("token_count") {
                return None;
            }
            let usage = get(&pl, &["info", "last_token_usage"]).filter(|v| v.is_object())?;
            let input = as_num(usage.get("input_tokens"));
            let output = as_num(usage.get("output_tokens"));
            let explicit = as_num(usage.get("total_tokens"));
            let total = if explicit != 0.0 {
                explicit
            } else {
                input + output
            };
            if total <= 0.0 {
                return None;
            }
            let ts = normalize_ts(obj.get("timestamp"));
            if ts == 0 {
                return None;
            }
            let (ws_name, ws_path) = workspace_of(ctx.cwd.as_deref());
            let session_id = ctx
                .session_id
                .clone()
                .unwrap_or_else(|| fallback_session.to_string());
            let ordinal = obj
                .get("ordinal")
                .and_then(|v| v.as_i64())
                .map(|o| o.to_string())
                .unwrap_or_else(|| ts.to_string());
            Some(Parsed {
                rec: UsageRecord {
                    id: format!("weak:{session_id}:{ordinal}"),
                    ts,
                    source: source.to_string(),
                    model: ctx
                        .model
                        .clone()
                        .unwrap_or_else(|| "(未知模型)".to_string()),
                    workspace: ws_name,
                    workspace_path: ws_path,
                    session_id,
                    input: as_i64(input),
                    output: as_i64(output),
                    cached_read: as_i64(as_num(usage.get("cached_input_tokens"))),
                    cache_write: as_i64(as_num(usage.get("cache_write_input_tokens"))),
                    reasoning: as_i64(as_num(usage.get("reasoning_output_tokens"))),
                    total: as_i64(total),
                    credit: 0.0,
                    cost_usd: 0.0,
                    cost_kind: "none".to_string(),
                    provider: String::new(),
                },
                weak: true,
            })
        }
        _ => None,
    }
}

/* ------------------------------------------------------------------ */
/* 目录遍历                                                            */
/* ------------------------------------------------------------------ */

fn walk_jsonl(dir: &Path, out: &mut Vec<PathBuf>, depth: usize) {
    if depth > 8 {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        match entry.file_type() {
            Ok(ft) if ft.is_dir() => {
                if name == "node_modules" || name.starts_with(".git") {
                    continue;
                }
                walk_jsonl(&path, out, depth + 1);
            }
            Ok(ft) if ft.is_file() && name.ends_with(".jsonl") => out.push(path),
            _ => {}
        }
    }
}

fn empty_stat(found: bool, path: String) -> SourceStat {
    SourceStat {
        found,
        path,
        files: 0,
        requests: 0,
        tokens: 0,
    }
}

/* ------------------------------------------------------------------ */
/* 扫描主流程                                                          */
/* ------------------------------------------------------------------ */

pub fn scan_all(state: &TokenUsageState, force: bool) -> ScanResult {
    let started = Instant::now();
    let mut files_map = state.files.lock();
    if force {
        files_map.clear();
    }

    let mut stats: HashMap<String, SourceStat> = HashMap::new();
    for s in SOURCE_ORDER {
        stats.insert(s.to_string(), empty_stat(false, String::new()));
    }
    let mut parse_errors = 0usize;
    let mut seen: HashSet<PathBuf> = HashSet::new();

    for (source, root) in data_roots() {
        let exists = root.is_dir();
        if let Some(st) = stats.get_mut(&source) {
            if exists {
                st.found = true;
                st.path = if st.path.is_empty() {
                    root.display().to_string()
                } else {
                    format!("{}\n{}", st.path, root.display())
                };
            }
        }
        if !exists {
            continue;
        }

        let mut files = Vec::new();
        walk_jsonl(&root, &mut files, 0);

        for file in files {
            seen.insert(file.clone());
            let Ok(meta) = fs::metadata(&file) else {
                continue;
            };
            let size = meta.len();

            if !files_map.contains_key(&file) {
                files_map.insert(
                    file.clone(),
                    FileState {
                        source: source.clone(),
                        offset: 0,
                        records: Vec::new(),
                        carried: String::new(),
                        strong: false,
                        ctx: ParseCtx::default(),
                    },
                );
            }

            let state_entry = files_map.get_mut(&file).expect("刚插入的文件状态");
            // 文件被截断或轮转：从头重解析
            if size < state_entry.offset {
                state_entry.offset = 0;
                state_entry.carried.clear();
                state_entry.records.clear();
                state_entry.strong = false;
                state_entry.ctx = ParseCtx::default();
            }
            if size > state_entry.offset {
                parse_errors += read_append(&file, state_entry, size);
            }
            if let Some(st) = stats.get_mut(&source) {
                st.files += 1;
            }
        }
    }

    // 清理已消失文件的状态
    files_map.retain(|path, _| seen.contains(path));

    // 归集 + 去重
    let mut by_id: HashMap<String, Parsed> = HashMap::new();
    let mut before = 0usize;
    for st in files_map.values() {
        for parsed in &st.records {
            if parsed.weak && st.strong {
                continue;
            }
            before += 1;
            let replace = match by_id.get(&parsed.rec.id) {
                None => true,
                Some(prev) => prev.weak && !parsed.weak,
            };
            if replace {
                by_id.insert(
                    parsed.rec.id.clone(),
                    Parsed {
                        rec: parsed.rec.clone(),
                        weak: parsed.weak,
                    },
                );
            }
        }
    }

    let deduped = before.saturating_sub(by_id.len());

    // CC Switch 代理流水：只补本地日志解析不到的来源（如 Claude 桌面版），
    // 它自带的账面费用原样保留，不再按价目表重算
    let mut cc = read_ccswitch();
    let mut cc_index: HashMap<(i64, i64), Vec<i64>> = HashMap::new();
    for rec in &cc.records {
        cc_index
            .entry((rec.total, rec.output))
            .or_default()
            .push(rec.ts);
    }
    for rec in cc.records.drain(..) {
        by_id
            .entry(rec.id.clone())
            .or_insert(Parsed { rec, weak: false });
    }

    // 抓包旁路记账：客户端直连各家中转时的真实 usage。
    // 同一次调用若代理流水里已有（总量与输出一致、时间相差一分钟内），
    // 以账面值为准，避免同一批流量重复计数
    let (capture_records, capture_stat) = read_capture(&state.capture_path);
    for rec in capture_records {
        let duplicate = cc_index
            .get(&(rec.total, rec.output))
            .is_some_and(|stamps| stamps.iter().any(|stamp| (stamp - rec.ts).abs() <= 60_000));
        if duplicate {
            continue;
        }
        by_id
            .entry(rec.id.clone())
            .or_insert(Parsed { rec, weak: false });
    }

    let mut records: Vec<UsageRecord> = by_id.into_values().map(|p| p.rec).collect();
    records.sort_by_key(|r| r.ts);

    let unpriced = apply_estimate(&mut records, &cc.pricing);
    let cost_usd: f64 = records.iter().map(|r| r.cost_usd).sum();

    for rec in &records {
        if let Some(st) = stats.get_mut(&rec.source) {
            st.requests += 1;
            st.tokens += rec.total;
        }
    }
    if let Some(st) = cc.stat.take() {
        stats.insert("ccswitch".to_string(), st);
    }
    {
        // 抓包来源：路径与文件数取自记账文件本身，计数以合并去重后的结果为准
        let mut st = capture_stat;
        st.requests = records.iter().filter(|r| r.source == "capture").count();
        st.tokens = records
            .iter()
            .filter(|r| r.source == "capture")
            .map(|r| r.total)
            .sum();
        stats.insert("capture".to_string(), st);
    }

    let scanned_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);

    ScanResult {
        records,
        stats: ScanStats {
            scanned_at,
            duration_ms: started.elapsed().as_millis() as u64,
            files: seen.len(),
            parse_errors,
            deduped,
            cost_usd,
            unpriced,
            sources: stats,
        },
    }
}

/// 从上次偏移继续解析，返回坏行数
fn read_append(file: &Path, st: &mut FileState, size: u64) -> usize {
    let mut errors = 0usize;
    let start = st.offset;
    let len = size.saturating_sub(start);
    if len == 0 {
        return 0;
    }

    let mut buf = vec![0u8; len as usize];
    let read = (|| -> std::io::Result<usize> {
        let mut f = fs::File::open(file)?;
        f.seek(SeekFrom::Start(start))?;
        f.read(&mut buf)
    })();

    let read = match read {
        Ok(n) => n,
        Err(_) => return 1,
    };
    if read == 0 {
        return 0;
    }

    // 只消费到最后一个换行符，半个字符/半行留到下次
    let slice = &buf[..read];
    let last_nl = match slice.iter().rposition(|b| *b == b'\n') {
        Some(i) => i,
        None => return 0,
    };

    let text = format!(
        "{}{}",
        st.carried,
        String::from_utf8_lossy(&slice[..=last_nl])
    );
    st.carried.clear();

    let fallback_session = file
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();

    for line in text.split('\n') {
        let s = line.trim();
        if s.is_empty() {
            continue;
        }
        let Ok(obj) = serde_json::from_str::<Value>(s) else {
            errors += 1;
            continue;
        };
        let source = st.source.clone();
        let parsed = if source == "codex" || source == "chatgpt" {
            parse_codex(&obj, &mut st.ctx, &fallback_session, &source)
        } else {
            parse_workbuddy(&obj, &st.ctx, &fallback_session, &source)
        };
        if let Some(p) = parsed {
            if !p.weak {
                st.strong = true;
            }
            st.records.push(p);
        }
    }

    st.offset = start + last_nl as u64 + 1;
    errors
}

/* ------------------------------------------------------------------ */
/* 数据源体检                                                          */
/* ------------------------------------------------------------------ */

struct ClientDef {
    id: &'static str,
    name: &'static str,
    kind: &'static str,
    source: Option<&'static str>,
    usage_roots: Vec<PathBuf>,
    detect: Vec<PathBuf>,
    blind_detail: &'static str,
}

fn client_defs() -> Vec<ClientDef> {
    let home = home_dir();
    #[cfg(target_os = "macos")]
    let app_support = home.join("Library").join("Application Support");
    #[cfg(not(target_os = "macos"))]
    let app_support = home.join("AppData").join("Roaming");

    let wb_projects = home.join(".workbuddy").join("projects");
    let codex_sessions = home.join(".codex").join("sessions");
    let codex_archived = home.join(".codex").join("archived_sessions");
    let claude_projects = home.join(".claude").join("projects");

    vec![
        ClientDef {
            id: "workbuddy",
            name: "WorkBuddy",
            kind: "desktop",
            source: Some("workbuddy"),
            usage_roots: vec![wb_projects],
            detect: vec![],
            blind_detail: "",
        },
        ClientDef {
            id: "codex",
            name: "Codex",
            kind: "cli",
            source: Some("codex"),
            usage_roots: vec![codex_sessions, codex_archived],
            detect: vec![],
            blind_detail: "",
        },
        ClientDef {
            id: "claude-code",
            name: "Claude Code",
            kind: "cli",
            source: Some("claude"),
            usage_roots: vec![claude_projects],
            detect: vec![home.join(".claude")],
            blind_detail: "",
        },
        ClientDef {
            id: "qoder",
            name: "Qoder",
            kind: "ide",
            source: None,
            usage_roots: vec![],
            detect: vec![
                app_support.join("com.qodercn.app.stable"),
                app_support.join("Qoder"),
                PathBuf::from("/Applications/Qoder CN.app"),
                PathBuf::from("/Applications/Qoder.app"),
            ],
            blind_detail: "本地只有会话与缓存，不含 token 用量字段",
        },
        ClientDef {
            id: "trae",
            name: "Trae",
            kind: "ide",
            source: None,
            usage_roots: vec![],
            detect: vec![
                app_support.join("Trae CN"),
                app_support.join("Trae"),
                PathBuf::from("/Applications/Trae CN.app"),
                PathBuf::from("/Applications/Trae.app"),
            ],
            blind_detail: "日志与数据库均无 token 用量",
        },
        ClientDef {
            id: "cursor",
            name: "Cursor",
            kind: "ide",
            source: None,
            usage_roots: vec![],
            detect: vec![
                app_support.join("Cursor"),
                PathBuf::from("/Applications/Cursor.app"),
            ],
            blind_detail: "不写 token 用量到本地",
        },
        ClientDef {
            id: "windsurf",
            name: "Windsurf",
            kind: "ide",
            source: None,
            usage_roots: vec![],
            detect: vec![
                app_support.join("Windsurf"),
                PathBuf::from("/Applications/Windsurf.app"),
            ],
            blind_detail: "不写 token 用量到本地",
        },
        ClientDef {
            id: "vscode",
            name: "VS Code / Copilot",
            kind: "ide",
            source: None,
            usage_roots: vec![],
            detect: vec![
                app_support.join("Code"),
                PathBuf::from("/Applications/Visual Studio Code.app"),
            ],
            blind_detail: "扩展的模型调用不落 token 用量",
        },
        ClientDef {
            id: "zed",
            name: "Zed",
            kind: "ide",
            source: None,
            usage_roots: vec![],
            detect: vec![
                app_support.join("Zed"),
                PathBuf::from("/Applications/Zed.app"),
            ],
            blind_detail: "会话在本地，但无 token 字段",
        },
        ClientDef {
            id: "claude-desktop",
            name: "Claude（桌面版）",
            kind: "desktop",
            source: None,
            usage_roots: vec![],
            detect: vec![
                app_support.join("Claude"),
                PathBuf::from("/Applications/Claude.app"),
            ],
            blind_detail: "本地只有套餐额度，没有逐次 token",
        },
        ClientDef {
            id: "chatgpt-desktop",
            name: "ChatGPT（桌面版）",
            kind: "desktop",
            source: None,
            usage_roots: vec![],
            detect: vec![
                app_support.join("ChatGPT"),
                PathBuf::from("/Applications/ChatGPT.app"),
            ],
            blind_detail: "不写 token 用量到本地",
        },
    ]
}

/// 目录体积（MB）：macOS / Linux 走 du，Windows 递归估算并设上限
fn dir_size_mb(dir: &Path) -> Option<f64> {
    #[cfg(unix)]
    {
        let out = std::process::Command::new("du")
            .arg("-sk")
            .arg(dir)
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        let text = String::from_utf8_lossy(&out.stdout);
        let kb = text.split_whitespace().next()?.parse::<f64>().ok()?;
        Some((kb / 1024.0 * 10.0).round() / 10.0)
    }
    #[cfg(not(unix))]
    {
        fn walk(dir: &Path, total: &mut u64, budget: &mut u32) {
            if *budget == 0 {
                return;
            }
            let Ok(entries) = fs::read_dir(dir) else {
                return;
            };
            for entry in entries.flatten() {
                if *budget == 0 {
                    return;
                }
                *budget -= 1;
                let Ok(meta) = entry.metadata() else { continue };
                if meta.is_dir() {
                    walk(&entry.path(), total, budget);
                } else {
                    *total += meta.len();
                }
            }
        }
        let mut total = 0u64;
        let mut budget = 200_000u32;
        walk(dir, &mut total, &mut budget);
        Some((total as f64 / 1024.0 / 1024.0 * 10.0).round() / 10.0)
    }
}

pub fn probe_clients(stats: &HashMap<String, SourceStat>) -> Vec<ClientProbe> {
    let mut out = Vec::new();

    for def in client_defs() {
        let mut paths: Vec<String> = Vec::new();
        let mut usage_exists = false;

        for root in &def.usage_roots {
            if root.is_dir() {
                usage_exists = true;
                paths.push(root.display().to_string());
            }
        }
        for p in &def.detect {
            if p.exists() {
                paths.push(p.display().to_string());
            }
        }

        let (status, detail, requests, tokens, data_mb) = if def.source.is_some() && usage_exists {
            let st = def.source.and_then(|s| stats.get(s));
            let requests = st.map(|s| s.requests).unwrap_or(0);
            let tokens = st.map(|s| s.tokens).unwrap_or(0);
            if requests > 0 {
                (
                    "covered",
                    format!("已从本机日志还原 {} 次调用", fmt_thousands(requests as i64)),
                    requests,
                    tokens,
                    None,
                )
            } else {
                (
                    "idle",
                    "日志目录在，但还没有调用记录".to_string(),
                    0,
                    0,
                    None,
                )
            }
        } else if def.source.is_some() {
            if paths.is_empty() {
                ("absent", "未检测到".to_string(), 0, 0, None)
            } else {
                (
                    "idle",
                    "已安装，但还没有可解析的调用日志".to_string(),
                    0,
                    0,
                    None,
                )
            }
        } else if !paths.is_empty() {
            let data_paths: Vec<&String> = paths.iter().filter(|p| !p.ends_with(".app")).collect();
            let size = data_paths
                .first()
                .and_then(|p| dir_size_mb(Path::new(p.as_str())));
            ("blind", def.blind_detail.to_string(), 0, 0, size)
        } else {
            ("absent", "未检测到".to_string(), 0, 0, None)
        };

        out.push(ClientProbe {
            id: def.id.to_string(),
            name: def.name.to_string(),
            kind: def.kind.to_string(),
            status: status.to_string(),
            source: def.source.map(|s| s.to_string()),
            paths,
            requests,
            tokens,
            data_mb,
            detail,
        });
    }

    // CC Switch 代理记账：本地日志读不到的客户端（如 Claude 桌面版）靠它补上
    if let Some(st) = stats.get("ccswitch") {
        if st.found {
            let db = ccswitch_db_path();
            let (status, detail) = if st.requests > 0 {
                (
                    "covered",
                    format!(
                        "已从 CC Switch 代理记账还原 {} 次调用（含 Claude 桌面版等本地无日志的客户端）",
                        fmt_thousands(st.requests as i64)
                    ),
                )
            } else {
                (
                    "idle",
                    "记账库在，但没有代理流水（未经它转发就没有记录）".to_string(),
                )
            };
            // 这批流量已经算在「CC Switch」一行里，桌面版不再单独占一行，
            // 否则同一批调用会在体检里出现两次、看起来像翻倍
            if st.requests > 0 {
                out.retain(|p| p.id != "claude-desktop");
            }
            out.push(ClientProbe {
                id: "ccswitch".to_string(),
                name: "CC Switch".to_string(),
                kind: "proxy".to_string(),
                status: status.to_string(),
                source: Some("ccswitch".to_string()),
                paths: vec![db.display().to_string()],
                requests: st.requests,
                tokens: st.tokens,
                data_mb: None,
                detail,
            });
        }
    }

    let weight = |s: &str| match s {
        "covered" => 0,
        "idle" => 1,
        "blind" => 2,
        _ => 3,
    };
    out.sort_by(|a, b| {
        weight(&a.status)
            .cmp(&weight(&b.status))
            .then(b.tokens.cmp(&a.tokens))
            .then(
                b.data_mb
                    .unwrap_or(0.0)
                    .partial_cmp(&a.data_mb.unwrap_or(0.0))
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
            .then(a.name.cmp(&b.name))
    });
    out
}

fn fmt_thousands(n: i64) -> String {
    let s = n.abs().to_string();
    let mut out = String::new();
    for (i, ch) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    if n < 0 {
        out.push('-');
    }
    out.chars().rev().collect()
}

/* ------------------------------------------------------------------ */
/* Tauri 命令                                                          */
/* ------------------------------------------------------------------ */

#[tauri::command]
pub fn token_usage_scan(state: State<'_, TokenUsageState>, force: Option<bool>) -> ScanResult {
    scan_all(&state, force.unwrap_or(false))
}

#[tauri::command]
pub fn token_usage_roots() -> Vec<RootInfo> {
    data_roots()
        .into_iter()
        .map(|(source, dir)| RootInfo {
            exists: dir.is_dir(),
            source,
            dir: dir.display().to_string(),
        })
        .collect()
}

#[tauri::command]
pub fn token_usage_clients(state: State<'_, TokenUsageState>) -> Vec<ClientProbe> {
    let result = scan_all(&state, false);
    probe_clients(&result.stats.sources)
}

#[tauri::command]
pub fn token_usage_reveal(path: String) -> Result<(), String> {
    let p = PathBuf::from(&path);
    if path.trim().is_empty() || !p.exists() {
        return Err("路径不存在".to_string());
    }
    #[cfg(target_os = "macos")]
    {
        // 目录：直接打开；文件：在访达中选中
        let mut cmd = std::process::Command::new("open");
        if p.is_dir() {
            cmd.arg(&p);
        } else {
            cmd.arg("-R").arg(&p);
        }
        let status = cmd.status().map_err(|e| format!("打开访达失败：{e}"))?;
        if !status.success() {
            return Err("打开访达失败".to_string());
        }
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(format!("/select,{}", p.display()))
            .spawn()
            .map_err(|e| format!("打开资源管理器失败：{e}"))?;
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let dir = if p.is_dir() {
            p.clone()
        } else {
            p.parent().map(|x| x.to_path_buf()).unwrap_or(p.clone())
        };
        std::process::Command::new("xdg-open")
            .arg(dir)
            .spawn()
            .map_err(|e| format!("打开文件管理器失败：{e}"))?;
    }
    Ok(())
}

/* ------------------------------------------------------------------ */
/* 单元测试                                                            */
/* ------------------------------------------------------------------ */

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_parse_matches_epoch() {
        // 2026-11-27T02:30:00Z == 1795746600000
        assert_eq!(
            parse_iso_ms("2026-11-27T02:30:00Z"),
            Some(1_795_746_600_000)
        );
    }

    #[test]
    fn iso_parse_handles_offset() {
        // 同一时刻的 +08:00 写法
        assert_eq!(
            parse_iso_ms("2026-11-27T10:30:00+08:00"),
            Some(1_795_746_600_000)
        );
    }

    #[test]
    fn ts_seconds_are_scaled() {
        let v = serde_json::json!(1_795_746_600i64);
        assert_eq!(normalize_ts(Some(&v)), 1_795_746_600_000);
    }

    #[test]
    fn workspace_takes_last_segment() {
        let (name, path) = workspace_of(Some("/Applications/my-system/ProjectFile/AstroMan/"));
        assert_eq!(name, "AstroMan");
        assert_eq!(path, "/Applications/my-system/ProjectFile/AstroMan");
    }

    #[test]
    fn workbuddy_anthropic_style_adds_cache_to_input() {
        let obj = serde_json::json!({
            "timestamp": "2026-11-27T02:30:00Z",
            "cwd": "/tmp/demo",
            "sessionId": "s1",
            "providerData": {
                "model": "deepseek-v4",
                "messageId": "m1",
                "usage": { "input_tokens": 100, "output_tokens": 20, "cache_read_input_tokens": 900 }
            }
        });
        let ctx = ParseCtx::default();
        let out = parse_workbuddy(&obj, &ctx, "fb", "workbuddy").expect("应解析出记录");
        assert_eq!(out.rec.input, 1000);
        assert_eq!(out.rec.cached_read, 900);
        assert_eq!(out.rec.total, 1020);
        assert_eq!(out.rec.workspace, "demo");
    }

    #[test]
    fn codex_keeps_model_from_turn_context() {
        let ctx_obj = serde_json::json!({
            "type": "turn_context",
            "payload": { "model": "gpt-5-codex", "cwd": "/tmp/ws" }
        });
        let mut ctx = ParseCtx::default();
        assert!(parse_codex(&ctx_obj, &mut ctx, "fb", "codex").is_none());

        let rec_obj = serde_json::json!({
            "type": "token_usage_record",
            "timestamp": "2026-11-27T02:30:00Z",
            "payload": {
                "response_id": "r1",
                "usage": { "input_tokens": 10, "output_tokens": 5, "total_tokens": 15 }
            }
        });
        let out = parse_codex(&rec_obj, &mut ctx, "fb", "codex").expect("应解析出记录");
        assert_eq!(out.rec.model, "gpt-5-codex");
        assert_eq!(out.rec.total, 15);
        assert_eq!(out.rec.workspace, "ws");
        assert!(!out.weak);
    }

    #[test]
    fn codex_summary_is_weak() {
        let obj = serde_json::json!({
            "type": "event_msg",
            "ordinal": 7,
            "timestamp": "2026-11-27T02:30:00Z",
            "payload": { "type": "token_count", "info": { "last_token_usage": { "input_tokens": 3, "output_tokens": 4 } } }
        });
        let mut ctx = ParseCtx {
            session_id: Some("s9".to_string()),
            ..Default::default()
        };
        let out = parse_codex(&obj, &mut ctx, "fb", "codex").expect("应解析出记录");
        assert!(out.weak);
        assert_eq!(out.rec.total, 7);
    }

    #[test]
    fn thousands_separator() {
        assert_eq!(fmt_thousands(1234567), "1,234,567");
    }

    #[test]
    fn capture_rows_merge_cache_into_input() {
        let dir = std::env::temp_dir().join(format!("astroman-capture-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("llm-usage.jsonl");
        let lines = [
            r#"{"id":"api.deepseek.com:443:msg_1","ts":1764200000000,"host":"api.deepseek.com:443","path":"/anthropic/v1/messages","model":"deepseek-chat","client":"Claude","status":200,"input":12,"output":40,"cacheRead":9000,"cacheWrite":300,"latencyMs":1500}"#,
            r#"{"id":"api.deepseek.com:443:msg_1","ts":1764200000000,"host":"api.deepseek.com:443","path":"/anthropic/v1/messages","model":"deepseek-chat","client":"Claude","status":200,"input":12,"output":40,"cacheRead":9000,"cacheWrite":300,"latencyMs":1500}"#,
            r#"{"id":"","ts":1764200000000,"host":"x","model":"y","input":1,"output":1}"#,
            r#"not json"#,
        ];
        fs::write(&path, lines.join("\n")).unwrap();

        let (records, stat) = read_capture(&path);
        assert_eq!(records.len(), 1, "重复 id 与坏行都要丢掉");
        assert_eq!(records[0].id, "capture:api.deepseek.com:443:msg_1");
        assert_eq!(
            records[0].input,
            12 + 9000 + 300,
            "input 要与本地日志同口径（含缓存）"
        );
        assert_eq!(records[0].output, 40);
        assert_eq!(records[0].total, 12 + 9000 + 300 + 40);
        assert_eq!(records[0].workspace, "api.deepseek.com:443");
        assert_eq!(records[0].provider, "Claude");
        assert_eq!(records[0].cost_kind, "", "费用留给 apply_estimate 填");
        assert!(stat.found);
        assert_eq!(stat.requests, 1);

        // 价目表命中后应标成 estimate
        let mut pricing = HashMap::new();
        pricing.insert(
            "deepseek-chat".to_string(),
            Price {
                input: 0.27,
                output: 1.10,
                cache_read: 0.07,
                cache_write: 0.27,
            },
        );
        let mut owned = records;
        let unpriced = apply_estimate(&mut owned, &pricing);
        assert_eq!(unpriced, 0);
        assert_eq!(owned[0].cost_kind, "estimate");
        assert!(owned[0].cost_usd > 0.0);
        let _ = fs::remove_dir_all(&dir);
    }

    /// 用本机真实日志跑一遍全流程，核对解析结果是否与独立工具 TokenPanel 一致。
    /// 依赖用户家目录，默认忽略；需要时 `cargo test -- --ignored token_usage` 手动跑。
    #[test]
    #[ignore]
    fn scans_real_home_logs() {
        // 真实日志核对只看 session 日志，抓包记账指向一个不存在的临时路径
        let state = TokenUsageState::new(
            std::env::temp_dir()
                .join("astroman-no-capture")
                .join("llm-usage.jsonl"),
        );
        let first = scan_all(&state, false);
        println!(
            "files={} records={} tokens={} deduped={} errors={} {}ms",
            first.stats.files,
            first.records.len(),
            first.records.iter().map(|r| r.total).sum::<i64>(),
            first.stats.deduped,
            first.stats.parse_errors,
            first.stats.duration_ms
        );
        for (source, stat) in &first.stats.sources {
            println!(
                "  {source}: found={} files={} requests={} tokens={}",
                stat.found, stat.files, stat.requests, stat.tokens
            );
        }
        // 增量扫描不应重复计数
        let second = scan_all(&state, false);
        assert_eq!(
            first.records.len(),
            second.records.len(),
            "增量扫描结果必须与首次一致"
        );
        assert_eq!(
            first.records.iter().map(|r| r.total).sum::<i64>(),
            second.records.iter().map(|r| r.total).sum::<i64>()
        );

        // 近 7 天窗口（含今天）：与独立工具 TokenPanel 的默认视图口径一致，用于人工核对
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        let cutoff = now_ms - 6 * 86_400_000;
        let (cnt, sum) = first
            .records
            .iter()
            .filter(|r| r.ts >= cutoff)
            .fold((0usize, 0i64), |acc, r| (acc.0 + 1, acc.1 + r.total));
        println!("近 7 天：{cnt} 条请求 / {sum} token");

        // 费用：账面 + 估算
        println!(
            "费用合计 ${:.4}，无价目 {} 条",
            first.stats.cost_usd, first.stats.unpriced
        );
        if let Some(st) = first.stats.sources.get("ccswitch") {
            println!(
                "  ccswitch: found={} requests={} tokens={}",
                st.found, st.requests, st.tokens
            );
        }
    }

    fn rec(model: &str, input: i64, cached: i64, cache_write: i64, output: i64) -> UsageRecord {
        UsageRecord {
            id: "t".to_string(),
            ts: 0,
            source: "workbuddy".to_string(),
            model: model.to_string(),
            workspace: "w".to_string(),
            workspace_path: String::new(),
            session_id: "s".to_string(),
            input,
            output,
            cached_read: cached,
            cache_write,
            reasoning: 0,
            total: input + output,
            credit: 0.0,
            cost_usd: 0.0,
            cost_kind: "none".to_string(),
            provider: String::new(),
        }
    }

    #[test]
    fn estimate_cost_prices_cache_separately() {
        let price = Price {
            input: 0.3,
            output: 1.2,
            cache_read: 0.006,
            cache_write: 0.0,
        };
        // 输入 100 万：其中 90 万是缓存读，未命中只有 10 万
        let r = rec("deepseek-v4.1-flash", 1_000_000, 900_000, 0, 100_000);
        let cost = estimate_cost(&r, &price);
        // 0.1M*0.3 + 0.9M*0.006 + 0.1M*1.2 = 0.03 + 0.0054 + 0.12
        assert!((cost - 0.1554).abs() < 1e-9, "实际 {cost}");
    }

    #[test]
    fn estimate_skips_billing_and_marks_unpriced() {
        let pricing: HashMap<String, Price> = HashMap::from([(
            "glm-5.3-flash".to_string(),
            Price {
                input: 0.15,
                output: 0.5,
                cache_read: 0.003,
                cache_write: 0.0,
            },
        )]);
        let mut billing = rec("unknown-model", 1000, 0, 0, 100);
        billing.cost_usd = 0.42;
        billing.cost_kind = "billing".to_string();
        let mut priced = rec("glm-5.3-flash", 1_000_000, 0, 0, 1_000_000);
        let mut unpriced = rec("no-such-model", 1000, 0, 0, 100);
        let mut list = vec![billing, priced.clone(), unpriced.clone()];
        let n = apply_estimate(&mut list, &pricing);
        assert_eq!(n, 1, "只有无价目的那条算作未计入");
        assert_eq!(list[0].cost_kind, "billing");
        assert!((list[0].cost_usd - 0.42).abs() < 1e-9);
        assert_eq!(list[1].cost_kind, "estimate");
        assert!((list[1].cost_usd - 0.65).abs() < 1e-9, "0.15 + 0.5");
        assert_eq!(list[2].cost_kind, "none");
        assert_eq!(list[2].cost_usd, 0.0);
        priced.cost_kind = "none".to_string();
        unpriced.cost_kind = "none".to_string();
    }

    #[test]
    fn covered_apps_are_excluded_from_ccswitch() {
        for app in ["workbuddy", "codex", "claude", "claude-code"] {
            assert!(CCSWITCH_COVERED_APPS.contains(&app), "{app} 必须排除");
        }
        assert!(!CCSWITCH_COVERED_APPS.contains(&"claude-desktop"));
    }
}
