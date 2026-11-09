//! 接口文档准备 / 渲染 / 导出
//!
//! 从 `commands/docs.rs` 下沉：命令层只负责取连接与草稿缓存，
//! 便于后续 CLI、文档站与测试共用同一套校验和上限。

use rusqlite::Connection;

use crate::docs::cache::DocsCache;
use crate::docs::{builder, model::*, overlay, render_html, render_markdown};
use crate::util::err;

pub const MAX_OUTLINE_BYTES: usize = 5 * 1024 * 1024;
pub const MAX_CONTENT_BYTES: usize = 20 * 1024 * 1024;

fn render_bundle(bundle: &DocBundle, format: &str) -> Result<String, String> {
    match format {
        "markdown" => Ok(render_markdown::render(bundle)),
        "html" => Ok(render_html::render(bundle)),
        other => Err(format!("E_FORMAT: 不支持的格式 {other}")),
    }
}

pub(crate) fn check_extension(path: &str, format: &str) -> Result<(), String> {
    let lower = path.to_ascii_lowercase();
    let matched = match format {
        "markdown" => lower.ends_with(".md"),
        "html" => lower.ends_with(".html") || lower.ends_with(".htm"),
        other => return Err(format!("E_FORMAT: 不支持的格式 {other}")),
    };
    if matched {
        Ok(())
    } else {
        Err(format!("E_FORMAT: 扩展名与格式不匹配（{format}）"))
    }
}

fn write_text(path: &str, content: &str) -> Result<(), String> {
    let path = crate::services::paths::prepare_write(path)?;
    std::fs::write(&path, content)
        .map_err(|error| format!("写入文件失败（{}）：{error}", path.display()))
}

pub fn prepare(
    conn: &Connection,
    cache: &DocsCache,
    payload: &DocsPreparePayload,
) -> Result<DocsPrepareResult, String> {
    let bundle = builder::build(conn, payload)?;
    let outline = builder::outline(&bundle);
    let size = serde_json::to_vec(&outline).map_err(err)?.len();
    if size > MAX_OUTLINE_BYTES {
        return Err("E_TOO_LARGE: 大纲超过 5 MB，请按集合分批导出".to_string());
    }
    let revision = bundle.revision.clone();
    let meta = DocMetaDefaults {
        title: bundle.scope.title.clone(),
        subtitle: bundle.subtitle.clone(),
    };
    let warnings = bundle.warnings.clone();
    let draft_id = cache.put(bundle);
    Ok(DocsPrepareResult {
        draft_id,
        revision,
        meta,
        outline,
        warnings,
    })
}

pub fn render(cache: &DocsCache, payload: &DocsRenderPayload) -> Result<DocsRenderResult, String> {
    let mut bundle = cache.get(&payload.draft_id)?;
    let warnings = overlay::apply(&mut bundle, &payload.overlay);
    let content = render_bundle(&bundle, &payload.format)?;
    if content.len() > MAX_CONTENT_BYTES {
        return Err("E_TOO_LARGE: 预览内容超过 20 MB，请改用 Markdown 或分批导出".to_string());
    }
    Ok(DocsRenderResult {
        bytes: content.len() as u64,
        content,
        warnings,
    })
}

pub fn export(
    conn: &Connection,
    cache: &DocsCache,
    payload: &DocsExportPayload,
) -> Result<DocsExportResult, String> {
    check_extension(&payload.path, &payload.format)?;
    let (bundle, mut warnings) = match &payload.source {
        DocsExportSource::Scope(prepare) => (builder::build(conn, prepare)?, Vec::new()),
        DocsExportSource::Draft { draft_id, overlay } => {
            let mut bundle = cache.get(draft_id)?;
            let warnings = overlay::apply(&mut bundle, overlay);
            (bundle, warnings)
        }
    };
    let content = render_bundle(&bundle, &payload.format)?;
    if content.len() > MAX_CONTENT_BYTES {
        return Err("E_TOO_LARGE: 导出内容超过 20 MB，请按集合分批导出".to_string());
    }
    write_text(&payload.path, &content)?;
    for warning in &bundle.warnings {
        if !warnings.contains(warning) {
            warnings.push(warning.clone());
        }
    }
    let scope = bundle.scope.kind.clone();
    let request_count = builder::request_count(&bundle);
    let collection_count = builder::collection_count(&bundle);
    Ok(DocsExportResult {
        path: payload.path.clone(),
        format: payload.format.clone(),
        scope,
        request_count,
        collection_count,
        bytes: content.len() as u64,
        warnings,
        pdf_engine: None,
        fallback_html_path: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_mismatched_extension() {
        assert!(check_extension("/tmp/a.md", "markdown").is_ok());
        assert!(check_extension("/tmp/a.html", "html").is_ok());
        assert!(check_extension("/tmp/a.htm", "html").is_ok());
        assert!(check_extension("/tmp/a.html", "markdown")
            .unwrap_err()
            .contains("E_FORMAT"));
        assert!(check_extension("/tmp/a.pdf", "pdf")
            .unwrap_err()
            .contains("E_FORMAT"));
    }
}
