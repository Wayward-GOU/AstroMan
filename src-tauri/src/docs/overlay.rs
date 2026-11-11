use std::collections::{BTreeSet, HashMap};

use super::builder::pointer_of;
use super::masking;
use super::model::*;

/// 把前端回传的稀疏覆盖层合并进 IR。
///
/// 覆盖层只包含用户改过的字段；这里不读数据库，也不做脱敏以外的加工。
pub fn apply(bundle: &mut DocBundle, overlay: &DocEditOverlay) -> Vec<String> {
    let mut warnings = Vec::new();
    if !overlay.revision.is_empty() && overlay.revision != bundle.revision {
        warnings.push("源数据已变化，部分编辑可能未生效".to_string());
    }
    if let Some(title) = overlay
        .meta
        .title
        .as_ref()
        .filter(|text| !text.trim().is_empty())
    {
        bundle.scope.title = title.clone();
    }
    if let Some(subtitle) = &overlay.meta.subtitle {
        bundle.subtitle = subtitle.clone();
    }
    for group in &mut bundle.groups {
        apply_group(group, overlay);
    }
    let excluded = &overlay.excluded;
    bundle
        .groups
        .retain(|group| !excluded.contains(&format!("g-{}", group.id)));
    for group in &mut bundle.groups {
        remove_excluded(group, excluded);
    }
    for group in &mut bundle.groups {
        reorder(group, overlay);
    }
    warnings
}

fn apply_group(group: &mut DocGroup, overlay: &DocEditOverlay) {
    if let Some(node) = overlay.nodes.get(&format!("g-{}", group.id)) {
        if let Some(description) = &node.description {
            group.description = description.clone();
        }
    }
    for request in &mut group.requests {
        apply_request(request, overlay);
    }
    for child in &mut group.children {
        apply_group(child, overlay);
    }
}

fn apply_request(request: &mut DocRequest, overlay: &DocEditOverlay) {
    let Some(node) = overlay.nodes.get(&format!("r-{}", request.id)) else {
        return;
    };
    if let Some(description) = &node.description {
        request.description = description.clone();
    }
    let mut seen = HashMap::new();
    for param in request
        .query
        .iter_mut()
        .chain(request.path_params.iter_mut())
        .chain(request.headers.iter_mut())
    {
        let occurrence = seen
            .entry((param.location.clone(), param.name.clone()))
            .or_insert(0usize);
        *occurrence += 1;
        let key = format!(
            "p-{}-{}-{}-{}",
            request.id, param.location, param.name, occurrence
        );
        if masking::is_sensitive_name(&param.name) {
            continue;
        }
        if let Some(value) = node.example_values.get(&key) {
            param.value = value.clone();
            param.example_source = "manual".into();
        }
    }
    let mut body_seen = HashMap::new();
    for field in &mut request.body.fields {
        let occurrence = body_seen.entry(field.name.clone()).or_insert(0usize);
        *occurrence += 1;
        let key = match request.body.kind.as_str() {
            "json" => format!("b-{}-{}", request.id, pointer_of(&field.name)),
            _ => format!("f-{}-{}-{}", request.id, field.name, occurrence),
        };
        if masking::is_sensitive_name(&field.name) {
            continue;
        }
        if let Some(value) = node.example_values.get(&key) {
            field.value = value.clone();
            field.example_source = "manual".into();
        }
    }
    if matches!(request.body.kind.as_str(), "json" | "form" | "multipart")
        && !request.body.fields.is_empty()
    {
        rebuild_body_text(request);
    }
}

fn rebuild_body_text(request: &mut DocRequest) {
    match request.body.kind.as_str() {
        "json" => {
            if let Ok(serde_json::Value::Object(mut map)) = serde_json::from_str(&request.body.text)
            {
                for field in &request.body.fields {
                    let value = serde_json::from_str::<serde_json::Value>(&field.value)
                        .unwrap_or_else(|_| serde_json::Value::String(field.value.clone()));
                    map.insert(field.name.clone(), value);
                }
                if let Ok(text) = serde_json::to_string_pretty(&serde_json::Value::Object(map)) {
                    request.body.text = text;
                }
            }
        }
        "form" => {
            request.body.text = request
                .body
                .fields
                .iter()
                .map(|field| format!("{}={}", encode_form(&field.name), encode_form(&field.value)))
                .collect::<Vec<_>>()
                .join("&");
        }
        "multipart" => rebuild_multipart(request),
        _ => {}
    }
}

/// 保留 multipart 原始条目的 filename / kind 等元数据，只替换文本值
fn rebuild_multipart(request: &mut DocRequest) {
    let mut items: Vec<serde_json::Value> =
        serde_json::from_str::<Vec<serde_json::Value>>(&request.body.text).unwrap_or_default();
    let mut fields = request.body.fields.iter();
    for item in &mut items {
        let enabled = item
            .get("enabled")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(true);
        if !enabled {
            continue;
        }
        let Some(field) = fields.next() else {
            break;
        };
        if let Some(object) = item.as_object_mut() {
            object.insert(
                "value".to_string(),
                serde_json::Value::String(field.value.clone()),
            );
        }
    }
    if let Ok(text) = serde_json::to_string_pretty(&serde_json::Value::Array(items)) {
        request.body.text = text;
    }
}

