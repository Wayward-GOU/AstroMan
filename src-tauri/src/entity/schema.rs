//! 实体字段树 → OpenAPI 3.0 schema
//!
//! 技术方案 4.2「第 0 步」：实体是契约快照与文档导出的共同数据源，
//! 这里只做结构映射，不写入任何字段取值（取值与凭据仍由示例链负责）。
//!
//! 规则：
//! - object → `type: object` + `properties` + `required`（required 且非 stale）；
//! - array → `type: array` + `items`（由 `itemType` 或子节点推断，空数组 `items: {}`）；
//! - string / number / boolean → 对应 `type`；null → `nullable: true`；
//! - file → `type: string` + `format: binary`；
//! - unknown → `{}`，保留说明，避免误判破坏性变更；
//! - `hiddenPaths` 与 `stale` 的字段不进入契约，避免「已删除字段」继续参与 diff。

use serde_json::{json, Map, Value};

use super::model::{node_path, EntityDoc, EntityNode};

/// 把实体根节点转成 schema；空实体返回 `None`
pub fn schema_from_raw(raw: Option<&str>) -> Option<Value> {
    let doc = super::decode_lenient(raw)?;
    if doc.is_empty() {
        return None;
    }
    Some(entity_schema(&doc))
}

pub fn entity_schema(doc: &EntityDoc) -> Value {
    node_schema(&doc.root, "", doc)
}

fn node_schema(node: &EntityNode, path: &str, doc: &EntityDoc) -> Value {
    let mut schema = match node.type_name.as_str() {
        "object" => object_schema(node, path, doc),
        "array" => array_schema(node, path, doc),
        other => scalar_schema(other),
    };
    if let Value::Object(map) = &mut schema {
        if !node.description.trim().is_empty() {
            map.insert(
                "description".to_string(),
                Value::String(node.description.trim().to_string()),
            );
        }
    }
    schema
}

fn object_schema(node: &EntityNode, path: &str, doc: &EntityDoc) -> Value {
    let mut properties = Map::new();
    let mut required = Vec::new();
    for child in visible_children(node, path, doc) {
        let child_path = node_path(path, &child.name, &child.type_name);
        properties.insert(child.name.clone(), node_schema(child, &child_path, doc));
        if child.required {
            required.push(Value::String(child.name.clone()));
        }
    }
    let mut map = Map::new();
    map.insert("type".to_string(), json!("object"));
    if !properties.is_empty() {
        map.insert("properties".to_string(), Value::Object(properties));
    }
    if !required.is_empty() {
        map.insert("required".to_string(), Value::Array(required));
    }
    Value::Object(map)
}

fn array_schema(node: &EntityNode, path: &str, doc: &EntityDoc) -> Value {
    let items = if !node.children.is_empty() {
        let element = EntityNode {
            name: node.name.clone(),
            type_name: "object".to_string(),
            children: node.children.clone(),
            ..EntityNode::default()
        };
        object_schema(&element, path, doc)
    } else if node.item_type.is_empty() {
        json!({})
    } else {
        scalar_schema(&node.item_type)
    };
    json!({ "type": "array", "items": items })
}

fn scalar_schema(type_name: &str) -> Value {
    match type_name {
        "string" => json!({ "type": "string" }),
        "number" | "integer" => json!({ "type": "number" }),
        "boolean" => json!({ "type": "boolean" }),
        "null" => json!({ "nullable": true }),
        "file" => json!({ "type": "string", "format": "binary" }),
        // unknown / array（缺元素信息）/ 其它历史值：不写 type，避免噪声
        _ => json!({}),
    }
}

/// OpenAPI / JSON Schema → 实体字段树（G-07）
///
/// `resolve` 由调用方提供（通常是 `$ref` 解析），这样实体模块不依赖任何
/// 具体导入器的文档遍历逻辑。说明（`description`）、`required`、数组元素
/// 类型与嵌套对象一并映射；`format: binary` 记为 file，`nullable` 记为 null。
pub fn entity_from_schema(schema: &Value, resolve: &dyn Fn(&Value) -> Value) -> Option<EntityDoc> {
    let root_schema = resolve(schema);
    let mut root = schema_node("$", &root_schema, resolve, 0)?;
    root.name = "$".to_string();
    let doc = EntityDoc::new(root, 0);
    if doc.is_empty() {
        None
    } else {
        Some(doc)
    }
}

