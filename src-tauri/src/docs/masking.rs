use super::model::MASK;
use serde_json::Value;

/// 需要在产物中抹掉的敏感值集合。
///
/// 收集顺序不重要：掩码时按长度倒序替换，避免短值先替换把长值切碎。
#[derive(Debug, Default, Clone)]
pub struct SecretSet {
    values: Vec<String>,
}

impl SecretSet {
    pub fn add(&mut self, value: &str) {
        let value = value.trim();
        if value.is_empty() || value == MASK {
            return;
        }
        if !self.values.iter().any(|item| item == value) {
            self.values.push(value.to_string());
        }
    }

    pub fn extend(&mut self, values: impl IntoIterator<Item = String>) {
        for value in values {
            self.add(&value);
        }
    }

    fn sorted(&self) -> Vec<&str> {
        let mut list: Vec<&str> = self.values.iter().map(String::as_str).collect();
        list.sort_by_key(|item| std::cmp::Reverse(item.chars().count()));
        list
    }

    /// 普通文本：长度 ≥ 4 的值按子串替换；更短的值只在整段完全相等时替换，
    /// 避免把 `abc` 这种短值打进正常单词里。
    pub fn mask_text(&self, text: &str) -> String {
        if text.is_empty() {
            return String::new();
        }
        let mut out = text.to_string();
        for secret in self.sorted() {
            if secret.chars().count() >= 4 {
                out = out.replace(secret, MASK);
            } else if out == secret {
                out = MASK.to_string();
            }
            if out == MASK {
                break;
            }
        }
        out
    }

    /// JSON：先按敏感字段名整体掩码，再逐字符串值做子串替换；解析失败时退化为文本掩码。
    pub fn mask_json(&self, raw: &str) -> String {
        if raw.trim().is_empty() {
            return raw.to_string();
        }
        let Ok(mut value) = serde_json::from_str::<Value>(raw) else {
            return self.mask_text(raw);
        };
        self.mask_json_value(&mut value);
        serde_json::to_string_pretty(&value).unwrap_or_else(|_| self.mask_text(raw))
    }

    fn mask_json_value(&self, value: &mut Value) {
        match value {
            Value::String(text) => *text = self.mask_text(text),
            Value::Array(items) => {
                for item in items {
                    self.mask_json_value(item);
                }
            }
            Value::Object(map) => {
                for (key, item) in map.iter_mut() {
                    if is_sensitive_name(key) {
                        *item = Value::String(MASK.to_string());
                    } else {
                        self.mask_json_value(item);
                    }
                }
            }
            _ => {}
        }
    }
}

/// 敏感名称判定：按 `_` / `-` / `.` / 空格与 camelCase 分词后整词比较。
///
/// 整词比较保证 `keyword`、`monkey`、`tokenizer`、`pageSize` 不会被误判。
pub fn is_sensitive_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "authorization" | "proxy-authorization" | "cookie" | "set-cookie" | "apikey"
    ) {
        return true;
    }
    split_words(name).iter().any(|word| {
        matches!(
            word.as_str(),
            "token" | "password" | "passwd" | "secret" | "key" | "credential"
        )
    })
}

fn split_words(name: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut prev_lower = false;
    for ch in name.chars() {
        if matches!(ch, '_' | '-' | '.' | ' ') {
            if !current.is_empty() {
                words.push(current.to_ascii_lowercase());
                current.clear();
            }
            prev_lower = false;
            continue;
        }
        if ch.is_ascii_uppercase() {
            if prev_lower && !current.is_empty() {
                words.push(current.to_ascii_lowercase());
                current.clear();
            }
            current.push(ch);
            prev_lower = false;
        } else {
            current.push(ch);
            prev_lower = ch.is_ascii_lowercase() || ch.is_ascii_digit();
        }
    }
    if !current.is_empty() {
        words.push(current.to_ascii_lowercase());
    }
    words
}

/// URL userinfo 整体掩码：`https://user:pass@host` → `https://••••••@host`
pub fn mask_userinfo(url: &str) -> String {
    if let Some(scheme_end) = url.find("://") {
        let rest = &url[scheme_end + 3..];
        if let Some(at) = rest.find('@') {
            let authority = &rest[..at];
            if authority.contains(':') {
                return format!("{}://{}@{}", &url[..scheme_end], MASK, &rest[at + 1..]);
            }
        }
    }
    url.to_string()
}

/// 按字节上限截断，且不会切断 UTF-8 字符
pub fn truncate_body(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_string();
    }
    let mut end = max_bytes;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n（已截断）", &text[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sensitive_names_use_whole_words() {
        for name in [
            "api_key",
            "apiKey",
            "accessToken",
            "X-API-Key",
            "password",
            "clientSecret",
        ] {
            assert!(is_sensitive_name(name), "{name} 应命中");
        }
        for name in ["keyword", "tokenizer", "monkey", "pageSize"] {
            assert!(!is_sensitive_name(name), "{name} 不应命中");
        }
    }

    #[test]
    fn secret_set_masks_long_values_first_and_short_values_only_exactly() {
        let mut set = SecretSet::default();
        set.add("abc");
        set.add("abcdef");
        set.add("");
        assert_eq!(set.mask_text("x abcdef y"), "x •••••• y");
        assert_eq!(set.mask_text("abc"), "••••••");
        assert_eq!(set.mask_text("abcd"), "abcd");
    }

    #[test]
    fn json_masking_replaces_values_without_breaking_structure() {
        let mut set = SecretSet::default();
        set.add("s3cr3t-value");
        let out = set.mask_json(r#"{"password":"p@ss","note":"prefix-s3cr3t-value-suffix","n":1}"#);
        let value: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(value["password"], "••••••");
        assert_eq!(value["note"], "prefix-••••••-suffix");
        assert_eq!(value["n"], 1);
    }

    #[test]
    fn userinfo_is_masked() {
        assert_eq!(
            mask_userinfo("https://u:p@example.com/a"),
            "https://••••••@example.com/a"
        );
        assert_eq!(
            mask_userinfo("https://example.com/a"),
            "https://example.com/a"
        );
    }

    #[test]
    fn truncation_keeps_char_boundaries() {
        let text = "中文内容".repeat(10);
        let out = truncate_body(&text, 7);
        assert!(out.contains("已截断"));
        assert!(out.len() > 7);
    }
}
