//! 截图识别：独立于聊天协议的一次性多模态调用。
//!
//! 只做识别、不落库；结果由前端确认后再走 `notes_create_from_image`。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::{Datelike, Local, NaiveDate, NaiveDateTime, TimeZone};
use serde::{Deserialize, Serialize};

use crate::commands::ai::{
    default_base, extract_api_error, humanize_api_error, humanize_network_error, load_provider,
    parse_full_text, record_usage,
};
use crate::state::AppState;
use crate::util::now_ms;

const PROMPT: &str = include_str!("../prompts/notes_extract.md");
const TIMEOUT_SECS: u64 = 60;
const MAX_TOKENS: i64 = 4096;
const MAX_ITEMS: usize = 20;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtractItem {
    pub title: String,
    pub detail: String,
    pub due_at: Option<i64>,
    pub repeat: String,
    pub priority: i64,
    pub tags: Vec<String>,
    pub warning: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtractResult {
    pub items: Vec<ExtractItem>,
    pub summary: String,
    pub raw_text: String,
}

/// 模型返回的原始结构，字段名兼容 snake_case / camelCase
#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct RawExtract {
    summary: String,
    raw_text: String,
    raw_text_alt: String,
    items: Vec<RawItem>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct RawItem {
    title: String,
    detail: String,
    due_at: Option<String>,
    repeat: String,
    priority: i64,
    tags: Vec<String>,
    warning: Option<String>,
}

#[derive(Default)]
struct Usage {
    prompt: i64,
    completion: i64,
}

struct CancelGuard<'a> {
    state: &'a AppState,
    id: &'a str,
}
impl Drop for CancelGuard<'_> {
    fn drop(&mut self) {
        self.state.ai_release(self.id);
    }
}

/// 识别一张图。`base64` 是前端压缩过的副本，原图由调用方另行入库。
pub async fn extract(
    state: &AppState,
    provider_id: Option<i64>,
    mime: &str,
    base64_data: &str,
    request_id: &str,
) -> Result<ExtractResult, String> {
    let settings = super::get_settings(&state.pool);
    let provider_id = provider_id.or(settings.vision_provider_id).ok_or_else(|| {
        "还没有配置截图识别服务：请到「航仔 → 模型服务」添加一个支持图片的模型".to_string()
    })?;
    let provider = load_provider(state, provider_id)?;
    if !provider.supports_vision {
        return Err(
            "当前服务商未标记为支持图片识别，请到「航仔 → 模型服务」勾选「支持图片识别」"
                .to_string(),
        );
    }
    if base64_data.trim().is_empty() {
        return Err("图片内容为空".into());
    }

    let cancel = state.ai_register(request_id);
    let _guard = CancelGuard {
        state,
        id: request_id,
    };

    let mut prompt = render_prompt();
    let mut last_error = String::new();
    for attempt in 0..2 {
        if attempt == 1 {
            prompt.push_str("\n\n只返回 JSON，不要解释，不要 Markdown 代码块。");
        }
        let (text, usage, latency, preview) =
            call_model(state, &provider, mime, base64_data, &prompt, &cancel).await?;
        match parse_extract(&text, now_ms()) {
            Ok(result) => {
                record_usage(
                    state,
                    &provider,
                    usage.prompt,
                    usage.completion,
                    true,
                    latency,
                    None,
                    &preview,
                );
                return Ok(result);
            }
            Err(error) => {
                last_error = error;
                record_usage(
                    state,
                    &provider,
                    usage.prompt,
                    usage.completion,
                    false,
                    latency,
                    Some("模型返回的不是合法 JSON"),
                    &preview,
                );
            }
        }
    }
    Err(format!(
        "E_NOTES_EXTRACT_FORMAT: 模型没有返回可解析的待办清单（{last_error}），可以重试或手动录入"
    ))
}

