//! 基准专用入口（`cargo run --release --features bench --example baseline`）。
//!
//! 这些函数只是把 `pub(crate)` 的实现包一层，避免为了跑基准把内部 API 全改成 public。
//! 默认构建不包含本模块。

use std::collections::HashMap;

use rusqlite::Connection;

use crate::docs::cache::DocsCache;
use crate::docs::model::{DocEditOverlay, DocsPreparePayload, DocsRenderPayload, DocsRenderResult};

pub use crate::docs::cache::DocsCache as BenchDocsCache;
pub use crate::docs::model::{
    DocsPreparePayload as BenchDocsPreparePayload, DocsRenderResult as BenchDocsRenderResult,
};

/// 构建项目 OpenAPI 3.0 文档
pub fn openapi_spec(conn: &Connection, project_id: i64) -> Result<serde_json::Value, String> {
    crate::services::openapi::build_openapi_spec(conn, project_id)
}

/// 两个版本之间的字段级 diff
pub fn revision_diff(
    conn: &Connection,
    request_id: i64,
    from: i64,
    to: i64,
) -> Result<crate::services::revision::RevisionDiff, String> {
    crate::services::revision::diff_revisions(conn, request_id, from, to)
}

/// 文档准备 + HTML 渲染（与界面预览同一条链路）
pub fn docs_render(
    conn: &Connection,
    cache: &DocsCache,
    payload: &DocsPreparePayload,
) -> Result<DocsRenderResult, String> {
    let prepared = crate::services::docs::prepare(conn, cache, payload)?;
    crate::services::docs::render(
        cache,
        &DocsRenderPayload {
            draft_id: prepared.draft_id,
            overlay: DocEditOverlay::default(),
            format: "html".to_string(),
        },
    )
}

/// 变量替换（文档生成热路径）
pub fn render_tracked(input: &str, vars: &HashMap<String, String>) -> String {
    let mut used = Vec::new();
    crate::util::render_tracked(input, vars, &mut used)
}
