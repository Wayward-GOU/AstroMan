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

/// 标题文本：换行折叠为空格（防止多行标题），`|` 转义（同名集合也可能出现在表格里）
fn title(text: &str) -> String {
    text.replace(['\n', '\r'], " ").replace('|', "\\|")
}

/// 表格单元格：转义反斜杠与竖线，换行变 `<br>`
fn cell(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('|', "\\|")
        .replace('\n', "<br>")
        .replace('\r', "")
}

fn param_table(title_text: &str, params: &[DocParam]) -> String {
    if params.is_empty() {
        return String::new();
    }
    let mut out =
        format!("#### {title_text}\n\n| 名称 | 必填 | 示例 | 示例来源 |\n|---|---|---|---|\n");
    for item in params {
        let value = if item.value.is_empty() {
            "—".to_string()
        } else {
            cell(&item.value)
        };
        out.push_str(&format!(
            "| {} | {} | {} | {} |\n",
            cell(&item.name),
            if item.required { "是" } else { "否" },
            value,
            cell(&item.example_source)
        ));
    }
    out.push('\n');
    out
}

fn entity_section(title_text: &str, entity: &DocEntity) -> String {
    if entity.fields.is_empty() {
        return String::new();
    }
    let mut out =
        format!("#### {title_text}\n\n| 字段 | 类型 | 必填 | 说明 |\n|---|---|---|---|\n");
    for field in &entity.fields {
        let mut description = field.description.clone();
        if field.stale {
            if !description.is_empty() {
                description.push(' ');
            }
            description.push_str("（最近一次发送未出现）");
        }
        out.push_str(&format!(
            "| {} | {} | {} | {} |\n",
            cell(&field.name),
            cell(&field.type_name),
            if field.required { "是" } else { "否" },
            cell(&description)
        ));
    }
    out.push('\n');
    out
}

fn body_section(body: &DocBody) -> String {
    if body.kind == "none" || body.text.trim().is_empty() {
        return String::new();
    }
    let lang = match body.kind.as_str() {
        "json" => "json",
        "graphql" => "graphql",
        _ => "text",
    };
    format!(
        "#### 请求体（{}）\n\n````{lang}\n{}\n````\n\n",
        body.kind, body.text
    )
}

fn response_section(response: &DocResponse) -> String {
    let mut out = format!(
        "#### 响应示例\n\n- 状态码：{} {}\n\n",
        response.status,
        title(&response.status_text)
    );
    if !response.headers.is_empty() {
        out.push_str("**响应头**\n\n| 名称 | 值 |\n|---|---|\n");
        for header in &response.headers {
            out.push_str(&format!(
                "| {} | {} |\n",
                cell(&header.name),
                cell(&header.value)
            ));
        }
        out.push('\n');
    }
    if !response.body.trim().is_empty() {
        let trimmed = response.body.trim_start();
        let lang = if trimmed.starts_with('{') || trimmed.starts_with('[') {
            "json"
        } else {
            "text"
        };
        out.push_str(&format!(
            "**响应体**\n\n````{lang}\n{}\n````\n\n",
            response.body
        ));
    }
    if response.truncated {
        out.push_str("> 响应体过长，已在抓取或导出时截断。\n\n");
    }
    out
}

fn message(bundle: &DocBundle) -> String {
    let mut out = format!("# {}\n\n", title(&bundle.scope.title));
    if !bundle.subtitle.trim().is_empty() {
        out.push_str(&format!("> {}\n", title(&bundle.subtitle)));
    }
    out.push_str(&format!(
        "> 项目：{} · 生成日期：{}\n\n",
        title(&bundle.scope.project_name),
        generated_text(bundle)
    ));
    if let Some(env) = &bundle.scope.environment_name {
        out.push_str(&format!("> 环境：{env}（本文档地址已按该环境解析）\n\n"));
    }
    out
}