async fn call_model(
    state: &AppState,
    provider: &crate::models::AiProvider,
    mime: &str,
    base64_data: &str,
    prompt: &str,
    cancel: &Arc<AtomicBool>,
) -> Result<(String, Usage, i64, String), String> {
    let base = if provider.base_url.trim().is_empty() {
        default_base(&provider.kind).to_string()
    } else {
        provider.base_url.trim().trim_end_matches('/').to_string()
    };
    let key = provider.api_key.trim();
    let mut temperature = None;
    if let Some(value) = provider.temperature {
        temperature = Some(value.clamp(0.0, 2.0));
    }

    let (_url, request) = match provider.kind.as_str() {
        "anthropic" => {
            let mut body = serde_json::json!({
                "model": provider.model,
                "max_tokens": MAX_TOKENS,
                "messages": [{
                    "role": "user",
                    "content": [
                        { "type": "image", "source": {
                            "type": "base64", "media_type": mime, "data": base64_data
                        }},
                        { "type": "text", "text": prompt }
                    ]
                }]
            });
            if let Some(value) = temperature {
                body["temperature"] = serde_json::json!(value);
            }
            let client = state.client(false);
            (
                format!("{base}/v1/messages"),
                client
                    .post(format!("{base}/v1/messages"))
                    .header("x-api-key", key)
                    .header("anthropic-version", "2023-06-01")
                    .timeout(Duration::from_secs(TIMEOUT_SECS))
                    .json(&body),
            )
        }
        "gemini" => {
            let mut config = serde_json::json!({ "maxOutputTokens": MAX_TOKENS });
            if let Some(value) = temperature {
                config["temperature"] = serde_json::json!(value);
            }
            let body = serde_json::json!({
                "contents": [{
                    "role": "user",
                    "parts": [
                        { "inline_data": { "mime_type": mime, "data": base64_data } },
                        { "text": prompt }
                    ]
                }],
                "generationConfig": config
            });
            let client = state.client(false);
            (
                format!(
                    "{base}/v1beta/models/{}:generateContent",
                    provider.model.trim()
                ),
                client
                    .post(format!(
                        "{base}/v1beta/models/{}:generateContent",
                        provider.model.trim()
                    ))
                    .header("x-goog-api-key", key)
                    .timeout(Duration::from_secs(TIMEOUT_SECS))
                    .json(&body),
            )
        }
        _ => {
            let mut body = serde_json::json!({
                "model": provider.model,
                "stream": false,
                "max_tokens": MAX_TOKENS,
                "messages": [{
                    "role": "user",
                    "content": [
                        { "type": "text", "text": prompt },
                        { "type": "image_url", "image_url": {
                            "url": format!("data:{mime};base64,{base64_data}")
                        }}
                    ]
                }]
            });
            if let Some(value) = temperature {
                body["temperature"] = serde_json::json!(value);
            }
            let client = state.client(false);
            (
                format!("{base}/chat/completions"),
                client
                    .post(format!("{base}/chat/completions"))
                    .header("Authorization", format!("Bearer {key}"))
                    .timeout(Duration::from_secs(TIMEOUT_SECS))
                    .json(&body),
            )
        }
    };

    let started = Instant::now();
    let response = send_with_cancel(request, cancel).await?;
    let status = response.status();
    let raw = response.text().await.unwrap_or_default();
    let latency = started.elapsed().as_millis() as i64;
    if !status.is_success() {
        let message = extract_api_error(&raw);
        return Err(map_error(status.as_u16(), &message));
    }

    let usage = parse_usage(&provider.kind, &raw);
    let text = parse_full_text(&provider.kind, &raw)?;
    let preview = format!(
        "[截图识别] {} · {}",
        provider.name,
        text.chars().take(120).collect::<String>()
    );
    Ok((text, usage, latency, preview))
}

async fn send_with_cancel(
    request: reqwest::RequestBuilder,
    cancel: &Arc<AtomicBool>,
) -> Result<reqwest::Response, String> {
    let future = request.send();
    tokio::pin!(future);
    loop {
        tokio::select! {
            result = &mut future => {
                return result.map_err(|error| humanize_network_error(&error.to_string()));
            }
            _ = tokio::time::sleep(Duration::from_millis(200)) => {
                if cancel.load(Ordering::SeqCst) {
                    return Err("已取消识别".to_string());
                }
            }
        }
    }
}

fn map_error(status: u16, message: &str) -> String {
    let lower = message.to_ascii_lowercase();
    if lower.contains("image")
        || lower.contains("vision")
        || lower.contains("multimodal")
        || lower.contains("image_url")
    {
        return format!("当前模型不支持图片输入，请换用视觉模型。（服务商返回：{message}）");
    }
    humanize_api_error(status, message)
}

