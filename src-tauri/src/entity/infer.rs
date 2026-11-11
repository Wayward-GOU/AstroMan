//! 从真实请求 / 响应正文推断字段树
//!
//! 规则（技术方案 5.8.2 / 13.3）：
//! - 对象按首次出现顺序递归；
//! - 数组把所有元素的字段取并集，字段只有在**所有元素**中都出现才标为必填；
//! - 同一路径类型冲突统一为 `unknown` 并记 warning；
//! - 表单字段一律 `string`，multipart 文件字段记为 `file`；
//! - 深度 12 / 节点 500 封顶，超出保留已推断部分并记 warning，不失败。

use std::collections::HashMap;

use serde_json::Value;

use crate::models::FormField;

use super::model::{node_path, EntityNode};

pub const MAX_DEPTH: usize = 12;
pub const MAX_NODES: usize = 500;
pub const MAX_INPUT_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone)]
pub struct InferResult {
    pub root: EntityNode,
    pub warnings: Vec<String>,
}

struct Ctx {
    nodes: usize,
    warnings: Vec<String>,
    limit_reported: bool,
}

impl Ctx {
    fn new() -> Self {
        Self {
            nodes: 0,
            warnings: Vec::new(),
            limit_reported: false,
        }
    }

    /// 占用一个节点配额；超过上限时只提示一次
    fn spend(&mut self) -> bool {
        if self.nodes >= MAX_NODES {
            if !self.limit_reported {
                self.limit_reported = true;
                self.warnings.push(format!(
                    "字段数超过 {MAX_NODES} 个，已保留前 {MAX_NODES} 个字段"
                ));
            }
            return false;
        }
        self.nodes += 1;
        true
    }
}

