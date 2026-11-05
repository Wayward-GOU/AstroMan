//! 抓包 IPC 命令：启停、状态、CA、系统代理与流量列表。

use crate::capture::ca::{self, CaInfo};
use crate::capture::handler::build_body;
use crate::capture::intercept::{
    BreakpointRule, Decision, InterceptView, MapLocalRule, MapRemoteRule, RequestEdit,
    ResponseEdit, WeakNetwork,
};
use crate::capture::store::{BodyPayload, FlowBrief, FlowDetail, HeaderEntry};
use crate::capture::{CaptureConfig, CaptureState, CaptureStatus};
use crate::state::AppState;
use base64::Engine as _;
use bytes::Bytes;
use std::time::{Duration, Instant};

fn default_config(
    port: Option<u16>,
    bind_addr: Option<String>,
    ssl_hosts: Option<Vec<String>>,
    ignore_patterns: Option<Vec<String>>,
    capture_static: Option<bool>,
    upstream_proxy: Option<String>,
) -> CaptureConfig {
    let mut config = CaptureConfig::default();
    if let Some(port) = port {
        config.port = port;
    }
    if let Some(bind_addr) = bind_addr {
        config.bind_addr = bind_addr;
    }
    if let Some(ssl_hosts) = ssl_hosts {
        config.ssl_hosts = ssl_hosts;
    }
    if let Some(ignore_patterns) = ignore_patterns {
        config.ignore_patterns = ignore_patterns;
    }
    if let Some(capture_static) = capture_static {
        config.capture_static = capture_static;
    }
    config.upstream_proxy = upstream_proxy;
    config
}

#[tauri::command]
pub fn capture_start(
    state: tauri::State<'_, AppState>,
    port: Option<u16>,
    bind_addr: Option<String>,
    ssl_hosts: Option<Vec<String>>,
    ignore_patterns: Option<Vec<String>>,
    capture_static: Option<bool>,
    upstream_proxy: Option<String>,
) -> Result<CaptureStatus, String> {
    let config = default_config(
        port,
        bind_addr,
        ssl_hosts,
        ignore_patterns,
        capture_static,
        upstream_proxy,
    );
    state.capture.start(Some(config))
}

#[tauri::command]
pub fn capture_stop(state: tauri::State<'_, AppState>) -> Result<CaptureStatus, String> {
    state.capture.stop()
}

#[tauri::command(async)]
pub fn capture_status(state: tauri::State<'_, AppState>) -> CaptureStatus {
    state.capture.status()
}

#[tauri::command(async)]
pub fn capture_flows(state: tauri::State<'_, AppState>, limit: Option<usize>) -> Vec<FlowBrief> {
    state.capture.flows(limit.unwrap_or(500))
}

#[tauri::command(async)]
pub fn capture_flow_detail(
    state: tauri::State<'_, AppState>,
    id: u64,
) -> Result<FlowDetail, String> {
    state
        .capture
        .flow_detail(id)
        .ok_or_else(|| format!("E_FLOW_NOT_FOUND: 流量 #{id} 不存在或已淘汰"))
}

#[tauri::command(async)]
pub fn capture_flow_clear(state: tauri::State<'_, AppState>) -> CaptureStatus {
    state.capture.clear_flows();
    state.capture.status()
}

/// 手机抓包时展示电脑的局域网 IPv4，优先返回带网关的地址。
#[tauri::command(async)]
pub fn capture_lan_ips() -> Vec<String> {
    let mut items: Vec<(bool, String)> = Vec::new();
    for adapter in crate::netinfo::adapters() {
        let has_gateway = !adapter.gateway.is_empty();
        for address in adapter.ipv4 {
            if address.starts_with("127.") || address.starts_with("169.254.") {
                continue;
            }
            if !items.iter().any(|(_, existing)| existing == &address) {
                items.push((has_gateway, address));
            }
        }
    }
    items.sort_by_key(|(has_gateway, _)| !*has_gateway);
    items.into_iter().map(|(_, address)| address).collect()
}

