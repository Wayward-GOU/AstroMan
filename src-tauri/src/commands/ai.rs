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
    AiToolLog, AiToolLogPage, AiToolLogStats, AiToolUsageStat, AiUsageDay, AiUsageStat,
    AiUsageStats, HttpResult, SendRequestPayload,
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

fn row_to_provider(row: &Row) -> rusqlite::Result<AiProvider> {
    let stored: String = row.get(5)?;
    Ok(AiProvider {
        id: Some(row.get(0)?),
        name: row.get(1)?,
        kind: row.get(2)?,
        base_url: row.get(3)?,
        model: row.get(4)?,
        api_key: decrypt_key(&stored),
        temperature: Some(row.get(6)?),
        is_default: row.get::<_, i64>(7)? != 0,
    })
}

const PROVIDER_COLUMNS: &str = "id, name, COALESCE(kind, 'openai'), base_url, model,
                                COALESCE(api_key_enc, ''), COALESCE(temperature, 0.3),
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
                         temperature = ?5, is_default = ?6, enabled = 1, updated_at = ?7
                     WHERE id = ?8",
                    params![
                        provider.name.trim(),
                        kind,
                        provider.base_url.trim(),
                        provider.model.trim(),
                        temperature,
                        provider.is_default as i64,
                        now,
                        id
                    ],
                )
                .map_err(err)?;
            } else {
                conn.execute(
                    "UPDATE ai_providers
                     SET name = ?1, kind = ?2, base_url = ?3, model = ?4, api_key_enc = ?5,
                         temperature = ?6, is_default = ?7, enabled = 1, updated_at = ?8
                     WHERE id = ?9",
                    params![
                        provider.name.trim(),
                        kind,
                        provider.base_url.trim(),
                        provider.model.trim(),
                        key_enc,
                        temperature,
                        provider.is_default as i64,
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
                   (name, kind, base_url, model, api_key_enc, temperature, is_default, enabled, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 1, ?8, ?8)",
                params![
                    provider.name.trim(),
                    kind,
                    provider.base_url.trim(),
                    provider.model.trim(),
                    key_enc,
                    temperature,
                    provider.is_default as i64,
                    now
                ],
            )
            .map_err(err)?;
            conn.last_insert_rowid()
        }
    };

    Ok(AiProvider {
        id: Some(id),
        name: provider.name.trim().to_string(),
        kind,
        base_url: provider.base_url.trim().to_string(),
        model: provider.model.trim().to_string(),
        api_key: provider.api_key,
        temperature: Some(temperature),
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
    conn.query_row(&sql, params![id], row_to_provider)
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
                "max_tokens": 4096,
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
            Ok(Prepared {
                url: format!("{base}/chat/completions"),
                headers: vec![("Authorization".to_string(), format!("Bearer {key}"))],
                body: serde_json::json!({
                    "model": provider.model,
                    "temperature": temperature,
                    "stream": stream,
                    "messages": chat,
                }),
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
            completion_tokens, ok, latency_ms, error, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            provider.id,
            provider.name,
            provider.model,
            prompt_tokens,
            completion_tokens,
            ok as i64,
            latency_ms,
            error.map(|text| text.chars().take(500).collect::<String>()),
            now_ms(),
        ],
    );
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
        .timeout(Duration::from_secs(AI_TIMEOUT_SECS))
        .header("Content-Type", "application/json")
        .json(&prepared.body);
    for (key, value) in &prepared.headers {
        request = request.header(key, value);
    }

    let response = match request.send().await {
        Ok(response) => response,
        Err(error) => {
            let text = format!("连接 AI 服务失败：{error}");
            emit(&on_event, "error", text.clone());
            record_usage(
                state.inner(),
                &provider,
                prompt_chars,
                0,
                false,
                started.elapsed().as_millis() as i64,
                Some(&text),
            );
            return Err(text);
        }
    };

    let status = response.status();
    if !status.is_success() {
        let raw = response.text().await.unwrap_or_default();
        let text = format!("AI 服务返回 {}：{}", status.as_u16(), extract_api_error(&raw));
        emit(&on_event, "error", text.clone());
        record_usage(
            state.inner(),
            &provider,
            prompt_chars,
            0,
            false,
            started.elapsed().as_millis() as i64,
            Some(&text),
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
            );
        }};
    }
    while let Some(chunk) = stream.next().await {
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
pub fn ai_session_list(state: State<'_, AppState>) -> Result<Vec<AiSession>, String> {
    let conn = state.pool.get().map_err(err)?;
    let mut stmt = conn
        .prepare(
            "SELECT s.id, s.title, s.provider_id,
                    (SELECT COUNT(*) FROM ai_messages m WHERE m.session_id = s.id),
                    s.created_at, s.updated_at
             FROM ai_sessions s
             ORDER BY s.updated_at DESC, s.id DESC
             LIMIT 200",
        )
        .map_err(err)?;
    let rows = stmt
        .query_map([], |row| {
            Ok(AiSession {
                id: row.get(0)?,
                title: row.get(1)?,
                provider_id: row.get(2)?,
                message_count: row.get(3)?,
                created_at: row.get(4)?,
                updated_at: row.get(5)?,
            })
        })
        .map_err(err)?;
    Ok(rows.filter_map(Result::ok).collect())
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
    conn.execute(
        "INSERT INTO ai_messages (session_id, role, content, reasoning, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![session_id, role, content, reasoning, now],
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
) -> Result<(), String> {
    let conn = state.pool.get().map_err(err)?;
    let args: String = args.chars().take(2000).collect();
    let preview: String = result_preview.chars().take(2000).collect();
    conn.execute(
        "INSERT INTO ai_tool_logs
           (session_id, tool, args, ok, duration_ms, result_preview, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            session_id,
            tool,
            args,
            ok as i64,
            duration_ms,
            preview,
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
                    duration_ms, COALESCE(result_preview, ''), created_at
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
             LIMIT 8",
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
                    COALESCE(SUM(prompt_tokens + completion_tokens), 0)
             FROM ai_usage_logs
             WHERE created_at >= ?1
             GROUP BY day ORDER BY day",
        )
        .map_err(err)?;
    let day_rows = day_stmt
        .query_map(params![since], |row| {
            Ok(AiUsageDay {
                day: row.get(0)?,
                calls: row.get(1)?,
                total_tokens: row.get(2)?,
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