fn parse_usage(kind: &str, raw: &str) -> Usage {
    let Ok(json) = serde_json::from_str::<serde_json::Value>(raw) else {
        return Usage::default();
    };
    let read = |value: Option<&serde_json::Value>| value.and_then(|v| v.as_i64()).unwrap_or(0);
    match kind {
        "anthropic" => Usage {
            prompt: read(json.get("usage").and_then(|u| u.get("input_tokens"))),
            completion: read(json.get("usage").and_then(|u| u.get("output_tokens"))),
        },
        "gemini" => Usage {
            prompt: read(
                json.get("usageMetadata")
                    .and_then(|u| u.get("promptTokenCount")),
            ),
            completion: read(
                json.get("usageMetadata")
                    .and_then(|u| u.get("candidatesTokenCount")),
            ),
        },
        _ => Usage {
            prompt: read(json.get("usage").and_then(|u| u.get("prompt_tokens"))),
            completion: read(json.get("usage").and_then(|u| u.get("completion_tokens"))),
        },
    }
}

fn render_prompt() -> String {
    let now = Local::now();
    let text = PROMPT
        .replace("{{NOW}}", &now.format("%Y-%m-%d %H:%M").to_string())
        .replace("{{WEEKDAY}}", &now.format("%A").to_string())
        .replace("{{OFFSET}}", &now.format("%:z").to_string());
    text
}

/// 去掉代码围栏，截取第一个 `{` 到最后一个 `}`，兼容 snake / camel 两种字段名
pub(crate) fn parse_extract(text: &str, now: i64) -> Result<ExtractResult, String> {
    let trimmed = text.trim();
    let start = trimmed
        .find('{')
        .ok_or_else(|| "没有找到 JSON 内容".to_string())?;
    let end = trimmed
        .rfind('}')
        .ok_or_else(|| "JSON 内容不完整".to_string())?;
    if end <= start {
        return Err("JSON 内容不完整".into());
    }
    let value: serde_json::Value = serde_json::from_str(&trimmed[start..=end])
        .map_err(|error| format!("JSON 解析失败：{error}"))?;
    let raw: RawExtract = serde_json::from_value(normalize_keys(value))
        .map_err(|error| format!("字段结构不符合约定：{error}"))?;

    let mut items = Vec::new();
    for item in raw.items.into_iter().take(MAX_ITEMS) {
        let title = item.title.trim();
        if title.is_empty() {
            continue;
        }
        let mut warnings: Vec<String> = Vec::new();
        if let Some(text) = item
            .warning
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
        {
            warnings.push(text.to_string());
        }
        let mut due_at = None;
        if let Some(text) = item
            .due_at
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
        {
            match parse_local_time(text) {
                Some(value) => {
                    if in_range(value) {
                        due_at = Some(value);
                    } else {
                        warnings.push("时间超出允许范围，已留空".to_string());
                    }
                }
                None => warnings.push("时间无法识别，已留空".to_string()),
            }
        }
        if let Some(due) = due_at {
            if due < now && !warnings.iter().any(|w| w.contains("已过")) {
                warnings.push("时间已过".to_string());
            }
        }
        let repeat = match item.repeat.trim() {
            "daily" | "weekdays" | "weekly" | "monthly" => item.repeat.trim().to_string(),
            _ => "none".to_string(),
        };
        if due_at.is_none() && repeat != "none" {
            // 只有重复词没有日期：由后端取下一次未来时刻
            let anchor = next_anchor_for_repeat(&repeat, now);
            due_at = Some(anchor);
            warnings.push("未写明确日期，已按下一次时间推断".to_string());
        }
        items.push(ExtractItem {
            title: title.chars().take(60).collect(),
            detail: item.detail.trim().chars().take(2000).collect(),
            due_at,
            repeat,
            priority: item.priority.clamp(0, 2),
            tags: super::normalize_tags(item.tags),
            warning: if warnings.is_empty() {
                None
            } else {
                Some(warnings.join("；"))
            },
        });
    }

    let raw_text = if raw.raw_text.trim().is_empty() {
        raw.raw_text_alt
    } else {
        raw.raw_text
    };
    Ok(ExtractResult {
        items,
        summary: raw.summary.trim().chars().take(200).collect(),
        raw_text: raw_text.chars().take(8000).collect(),
    })
}

