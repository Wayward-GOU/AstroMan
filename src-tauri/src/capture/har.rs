//! 把当前会话导出成 HAR 1.2，方便导入 Chrome DevTools / Charles / Postman 对照。
//!
//! 导出走流式写法：调用方分批取流量，本模块用 `serde_json::to_writer` 逐条
//! 落盘，不再先拼一份完整的 JSON 字符串，避免满载时内存翻倍。
//!
//! 只导出内存里现存的流量；body 若是二进制且没有 base64 预览，只保留大小。
//! `metadata_only` 为 true 时只保留大小 / 类型 / 头部，不写请求与响应正文。

use crate::capture::store::{BodyPayload, FlowDetail, HeaderEntry};
use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::{json, Map, Value};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

const PREFIX: &str = concat!(
    "{\"log\":{\"version\":\"1.2\",\"creator\":{\"name\":\"AstroMan\",\"version\":\"",
    env!("CARGO_PKG_VERSION"),
    "\"},\"entries\":["
);
const SUFFIX: &str = "]}}";

/// 逐条写 HAR 的写入器，供文件导出与 ZIP 内文本共用。
pub struct HarWriter<W: Write> {
    writer: W,
    first: bool,
}

impl<W: Write> HarWriter<W> {
    pub fn new(writer: W) -> Result<Self, String> {
        let mut writer = writer;
        writer.write_all(PREFIX.as_bytes()).map_err(io_error)?;
        Ok(Self {
            writer,
            first: true,
        })
    }

    pub fn write_flow(&mut self, flow: &FlowDetail, metadata_only: bool) -> Result<(), String> {
        if !self.first {
            self.writer.write_all(b",").map_err(io_error)?;
        }
        self.first = false;
        serde_json::to_writer(&mut self.writer, &entry(flow, metadata_only))
            .map_err(|error| format!("序列化 HAR 失败：{error}"))
    }

    pub fn finish(mut self) -> Result<(), String> {
        self.writer.write_all(SUFFIX.as_bytes()).map_err(io_error)?;
        self.writer.flush().map_err(io_error)
    }
}

fn io_error(error: std::io::Error) -> String {
    format!("写入 HAR 失败：{error}")
}

/// 流式导出到文件：`emit` 里逐条调用 `write_flow`。
pub fn export_stream<F>(path: &Path, emit: F) -> Result<(), String>
where
    F: FnOnce(&mut HarWriter<BufWriter<File>>) -> Result<(), String>,
{
    let file = File::create(path).map_err(io_error)?;
    let mut writer = HarWriter::new(BufWriter::new(file))?;
    emit(&mut writer)?;
    writer.finish()
}

/// 生成 HAR 文本，供 ZIP 内的 capture.har 共用。
pub fn render(flows: &[FlowDetail]) -> Result<String, String> {
    let mut buffer = Vec::new();
    {
        let mut writer = HarWriter::new(&mut buffer)?;
        for flow in flows {
            writer.write_flow(flow, false)?;
        }
        writer.finish()?;
    }
    String::from_utf8(buffer).map_err(|error| format!("序列化 HAR 失败：{error}"))
}

fn entry(flow: &FlowDetail, metadata_only: bool) -> Value {
    let url = format!(
        "{}://{}{}",
        flow.brief.scheme, flow.brief.host, flow.brief.path
    );
    let started = DateTime::<Utc>::from_timestamp_millis(flow.brief.started_at)
        .unwrap_or_else(Utc::now)
        .to_rfc3339_opts(SecondsFormat::Millis, true);
    let duration = flow.brief.duration_ms.max(0);

    let mut request = Map::new();
    request.insert("method".into(), json!(flow.brief.method));
    request.insert("url".into(), json!(url));
    request.insert("httpVersion".into(), json!(flow.brief.http_version));
    request.insert("headers".into(), json!(headers(&flow.request_headers)));
    request.insert("queryString".into(), json!(query(&flow.brief.path)));
    request.insert("cookies".into(), json!([]));
    request.insert("headersSize".into(), json!(-1));
    request.insert("bodySize".into(), json!(flow.brief.request_size));
    if let Some(post) = post_data(&flow.request_body, metadata_only) {
        request.insert("postData".into(), post);
    }

    let mut content = Map::new();
    content.insert("size".into(), json!(flow.response_body.size));
    if !flow.response_body.content_type.is_empty() {
        content.insert("mimeType".into(), json!(flow.response_body.content_type));
    }
    if !metadata_only {
        if let Some(text) = &flow.response_body.text {
            content.insert("text".into(), json!(text));
        } else if let Some(base64) = &flow.response_body.base64 {
            content.insert("text".into(), json!(base64));
            content.insert("encoding".into(), json!("base64"));
        }
    }

    let mut response = Map::new();
    response.insert("status".into(), json!(flow.brief.status.unwrap_or(0)));
    response.insert("statusText".into(), json!(""));
    response.insert("httpVersion".into(), json!(flow.brief.http_version));
    response.insert("headers".into(), json!(headers(&flow.response_headers)));
    response.insert("cookies".into(), json!([]));
    response.insert("content".into(), Value::Object(content));
    response.insert("redirectURL".into(), json!(""));
    response.insert("headersSize".into(), json!(-1));
    response.insert("bodySize".into(), json!(flow.brief.response_size));

    let mut entry = Map::new();
    entry.insert("startedDateTime".into(), json!(started));
    entry.insert("time".into(), json!(duration));
    entry.insert("request".into(), Value::Object(request));
    entry.insert("response".into(), Value::Object(response));
    entry.insert("cache".into(), json!({}));
    entry.insert(
        "timings".into(),
        json!({ "send": 0, "wait": duration, "receive": 0 }),
    );
    if let Some(error) = &flow.brief.error {
        entry.insert("comment".into(), json!(error));
    }
    Value::Object(entry)
}

