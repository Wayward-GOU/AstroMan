use std::sync::atomic::Ordering;
use std::time::Duration;

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use futures_util::StreamExt;
use rusqlite::{params, Row};
use sha2::{Digest, Sha256};
use tauri::ipc::Channel;
use tauri::State;

use crate::models::{
    AiChatPayload, AiMessage, AiProvider, AiSession, AiStoredMessage, AiStreamEvent, AiToolDayStat,
    AiToolLog, AiToolLogPage, AiToolLogStats, AiToolUsageStat, AiUsageDay, AiUsageLog, AiUsagePage,
    AiUsageStat, AiUsageStats, HttpResult, SendRequestPayload,
};
use crate::prompts::XIAO_HI_SYSTEM_PROMPT;
use crate::state::AppState;
use crate::util::{err, now_ms};

/// 派生 AES-256 密钥的固定种子。
/// 属于「本机静态混淆」级别：能防止直接打开库文件看到明文 Key，但拿到程序本身仍可解出。
/// 不引入系统钥匙串，保证 Windows / macOS 行为一致。
const KEY_SEED: &str = "FbinHttp::ai::provider::v1";
const CIPHER_PREFIX: &str = "v1:";
/// AI 请求整体超时，长回答（尤其推理模型）留足时间
const AI_TIMEOUT_SECS: u64 = 300;

fn cipher() -> Aes256Gcm {
    let mut hasher = Sha256::new();
    hasher.update(KEY_SEED.as_bytes());
    let key = hasher.finalize();
    Aes256Gcm::new_from_slice(&key).expect("SHA-256 输出固定 32 字节")
}

fn encrypt_key(plain: &str) -> Result<String, String> {
    if plain.is_empty() {
        return Ok(String::new());
    }
    // 12 字节随机 nonce 取自 UUID v4，避免引入 rand 依赖
    let raw = uuid::Uuid::new_v4().into_bytes();
    let nonce_bytes = &raw[..12];
    let ciphertext = cipher()
        .encrypt(Nonce::from_slice(nonce_bytes), plain.as_bytes())
        .map_err(|_| "API Key 加密失败".to_string())?;
    let mut packed = nonce_bytes.to_vec();
    packed.extend_from_slice(&ciphertext);
    Ok(format!("{CIPHER_PREFIX}{}", STANDARD.encode(packed)))
}

fn decrypt_key(stored: &str) -> String {
    let Some(body) = stored.strip_prefix(CIPHER_PREFIX) else {
        // 兼容历史上以明文写入的旧行
        return stored.to_string();
    };
    let Ok(packed) = STANDARD.decode(body) else {
        return String::new();
    };
    if packed.len() <= 12 {
        return String::new();
    }
    let (nonce_bytes, ciphertext) = packed.split_at(12);
    match cipher().decrypt(Nonce::from_slice(nonce_bytes), ciphertext) {
        Ok(plain) => String::from_utf8(plain).unwrap_or_default(),
        Err(_) => String::new(),
    }
}

/// 列表回显用掩码：只留前 4 位与后 2 位，短 Key 全掩码
fn mask_key(plain: &str) -> String {
    if plain.is_empty() {
        return String::new();
    }
    if plain.chars().count() <= 6 {
        return "******".to_string();
    }
    let head: String = plain.chars().take(4).collect();
    let tail: String = plain
        .chars()
        .rev()
        .take(2)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("{head}******{tail}")
}

/// 列表 / 编辑回显：绝不返回明文 Key，只给掩码与「是否已配置」
fn row_to_provider(row: &Row) -> rusqlite::Result<AiProvider> {
    let stored: String = row.get(5)?;
    let plain = decrypt_key(&stored);
    Ok(AiProvider {
        id: Some(row.get(0)?),
        name: row.get(1)?,
        kind: row.get(2)?,
        base_url: row.get(3)?,
        model: row.get(4)?,
        api_key: String::new(),
        api_key_masked: mask_key(&plain),
        has_key: !plain.is_empty(),
        temperature: Some(row.get(6)?),
        max_tokens: Some(row.get::<_, i64>(7)?),
        timeout_secs: Some(row.get::<_, i64>(8)?),
        is_default: row.get::<_, i64>(9)? != 0,
    })
}

/// 内部调用（发送请求）才解密明文，明文不出 Rust 进程
fn row_to_provider_plain(row: &Row) -> rusqlite::Result<AiProvider> {
    let stored: String = row.get(5)?;
    let plain = decrypt_key(&stored);
    Ok(AiProvider {
        id: Some(row.get(0)?),
        name: row.get(1)?,
        kind: row.get(2)?,
        base_url: row.get(3)?,
        model: row.get(4)?,
        api_key: plain.clone(),
        api_key_masked: mask_key(&plain),
        has_key: !plain.is_empty(),
        temperature: Some(row.get(6)?),
        max_tokens: Some(row.get::<_, i64>(7)?),
        timeout_secs: Some(row.get::<_, i64>(8)?),
        is_default: row.get::<_, i64>(9)? != 0,
    })
}

const PROVIDER_COLUMNS: &str = "id, name, COALESCE(kind, 'openai'), base_url, model,
                                COALESCE(api_key_enc, ''), COALESCE(temperature, 0.3),
                                COALESCE(max_tokens, 0), COALESCE(timeout_secs, 300),
                                COALESCE(is_default, 0)";

#[tauri::command]
pub fn ai_provider_list(state: State<'_, AppState>) -> Result<Vec<AiProvider>, String> {
    let conn = state.pool.get().map_err(err)?;
    let sql = format!(
        "SELECT {PROVIDER_COLUMNS} FROM ai_providers
         WHERE COALESCE(enabled, 1) = 1
         ORDER BY is_default DESC, id ASC"
    );
    let mut stmt = conn.prepare(&sql).map_err(err)?;
    let rows = stmt.query_map([], row_to_provider).map_err(err)?;
    Ok(rows.filter_map(Result::ok).collect())
}

#[tauri::command]
pub fn ai_provider_save(
    state: State<'_, AppState>,
    provider: AiProvider,
) -> Result<AiProvider, String> {
    if provider.name.trim().is_empty() {
        return Err("请填写服务名称".to_string());
    }
    if provider.base_url.trim().is_empty() {
        return Err("请填写 Base URL".to_string());
    }
    if provider.model.trim().is_empty() {
        return Err("请填写模型名称".to_string());
    }
    let kind = match provider.kind.as_str() {
        "openai" | "anthropic" | "gemini" => provider.kind.clone(),
        other => return Err(format!("不支持的协议类型：{other}")),
    };

    let conn = state.pool.get().map_err(err)?;
    let now = now_ms();
    let temperature = provider.temperature.unwrap_or(0.3).clamp(0.0, 2.0);
    let max_tokens = provider.max_tokens.unwrap_or(0).max(0);
    let timeout_secs = match provider.timeout_secs.unwrap_or(AI_TIMEOUT_SECS as i64) {
        0 => AI_TIMEOUT_SECS as i64,
        value => value.clamp(10, 3600),
    };
    let key_enc = encrypt_key(&provider.api_key)?;

    if provider.is_default {
        conn.execute("UPDATE ai_providers SET is_default = 0", [])
            .map_err(err)?;
    }

    let id = match provider.id {
        Some(id) => {
            if provider.api_key.is_empty() {
                // 编辑时留空表示沿用原 Key
                conn.execute(
                    "UPDATE ai_providers
                     SET name = ?1, kind = ?2, base_url = ?3, model = ?4,
                         temperature = ?5, is_default = ?6, max_tokens = ?7,
                         timeout_secs = ?8, enabled = 1, updated_at = ?9
                     WHERE id = ?10",
                    params![
                        provider.name.trim(),
                        kind,
                        provider.base_url.trim(),
                        provider.model.trim(),
                        temperature,
                        provider.is_default as i64,
                        max_tokens,
                        timeout_secs,
                        now,
                        id
                    ],
                )
                .map_err(err)?;
            } else {
                conn.execute(
                    "UPDATE ai_providers
                     SET name = ?1, kind = ?2, base_url = ?3, model = ?4, api_key_enc = ?5,
                         temperature = ?6, is_default = ?7, max_tokens = ?8,
                         timeout_secs = ?9, enabled = 1, updated_at = ?10
                     WHERE id = ?11",
                    params![
                        provider.name.trim(),
                        kind,
                        provider.base_url.trim(),
                        provider.model.trim(),
                        key_enc,
                        temperature,
                        provider.is_default as i64,
                        max_tokens,
                        timeout_secs,
                        now,
                        id
                    ],
                )
                .map_err(err)?;
            }
            id
        }
        None => {
            conn.execute(
                "INSERT INTO ai_providers
                   (name, kind, base_url, model, api_key_enc, temperature, max_tokens,
                    timeout_secs, is_default, enabled, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 1, ?10, ?10)",
                params![
                    provider.name.trim(),
                    kind,
                    provider.base_url.trim(),
                    provider.model.trim(),
                    key_enc,
                    temperature,
                    max_tokens,
                    timeout_secs,
                    provider.is_default as i64,
                    now
                ],
            )
            .map_err(err)?;
            conn.last_insert_rowid()
        }
    };

    let plain = if provider.api_key.is_empty() {
        // 沿用原 Key 时读回掩码，避免返回明文
        conn.query_row(
            "SELECT COALESCE(api_key_enc, '') FROM ai_providers WHERE id = ?1",
            params![id],
            |row| row.get::<_, String>(0),
        )
        .map(|stored| decrypt_key(&stored))
        .unwrap_or_default()
    } else {
        provider.api_key.clone()
    };

    Ok(AiProvider {
        id: Some(id),
        name: provider.name.trim().to_string(),
        kind,
        base_url: provider.base_url.trim().to_string(),
        model: provider.model.trim().to_string(),
        api_key: String::new(),
        api_key_masked: mask_key(&plain),
        has_key: !plain.is_empty(),
        temperature: Some(temperature),
        max_tokens: Some(max_tokens),
        timeout_secs: Some(timeout_secs),
        is_default: provider.is_default,
    })
}

