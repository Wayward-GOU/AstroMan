//! 把当前流量导出成 ZIP：索引页、HAR、汇总与每条流量的可读文本。
//!
//! 目录结构固定，用户解压后可以直接用浏览器或文本编辑器查看：
//!
//! ```text
//! astroman-capture-20261103-143000/
//!   index.html
//!   README.txt
//!   summary.json
//!   capture.har
//!   flows/0001-GET-example.com/
//!     meta.json
//!     request.txt
//!     response.txt
//!     response.body.png   (仅图片 / 二进制)
//! ```

use crate::capture::har;
use crate::capture::store::{BodyPayload, FlowDetail};
use base64::Engine;
use chrono::{DateTime, Local, SecondsFormat, Utc};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

pub fn export(path: &Path, flows: &[FlowDetail]) -> Result<(), String> {
    let result = write_archive(path, flows);
    if result.is_err() {
        // 写一半失败时不要留下打不开的半成品
        let _ = std::fs::remove_file(path);
    }
    result
}

struct Entry<'a> {
    seq: usize,
    folder: String,
    flow: &'a FlowDetail,
}

struct Summary {
    flow_count: usize,
    hosts: BTreeMap<String, usize>,
    total_request: u64,
    total_response: u64,
}

fn write_archive(path: &Path, flows: &[FlowDetail]) -> Result<(), String> {
    let file = std::fs::File::create(path)
        .map_err(|error| format!("E_ARCHIVE: 创建 ZIP 失败：{error}"))?;
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

    let now = Local::now();
    let root = format!("astroman-capture-{}", now.format("%Y%m%d-%H%M%S"));
    let entries: Vec<Entry> = flows
        .iter()
        .enumerate()
        .map(|(index, flow)| Entry {
            seq: index + 1,
            folder: format!(
                "{:04}-{}-{}",
                index + 1,
                safe_segment(&flow.brief.method, 8),
                safe_segment(&flow.brief.host, 48)
            ),
            flow,
        })
        .collect();
    let summary = summarize(&entries);

    write_file(
        &mut zip,
        &format!("{root}/index.html"),
        index_html(&entries, &summary, now).as_bytes(),
        options,
    )?;
    write_file(
        &mut zip,
        &format!("{root}/README.txt"),
        readme(&summary, now).as_bytes(),
        options,
    )?;
    write_file(
        &mut zip,
        &format!("{root}/summary.json"),
        summary_json(&summary, now).to_string().as_bytes(),
        options,
    )?;

    let har_text = har::render(flows)?;
    write_file(
        &mut zip,
        &format!("{root}/capture.har"),
        har_text.as_bytes(),
        options,
    )?;
    zip.add_directory(format!("{root}/flows/"), options)
        .map_err(|error| format!("E_ARCHIVE: 创建 flows 目录失败：{error}"))?;

    for entry in &entries {
        let dir = format!("{root}/flows/{}", entry.folder);
        let request_raw = raw_body("request", &entry.flow.request_body);
        let response_raw = raw_body("response", &entry.flow.response_body);

        write_file(
            &mut zip,
            &format!("{dir}/request.txt"),
            request_text(
                entry.flow,
                request_raw.as_ref().map(|(name, _)| name.as_str()),
            )
            .as_bytes(),
            options,
        )?;
        write_file(
            &mut zip,
            &format!("{dir}/response.txt"),
            response_text(
                entry.flow,
                response_raw.as_ref().map(|(name, _)| name.as_str()),
            )
            .as_bytes(),
            options,
        )?;
        write_file(
            &mut zip,
            &format!("{dir}/meta.json"),
            meta_json(
                entry.flow,
                request_raw.as_ref().map(|(name, _)| name.as_str()),
                response_raw.as_ref().map(|(name, _)| name.as_str()),
            )
            .to_string()
            .as_bytes(),
            options,
        )?;

        if let Some((name, bytes)) = request_raw {
            write_file(&mut zip, &format!("{dir}/{name}"), &bytes, options)?;
        }
        if let Some((name, bytes)) = response_raw {
            write_file(&mut zip, &format!("{dir}/{name}"), &bytes, options)?;
        }
    }

    zip.finish()
        .map(|_| ())
        .map_err(|error| format!("E_ARCHIVE: 完成 ZIP 失败：{error}"))
}

fn write_file(
    zip: &mut ZipWriter<std::fs::File>,
    name: &str,
    data: &[u8],
    options: SimpleFileOptions,
) -> Result<(), String> {
    zip.start_file(name, options)
        .map_err(|error| format!("E_ARCHIVE: 写入 {name} 失败：{error}"))?;
    zip.write_all(data)
        .map_err(|error| format!("E_ARCHIVE: 写入 {name} 失败：{error}"))
}

