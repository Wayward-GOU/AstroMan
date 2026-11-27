//! 大模型接口用量旁路嗅探。
//!
//! 目标：抓包开启时，把客户端（桌面版 / CLI / 任意工具）直连各家接口的真实
//! `usage` 抽出来记账，供 Token 看板合并。**只抽数字，不落正文**——SSE 响应
//! 本身就不进 `store`（`plan_response_body` 明确拒收 `text/event-stream`），
//! 这里也只是在转发路径上旁听字节，解析完立即丢弃。
//!
//! 支持三种协议形态：
//!   - Anthropic Messages：`message_start` 给输入与缓存分项，`message_delta` 给累计输出
//!   - OpenAI Chat Completions：末包（或 `stream_options.include_usage` 打开时的独立块）带 usage
//!   - OpenAI Responses（Codex）：`response.completed` 事件带 usage
//!
//! 一条 assistant 消息写一行 JSONL，按 message id 去重；id 缺失时用
//! 「主机 + 时间戳 + 序号」兜底，保证同一进程内不重复。

use futures_util::Stream;
use http_body_util::BodyExt;
use hudsucker::hyper::body::Bytes;
use hudsucker::hyper::Response;
use hudsucker::Body;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};

/// 记账文件相对于数据目录的位置
pub const USAGE_FILE: &str = "llm-usage.jsonl";
/// 单条响应允许缓存的最大未成行字节；防止异常流把内存吃穿
const MAX_LINE_BUFFER: usize = 256 * 1024;

/* ------------------------------------------------------------------ */
/* 记录                                                                */
/* ------------------------------------------------------------------ */

/// 一条 assistant 消息的真实用量。字段是「原始口径」：
/// `input` 不含缓存，缓存读写单独给，读取方按看板口径再合并。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmUsageLine {
    pub id: String,
    pub ts: i64,
    pub host: String,
    pub path: String,
    pub model: String,
    /// 客户端标识（UA 简写，取不到时为空）
    pub client: String,
    pub status: u16,
    pub input: i64,
    pub output: i64,
    pub cache_read: i64,
    pub cache_write: i64,
    /// 从收到请求到流结束的毫秒数
    pub latency_ms: i64,
}

impl LlmUsageLine {
    fn is_empty(&self) -> bool {
        self.input == 0 && self.output == 0 && self.cache_read == 0 && self.cache_write == 0
    }
}

/* ------------------------------------------------------------------ */
/* 落盘                                                                */
/* ------------------------------------------------------------------ */

/// 追加式记账。单进程写、每次追加都重新打开文件：
/// 避免长时间持句柄（用户可能清空 / 迁移数据目录），也方便外部按增量为读。
pub struct UsageSink {
    path: PathBuf,
    seq: AtomicU64,
}