#[tauri::command]
pub fn ai_provider_delete(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    let conn = state.pool.get().map_err(err)?;
    conn.execute("DELETE FROM ai_providers WHERE id = ?1", params![id])
        .map_err(err)?;
    Ok(())
}

fn load_provider(state: &AppState, id: i64) -> Result<AiProvider, String> {
    let conn = state.pool.get().map_err(err)?;
    let sql = format!(
        "SELECT {PROVIDER_COLUMNS} FROM ai_providers
         WHERE id = ?1 AND COALESCE(enabled, 1) = 1"
    );
    conn.query_row(&sql, params![id], row_to_provider_plain)
        .map_err(|_| "AI 服务不存在或已停用，请先配置".to_string())
}

/// 三种协议统一映射成「URL + 头 + JSON Body」
struct Prepared {
    url: String,
    headers: Vec<(String, String)>,
    body: serde_json::Value,
}

fn default_base(kind: &str) -> &'static str {
    match kind {
        "anthropic" => "https://api.anthropic.com",
        "gemini" => "https://generativelanguage.googleapis.com",
        _ => "https://api.openai.com/v1",
    }
}

fn prepare(
    provider: &AiProvider,
    messages: &[AiMessage],
    temperature: f64,
    stream: bool,
) -> Result<Prepared, String> {
    let base = if provider.base_url.trim().is_empty() {
        default_base(&provider.kind).to_string()
    } else {
        provider
            .base_url
            .trim()
            .trim_end_matches('/')
            .to_string()
    };
    let key = provider.api_key.trim();

    match provider.kind.as_str() {
        "anthropic" => {
            let system: Vec<String> = messages
                .iter()
                .filter(|m| m.role == "system")
                .map(|m| m.content.clone())
                .collect();
            let chat: Vec<serde_json::Value> = messages
                .iter()
                .filter(|m| m.role != "system")
                .map(|m| {
                    serde_json::json!({
                        "role": if m.role == "assistant" { "assistant" } else { "user" },
                        "content": m.content,
                    })
                })
                .collect();
            let mut body = serde_json::json!({
                "model": provider.model,
                "max_tokens": provider.max_tokens.filter(|value| *value > 0).unwrap_or(4096),
                "temperature": temperature,
                "stream": stream,
                "messages": chat,
            });
            if !system.is_empty() {
                body["system"] = serde_json::Value::String(system.join("\n\n"));
            }
            Ok(Prepared {
                url: format!("{base}/v1/messages"),
                headers: vec![
                    ("x-api-key".to_string(), key.to_string()),
                    ("anthropic-version".to_string(), "2023-06-01".to_string()),
                ],
                body,
            })
        }
        "gemini" => {
            let system: Vec<String> = messages
                .iter()
                .filter(|m| m.role == "system")
                .map(|m| m.content.clone())
                .collect();
            let contents: Vec<serde_json::Value> = messages
                .iter()
                .filter(|m| m.role != "system")
                .map(|m| {
                    serde_json::json!({
                        "role": if m.role == "assistant" { "model" } else { "user" },
                        "parts": [{ "text": m.content }],
                    })
                })
                .collect();
            let mut body = serde_json::json!({
                "contents": contents,
                "generationConfig": { "temperature": temperature },
            });
            if let Some(max) = provider.max_tokens.filter(|value| *value > 0) {
                body["generationConfig"]["maxOutputTokens"] = serde_json::json!(max);
            }
            if !system.is_empty() {
                body["systemInstruction"] = serde_json::json!({
                    "parts": [{ "text": system.join("\n\n") }]
                });
            }
            let action = if stream {
                "streamGenerateContent"
            } else {
                "generateContent"
            };
            let suffix = if stream { "?alt=sse" } else { "" };
            Ok(Prepared {
                url: format!(
                    "{base}/v1beta/models/{}:{action}{suffix}",
                    provider.model.trim()
                ),
                headers: vec![("x-goog-api-key".to_string(), key.to_string())],
                body,
            })
        }
        _ => {
            let chat: Vec<serde_json::Value> = messages
                .iter()
                .map(|m| {
                    let role = match m.role.as_str() {
                        "system" => "system",
                        "assistant" => "assistant",
                        _ => "user",
                    };
                    serde_json::json!({ "role": role, "content": m.content })
                })
                .collect();
            let mut body = serde_json::json!({
                "model": provider.model,
                "temperature": temperature,
                "stream": stream,
                "messages": chat,
            });
            // 不配置时不传，交给服务商默认值，避免部分网关拒绝未知字段
            if let Some(max) = provider.max_tokens.filter(|value| *value > 0) {
                body["max_tokens"] = serde_json::json!(max);
            }
            Ok(Prepared {
                url: format!("{base}/chat/completions"),
                headers: vec![("Authorization".to_string(), format!("Bearer {key}"))],
                body,
            })
        }
    }
}

/// 从错误响应体里尽量捞出可读信息
fn extract_api_error(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return "服务无响应内容".to_string();
    }
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(trimmed) {
        if let Some(message) = json
            .get("error")
            .and_then(|e| e.get("message").or_else(|| e.get("msg")))
            .and_then(|m| m.as_str())
        {
            return message.to_string();
        }
        if let Some(message) = json.get("message").and_then(|m| m.as_str()) {
            return message.to_string();
        }
    }
    trimmed.chars().take(300).collect()
}

/// 常见服务商错误转成中文提示；识别不了的保留原文，避免吞掉有效信息
fn humanize_api_error(status: u16, message: &str) -> String {
    let lower = message.to_ascii_lowercase();

    // 模型名不被支持：The supported API model names are A, B, but you passed C.
    if let Some(pos) = lower.find("supported api model names are") {
        let head = "supported api model names are";
        let rest = &message[pos + head.len()..];
        let rest_lower = rest.to_ascii_lowercase();
        let (supported, passed) = match rest_lower.find("but you passed") {
            Some(split) => (
                rest[..split].trim().trim_end_matches(',').trim().to_string(),
                rest[split + "but you passed".len()..]
                    .trim()
                    .trim_end_matches('.')
                    .trim()
                    .to_string(),
            ),
            None => (rest.trim().trim_end_matches('.').to_string(), String::new()),
        };
        let mut text = if passed.is_empty() {
            "当前服务商不支持所选模型".to_string()
        } else {
            format!("当前服务商不支持模型「{passed}」")
        };
        if !supported.is_empty() {
            text.push_str(&format!("。可用模型：{supported}"));
        }
        text.push_str("。请到「模型服务配置」里从下拉列表重新选择模型。");
        return text;
    }

    if status == 401
        || lower.contains("invalid api key")
        || lower.contains("incorrect api key")
        || lower.contains("invalid_api_key")
        || lower.contains("unauthorized")
    {
        return "API Key 无效或已过期，请到「模型服务配置」重新填写密钥。".to_string();
    }
    if status == 402
        || lower.contains("insufficient balance")
        || lower.contains("insufficient_quota")
        || lower.contains("insufficient quota")
    {
        return "账户余额或额度不足，请到服务商控制台充值后再试。".to_string();
    }
    if status == 403 {
        return "没有访问权限：当前密钥无权调用该模型，或所在地区受限。".to_string();
    }
    if status == 404 {
        return "接口地址或模型不存在，请检查 Base URL 与模型名是否正确。".to_string();
    }
    if status == 429 || lower.contains("rate limit") {
        return "请求过于频繁或已超出配额，请稍后再试。".to_string();
    }
    if lower.contains("model") {
        let missing = lower.contains("not exist")
            || lower.contains("not found")
            || lower.contains("does not exist")
            || lower.contains("unsupported model")
            || lower.contains("unknown model");
        if missing {
            return "模型不存在或已下线，请从「模型服务配置」的模型下拉列表里选择服务商在售的模型。"
                .to_string();
        }
    }
    if lower.contains("context length")
        || lower.contains("maximum context")
        || lower.contains("context_length_exceeded")
        || lower.contains("too many tokens")
    {
        return "内容超出模型上下文长度上限，请精简问题、减少附件，或改用上下文更大的模型。".to_string();
    }
    if status >= 500 {
        return format!("服务商暂时不可用（HTTP {status}），请稍后重试。");
    }
    if status == 400 {
        return format!("请求参数被服务商拒绝：{message}");
    }
    message.to_string()
}

