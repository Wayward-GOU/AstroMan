//! C-01/C-02 冒烟测试：真实起一个代理，通过它访问本地 HTTP 上游。
//!
//! 不依赖浏览器或外网，`cargo test capture::tests` 即可复现。

use super::{CaptureConfig, CaptureManager, CaptureState};
use crate::capture::store::{BodyPayload, FlowBrief, FlowDetail, HeaderEntry};
use crate::commands::capture::{repeat_request, RepeatOverride};
use base64::Engine;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::Duration;

fn temp_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("astroman-capture-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

/// 读取 ZIP 中路径以 `suffix` 结尾的第一个条目。
fn read_zip_entry(archive: &mut zip::ZipArchive<std::fs::File>, suffix: &str) -> Vec<u8> {
    let index = (0..archive.len())
        .find(|index| {
            archive
                .by_index(*index)
                .map(|entry| entry.name().ends_with(suffix))
                .unwrap_or(false)
        })
        .unwrap_or_else(|| panic!("ZIP 中找不到 {suffix}"));
    let mut entry = archive.by_index(index).expect("open zip entry");
    let mut bytes = Vec::new();
    entry.read_to_end(&mut bytes).expect("read zip entry");
    bytes
}

fn read_zip_text(archive: &mut zip::ZipArchive<std::fs::File>, suffix: &str) -> String {
    String::from_utf8(read_zip_entry(archive, suffix)).expect("zip entry should be utf-8")
}

/// 起一个只回一次 200 的上游服务，返回端口与 join handle。
fn spawn_upstream() -> (u16, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind upstream");
    let port = listener.local_addr().expect("upstream addr").port();
    let handle = std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
            let mut buffer = [0u8; 4096];
            let _ = stream.read(&mut buffer);
            let body = b"hello-from-upstream";
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                String::from_utf8_lossy(body)
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
        }
    });
    (port, handle)
}