impl UsageSink {
    pub fn new(data_dir: &Path) -> Self {
        Self {
            path: data_dir.join("capture").join(USAGE_FILE),
            seq: AtomicU64::new(0),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn append(&self, line: &LlmUsageLine) {
        let Ok(mut text) = serde_json::to_string(line) else {
            return;
        };
        text.push('\n');
        if let Some(dir) = self.path.parent() {
            let _ = fs::create_dir_all(dir);
        }
        match OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
        {
            Ok(mut file) => {
                if let Err(error) = file.write_all(text.as_bytes()) {
                    log::warn!("写入 token 记账失败：{error}");
                }
            }
            Err(error) => log::warn!("打开 token 记账文件失败：{error}"),
        }
    }

    fn next_seq(&self) -> u64 {
        self.seq.fetch_add(1, Ordering::Relaxed)
    }
}

/* ------------------------------------------------------------------ */
/* 协议识别                                                            */
/* ------------------------------------------------------------------ */

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApiFlavor {
    Anthropic,
    OpenAiChat,
    OpenAiResponses,
}

/// 命中才旁路解析。`/v1/messages/count_tokens` 这类不算。
pub fn flavor_of(path: &str) -> Option<ApiFlavor> {
    let bare = path.split(['?', '#']).next().unwrap_or(path);
    let bare = bare.trim_end_matches('/');
    if bare.ends_with("/v1/messages") {
        Some(ApiFlavor::Anthropic)
    } else if bare.ends_with("/chat/completions") {
        Some(ApiFlavor::OpenAiChat)
    } else if bare.ends_with("/responses") {
        Some(ApiFlavor::OpenAiResponses)
    } else {
        None
    }
}

/* ------------------------------------------------------------------ */
/* 嗅探器                                                              */
/* ------------------------------------------------------------------ */

pub struct UsageSniffer {
    flavor: ApiFlavor,
    host: String,
    path: String,
    client: String,
    status: u16,
    started_ms: i64,
    sink: Arc<UsageSink>,
    buf: Vec<u8>,
    msg_id: String,
    model: String,
    input: i64,
    output: i64,
    cache_read: i64,
    cache_write: i64,
    emitted: bool,
}

impl UsageSniffer {
    pub fn new(
        flavor: ApiFlavor,
        host: String,
        path: String,
        client: String,
        status: u16,
        sink: Arc<UsageSink>,
    ) -> Self {
        Self {
            flavor,
            host,
            path,
            client,
            status,
            started_ms: crate::util::now_ms(),
            sink,
            buf: Vec::new(),
            msg_id: String::new(),
            model: String::new(),
            input: 0,
            output: 0,
            cache_read: 0,
            cache_write: 0,
            emitted: false,
        }
    }

    /// 旁听一段响应字节（原样转发由调用方负责，这里不改内容）
    pub fn feed(&mut self, chunk: &[u8]) {
        if self.emitted {
            return;
        }
        self.buf.extend_from_slice(chunk);
        while let Some(pos) = self.buf.iter().position(|byte| *byte == b'\n') {
            let line: Vec<u8> = self.buf.drain(..=pos).collect();
            self.on_line(&String::from_utf8_lossy(&line));
            if self.emitted {
                self.buf.clear();
                return;
            }
        }
        if self.buf.len() > MAX_LINE_BUFFER {
            self.buf.clear();
        }
    }

    /// 流结束：做最后一次解析与兜底落账
    pub fn finish(&mut self) {
        if !self.buf.is_empty() {
            let tail = std::mem::take(&mut self.buf);
            self.on_line(&String::from_utf8_lossy(&tail));
        }
        self.flush();
    }

    fn on_line(&mut self, line: &str) {
        let line = line.trim();
        if line.is_empty() || line.starts_with(':') || line.starts_with("event:") {
            return;
        }
        let payload = line.strip_prefix("data:").map(str::trim).unwrap_or(line);
        if payload == "[DONE]" || payload.is_empty() {
            return;
        }
        // 没有 usage 的块（绝大多数增量）直接跳过，避免整段 JSON 反序列化
        if !payload.contains("usage") {
            return;
        }
        let Ok(value) = serde_json::from_str::<Value>(payload) else {
            return;
        };
        self.absorb(&value);
    }

    fn absorb(&mut self, value: &Value) {
        match self.flavor {
            ApiFlavor::Anthropic => self.absorb_anthropic(value),
            ApiFlavor::OpenAiChat => self.absorb_chat(value),
            ApiFlavor::OpenAiResponses => self.absorb_responses(value),
        }
    }

    fn absorb_anthropic(&mut self, value: &Value) {
        match value.get("type").and_then(Value::as_str) {
            Some("message_start") => {
                let Some(message) = value.get("message") else {
                    return;
                };
                self.note_id(message, "id");
                self.note_model(message);
                if let Some(usage) = message.get("usage") {
                    self.take_anthropic_usage(usage);
                }
            }
            Some("message_delta") => {
                if let Some(usage) = value.get("usage") {
                    self.take_anthropic_usage(usage);
                }
                // message_delta 是最后一个带 usage 的块，先落账；
                // 后面的 message_stop 再触发是空操作
                self.flush();
            }
            Some("message_stop") => self.flush(),
            // 非流式：整条 Message 直接带 usage
            _ => {
                if value.get("usage").is_some() {
                    self.note_id(value, "id");
                    self.note_model(value);
                    self.take_anthropic_usage(&value["usage"]);
                    self.flush();
                }
            }
        }
    }

    fn take_anthropic_usage(&mut self, usage: &Value) {
        self.input = self.input.max(int_of(usage, "input_tokens"));
        self.output = self.output.max(int_of(usage, "output_tokens"));
        self.cache_read = self
            .cache_read
            .max(int_of(usage, "cache_read_input_tokens"));
        self.cache_write = self
            .cache_write
            .max(int_of(usage, "cache_creation_input_tokens"));
    }

    fn absorb_chat(&mut self, value: &Value) {
        let Some(usage) = value.get("usage").filter(|u| u.is_object()) else {
            return;
        };
        self.note_id(value, "id");
        self.note_model(value);
        let prompt = int_of(usage, "prompt_tokens");
        let cached = usage
            .get("prompt_tokens_details")
            .map(|details| int_of(details, "cached_tokens"))
            .unwrap_or(0);
        // OpenAI 的 prompt_tokens 含缓存命中，这里换成与 Anthropic 一致的「不含缓存」口径
        self.input = self.input.max((prompt - cached).max(0));
        self.output = self.output.max(int_of(usage, "completion_tokens"));
        self.cache_read = self.cache_read.max(cached);
        self.flush();
    }

    fn absorb_responses(&mut self, value: &Value) {
        // 流式：{"type":"response.completed","response":{...}}
        let node = match value.get("response") {
            Some(response) => response,
            None => value,
        };
        let Some(usage) = node.get("usage").filter(|u| u.is_object()) else {
            return;
        };
        self.note_id(node, "id");
        self.note_model(node);
        let input = int_of(usage, "input_tokens");
        let cached = usage
            .get("input_tokens_details")
            .map(|details| int_of(details, "cached_tokens"))
            .unwrap_or(0);
        self.input = self.input.max((input - cached).max(0));
        self.output = self.output.max(int_of(usage, "output_tokens"));
        self.cache_read = self.cache_read.max(cached);
        self.flush();
    }

    fn note_id(&mut self, value: &Value, key: &str) {
        if !self.msg_id.is_empty() {
            return;
        }
        if let Some(id) = value.get(key).and_then(Value::as_str) {
            if !id.trim().is_empty() {
                self.msg_id = id.trim().to_string();
            }
        }
    }

    fn note_model(&mut self, value: &Value) {
        if !self.model.is_empty() {
            return;
        }
        if let Some(model) = value.get("model").and_then(Value::as_str) {
            if !model.trim().is_empty() {
                self.model = model.trim().to_string();
            }
        }
    }

    fn flush(&mut self) {
        if self.emitted {
            return;
        }
        let mut line = LlmUsageLine {
            id: String::new(),
            ts: self.started_ms,
            host: self.host.clone(),
            path: self.path.clone(),
            model: if self.model.is_empty() {
                "(未知模型)".to_string()
            } else {
                self.model.clone()
            },
            client: self.client.clone(),
            status: self.status,
            input: self.input,
            output: self.output,
            cache_read: self.cache_read,
            cache_write: self.cache_write,
            latency_ms: (crate::util::now_ms() - self.started_ms).max(0),
        };
        if line.is_empty() {
            return;
        }
        line.id = if self.msg_id.is_empty() {
            format!("{}:{}:{}", self.host, self.started_ms, self.sink.next_seq())
        } else {
            format!("{}:{}", self.host, self.msg_id)
        };
        self.sink.append(&line);
        self.emitted = true;
    }
}

fn int_of(value: &Value, key: &str) -> i64 {
    value
        .get(key)
        .and_then(|v| {
            v.as_i64()
                .or_else(|| v.as_f64().map(|n| n as i64))
                .or_else(|| v.as_str().and_then(|s| s.trim().parse::<i64>().ok()))
        })
        .unwrap_or(0)
}

/// UA 太长没意义，只留客户端名（`Claude/1.2 (macOS)` → `Claude`）
pub fn client_name(user_agent: &str) -> String {
    let head = user_agent.split_whitespace().next().unwrap_or("");
    let name = head.split('/').next().unwrap_or("").trim();
    name.chars().take(40).collect()
}

/* ------------------------------------------------------------------ */
/* 与抓包管道的接线                                                    */
/* ------------------------------------------------------------------ */

/// 把响应体分叉：一边原样转发给客户端（保留流式时延），一边喂嗅探器。
/// 只有在调用方确认「命中 LLM 接口且正在录制」时才会走到这里。
pub fn tap_response(
    res: Response<Body>,
    flavor: ApiFlavor,
    host: String,
    path: String,
    client: String,
    status: u16,
    sink: Arc<UsageSink>,
) -> Response<Body> {
    let sniffer = UsageSniffer::new(flavor, host, path, client, status, sink);
    let (parts, body) = res.into_parts();
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Bytes, hudsucker::Error>>(16);
    // 泵任务只持有响应体，客户端断开或流结束即自行退出
    tokio::spawn(async move {
        let mut body = body;
        let mut sniffer = sniffer;
        while let Some(frame) = body.frame().await {
            match frame {
                Ok(frame) => {
                    let Some(data) = frame.data_ref() else {
                        continue; // trailers 不参与解析
                    };
                    sniffer.feed(data);
                    if tx.send(Ok(data.clone())).await.is_err() {
                        return; // 客户端断开，停止旁听
                    }
                }
                Err(error) => {
                    let _ = tx.send(Err(error)).await;
                    return;
                }
            }
        }
        sniffer.finish();
    });
    Response::from_parts(parts, Body::from_stream(ChannelStream(rx)))
}

/// 非流式响应（JSON 整包）在缓冲区里直接解析，不额外拷贝
pub fn sniff_buffered(
    flavor: ApiFlavor,
    body: &[u8],
    host: &str,
    path: &str,
    client: &str,
    status: u16,
    sink: &Arc<UsageSink>,
) {
    let Ok(value) = serde_json::from_slice::<Value>(body) else {
        return;
    };
    let mut sniffer = UsageSniffer::new(
        flavor,
        host.to_string(),
        path.to_string(),
        client.to_string(),
        status,
        sink.clone(),
    );
    sniffer.absorb(&value);
    sniffer.finish();
}

/// mpsc → Stream 的适配层。`Receiver` 在 `T: Send` 时是 `Sync` 的，
/// 满足 `Body::from_stream` 对 `Send + Sync` 的要求。
struct ChannelStream(tokio::sync::mpsc::Receiver<Result<Bytes, hudsucker::Error>>);

impl Stream for ChannelStream {
    type Item = Result<Bytes, hudsucker::Error>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.get_mut().0.poll_recv(cx)
    }
}

/* ------------------------------------------------------------------ */
/* 单测                                                                */
/* ------------------------------------------------------------------ */

#[cfg(test)]
mod tests {
    use super::*;
    use hudsucker::hyper::StatusCode;

