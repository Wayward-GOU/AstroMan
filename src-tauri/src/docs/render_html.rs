use super::model::{DocBody, DocBundle, DocEntity, DocGroup, DocParam, DocRequest, DocResponse};
use chrono::TimeZone;

fn generated_text(bundle: &DocBundle) -> String {
    let moment = chrono::Local
        .timestamp_millis_opt(bundle.generated_at)
        .single()
        .unwrap_or_else(chrono::Local::now);
    if bundle.include_generated_at {
        moment.format("%Y-%m-%d %H:%M").to_string()
    } else {
        moment.format("%Y-%m-%d").to_string()
    }
}

fn esc(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn table(title: &str, params: &[DocParam]) -> String {
    if params.is_empty() {
        return String::new();
    }
    let mut out = format!(
        "<h4>{}</h4><table><thead><tr><th>名称</th><th>必填</th><th>示例</th><th>来源</th></tr></thead><tbody>",
        esc(title)
    );
    for item in params {
        let value = if item.value.is_empty() {
            "—".to_string()
        } else {
            esc(&item.value)
        };
        out.push_str(&format!(
            "<tr><td>{}</td><td>{}</td><td><code>{}</code></td><td>{}</td></tr>",
            esc(&item.name),
            if item.required { "是" } else { "否" },
            value,
            esc(&item.example_source)
        ));
    }
    out.push_str("</tbody></table>");
    out
}

fn entity_block(title: &str, entity: &DocEntity) -> String {
    if entity.fields.is_empty() {
        return String::new();
    }
    let mut out = format!(
        "<h4>{}</h4><table><thead><tr><th>字段</th><th>类型</th><th>必填</th><th>说明</th></tr></thead><tbody>",
        esc(title)
    );
    for field in &entity.fields {
        let mut description = field.description.clone();
        if field.stale {
            if !description.is_empty() {
                description.push(' ');
            }
            description.push_str("（最近一次发送未出现）");
        }
        out.push_str(&format!(
            "<tr><td><span style=\"padding-left:{}px\">{}</span></td><td>{}</td><td>{}</td><td>{}</td></tr>",
            field.depth.saturating_mul(12),
            esc(&field.name),
            esc(&field.type_name),
            if field.required { "是" } else { "否" },
            esc(&description)
        ));
    }
    out.push_str("</tbody></table>");
    out
}

fn body_block(body: &DocBody) -> String {
    if body.kind == "none" || body.text.trim().is_empty() {
        return String::new();
    }
    format!(
        "<h4>请求体（{}）</h4><pre><code>{}</code></pre>",
        esc(&body.kind),
        esc(&body.text)
    )
}

fn response_block(response: &DocResponse) -> String {
    let mut out = format!(
        "<h4>响应示例</h4><p class=\"meta\">状态码：<code>{}</code> {}</p>",
        response.status,
        esc(&response.status_text)
    );
    if !response.headers.is_empty() {
        out.push_str(
            "<h5>响应头</h5><table><thead><tr><th>名称</th><th>值</th></tr></thead><tbody>",
        );
        for header in &response.headers {
            out.push_str(&format!(
                "<tr><td>{}</td><td><code>{}</code></td></tr>",
                esc(&header.name),
                esc(&header.value)
            ));
        }
        out.push_str("</tbody></table>");
    }
    if !response.body.trim().is_empty() {
        out.push_str(&format!(
            "<h5>响应体</h5><pre><code>{}</code></pre>",
            esc(&response.body)
        ));
    }
    if response.truncated {
        out.push_str("<p class=\"desc\">响应体过长，已在抓取或导出时截断。</p>");
    }
    out
}

fn request(req: &DocRequest, open: bool) -> String {
    let mut out = format!(
        "<details class=\"req\"{}><summary><span class=\"method\">{}</span> <span class=\"name\">{}</span> <code>{}</code></summary>",
        if open { " open" } else { "" },
        esc(&req.method),
        esc(&req.name),
        esc(&req.path)
    );
    out.push_str("<div class=\"meta\">");
    out.push_str(&format!("<div>地址：<code>{}</code></div>", esc(&req.url)));
    if !req.description.trim().is_empty() && req.description != req.name {
        out.push_str(&format!("<div>说明：{}</div>", esc(&req.description)));
    }
    if !req.auth.kind.is_empty() && req.auth.kind != "none" {
        out.push_str(&format!(
            "<div>认证：{} · {}</div>",
            esc(&req.auth.kind),
            esc(&req.auth.note)
        ));
    }
    out.push_str("</div>");
    out.push_str(&table("请求参数", &req.query));
    out.push_str(&table("路径参数", &req.path_params));
    out.push_str(&table("请求头", &req.headers));
    out.push_str(&body_block(&req.body));
    if let Some(entity) = &req.request_entity {
        out.push_str(&entity_block("请求实体", entity));
    }
    if let Some(entity) = &req.response_entity {
        out.push_str(&entity_block("返回实体", entity));
    }
    if let Some(response) = &req.response {
        out.push_str(&response_block(response));
    }
    if let Some(scripts) = &req.scripts {
        if !scripts.pre.trim().is_empty() {
            out.push_str(&format!(
                "<h4>前置脚本</h4><pre><code>{}</code></pre>",
                esc(&scripts.pre)
            ));
        }
        if !scripts.post.trim().is_empty() {
            out.push_str(&format!(
                "<h4>后置脚本</h4><pre><code>{}</code></pre>",
                esc(&scripts.post)
            ));
        }
    }
    if let Some(assertions) = &req.assertions {
        if !assertions.trim().is_empty() {
            out.push_str(&format!(
                "<h4>断言</h4><pre><code>{}</code></pre>",
                esc(assertions)
            ));
        }
    }
    out.push_str("</details>");
    out
}

fn group(node: &DocGroup, depth: usize) -> String {
    let level = (2 + depth).min(6);
    let mut out = format!("<h{level}>{}</h{level}>", esc(&node.name));
    if !node.description.trim().is_empty() && node.description != node.name {
        out.push_str(&format!("<p class=\"desc\">{}</p>", esc(&node.description)));
    }
    for req in &node.requests {
        out.push_str(&request(req, false));
    }
    for child in &node.children {
        out.push_str(&group(child, depth + 1));
    }
    out
}

pub fn render(bundle: &DocBundle) -> String {
    let mut body = String::new();
    body.push_str(&format!("<h1>{}</h1>", esc(&bundle.scope.title)));
    if !bundle.subtitle.trim().is_empty() {
        body.push_str(&format!(
            "<p class=\"subtitle\">{}</p>",
            esc(&bundle.subtitle)
        ));
    }
    body.push_str(&format!(
        "<p class=\"meta\">项目：{} · 生成日期：{}</p>",
        esc(&bundle.scope.project_name),
        generated_text(bundle)
    ));
    if let Some(env) = &bundle.scope.environment_name {
        body.push_str(&format!(
            "<p class=\"meta\">环境：{}（本文档地址已按该环境解析）</p>",
            esc(env)
        ));
    }
    if !bundle.environments.is_empty() {
        body.push_str("<h2>环境变量</h2>");
        for env in &bundle.environments {
            body.push_str(&format!(
                "<h3>{}</h3><table><thead><tr><th>变量</th><th>值</th></tr></thead><tbody>",
                esc(&env.name)
            ));
            for item in &env.variables {
                body.push_str(&format!(
                    "<tr><td>{}</td><td>{}</td></tr>",
                    esc(&item.key),
                    esc(&item.value)
                ));
            }
            body.push_str("</tbody></table>");
        }
    }
    if bundle.scope.kind == "request" {
        // 单接口文档：不显示集合标题，接口卡片默认展开
        for node in &bundle.groups {
            for req in &node.requests {
                body.push_str(&request(req, true));
            }
        }
    } else {
        for node in &bundle.groups {
            body.push_str(&group(node, 0));
        }
    }
    if bundle.groups.is_empty() {
        body.push_str("<p class=\"empty\">该项目暂无接口</p>");
    }
    if !bundle.warnings.is_empty() {
        body.push_str("<h2>导出提示</h2><ul>");
        for warning in &bundle.warnings {
            body.push_str(&format!("<li>{}</li>", esc(warning)));
        }
        body.push_str("</ul>");
    }

    format!(
        r#"<!doctype html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'; img-src data:;">
<title>{title}</title>
<style>
  :root {{ color-scheme: light; }}
  body {{ margin: 0 auto; max-width: 980px; padding: 32px 24px 80px; font: 14px/1.7 -apple-system, "Segoe UI", "Microsoft YaHei", sans-serif; color: #1f2329; background: #fff; }}
  h1 {{ font-size: 28px; margin: 0 0 8px; }}
  h2 {{ font-size: 22px; border-bottom: 1px solid #e5e6eb; padding-bottom: 6px; margin-top: 36px; }}
  h3, h4 {{ margin: 22px 0 8px; }}
  h5 {{ margin: 16px 0 6px; font-size: 14px; }}
  .subtitle, .meta, .desc {{ color: #646a73; }}
  .req {{ border: 1px solid #e5e6eb; border-radius: 6px; margin: 14px 0; padding: 10px 14px; page-break-inside: avoid; }}
  .req summary {{ cursor: pointer; }}
  .method {{ display: inline-block; min-width: 52px; color: #009688; font-weight: 700; }}
  .name {{ font-weight: 600; }}
  table {{ border-collapse: collapse; width: 100%; margin: 10px 0; }}
  th, td {{ border: 1px solid #e5e6eb; padding: 6px 8px; text-align: left; vertical-align: top; }}
  th {{ background: #f7f8fa; }}
  code, pre {{ font-family: Consolas, "Courier New", monospace; }}
  pre {{ padding: 12px; background: #f7f8fa; border: 1px solid #e5e6eb; overflow: auto; white-space: pre-wrap; word-break: break-word; }}
  @page {{ size: A4; margin: 16mm; }}
  @media print {{ body {{ max-width: none; padding: 0; }} .req {{ break-inside: avoid; }} }}
  .page-break .req {{ page-break-before: always; break-before: page; }}
</style>
</head>
<body{page_class}>
{body}
</body>
</html>"#,
        title = esc(&bundle.scope.title),
        page_class = if bundle.page_break_per_request {
            " class=\"page-break\""
        } else {
            ""
        },
        body = body
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::docs::model::*;

    fn fixture(name: &str) -> DocBundle {
        DocBundle {
            generated_at: 0,
            app_version: "1.1.0".into(),
            revision: "r1".into(),
            subtitle: String::new(),
            include_generated_at: false,
            page_break_per_request: true,
            scope: DocScopeInfo {
                kind: "request".into(),
                title: "<script>alert(1)</script>".into(),
                project_name: "项目".into(),
                target_id: 1,
                environment_id: None,
                environment_name: None,
            },
            warnings: vec![],
            environments: vec![],
            groups: vec![DocGroup {
                id: 1,
                name: "集合".into(),
                description: String::new(),
                path: vec!["集合".into()],
                children: vec![],
                requests: vec![DocRequest {
                    id: 2,
                    name: name.into(),
                    method: "GET".into(),
                    url: "https://example.com/a?x=1&y=2".into(),
                    path: "/a".into(),
                    description: String::new(),
                    query: vec![],
                    path_params: vec![],
                    headers: vec![],
                    auth: DocAuth::default(),
                    body: DocBody::default(),
                    request_entity: None,
                    response_entity: None,
                    response: None,
                    scripts: None,
                    assertions: None,
                }],
            }],
        }
    }

    #[test]
    fn escapes_script_tags_and_has_no_javascript() {
        let html = render(&fixture("<script>alert(1)</script>"));
        assert!(!html.contains("<script"), "{html}");
        assert!(html.contains("&lt;script&gt;"), "{html}");
        assert!(!html.contains("javascript:"), "{html}");
        assert!(html.contains("<!doctype html>"));
        // 单接口文档：不显示集合标题，接口卡片默认展开
        assert!(!html.contains("<h2>集合</h2>"), "{html}");
        assert!(html.contains("<details class=\"req\" open>"), "{html}");
    }

    #[test]
    fn page_break_class_follows_option() {
        let mut data = fixture("分页");
        data.page_break_per_request = true;
        let html = render(&data);
        assert!(html.contains("<body class=\"page-break\">"), "{html}");
        assert!(html.contains("page-break-before: always"), "{html}");

        data.page_break_per_request = false;
        let html = render(&data);
        assert!(html.contains("<body>"), "{html}");
        assert!(!html.contains("<body class=\"page-break\">"), "{html}");
    }

    #[test]
    fn generated_time_option_controls_minutes() {
        fn date_text(html: &str) -> &str {
            let marker = "生成日期：";
            let start = html.find(marker).expect("生成日期") + marker.len();
            let rest = &html[start..];
            let end = rest.find("</p>").expect("日期段落结束");
            &rest[..end]
        }

        let mut data = fixture("时间");
        data.generated_at = 1_700_000_000_000;
        data.include_generated_at = false;
        let html = render(&data);
        assert!(!date_text(&html).contains(':'), "{}", date_text(&html));

        data.include_generated_at = true;
        let html = render(&data);
        assert!(date_text(&html).contains(':'), "{}", date_text(&html));
    }

    #[test]
    fn renders_entity_tables_escaped_with_depth_indent() {
        let mut data = fixture("实体");
        data.groups[0].requests[0].response_entity = Some(DocEntity {
            fields: vec![DocEntityField {
                name: "data.user.<script>".into(),
                type_name: "string".into(),
                required: true,
                description: "<b>说明</b>".into(),
                stale: false,
                depth: 2,
            }],
        });
        let html = render(&data);
        assert!(html.contains("<h4>返回实体</h4>"), "{html}");
        assert!(!html.contains("<script>"), "{html}");
        assert!(html.contains("&lt;script&gt;"), "{html}");
        assert!(html.contains("&lt;b&gt;说明&lt;/b&gt;"), "{html}");
        assert!(html.contains("padding-left:24px"), "{html}");
    }
}