#[test]
fn proxy_forwards_http_and_ca_is_stable() {
    let data_dir = temp_dir();
    let manager = CaptureManager::new(data_dir.clone());
    let (upstream_port, upstream) = spawn_upstream();

    let status = manager
        .start(Some(CaptureConfig {
            port: 0,
            bind_addr: "127.0.0.1".into(),
            ..CaptureConfig::default()
        }))
        .expect("start capture");
    assert_eq!(status.state, CaptureState::Running);
    assert!(status.port > 0);

    let mut client = TcpStream::connect(("127.0.0.1", status.port)).expect("connect proxy");
    let _ = client.set_read_timeout(Some(Duration::from_secs(10)));
    let request = format!(
        "POST http://127.0.0.1:{upstream_port}/hello HTTP/1.1\r\nHost: 127.0.0.1:{upstream_port}\r\nContent-Type: text/plain\r\nContent-Length: 4\r\nConnection: close\r\n\r\nping"
    );
    client
        .write_all(request.as_bytes())
        .expect("write proxy request");
    let mut response = Vec::new();
    let _ = client.read_to_end(&mut response);
    let text = String::from_utf8_lossy(&response);
    assert!(
        text.starts_with("HTTP/1.1 200"),
        "unexpected response: {text}"
    );
    assert!(text.contains("hello-from-upstream"));

    // handler 在响应返回后落记录，轮询等待一下，避免线程调度导致的偶发空列表
    let mut flows = manager.flows(100);
    for _ in 0..50 {
        if !flows.is_empty() {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
        flows = manager.flows(100);
    }
    assert_eq!(flows.len(), 1, "expected one recorded flow, got {flows:?}");
    let flow = &flows[0];
    assert_eq!(flow.method, "POST");
    assert_eq!(flow.host, format!("127.0.0.1:{upstream_port}"));
    assert_eq!(flow.path, "/hello");
    assert_eq!(flow.http_version, "HTTP/1.1");
    assert_eq!(flow.status, Some(200));
    assert_eq!(flow.request_size, 4);
    assert_eq!(flow.response_size, "hello-from-upstream".len() as u64);
    let detail = manager
        .flow_detail(flow.id)
        .expect("flow detail should exist");
    assert!(!detail.request_headers.is_empty());
    assert_eq!(detail.request_body.text.as_deref(), Some("ping"));
    assert_eq!(
        detail.response_body.text.as_deref(),
        Some("hello-from-upstream")
    );
    assert_eq!(detail.response_body.content_type, "text/plain");

    // HAR 导出：写入临时目录并确认包含这条流量
    let har_path = data_dir.join("session.har");
    manager.export_har(&har_path).expect("export har");
    let har = std::fs::read_to_string(&har_path).expect("read har");
    assert!(har.contains("\"entries\""));
    assert!(har.contains("hello-from-upstream"));

    // 按 id 导出（域名分组右键导出）：只写选中的流量
    let group_path = data_dir.join("group.har");
    manager
        .export_har_ids(&group_path, &[flow.id])
        .expect("export group har");
    let group_har = std::fs::read_to_string(&group_path).expect("read group har");
    assert!(group_har.contains("hello-from-upstream"));
    assert!(manager.export_har_ids(&group_path, &[]).is_err());
    assert!(manager.export_har_ids(&group_path, &[u64::MAX]).is_err());

    // ZIP 导出：索引页、HAR、汇总与按域名分组的流量目录都要有
    let zip_path = data_dir.join("session.zip");
    manager.export_zip(&zip_path).expect("export zip");
    let mut archive =
        zip::ZipArchive::new(std::fs::File::open(&zip_path).expect("open zip")).expect("read zip");
    let names: Vec<String> = (0..archive.len())
        .filter_map(|index| {
            archive
                .by_index(index)
                .ok()
                .map(|entry| entry.name().to_string())
        })
        .collect();
    assert!(names.iter().any(|name| name.ends_with("/index.html")));
    assert!(names.iter().any(|name| name.ends_with("/summary.json")));
    assert!(names.iter().any(|name| name.ends_with("/capture.har")));
    assert!(
        names.iter().any(|name| name.contains("/flows/0001-POST-")),
        "unexpected zip entries: {names:?}"
    );
    let index = read_zip_text(&mut archive, "/index.html");
    assert!(index.contains(&format!("127.0.0.1:{upstream_port}")));
    assert!(manager.export_zip_ids(&zip_path, &[]).is_err());
    assert!(manager.export_zip_ids(&zip_path, &[u64::MAX]).is_err());

    let stopped = manager.stop().expect("stop capture");
    assert_eq!(stopped.state, CaptureState::Idle);
    upstream.join().expect("upstream thread");

    let first = super::ca::info(&data_dir);
    assert!(first.exists);
    assert!(!first.fingerprint.is_empty());
    assert_eq!(
        first.thumbprint.len(),
        40,
        "SHA-1 thumbprint should be 40 hex chars"
    );
    let second = super::ca::info(&data_dir);
    assert_eq!(first.fingerprint, second.fingerprint);
    assert_eq!(first.thumbprint, second.thumbprint);
    assert!(super::ca::certs_dir(&data_dir)
        .join("astroman-ca.key")
        .is_file());

    let _ = std::fs::remove_dir_all(&data_dir);
}

/// ZIP 解压后的文本、元数据与二进制 body 要能直接看懂 / 打开。
#[test]
fn zip_export_contains_readable_text_and_binary_bodies() {
    let dir = temp_dir();
    let zip_path = dir.join("export.zip");
    let request_text = "{\"name\":\"alice\"}";
    let response_text = "{\"ok\":true}";
    let text_flow = FlowDetail {
        brief: FlowBrief {
            id: 7,
            started_at: 1_760_000_000_000,
            method: "POST".into(),
            scheme: "https".into(),
            host: "api.example.com".into(),
            path: "/login".into(),
            http_version: "HTTP/2".into(),
            status: Some(200),
            duration_ms: 42,
            request_size: request_text.len() as u64,
            response_size: response_text.len() as u64,
            content_type: "application/json".into(),
            client_addr: "127.0.0.1:5555".into(),
            error: None,
        },
        request_headers: vec![HeaderEntry {
            name: "content-type".into(),
            value: "application/json".into(),
        }],
        response_headers: vec![HeaderEntry {
            name: "content-type".into(),
            value: "application/json".into(),
        }],
        request_body: BodyPayload {
            size: request_text.len() as u64,
            truncated: false,
            binary: false,
            text: Some(request_text.into()),
            base64: None,
            content_type: "application/json".into(),
        },
        response_body: BodyPayload {
            size: response_text.len() as u64,
            truncated: false,
            binary: false,
            text: Some(response_text.into()),
            base64: None,
            content_type: "application/json".into(),
        },
    };
    let image_bytes = b"fake-png";
    let image_flow = FlowDetail {
        brief: FlowBrief {
            id: 8,
            started_at: 1_760_000_001_000,
            method: "GET".into(),
            scheme: "https".into(),
            host: "cdn.example.com".into(),
            path: "/logo.png".into(),
            http_version: "HTTP/2".into(),
            status: Some(200),
            duration_ms: 11,
            request_size: 0,
            response_size: image_bytes.len() as u64,
            content_type: "image/png".into(),
            client_addr: "127.0.0.1:5556".into(),
            error: None,
        },
        request_headers: Vec::new(),
        response_headers: vec![HeaderEntry {
            name: "content-type".into(),
            value: "image/png".into(),
        }],
        request_body: BodyPayload::default(),
        response_body: BodyPayload {
            size: image_bytes.len() as u64,
            truncated: false,
            binary: true,
            text: None,
            base64: Some(base64::engine::general_purpose::STANDARD.encode(image_bytes)),
            content_type: "image/png".into(),
        },
    };

    super::archive::export(&zip_path, &[text_flow, image_flow]).expect("export zip");
    let mut archive =
        zip::ZipArchive::new(std::fs::File::open(&zip_path).expect("open zip")).expect("read zip");

    let request = read_zip_text(&mut archive, "/0001-POST-api.example.com/request.txt");
    assert!(request.starts_with("POST /login HTTP/2"));
    assert!(request.contains(request_text));
    let response = read_zip_text(&mut archive, "/0001-POST-api.example.com/response.txt");
    assert!(response.starts_with("HTTP/2 200"));
    assert!(response.contains(response_text));
    let meta = read_zip_text(&mut archive, "/0001-POST-api.example.com/meta.json");
    assert!(meta.contains("\"rawFile\":null"));

    let image_meta = read_zip_text(&mut archive, "/0002-GET-cdn.example.com/meta.json");
    assert!(image_meta.contains("response.body.png"));
    let raw = read_zip_entry(&mut archive, "/0002-GET-cdn.example.com/response.body.png");
    assert_eq!(raw, image_bytes);

    let index = read_zip_text(&mut archive, "/index.html");
    assert!(index.contains("api.example.com"));
    assert!(index.contains("cdn.example.com"));
    let summary: serde_json::Value =
        serde_json::from_str(&read_zip_text(&mut archive, "/summary.json")).expect("summary json");
    assert_eq!(summary["flowCount"], 2);
    assert_eq!(summary["hostCount"], 2);
    let har = read_zip_text(&mut archive, "/capture.har");
    assert!(har.contains("\"entries\""));
    assert!(har.contains("api.example.com"));

    let _ = std::fs::remove_dir_all(&dir);
}

/// 起一个把完整请求原文回显成响应体的上游，用于校验重发是否按覆盖参数发出。
fn spawn_echo_upstream() -> (u16, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind echo upstream");
    let port = listener.local_addr().expect("echo upstream addr").port();
    let handle = std::thread::spawn(move || {
        let Ok((mut stream, _)) = listener.accept() else {
            return;
        };
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
        let mut raw = Vec::new();
        let mut chunk = [0u8; 2048];
        let mut header_end: Option<usize> = None;
        loop {
            let read = stream.read(&mut chunk).unwrap_or(0);
            if read == 0 {
                break;
            }
            raw.extend_from_slice(&chunk[..read]);
            if header_end.is_none() {
                header_end = raw
                    .windows(4)
                    .position(|window| window == b"\r\n\r\n")
                    .map(|position| position + 4);
            }
            if let Some(start) = header_end {
                let headers = String::from_utf8_lossy(&raw[..start]).to_ascii_lowercase();
                let length = headers
                    .lines()
                    .find_map(|line| line.strip_prefix("content-length:"))
                    .and_then(|value| value.trim().parse::<usize>().ok())
                    .unwrap_or(0);
                if raw.len() >= start + length {
                    break;
                }
            }
        }
        let text = String::from_utf8_lossy(&raw).to_string();
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            text.len(),
            text
        );
        let _ = stream.write_all(response.as_bytes());
        let _ = stream.flush();
    });
    (port, handle)
}