#[tauri::command(async)]
pub fn capture_ca_info(state: tauri::State<'_, AppState>) -> CaInfo {
    ca::info(state.capture.data_dir())
}

#[tauri::command]
pub fn capture_ca_generate(
    state: tauri::State<'_, AppState>,
    force: Option<bool>,
) -> Result<CaInfo, String> {
    ca::generate(state.capture.data_dir(), force.unwrap_or(false))?;
    Ok(ca::info(state.capture.data_dir()))
}

#[tauri::command]
pub fn capture_ca_install(state: tauri::State<'_, AppState>) -> Result<CaInfo, String> {
    ca::install(state.capture.data_dir())
}

#[tauri::command(async)]
pub fn capture_ca_export(state: tauri::State<'_, AppState>, path: String) -> Result<(), String> {
    if path.trim().is_empty() {
        return Err("E_ARG: 导出路径不能为空".into());
    }
    ca::export(state.capture.data_dir(), std::path::Path::new(&path))
}

#[tauri::command]
pub fn capture_sysproxy_enable(state: tauri::State<'_, AppState>) -> Result<CaptureStatus, String> {
    state.capture.sysproxy_enable()
}

#[tauri::command]
pub fn capture_sysproxy_disable(
    state: tauri::State<'_, AppState>,
) -> Result<CaptureStatus, String> {
    state.capture.sysproxy_disable()
}

/// 重发结果。录制中会生成新记录；未录制时直接发送，响应只在弹窗里展示。
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepeatResult {
    pub recorded: bool,
    pub status: u16,
    pub status_text: String,
    pub duration_ms: u64,
    pub url: String,
    pub headers: Vec<HeaderEntry>,
    pub body: BodyPayload,
}

/// 这些头由客户端重新计算，原样带上会破坏请求。
fn repeat_skip_header(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "host"
            | "content-length"
            | "connection"
            | "proxy-connection"
            | "transfer-encoding"
            | "upgrade"
            | "keep-alive"
            | "content-encoding"
    )
}

pub(crate) struct RepeatOverride {
    pub method: Option<String>,
    pub url: Option<String>,
    pub headers: Option<Vec<HeaderEntry>>,
    pub body: Option<String>,
}