/// 网络层错误转中文，保留原文便于排查代理与本地服务
fn humanize_network_error(raw: &str) -> String {
    let lower = raw.to_ascii_lowercase();
    if lower.contains("dns") || lower.contains("failed to lookup address") {
        return format!("无法解析服务商域名，请检查网络或 Base URL 是否写错。（原始错误：{raw}）");
    }
    if lower.contains("connection refused") || lower.contains("connection reset") {
        return format!(
            "连接被拒绝，请确认地址与端口正确；本地模型需先启动服务。（原始错误：{raw}）"
        );
    }
    if lower.contains("timed out") || lower.contains("timeout") {
        return format!("连接超时，请检查网络、代理或服务商状态。（原始错误：{raw}）");
    }
    if lower.contains("certificate") || lower.contains("tls") {
        return format!("HTTPS 证书校验失败，请检查代理或系统证书。（原始错误：{raw}）");
    }
    format!("连接 AI 服务失败：{raw}")
}

/// 把非流式响应解析成纯文本
fn parse_full_text(kind: &str, raw: &str) -> Result<String, String> {
    let json: serde_json::Value = serde_json::from_str(raw).map_err(|_| extract_api_error(raw))?;
    let collect_text = |items: Option<&serde_json::Value>| -> String {
        items
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|item| item.get("text").and_then(|t| t.as_str()))
                    .collect::<Vec<_>>()
                    .join("")
            })
            .unwrap_or_default()
    };
    let text = match kind {
        "anthropic" => collect_text(json.get("content")),
        "gemini" => collect_text(
            json.get("candidates")
                .and_then(|c| c.get(0))
                .and_then(|c| c.get("content"))
                .and_then(|c| c.get("parts")),
        ),
        _ => json
            .get("choices")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("message"))
            .and_then(|m| m.get("content"))
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
    };
    if text.is_empty() {
        return Err(extract_api_error(raw));
    }
    Ok(text)
}

#[derive(Debug, PartialEq)]
enum SseOutcome {
    Text(String),
    Reasoning(String),
    Usage { prompt: i64, completion: i64 },
    Done,
    Ignore,
}

fn num(value: Option<&serde_json::Value>) -> i64 {
    value.and_then(|v| v.as_i64()).unwrap_or(0)
}

/// 从各协议的 JSON 中取 token 用量（没有就返回 None）
fn usage_of(kind: &str, json: &serde_json::Value) -> Option<(i64, i64)> {
    let pair = |prompt: Option<&serde_json::Value>, completion: Option<&serde_json::Value>| {
        let prompt = num(prompt);
        let completion = num(completion);
        if prompt == 0 && completion == 0 {
            None
        } else {
            Some((prompt, completion))
        }
    };
    match kind {
        "anthropic" => match json.get("type").and_then(|t| t.as_str()).unwrap_or("") {
            "message_start" => pair(
                json.get("message").and_then(|m| m.get("usage")).and_then(|u| u.get("input_tokens")),
                json.get("message").and_then(|m| m.get("usage")).and_then(|u| u.get("output_tokens")),
            ),
            "message_delta" => pair(
                None,
                json.get("usage").and_then(|u| u.get("output_tokens")),
            ),
            _ => None,
        },
        "gemini" => json.get("usageMetadata").and_then(|u| {
            pair(u.get("promptTokenCount"), u.get("candidatesTokenCount"))
        }),
        _ => json.get("usage").and_then(|u| {
            pair(
                u.get("prompt_tokens").or_else(|| u.get("input_tokens")),
                u.get("completion_tokens").or_else(|| u.get("output_tokens")),
            )
        }),
    }
}

/// 粗略估算 token：中日韩字符约 1 token/字，其余按 4 字符 1 token
fn estimate_tokens(text: &str) -> i64 {
    let mut cjk = 0i64;
    let mut other = 0i64;
    for ch in text.chars() {
        if ('\u{4e00}'..='\u{9fff}').contains(&ch)
            || ('\u{3000}'..='\u{30ff}').contains(&ch)
            || ('\u{ac00}'..='\u{d7af}').contains(&ch)
        {
            cjk += 1;
        } else {
            other += 1;
        }
    }
    cjk + (other + 3) / 4
}

fn parse_sse(kind: &str, line: &str) -> Result<SseOutcome, String> {
    let Some(payload) = line.strip_prefix("data:") else {
        return Ok(SseOutcome::Ignore);
    };
    let payload = payload.trim();
    if payload.is_empty() {
        return Ok(SseOutcome::Ignore);
    }
    if payload == "[DONE]" {
        return Ok(SseOutcome::Done);
    }
    let json: serde_json::Value = match serde_json::from_str(payload) {
        Ok(value) => value,
        Err(_) => return Ok(SseOutcome::Ignore),
    };

    if let Some((prompt, completion)) = usage_of(kind, &json) {
        return Ok(SseOutcome::Usage { prompt, completion });
    }

    match kind {
        "anthropic" => match json.get("type").and_then(|t| t.as_str()).unwrap_or("") {
            "content_block_delta" => {
                let delta = json.get("delta");
                let thinking = delta
                    .and_then(|d| d.get("thinking"))
                    .and_then(|t| t.as_str())
                    .unwrap_or_default();
                if !thinking.is_empty() {
                    return Ok(SseOutcome::Reasoning(thinking.to_string()));
                }
                let text = delta
                    .and_then(|d| d.get("text"))
                    .and_then(|t| t.as_str())
                    .unwrap_or_default();
                Ok(SseOutcome::Text(text.to_string()))
            }
            "message_stop" => Ok(SseOutcome::Done),
            "error" => Err(extract_api_error(payload)),
            _ => Ok(SseOutcome::Ignore),
        },
        "gemini" => {
            if let Some(reason) = json
                .get("promptFeedback")
                .and_then(|p| p.get("blockReason"))
                .and_then(|r| r.as_str())
            {
                return Err(format!("请求被 Gemini 拦截：{reason}"));
            }
            let parts = json
                .get("candidates")
                .and_then(|c| c.get(0))
                .and_then(|c| c.get("content"))
                .and_then(|c| c.get("parts"))
                .and_then(|p| p.as_array());
            let collect = |thought: bool| {
                parts
                    .map(|parts| {
                        parts
                            .iter()
                            .filter(|p| (p.get("thought").and_then(|t| t.as_bool()) == Some(true)) == thought)
                            .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
                            .collect::<Vec<_>>()
                            .join("")
                    })
                    .unwrap_or_default()
            };
            let thinking = collect(true);
            if !thinking.is_empty() {
                return Ok(SseOutcome::Reasoning(thinking));
            }
            let text = collect(false);
            if text.is_empty() {
                Ok(SseOutcome::Ignore)
            } else {
                Ok(SseOutcome::Text(text))
            }
        }
        _ => {
            let Some(choice) = json.get("choices").and_then(|c| c.get(0)) else {
                return Ok(SseOutcome::Ignore);
            };
            let delta = choice.get("delta");
            let reasoning = delta
                .and_then(|d| d.get("reasoning_content"))
                .and_then(|c| c.as_str())
                .unwrap_or_default();
            if !reasoning.is_empty() {
                return Ok(SseOutcome::Reasoning(reasoning.to_string()));
            }
            let text = delta
                .and_then(|d| d.get("content"))
                .and_then(|c| c.as_str())
                .unwrap_or_default();
            if text.is_empty() {
                Ok(SseOutcome::Ignore)
            } else {
                Ok(SseOutcome::Text(text.to_string()))
            }
        }
    }
}

fn emit(channel: &Channel<AiStreamEvent>, kind: &str, text: impl Into<String>) {
    let _ = channel.send(AiStreamEvent {
        kind: kind.to_string(),
        text: text.into(),
    });
}

/// 记录一次模型调用的 token 与成败，供「模型统计」使用
#[allow(clippy::too_many_arguments)]
fn record_usage(
    state: &AppState,
    provider: &AiProvider,
    prompt_tokens: i64,
    completion_tokens: i64,
    ok: bool,
    latency_ms: i64,
    error: Option<&str>,
    request_preview: &str,
) {
    let Ok(conn) = state.pool.get() else {
        return;
    };
    let _ = conn.execute(
        "DELETE FROM ai_usage_logs WHERE created_at < ?1",
        params![now_ms() - USAGE_RETENTION_DAYS * DAY_MS],
    );
    let _ = conn.execute(
        "INSERT INTO ai_usage_logs (provider_id, provider_name, model, prompt_tokens,
            completion_tokens, ok, latency_ms, error, request_preview, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            provider.id,
            provider.name,
            provider.model,
            prompt_tokens,
            completion_tokens,
            ok as i64,
            latency_ms,
            error.map(|text| text.chars().take(500).collect::<String>()),
            request_preview,
            now_ms(),
        ],
    );
}

fn clip_chars(text: &str, limit: usize) -> String {
    let total = text.chars().count();
    let mut out: String = text.chars().take(limit).collect();
    if total > limit {
        out.push_str("…（已截断）");
    }
    out
}

/// 记录这轮到底问了模型什么：默认取本轮用户消息；
/// 若本轮是工具结果回灌，则同时带上最初的问题，避免只看得到「【工具 X 的结果】」。
fn request_preview(messages: &[AiMessage]) -> String {
    let users: Vec<&AiMessage> = messages.iter().filter(|item| item.role == "user").collect();
    let Some(last) = users.last() else {
        return String::new();
    };
    let last_text = last.content.trim();
    if users.len() <= 1 {
        return clip_chars(last_text, 2000);
    }
    let first_text = users[0].content.trim();
    if first_text == last_text {
        return clip_chars(last_text, 2000);
    }
    format!(
        "首问：{}\n—— 本轮：{}",
        clip_chars(first_text, 400),
        clip_chars(last_text, 1500)
    )
}