fn schema_node(
    name: &str,
    schema: &Value,
    resolve: &dyn Fn(&Value) -> Value,
    depth: usize,
) -> Option<EntityNode> {
    if depth > 12 {
        return None;
    }
    let schema = resolve(schema);
    let schema = if let Some(found) = merge_all_of(&schema) {
        resolve(&found)
    } else {
        schema
    };
    if !schema.is_object() {
        return None;
    }
    let mut node = EntityNode {
        name: name.to_string(),
        ..EntityNode::default()
    };
    if let Some(text) = schema.get("description").and_then(|v| v.as_str()) {
        node.description = text.trim().to_string();
    }
    let format = schema.get("format").and_then(|v| v.as_str()).unwrap_or("");
    let declared = schema.get("type").and_then(|v| v.as_str()).unwrap_or("");
    let has_props = schema.get("properties").is_some();
    let type_name = if declared.is_empty() && has_props {
        "object"
    } else {
        declared
    };
    match type_name {
        "object" if has_props => {
            node.type_name = "object".to_string();
            let required: Vec<&str> = schema
                .get("required")
                .and_then(|v| v.as_array())
                .map(|list| list.iter().filter_map(|v| v.as_str()).collect())
                .unwrap_or_default();
            if let Some(properties) = schema.get("properties").and_then(|v| v.as_object()) {
                for (key, child_schema) in properties {
                    if let Some(mut child) = schema_node(key, child_schema, resolve, depth + 1) {
                        child.required = required.iter().any(|item| *item == key);
                        node.children.push(child);
                    }
                }
            }
        }
        "array" => {
            node.type_name = "array".to_string();
            let items = schema.get("items").cloned().unwrap_or(Value::Null);
            if let Some(item) = schema_node(name, &items, resolve, depth + 1) {
                if item.type_name == "object" {
                    node.item_type = "object".to_string();
                    node.children = item.children;
                } else {
                    node.item_type = item.type_name;
                }
            }
        }
        "integer" | "number" => node.type_name = "number".to_string(),
        "boolean" => node.type_name = "boolean".to_string(),
        "string" if format == "binary" || format == "byte" => node.type_name = "file".to_string(),
        "string" => node.type_name = "string".to_string(),
        "null" => node.type_name = "null".to_string(),
        _ if schema.get("nullable").and_then(|v| v.as_bool()) == Some(true) => {
            node.type_name = "null".to_string()
        }
        _ => node.type_name = "unknown".to_string(),
    }
    Some(node)
}

/// `allOf` 合并：OpenAPI 常见组合写法，按 properties / required 取并集
fn merge_all_of(schema: &Value) -> Option<Value> {
    let list = schema.get("allOf").and_then(|v| v.as_array())?;
    let mut merged = schema.clone();
    let merged_map = merged.as_object_mut()?;
    merged_map.remove("allOf");
    let mut properties = Map::new();
    let mut required: Vec<Value> = Vec::new();
    for item in list {
        if let Some(props) = item.get("properties").and_then(|v| v.as_object()) {
            for (key, value) in props {
                properties.insert(key.clone(), value.clone());
            }
        }
        if let Some(list) = item.get("required").and_then(|v| v.as_array()) {
            for value in list {
                if !required.iter().any(|item| item == value) {
                    required.push(value.clone());
                }
            }
        }
    }
    if !properties.is_empty() {
        merged_map.insert("type".to_string(), json!("object"));
        merged_map.insert("properties".to_string(), Value::Object(properties));
    }
    if !required.is_empty() {
        merged_map.insert("required".to_string(), Value::Array(required));
    }
    Some(merged)
}

