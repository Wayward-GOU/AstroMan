//! 文档示例值（G-05 起复用 `services::faker`）
//!
//! 确定性：同一接口、同一字段序号，反复导出结果一致。只在「没有定义值、
//! 没有运行记录」时使用，用来让文档里的参数表不至于空着。

use crate::services::faker::Faker;

/// 确定性示例值：同一接口、同一字段序号，反复导出结果一致。
pub fn auto_value(name: &str, value_hint: &str, seq: usize, request_id: i64) -> String {
    let lower = name.to_ascii_lowercase();
    let hint = value_hint.to_ascii_lowercase();
    let mut faker = Faker::deterministic(Faker::seed_for(request_id, seq));

    // 分页 / 数量 / 金额：文档里固定值比随机值更好读
    if lower == "page" || lower == "offset" || lower.ends_with("_page") {
        return "1".to_string();
    }
    if matches!(
        lower.as_str(),
        "size" | "limit" | "pagesize" | "page_size" | "per_page"
    ) {
        return "10".to_string();
    }
    if lower.contains("price") || lower.contains("amount") || lower.contains("total") {
        return "99.90".to_string();
    }

    let expr = if lower.contains("email") {
        "email"
    } else if lower.contains("phone") || lower.contains("mobile") {
        "phone"
    } else if lower.contains("uuid") || lower.contains("guid") {
        "uuid"
    } else if lower.contains("datetime") || lower.contains("time") {
        "datetime"
    } else if lower.contains("date") {
        "date"
    } else if lower.contains("city") {
        "city"
    } else if lower.contains("company") {
        "company"
    } else if lower.contains("name") || lower.contains("username") {
        "nameZh"
    } else if hint.contains("bool") || lower.starts_with("is_") || lower.starts_with("has_") {
        "bool"
    } else if lower.contains("id")
        || lower.ends_with("no")
        || hint.contains("int")
        || hint.contains("number")
    {
        // ID 类固定成可读序号，避免每次文档出现大段随机数字
        return (10001 + seq).to_string();
    } else {
        "word"
    };

    faker
        .value(expr)
        .unwrap_or_else(|| format!("示例值{}", seq + 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_values_are_deterministic() {
        assert_eq!(auto_value("page", "", 0, 7), "1");
        assert_eq!(auto_value("size", "", 1, 7), "10");
        assert_eq!(auto_value("email", "", 2, 7), auto_value("email", "", 2, 7));
        assert_eq!(auto_value("userId", "", 0, 7), "10001");
        assert_eq!(
            auto_value("userId", "", 0, 7),
            auto_value("userId", "", 0, 7)
        );
    }

    #[test]
    fn auto_values_cover_common_field_shapes() {
        let email = auto_value("email", "", 2, 7);
        assert!(email.contains('@'));
        let phone = auto_value("phone", "", 3, 7);
        assert_eq!(phone.len(), 11);
        let uuid = auto_value("uuid", "", 4, 7);
        assert_eq!(uuid.len(), 36);
        let name = auto_value("name", "", 5, 7);
        assert!(!name.is_empty());
        assert_ne!(name, auto_value("name", "", 6, 7));
        // 不同接口同名字段也应得到不同示例
        assert_ne!(auto_value("name", "", 5, 1), auto_value("name", "", 5, 2));
    }
}
