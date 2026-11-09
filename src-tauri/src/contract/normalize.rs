//! OpenAPI 快照归一化
//!
//! 契约 diff 的敌人是噪声：同一项目两次生成必须逐字节一致。
//! 规则见技术方案 4.2：
//! 1. 对象键按字典序；
//! 2. `servers` 固定占位，不写真实环境地址；
//! 3. 剔除 `example / examples`（取值不进入快照）；
//! 4. `{{ var }}` 统一为 `{{var}}`；
//! 5. `null` 与空 `description / summary` 视为缺失；
//! 6. `x-` 扩展保留（键排序后天然稳定）。

use serde_json::{json, Value};

pub fn normalize_spec(spec: &Value) -> Value {
    let mut spec = spec.clone();
    if let Some(object) = spec.as_object_mut() {
        object.insert(
            "servers".to_string(),
            json!([{ "url": "http://localhost" }]),
        );
    }
    normalize_value(&mut spec);
    spec
}

fn normalize_value(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.remove("example");
            map.remove("examples");
            for child in map.values_mut() {
                normalize_value(child);
            }
            map.retain(|key, child| {
                if child.is_null() {
                    return false;
                }
                if matches!(key.as_str(), "description" | "summary")
                    && child.as_str().is_some_and(|text| text.trim().is_empty())
                {
                    return false;
                }
                true
            });
            let mut entries: Vec<(String, Value)> = std::mem::take(map).into_iter().collect();
            entries.sort_by(|left, right| left.0.cmp(&right.0));
            map.extend(entries);
        }
        Value::Array(items) => {
            for item in items {
                normalize_value(item);
            }
        }
        Value::String(text) => {
            *text = normalize_variables(text);
        }
        _ => {}
    }
}

/// 把 `{{  var  }}` 收敛成 `{{var}}`，避免空格差异制造 diff
fn normalize_variables(text: &str) -> String {
    if !text.contains("{{") {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("{{") {
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else {
            break;
        };
        out.push_str(&rest[..start]);
        out.push_str("{{");
        out.push_str(after[..end].trim());
        out.push_str("}}");
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_sorted_and_examples_removed() {
        let spec = json!({
            "openapi": "3.0.3",
            "servers": [{ "url": "https://real.example.com" }],
            "paths": {
                "/b": { "get": { "summary": "B", "responses": {
                    "200": { "description": "OK", "content": {
                        "application/json": { "example": { "token": "secret" } }
                    }}
                }}},
                "/a": { "get": { "summary": "A" } }
            }
        });
        let normalized = normalize_spec(&spec);
        assert_eq!(normalized["servers"][0]["url"], "http://localhost");
        let text = serde_json::to_string(&normalized).unwrap();
        assert!(!text.contains("secret"), "示例值不能进入快照");
        assert!(text.find("\"/a\"").unwrap() < text.find("\"/b\"").unwrap());
    }

    #[test]
    fn nulls_and_empty_descriptions_are_removed() {
        let spec = json!({
            "info": { "title": "T", "description": "  ", "version": "1" },
            "paths": { "/a": { "get": { "description": "", "nullable": null } } }
        });
        let normalized = normalize_spec(&spec);
        assert!(normalized["info"].get("description").is_none());
        assert!(normalized["paths"]["/a"]["get"]
            .get("description")
            .is_none());
        assert!(normalized["paths"]["/a"]["get"].get("nullable").is_none());
    }

    #[test]
    fn variables_are_trimmed_and_extensions_kept() {
        let spec = json!({
            "paths": { "/a": { "get": { "description": "{{ baseUrl }}/x/{{id }}" } } },
            "x-astroman": { "b": 1, "a": 2 }
        });
        let normalized = normalize_spec(&spec);
        assert_eq!(
            normalized["paths"]["/a"]["get"]["description"],
            "{{baseUrl}}/x/{{id}}"
        );
        let extension = normalized["x-astroman"].as_object().unwrap();
        let keys: Vec<&String> = extension.keys().collect();
        assert_eq!(keys, vec!["a", "b"]);
    }

    #[test]
    fn repeated_normalization_is_stable() {
        let spec = json!({
            "openapi": "3.0.3",
            "paths": { "/a": { "get": { "responses": { "200": { "description": "OK" } } } } }
        });
        let once = normalize_spec(&spec);
        let twice = normalize_spec(&once);
        assert_eq!(once, twice);
        assert_eq!(
            serde_json::to_string(&once).unwrap(),
            serde_json::to_string(&twice).unwrap()
        );
    }

    #[test]
    fn empty_objects_survive_because_unknown_schema_is_meaningful() {
        let spec = json!({
            "paths": { "/a": { "get": { "responses": {
                "200": { "content": { "application/json": { "schema": {
                    "type": "object", "properties": { "x": {} }
                }}}}
            }}}}
        });
        let normalized = normalize_spec(&spec);
        let schema = &normalized["paths"]["/a"]["get"]["responses"]["200"]["content"]
            ["application/json"]["schema"];
        assert!(schema["properties"]["x"].as_object().unwrap().is_empty());
    }
}