    fn temp_sink(tag: &str) -> (Arc<UsageSink>, PathBuf) {
        let dir =
            std::env::temp_dir().join(format!("astroman-llm-usage-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        (Arc::new(UsageSink::new(&dir)), dir)
    }

    fn read_lines(dir: &Path) -> Vec<LlmUsageLine> {
        let path = dir.join("capture").join(USAGE_FILE);
        let Ok(text) = fs::read_to_string(path) else {
            return Vec::new();
        };
        text.lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect()
    }

    fn sniffer(flavor: ApiFlavor, sink: Arc<UsageSink>) -> UsageSniffer {
        UsageSniffer::new(
            flavor,
            "api.deepseek.com:443".to_string(),
            "/anthropic/v1/messages?beta=true".to_string(),
            "Claude".to_string(),
            StatusCode::OK.as_u16(),
            sink,
        )
    }

    #[test]
    fn flavor_matches_llm_endpoints_only() {
        assert_eq!(
            flavor_of("/anthropic/v1/messages"),
            Some(ApiFlavor::Anthropic)
        );
        assert_eq!(flavor_of("/v1/messages?beta=1"), Some(ApiFlavor::Anthropic));
        assert_eq!(flavor_of("/v1/messages/count_tokens"), None);
        assert_eq!(
            flavor_of("/v1/chat/completions"),
            Some(ApiFlavor::OpenAiChat)
        );
        assert_eq!(flavor_of("/v1/responses"), Some(ApiFlavor::OpenAiResponses));
        assert_eq!(flavor_of("/v1/models"), None);
    }

    #[test]
    fn anthropic_stream_takes_final_totals() {
        let (sink, dir) = temp_sink("anthropic");
        let mut s = sniffer(ApiFlavor::Anthropic, sink);
        let sse = concat!(
            "event: message_start\n",
            "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"model\":\"deepseek-chat\",\"usage\":{\"input_tokens\":6,\"cache_creation_input_tokens\":0,\"cache_read_input_tokens\":0,\"output_tokens\":0}}}\n\n",
            "event: content_block_delta\n",
            "data: {\"type\":\"content_block_delta\",\"delta\":{\"text\":\"字\"}}\n\n",
            "event: message_delta\n",
            "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"input_tokens\":6,\"cache_creation_input_tokens\":0,\"cache_read_input_tokens\":0,\"output_tokens\":2}}\n\n",
            "event: message_stop\n",
            "data: {\"type\":\"message_stop\"}\n\n",
        );
        // 分片喂，验证跨 chunk 的半行不会丢
        for chunk in sse.as_bytes().chunks(7) {
            s.feed(chunk);
        }
        s.finish();
        let lines = read_lines(&dir);
        assert_eq!(lines.len(), 1, "一条消息只记一行");
        assert_eq!(lines[0].id, "api.deepseek.com:443:msg_1");
        assert_eq!(lines[0].model, "deepseek-chat");
        assert_eq!(lines[0].input, 6);
        assert_eq!(lines[0].output, 2);
        assert_eq!(lines[0].client, "Claude");
    }

    #[test]
    fn anthropic_cache_fields_kept_separate() {
        let (sink, dir) = temp_sink("anthropic-cache");
        let mut s = sniffer(ApiFlavor::Anthropic, sink);
        s.feed(br#"data: {"type":"message_start","message":{"id":"msg_2","model":"claude-sonnet-4-5","usage":{"input_tokens":12,"cache_creation_input_tokens":300,"cache_read_input_tokens":9000,"output_tokens":0}}}"#);
        s.feed(b"\n");
        s.feed(br#"data: {"type":"message_delta","usage":{"output_tokens":41}}"#);
        s.feed(b"\n");
        s.finish();
        let lines = read_lines(&dir);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].input, 12, "输入不含缓存");
        assert_eq!(lines[0].cache_write, 300);
        assert_eq!(lines[0].cache_read, 9000);
        assert_eq!(lines[0].output, 41);
    }

    #[test]
    fn openai_chat_subtracts_cached_tokens() {
        let (sink, dir) = temp_sink("chat");
        let mut s = UsageSniffer::new(
            ApiFlavor::OpenAiChat,
            "api.qnaigc.com:443".to_string(),
            "/v1/chat/completions".to_string(),
            "Codex".to_string(),
            200,
            sink,
        );
        s.feed(br#"data: {"id":"chatcmpl-9","model":"gpt-5.6-sol","usage":{"prompt_tokens":1000,"completion_tokens":80,"prompt_tokens_details":{"cached_tokens":768}}}"#);
        s.feed(b"\n");
        s.feed(b"data: [DONE]\n");
        s.finish();
        let lines = read_lines(&dir);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].input, 232, "prompt_tokens 含缓存，要减掉");
        assert_eq!(lines[0].cache_read, 768);
        assert_eq!(lines[0].output, 80);
    }

    #[test]
    fn responses_event_and_non_stream_both_work() {
        let (sink, dir) = temp_sink("responses");
        let mut stream = UsageSniffer::new(
            ApiFlavor::OpenAiResponses,
            "api.qnaigc.com:443".to_string(),
            "/v1/responses".to_string(),
            String::new(),
            200,
            sink.clone(),
        );
        stream.feed(br#"data: {"type":"response.completed","response":{"id":"resp_7","model":"gpt-5.6-sol","usage":{"input_tokens":500,"output_tokens":60,"input_tokens_details":{"cached_tokens":128}}}}"#);
        stream.feed(b"\n");
        stream.finish();

        sniff_buffered(
            ApiFlavor::OpenAiResponses,
            br#"{"id":"resp_8","object":"response","model":"gpt-5.6-sol","usage":{"input_tokens":300,"output_tokens":20,"input_tokens_details":{"cached_tokens":0}}}"#,
            "api.qnaigc.com:443",
            "/v1/responses",
            "",
            200,
            &sink,
        );

        let lines = read_lines(&dir);
        assert_eq!(lines.len(), 2, "流式与非流式各记一条");
        assert_eq!(lines[0].input, 372);
        assert_eq!(lines[0].cache_read, 128);
        assert_eq!(lines[0].output, 60);
        assert_eq!(lines[1].input, 300);
        assert_eq!(lines[1].output, 20);
    }

    #[test]
    fn missing_id_falls_back_to_host_and_seq() {
        let (sink, dir) = temp_sink("no-id");
        let mut s = sniffer(ApiFlavor::OpenAiChat, sink);
        // 没有 id / model 的异常包，仍应按兜底 id 记账
        s.feed(br#"data: {"usage":{"prompt_tokens":10,"completion_tokens":5}}"#);
        s.feed(b"\n");
        s.finish();
        let lines = read_lines(&dir);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].id.starts_with("api.deepseek.com:443:"));
        assert_eq!(lines[0].model, "(未知模型)");
        assert_eq!(lines[0].input, 10);
        assert_eq!(lines[0].output, 5);
    }

    #[test]
    fn empty_stream_writes_nothing() {
        let (sink, dir) = temp_sink("empty");
        let mut s = sniffer(ApiFlavor::Anthropic, sink);
        s.feed(b"\n");
        s.feed(br#"data: {"type":"content_block_delta","delta":{"text":"hi"}}"#);
        s.feed(b"\n");
        s.finish();
        assert!(read_lines(&dir).is_empty(), "没有 usage 不应留下记录");
    }

    #[test]
    fn client_name_uses_product_token() {
        assert_eq!(client_name("Claude/1.2.3 (macOS 15.0)"), "Claude");
        assert_eq!(client_name("codex_cli_rs/0.51.0"), "codex_cli_rs");
        assert_eq!(client_name(""), "");
    }
}