fn headers(list: &[HeaderEntry]) -> Vec<Value> {
    list.iter()
        .map(|header| json!({ "name": header.name, "value": header.value }))
        .collect()
}

fn query(path: &str) -> Vec<Value> {
    let Some((_, query)) = path.split_once('?') else {
        return Vec::new();
    };
    query
        .split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| {
            let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
            json!({ "name": name, "value": value })
        })
        .collect()
}

fn post_data(body: &BodyPayload, metadata_only: bool) -> Option<Value> {
    let mut post = Map::new();
    let mime = if body.content_type.is_empty() {
        "application/octet-stream"
    } else {
        body.content_type.as_str()
    };
    post.insert("mimeType".into(), json!(mime));
    if !metadata_only {
        if let Some(text) = &body.text {
            post.insert("text".into(), json!(text));
        } else if let Some(base64) = &body.base64 {
            post.insert("text".into(), json!(base64));
            post.insert("encoding".into(), json!("base64"));
        } else if body.size == 0 {
            return None;
        }
    } else if body.size == 0 {
        return None;
    }
    Some(Value::Object(post))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::store::{BodyPayload, FlowBrief};

    fn sample() -> FlowDetail {
        FlowDetail {
            brief: FlowBrief {
                id: 1,
                started_at: 1_700_000_000_000,
                method: "POST".into(),
                scheme: "http".into(),
                host: "example.com".into(),
                path: "/login?next=%2Fhome".into(),
                http_version: "HTTP/1.1".into(),
                status: Some(200),
                duration_ms: 12,
                request_size: 4,
                response_size: 5,
                content_type: "application/json".into(),
                client_addr: String::new(),
                error: None,
                mapped: false,
                modified: false,
                remote: None,
            },
            request_headers: vec![HeaderEntry {
                name: "Content-Type".into(),
                value: "application/json".into(),
            }],
            response_headers: Vec::new(),
            request_body: BodyPayload {
                size: 4,
                text: Some("ping".into()),
                content_type: "application/json".into(),
                ..BodyPayload::default()
            },
            response_body: BodyPayload {
                size: 5,
                text: Some("pong!".into()),
                content_type: "application/json".into(),
                ..BodyPayload::default()
            },
        }
    }

    #[test]
    fn streaming_output_matches_render_and_can_omit_bodies() {
        let flow = sample();
        let text = render(std::slice::from_ref(&flow)).unwrap();
        let value: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["log"]["entries"].as_array().unwrap().len(), 1);
        assert_eq!(
            value["log"]["entries"][0]["response"]["content"]["text"],
            "pong!"
        );

        let dir = crate::util::test_dir("capture-har");
        let path = dir.join("meta.har");
        export_stream(&path, |writer| writer.write_flow(&flow, true)).unwrap();
        let meta = std::fs::read_to_string(&path).unwrap();
        let meta_value: Value = serde_json::from_str(&meta).unwrap();
        let entry = &meta_value["log"]["entries"][0];
        assert_eq!(entry["response"]["content"]["size"], 5);
        assert!(entry["response"]["content"].get("text").is_none());
        assert!(entry["request"]["postData"].get("text").is_none());
        let _ = std::fs::remove_dir_all(dir);
    }
}
