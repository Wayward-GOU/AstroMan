//! 接口实体（请求 / 返回字段树）的持久化模型
//!
//! 只存字段结构：名称、类型、必填、说明与手工标记；**不含任何字段取值**，
//! 因此实体本身不会成为凭据泄漏源。JSON 列序列化时使用 camelCase，
//! 与前端 `EntityDoc` / `EntityNode` 直接对齐。

use serde::{Deserialize, Serialize};

pub const ENTITY_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntityDoc {
    pub version: u32,
    pub updated_at: i64,
    #[serde(default)]
    pub hidden_paths: Vec<String>,
    pub root: EntityNode,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct EntityNode {
    pub name: String,
    #[serde(rename = "type")]
    pub type_name: String,
    pub item_type: String,
    pub required: bool,
    pub description: String,
    /// 用户手工新增，或手工改过必填：自动同步保留，不参与消失判定
    pub manual: bool,
    /// 最近一次发送中已消失，但为防误删仍保留
    pub stale: bool,
    pub children: Vec<EntityNode>,
}

impl Default for EntityNode {
    fn default() -> Self {
        Self {
            name: String::new(),
            type_name: "object".to_string(),
            item_type: String::new(),
            required: false,
            description: String::new(),
            manual: false,
            stale: false,
            children: Vec::new(),
        }
    }
}

impl EntityNode {
    pub fn new_root(type_name: &str) -> Self {
        Self {
            name: "$".to_string(),
            type_name: type_name.to_string(),
            required: false,
            ..Self::default()
        }
    }
}

impl EntityDoc {
    pub fn new(root: EntityNode, updated_at: i64) -> Self {
        Self {
            version: ENTITY_VERSION,
            updated_at,
            hidden_paths: Vec::new(),
            root,
        }
    }

    /// 解码实体列。版本不认识或 JSON 损坏时返回可读错误，由调用方决定忽略还是提示。
    pub fn decode(raw: &str) -> Result<Self, String> {
        let doc: Self = serde_json::from_str(raw)
            .map_err(|e| format!("E_ENTITY_PARSE: 实体数据无法解析：{e}"))?;
        if doc.version != ENTITY_VERSION {
            return Err(format!(
                "E_ENTITY_VERSION: 实体版本 {} 不受支持（当前 {}）",
                doc.version, ENTITY_VERSION
            ));
        }
        Ok(doc)
    }

    pub fn encode(&self) -> Result<String, String> {
        serde_json::to_string(self).map_err(|e| format!("实体序列化失败：{e}"))
    }

    /// 根节点没有字段时视为空实体，文档中整节省略
    pub fn is_empty(&self) -> bool {
        self.root.children.is_empty()
    }

    /// 深度优先拍平：文档只需要「字段路径 + 类型 + 必填 + 说明」。
    ///
    /// 数组节点的路径带 `[]` 后缀（如 `data.roles[]`），其元素字段变成
    /// `data.roles[].name`；根节点本身不输出。
    pub fn flatten(&self) -> Vec<FlatEntityField> {
        let mut out = Vec::new();
        flatten_children(&self.root, "", 0, &mut out);
        out
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct FlatEntityField {
    pub path: String,
    pub type_name: String,
    pub required: bool,
    pub description: String,
    pub stale: bool,
    pub depth: u16,
}

fn flatten_children(
    parent: &EntityNode,
    parent_path: &str,
    depth: u16,
    out: &mut Vec<FlatEntityField>,
) {
    for child in &parent.children {
        let path = node_path(parent_path, &child.name, &child.type_name);
        let type_name = if child.type_name == "array" {
            if !child.item_type.is_empty() {
                child.item_type.clone()
            } else if child.children.is_empty() {
                "array（空数组）".to_string()
            } else {
                "array".to_string()
            }
        } else if child.type_name == "object" && child.children.is_empty() {
            "object（空对象）".to_string()
        } else {
            child.type_name.clone()
        };
        out.push(FlatEntityField {
            path: path.clone(),
            type_name,
            required: child.required,
            description: child.description.clone(),
            stale: child.stale,
            depth,
        });
        flatten_children(child, &path, depth + 1, out);
    }
}

/// 拼接字段路径：父路径为空 / `$` 时取字段名；数组节点自身带 `[]`。
pub fn node_path(parent_path: &str, name: &str, type_name: &str) -> String {
    let base = if parent_path.is_empty() || parent_path == "$" {
        name.to_string()
    } else {
        format!("{parent_path}.{name}")
    };
    if type_name == "array" {
        format!("{base}[]")
    } else {
        base
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(name: &str, type_name: &str, children: Vec<EntityNode>) -> EntityNode {
        EntityNode {
            name: name.to_string(),
            type_name: type_name.to_string(),
            required: true,
            children,
            ..EntityNode::default()
        }
    }

    #[test]
    fn decode_rejects_unknown_version() {
        let raw = r#"{"version":2,"updatedAt":0,"hiddenPaths":[],"root":{"name":"$","type":"object","itemType":"","required":false,"description":"","manual":false,"stale":false,"children":[]}}"#;
        assert!(EntityDoc::decode(raw)
            .unwrap_err()
            .contains("E_ENTITY_VERSION"));
        assert!(EntityDoc::decode("{")
            .unwrap_err()
            .contains("E_ENTITY_PARSE"));
    }

    #[test]
    fn flatten_uses_dotted_paths_and_array_suffixes() {
        let mut roles = node("roles", "array", vec![node("name", "string", vec![])]);
        roles.item_type = "string".into();
        let doc = EntityDoc::new(
            EntityNode {
                children: vec![
                    node(
                        "data",
                        "object",
                        vec![node("user", "object", vec![node("id", "number", vec![])])],
                    ),
                    roles,
                ],
                ..EntityNode::new_root("object")
            },
            0,
        );
        let fields = doc.flatten();
        let paths: Vec<&str> = fields.iter().map(|item| item.path.as_str()).collect();
        assert_eq!(
            paths,
            vec![
                "data",
                "data.user",
                "data.user.id",
                "roles[]",
                "roles[].name"
            ]
        );
        assert_eq!(fields[3].type_name, "string");
    }

    #[test]
    fn empty_containers_are_labelled() {
        let doc = EntityDoc::new(
            EntityNode {
                children: vec![
                    node("empty", "object", vec![]),
                    node("list", "array", vec![]),
                ],
                ..EntityNode::new_root("object")
            },
            0,
        );
        let types: Vec<String> = doc
            .flatten()
            .into_iter()
            .map(|item| item.type_name)
            .collect();
        assert_eq!(types, vec!["object（空对象）", "array（空数组）"]);
    }
}