fn environments(bundle: &DocBundle) -> String {
    if bundle.environments.is_empty() {
        return String::new();
    }
    let mut out = String::from("## 环境变量\n\n");
    for env in &bundle.environments {
        out.push_str(&format!(
            "### {}\n\n| 变量 | 值 |\n|---|---|\n",
            title(&env.name)
        ));
        for item in &env.variables {
            out.push_str(&format!(
                "| {} | {} |\n",
                cell(&item.key),
                cell(&item.value)
            ));
        }
        out.push('\n');
    }
    out
}

fn request_section(request: &DocRequest, level: usize, show_heading: bool) -> String {
    let mut out = String::new();
    if show_heading {
        out.push_str(&format!(
            "{} {}\n\n",
            "#".repeat(level),
            title(&request.name)
        ));
    }
    out.push_str(&format!(
        "- 方法：{}\n- 地址：{}\n- 路径：{}\n",
        request.method, request.url, request.path
    ));
    if !request.description.trim().is_empty() && request.description != request.name {
        out.push_str(&format!("- 说明：{}\n", title(&request.description)));
    }
    if !request.auth.kind.is_empty() && request.auth.kind != "none" {
        out.push_str(&format!(
            "- 认证：{} · {}\n",
            request.auth.kind,
            title(&request.auth.note)
        ));
    }
    out.push('\n');
    out.push_str(&param_table("请求参数", &request.query));
    out.push_str(&param_table("路径参数", &request.path_params));
    out.push_str(&param_table("请求头", &request.headers));
    out.push_str(&body_section(&request.body));
    if let Some(entity) = &request.request_entity {
        out.push_str(&entity_section("请求实体", entity));
    }
    if let Some(entity) = &request.response_entity {
        out.push_str(&entity_section("返回实体", entity));
    }
    if let Some(response) = &request.response {
        out.push_str(&response_section(response));
    }
    if let Some(scripts) = &request.scripts {
        if !scripts.pre.trim().is_empty() || !scripts.post.trim().is_empty() {
            out.push_str("#### 脚本\n\n");
            if !scripts.pre.trim().is_empty() {
                out.push_str(&format!(
                    "**前置脚本**\n\n````js\n{}\n````\n\n",
                    scripts.pre
                ));
            }
            if !scripts.post.trim().is_empty() {
                out.push_str(&format!(
                    "**后置脚本**\n\n````js\n{}\n````\n\n",
                    scripts.post
                ));
            }
        }
    }
    if let Some(assertions) = &request.assertions {
        if !assertions.trim().is_empty() {
            out.push_str(&format!("#### 断言\n\n````text\n{assertions}\n````\n\n"));
        }
    }
    out
}

fn group_section(group: &DocGroup, depth: usize) -> String {
    let level = (2 + depth).min(6);
    let mut out = format!("{} {}\n\n", "#".repeat(level), title(&group.name));
    if !group.description.trim().is_empty() && group.description != group.name {
        out.push_str(&format!("{}\n\n", title(&group.description)));
    }
    for request in &group.requests {
        out.push_str(&request_section(request, (4 + depth).min(6), true));
    }
    for child in &group.children {
        out.push_str(&group_section(child, depth + 1));
    }
    out
}