fn summarize(entries: &[Entry]) -> Summary {
    let mut hosts = BTreeMap::new();
    let mut total_request = 0;
    let mut total_response = 0;
    for entry in entries {
        *hosts.entry(entry.flow.brief.host.clone()).or_insert(0) += 1;
        total_request += entry.flow.brief.request_size;
        total_response += entry.flow.brief.response_size;
    }
    Summary {
        flow_count: entries.len(),
        hosts,
        total_request,
        total_response,
    }
}

fn summary_json(summary: &Summary, now: DateTime<Local>) -> Value {
    let hosts: Vec<Value> = summary
        .hosts
        .iter()
        .map(|(host, count)| json!({ "host": host, "count": count }))
        .collect();
    json!({
        "app": "AstroMan",
        "version": env!("CARGO_PKG_VERSION"),
        "exportedAt": now.to_rfc3339_opts(SecondsFormat::Millis, false),
        "flowCount": summary.flow_count,
        "hostCount": summary.hosts.len(),
        "totalRequestBytes": summary.total_request,
        "totalResponseBytes": summary.total_response,
        "hosts": hosts,
        "layout": {
            "index": "index.html",
            "har": "capture.har",
            "flows": "flows/"
        }
    })
}

fn readme(summary: &Summary, now: DateTime<Local>) -> String {
    format!(
        "AstroMan 抓包导出\r\n\
         ==================\r\n\r\n\
         导出时间：{}\r\n\
         流量条数：{}\r\n\
         域名数量：{}\r\n\r\n\
         index.html    双击用浏览器打开，可查看汇总并跳转到每条流量\r\n\
         capture.har   标准 HAR，可导入 Chrome DevTools / Charles / Postman\r\n\
         summary.json  机器可读的条数、域名与体量汇总\r\n\
         flows/        每条流量一个目录：meta.json、request.txt、response.txt\r\n\
                       图片或二进制 body 会额外保存为 request.body.* / response.body.*\r\n\r\n\
         注意：抓包内容只来自导出时的内存，未落库；二进制且未保留内容的 body 只能在 meta.json 看到大小与类型。\r\n",
        now.format("%Y-%m-%d %H:%M:%S"),
        summary.flow_count,
        summary.hosts.len(),
    )
}