fn json_type(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// 推断 JSON 正文；非 JSON 返回 `E_ENTITY_PARSE`
pub fn infer_json(text: &str) -> Result<InferResult, String> {
    if text.trim().is_empty() {
        return Err("E_ENTITY_PARSE: 正文为空，无法推断实体".to_string());
    }
    let mut ctx = Ctx::new();
    if text.len() > MAX_INPUT_BYTES {
        ctx.warnings.push(format!(
            "正文超过 {} KB，实体只保留前 {MAX_NODES} 个字段",
            MAX_INPUT_BYTES / 1024
        ));
    }
    let value: Value = serde_json::from_str(text)
        .map_err(|e| format!("E_ENTITY_PARSE: 正文不是合法 JSON：{e}"))?;
    let root = infer_root(&value, &mut ctx);
    Ok(InferResult {
        root,
        warnings: ctx.warnings,
    })
}

/// 按 Body 类型推断请求实体；不支持的类型返回 `E_ENTITY_PARSE`
pub fn infer_body(body_type: &str, text: &str) -> Result<InferResult, String> {
    match body_type {
        "form" => infer_form(text),
        "multipart" => infer_multipart(text),
        "none" | "binary" => Err("E_ENTITY_PARSE: 该 Body 类型不参与实体推断".to_string()),
        _ => infer_json(text),
    }
}

fn infer_root(value: &Value, ctx: &mut Ctx) -> EntityNode {
    match value {
        Value::Object(_) => {
            let values = [value];
            EntityNode {
                children: infer_object_fields(&values, 1, 0, ctx, ""),
                ..EntityNode::new_root("object")
            }
        }
        Value::Array(items) => {
            let refs: Vec<&Value> = items.iter().collect();
            let mut root = EntityNode::new_root("array");
            fill_array_node(&mut root, &refs, 0, ctx, "");
            root
        }
        _ => EntityNode::new_root(json_type(value)),
    }
}

/// 把多个对象观测合并成字段列表：键按首次出现顺序，必填按出现次数判定
fn infer_object_fields(
    objects: &[&Value],
    total: usize,
    depth: usize,
    ctx: &mut Ctx,
    parent_path: &str,
) -> Vec<EntityNode> {
    let mut order: Vec<String> = Vec::new();
    let mut groups: HashMap<String, Vec<&Value>> = HashMap::new();
    for object in objects {
        let Some(map) = object.as_object() else {
            continue;
        };
        for (key, value) in map {
            if !groups.contains_key(key) {
                order.push(key.clone());
            }
            groups.entry(key.clone()).or_default().push(value);
        }
    }

    let mut children = Vec::new();
    for key in order {
        let Some(values) = groups.get(&key) else {
            continue;
        };
        match infer_values(&key, values, total, depth, ctx, parent_path) {
            Some(node) => children.push(node),
            None => break,
        }
    }
    children
}

fn infer_values(
    name: &str,
    values: &[&Value],
    total: usize,
    depth: usize,
    ctx: &mut Ctx,
    parent_path: &str,
) -> Option<EntityNode> {
    if !ctx.spend() {
        return None;
    }

    let mut types: Vec<&'static str> = Vec::new();
    for value in values {
        let kind = json_type(value);
        if !types.contains(&kind) {
            types.push(kind);
        }
    }

    let mut node = EntityNode {
        name: name.to_string(),
        required: values.len() == total,
        ..EntityNode::default()
    };

    if types.len() != 1 {
        node.type_name = "unknown".to_string();
        ctx.warnings.push(format!(
            "字段 {} 类型不一致（{}），已记为 unknown",
            node_path(parent_path, name, "unknown"),
            types.join(" / ")
        ));
        return Some(node);
    }

    let kind = types[0];
    if matches!(kind, "object" | "array") && depth >= MAX_DEPTH {
        ctx.warnings.push(format!(
            "字段 {} 嵌套超过 {MAX_DEPTH} 层，已截断",
            node_path(parent_path, name, kind)
        ));
        node.type_name = "unknown".to_string();
        return Some(node);
    }

    node.type_name = kind.to_string();
    let path = node_path(parent_path, name, kind);
    match kind {
        "object" => {
            node.children = infer_object_fields(values, values.len(), depth + 1, ctx, &path);
        }
        "array" => {
            let mut elements: Vec<&Value> = Vec::new();
            for value in values {
                if let Some(items) = value.as_array() {
                    elements.extend(items.iter());
                }
            }
            fill_array_node(&mut node, &elements, depth + 1, ctx, &path);
        }
        _ => {}
    }
    Some(node)
}

/// 数组节点：`itemType` 取元素类型合并结果，对象元素字段取并集
fn fill_array_node(
    node: &mut EntityNode,
    elements: &[&Value],
    depth: usize,
    ctx: &mut Ctx,
    path: &str,
) {
    node.type_name = "array".to_string();
    let mut types: Vec<&'static str> = Vec::new();
    for value in elements {
        let kind = json_type(value);
        if !types.contains(&kind) {
            types.push(kind);
        }
    }
    node.item_type = match types.as_slice() {
        [] => String::new(),
        [only] => only.to_string(),
        _ => {
            ctx.warnings.push(format!(
                "字段 {path} 的数组元素类型不一致（{}），元素类型已记为 unknown",
                types.join(" / ")
            ));
            String::new()
        }
    };

    if elements.is_empty() || depth >= MAX_DEPTH {
        if depth >= MAX_DEPTH && !elements.is_empty() {
            ctx.warnings
                .push(format!("字段 {path} 嵌套超过 {MAX_DEPTH} 层，已截断"));
        }
        return;
    }

    let objects: Vec<&Value> = elements
        .iter()
        .copied()
        .filter(|value| value.is_object())
        .collect();
    if objects.is_empty() {
        return;
    }
    node.children = infer_object_fields(&objects, elements.len(), depth, ctx, path);
}

fn infer_form(text: &str) -> Result<InferResult, String> {
    let mut root = EntityNode::new_root("object");
    let mut seen = HashMap::new();
    for chunk in text.split('&').filter(|item| !item.trim().is_empty()) {
        let (raw_key, _) = chunk.split_once('=').unwrap_or((chunk, ""));
        let key = percent_decode(raw_key);
        if key.is_empty() || seen.contains_key(&key) {
            continue;
        }
        seen.insert(key.clone(), true);
        root.children.push(EntityNode {
            name: key,
            type_name: "string".to_string(),
            required: true,
            ..EntityNode::default()
        });
    }
    if root.children.is_empty() {
        return Err("E_ENTITY_PARSE: 表单没有可推断的字段".to_string());
    }
    Ok(InferResult {
        root,
        warnings: Vec::new(),
    })
}

fn infer_multipart(text: &str) -> Result<InferResult, String> {
    let fields: Vec<FormField> = serde_json::from_str(text)
        .map_err(|e| format!("E_ENTITY_PARSE: 多部分表单不是合法 JSON：{e}"))?;
    let mut root = EntityNode::new_root("object");
    let mut seen: HashMap<String, usize> = HashMap::new();
    for field in fields
        .iter()
        .filter(|item| item.enabled && !item.key.trim().is_empty())
    {
        let kind = if field.kind == "file" {
            "file"
        } else {
            "string"
        };
        if let Some(index) = seen.get(&field.key).copied() {
            let existing = &mut root.children[index];
            if existing.type_name != kind {
                existing.type_name = "unknown".to_string();
                existing.required = true;
            }
            continue;
        }
        seen.insert(field.key.clone(), root.children.len());
        root.children.push(EntityNode {
            name: field.key.clone(),
            type_name: kind.to_string(),
            required: true,
            ..EntityNode::default()
        });
    }
    if root.children.is_empty() {
        return Err("E_ENTITY_PARSE: 多部分表单没有可推断的字段".to_string());
    }
    Ok(InferResult {
        root,
        warnings: Vec::new(),
    })
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => {
                out.push(b' ');
                index += 1;
            }
            b'%' if index + 2 < bytes.len() => {
                let hex = &input[index + 1..index + 3];
                if let Ok(value) = u8::from_str_radix(hex, 16) {
                    out.push(value);
                    index += 3;
                } else {
                    out.push(bytes[index]);
                    index += 1;
                }
            }
            byte => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn child<'a>(node: &'a EntityNode, name: &str) -> &'a EntityNode {
        node.children
            .iter()
            .find(|item| item.name == name)
            .unwrap_or_else(|| panic!("缺少字段 {name}"))
    }

    #[test]
    fn objects_keep_first_appearance_order_and_are_required() {
        let result = infer_json(r#"{"b":1,"a":{"c":true,"d":null}}"#).unwrap();
        let names: Vec<&str> = result
            .root
            .children
            .iter()
            .map(|item| item.name.as_str())
            .collect();
        assert_eq!(names, vec!["b", "a"]);
        let a = child(&result.root, "a");
        assert_eq!(a.type_name, "object");
        assert!(a.required);
        assert_eq!(a.children.len(), 2);
        assert_eq!(child(a, "c").type_name, "boolean");
        assert_eq!(child(a, "d").type_name, "null");
    }

    #[test]
    fn array_fields_are_required_only_when_present_in_all_elements() {
        let result = infer_json(r#"{"items":[{"id":1,"name":"a"},{"id":2}],"total":2}"#).unwrap();
        let items = child(&result.root, "items");
        assert_eq!(items.type_name, "array");
        assert_eq!(items.item_type, "object");
        assert!(child(items, "id").required);
        assert!(!child(items, "name").required);
    }

    #[test]
    fn type_conflicts_become_unknown_with_warning() {
        let result = infer_json(r#"{"items":[{"value":1},{"value":"x"}]}"#).unwrap();
        let value = child(child(&result.root, "items"), "value");
        assert_eq!(value.type_name, "unknown");
        assert!(result
            .warnings
            .iter()
            .any(|item| item.contains("类型不一致")));
    }

    #[test]
    fn top_level_array_exposes_element_fields() {
        let result = infer_json(r#"[{"id":1},{"id":2,"name":"b"}]"#).unwrap();
        assert_eq!(result.root.type_name, "array");
        assert_eq!(result.root.item_type, "object");
        assert!(child(&result.root, "id").required);
        assert!(!child(&result.root, "name").required);
    }

    #[test]
    fn form_and_multipart_are_flat() {
        let form = infer_body("form", "user=alice&profile%5Bcity%5D=shanghai").unwrap();
        let names: Vec<&str> = form
            .root
            .children
            .iter()
            .map(|item| item.name.as_str())
            .collect();
        assert_eq!(names, vec!["user", "profile[city]"]);
        assert!(form
            .root
            .children
            .iter()
            .all(|item| item.type_name == "string"));

        let multipart = infer_body(
            "multipart",
            r#"[{"key":"avatar","value":"C:/a.png","enabled":true,"kind":"file"},{"key":"name","value":"x","enabled":true,"kind":"text"}]"#,
        )
        .unwrap();
        assert_eq!(child(&multipart.root, "avatar").type_name, "file");
        assert_eq!(child(&multipart.root, "name").type_name, "string");
    }

    #[test]
    fn invalid_input_reports_parse_error() {
        assert!(infer_json("<html></html>")
            .unwrap_err()
            .contains("E_ENTITY_PARSE"));
        assert!(infer_body("none", "x")
            .unwrap_err()
            .contains("E_ENTITY_PARSE"));
    }

    #[test]
    fn depth_and_node_limits_keep_partial_result() {
        let mut deep = String::new();
        for _ in 0..20 {
            deep.push_str("{\"a\":");
        }
        deep.push('1');
        for _ in 0..20 {
            deep.push('}');
        }
        let result = infer_json(&deep).unwrap();
        assert!(result.warnings.iter().any(|item| item.contains("嵌套超过")));

        let wide = format!(
            "{{{}}}",
            (0..MAX_NODES + 20)
                .map(|index| format!("\"k{index}\":1"))
                .collect::<Vec<_>>()
                .join(",")
        );
        let result = infer_json(&wide).unwrap();
        assert_eq!(result.root.children.len(), MAX_NODES);
        assert!(result
            .warnings
            .iter()
            .any(|item| item.contains("字段数超过")));
    }

    #[test]
    fn oversized_input_keeps_inference_and_warns() {
        let text = format!(
            "{{\"blob\":\"{}\",\"id\":1}}",
            "a".repeat(MAX_INPUT_BYTES + 10)
        );
        let result = infer_json(&text).unwrap();
        assert!(result
            .warnings
            .iter()
            .any(|item| item.contains("超过 256 KB")));
        assert!(result.root.children.iter().any(|item| item.name == "id"));
    }
}