fn normalize_keys(value: serde_json::Value) -> serde_json::Value {
    let serde_json::Value::Object(mut map) = value else {
        return value;
    };
    for (from, to) in [("rawText", "raw_text"), ("dueAt", "due_at")] {
        if let Some(field) = map.remove(from) {
            map.entry(to.to_string()).or_insert(field);
        }
    }
    if let Some(serde_json::Value::Array(items)) = map.get_mut("items") {
        for item in items.iter_mut() {
            if let serde_json::Value::Object(fields) = item {
                if let Some(field) = fields.remove("dueAt") {
                    fields.entry("due_at".to_string()).or_insert(field);
                }
            }
        }
    }
    serde_json::Value::Object(map)
}

fn parse_local_time(text: &str) -> Option<i64> {
    let text = text.trim();
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(text) {
        return Some(dt.timestamp_millis());
    }
    for format in ["%Y-%m-%d %H:%M:%S", "%Y-%m-%d %H:%M", "%Y/%m/%d %H:%M"] {
        if let Ok(naive) = NaiveDateTime::parse_from_str(text, format) {
            return super::local_millis(naive);
        }
    }
    for format in ["%Y-%m-%d", "%Y/%m/%d"] {
        if let Ok(date) = NaiveDate::parse_from_str(text, format) {
            return super::local_millis(date.and_hms_opt(18, 0, 0)?);
        }
    }
    None
}

fn in_range(value: i64) -> bool {
    let min = NaiveDate::from_ymd_opt(2000, 1, 1)
        .and_then(|date| date.and_hms_opt(0, 0, 0))
        .and_then(super::local_millis)
        .unwrap_or(946_656_000_000);
    let max = NaiveDate::from_ymd_opt(2100, 1, 1)
        .and_then(|date| date.and_hms_opt(0, 0, 0))
        .and_then(super::local_millis)
        .unwrap_or(4_102_444_800_000);
    value >= min && value <= max
}

fn next_anchor_for_repeat(repeat: &str, now: i64) -> i64 {
    let today = super::start_of_today(now);
    let dt = Local
        .timestamp_millis_opt(today)
        .single()
        .unwrap_or_else(Local::now);
    let base = dt
        .date_naive()
        .and_hms_opt(18, 0, 0)
        .map(|naive| super::local_millis(naive).unwrap_or(now))
        .unwrap_or(now);
    let candidate = if base > now {
        base
    } else {
        today + 24 * 60 * 60 * 1000
    };
    match repeat {
        "daily" => candidate,
        "weekdays" => {
            let mut value = candidate;
            for _ in 0..7 {
                let weekday = Local
                    .timestamp_millis_opt(value)
                    .single()
                    .map(|d| d.weekday().num_days_from_monday())
                    .unwrap_or(0);
                if weekday < 5 {
                    break;
                }
                value += 24 * 60 * 60 * 1000;
            }
            value
        }
        "weekly" => {
            let mut value = candidate;
            for _ in 0..7 {
                let weekday = Local
                    .timestamp_millis_opt(value)
                    .single()
                    .map(|d| d.weekday().num_days_from_monday())
                    .unwrap_or(0);
                if weekday == 0 {
                    break;
                }
                value += 24 * 60 * 60 * 1000;
            }
            value
        }
        _ => candidate,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fenced_json_and_camel_fields() {
        let text = r#"```json
        {"summary":"群通知","rawText":"周五交周报","items":[
          {"title":"提交周报","detail":"发到群里","dueAt":"2026-11-06 18:00","repeat":"weekly","priority":1,"tags":["工作"]}
        ]}
        ```"#;
        let now = NaiveDate::from_ymd_opt(2026, 11, 4)
            .unwrap()
            .and_hms_opt(9, 0, 0)
            .and_then(super::super::local_millis)
            .unwrap();
        let result = parse_extract(text, now).unwrap();
        assert_eq!(result.items.len(), 1);
        assert_eq!(result.items[0].title, "提交周报");
        assert!(result.items[0].due_at.is_some());
        assert_eq!(result.raw_text, "周五交周报");
    }

    #[test]
    fn empty_items_is_legal() {
        let result = parse_extract(r#"{"items":[]}"#, now_ms()).unwrap();
        assert!(result.items.is_empty());
    }
}
