//! 实体合并：自动同步与手动生成共用
//!
//! 合并而非替换（技术方案 5.8.4 / 13.4）：
//! - 字段说明永远保留；
//! - `manual` 字段不参与「消失」判定，必填也不被自动改写；
//! - 新字段追加到父节点末尾，旧字段消失保留并标 `stale`；
//! - `hiddenPaths` 中的字段不再自动加回，显式「重新生成」时清空。

use std::collections::HashSet;

use super::model::{node_path, EntityDoc, EntityNode, ENTITY_VERSION};

#[derive(Debug, Clone)]
pub struct MergeResult {
    pub doc: EntityDoc,
    pub warnings: Vec<String>,
}

/// `clear_hidden = true` 对应界面上的「重新生成」：清空隐藏路径并允许字段回归。
pub fn merge(
    existing: Option<EntityDoc>,
    incoming: EntityNode,
    clear_hidden: bool,
    now: i64,
) -> MergeResult {
    let mut warnings = Vec::new();
    let hidden: HashSet<String> = if clear_hidden {
        HashSet::new()
    } else {
        existing
            .as_ref()
            .map(|doc| doc.hidden_paths.iter().cloned().collect())
            .unwrap_or_default()
    };

    let Some(mut old) = existing else {
        let mut root = incoming;
        sanitize_new(&mut root);
        return MergeResult {
            doc: EntityDoc::new(root, now),
            warnings,
        };
    };

    if old.root.type_name != incoming.type_name {
        warnings.push(format!(
            "实体根类型由 {} 变为 {}，已按最新结构重建（字段说明需重新填写）",
            old.root.type_name, incoming.type_name
        ));
        let mut root = incoming;
        sanitize_new(&mut root);
        old.root = root;
    } else {
        merge_node(&mut old.root, &incoming, "$", &hidden);
    }

    if clear_hidden {
        old.hidden_paths.clear();
    }
    old.version = ENTITY_VERSION;
    old.updated_at = now;
    MergeResult { doc: old, warnings }
}

fn sanitize_new(node: &mut EntityNode) {
    node.manual = false;
    node.stale = false;
    node.description.clear();
    for child in &mut node.children {
        sanitize_new(child);
    }
}