/// 请求结束时释放取消登记（无论成功 / 失败 / 被停止）
struct AiCancelGuard<'a> {
    state: &'a AppState,
    id: Option<&'a str>,
}

impl Drop for AiCancelGuard<'_> {
    fn drop(&mut self) {
        if let Some(id) = self.id {
            self.state.ai_release(id);
        }
    }
}

/// 停止在途生成：命中返回 true，未命中（已结束）返回 false
#[tauri::command]
pub fn ai_chat_cancel(state: State<'_, AppState>, request_id: String) -> bool {
    state.ai_request_cancel(&request_id)
}

/// 429 响应里的 Retry-After（秒），解析失败退回 None
fn retry_after_secs(headers: &reqwest::header::HeaderMap) -> Option<u64> {
    headers
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map(|value| value.min(30))
}

#[tauri::command]
pub async fn ai_chat(
    state: State<'_, AppState>,
    payload: AiChatPayload,
    on_event: Channel<AiStreamEvent>,
) -> Result<(), String> {
    if payload.messages.is_empty() {
        return Err("消息不能为空".to_string());
    }
    let provider = load_provider(state.inner(), payload.provider_id)?;
    let temperature = payload
        .temperature
        .or(provider.temperature)
        .unwrap_or(0.3)
        .clamp(0.0, 2.0);
    let timeout_secs = provider
        .timeout_secs
        .unwrap_or(AI_TIMEOUT_SECS as i64)
        .clamp(10, 3600) as u64;

    // 停止生成：requestId 登记后，前端可随时调用 ai_chat_cancel 置位
    let request_id = payload.request_id.clone();
    let cancel = request_id
        .as_deref()
        .map(|id| state.ai_register(id));
    let is_cancelled = || -> bool {
        cancel
            .as_ref()
            .map(|flag| flag.load(Ordering::SeqCst))
            .unwrap_or(false)
    };
    let _release = AiCancelGuard {
        state: state.inner(),
        id: request_id.as_deref(),
    };

    // 小HI 的系统提示词由后端统一注入（编译期嵌入），客户端无法覆盖或移除；
    // 前端追加的 system 消息只承载工具协议，排在其后。
    let mut messages = payload.messages.clone();
    messages.retain(|message| message.role != "system" || message.content.trim() != XIAO_HI_SYSTEM_PROMPT.trim());
    messages.insert(
        0,
        AiMessage {
            role: "system".to_string(),
            content: XIAO_HI_SYSTEM_PROMPT.to_string(),
        },
    );
    // 本次调用实际「问了什么」，随用量流水一起落库
    let preview = request_preview(&messages);

    let prepared = prepare(&provider, &messages, temperature, true)?;

    // 本地估算兜底：部分网关不回 usage
    let prompt_chars: i64 = messages
        .iter()
        .map(|message| estimate_tokens(&message.content))
        .sum();
    let started = std::time::Instant::now();
    let mut prompt_tokens = 0i64;
    let mut completion_tokens = 0i64;
    let mut reply = String::new();

    let mut request = state
        .client(false)
        .post(&prepared.url)
        .timeout(Duration::from_secs(timeout_secs))
        .header("Content-Type", "application/json")
        .json(&prepared.body);
    for (key, value) in &prepared.headers {
        request = request.header(key, value);
    }

    if is_cancelled() {
        emit(&on_event, "cancelled", "");
        return Ok(());
    }

    // 首 delta 之前允许自动重试（429 / 5xx / 网络错误），最多 2 次；
    // 一旦给用户吐出内容就不再静默重试，否则会出现半句话后整段重来
    let mut attempt = 0u32;
    let response = loop {
        attempt += 1;
        let attempt_request = request
            .try_clone()
            .ok_or_else(|| "请求体无法复用，已停止重试".to_string())?;
        match attempt_request.send().await {
            Ok(response) => {
                let status = response.status();
                let retryable = status.as_u16() == 429 || status.is_server_error();
                if retryable && attempt <= 2 {
                    let wait = retry_after_secs(response.headers())
                        .unwrap_or(if attempt == 1 { 1 } else { 3 });
                    emit(
                        &on_event,
                        "retry",
                        format!(
                            "服务{}，{wait} 秒后重试（{attempt}/2）",
                            if status.as_u16() == 429 { "限流" } else { "繁忙" }
                        ),
                    );
                    tokio::time::sleep(Duration::from_secs(wait)).await;
                    if is_cancelled() {
                        emit(&on_event, "cancelled", "");
                        return Ok(());
                    }
                    continue;
                }
                break response;
            }
            Err(error) => {
                if attempt <= 2 {
                    let wait = if attempt == 1 { 1 } else { 3 };
                    emit(
                        &on_event,
                        "retry",
                        format!("网络异常，{wait} 秒后重试（{attempt}/2）"),
                    );
                    tokio::time::sleep(Duration::from_secs(wait)).await;
                    if is_cancelled() {
                        emit(&on_event, "cancelled", "");
                        return Ok(());
                    }
                    continue;
                }
                let text = humanize_network_error(&error.to_string());
                emit(&on_event, "error", text.clone());
                record_usage(
                    state.inner(),
                    &provider,
                    prompt_chars,
                    0,
                    false,
                    started.elapsed().as_millis() as i64,
                    Some(&text),
                    &preview,
                );
                return Err(text);
            }
        }
    };

    let status = response.status();
    if !status.is_success() {
        let raw = response.text().await.unwrap_or_default();
        let text = format!(
            "AI 服务返回 {}：{}",
            status.as_u16(),
            humanize_api_error(status.as_u16(), &extract_api_error(&raw))
        );
        emit(&on_event, "error", text.clone());
        record_usage(
            state.inner(),
            &provider,
            prompt_chars,
            0,
            false,
            started.elapsed().as_millis() as i64,
            Some(&text),
            &preview,
        );
        return Err(text);
    }

    let is_sse = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.contains("text/event-stream"))
        .unwrap_or(false);

    if !is_sse {
        // 部分代理会忽略 stream 参数，直接整包返回 JSON
        let raw = response.text().await.map_err(err)?;
        if is_cancelled() {
            emit(&on_event, "cancelled", "");
            return Ok(());
        }
        let (prompt, completion) = usage_of(&provider.kind, &serde_json::from_str(&raw).unwrap_or_default())
            .unwrap_or((prompt_chars, 0));
        return match parse_full_text(&provider.kind, &raw) {
            Ok(text) => {
                emit(&on_event, "delta", &text);
                emit(&on_event, "done", "");
                let completion = if completion > 0 { completion } else { estimate_tokens(&text) };
                record_usage(
                    state.inner(),
                    &provider,
                    prompt,
                    completion,
                    true,
                    started.elapsed().as_millis() as i64,
                    None,
                    &preview,
                );
                Ok(())
            }
            Err(text) => {
                emit(&on_event, "error", text.clone());
                record_usage(
                    state.inner(),
                    &provider,
                    prompt_chars,
                    0,
                    false,
                    started.elapsed().as_millis() as i64,
                    Some(&text),
                    &preview,
                );
                Err(text)
            }
        };
    }

    let mut stream = response.bytes_stream();
    let mut buffer: Vec<u8> = Vec::new();
    // 成功路径统一收尾：补齐估算值并落库
    macro_rules! finish {
        ($ok:expr, $err:expr) => {{
            if prompt_tokens == 0 {
                prompt_tokens = prompt_chars;
            }
            if completion_tokens == 0 {
                completion_tokens = estimate_tokens(&reply);
            }
            record_usage(
                state.inner(),
                &provider,
                prompt_tokens,
                completion_tokens,
                $ok,
                started.elapsed().as_millis() as i64,
                $err,
                &preview,
            );
        }};
    }
    while let Some(chunk) = stream.next().await {
        // 用户点「停止」：立即停流并释放，后台不再继续消耗 token
        if is_cancelled() {
            emit(&on_event, "cancelled", "");
            finish!(true, None);
            return Ok(());
        }
        let chunk = match chunk {
            Ok(chunk) => chunk,
            Err(error) => {
                let text = format!("读取流式响应失败：{error}");
                emit(&on_event, "error", text.clone());
                finish!(false, Some(text.as_str()));
                return Err(text);
            }
        };
        buffer.extend_from_slice(&chunk);
        while let Some(pos) = buffer.iter().position(|byte| *byte == b'\n') {
            let line: Vec<u8> = buffer.drain(..=pos).collect();
            let line = String::from_utf8_lossy(&line[..line.len() - 1]);
            let line = line.trim_end_matches('\r').trim();
            if line.is_empty() {
                continue;
            }
            match parse_sse(&provider.kind, line) {
                Ok(SseOutcome::Text(text)) => {
                    if !text.is_empty() {
                        reply.push_str(&text);
                        emit(&on_event, "delta", text);
                    }
                }
                Ok(SseOutcome::Reasoning(text)) => {
                    if !text.is_empty() {
                        reply.push_str(&text);
                        emit(&on_event, "reasoning", text);
                    }
                }
                Ok(SseOutcome::Usage { prompt, completion }) => {
                    if prompt > 0 {
                        prompt_tokens = prompt;
                    }
                    if completion > 0 {
                        completion_tokens = completion;
                    }
                }
                Ok(SseOutcome::Done) => {
                    emit(&on_event, "done", "");
                    finish!(true, None);
                    return Ok(());
                }
                Ok(SseOutcome::Ignore) => {}
                Err(text) => {
                    emit(&on_event, "error", text.clone());
                    finish!(false, Some(text.as_str()));
                    return Err(text);
                }
            }
        }
    }
    emit(&on_event, "done", "");
    finish!(true, None);
    Ok(())
}