/// 重发核心逻辑：录制中走本机代理（proxy_port 有值），未录制时直连。
pub(crate) async fn repeat_request(
    detail: &FlowDetail,
    overrides: RepeatOverride,
    proxy_port: Option<u16>,
) -> Result<RepeatResult, String> {
    let RepeatOverride {
        method,
        url,
        headers,
        body,
    } = overrides;
    let raw_url = url
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| {
            format!(
                "{}://{}{}",
                detail.brief.scheme, detail.brief.host, detail.brief.path
            )
        });
    let normalized = if raw_url.contains("://") {
        raw_url.trim().to_string()
    } else {
        format!("http://{}", raw_url.trim())
    };
    let parsed =
        reqwest::Url::parse(&normalized).map_err(|error| format!("E_REPEAT: URL 无效 {error}"))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err("E_REPEAT: 只支持 http / https 请求".into());
    }

    let method_text = method
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| detail.brief.method.clone());
    let method_text = method_text.trim().to_ascii_uppercase();
    let req_method = reqwest::Method::from_bytes(method_text.as_bytes())
        .map_err(|error| format!("E_REPEAT: 无效的方法 {error}"))?;

    let mut builder = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(120));
    if let Some(port) = proxy_port {
        builder = builder.proxy(
            reqwest::Proxy::http(format!("http://127.0.0.1:{port}"))
                .map_err(|error| format!("E_REPEAT: {error}"))?,
        );
    }
    let client = builder
        .build()
        .map_err(|error| format!("E_REPEAT: {error}"))?;

    let mut request = client.request(req_method, parsed.clone());
    let source_headers = headers.unwrap_or_else(|| detail.request_headers.clone());
    for header in &source_headers {
        if header.name.trim().is_empty() || repeat_skip_header(&header.name) {
            continue;
        }
        request = request.header(&header.name, &header.value);
    }
    if let Some(text) = body {
        request = request.body(text);
    } else if let Some(text) = &detail.request_body.text {
        request = request.body(text.clone());
    } else if let Some(base64) = &detail.request_body.base64 {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(base64)
            .map_err(|error| format!("E_REPEAT: 请求体解码失败 {error}"))?;
        request = request.body(bytes);
    } else if detail.request_body.size > 0 {
        return Err("E_REPEAT: 请求体为二进制，暂不支持重发".into());
    }

    let started = Instant::now();
    let response = request
        .send()
        .await
        .map_err(|error| format!("E_REPEAT: {error}"))?;
    let duration_ms = started.elapsed().as_millis() as u64;
    let status = response.status();
    let response_headers = response
        .headers()
        .iter()
        .map(|(name, value)| HeaderEntry {
            name: name.as_str().to_string(),
            value: value.to_str().unwrap_or_default().to_string(),
        })
        .collect::<Vec<_>>();
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_string();
    // 读完响应体，确保录制管道完整跑完；未录制时用于在弹窗里展示
    let bytes = response
        .bytes()
        .await
        .map_err(|error| format!("E_REPEAT: 读取响应失败 {error}"))?;
    let body = build_body(&Bytes::copy_from_slice(&bytes), &content_type, true);

    Ok(RepeatResult {
        recorded: proxy_port.is_some(),
        status: status.as_u16(),
        status_text: status.canonical_reason().unwrap_or_default().to_string(),
        duration_ms,
        url: parsed.to_string(),
        headers: response_headers,
        body,
    })
}

/// 重发：支持覆盖方法、URL、请求头和请求体。
/// 录制中经过本机代理，新请求会被录制；未录制时直接发送，不影响录制状态。
#[tauri::command]
pub async fn capture_repeat(
    state: tauri::State<'_, AppState>,
    id: u64,
    method: Option<String>,
    url: Option<String>,
    headers: Option<Vec<HeaderEntry>>,
    body: Option<String>,
) -> Result<RepeatResult, String> {
    let detail = state
        .capture
        .flow_detail(id)
        .ok_or_else(|| format!("E_FLOW_NOT_FOUND: 流量 #{id} 不存在或已淘汰"))?;
    let proxy_port = {
        let status = state.capture.status();
        (status.state == CaptureState::Running && status.port != 0).then_some(status.port)
    };
    repeat_request(
        &detail,
        RepeatOverride {
            method,
            url,
            headers,
            body,
        },
        proxy_port,
    )
    .await
}

#[tauri::command(async)]
pub fn capture_export_har(
    state: tauri::State<'_, AppState>,
    path: String,
    ids: Option<Vec<u64>>,
) -> Result<(), String> {
    if path.trim().is_empty() {
        return Err("E_ARG: 导出路径不能为空".into());
    }
    match ids {
        Some(ids) if !ids.is_empty() => state
            .capture
            .export_har_ids(std::path::Path::new(&path), &ids),
        _ => state.capture.export_har(std::path::Path::new(&path)),
    }
}

/// 导出 ZIP：索引页、HAR、汇总与每条流量的可读明细。
#[tauri::command(async)]
pub fn capture_export_zip(
    state: tauri::State<'_, AppState>,
    path: String,
    ids: Option<Vec<u64>>,
) -> Result<(), String> {
    if path.trim().is_empty() {
        return Err("E_ARG: 导出路径不能为空".into());
    }
    match ids {
        Some(ids) if !ids.is_empty() => state
            .capture
            .export_zip_ids(std::path::Path::new(&path), &ids),
        _ => state.capture.export_zip(std::path::Path::new(&path)),
    }
}