fn repeat_detail(port: u16) -> FlowDetail {
    FlowDetail {
        brief: FlowBrief {
            id: 1,
            started_at: 0,
            method: "GET".into(),
            scheme: "http".into(),
            host: format!("127.0.0.1:{port}"),
            path: "/original".into(),
            http_version: "HTTP/1.1".into(),
            status: Some(200),
            duration_ms: 0,
            request_size: 0,
            response_size: 0,
            content_type: "text/plain".into(),
            client_addr: String::new(),
            error: None,
        },
        request_headers: vec![HeaderEntry {
            name: "host".into(),
            value: format!("127.0.0.1:{port}"),
        }],
        response_headers: Vec::new(),
        request_body: BodyPayload::default(),
        response_body: BodyPayload::default(),
    }
}

/// 未录制时重发应直接发送，并支持覆盖方法、URL、请求头和请求体。
#[tokio::test]
async fn repeat_without_recording_applies_overrides() {
    let (upstream_port, upstream) = spawn_echo_upstream();
    let detail = repeat_detail(upstream_port);

    let result = repeat_request(
        &detail,
        RepeatOverride {
            method: Some("post".into()),
            url: Some(format!("http://127.0.0.1:{upstream_port}/edited?x=1")),
            headers: Some(vec![HeaderEntry {
                name: "x-test".into(),
                value: "1".into(),
            }]),
            body: Some("edited-body".into()),
        },
        None,
    )
    .await
    .expect("repeat should succeed without recording");

    assert!(!result.recorded);
    assert_eq!(result.status, 200);
    assert_eq!(result.status_text, "OK");
    assert!(result.url.ends_with("/edited?x=1"));
    let echoed = result
        .body
        .text
        .expect("text response")
        .to_ascii_lowercase();
    assert!(
        echoed.contains("post /edited?x=1 http/1.1"),
        "unexpected request line: {echoed}"
    );
    assert!(
        echoed.contains("x-test: 1"),
        "missing overridden header: {echoed}"
    );
    assert!(
        echoed.contains("edited-body"),
        "missing overridden body: {echoed}"
    );

    upstream.join().expect("echo upstream thread");
}