/// 测试连通性：发一条极短的问候，返回模型原文
#[tauri::command]
pub async fn ai_provider_test(state: State<'_, AppState>, id: i64) -> Result<String, String> {
    let provider = load_provider(state.inner(), id)?;
    let messages = vec![AiMessage {
        role: "user".to_string(),
        content: "只回复两个字：可用".to_string(),
    }];
    let prepared = prepare(&provider, &messages, 0.0, false)?;

    let mut request = state
        .client(false)
        .post(&prepared.url)
        .timeout(Duration::from_secs(60))
        .header("Content-Type", "application/json")
        .json(&prepared.body);
    for (key, value) in &prepared.headers {
        request = request.header(key, value);
    }
    let response = request
        .send()
        .await
        .map_err(|error| format!("连接失败：{error}"))?;
    let status = response.status();
    let raw = response.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(format!("返回 {}：{}", status.as_u16(), extract_api_error(&raw)));
    }
    let text = parse_full_text(&provider.kind, &raw)?;
    let text = text.trim();
    if text.is_empty() {
        Ok("服务已连通".to_string())
    } else {
        Ok(text.chars().take(60).collect())
    }
}

/// 拉取服务商实际可用的模型列表（前端缓存 24 小时，失败回退内置清单）
#[tauri::command]
pub async fn ai_models_list(
    state: State<'_, AppState>,
    provider_id: i64,
) -> Result<Vec<String>, String> {
    let provider = load_provider(state.inner(), provider_id)?;
    let base = if provider.base_url.trim().is_empty() {
        default_base(&provider.kind).to_string()
    } else {
        provider
            .base_url
            .trim()
            .trim_end_matches('/')
            .to_string()
    };
    let key = provider.api_key.trim();
    let client = state.client(false);
    let (url, request) = match provider.kind.as_str() {
        "anthropic" => (
            format!("{base}/v1/models"),
            client
                .get(format!("{base}/v1/models"))
                .header("x-api-key", key)
                .header("anthropic-version", "2023-06-01"),
        ),
        "gemini" => (
            format!("{base}/v1beta/models"),
            client.get(format!("{base}/v1beta/models")).header("x-goog-api-key", key),
        ),
        _ => (
            format!("{base}/models"),
            client
                .get(format!("{base}/models"))
                .header("Authorization", format!("Bearer {key}")),
        ),
    };
    let response = request
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .map_err(|error| humanize_network_error(&error.to_string()))?;
    let status = response.status();
    let raw = response.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(format!(
            "拉取模型失败（{}）：{}",
            status.as_u16(),
            humanize_api_error(status.as_u16(), &extract_api_error(&raw))
        ));
    }
    let json: serde_json::Value = serde_json::from_str(&raw)
        .map_err(|_| format!("{url} 返回的不是合法 JSON，无法解析模型列表"))?;
    let mut models: Vec<String> = Vec::new();
    if provider.kind == "gemini" {
        if let Some(list) = json["models"].as_array() {
            for item in list {
                if let Some(name) = item["name"].as_str() {
                    models.push(name.trim_start_matches("models/").to_string());
                }
            }
        }
    } else if let Some(list) = json["data"].as_array() {
        for item in list {
            if let Some(id) = item["id"].as_str() {
                models.push(id.to_string());
            }
        }
    }
    models.sort();
    models.dedup();
    if models.is_empty() {
        return Err("服务商未返回可用模型列表".to_string());
    }
    Ok(models)
}

/// AI 生成的请求执行入口：与手动发送共用同一实现（历史、Cookie、快照一致）
#[tauri::command]
pub async fn ai_execute(
    state: State<'_, AppState>,
    payload: SendRequestPayload,
) -> Result<HttpResult, String> {
    crate::commands::http::execute(state.inner(), payload).await
}

/* ---------------- 会话记录 ---------------- */

fn truncate_title(text: &str) -> String {
    let cleaned: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let title: String = cleaned.chars().take(24).collect();
    if cleaned.chars().count() > 24 {
        format!("{title}…")
    } else if title.is_empty() {
        "新对话".to_string()
    } else {
        title
    }
}

#[tauri::command]
pub fn ai_session_list(
    state: State<'_, AppState>,
    keyword: Option<String>,
) -> Result<Vec<AiSession>, String> {
    let conn = state.pool.get().map_err(err)?;
    let keyword = keyword.unwrap_or_default().trim().to_string();
    let pattern = format!("%{keyword}%");
    let mut stmt = conn
        .prepare(
            "SELECT s.id, s.title, s.provider_id,
                    (SELECT COUNT(*) FROM ai_messages m WHERE m.session_id = s.id),
                    COALESCE(s.pinned, 0), s.summary, COALESCE(s.summary_until, 0),
                    s.created_at, s.updated_at
             FROM ai_sessions s
             WHERE ?1 = ''
                OR s.title LIKE ?2
                OR EXISTS (
                    SELECT 1 FROM ai_messages m
                    WHERE m.session_id = s.id AND m.content LIKE ?2
                )
             ORDER BY COALESCE(s.pinned, 0) DESC, s.updated_at DESC, s.id DESC
             LIMIT 200",
        )
        .map_err(err)?;
    let rows = stmt
        .query_map(params![keyword, pattern], |row| {
            Ok(AiSession {
                id: row.get(0)?,
                title: row.get(1)?,
                provider_id: row.get(2)?,
                message_count: row.get(3)?,
                pinned: row.get::<_, i64>(4)? != 0,
                summary: row.get(5)?,
                summary_until: row.get(6)?,
                created_at: row.get(7)?,
                updated_at: row.get(8)?,
            })
        })
        .map_err(err)?;
    Ok(rows.filter_map(Result::ok).collect())
}

/// 置顶 / 取消置顶：会话多了以后按重要程度排序
#[tauri::command]
pub fn ai_session_pin(state: State<'_, AppState>, id: i64, pinned: bool) -> Result<(), String> {
    let conn = state.pool.get().map_err(err)?;
    conn.execute(
        "UPDATE ai_sessions SET pinned = ?1 WHERE id = ?2",
        params![pinned as i64, id],
    )
    .map_err(err)?;
    Ok(())
}

/// 保存历史摘要：从下一轮开始替代 summary_until 之前的原文
#[tauri::command]
pub fn ai_session_summary_save(
    state: State<'_, AppState>,
    id: i64,
    summary: String,
    summary_until: i64,
) -> Result<(), String> {
    let conn = state.pool.get().map_err(err)?;
    conn.execute(
        "UPDATE ai_sessions SET summary = ?1, summary_until = ?2 WHERE id = ?3",
        params![summary, summary_until, id],
    )
    .map_err(err)?;
    Ok(())
}

#[tauri::command]
pub fn ai_session_create(
    state: State<'_, AppState>,
    title: Option<String>,
    provider_id: Option<i64>,
) -> Result<AiSession, String> {
    let conn = state.pool.get().map_err(err)?;
    let now = now_ms();
    let title = title
        .map(|value| truncate_title(&value))
        .unwrap_or_else(|| "新对话".to_string());
    conn.execute(
        "INSERT INTO ai_sessions (title, provider_id, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?3)",
        params![title, provider_id, now],
    )
    .map_err(err)?;
    Ok(AiSession {
        id: conn.last_insert_rowid(),
        title,
        provider_id,
        message_count: 0,
        pinned: false,
        summary: None,
        summary_until: 0,
        created_at: now,
        updated_at: now,
    })
}

#[tauri::command]
pub fn ai_session_rename(
    state: State<'_, AppState>,
    id: i64,
    title: String,
) -> Result<(), String> {
    if title.trim().is_empty() {
        return Err("标题不能为空".to_string());
    }
    let conn = state.pool.get().map_err(err)?;
    conn.execute(
        "UPDATE ai_sessions SET title = ?1, updated_at = ?2 WHERE id = ?3",
        params![truncate_title(&title), now_ms(), id],
    )
    .map_err(err)?;
    Ok(())
}

#[tauri::command]
pub fn ai_session_delete(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    let mut conn = state.pool.get().map_err(err)?;
    let tx = conn.transaction().map_err(err)?;
    tx.execute("DELETE FROM ai_messages WHERE session_id = ?1", params![id])
        .map_err(err)?;
    tx.execute("DELETE FROM ai_tool_logs WHERE session_id = ?1", params![id])
        .map_err(err)?;
    tx.execute("DELETE FROM ai_sessions WHERE id = ?1", params![id])
        .map_err(err)?;
    tx.commit().map_err(err)?;
    Ok(())
}

