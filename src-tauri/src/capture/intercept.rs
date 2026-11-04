//! 抓包拦截层：断点改包、本地映射（Map Local）、远程映射（Map Remote）与弱网模拟。
//!
//! 四类能力共享一份运行时状态（`InterceptState`）：代理线程只读规则、
//! UI 通过 IPC 读写规则并处理暂停中的流量。规则落盘在
//! `<数据目录>/capture/intercept.json`，重启后仍在。
//!
//! 设计约束：
//! - 断点是「挂起等待」，绝不能在持锁状态下 await，否则代理线程会死锁；
//! - 120 秒没人处理自动放行；同时挂起超过 32 条后新请求不再挂起，避免把代理拖死；
//! - 弱网与映射对进入代理的流量都生效（包含被过滤掉的静态资源）；
//! - 本地映射优先于远程映射：同一条流量只可能被其中一种规则接管；
//! - 二进制 / 超大 / 流式 body 不进断点编辑，热路径不会为它多缓冲一份。

use crate::capture::store::{BodyPayload, HeaderEntry};
use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::oneshot;

/// 断点最长挂起时间：超时自动放行，客户端不至于一直挂着。
pub const PAUSE_TIMEOUT: Duration = Duration::from_secs(120);
/// 同时挂起的请求上限，超过后新请求直接放行。
pub const MAX_PENDING: usize = 32;
/// 弱网限速单次最多等 120 秒，避免超大响应把连接挂死。
const MAX_THROTTLE: Duration = Duration::from_secs(120);
/// 本地映射文件最大读 50 MB，超出视为配置错误。
pub const MAX_MAP_FILE: u64 = 50 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BreakpointPhase {
    Request,
    Response,
    Both,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PendingPhase {
    Request,
    Response,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MapSource {
    File,
    Inline,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BreakpointRule {
    pub id: u64,
    pub enabled: bool,
    pub name: String,
    /// URL 匹配：含 `*` 走通配，否则走「包含」；只写 `host/path` 也能命中。
    pub pattern: String,
    /// 空串或 `any` 表示不限方法。
    pub method: String,
    pub phase: BreakpointPhase,
}

impl Default for BreakpointRule {
    fn default() -> Self {
        Self {
            id: 0,
            enabled: true,
            name: String::new(),
            pattern: String::new(),
            method: String::new(),
            phase: BreakpointPhase::Both,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MapLocalRule {
    pub id: u64,
    pub enabled: bool,
    pub name: String,
    pub pattern: String,
    pub method: String,
    pub source: MapSource,
    /// `source = file` 时的本地路径；每次命中实时读取，改完文件即生效。
    pub file_path: String,
    pub status: u16,
    /// 留空时按扩展名（文件）或内容（内联）猜。
    pub content_type: String,
    /// `source = inline` 时的响应正文。
    pub body: String,
    pub headers: Vec<HeaderEntry>,
}

impl Default for MapLocalRule {
    fn default() -> Self {
        Self {
            id: 0,
            enabled: true,
            name: String::new(),
            pattern: String::new(),
            method: String::new(),
            source: MapSource::File,
            file_path: String::new(),
            status: 200,
            content_type: String::new(),
            body: String::new(),
            headers: Vec::new(),
        }
    }
}

/// 远程映射：命中后把请求改发到 `target`，客户端仍以为自己请求的是原地址。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MapRemoteRule {
    pub id: u64,
    pub enabled: bool,
    pub name: String,
    pub pattern: String,
    pub method: String,
    /// 目标地址；可以只写 `host[:port]`，也可以写带路径的完整 URL。
    pub target: String,
    /// true 表示保留原请求的路径与查询串，拼在目标地址后面（默认）。
    pub keep_path: bool,
}

impl Default for MapRemoteRule {
    fn default() -> Self {
        Self {
            id: 0,
            enabled: true,
            name: String::new(),
            pattern: String::new(),
            method: String::new(),
            target: String::new(),
            keep_path: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WeakNetwork {
    pub enabled: bool,
    /// 只记录 UI 选中的预设名，参数仍可单独改。
    pub preset: String,
    pub latency_ms: u64,
    pub jitter_ms: u64,
    /// 响应下行带宽（kbps），0 表示不限速。
    pub bandwidth_kbps: u64,
    /// 0~100，命中的请求直接返回 504。
    pub loss_percent: u8,
    /// 作用范围，空表示全部流量。
    pub patterns: Vec<String>,
}

impl Default for WeakNetwork {
    fn default() -> Self {
        Self {
            enabled: false,
            preset: "custom".into(),
            latency_ms: 200,
            jitter_ms: 50,
            bandwidth_kbps: 0,
            loss_percent: 0,
            patterns: Vec::new(),
        }
    }
}

/// 一条被断点挂起的流量；前端按 `phase` 决定哪些字段可编辑。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingInfo {
    pub pause_id: u64,
    pub phase: PendingPhase,
    /// 命中的规则名，方便确认「为什么被拦」
    pub rule: String,
    pub method: String,
    pub url: String,
    pub http_version: String,
    pub status: Option<u16>,
    pub request_headers: Vec<HeaderEntry>,
    pub request_body: BodyPayload,
    /// false 表示请求体是二进制或过大，只能在放行时原样发出
    pub request_editable: bool,
    pub response_headers: Vec<HeaderEntry>,
    pub response_body: BodyPayload,
    /// false 表示响应体未缓冲（流式或过大），只能改状态码与响应头
    pub response_editable: bool,
    pub created_at: i64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RequestEdit {
    pub method: Option<String>,
    pub url: Option<String>,
    pub headers: Option<Vec<HeaderEntry>>,
    pub body: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ResponseEdit {
    pub status: Option<u16>,
    pub headers: Option<Vec<HeaderEntry>>,
    pub body: Option<String>,
}

/// 断点处理结果。
#[derive(Debug, Clone)]
pub enum Decision {
    Continue,
    Abort,
    Request(RequestEdit),
    Response(ResponseEdit),
}

/// 请求阶段的弱网计划。
#[derive(Debug, Clone)]
pub struct WeakPlan {
    pub delay: Duration,
    pub drop: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct InterceptConfigFile {
    pub breakpoints_enabled: bool,
    pub map_enabled: bool,
    pub remote_enabled: bool,
    pub breakpoint_rules: Vec<BreakpointRule>,
    pub map_rules: Vec<MapLocalRule>,
    pub remote_rules: Vec<MapRemoteRule>,
    pub weak: WeakNetwork,
}

struct PendingSlot {
    info: PendingInfo,
    resolve: Option<oneshot::Sender<Decision>>,
}

/// 代理线程与 IPC 共享的拦截状态。
pub struct InterceptState {
    breakpoints_enabled: AtomicBool,
    map_enabled: AtomicBool,
    remote_enabled: AtomicBool,
    breakpoint_rules: RwLock<Vec<BreakpointRule>>,
    map_rules: RwLock<Vec<MapLocalRule>>,
    remote_rules: RwLock<Vec<MapRemoteRule>>,
    weak: RwLock<WeakNetwork>,
    pending: Mutex<HashMap<u64, PendingSlot>>,
    next_pause: AtomicU64,
    /// 累计挂起次数，前端用来提示「这一轮拦了多少」
    paused_total: AtomicU64,
}

impl Default for InterceptState {
    fn default() -> Self {
        Self::new()
    }
}

impl InterceptState {
    pub fn new() -> Self {
        Self {
            breakpoints_enabled: AtomicBool::new(false),
            map_enabled: AtomicBool::new(false),
            remote_enabled: AtomicBool::new(false),
            breakpoint_rules: RwLock::new(Vec::new()),
            map_rules: RwLock::new(Vec::new()),
            remote_rules: RwLock::new(Vec::new()),
            weak: RwLock::new(WeakNetwork::default()),
            pending: Mutex::new(HashMap::new()),
            next_pause: AtomicU64::new(1),
            paused_total: AtomicU64::new(0),
        }
    }

    pub fn load(path: &Path) -> Self {
        let state = Self::new();
        let Ok(text) = std::fs::read_to_string(path) else {
            return state;
        };
        match serde_json::from_str::<InterceptConfigFile>(&text) {
            Ok(config) => {
                state
                    .breakpoints_enabled
                    .store(config.breakpoints_enabled, Ordering::Relaxed);
                state
                    .map_enabled
                    .store(config.map_enabled, Ordering::Relaxed);
                state
                    .remote_enabled
                    .store(config.remote_enabled, Ordering::Relaxed);
                *state.breakpoint_rules.write() = config.breakpoint_rules;
                *state.map_rules.write() = config.map_rules;
                *state.remote_rules.write() = config.remote_rules;
                *state.weak.write() = config.weak;
                state
            }
            Err(error) => {
                log::warn!("抓包拦截规则读取失败，改用默认值：{error}");
                state
            }
        }
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| format!("创建目录失败：{error}"))?;
        }
        let text = serde_json::to_string_pretty(&self.config_file())
            .map_err(|error| format!("序列化失败：{error}"))?;
        // 先写临时文件再改名，避免写一半崩溃留下半个 JSON
        let tmp = path.with_extension("json.part");
        std::fs::write(&tmp, text).map_err(|error| format!("写入失败：{error}"))?;
        std::fs::rename(&tmp, path).map_err(|error| format!("替换失败：{error}"))?;
        Ok(())
    }

    fn config_file(&self) -> InterceptConfigFile {
        InterceptConfigFile {
            breakpoints_enabled: self.breakpoints_enabled.load(Ordering::Relaxed),
            map_enabled: self.map_enabled.load(Ordering::Relaxed),
            remote_enabled: self.remote_enabled.load(Ordering::Relaxed),
            breakpoint_rules: self.breakpoint_rules.read().clone(),
            map_rules: self.map_rules.read().clone(),
            remote_rules: self.remote_rules.read().clone(),
            weak: self.weak.read().clone(),
        }
    }

    pub fn set_breakpoints(&self, enabled: Option<bool>, rules: Option<Vec<BreakpointRule>>) {
        if let Some(enabled) = enabled {
            self.breakpoints_enabled.store(enabled, Ordering::Relaxed);
        }
        if let Some(rules) = rules {
            *self.breakpoint_rules.write() = normalize_rules(rules);
        }
    }

    pub fn set_map(&self, enabled: Option<bool>, rules: Option<Vec<MapLocalRule>>) {
        if let Some(enabled) = enabled {
            self.map_enabled.store(enabled, Ordering::Relaxed);
        }
        if let Some(rules) = rules {
            *self.map_rules.write() = normalize_rules(rules);
        }
    }

    pub fn set_remote(&self, enabled: Option<bool>, rules: Option<Vec<MapRemoteRule>>) {
        if let Some(enabled) = enabled {
            self.remote_enabled.store(enabled, Ordering::Relaxed);
        }
        if let Some(rules) = rules {
            *self.remote_rules.write() = normalize_rules(rules);
        }
    }

    pub fn set_weak(&self, weak: WeakNetwork) {
        let bounded = WeakNetwork {
            latency_ms: weak.latency_ms.min(60_000),
            jitter_ms: weak.jitter_ms.min(30_000),
            bandwidth_kbps: weak.bandwidth_kbps.min(100_000_000),
            loss_percent: weak.loss_percent.min(100),
            ..weak
        };
        *self.weak.write() = bounded;
    }

    /// 热路径先问一句，规则没开时不必做 URL 拼接与匹配。
    pub fn breakpoints_active(&self) -> bool {
        self.breakpoints_enabled.load(Ordering::Relaxed) && !self.breakpoint_rules.read().is_empty()
    }

    pub fn map_active(&self) -> bool {
        self.map_enabled.load(Ordering::Relaxed) && !self.map_rules.read().is_empty()
    }

    pub fn remote_active(&self) -> bool {
        self.remote_enabled.load(Ordering::Relaxed) && !self.remote_rules.read().is_empty()
    }

    pub fn snapshot(&self) -> InterceptView {
        let pending = self
            .pending
            .lock()
            .values()
            .map(|slot| slot.info.clone())
            .collect::<Vec<_>>();
        InterceptView {
            breakpoints_enabled: self.breakpoints_enabled.load(Ordering::Relaxed),
            breakpoints_active: self.breakpoints_active(),
            map_enabled: self.map_enabled.load(Ordering::Relaxed),
            map_active: self.map_active(),
            remote_enabled: self.remote_enabled.load(Ordering::Relaxed),
            remote_active: self.remote_active(),
            weak: self.weak.read().clone(),
            breakpoint_rules: self.breakpoint_rules.read().clone(),
            map_rules: self.map_rules.read().clone(),
            remote_rules: self.remote_rules.read().clone(),
            paused_total: self.paused_total.load(Ordering::Relaxed),
            pending_count: pending.len(),
            max_pending: MAX_PENDING,
            timeout_seconds: PAUSE_TIMEOUT.as_secs(),
            pending,
        }
    }

    /// 命中第一条启用的规则。正文不可编辑时（二进制、压缩、超大）仍然命中，
    /// 只是前端把 body 锁住，只让改状态码和请求/响应头。
    pub fn match_breakpoint(
        &self,
        url: &str,
        method: &str,
        phase: PendingPhase,
    ) -> Option<BreakpointRule> {
        if !self.breakpoints_active() {
            return None;
        }
        self.breakpoint_rules
            .read()
            .iter()
            .find(|rule| {
                rule.enabled
                    && method_matches(&rule.method, method)
                    && phase_matches(rule.phase, phase)
                    && url_matches(&rule.pattern, url)
            })
            .cloned()
    }

    pub fn match_map(&self, url: &str, method: &str) -> Option<MapLocalRule> {
        if !self.map_active() {
            return None;
        }
        self.map_rules
            .read()
            .iter()
            .find(|rule| {
                rule.enabled
                    && method_matches(&rule.method, method)
                    && url_matches(&rule.pattern, url)
            })
            .cloned()
    }

    pub fn match_remote(&self, url: &str, method: &str) -> Option<MapRemoteRule> {
        if !self.remote_active() {
            return None;
        }
        self.remote_rules
            .read()
            .iter()
            .find(|rule| {
                rule.enabled
                    && !rule.target.trim().is_empty()
                    && method_matches(&rule.method, method)
                    && url_matches(&rule.pattern, url)
            })
            .cloned()
    }

    /// 请求阶段：首包延迟 + 抖动，并按丢包率决定是否直接判死。
    pub fn weak_request_plan(&self, url: &str) -> Option<WeakPlan> {
        let weak = self.weak.read();
        if !weak.enabled || !in_weak_scope(&weak, url) {
            return None;
        }
        let jitter = if weak.jitter_ms > 0 {
            next_random() % (weak.jitter_ms * 2 + 1)
        } else {
            0
        };
        let delay = Duration::from_millis(weak.latency_ms.saturating_add(jitter));
        let drop = weak.loss_percent > 0 && next_random() % 100 < weak.loss_percent as u64;
        Some(WeakPlan { delay, drop })
    }

    /// 响应阶段：按响应体大小把带宽折算成一段等待。
    pub fn weak_response_delay(&self, url: &str, bytes: u64) -> Option<Duration> {
        let weak = self.weak.read();
        if !weak.enabled || weak.bandwidth_kbps == 0 || bytes == 0 || !in_weak_scope(&weak, url) {
            return None;
        }
        let seconds = ((bytes * 8) as f64 / (weak.bandwidth_kbps * 1000) as f64)
            .min(MAX_THROTTLE.as_secs_f64());
        (seconds > 0.0).then(|| Duration::from_secs_f64(seconds))
    }

    /// 挂起等待前端处理；队列已满直接放行，超时按放行处理。
    pub async fn pause(&self, info: PendingInfo) -> Decision {
        if self.pending_count() >= MAX_PENDING {
            return Decision::Continue;
        }
        let pause_id = info.pause_id;
        let (sender, receiver) = oneshot::channel();
        self.pending.lock().insert(
            pause_id,
            PendingSlot {
                info: info.clone(),
                resolve: Some(sender),
            },
        );
        self.paused_total.fetch_add(1, Ordering::Relaxed);
        let decision = match tokio::time::timeout(PAUSE_TIMEOUT, receiver).await {
            Ok(Ok(decision)) => decision,
            _ => Decision::Continue,
        };
        // 超时或通道被丢弃时，这里负责清掉残留槽位
        self.pending.lock().remove(&pause_id);
        decision
    }

    pub fn pending_count(&self) -> usize {
        self.pending.lock().len()
    }

    pub fn next_pause_id(&self) -> u64 {
        self.next_pause.fetch_add(1, Ordering::Relaxed)
    }

    /// 前端处理一条断点。
    pub fn resolve(&self, pause_id: u64, decision: Decision) -> Result<(), String> {
        let slot = self.pending.lock().remove(&pause_id);
        let Some(mut slot) = slot else {
            return Err(format!("E_PAUSE_GONE: 断点 #{pause_id} 已超时放行或不存在"));
        };
        if let Some(sender) = slot.resolve.take() {
            // 接收端还在等时必然成功；失败说明这条已被超时回收
            let _ = sender.send(decision);
        }
        Ok(())
    }

    /// 停止录制或点「全部放行」时释放所有挂起流量。
    pub fn release_all(&self) -> usize {
        let mut pending = self.pending.lock();
        let count = pending.len();
        for slot in pending.values_mut() {
            if let Some(sender) = slot.resolve.take() {
                let _ = sender.send(Decision::Continue);
            }
        }
        pending.clear();
        count
    }
}

/// 提供给前端的拦截总览。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InterceptView {
    pub breakpoints_enabled: bool,
    pub breakpoints_active: bool,
    pub map_enabled: bool,
    pub map_active: bool,
    pub remote_enabled: bool,
    pub remote_active: bool,
    pub weak: WeakNetwork,
    pub breakpoint_rules: Vec<BreakpointRule>,
    pub map_rules: Vec<MapLocalRule>,
    pub remote_rules: Vec<MapRemoteRule>,
    pub paused_total: u64,
    pub pending_count: usize,
    pub max_pending: usize,
    pub timeout_seconds: u64,
    pub pending: Vec<PendingInfo>,
}

fn normalize_rules<T: RuleLike>(mut rules: Vec<T>) -> Vec<T> {
    rules.retain(|rule| !rule.pattern().trim().is_empty());
    for (index, rule) in rules.iter_mut().enumerate() {
        if rule.id() == 0 {
            rule.set_id(index as u64 + 1);
        }
    }
    rules.truncate(200);
    rules
}

trait RuleLike {
    fn id(&self) -> u64;
    fn set_id(&mut self, id: u64);
    fn pattern(&self) -> &str;
}

impl RuleLike for BreakpointRule {
    fn id(&self) -> u64 {
        self.id
    }
    fn set_id(&mut self, id: u64) {
        self.id = id;
    }
    fn pattern(&self) -> &str {
        &self.pattern
    }
}

impl RuleLike for MapLocalRule {
    fn id(&self) -> u64 {
        self.id
    }
    fn set_id(&mut self, id: u64) {
        self.id = id;
    }
    fn pattern(&self) -> &str {
        &self.pattern
    }
}

impl RuleLike for MapRemoteRule {
    fn id(&self) -> u64 {
        self.id
    }
    fn set_id(&mut self, id: u64) {
        self.id = id;
    }
    fn pattern(&self) -> &str {
        &self.pattern
    }
}

fn phase_matches(rule: BreakpointPhase, pending: PendingPhase) -> bool {
    matches!(
        (rule, pending),
        (BreakpointPhase::Both, _)
            | (BreakpointPhase::Request, PendingPhase::Request)
            | (BreakpointPhase::Response, PendingPhase::Response)
    )
}

fn method_matches(rule: &str, method: &str) -> bool {
    let rule = rule.trim();
    rule.is_empty()
        || rule.eq_ignore_ascii_case("any")
        || rule.eq_ignore_ascii_case("*")
        || rule.eq_ignore_ascii_case(method)
}

fn in_weak_scope(weak: &WeakNetwork, url: &str) -> bool {
    if weak.patterns.is_empty() {
        return true;
    }
    weak.patterns
        .iter()
        .map(|pattern| pattern.trim())
        .filter(|pattern| !pattern.is_empty())
        .any(|pattern| url_matches(pattern, url))
}

/// URL 匹配：带 `*` 走通配，否则走包含；额外兼容缺协议的相对形式。
pub fn url_matches(pattern: &str, url: &str) -> bool {
    let pattern = pattern.trim();
    if pattern.is_empty() {
        return false;
    }
    let url = url.trim().to_ascii_lowercase();
    let pattern = pattern.to_ascii_lowercase();
    // 相对形式的 URL 去掉协议再比一次，规则只写 host/path 也能命中
    let relative = url.split_once("://").map(|(_, rest)| rest).unwrap_or(&url);
    if pattern.contains('*') {
        glob_match(&pattern, &url) || glob_match(&pattern, relative)
    } else {
        url.contains(&pattern) || relative.contains(&pattern)
    }
}

/// 计算远程映射的最终目标地址。
///
/// - `keep_path = true`：把原请求的路径与查询串拼到目标地址后面，适合「整体换域名 / 换环境」；
/// - `keep_path = false`：完全使用规则里写的目标地址，适合把某个接口固定指到另一个接口。
///
/// 目标地址不带协议时按 `http` 处理；只写 `host:port` 也能用。
pub fn resolve_remote_target(rule: &MapRemoteRule, url: &str) -> Option<String> {
    let target = rule.target.trim();
    if target.is_empty() {
        return None;
    }
    let target = if target.contains("://") {
        target.to_string()
    } else {
        format!("http://{target}")
    };
    let (origin, target_path) = split_origin(&target);
    if !rule.keep_path {
        // 只给了 host[:port] 时补一个根路径，避免拼出没有 path 的 URI
        return Some(if target_path.is_empty() {
            format!("{origin}/")
        } else {
            target.to_string()
        });
    }
    // 目标自带的查询串在「保留原路径」模式下没有意义，只取目录前缀
    let prefix = target_path
        .split('?')
        .next()
        .unwrap_or_default()
        .trim_end_matches('/');
    Some(format!("{origin}{prefix}{}", path_and_query(url)))
}

/// 拆成 `scheme://authority` 与 `/path?query` 两段；没有 `://` 时按相对地址处理。
fn split_origin(url: &str) -> (&str, &str) {
    let authority_start = url.find("://").map(|index| index + 3).unwrap_or(0);
    match url[authority_start..].find('/') {
        Some(offset) => (
            &url[..authority_start + offset],
            &url[authority_start + offset..],
        ),
        None => (url, ""),
    }
}

/// 取 URL 的路径与查询串，缺省为 `/`。
fn path_and_query(url: &str) -> &str {
    let authority_start = url.find("://").map(|index| index + 3).unwrap_or(0);
    match url[authority_start..].find('/') {
        Some(offset) => &url[authority_start + offset..],
        None => "/",
    }
}

fn glob_match(pattern: &str, text: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return text == pattern;
    }
    let mut cursor = 0usize;
    if !pattern.starts_with('*') {
        let head = parts[0];
        if !text.starts_with(head) {
            return false;
        }
        cursor = head.len();
    }
    for part in &parts[1..parts.len() - 1] {
        if part.is_empty() {
            continue;
        }
        let Some(offset) = text[cursor..].find(part) else {
            return false;
        };
        cursor += offset + part.len();
    }
    if pattern.ends_with('*') {
        return true;
    }
    let tail = parts[parts.len() - 1];
    tail.is_empty() || text[cursor.min(text.len())..].ends_with(tail) || text.ends_with(tail)
}

/// 极简 xorshift 随机数：只为抖动与丢包采样，不引额外依赖。
fn next_random() -> u64 {
    thread_local! {
        static STATE: std::cell::Cell<u64> = std::cell::Cell::new(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|value| value.as_nanos() as u64 | 1)
                .unwrap_or(0x2545_F491_4F6C_DD1D),
        );
    }
    STATE.with(|cell| {
        let mut value = cell.get();
        value ^= value << 13;
        value ^= value >> 7;
        value ^= value << 17;
        cell.set(value);
        value
    })
}

/// 本地映射没显式填 Content-Type 时，按扩展名或内容猜。
pub fn guess_content_type(path_or_body: &str, is_file: bool) -> String {
    let lowered = path_or_body.to_ascii_lowercase();
    let extension = if is_file {
        lowered.rsplit('.').next().unwrap_or_default()
    } else {
        ""
    };
    let guess = match extension {
        "json" => "application/json",
        "js" | "mjs" => "text/javascript",
        "css" => "text/css",
        "html" | "htm" => "text/html",
        "xml" => "application/xml",
        "txt" => "text/plain",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "ico" => "image/x-icon",
        "wasm" => "application/wasm",
        _ => {
            return if !is_file && lowered.trim_start().starts_with('{') {
                "application/json".to_string()
            } else {
                "text/plain".to_string()
            }
        }
    };
    guess.to_string()
}

/// 断点里请求头/响应头的逐跳头，重建时由代理重算。
/// 注意不含 content-encoding：未解码的压缩响应必须保留它。
pub fn is_hop_by_hop(name: &str) -> bool {
    matches!(
        name.trim().to_ascii_lowercase().as_str(),
        "content-length"
            | "transfer-encoding"
            | "connection"
            | "proxy-connection"
            | "keep-alive"
            | "upgrade"
            | "te"
            | "trailer"
    )
}