fn encode_form(value: &str) -> String {
    value
        .replace('%', "%25")
        .replace('&', "%26")
        .replace('=', "%3D")
        .replace('+', "%2B")
        .replace(' ', "%20")
}

fn remove_excluded(group: &mut DocGroup, excluded: &BTreeSet<String>) {
    group
        .requests
        .retain(|request| !excluded.contains(&format!("r-{}", request.id)));
    group
        .children
        .retain(|child| !excluded.contains(&format!("g-{}", child.id)));
    for child in &mut group.children {
        remove_excluded(child, excluded);
    }
}

fn reorder(group: &mut DocGroup, overlay: &DocEditOverlay) {
    if let Some(order) = overlay.order.get(&format!("g-{}", group.id)) {
        let rank: HashMap<&str, usize> = order
            .iter()
            .enumerate()
            .map(|(index, key)| (key.as_str(), index))
            .collect();
        group.requests.sort_by_key(|request| {
            rank.get(format!("r-{}", request.id).as_str())
                .copied()
                .unwrap_or(usize::MAX)
        });
        group.children.sort_by_key(|child| {
            rank.get(format!("g-{}", child.id).as_str())
                .copied()
                .unwrap_or(usize::MAX)
        });
    }
    for child in &mut group.children {
        reorder(child, overlay);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bundle() -> DocBundle {
        DocBundle {
            generated_at: 0,
            app_version: "1".into(),
            revision: "rev1".into(),
            subtitle: String::new(),
            include_generated_at: false,
            page_break_per_request: true,
            scope: DocScopeInfo {
                kind: "project".into(),
                title: "旧标题".into(),
                project_name: "P".into(),
                target_id: 1,
                environment_id: None,
                environment_name: None,
            },
            warnings: vec![],
            environments: vec![],
            groups: vec![DocGroup {
                id: 1,
                name: "集合".into(),
                description: "集合".into(),
                path: vec!["集合".into()],
                children: vec![],
                requests: vec![DocRequest {
                    id: 2,
                    name: "接口".into(),
                    method: "GET".into(),
                    url: "https://example.com".into(),
                    path: "/".into(),
                    description: "接口".into(),
                    query: vec![DocParam {
                        name: "page".into(),
                        value: "1".into(),
                        required: false,
                        location: "query".into(),
                        example_source: "auto".into(),
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
    fn applies_meta_description_values_and_exclusion() {
        let mut data = bundle();
        let mut overlay = DocEditOverlay {
            revision: "rev1".into(),
            ..Default::default()
        };
        overlay.meta.title = Some("新标题".into());
        overlay.nodes.insert(
            "r-2".into(),
            DocNodeEdit {
                description: Some("新说明".into()),
                example_values: [("p-2-query-page-1".into(), "9".into())]
                    .into_iter()
                    .collect(),
            },
        );
        let warnings = apply(&mut data, &overlay);
        assert!(warnings.is_empty());
        assert_eq!(data.scope.title, "新标题");
        assert_eq!(data.groups[0].requests[0].description, "新说明");
        assert_eq!(data.groups[0].requests[0].query[0].value, "9");

        overlay.excluded.insert("r-2".into());
        apply(&mut data, &overlay);
        assert!(data.groups[0].requests.is_empty());
        assert_eq!(data.groups[0].children.len(), 0);
    }

    #[test]
    fn stale_revision_warns_but_still_applies() {
        let mut data = bundle();
        let overlay = DocEditOverlay {
            revision: "old".into(),
            ..Default::default()
        };
        let warnings = apply(&mut data, &overlay);
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn sensitive_fields_cannot_be_overridden() {
        let mut data = bundle();
        data.groups[0].requests[0].query.push(DocParam {
            name: "api_key".into(),
            value: "••••••".into(),
            required: false,
            location: "query".into(),
            example_source: "defined".into(),
        });
        let mut overlay = DocEditOverlay {
            revision: "rev1".into(),
            ..Default::default()
        };
        overlay.nodes.insert(
            "r-2".into(),
            DocNodeEdit {
                description: None,
                example_values: [("p-2-query-api_key-1".into(), "real-secret".into())]
                    .into_iter()
                    .collect(),
            },
        );
        apply(&mut data, &overlay);
        let values: Vec<&str> = data.groups[0].requests[0]
            .query
            .iter()
            .map(|item| item.value.as_str())
            .collect();
        assert!(values.contains(&"••••••"));
        assert!(!values.contains(&"real-secret"));
    }
}