#[tauri::command]
pub fn ai_message_list(
    state: State<'_, AppState>,
    session_id: i64,
) -> Result<Vec<AiStoredMessage>, String> {
    let conn = state.pool.get().map_err(err)?;
    let mut stmt = conn
        .prepare(
            "SELECT id, session_id, role, content, reasoning, created_at
             FROM ai_messages WHERE session_id = ?1 ORDER BY id ASC",
        )
        .map_err(err)?;
    let rows = stmt
        .query_map(params![session_id], |row| {
            Ok(AiStoredMessage {
                id: row.get(0)?,
                session_id: row.get(1)?,
                role: row.get(2)?,
                content: row.get(3)?,
                reasoning: row.get(4)?,
                created_at: row.get(5)?,
            })
        })
        .map_err(err)?;
    Ok(rows.filter_map(Result::ok).collect())
}

/// 会话落库上限：只保留关键运行信息，超长上下文不进数据库，避免库无限膨胀
const MESSAGE_MAX_CHARS: usize = 8000;
const REASONING_MAX_CHARS: usize = 2000;

/// 截断到上限，并标注原长度，便于判断是否被裁剪
fn clip_trace(text: &str, max: usize) -> String {
    let total = text.chars().count();
    if total <= max {
        return text.to_string();
    }
    let kept: String = text.chars().take(max).collect();
    format!("{kept}\n…（已截断，原长度 {total} 字）")
}

/// 把 ```fbin-file 名称\n正文\n``` 的附件正文换成摘要：
/// 正文只用于本轮推理，落库时仅保留文件名与字符数，避免大文件撑大数据库
fn strip_file_blocks(text: &str) -> String {
    const OPEN: &str = "```fbin-file ";
    let mut out = String::with_capacity(text.len().min(9000));
    let mut rest = text;
    while let Some(start) = rest.find(OPEN) {
        let (head, tail) = rest.split_at(start);
        out.push_str(head);
        let after = &tail[OPEN.len()..];
        let Some(name_end) = after.find('\n') else {
            out.push_str(tail);
            return out;
        };
        let name = &after[..name_end];
        let body_part = &after[name_end + 1..];
        let Some(end) = body_part.find("\n```") else {
            out.push_str(tail);
            return out;
        };
        let chars = body_part[..end].chars().count();
        out.push_str(&format!("【附件 {name} · {chars} 字符，正文未入库】"));
        rest = &body_part[end + 4..];
    }
    out.push_str(rest);
    out
}

/// 追加一条消息；会话标题为空时用首条用户消息自动命名
#[tauri::command]
pub fn ai_message_add(
    state: State<'_, AppState>,
    session_id: i64,
    role: String,
    content: String,
    reasoning: Option<String>,
) -> Result<AiStoredMessage, String> {
    let conn = state.pool.get().map_err(err)?;
    let now = now_ms();
    let content = clip_trace(&strip_file_blocks(&content), MESSAGE_MAX_CHARS);
    let reasoning = reasoning.map(|text| clip_trace(&text, REASONING_MAX_CHARS));
    let token_count = estimate_tokens(&content);
    conn.execute(
        "INSERT INTO ai_messages (session_id, role, content, reasoning, token_count, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![session_id, role, content, reasoning, token_count, now],
    )
    .map_err(err)?;
    let id = conn.last_insert_rowid();

    let current: String = conn
        .query_row(
            "SELECT title FROM ai_sessions WHERE id = ?1",
            params![session_id],
            |row| row.get(0),
        )
        .unwrap_or_else(|_| "新对话".to_string());
    if role == "user" && current == "新对话" {
        conn.execute(
            "UPDATE ai_sessions SET title = ?1, updated_at = ?2 WHERE id = ?3",
            params![truncate_title(&content), now, session_id],
        )
        .map_err(err)?;
    } else {
        conn.execute(
            "UPDATE ai_sessions SET updated_at = ?1 WHERE id = ?2",
            params![now, session_id],
        )
        .map_err(err)?;
    }

    Ok(AiStoredMessage {
        id,
        session_id,
        role,
        content,
        reasoning,
        created_at: now,
    })
}

#[tauri::command]
pub fn ai_message_clear(state: State<'_, AppState>, session_id: i64) -> Result<(), String> {
    let conn = state.pool.get().map_err(err)?;
    conn.execute("DELETE FROM ai_messages WHERE session_id = ?1", params![session_id])
        .map_err(err)?;
    Ok(())
}

/// 编辑重发：删除指定时间起的消息（含被编辑的那条），避免重发后旧分支残留
#[tauri::command]
pub fn ai_message_delete_from(
    state: State<'_, AppState>,
    session_id: i64,
    created_at: i64,
    inclusive: Option<bool>,
) -> Result<(), String> {
    let conn = state.pool.get().map_err(err)?;
    let compare = if inclusive.unwrap_or(true) { ">=" } else { ">" };
    conn.execute(
        &format!(
            "DELETE FROM ai_messages WHERE session_id = ?1 AND created_at {compare} ?2"
        ),
        params![session_id, created_at],
    )
    .map_err(err)?;
    Ok(())
}

/// 删除单条消息（按数据库 id）：消息级「删除」只移除这一条，不动前后对话
#[tauri::command]
pub fn ai_message_delete(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    let conn = state.pool.get().map_err(err)?;
    conn.execute("DELETE FROM ai_messages WHERE id = ?1", params![id])
        .map_err(err)?;
    Ok(())
}

/* ---------------- 工具调用流水 ---------------- */

/// 工具流水保留上限：近 1 年
const TOOL_LOG_RETENTION_DAYS: i64 = 365;
/// 模型用量记录同样保留近 1 年
const USAGE_RETENTION_DAYS: i64 = 365;
const DAY_MS: i64 = 86_400_000;

/// 清理超出保留期的流水。只清理旧的，不会动用户近一年的记录。
fn purge_old_tool_logs(conn: &rusqlite::Connection) {
    let cutoff = now_ms() - TOOL_LOG_RETENTION_DAYS * DAY_MS;
    let _ = conn.execute(
        "DELETE FROM ai_tool_logs WHERE created_at < ?1",
        params![cutoff],
    );
}

#[tauri::command]
pub fn ai_tool_log_add(
    state: State<'_, AppState>,
    session_id: Option<i64>,
    tool: String,
    args: String,
    ok: bool,
    duration_ms: i64,
    result_preview: String,
    round: Option<i64>,
    trace_id: Option<String>,
    timeout: Option<bool>,
) -> Result<(), String> {
    let conn = state.pool.get().map_err(err)?;
    let args: String = args.chars().take(2000).collect();
    let preview: String = result_preview.chars().take(2000).collect();
    conn.execute(
        "INSERT INTO ai_tool_logs
           (session_id, tool, args, ok, duration_ms, result_preview, round, trace_id, timeout, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            session_id,
            tool,
            args,
            ok as i64,
            duration_ms,
            preview,
            round.unwrap_or(0),
            trace_id,
            timeout.unwrap_or(false) as i64,
            now_ms()
        ],
    )
    .map_err(err)?;
    purge_old_tool_logs(&conn);
    Ok(())
}

#[tauri::command]
pub fn ai_tool_log_list(
    state: State<'_, AppState>,
    session_id: Option<i64>,
    tool: Option<String>,
    ok: Option<bool>,
    days: Option<i64>,
    page: Option<i64>,
    page_size: Option<i64>,
) -> Result<AiToolLogPage, String> {
    let conn = state.pool.get().map_err(err)?;
    let page = page.unwrap_or(1).max(1);
    let page_size = page_size.unwrap_or(20).clamp(1, 200);
    let offset = (page - 1) * page_size;
    let tool = tool.unwrap_or_default();
    let ok_filter = ok.map(|value| value as i64);
    let days = days.unwrap_or(TOOL_LOG_RETENTION_DAYS).clamp(1, TOOL_LOG_RETENTION_DAYS);
    let since = now_ms() - days * DAY_MS;

    let total: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM ai_tool_logs
             WHERE (?1 IS NULL OR session_id = ?1)
               AND (?2 = '' OR tool = ?2)
               AND (?3 IS NULL OR ok = ?3)
               AND created_at >= ?4",
            params![session_id, tool, ok_filter, since],
            |row| row.get(0),
        )
        .map_err(err)?;

    let mut stmt = conn
        .prepare(
            "SELECT id, session_id, tool, COALESCE(args, ''), ok,
                    duration_ms, COALESCE(result_preview, ''), created_at,
                    COALESCE(round, 0), trace_id, COALESCE(timeout, 0)
             FROM ai_tool_logs
             WHERE (?1 IS NULL OR session_id = ?1)
               AND (?2 = '' OR tool = ?2)
               AND (?3 IS NULL OR ok = ?3)
               AND created_at >= ?4
             ORDER BY id DESC
             LIMIT ?5 OFFSET ?6",
        )
        .map_err(err)?;
    let rows = stmt
        .query_map(
            params![session_id, tool, ok_filter, since, page_size, offset],
            |row| {
                Ok(AiToolLog {
                    id: row.get(0)?,
                    session_id: row.get(1)?,
                    tool: row.get(2)?,
                    args: row.get(3)?,
                    ok: row.get::<_, i64>(4)? != 0,
                    duration_ms: row.get(5)?,
                    result_preview: row.get(6)?,
                    created_at: row.get(7)?,
                    round: row.get(8)?,
                    trace_id: row.get(9)?,
                    timeout: row.get::<_, i64>(10)? != 0,
                })
            },
        )
        .map_err(err)?;
    let items: Vec<AiToolLog> = rows.filter_map(Result::ok).collect();
    Ok(AiToolLogPage { items, total })
}