/// 可见子节点：排除隐藏路径与 stale（已消失但暂留）的节点
fn visible_children<'a>(node: &'a EntityNode, path: &str, doc: &EntityDoc) -> Vec<&'a EntityNode> {
    node.children
        .iter()
        .filter(|child| {
            if child.stale {
                return false;
            }
            let child_path = node_path(path, &child.name, &child.type_name);
            !doc.hidden_paths.iter().any(|item| item == &child_path)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(name: &str, type_name: &str) -> EntityNode {
        EntityNode {
            name: name.to_string(),
            type_name: type_name.to_string(),
            ..EntityNode::default()
        }
    }

    fn doc(root: EntityNode) -> EntityDoc {
        EntityDoc::new(root, 0)
    }

    #[test]
    fn object_maps_properties_and_required() {
        let mut id = node("id", "number");
        id.required = true;
        let mut name = node("name", "string");
        name.description = " 用户昵称 ".into();
        let root = EntityNode {
            children: vec![id, name],
            ..EntityNode::new_root("object")
        };
        let schema = entity_schema(&doc(root));
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["properties"]["id"]["type"], "number");
        assert_eq!(schema["properties"]["name"]["type"], "string");
        assert_eq!(schema["properties"]["name"]["description"], "用户昵称");
        assert_eq!(schema["required"], json!(["id"]));
    }

    #[test]
    fn arrays_map_items_from_children_or_item_type() {
        let mut users = node("users", "array");
        users.item_type = "object".into();
        users.children = vec![node("id", "number")];
        let mut tags = node("tags", "array");
        tags.item_type = "string".into();
        let root = EntityNode {
            children: vec![users, tags, node("empty", "array")],
            ..EntityNode::new_root("object")
        };
        let schema = entity_schema(&doc(root));
        assert_eq!(schema["properties"]["users"]["items"]["type"], "object");
        assert_eq!(
            schema["properties"]["users"]["items"]["properties"]["id"]["type"],
            "number"
        );
        assert_eq!(schema["properties"]["tags"]["items"]["type"], "string");
        assert_eq!(schema["properties"]["empty"]["items"], json!({}));
    }

    #[test]
    fn scalars_files_and_unknown_are_mapped() {
        let root = EntityNode {
            children: vec![
                node("ok", "boolean"),
                node("nothing", "null"),
                node("avatar", "file"),
                node("mystery", "unknown"),
            ],
            ..EntityNode::new_root("object")
        };
        let schema = entity_schema(&doc(root));
        assert_eq!(schema["properties"]["ok"]["type"], "boolean");
        assert_eq!(schema["properties"]["nothing"]["nullable"], true);
        assert_eq!(schema["properties"]["avatar"]["type"], "string");
        assert_eq!(schema["properties"]["avatar"]["format"], "binary");
        assert!(schema["properties"]["mystery"]
            .as_object()
            .unwrap()
            .is_empty());
    }

    #[test]
    fn hidden_and_stale_fields_are_excluded() {
        let mut stale = node("legacy", "string");
        stale.stale = true;
        let hidden = node("secret", "string");
        let root = EntityNode {
            children: vec![node("keep", "string"), stale, hidden],
            ..EntityNode::new_root("object")
        };
        let mut entity = doc(root);
        entity.hidden_paths = vec!["secret".to_string()];
        let schema = entity_schema(&entity);
        let props = schema["properties"].as_object().unwrap();
        assert!(props.contains_key("keep"));
        assert!(!props.contains_key("legacy"));
        assert!(!props.contains_key("secret"));
    }

    #[test]
    fn empty_entity_has_no_schema() {
        assert!(schema_from_raw(None).is_none());
        assert!(schema_from_raw(Some("{}")).is_none());
        let empty = EntityDoc::new(EntityNode::new_root("object"), 0)
            .encode()
            .unwrap();
        assert!(schema_from_raw(Some(&empty)).is_none());
    }

    #[test]
    fn schema_maps_back_to_entity_with_descriptions() {
        let schema = json!({
            "type": "object",
            "required": ["id"],
            "properties": {
                "id": {"type": "integer", "description": "编号"},
                "title": {"type": "string", "description": "标题"},
                "tags": {"type": "array", "items": {"type": "string"}},
                "roles": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {"name": {"type": "string", "description": "角色名"}}
                    }
                },
                "avatar": {"type": "string", "format": "binary"}
            }
        });
        let doc = entity_from_schema(&schema, &|value| value.clone()).unwrap();
        let flat = doc.flatten();
        let find = |path: &str| flat.iter().find(|field| field.path == path).unwrap();
        assert_eq!(find("id").type_name, "number");
        assert!(find("id").required);
        assert_eq!(find("id").description, "编号");
        assert_eq!(find("title").description, "标题");
        assert_eq!(find("tags[]").type_name, "string");
        assert_eq!(find("roles[]").type_name, "object");
        assert_eq!(find("roles[].name").description, "角色名");
        assert_eq!(find("avatar").type_name, "file");
    }

    #[test]
    fn schema_ref_and_all_of_are_resolved() {
        let root = json!({
            "components": {"schemas": {"User": {
                "allOf": [
                    {"type": "object", "properties": {"id": {"type": "number"}}},
                    {"type": "object", "properties": {"name": {"type": "string", "description": "名"}}}
                ]
            }}}
        });
        let schema = json!({"$ref": "#/components/schemas/User"});
        let resolve = |value: &Value| -> Value {
            let Some(reference) = value.get("$ref").and_then(|v| v.as_str()) else {
                return value.clone();
            };
            let path = reference.strip_prefix("#/").unwrap_or(reference);
            let mut node = &root;
            for part in path.split('/') {
                node = node.get(part).unwrap_or(&Value::Null);
            }
            node.clone()
        };
        let doc = entity_from_schema(&schema, &resolve).unwrap();
        let flat = doc.flatten();
        assert_eq!(flat.len(), 2);
        assert_eq!(
            flat.iter()
                .find(|field| field.path == "name")
                .unwrap()
                .description,
            "名"
        );
    }
}
