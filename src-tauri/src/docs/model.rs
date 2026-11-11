use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// 值级脱敏统一使用的掩码，与 secret 模块保持一致
pub const MASK: &str = crate::secret::MASK;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocBundle {
    pub generated_at: i64,
    pub app_version: String,
    pub revision: String,
    pub subtitle: String,
    /// true 时生成时间精确到时分，false 时只到日期
    pub include_generated_at: bool,
    /// 打印 / PDF 时每个接口是否另起一页
    pub page_break_per_request: bool,
    pub scope: DocScopeInfo,
    pub groups: Vec<DocGroup>,
    pub environments: Vec<DocEnvironment>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocScopeInfo {
    pub kind: String,
    pub title: String,
    pub project_name: String,
    pub target_id: i64,
    pub environment_id: Option<i64>,
    pub environment_name: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocGroup {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub path: Vec<String>,
    pub requests: Vec<DocRequest>,
    pub children: Vec<DocGroup>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocRequest {
    pub id: i64,
    pub name: String,
    pub method: String,
    pub url: String,
    pub path: String,
    pub description: String,
    pub query: Vec<DocParam>,
    pub path_params: Vec<DocParam>,
    pub headers: Vec<DocParam>,
    pub auth: DocAuth,
    pub body: DocBody,
    pub request_entity: Option<DocEntity>,
    pub response_entity: Option<DocEntity>,
    pub response: Option<DocResponse>,
    pub scripts: Option<DocScripts>,
    pub assertions: Option<String>,
}

/// 接口实体：已在构建阶段拍平成有序字段表，渲染器只需按 `depth` 缩进
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocEntity {
    pub fields: Vec<DocEntityField>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocEntityField {
    /// 点号路径，数组字段带 `[]` 后缀（如 data.roles[]）
    pub name: String,
    pub type_name: String,
    pub required: bool,
    pub description: String,
    pub stale: bool,
    pub depth: u16,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocParam {
    pub name: String,
    pub value: String,
    pub required: bool,
    pub location: String,
    pub example_source: String,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocAuth {
    pub kind: String,
    pub add_to: String,
    pub key_name: String,
    pub note: String,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocBody {
    /// none | json | raw | text | form | multipart | binary | graphql
    pub kind: String,
    pub text: String,
    pub fields: Vec<DocParam>,
}

/// 最近一次成功运行的响应示例（响应头与响应体均已脱敏、截断）
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocResponse {
    pub status: i64,
    pub status_text: String,
    pub headers: Vec<DocParam>,
    pub body: String,
    pub truncated: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocScripts {
    pub pre: String,
    pub post: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocEnvironment {
    pub name: String,
    pub is_active: bool,
    pub variables: Vec<DocEnvVar>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocEnvVar {
    pub key: String,
    pub value: String,
    pub is_secret: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocsPreparePayload {
    pub scope: String,
    pub target_id: i64,
    pub environment_id: Option<i64>,
    #[serde(default)]
    pub options: DocsOptions,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase", default)]
pub struct DocsOptions {
    pub include_environment: bool,
    pub include_scripts: bool,
    pub include_assertions: bool,
    pub include_response_example: bool,
    pub resolve_environment: bool,
    pub use_run_examples: bool,
    pub auto_examples: bool,
    pub run_examples_max_age_days: u32,
    pub page_break_per_request: bool,
    pub include_generated_at: bool,
}

/// 必须手写默认值：布尔字段用 derive(Default) 会全部变成 false
impl Default for DocsOptions {
    fn default() -> Self {
        Self {
            include_environment: true,
            include_scripts: false,
            include_assertions: false,
            include_response_example: true,
            resolve_environment: true,
            use_run_examples: true,
            auto_examples: true,
            run_examples_max_age_days: 30,
            page_break_per_request: true,
            include_generated_at: false,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocOutline {
    pub groups: Vec<DocOutlineGroup>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocOutlineGroup {
    pub key: String,
    pub name: String,
    pub path: Vec<String>,
    pub requests: Vec<DocOutlineRequest>,
    pub children: Vec<DocOutlineGroup>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocOutlineRequest {
    pub key: String,
    pub name: String,
    pub method: String,
    pub url: String,
    pub description: String,
    pub included: bool,
    pub params: Vec<DocOutlineField>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocOutlineField {
    pub key: String,
    pub name: String,
    pub location: String,
    pub value: String,
    pub source: String,
    pub sensitive: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocEditOverlay {
    pub revision: String,
    #[serde(default)]
    pub meta: DocEditMeta,
    #[serde(default)]
    pub nodes: BTreeMap<String, DocNodeEdit>,
    #[serde(default)]
    pub excluded: BTreeSet<String>,
    #[serde(default)]
    pub order: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocNodeEdit {
    pub description: Option<String>,
    #[serde(default)]
    pub example_values: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocEditMeta {
    pub title: Option<String>,
    pub subtitle: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocMetaDefaults {
    pub title: String,
    pub subtitle: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocsPrepareResult {
    pub draft_id: String,
    pub revision: String,
    pub meta: DocMetaDefaults,
    pub outline: DocOutline,
    pub warnings: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocsRenderPayload {
    pub draft_id: String,
    pub overlay: DocEditOverlay,
    pub format: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocsRenderResult {
    pub content: String,
    pub bytes: u64,
    pub warnings: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum DocsExportSource {
    Scope(DocsPreparePayload),
    Draft {
        draft_id: String,
        overlay: DocEditOverlay,
    },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocsExportPayload {
    pub format: String,
    pub path: String,
    pub source: DocsExportSource,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocsExportResult {
    pub path: String,
    pub format: String,
    pub scope: String,
    pub request_count: usize,
    pub collection_count: usize,
    pub bytes: u64,
    pub warnings: Vec<String>,
    pub pdf_engine: Option<String>,
    pub fallback_html_path: Option<String>,
}