fn index_html(entries: &[Entry], summary: &Summary, now: DateTime<Local>) -> String {
    let mut rows = String::new();
    for entry in entries {
        let flow = entry.flow;
        let url = url_of(flow);
        let status_text = flow
            .brief
            .status
            .map(|status| status.to_string())
            .unwrap_or_else(|| "—".into());
        let status_class = status_class(flow.brief.status);
        rows.push_str(&format!(
            "<tr>\
               <td class=\"num\">{seq}</td>\
               <td class=\"nowrap\">{time}</td>\
               <td><span class=\"method\">{method}</span></td>\
               <td><span class=\"status {status_class}\">{status}</span></td>\
               <td class=\"num\">{duration} ms</td>\
               <td class=\"num\">{size}</td>\
               <td class=\"url\" title=\"{url_attr}\">{url_text}</td>\
               <td class=\"links nowrap\">\
                 <a href=\"flows/{folder}/request.txt\">请求</a>\
                 <a href=\"flows/{folder}/response.txt\">响应</a>\
                 <a href=\"flows/{folder}/meta.json\">元数据</a>\
               </td>\
             </tr>",
            seq = entry.seq,
            time = escape(&local_time(flow.brief.started_at)),
            method = escape(&flow.brief.method),
            status_class = status_class,
            status = escape(&status_text),
            duration = flow.brief.duration_ms.max(0),
            size = human_size(flow.brief.request_size + flow.brief.response_size),
            url_attr = escape(&url),
            url_text = escape(&url),
            folder = entry.folder,
        ));
    }

    format!(
        "<!doctype html>\
<html lang=\"zh-CN\">\
<head>\
<meta charset=\"utf-8\">\
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
<title>AstroMan 抓包导出</title>\
<style>\
:root{{color-scheme:light dark}}\
*{{box-sizing:border-box}}\
body{{margin:0;padding:28px 32px 48px;background:#f6f7f9;color:#1f2329;font:13px/1.6 -apple-system,BlinkMacSystemFont,'Segoe UI','Microsoft YaHei',sans-serif}}\
h1{{margin:0 0 4px;font-size:22px}}\
.muted{{margin:0;color:#8a9099}}\
.cards{{display:grid;grid-template-columns:repeat(auto-fit,minmax(150px,1fr));gap:12px;margin:20px 0 18px}}\
.card{{padding:14px 16px;border:1px solid #e3e6eb;border-radius:8px;background:#fff}}\
.card b{{display:block;margin-top:4px;font-size:20px;font-weight:600}}\
.card span{{color:#8a9099;font-size:12px}}\
.tip{{margin:0 0 12px;color:#5c6370}}\
table{{width:100%;border-collapse:collapse;border:1px solid #e3e6eb;border-radius:8px;overflow:hidden;background:#fff}}\
th,td{{padding:8px 10px;border-bottom:1px solid #eef0f3;text-align:left;vertical-align:middle}}\
th{{background:#fafbfc;color:#5c6370;font-weight:500;white-space:nowrap}}\
tr:last-child td{{border-bottom:none}}\
tr:hover td{{background:#f8fafc}}\
.num{{text-align:right;font-variant-numeric:tabular-nums;white-space:nowrap}}\
.nowrap{{white-space:nowrap}}\
.method{{display:inline-block;min-width:46px;font-weight:600;font-family:ui-monospace,Consolas,monospace}}\
.status{{display:inline-block;min-width:38px;padding:0 6px;border-radius:4px;text-align:center;font-variant-numeric:tabular-nums}}\
.status.ok{{background:#e8f7ee;color:#1a7f45}}\
.status.warn{{background:#fff6e5;color:#a66a00}}\
.status.err{{background:#fdecec;color:#c0392b}}\
.status.none{{background:#eef0f3;color:#8a9099}}\
.url{{max-width:520px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;font-family:ui-monospace,Consolas,monospace;font-size:12px}}\
.links a{{margin-right:10px;color:#0b66c3;text-decoration:none}}\
.links a:hover{{text-decoration:underline}}\
footer{{margin-top:18px;color:#8a9099;font-size:12px}}\
@media (prefers-color-scheme:dark){{\
body{{background:#15181d;color:#e6e8eb}}\
.muted,.card span,.tip,footer,th{{color:#9aa1ab}}\
.card,table{{background:#1d2127;border-color:#2c323a}}\
th{{background:#20252c}}\
th,td{{border-color:#2c323a}}\
tr:hover td{{background:#242a32}}\
.status.ok{{background:#153b28;color:#66d99a}}\
.status.warn{{background:#3d2f14;color:#f0bc5e}}\
.status.err{{background:#3e1e1e;color:#ff8a80}}\
.status.none{{background:#2a3038;color:#9aa1ab}}\
.links a{{color:#6cb6ff}}\
}}\
</style>\
</head>\
<body>\
<header>\
<h1>AstroMan 抓包导出</h1>\
<p class=\"muted\">导出时间 {time} · 共 {count} 条流量 · {hosts} 个域名</p>\
</header>\
<section class=\"cards\">\
<div class=\"card\"><span>流量条数</span><b>{count}</b></div>\
<div class=\"card\"><span>域名数量</span><b>{hosts}</b></div>\
<div class=\"card\"><span>请求体量</span><b>{request_size}</b></div>\
<div class=\"card\"><span>响应体量</span><b>{response_size}</b></div>\
</section>\
<p class=\"tip\">点「请求 / 响应」直接打开文本内容；图片与二进制在对应 flow 目录下另存。标准 HAR 在根目录 <code>capture.har</code>。</p>\
<table>\
<thead><tr><th>#</th><th>时间</th><th>方法</th><th>状态</th><th>耗时</th><th>大小</th><th>URL</th><th>明细</th></tr></thead>\
<tbody>{rows}</tbody>\
</table>\
<footer>由 AstroMan v{version} 导出 · 静态页面，可直接分享或归档</footer>\
</body>\
</html>",
        time = now.format("%Y-%m-%d %H:%M:%S"),
        count = summary.flow_count,
        hosts = summary.hosts.len(),
        request_size = human_size(summary.total_request),
        response_size = human_size(summary.total_response),
        rows = rows,
        version = env!("CARGO_PKG_VERSION"),
    )
}

fn request_text(flow: &FlowDetail, raw: Option<&str>) -> String {
    let mut out = format!(
        "{} {} {}\r\n",
        flow.brief.method, flow.brief.path, flow.brief.http_version
    );
    for header in &flow.request_headers {
        out.push_str(&format!("{}: {}\r\n", header.name, header.value));
    }
    out.push_str("\r\n");
    push_body(&mut out, &flow.request_body, raw);
    out
}

fn response_text(flow: &FlowDetail, raw: Option<&str>) -> String {
    let mut out = match flow.brief.status {
        Some(status) => format!("{} {}\r\n", flow.brief.http_version, status),
        None => format!("{} (no response)\r\n", flow.brief.http_version),
    };
    for header in &flow.response_headers {
        out.push_str(&format!("{}: {}\r\n", header.name, header.value));
    }
    out.push_str("\r\n");
    push_body(&mut out, &flow.response_body, raw);
    if let Some(error) = &flow.brief.error {
        out.push_str(&format!("\r\n[error] {error}\r\n"));
    }
    out
}

fn push_body(out: &mut String, body: &BodyPayload, raw: Option<&str>) {
    if let Some(text) = &body.text {
        out.push_str(text);
        if body.truncated {
            out.push_str(&format!(
                "\r\n\r\n[body truncated: original {} bytes, only the prefix above is stored]",
                body.size
            ));
        }
    } else if body.size == 0 {
        // 没有 body，保持空白
    } else if let Some(name) = raw {
        out.push_str(&format!(
            "[binary body: {} bytes, {}; raw file: {}]",
            body.size, body.content_type, name
        ));
    } else {
        out.push_str(&format!(
            "[binary body not stored: {} bytes, {}]",
            body.size, body.content_type
        ));
    }
}

fn meta_json(flow: &FlowDetail, request_raw: Option<&str>, response_raw: Option<&str>) -> Value {
    json!({
        "id": flow.brief.id,
        "startedAt": rfc3339_local(flow.brief.started_at),
        "startedAtMs": flow.brief.started_at,
        "method": flow.brief.method,
        "url": url_of(flow),
        "httpVersion": flow.brief.http_version,
        "status": flow.brief.status,
        "durationMs": flow.brief.duration_ms,
        "requestSize": flow.brief.request_size,
        "responseSize": flow.brief.response_size,
        "contentType": flow.brief.content_type,
        "clientAddr": flow.brief.client_addr,
        "error": flow.brief.error,
        "request": {
            "headers": flow.request_headers,
            "body": body_meta(&flow.request_body, request_raw),
        },
        "response": {
            "headers": flow.response_headers,
            "body": body_meta(&flow.response_body, response_raw),
        },
    })
}

fn body_meta(body: &BodyPayload, raw: Option<&str>) -> Value {
    json!({
        "size": body.size,
        "truncated": body.truncated,
        "binary": body.binary,
        "contentType": body.content_type,
        "textStored": body.text.is_some(),
        "base64Stored": body.base64.is_some(),
        "rawFile": raw,
    })
}

fn raw_body(part: &str, body: &BodyPayload) -> Option<(String, Vec<u8>)> {
    let encoded = body.base64.as_ref()?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .ok()?;
    Some((
        format!("{part}.body.{}", extension_for(&body.content_type)),
        bytes,
    ))
}

fn extension_for(content_type: &str) -> &'static str {
    let content_type = content_type.to_ascii_lowercase();
    if content_type.contains("png") {
        "png"
    } else if content_type.contains("jpeg") || content_type.contains("jpg") {
        "jpg"
    } else if content_type.contains("gif") {
        "gif"
    } else if content_type.contains("webp") {
        "webp"
    } else if content_type.contains("svg") {
        "svg"
    } else if content_type.contains("pdf") {
        "pdf"
    } else if content_type.contains("json") {
        "json"
    } else if content_type.contains("html") {
        "html"
    } else if content_type.contains("xml") {
        "xml"
    } else {
        "bin"
    }
}

fn url_of(flow: &FlowDetail) -> String {
    format!(
        "{}://{}{}",
        flow.brief.scheme, flow.brief.host, flow.brief.path
    )
}

fn status_class(status: Option<u16>) -> &'static str {
    match status {
        Some(200..=299) => "ok",
        Some(300..=399) => "warn",
        Some(400..=599) => "err",
        _ => "none",
    }
}

fn local_time(ms: i64) -> String {
    DateTime::<Utc>::from_timestamp_millis(ms)
        .map(|time| {
            time.with_timezone(&Local)
                .format("%Y-%m-%d %H:%M:%S")
                .to_string()
        })
        .unwrap_or_else(|| "—".into())
}

fn rfc3339_local(ms: i64) -> String {
    DateTime::<Utc>::from_timestamp_millis(ms)
        .map(|time| {
            time.with_timezone(&Local)
                .to_rfc3339_opts(SecondsFormat::Millis, false)
        })
        .unwrap_or_default()
}

fn human_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    let value = bytes as f64;
    if value < KB {
        format!("{bytes} B")
    } else if value < MB {
        format!("{:.1} KB", value / KB)
    } else {
        format!("{:.1} MB", value / MB)
    }
}

fn safe_segment(value: &str, max_chars: usize) -> String {
    let mut result = String::new();
    for ch in value.chars().take(max_chars) {
        result.push(
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
                ch
            } else {
                '_'
            },
        );
    }
    let trimmed = result.trim_matches('_');
    if trimmed.is_empty() {
        "flow".into()
    } else {
        trimmed.to_string()
    }
}

fn escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}