/// 保存请求体 / 响应体到文件；文本直接写，图片等二进制按 base64 还原。
#[tauri::command(async)]
pub fn capture_save_body(
    state: tauri::State<'_, AppState>,
    id: u64,
    part: String,
    path: String,
) -> Result<(), String> {
    if path.trim().is_empty() {
        return Err("E_ARG: 保存路径不能为空".into());
    }
    let detail = state
        .capture
        .flow_detail(id)
        .ok_or_else(|| format!("E_FLOW_NOT_FOUND: 流量 #{id} 不存在或已淘汰"))?;
    let body = match part.as_str() {
        "request" => &detail.request_body,
        "response" => &detail.response_body,
        other => return Err(format!("E_ARG: 未知的 body 类型 {other}")),
    };
    let bytes = if let Some(text) = &body.text {
        text.as_bytes().to_vec()
    } else if let Some(base64) = &body.base64 {
        base64::engine::general_purpose::STANDARD
            .decode(base64)
            .map_err(|error| format!("E_BODY: 解码失败 {error}"))?
    } else {
        return Err("E_BODY: 该流量没有可保存的 body".into());
    };
    std::fs::write(&path, bytes).map_err(|error| format!("保存 body 失败：{error}"))
}

/// 拦截总览：断点/映射/弱网规则 + 当前挂起的流量。
#[tauri::command(async)]
pub fn capture_intercept_state(state: tauri::State<'_, AppState>) -> InterceptView {
    state.capture.intercept_view()
}

/// 保存断点开关与规则；只传其中一个也可以。
#[tauri::command]
pub fn capture_breakpoint_set_rules(
    state: tauri::State<'_, AppState>,
    enabled: Option<bool>,
    rules: Option<Vec<BreakpointRule>>,
) -> Result<InterceptView, String> {
    state.capture.set_breakpoints(enabled, rules)
}

/// 保存本地映射开关与规则。
#[tauri::command]
pub fn capture_map_set_rules(
    state: tauri::State<'_, AppState>,
    enabled: Option<bool>,
    rules: Option<Vec<MapLocalRule>>,
) -> Result<InterceptView, String> {
    state.capture.set_map(enabled, rules)
}

/// 保存远程映射开关与规则。
#[tauri::command]
pub fn capture_remote_set_rules(
    state: tauri::State<'_, AppState>,
    enabled: Option<bool>,
    rules: Option<Vec<MapRemoteRule>>,
) -> Result<InterceptView, String> {
    state.capture.set_remote(enabled, rules)
}

/// 保存弱网参数。
#[tauri::command]
pub fn capture_weak_set(
    state: tauri::State<'_, AppState>,
    weak: WeakNetwork,
) -> Result<InterceptView, String> {
    state.capture.set_weak(weak)
}

/// 处理一条挂起的断点：放行 / 中止 / 带着修改继续。
#[tauri::command]
pub fn capture_breakpoint_resolve(
    state: tauri::State<'_, AppState>,
    pause_id: u64,
    action: String,
    request: Option<RequestEdit>,
    response: Option<ResponseEdit>,
) -> Result<InterceptView, String> {
    let decision = match action.as_str() {
        "continue" => Decision::Continue,
        "abort" => Decision::Abort,
        "edit" => {
            if let Some(request) = request {
                Decision::Request(request)
            } else if let Some(response) = response {
                Decision::Response(response)
            } else {
                return Err("E_ARG: 改包请求缺少 request / response 内容".into());
            }
        }
        other => return Err(format!("E_ARG: 未知的断点操作 {other}")),
    };
    state.capture.resolve_pause(pause_id, decision)
}

/// 全部放行挂起的断点。
#[tauri::command]
pub fn capture_breakpoint_release_all(state: tauri::State<'_, AppState>) -> InterceptView {
    state.capture.release_pauses();
    state.capture.intercept_view()
}