#[tauri::command]
pub fn ai_tool_log_stats(
    state: State<'_, AppState>,
    days: Option<i64>,
) -> Result<AiToolLogStats, String> {
    let conn = state.pool.get().map_err(err)?;
    let days = days.unwrap_or(30).clamp(1, TOOL_LOG_RETENTION_DAYS);
    let since = now_ms() - days * DAY_MS;

    let (total, ok_count, failed, avg_duration): (i64, i64, i64, i64) = conn
        .query_row(
            "SELECT COUNT(*),
                    COALESCE(SUM(CASE WHEN ok != 0 THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN ok = 0 THEN 1 ELSE 0 END), 0),
                    COALESCE(CAST(AVG(duration_ms) AS INTEGER), 0)
             FROM ai_tool_logs WHERE created_at >= ?1",
            params![since],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .map_err(err)?;

    let last_24h: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM ai_tool_logs WHERE created_at >= ?1",
            params![now_ms() - DAY_MS],
            |row| row.get(0),
        )
        .map_err(err)?;

    let mut day_stmt = conn
        .prepare(
            "SELECT strftime('%Y-%m-%d', created_at / 1000, 'unixepoch', 'localtime') AS day,
                    COUNT(*),
                    COALESCE(SUM(CASE WHEN ok = 0 THEN 1 ELSE 0 END), 0)
             FROM ai_tool_logs
             WHERE created_at >= ?1
             GROUP BY day
             ORDER BY day",
        )
        .map_err(err)?;
    let day_rows = day_stmt
        .query_map(params![since], |row| {
            Ok(AiToolDayStat {
                date: row.get(0)?,
                total: row.get(1)?,
                failed: row.get(2)?,
            })
        })
        .map_err(err)?;
    let day_stats: Vec<AiToolDayStat> = day_rows.filter_map(Result::ok).collect();

    let mut tool_stmt = conn
        .prepare(
            "SELECT tool,
                    COUNT(*),
                    COALESCE(SUM(CASE WHEN ok = 0 THEN 1 ELSE 0 END), 0),
                    COALESCE(CAST(AVG(duration_ms) AS INTEGER), 0)
             FROM ai_tool_logs
             WHERE created_at >= ?1
             GROUP BY tool
             ORDER BY COUNT(*) DESC
             LIMIT 60",
        )
        .map_err(err)?;
    let tool_rows = tool_stmt
        .query_map(params![since], |row| {
            Ok(AiToolUsageStat {
                tool: row.get(0)?,
                total: row.get(1)?,
                failed: row.get(2)?,
                avg_duration_ms: row.get(3)?,
            })
        })
        .map_err(err)?;
    let tool_stats: Vec<AiToolUsageStat> = tool_rows.filter_map(Result::ok).collect();

    Ok(AiToolLogStats {
        total,
        ok: ok_count,
        failed,
        avg_duration_ms: avg_duration,
        last_24h,
        days: day_stats,
        tools: tool_stats,
    })
}

#[tauri::command]
pub fn ai_tool_log_clear(
    state: State<'_, AppState>,
    session_id: Option<i64>,
    days: Option<i64>,
) -> Result<usize, String> {
    let conn = state.pool.get().map_err(err)?;
    // days 为空表示清空全部；否则只清该时间窗口内的记录
    let since = days.map(|value| now_ms() - value.max(1) * DAY_MS);
    let affected = conn
        .execute(
            "DELETE FROM ai_tool_logs
             WHERE (?1 IS NULL OR session_id = ?1)
               AND (?2 IS NULL OR created_at >= ?2)",
            params![session_id, since],
        )
        .map_err(err)?;
    Ok(affected)
}

