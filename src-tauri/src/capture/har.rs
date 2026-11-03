//! 把当前会话导出成 HAR 1.2，方便导入 Chrome DevTools / Charles / Postman 对照。
//!
//! 只导出内存里现存的流量；body 若是二进制且没有 base64 预览，只保留大小。

use crate::capture::store::{BodyPayload, FlowDetail, HeaderEntry};
use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::{json, Map, Value};
use std::path::Path;

pub fn export(path: &Path, flows: &[FlowDetail]) -> Result<(), String> {
    let text = render(flows)?;
    std::fs::write(path, text).map_err(|error| format!("写入 HAR 失败：{error}"))
}

/// 生成 HAR 文本，供独立 .har 文件与 ZIP 内的 capture.har 共用。
pub fn render(flows: &[FlowDetail]) -> Result<String, String> {
    let entries: Vec<Value> = flows.iter().map(entry).collect();
    let har = json!({
        "log": {
            "version": "1.2",
            "creator": {
                "name": "AstroMan",
                "version": env!("CARGO_PKG_VERSION")
            },
            "entries": entries
        }
    });
    serde_json::to_string_pretty(&har).map_err(|error| format!("序列化 HAR 失败：{error}"))
}

fn entry(flow: &FlowDetail) -> Value {
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
    if let Some(post) = post_data(&flow.request_body) {
        request.insert("postData".into(), post);
    }

    let mut content = Map::new();
    content.insert("size".into(), json!(flow.response_body.size));
    if !flow.response_body.content_type.is_empty() {
        content.insert("mimeType".into(), json!(flow.response_body.content_type));
    }
    if let Some(text) = &flow.response_body.text {
        content.insert("text".into(), json!(text));
    } else if let Some(base64) = &flow.response_body.base64 {
        content.insert("text".into(), json!(base64));
        content.insert("encoding".into(), json!("base64"));
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

fn post_data(body: &BodyPayload) -> Option<Value> {
    let mut post = Map::new();
    let mime = if body.content_type.is_empty() {
        "application/octet-stream"
    } else {
        body.content_type.as_str()
    };
    post.insert("mimeType".into(), json!(mime));
    if let Some(text) = &body.text {
        post.insert("text".into(), json!(text));
    } else if let Some(base64) = &body.base64 {
        post.insert("text".into(), json!(base64));
        post.insert("encoding".into(), json!("base64"));
    } else if body.size == 0 {
        return None;
    }
    Some(Value::Object(post))
}