fn merge_node(
    old: &mut EntityNode,
    incoming: &EntityNode,
    parent_path: &str,
    hidden: &HashSet<String>,
) {
    old.type_name = incoming.type_name.clone();
    old.item_type = incoming.item_type.clone();
    if !old.manual {
        old.required = incoming.required;
    }
    old.stale = false;

    let mut merged: Vec<EntityNode> =
        Vec::with_capacity(old.children.len() + incoming.children.len());
    for child in &incoming.children {
        let path = node_path(parent_path, &child.name, &child.type_name);
        if hidden.contains(&path) {
            continue;
        }
        if let Some(index) = old.children.iter().position(|item| item.name == child.name) {
            let mut kept = old.children.remove(index);
            merge_node(&mut kept, child, &path, hidden);
            merged.push(kept);
        } else {
            let mut added = child.clone();
            sanitize_new(&mut added);
            merged.push(added);
        }
    }

    // 旧字段在新观测中消失：保留并标记 stale，提醒用户确认后再删
    for mut leftover in old.children.drain(..) {
        let path = node_path(parent_path, &leftover.name, &leftover.type_name);
        if hidden.contains(&path) {
            continue;
        }
        leftover.stale = !leftover.manual;
        merged.push(leftover);
    }

    old.children = merged;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(name: &str, type_name: &str, required: bool) -> EntityNode {
        EntityNode {
            name: name.to_string(),
            type_name: type_name.to_string(),
            required,
            ..EntityNode::default()
        }
    }

    fn doc(children: Vec<EntityNode>) -> EntityDoc {
        EntityDoc::new(
            EntityNode {
                children,
                ..EntityNode::new_root("object")
            },
            1,
        )
    }

    fn find<'a>(node: &'a EntityNode, name: &str) -> &'a EntityNode {
        node.children
            .iter()
            .find(|item| item.name == name)
            .unwrap_or_else(|| panic!("缺少字段 {name}"))
    }

    #[test]
    fn merge_preserves_descriptions_and_manual_required() {
        let mut old = doc(vec![
            node("id", "number", true),
            node("name", "string", true),
        ]);
        old.root.children[0].description = "用户 ID".to_string();
        old.root.children[1].required = false;
        old.root.children[1].manual = true;

        let incoming = EntityNode {
            children: vec![node("id", "string", false), node("name", "string", true)],
            ..EntityNode::new_root("object")
        };
        let result = merge(Some(old), incoming, false, 2);
        let id = find(&result.doc.root, "id");
        assert_eq!(id.type_name, "string");
        assert_eq!(id.description, "用户 ID");
        assert!(!id.required);
        let name = find(&result.doc.root, "name");
        assert!(!name.required);
        assert!(name.manual);
        assert_eq!(result.doc.updated_at, 2);
    }

    #[test]
    fn disappeared_fields_are_kept_as_stale_and_new_fields_appended() {
        let old = doc(vec![node("legacy", "string", true)]);
        let incoming = EntityNode {
            children: vec![node("fresh", "number", true)],
            ..EntityNode::new_root("object")
        };
        let result = merge(Some(old), incoming, false, 2);
        let names: Vec<&str> = result
            .doc
            .root
            .children
            .iter()
            .map(|item| item.name.as_str())
            .collect();
        assert_eq!(names, vec!["fresh", "legacy"]);
        assert!(find(&result.doc.root, "legacy").stale);
        assert!(!find(&result.doc.root, "fresh").stale);
    }

    #[test]
    fn reappearing_field_clears_stale() {
        let mut old = doc(vec![node("id", "number", true)]);
        old.root.children[0].stale = true;
        let incoming = EntityNode {
            children: vec![node("id", "number", true)],
            ..EntityNode::new_root("object")
        };
        let result = merge(Some(old), incoming, false, 2);
        assert!(!find(&result.doc.root, "id").stale);
    }

    #[test]
    fn hidden_paths_block_auto_add_until_cleared() {
        let mut old = doc(vec![]);
        old.hidden_paths = vec!["debug".to_string()];
        let incoming = EntityNode {
            children: vec![node("debug", "object", true), node("data", "object", true)],
            ..EntityNode::new_root("object")
        };
        let result = merge(Some(old), incoming.clone(), false, 2);
        assert!(result
            .doc
            .root
            .children
            .iter()
            .all(|item| item.name != "debug"));
        assert_eq!(result.doc.hidden_paths, vec!["debug".to_string()]);

        let result = merge(Some(result.doc), incoming, true, 3);
        assert!(result
            .doc
            .root
            .children
            .iter()
            .any(|item| item.name == "debug"));
        assert!(result.doc.hidden_paths.is_empty());
    }

    #[test]
    fn manual_fields_are_never_marked_stale() {
        let mut old = doc(vec![node("custom", "string", false)]);
        old.root.children[0].manual = true;
        let incoming = EntityNode::new_root("object");
        let result = merge(Some(old), incoming, false, 2);
        assert!(result.doc.root.children.iter().all(|item| !item.stale));
    }

    #[test]
    fn root_type_switch_rebuilds_tree() {
        let old = doc(vec![node("id", "number", true)]);
        let incoming = EntityNode {
            children: vec![node("id", "number", true)],
            ..EntityNode::new_root("array")
        };
        let result = merge(Some(old), incoming, false, 2);
        assert_eq!(result.doc.root.type_name, "array");
        assert!(result.warnings.iter().any(|item| item.contains("根类型")));
    }

    #[test]
    fn repeated_merge_is_idempotent() {
        let old = doc(vec![node("id", "number", true)]);
        let incoming = EntityNode {
            children: vec![node("id", "number", true), node("name", "string", true)],
            ..EntityNode::new_root("object")
        };
        let first = merge(Some(old), incoming.clone(), false, 2).doc;
        let second = merge(Some(first.clone()), incoming, false, 2).doc;
        assert_eq!(first, second);
    }
}