/// 模型用量统计：按服务商/模型聚合调用次数、成功率、token、耗时，以及该模型会话下的工具成功率
#[tauri::command]
pub fn ai_usage_stats(
    state: State<'_, AppState>,
    days: Option<i64>,
) -> Result<AiUsageStats, String> {
    let conn = state.pool.get().map_err(err)?;
    let days = days.unwrap_or(30).clamp(1, 365);
    let since = now_ms() - days * DAY_MS;

    let mut stmt = conn
        .prepare(
            "SELECT provider_id, provider_name, model,
                    COUNT(*),
                    COALESCE(SUM(CASE WHEN ok != 0 THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(prompt_tokens), 0),
                    COALESCE(SUM(completion_tokens), 0),
                    COALESCE(CAST(AVG(latency_ms) AS INTEGER), 0),
                    COALESCE(MAX(created_at), 0)
             FROM ai_usage_logs
             WHERE created_at >= ?1
             GROUP BY provider_id, provider_name, model
             ORDER BY COUNT(*) DESC",
        )
        .map_err(err)?;
    let rows = stmt
        .query_map(params![since], |row| {
            let calls: i64 = row.get(3)?;
            let ok_calls: i64 = row.get(4)?;
            Ok(AiUsageStat {
                provider_id: row.get(0)?,
                provider_name: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                model: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                calls,
                ok_calls,
                success_rate: rate(ok_calls, calls),
                prompt_tokens: row.get(5)?,
                completion_tokens: row.get(6)?,
                total_tokens: row.get::<_, i64>(5)? + row.get::<_, i64>(6)?,
                avg_latency_ms: row.get(7)?,
                tool_calls: 0,
                tool_ok: 0,
                tool_success_rate: 0.0,
                last_used_at: row.get(8)?,
            })
        })
        .map_err(err)?;
    let mut items: Vec<AiUsageStat> = rows.filter_map(Result::ok).collect();

    // 工具成功率按会话归属到服务商
    for item in items.iter_mut() {
        if let Some(provider_id) = item.provider_id {
            let (tool_calls, tool_ok): (i64, i64) = conn
                .query_row(
                    "SELECT COUNT(*), COALESCE(SUM(CASE WHEN l.ok != 0 THEN 1 ELSE 0 END), 0)
                     FROM ai_tool_logs l
                     JOIN ai_sessions s ON s.id = l.session_id
                     WHERE s.provider_id = ?1 AND l.created_at >= ?2",
                    params![provider_id, since],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .unwrap_or((0, 0));
            item.tool_calls = tool_calls;
            item.tool_ok = tool_ok;
            item.tool_success_rate = rate(tool_ok, tool_calls);
        }
    }

    let (total_calls, ok_calls, prompt_tokens, completion_tokens, avg_latency): (
        i64,
        i64,
        i64,
        i64,
        i64,
    ) = conn
        .query_row(
            "SELECT COUNT(*),
                    COALESCE(SUM(CASE WHEN ok != 0 THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(prompt_tokens), 0),
                    COALESCE(SUM(completion_tokens), 0),
                    COALESCE(CAST(AVG(latency_ms) AS INTEGER), 0)
             FROM ai_usage_logs WHERE created_at >= ?1",
            params![since],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
        )
        .map_err(err)?;

    let (tool_calls, tool_ok): (i64, i64) = conn
        .query_row(
            "SELECT COUNT(*), COALESCE(SUM(CASE WHEN ok != 0 THEN 1 ELSE 0 END), 0)
             FROM ai_tool_logs WHERE created_at >= ?1",
            params![since],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap_or((0, 0));

    let mut day_stmt = conn
        .prepare(
            "SELECT strftime('%Y-%m-%d', created_at / 1000, 'unixepoch', 'localtime') AS day,
                    COUNT(*),
                    COALESCE(SUM(prompt_tokens), 0),
                    COALESCE(SUM(completion_tokens), 0)
             FROM ai_usage_logs
             WHERE created_at >= ?1
             GROUP BY day ORDER BY day",
        )
        .map_err(err)?;
    let day_rows = day_stmt
        .query_map(params![since], |row| {
            let prompt_tokens: i64 = row.get(2)?;
            let completion_tokens: i64 = row.get(3)?;
            Ok(AiUsageDay {
                day: row.get(0)?,
                calls: row.get(1)?,
                prompt_tokens,
                completion_tokens,
                total_tokens: prompt_tokens + completion_tokens,
            })
        })
        .map_err(err)?;
    let daily: Vec<AiUsageDay> = day_rows.filter_map(Result::ok).collect();

    Ok(AiUsageStats {
        days,
        total_calls,
        ok_calls,
        success_rate: rate(ok_calls, total_calls),
        prompt_tokens,
        completion_tokens,
        total_tokens: prompt_tokens + completion_tokens,
        avg_latency_ms: avg_latency,
        tool_calls,
        tool_ok,
        tool_success_rate: rate(tool_ok, tool_calls),
        items,
        daily,
    })
}

/// 用量明细：每次模型调用一条，按时间倒序分页
#[tauri::command]
pub fn ai_usage_logs(
    state: State<'_, AppState>,
    days: Option<i64>,
    page: Option<i64>,
    page_size: Option<i64>,
    ok: Option<bool>,
) -> Result<AiUsagePage, String> {
    let conn = state.pool.get().map_err(err)?;
    let days = days.unwrap_or(30).clamp(1, USAGE_RETENTION_DAYS);
    let since = now_ms() - days * DAY_MS;
    let page = page.unwrap_or(1).max(1);
    let page_size = page_size.unwrap_or(10).clamp(1, 200);
    let offset = (page - 1) * page_size;
    let ok_flag: Option<i64> = ok.map(|value| if value { 1 } else { 0 });

    let total: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM ai_usage_logs
             WHERE created_at >= ?1 AND (?2 IS NULL OR ok = ?2)",
            params![since, ok_flag],
            |row| row.get(0),
        )
        .map_err(err)?;

    let mut stmt = conn
        .prepare(
            "SELECT id, COALESCE(provider_name, ''), COALESCE(model, ''),
                    prompt_tokens, completion_tokens, ok, latency_ms,
                    COALESCE(error, ''), COALESCE(request_preview, ''), created_at
             FROM ai_usage_logs
             WHERE created_at >= ?1 AND (?2 IS NULL OR ok = ?2)
             ORDER BY id DESC
             LIMIT ?3 OFFSET ?4",
        )
        .map_err(err)?;
    let rows = stmt
        .query_map(params![since, ok_flag, page_size, offset], |row| {
            let prompt_tokens: i64 = row.get(3)?;
            let completion_tokens: i64 = row.get(4)?;
            Ok(AiUsageLog {
                id: row.get(0)?,
                provider_name: row.get(1)?,
                model: row.get(2)?,
                prompt_tokens,
                completion_tokens,
                total_tokens: prompt_tokens + completion_tokens,
                ok: row.get::<_, i64>(5)? != 0,
                latency_ms: row.get(6)?,
                error: row.get(7)?,
                request_preview: row.get(8)?,
                created_at: row.get(9)?,
            })
        })
        .map_err(err)?;
    let items: Vec<AiUsageLog> = rows.filter_map(Result::ok).collect();
    Ok(AiUsagePage { items, total })
}

/// 清空用量记录（默认只清超过 365 天的，explicit=true 时全清）
#[tauri::command]
pub fn ai_usage_clear(state: State<'_, AppState>, all: Option<bool>) -> Result<usize, String> {
    let conn = state.pool.get().map_err(err)?;
    let affected = if all.unwrap_or(false) {
        conn.execute("DELETE FROM ai_usage_logs", [])
    } else {
        conn.execute(
            "DELETE FROM ai_usage_logs WHERE created_at < ?1",
            params![now_ms() - USAGE_RETENTION_DAYS * DAY_MS],
        )
    }
    .map_err(err)?;
    Ok(affected)
}

fn rate(part: i64, total: i64) -> f64 {
    if total <= 0 {
        0.0
    } else {
        part as f64 / total as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mask_key_keeps_head_and_tail() {
        assert_eq!(mask_key("sk-1234567890abcdef"), "sk-1******ef");
        assert_eq!(mask_key("short"), "******");
        assert_eq!(mask_key(""), "");
    }

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let plain = "sk-test-不会出现在明文里的密钥";
        let packed = encrypt_key(plain).expect("encrypt");
        assert_ne!(packed, plain);
        assert_eq!(decrypt_key(&packed), plain);
        // 空串与损坏密文都不 panic
        assert_eq!(decrypt_key(""), "");
        // 历史明文行按原样返回，带前缀但损坏的密文返回空串
        assert_eq!(decrypt_key("legacy-plain-key"), "legacy-plain-key");
        assert_eq!(decrypt_key(&format!("{CIPHER_PREFIX}not-base64!!!")), "");
    }

    #[test]
    fn humanize_api_error_maps_common_status() {
        assert!(humanize_api_error(401, "x").contains("API Key 无效"));
        assert!(humanize_api_error(402, "x").contains("余额"));
        assert!(humanize_api_error(403, "x").contains("没有访问权限"));
        assert!(humanize_api_error(404, "x").contains("接口地址或模型不存在"));
        assert!(humanize_api_error(429, "x").contains("请求过于频繁"));
        assert!(humanize_api_error(503, "x").contains("HTTP 503"));
        assert!(humanize_api_error(400, "bad param").contains("bad param"));
        // 未知状态码保留原文
        assert_eq!(humanize_api_error(418, "teapot"), "teapot");
    }

    #[test]
    fn humanize_api_error_supported_models() {
        let msg = "The supported API model names are deepseek-chat, deepseek-reasoner, but you passed gpt-4.";
        let text = humanize_api_error(400, msg);
        assert!(text.contains("gpt-4"));
        assert!(text.contains("deepseek-chat"));
        assert!(text.contains("模型服务配置"));
    }

    #[test]
    fn humanize_api_error_context_length() {
        let text = humanize_api_error(400, "This model's maximum context length is 8192 tokens");
        assert!(text.contains("上下文长度"));
    }

    #[test]
    fn humanize_network_error_categories() {
        assert!(humanize_network_error("dns error: failed to lookup address").contains("无法解析"));
        assert!(humanize_network_error("connection refused").contains("连接被拒绝"));
        assert!(humanize_network_error("operation timed out").contains("连接超时"));
        assert!(humanize_network_error("certificate verify failed").contains("证书"));
        assert!(humanize_network_error("boom").starts_with("连接 AI 服务失败"));
    }

    #[test]
    fn truncate_title_handles_cjk_and_whitespace() {
        assert_eq!(truncate_title("  你好   世界  "), "你好 世界");
        assert_eq!(truncate_title(""), "新对话");
        let long = "一".repeat(30);
        let title = truncate_title(&long);
        assert_eq!(title.chars().count(), 25);
        assert!(title.ends_with('…'));
    }

    #[test]
    fn estimate_tokens_counts_cjk_as_one() {
        assert_eq!(estimate_tokens("你好"), 2);
        assert_eq!(estimate_tokens("abcd"), 1);
        // 非中文按 4 字符 1 token 向上取整
        assert_eq!(estimate_tokens("abcde"), 2);
    }

    #[test]
    fn clip_chars_and_trace_add_notice() {
        assert_eq!(clip_chars("abc", 5), "abc");
        let clipped = clip_chars("abcdef", 3);
        assert!(clipped.starts_with("abc"));
        assert!(clipped.contains("已截断"));
        let trace = clip_trace("abcdef", 3);
        assert!(trace.contains("原长度 6"));
    }

    #[test]
    fn strip_file_blocks_replaces_body_with_summary() {
        let text = "看附件：\n```fbin-file demo.json\n{\"a\":1}\n```\n结束";
        let stored = strip_file_blocks(text);
        assert!(stored.contains("【附件 demo.json · 7 字符，正文未入库】"));
        assert!(!stored.contains("{\"a\":1}"));
        // 不完整围栏原样返回，不做破坏性处理
        let broken = "```fbin-file demo.json\n没有收尾";
        assert_eq!(strip_file_blocks(broken), broken);
    }

    #[test]
    fn parse_sse_openai_text_and_done() {
        let line = r#"data: {"choices":[{"delta":{"content":"你好"}}]}"#;
        assert_eq!(parse_sse("openai", line), Ok(SseOutcome::Text("你好".into())));
        assert_eq!(parse_sse("openai", "data: [DONE]"), Ok(SseOutcome::Done));
        assert_eq!(parse_sse("openai", "event: ping"), Ok(SseOutcome::Ignore));
        assert_eq!(parse_sse("openai", "data: not-json"), Ok(SseOutcome::Ignore));
        let reasoning = r#"data: {"choices":[{"delta":{"reasoning_content":"思考"}}]}"#;
        assert_eq!(
            parse_sse("openai", reasoning),
            Ok(SseOutcome::Reasoning("思考".into()))
        );
    }

    #[test]
    fn parse_sse_anthropic_and_gemini() {
        let anth = r#"data: {"type":"content_block_delta","delta":{"text":"hi"}}"#;
        assert_eq!(parse_sse("anthropic", anth), Ok(SseOutcome::Text("hi".into())));
        let thinking = r#"data: {"type":"content_block_delta","delta":{"thinking":"想"}}"#;
        assert_eq!(
            parse_sse("anthropic", thinking),
            Ok(SseOutcome::Reasoning("想".into()))
        );
        assert_eq!(
            parse_sse("anthropic", r#"data: {"type":"message_stop"}"#),
            Ok(SseOutcome::Done)
        );
        let gemini = r#"data: {"candidates":[{"content":{"parts":[{"text":"哈"}]}}]}"#;
        assert_eq!(parse_sse("gemini", gemini), Ok(SseOutcome::Text("哈".into())));
        let blocked = r#"data: {"promptFeedback":{"blockReason":"SAFETY"}}"#;
        assert!(parse_sse("gemini", blocked).is_err());
    }

    #[test]
    fn extract_api_error_reads_message_fields() {
        assert_eq!(extract_api_error(""), "服务无响应内容");
        assert_eq!(
            extract_api_error(r#"{"error":{"message":"bad"}}"#),
            "bad"
        );
        assert_eq!(extract_api_error(r#"{"message":"oops"}"#), "oops");
        assert_eq!(extract_api_error("plain text"), "plain text");
    }

    #[test]
    fn retry_after_reads_header() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(reqwest::header::RETRY_AFTER, "3".parse().unwrap());
        assert_eq!(retry_after_secs(&headers), Some(3));
        assert_eq!(retry_after_secs(&reqwest::header::HeaderMap::new()), None);
    }
}