pub fn render(bundle: &DocBundle) -> String {
    let mut out = message(bundle);
    out.push_str(&environments(bundle));
    if bundle.scope.kind == "request" {
        // 单接口文档不再套集合标题，接口直接作为正文二级标题
        for group in &bundle.groups {
            for request in &group.requests {
                let show_heading = request.name != bundle.scope.title;
                out.push_str(&request_section(request, 2, show_heading));
            }
        }
    } else {
        for group in &bundle.groups {
            out.push_str(&group_section(group, 0));
        }
    }
    if bundle.groups.is_empty() {
        out.push_str("_该项目暂无接口_\n");
    }
    if !bundle.warnings.is_empty() {
        out.push_str("\n## 导出提示\n\n");
        for warning in &bundle.warnings {
            out.push_str(&format!("- {warning}\n"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::docs::model::*;

    fn fixture() -> DocBundle {
        DocBundle {
            generated_at: 0,
            app_version: "1.1.0".into(),
            revision: "r1".into(),
            subtitle: "测试副标题".into(),
            include_generated_at: false,
            page_break_per_request: true,
            scope: DocScopeInfo {
                kind: "project".into(),
                title: "开放平台".into(),
                project_name: "开放平台".into(),
                target_id: 1,
                environment_id: None,
                environment_name: Some("测试环境".into()),
            },
            warnings: vec![],
            environments: vec![],
            groups: vec![DocGroup {
                id: 10,
                name: "用户 | 中心".into(),
                description: "".into(),
                path: vec!["用户 | 中心".into()],
                children: vec![],
                requests: vec![DocRequest {
                    id: 20,
                    name: "登录\n# 不能变标题".into(),
                    method: "POST".into(),
                    url: "https://api.example.com/login".into(),
                    path: "/login".into(),
                    description: "".into(),
                    query: vec![DocParam {
                        name: "user".into(),
                        value: "a|b".into(),
                        required: true,
                        location: "query".into(),
                        example_source: "defined".into(),
                    }],
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
    fn escapes_table_pipes_and_heading_breaks() {
        let text = render(&fixture());
        assert!(text.contains("用户 \\| 中心"), "{text}");
        assert!(text.contains("a\\|b"), "{text}");
        assert!(text.contains("登录 # 不能变标题"), "{text}");
        assert!(!text.contains("登录\n#"), "{text}");
    }

    #[test]
    fn single_request_skips_collection_heading_and_renders_response() {
        let mut data = fixture();
        data.scope.kind = "request".into();
        data.scope.title = "登录\n# 不能变标题".into();
        data.groups[0].requests[0].response = Some(DocResponse {
            status: 200,
            status_text: "OK".into(),
            headers: vec![DocParam {
                name: "Content-Type".into(),
                value: "application/json".into(),
                required: false,
                location: "response".into(),
                example_source: "run".into(),
            }],
            body: "{\"ok\":true}".into(),
            truncated: false,
        });
        let text = render(&data);
        assert!(!text.contains("## 用户"), "{text}");
        assert!(text.contains("#### 响应示例"), "{text}");
        assert!(text.contains("````json"), "{text}");
        assert!(text.contains("Content-Type"), "{text}");
    }

    #[test]
    fn renders_entity_tables_with_stale_note_and_blank_description() {
        let mut data = fixture();
        data.groups[0].requests[0].request_entity = Some(DocEntity {
            fields: vec![
                DocEntityField {
                    name: "username".into(),
                    type_name: "string".into(),
                    required: true,
                    description: String::new(),
                    stale: false,
                    depth: 0,
                },
                DocEntityField {
                    name: "profile.city".into(),
                    type_name: "string".into(),
                    required: false,
                    description: "城市".into(),
                    stale: true,
                    depth: 1,
                },
                DocEntityField {
                    name: "a|b".into(),
                    type_name: "string".into(),
                    required: false,
                    description: "line1\nline2".into(),
                    stale: false,
                    depth: 0,
                },
            ],
        });
        let text = render(&data);
        assert!(text.contains("#### 请求实体"), "{text}");
        assert!(text.contains("| username | string | 是 |  |"), "{text}");
        assert!(text.contains("profile.city"), "{text}");
        assert!(text.contains("城市 （最近一次发送未出现）"), "{text}");
        assert!(
            text.contains("| a\\|b | string | 否 | line1<br>line2 |"),
            "{text}"
        );
    }

    #[test]
    fn omits_entity_sections_when_absent_or_empty() {
        let data = fixture();
        let text = render(&data);
        assert!(!text.contains("请求实体"), "{text}");
        assert!(!text.contains("返回实体"), "{text}");

        let mut data = fixture();
        data.groups[0].requests[0].response_entity = Some(DocEntity { fields: vec![] });
        let text = render(&data);
        assert!(!text.contains("返回实体"), "{text}");
    }
}
