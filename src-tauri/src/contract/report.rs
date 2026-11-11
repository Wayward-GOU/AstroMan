//! 契约变更报告（Markdown / HTML），供前端预览与 CI 归档

use serde_json::Value;

use super::diff::{ContractChange, ContractDiff};

fn value_text(value: &Value) -> String {
    if value.is_null() {
        return "-".to_string();
    }
    let text = match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    };
    let mut clipped: String = text.chars().take(120).collect();
    if text.chars().count() > 120 {
        clipped.push('…');
    }
    clipped
}

fn change_rows(items: &[ContractChange]) -> Vec<[String; 6]> {
    items
        .iter()
        .map(|item| {
            [
                if item.method.is_empty() {
                    item.path.clone()
                } else {
                    format!("{} {}", item.method.to_uppercase(), item.path)
                },
                item.field.clone(),
                item.kind.clone(),
                item.message.clone(),
                value_text(&item.before),
                value_text(&item.after),
            ]
        })
        .collect()
}

pub fn render_markdown(diff: &ContractDiff, baseline_label: &str) -> String {
    let mut out = String::new();
    out.push_str("# 契约变更报告\n\n");
    out.push_str(&format!("- 基线：{baseline_label}\n"));
    out.push_str(&format!(
        "- 当前接口：{} 个路径 / {} 个操作\n",
        diff.summary.paths, diff.summary.operations
    ));
    out.push_str(&format!(
        "- 破坏性：{} / 兼容性：{} / 提示：{}\n",
        diff.summary.breaking, diff.summary.compatible, diff.summary.warnings
    ));
    if !diff.schema_missing.is_empty() {
        out.push_str(&format!(
            "- 无返回实体（响应侧不参与判定）：{}\n",
            diff.schema_missing.join("、")
        ));
    }
    out.push('\n');

    for (title, items) in [
        ("破坏性变更", &diff.breaking),
        ("兼容性变更", &diff.compatible),
        ("提示", &diff.warnings),
    ] {
        out.push_str(&format!("## {title}（{}）\n\n", items.len()));
        if items.is_empty() {
            out.push_str("无\n\n");
            continue;
        }
        out.push_str("| 接口 | 字段 | 类型 | 说明 | 变更前 | 变更后 |\n");
        out.push_str("|---|---|---|---|---|---|\n");
        for row in change_rows(items) {
            out.push_str(&format!(
                "| {} | {} | {} | {} | {} | {} |\n",
                row[0].replace('|', "\\|"),
                row[1].replace('|', "\\|"),
                row[2],
                row[3].replace('|', "\\|"),
                row[4].replace('|', "\\|"),
                row[5].replace('|', "\\|"),
            ));
        }
        out.push('\n');
    }
    out
}

fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

pub fn render_html(diff: &ContractDiff, baseline_label: &str) -> String {
    let mut sections = String::new();
    for (title, items) in [
        ("破坏性变更", &diff.breaking),
        ("兼容性变更", &diff.compatible),
        ("提示", &diff.warnings),
    ] {
        sections.push_str(&format!("<h2>{title}（{}）</h2>", items.len()));
        if items.is_empty() {
            sections.push_str("<p class=\"empty\">无</p>");
            continue;
        }
        sections.push_str(
            "<table><thead><tr><th>接口</th><th>字段</th><th>类型</th><th>说明</th><th>变更前</th><th>变更后</th></tr></thead><tbody>",
        );
        for row in change_rows(items) {
            sections.push_str("<tr>");
            for cell in row {
                sections.push_str(&format!("<td>{}</td>", escape_html(&cell)));
            }
            sections.push_str("</tr>");
        }
        sections.push_str("</tbody></table>");
    }
    let missing = if diff.schema_missing.is_empty() {
        String::new()
    } else {
        format!(
            "<p class=\"missing\">无返回实体（响应侧不参与判定）：{}</p>",
            escape_html(&diff.schema_missing.join("、"))
        )
    };
    format!(
        r#"<!doctype html>
<html lang="zh-CN"><head><meta charset="utf-8">
<title>契约变更报告</title>
<style>
body{{font-family:-apple-system,"Segoe UI","Microsoft YaHei",sans-serif;margin:32px;color:#1f2329;line-height:1.6}}
h1{{font-size:22px;margin:0 0 12px}} h2{{font-size:16px;margin:24px 0 8px}}
.meta{{color:#646a73;font-size:13px}} .missing{{color:#ad6800;font-size:13px}}
table{{border-collapse:collapse;width:100%;font-size:13px}}
th,td{{border:1px solid #e5e6eb;padding:6px 8px;text-align:left;vertical-align:top}}
th{{background:#f7f8fa}} .empty{{color:#8f959e;font-size:13px}}
</style></head><body>
<h1>契约变更报告</h1>
<p class="meta">基线：{}<br>当前接口：{} 个路径 / {} 个操作<br>破坏性 {} / 兼容性 {} / 提示 {}</p>
{}{}
</body></html>"#,
        escape_html(baseline_label),
        diff.summary.paths,
        diff.summary.operations,
        diff.summary.breaking,
        diff.summary.compatible,
        diff.summary.warnings,
        missing,
        sections
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::diff::ContractChange;

    fn change(message: &str) -> ContractChange {
        ContractChange {
            path: "/users".into(),
            method: "post".into(),
            field: "responses.200.data".into(),
            kind: "removed_response_field".into(),
            message: message.into(),
            before: Value::Null,
            after: Value::Null,
        }
    }

    #[test]
    fn markdown_lists_counts_and_rows() {
        let mut diff = ContractDiff::default();
        diff.breaking.push(change("响应字段已删除：data <id>"));
        diff.schema_missing.push("GET /health".into());
        diff.summary = crate::contract::diff::ContractSummary {
            breaking: 1,
            paths: 2,
            operations: 3,
            ..Default::default()
        };
        let text = render_markdown(&diff, "v1.0.0 (#3)");
        assert!(text.contains("破坏性：1"));
        assert!(text.contains("POST /users"));
        assert!(text.contains("GET /health"));
        assert!(text.contains("v1.0.0 (#3)"));
    }

    #[test]
    fn html_escapes_content() {
        let mut diff = ContractDiff::default();
        diff.warnings.push(change("<script>alert(1)</script>"));
        let html = render_html(&diff, "v1");
        assert!(html.contains("&lt;script&gt;"));
        assert!(!html.contains("<script>alert(1)</script>"));
    }
}
