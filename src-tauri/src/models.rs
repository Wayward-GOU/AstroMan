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

#[derive(Debug, Serialize, Deserialize)]
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
    pub request_name: Option<String>,
    pub method: String,
    pub url: String,
    pub status_code: Option<i64>,
    pub duration_ms: Option<i64>,
    pub created_at: i64,
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
