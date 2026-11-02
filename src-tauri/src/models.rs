use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub color: Option<String>,
    pub sort_order: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionNode {
    pub id: i64,
    pub project_id: i64,
    pub parent_id: Option<i64>,
    pub name: String,
    pub sort_order: i64,
    pub children: Vec<CollectionNode>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestSummary {
    pub id: i64,
    pub collection_id: i64,
    pub name: String,
    pub method: String,
    pub url: String,
    pub sort_order: i64,
    pub updated_at: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestSearchHit {
    pub id: i64,
    pub collection_id: i64,
    pub collection_name: String,
    pub name: String,
    pub method: String,
    pub url: String,
    pub updated_at: i64,
}

fn default_true() -> bool {
    true
}

fn default_method() -> String {
    "GET".to_string()
}

fn default_body_type() -> String {
    "none".to_string()
}

fn default_timeout() -> u64 {
    30_000
}

fn default_connect_timeout() -> u64 {
    10_000
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyValueItem {
    #[serde(default)]
    pub key: String,
    #[serde(default)]
    pub value: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

/// 认证配置（Bearer / Basic / API Key），随请求保存
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RequestAuth {
    /// none / bearer / basic / apikey / oauth2
    pub kind: String,
    pub token: String,
    pub username: String,
    pub password: String,
    pub key: String,
    pub value: String,
    /// header / query
    pub add_to: String,
    /// OAuth2 Client Credentials
    #[serde(default)]
    pub token_url: String,
    #[serde(default)]
    pub client_id: String,
    #[serde(default)]
    pub client_secret: String,
    #[serde(default)]
    pub scope: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpRequestDetail {
    #[serde(default)]
    pub id: Option<i64>,
    pub collection_id: i64,
    pub name: String,
    #[serde(default = "default_method")]
    pub method: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub headers: Vec<KeyValueItem>,
    #[serde(default)]
    pub params: Vec<KeyValueItem>,
    #[serde(default = "default_body_type")]
    pub body_type: String,
    #[serde(default)]
    pub body: String,
    /// 各 Body 类型的草稿（JSON：{"json":"...","form":"...","raw":"..."}）
    #[serde(default)]
    pub body_meta: String,
    #[serde(default)]
    pub pre_script: String,
    #[serde(default)]
    pub post_script: String,
    #[serde(default)]
    pub assertions: String,
    #[serde(default)]
    pub auth: RequestAuth,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendRequestPayload {
    #[serde(default)]
    pub request_id: Option<i64>,
    #[serde(default)]
    pub name: Option<String>,
    pub method: String,
    pub url: String,
    #[serde(default)]
    pub headers: Vec<KeyValueItem>,
    #[serde(default)]
    pub params: Vec<KeyValueItem>,
    #[serde(default = "default_body_type")]
    pub body_type: String,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub env_id: Option<i64>,
    #[serde(default)]
    pub insecure: bool,
    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,
    /// 认证配置，为空表示不使用
    #[serde(default)]
    pub auth: Option<RequestAuth>,
    /// 是否自动携带本机 Cookie，默认开启
    #[serde(default)]
    pub cookie_enabled: Option<bool>,
    /// 执行来源：manual（默认）/ batch / suite / cli
    #[serde(default)]
    pub origin: Option<String>,
    /// 前端生成的请求标识，用于「停止」按钮定位在途请求
    #[serde(default)]
    pub request_key: Option<String>,
    /// 协议偏好：auto（默认）/ http1 / http2
    #[serde(default)]
    pub protocol: Option<String>,
    /// 是否跟随重定向；为空时沿用默认（跟随）
    #[serde(default)]
    pub follow_redirects: Option<bool>,
    /// 按请求覆盖连接超时
    #[serde(default)]
    pub connect_timeout_ms: Option<u64>,
    /// 重试配置
    #[serde(default)]
    pub retry: Option<RetryConfig>,
}

/// 幂等重试配置（4.6）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RetryConfig {
    #[serde(default)]
    pub enabled: bool,
    /// 最多尝试次数（含首次），上限 4
    #[serde(default = "default_retry_attempts")]
    pub max_attempts: u32,
    /// 允许对非幂等方法重试（默认关闭）
    #[serde(default)]
    pub allow_non_idempotent: bool,
}

fn default_retry_attempts() -> u32 {
    3
}

/// multipart/form-data 的表单字段
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FormField {
    pub key: String,
    pub value: String,
    pub enabled: bool,
    /// text / file
    pub kind: String,
}

/// 本机保存的 Cookie
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CookieItem {
    pub id: i64,
    pub domain: String,
    pub path: String,
    pub name: String,
    pub value: String,
    pub secure: bool,
    pub http_only: bool,
    pub expires_at: Option<i64>,
    pub updated_at: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpResult {
    pub status: u16,
    pub status_text: String,
    pub duration_ms: u64,
    /// 解压后的正文长度（与 `body_bytes` 保持一致）
    pub size_bytes: usize,
    pub headers: HashMap<String, String>,
    pub body: String,
    pub truncated: bool,
    pub error: Option<String>,
    /// text | json | binary | stream（二进制与超大响应落盘）
    #[serde(default)]
    pub body_kind: String,
    /// 落盘后的相对路径（相对数据目录），前端用 `response_file_path` 换成绝对路径
    #[serde(default)]
    pub body_file: Option<String>,
    /// 正文实际字节数（解压后）
    #[serde(default)]
    pub body_bytes: u64,
    /// 压缩态字节数（来自 Content-Length；gzip/br 时与 body_bytes 不同）
    #[serde(default)]
    pub raw_bytes: u64,
    /// 小片段 Base64（图片 / PDF 首页），供 `asset:` 不可用时兜底
    #[serde(default)]
    pub preview_base64: Option<String>,
    /// 耗时分解
    #[serde(default)]
    pub timings: Option<RequestTimings>,
    /// 实际协商到的协议：HTTP/1.1 / HTTP/2
    #[serde(default)]
    pub http_version: String,
    /// 手工跟随的重定向链
    #[serde(default)]
    pub redirects: Vec<RedirectHop>,
    /// 重试记录（含被放弃的尝试）
    #[serde(default)]
    pub retries: Vec<RetryAttempt>,
    /// 因 HTTP/2 被剔除的请求头
    #[serde(default)]
    pub ignored_headers: Vec<String>,
    /// 用户主动取消
    #[serde(default)]
    pub cancelled: bool,
}

/// 耗时分解（口径见技术方案 4.5）
///
/// `connectMs` 只统计「新建 TCP（含 TLS）」；复用连接时为 0 且 `reusedConnection = true`，
/// 不代表「连接极快」。代理场景下不含代理握手，UI 必须标注。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestTimings {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dns_ms: Option<f64>,
    pub connect_ms: f64,
    pub ttfb_ms: f64,
    pub download_ms: f64,
    pub total_ms: f64,
    pub reused_connection: bool,
    pub proxy: bool,
}

/// 重定向链上的一跳
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RedirectHop {
    pub status: u16,
    pub location: String,
    pub host: String,
    pub duration_ms: f64,
}

/// 一次重试尝试
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RetryAttempt {
    pub attempt: u32,
    pub status: u16,
    pub delay_ms: u64,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Environment {
    pub id: i64,
    pub project_id: i64,
    pub name: String,
    pub is_active: bool,
    #[serde(default)]
    pub script: String,
    pub variables: Vec<EnvVariable>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvVariable {
    #[serde(default)]
    pub id: Option<i64>,
    #[serde(default)]
    pub key: String,
    /// 敏感值只返回掩码；`has_value` 表示库里确实有值（用于提示「已设置」）
    #[serde(default)]
    pub value: String,
    #[serde(default)]
    pub is_secret: bool,
    #[serde(default)]
    pub sort_order: i64,
    #[serde(default)]
    pub has_value: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryItem {
    pub id: i64,
    pub request_id: Option<i64>,
    pub project_id: Option<i64>,
    pub request_name: Option<String>,
    pub method: String,
    pub url: String,
    pub status_code: Option<i64>,
    pub duration_ms: Option<i64>,
    /// 执行来源：manual / batch / suite / cli
    pub origin: String,
    pub created_at: i64,
    /// 耗时分解（JSON 字符串，前端解析后画瀑布）
    #[serde(default)]
    pub timings_json: Option<String>,
    /// text | json | binary | stream
    #[serde(default)]
    pub body_kind: Option<String>,
    /// 响应文件相对路径（二进制或超大响应）
    #[serde(default)]
    pub body_file: Option<String>,
}

/// 请求历史统计（用于清理前预览数据规模）
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryStats {
    pub total: i64,
    pub oldest_at: Option<i64>,
    pub newest_at: Option<i64>,
    pub runs_total: i64,
}

/// 历史数据清理结果
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupResult {
    pub history: u32,
    pub runs: u32,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestRun {
    pub id: i64,
    pub request_id: i64,
    pub method: String,
    pub url: String,
    pub headers: Vec<KeyValueItem>,
    pub params: Vec<KeyValueItem>,
    pub body_type: String,
    pub body: String,
    pub response_status: i64,
    pub response_status_text: String,
    pub response_headers: HashMap<String, String>,
    pub response_body: String,
    pub duration_ms: i64,
    pub size_bytes: i64,
    pub error: Option<String>,
    /// 执行来源：manual / batch / suite / cli
    pub origin: String,
    pub created_at: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub version: String,
    pub data_dir: String,
    pub db_path: String,
}

/// 工作台总览统计
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceStats {
    pub project_count: i64,
    pub collection_count: i64,
    pub request_count: i64,
    pub environment_count: i64,
    pub history_count: i64,
    pub report_count: i64,
    pub report_request_total: i64,
    pub report_success_total: i64,
    pub recent_history: i64,
    pub recent_reports: i64,
}

/// 数据大屏：按日请求趋势
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyPoint {
    pub date: String,
    pub total: i64,
    pub success: i64,
    pub avg_rt: f64,
}

/// 数据大屏：状态码分组
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeBucket {
    pub label: String,
    pub count: i64,
}

/// 数据大屏：调用最频繁的接口
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TopRequest {
    pub name: String,
    pub method: String,
    pub url: String,
    pub count: i64,
    pub avg_rt: f64,
    pub success_rate: f64,
}

/// 数据大屏：最近压测报告走势
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportPoint {
    pub id: i64,
    pub name: String,
    pub p95_rt: f64,
    pub error_rate: f64,
    pub total_requests: i64,
    pub created_at: i64,
}

/// 数据大屏：按小时请求量
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HourPoint {
    pub hour: i64,
    pub count: i64,
}

/// 数据大屏：项目维度的调用量
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectPoint {
    pub name: String,
    pub count: i64,
    pub avg_rt: f64,
    pub success_rate: f64,
}

/// 数据大屏聚合结果
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DashboardMetrics {
    pub days: Vec<DailyPoint>,
    pub code_buckets: Vec<CodeBucket>,
    pub top_requests: Vec<TopRequest>,
    pub report_points: Vec<ReportPoint>,
    pub method_buckets: Vec<CodeBucket>,
    pub hour_buckets: Vec<HourPoint>,
    pub latency_buckets: Vec<CodeBucket>,
    pub top_projects: Vec<ProjectPoint>,
}

fn default_concurrent() -> u32 {
    10
}

fn default_duration() -> u32 {
    60
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StressStage {
    pub concurrent: u32,
    #[serde(default = "default_duration")]
    pub duration_sec: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SlaTargets {
    #[serde(default)]
    pub p95_ms: Option<f64>,
    #[serde(default)]
    pub p99_ms: Option<f64>,
    #[serde(default)]
    pub error_rate: Option<f64>,
    #[serde(default)]
    pub min_rps: Option<f64>,
}

/// 逐请求明细的采集策略
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RecordDetailMode {
    /// 每请求都记录明细
    Full,
    /// 按比例采样记录
    Sample,
    /// 完全不记录明细（同时关闭响应体读取）
    Off,
    /// 由引擎按预估/实测 QPS 自动定档，只降不升
    #[default]
    Auto,
}

impl RecordDetailMode {
    pub fn label(&self) -> &'static str {
        match self {
            RecordDetailMode::Full => "full",
            RecordDetailMode::Sample => "sample",
            RecordDetailMode::Off => "off",
            RecordDetailMode::Auto => "auto",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StressConfig {
    #[serde(default)]
    pub request_id: Option<i64>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default = "default_method")]
    pub method: String,
    pub url: String,
    #[serde(default)]
    pub headers: Vec<KeyValueItem>,
    #[serde(default)]
    pub params: Vec<KeyValueItem>,
    #[serde(default = "default_body_type")]
    pub body_type: String,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub env_id: Option<i64>,
    /// 运行期变量：优先级高于环境变量，不落库
    #[serde(default)]
    pub vars: HashMap<String, String>,
    /// 认证配置，与手工请求一致（Bearer / Basic / API Key）
    #[serde(default)]
    pub auth: Option<RequestAuth>,
    /// 是否自动携带本机 Cookie，默认开启
    #[serde(default)]
    pub cookie_enabled: Option<bool>,
    #[serde(default)]
    pub insecure: bool,
    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,
    /// 建立连接的超时（毫秒）
    #[serde(default = "default_connect_timeout")]
    pub connect_timeout_ms: u64,
    /// 是否跟随 3xx 重定向
    #[serde(default = "default_true")]
    pub follow_redirects: bool,
    /// 自动停止：错误率阈值（%），需同时满足最少请求数
    #[serde(default)]
    pub stop_error_rate: Option<f64>,
    /// 自动停止：触发判定的最少请求数
    #[serde(default)]
    pub stop_min_requests: Option<u32>,
    #[serde(default = "default_concurrent")]
    pub concurrent: u32,
    #[serde(default = "default_duration")]
    pub duration_sec: u32,
    #[serde(default)]
    pub ramp_up_sec: u32,
    #[serde(default)]
    pub think_time_ms: u64,
    /// 阶梯加压阶段，非空时忽略 concurrent / duration_sec
    #[serde(default)]
    pub stages: Vec<StressStage>,
    /// 全局限速（每秒请求数），0 或空表示不限速
    #[serde(default)]
    pub target_rps: Option<f64>,
    #[serde(default)]
    pub sla: Option<SlaTargets>,
    /// 逐请求明细采集策略，默认 auto
    #[serde(default)]
    pub record_detail: RecordDetailMode,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportSample {
    pub elapsed_sec: u64,
    pub concurrent: u32,
    pub qps: f64,
    pub avg_rt: f64,
    pub p50_rt: f64,
    pub p95_rt: f64,
    pub p99_rt: f64,
    pub ttfb_avg: f64,
    pub bytes: u64,
    pub success_count: u64,
    pub error_count: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportError {
    pub elapsed_sec: i64,
    pub error_type: String,
    pub error_message: String,
    pub request_url: String,
    pub response_status: Option<i64>,
    pub response_body: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportRequestRecord {
    pub elapsed_ms: i64,
    pub time_ms: i64,
    pub method: String,
    pub url: String,
    pub status: i64,
    pub latency_ms: i64,
    pub ttfb_ms: i64,
    pub bytes: i64,
    pub success: bool,
    pub body: Option<String>,
    /// 查询参数（原始 query string）
    pub params: Option<String>,
    /// 请求头（JSON 数组 [[key, value], ...]）
    pub headers: Option<String>,
    /// 响应头（JSON 数组 [[key, value], ...]）
    pub response_headers: Option<String>,
    /// 响应体（截断保留）
    pub response_body: Option<String>,
    pub error_type: Option<String>,
    pub error_message: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportRequestsPage {
    pub total: i64,
    pub items: Vec<ReportRequestRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusCount {
    pub code: i64,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorTypeCount {
    pub error_type: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistogramBucket {
    pub label: String,
    pub min_ms: f64,
    pub max_ms: f64,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StageSummary {
    pub index: i64,
    pub concurrent: i64,
    pub duration_sec: i64,
    pub requests: i64,
    pub errors: i64,
    pub avg_qps: f64,
    pub avg_rt: f64,
    pub p95_rt: f64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StressReport {
    pub id: i64,
    pub plan_name: String,
    pub request_id: Option<i64>,
    pub total_requests: i64,
    pub success_count: i64,
    pub error_count: i64,
    pub avg_rt: f64,
    pub min_rt: f64,
    pub max_rt: f64,
    pub p50_rt: f64,
    pub p75_rt: f64,
    pub p90_rt: f64,
    pub p95_rt: f64,
    pub p99_rt: f64,
    pub p999_rt: f64,
    pub stddev_rt: f64,
    pub ttfb_avg: f64,
    pub ttfb_p95: f64,
    pub apdex: f64,
    pub bytes_total: i64,
    pub max_qps: f64,
    pub avg_qps: f64,
    pub target_rps: f64,
    pub error_rate: f64,
    pub concurrency: i64,
    pub status: String,
    pub start_time: Option<i64>,
    pub end_time: Option<i64>,
    pub created_at: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportDetail {
    pub report: StressReport,
    pub samples: Vec<ReportSample>,
    pub errors: Vec<ReportError>,
    pub status_codes: Vec<StatusCount>,
    pub error_types: Vec<ErrorTypeCount>,
    pub histogram: Vec<HistogramBucket>,
    pub stages: Vec<StageSummary>,
    pub config: Option<serde_json::Value>,
    /// 用户可编辑的测试场景描述
    pub scenario_text: Option<String>,
    /// 用户可编辑的报告总结
    pub summary_text: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StressProgress {
    pub name: String,
    pub running: bool,
    pub elapsed_sec: u64,
    pub total: u64,
    pub success: u64,
    pub errors: u64,
    pub rps: f64,
    pub avg_rt: f64,
    pub p95_rt: f64,
    pub p99_rt: f64,
    pub concurrent: u32,
    pub stage_index: i64,
    pub active_concurrent: u32,
    pub status_codes: Vec<StatusCount>,
    /// 本次新增的采样点（增量，非全量）
    pub samples: Vec<ReportSample>,
    /// 当前生效的明细采集模式：full / sample / off
    pub record_mode: String,
    /// 一次性提示（如自动降档），前端 toast 后即丢弃
    pub notice: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StressDone {
    pub report_id: Option<i64>,
    pub name: String,
    pub status: String,
    pub total: u64,
    pub success: u64,
    pub errors: u64,
    pub elapsed_sec: u64,
    pub message: Option<String>,
}

fn default_ai_kind() -> String {
    "openai".to_string()
}

/// AI 服务商配置，kind 决定协议：openai / anthropic / gemini
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiProvider {
    #[serde(default)]
    pub id: Option<i64>,
    pub name: String,
    #[serde(default = "default_ai_kind")]
    pub kind: String,
    pub base_url: String,
    pub model: String,
    /// 明文只在内存与传输层出现，落库前统一加密；列表接口不再返回明文
    #[serde(default)]
    pub api_key: String,
    /// 列表回显用掩码（sk-****abcd），由 Rust 侧即时计算
    #[serde(default)]
    pub api_key_masked: String,
    /// 是否已配置 Key：编辑时留空表示沿用原 Key
    #[serde(default)]
    pub has_key: bool,
    #[serde(default)]
    pub temperature: Option<f64>,
    /// 单次生成最大 token，0 = 按协议默认
    #[serde(default)]
    pub max_tokens: Option<i64>,
    /// 单次请求超时秒数，0 = 默认 300
    #[serde(default)]
    pub timeout_secs: Option<i64>,
    #[serde(default)]
    pub is_default: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiChatPayload {
    pub provider_id: i64,
    pub messages: Vec<AiMessage>,
    #[serde(default)]
    pub temperature: Option<f64>,
    /// 在途请求标识：前端用同一个 id 调用 ai_chat_cancel 停止生成
    #[serde(default)]
    pub request_id: Option<String>,
}

/// 流式事件：delta 增量文本 / reasoning 思考 / done 结束 / cancelled 已停止 / error 错误
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiStreamEvent {
    pub kind: String,
    pub text: String,
}

/// AI 会话：一次连续对话
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiSession {
    pub id: i64,
    pub title: String,
    pub provider_id: Option<i64>,
    pub message_count: i64,
    #[serde(default)]
    pub pinned: bool,
    /// 较早对话压缩后的结构化摘要，从下一轮开始替代原文进入上下文
    #[serde(default)]
    pub summary: Option<String>,
    /// 摘要覆盖到的消息时间（毫秒），早于它的消息不再逐条发送
    #[serde(default)]
    pub summary_until: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiStoredMessage {
    pub id: i64,
    pub session_id: i64,
    pub role: String,
    pub content: String,
    /// 模型思考过程（仅助手消息），用于 Agent 行为回溯
    pub reasoning: Option<String>,
    pub created_at: i64,
}

/// AI 工具调用流水
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiToolLog {
    pub id: i64,
    pub session_id: Option<i64>,
    pub tool: String,
    pub args: String,
    pub ok: bool,
    pub duration_ms: i64,
    pub result_preview: String,
    /// 第几轮工具调用（1 起），用于复盘时间线
    #[serde(default)]
    pub round: i64,
    /// 同一轮任务的链路标识
    #[serde(default)]
    pub trace_id: Option<String>,
    /// 前端等待超时（工具仍在后台执行）
    #[serde(default)]
    pub timeout: bool,
    pub created_at: i64,
}

/// 工具流水分页结果
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiToolLogPage {
    pub items: Vec<AiToolLog>,
    pub total: i64,
}

/// 工具流水按天汇总
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiToolDayStat {
    /// 形如 2026-10-27
    pub date: String,
    pub total: i64,
    pub failed: i64,
}

/// 工具流水按工具汇总
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiToolUsageStat {
    pub tool: String,
    pub total: i64,
    pub failed: i64,
    pub avg_duration_ms: i64,
}

/// 工具流水看板
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiToolLogStats {
    pub total: i64,
    pub ok: i64,
    pub failed: i64,
    pub avg_duration_ms: i64,
    pub last_24h: i64,
    pub days: Vec<AiToolDayStat>,
    pub tools: Vec<AiToolUsageStat>,
}

/// 单个模型（服务商）的用量与准确率
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiUsageStat {
    pub provider_id: Option<i64>,
    pub provider_name: String,
    pub model: String,
    pub calls: i64,
    pub ok_calls: i64,
    /// 成功调用占比 0~1
    pub success_rate: f64,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub total_tokens: i64,
    pub avg_latency_ms: i64,
    /// 该模型所在会话产生的工具调用数
    pub tool_calls: i64,
    pub tool_ok: i64,
    pub tool_success_rate: f64,
    pub last_used_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiUsageDay {
    pub day: String,
    pub calls: i64,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub total_tokens: i64,
}

/// 单次模型调用记录（调一次记一条，用于分页查看）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiUsageLog {
    pub id: i64,
    pub provider_name: String,
    pub model: String,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub total_tokens: i64,
    pub ok: bool,
    pub latency_ms: i64,
    pub error: String,
    /// 这轮实际发给模型的内容（本轮用户消息，工具续跑时附带首问）
    pub request_preview: String,
    pub created_at: i64,
}

/// 用量明细分页结果
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiUsagePage {
    pub items: Vec<AiUsageLog>,
    pub total: i64,
}

/// 模型用量看板
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiUsageStats {
    pub days: i64,
    pub total_calls: i64,
    pub ok_calls: i64,
    pub success_rate: f64,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub total_tokens: i64,
    pub avg_latency_ms: i64,
    pub tool_calls: i64,
    pub tool_ok: i64,
    pub tool_success_rate: f64,
    pub items: Vec<AiUsageStat>,
    pub daily: Vec<AiUsageDay>,
}

// ---------- v1.0.8 自动化测试场景 ----------

fn default_case_kind() -> String {
    "request".to_string()
}

fn default_case_timeout() -> i64 {
    30_000
}

fn default_dataset_format() -> String {
    "csv".to_string()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestSuite {
    pub id: i64,
    pub project_id: i64,
    pub name: String,
    pub description: Option<String>,
    pub env_id: Option<i64>,
    pub tags: Option<String>,
    pub python_path: Option<String>,
    pub config_json: Option<String>,
    pub setup_script: Option<String>,
    pub teardown_script: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SuiteSummary {
    pub id: i64,
    pub project_id: i64,
    pub name: String,
    pub description: Option<String>,
    pub env_id: Option<i64>,
    pub env_name: Option<String>,
    pub tags: Option<String>,
    pub python_path: Option<String>,
    pub config_json: Option<String>,
    pub case_count: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestCase {
    pub id: i64,
    pub suite_id: i64,
    pub name: String,
    pub kind: String,
    pub request_id: Option<i64>,
    pub plan_id: Option<i64>,
    pub dataset_id: Option<i64>,
    pub depends_on: Option<String>,
    pub sort_order: i64,
    pub enabled: bool,
    pub timeout_ms: i64,
    pub retries: i64,
    pub retry_on_failure: bool,
    pub continue_on_failure: bool,
    pub tags: Option<String>,
    pub variables_json: Option<String>,
    pub assertions_json: Option<String>,
    pub pre_script: Option<String>,
    pub post_script: Option<String>,
    pub main_script: Option<String>,
    pub stress_json: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestDataset {
    pub id: i64,
    pub suite_id: i64,
    pub name: String,
    pub format: String,
    pub path: Option<String>,
    /// 列表接口不返回内容，按 id 单独取
    pub content: Option<String>,
    pub row_count: i64,
    pub sha256: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SuiteDetail {
    pub suite: TestSuite,
    pub cases: Vec<TestCase>,
    pub datasets: Vec<TestDataset>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestSuitePayload {
    #[serde(default)]
    pub id: Option<i64>,
    pub project_id: i64,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub env_id: Option<i64>,
    #[serde(default)]
    pub tags: Option<String>,
    #[serde(default)]
    pub python_path: Option<String>,
    #[serde(default)]
    pub config_json: Option<String>,
    #[serde(default)]
    pub setup_script: Option<String>,
    #[serde(default)]
    pub teardown_script: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestCasePayload {
    #[serde(default)]
    pub id: Option<i64>,
    pub suite_id: i64,
    pub name: String,
    #[serde(default = "default_case_kind")]
    pub kind: String,
    #[serde(default)]
    pub request_id: Option<i64>,
    #[serde(default)]
    pub plan_id: Option<i64>,
    #[serde(default)]
    pub dataset_id: Option<i64>,
    #[serde(default)]
    pub depends_on: Option<String>,
    #[serde(default)]
    pub sort_order: i64,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_case_timeout")]
    pub timeout_ms: i64,
    #[serde(default)]
    pub retries: i64,
    #[serde(default)]
    pub retry_on_failure: bool,
    #[serde(default)]
    pub continue_on_failure: bool,
    #[serde(default)]
    pub tags: Option<String>,
    #[serde(default)]
    pub variables_json: Option<String>,
    #[serde(default)]
    pub assertions_json: Option<String>,
    #[serde(default)]
    pub pre_script: Option<String>,
    #[serde(default)]
    pub post_script: Option<String>,
    #[serde(default)]
    pub main_script: Option<String>,
    #[serde(default)]
    pub stress_json: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestDatasetPayload {
    #[serde(default)]
    pub id: Option<i64>,
    pub suite_id: i64,
    pub name: String,
    #[serde(default = "default_dataset_format")]
    pub format: String,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub content: Option<String>,
}

// ---------- v1.0.8 套件运行 ----------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestRun {
    pub id: i64,
    pub suite_id: i64,
    pub env_id: Option<i64>,
    pub env_name: Option<String>,
    pub trigger: String,
    pub status: String,
    pub total: i64,
    pub passed: i64,
    pub failed: i64,
    pub skipped: i64,
    pub errored: i64,
    pub duration_ms: i64,
    pub config_json: Option<String>,
    pub summary_json: Option<String>,
    pub app_version: Option<String>,
    pub python_version: Option<String>,
    pub report_dir: Option<String>,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestRunCase {
    pub id: i64,
    pub run_id: i64,
    pub case_id: Option<i64>,
    pub name: String,
    pub kind: String,
    pub status: String,
    pub attempts: i64,
    pub duration_ms: i64,
    pub http_duration_ms: i64,
    pub python_duration_ms: i64,
    pub request_snapshot: Option<String>,
    pub response_snapshot: Option<String>,
    pub assertions_total: i64,
    pub assertions_failed: i64,
    pub error: Option<String>,
    pub logs: Option<String>,
    pub python_version: Option<String>,
    pub script_hash: Option<String>,
    pub report_id: Option<i64>,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestRunAssertion {
    pub id: i64,
    pub run_case_id: i64,
    pub name: String,
    pub passed: bool,
    pub message: Option<String>,
    pub duration_ms: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunDetail {
    pub run: TestRun,
    pub cases: Vec<TestRunCase>,
    pub assertions: Vec<TestRunAssertion>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunSummary {
    pub id: i64,
    pub suite_id: i64,
    pub suite_name: Option<String>,
    pub env_name: Option<String>,
    pub trigger: String,
    pub status: String,
    pub total: i64,
    pub passed: i64,
    pub failed: i64,
    pub skipped: i64,
    pub errored: i64,
    pub duration_ms: i64,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
    pub created_at: i64,
}

/// 启动运行时允许覆盖的配置（CLI / AI / UI 共用）
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RunOverrides {
    pub vars: Option<HashMap<String, String>>,
    pub fail_fast: Option<bool>,
    pub env_id: Option<i64>,
    /// 仅本次运行生效的额外密钥（套件包 / --env-file 注入）
    pub secrets: Option<HashMap<String, String>>,
}

/// 预检单条结果
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreflightItem {
    /// error（阻止运行）/ warning（可运行但需注意）
    pub level: String,
    /// 机器可读的检查项编码，如 MISSING_VARIABLE / JS_HOOK_SKIPPED
    pub code: String,
    pub message: String,
    pub case_id: Option<i64>,
    pub case_name: Option<String>,
    pub hint: Option<String>,
}

/// 套件运行前预检报告
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreflightReport {
    pub suite_id: i64,
    pub suite_name: String,
    pub env_id: Option<i64>,
    pub env_name: Option<String>,
    pub cookie_mode: String,
    pub strict_scripts: bool,
    pub total_cases: i64,
    pub enabled_cases: i64,
    pub errors: i64,
    pub warnings: i64,
    /// 无 error 时可直接运行；strictScripts 会把跳过项升级为 error
    pub runnable: bool,
    pub items: Vec<PreflightItem>,
}
