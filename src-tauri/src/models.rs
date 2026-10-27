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
    pub size_bytes: usize,
    pub headers: HashMap<String, String>,
    pub body: String,
    pub truncated: bool,
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
    #[serde(default)]
    pub value: String,
    #[serde(default)]
    pub is_secret: bool,
    #[serde(default)]
    pub sort_order: i64,
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
    pub created_at: i64,
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
    pub samples: Vec<ReportSample>,
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
    /// 明文只在内存与传输层出现，落库前统一加密
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub temperature: Option<f64>,
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
}

/// 流式事件：delta 增量文本 / done 结束 / error 错误
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
    pub total_tokens: i64,
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
