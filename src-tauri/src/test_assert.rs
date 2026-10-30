//! 声明式断言引擎
//!
//! 用例的 `assertions_json` 存放断言数组，元素形如：
//! `{ "type": "status", "op": "in", "value": [200, 201], "name": "状态码 2xx" }`
//!
//! 支持 6 类断言：
//! | type | 说明 | 默认 op |
//! |---|---|---|
//! | `status` | 响应状态码 | `eq` |
//! | `durationMs` | 本次 HTTP 耗时 | `lte` |
//! | `header` | 响应头，`path` 为头名 | `eq` |
//! | `body` | 响应体原文（字符串匹配） | `contains` |
//! | `regex` | 响应体正则，`value` 为正则 | `matches` |
//! | `jsonPath` | JSON 取值，`path` 形如 `$.data.id` | `exists` |

use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssertionSpec {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub op: Option<String>,
    #[serde(default)]
    pub value: Option<Value>,
    /// header 名或 JSONPath 表达式
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub ignore_case: bool,
}

#[derive(Debug, Clone)]
pub struct AssertionOutcome {
    pub name: String,
    pub passed: bool,
    pub message: String,
}

/// 断言执行所需的响应视图
pub struct AssertInput<'a> {
    pub status: u16,
    pub duration_ms: u64,
    pub headers: &'a HashMap<String, String>,
    pub body: &'a str,
}

pub fn parse_specs(raw: Option<&str>) -> Result<Vec<AssertionSpec>, String> {
    let Some(text) = raw.map(str::trim).filter(|text| !text.is_empty()) else {
        return Ok(Vec::new());
    };
    serde_json::from_str::<Vec<AssertionSpec>>(text)
        .map_err(|e| format!("断言配置格式错误：{e}"))
}

pub fn evaluate(specs: &[AssertionSpec], input: &AssertInput<'_>) -> Vec<AssertionOutcome> {
    specs.iter().map(|spec| evaluate_one(spec, input)).collect()
}

fn evaluate_one(spec: &AssertionSpec, input: &AssertInput<'_>) -> AssertionOutcome {
    let name = spec
        .name
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| default_name(spec));
    let result = check(spec, input);
    match result {
        Ok(()) => AssertionOutcome {
            name,
            passed: true,
            message: String::new(),
        },
        Err(message) => AssertionOutcome {
            name,
            passed: false,
            message,
        },
    }
}

fn default_name(spec: &AssertionSpec) -> String {
    let op = spec.op.as_deref().unwrap_or("");
    match spec.kind.as_str() {
        "status" => format!("状态码 {op} {}", display(&spec.value.clone().unwrap_or(Value::Null))),
        "durationMs" => format!(
            "响应时间 {op} {} ms",
            display(&spec.value.clone().unwrap_or(Value::Null))
        ),
        "header" => format!(
            "响应头 {} {op} {}",
            spec.path.as_deref().unwrap_or(""),
            display(&spec.value.clone().unwrap_or(Value::Null))
        ),
        "body" => format!("响应体 {op} 指定文本"),
        "regex" => format!("响应体匹配 {}", display(&spec.value.clone().unwrap_or(Value::Null))),
        "jsonPath" => format!(
            "{} {op} {}",
            spec.path.as_deref().unwrap_or("$"),
            display(&spec.value.clone().unwrap_or(Value::Null))
        ),
        other => format!("未知断言 {other}"),
    }
}

fn display(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

fn check(spec: &AssertionSpec, input: &AssertInput<'_>) -> Result<(), String> {
    let op = spec
        .op
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_ascii_lowercase)
        .unwrap_or_else(|| default_op(&spec.kind).to_string());

    match spec.kind.as_str() {
        "status" => {
            let actual = Value::from(input.status);
            compare(op.as_str(), &actual, spec, input)
        }
        "durationMs" => {
            let actual = Value::from(input.duration_ms as f64);
            compare(op.as_str(), &actual, spec, input)
        }
        "header" => {
            let header = spec.path.as_deref().unwrap_or("").trim();
            if header.is_empty() {
                return Err("header 断言缺少 path（头名称）".to_string());
            }
            match lookup_header(input.headers, header) {
                Some(actual) => compare(op.as_str(), &Value::String(actual.to_string()), spec, input),
                None if op == "notexists" => Ok(()),
                None => Err(format!("响应头 {header} 不存在")),
            }
        }
        "body" => {
            let actual = Value::String(input.body.to_string());
            if op == "notexists" {
                return Ok(());
            }
            if op == "exists" {
                return if input.body.is_empty() {
                    Err("响应体为空".to_string())
                } else {
                    Ok(())
                };
            }
            compare(op.as_str(), &actual, spec, input)
        }
        "regex" => {
            let pattern = match &spec.value {
                Some(Value::String(text)) => text.clone(),
                Some(other) => other.to_string(),
                None => return Err("regex 断言缺少 value（正则表达式）".to_string()),
            };
            let regex = regex::RegexBuilder::new(&pattern)
                .case_insensitive(spec.ignore_case)
                .build()
                .map_err(|e| format!("正则表达式无效：{e}"))?;
            if regex.is_match(input.body) {
                Ok(())
            } else {
                Err(format!("响应体未匹配正则 {pattern}"))
            }
        }
        "jsonPath" => {
            let path = spec.path.as_deref().unwrap_or("").trim();
            if path.is_empty() {
                return Err("jsonPath 断言缺少 path（例如 $.data.id）".to_string());
            }
            let parsed: Value = match serde_json::from_str(input.body) {
                Ok(value) => value,
                Err(e) => return Err(format!("响应体不是合法 JSON：{e}")),
            };
            match resolve_path(&parsed, path) {
                Some(actual) => {
                    if op == "exists" {
                        return Ok(());
                    }
                    compare(op.as_str(), actual, spec, input)
                }
                None if op == "notexists" => Ok(()),
                None => Err(format!("JSONPath {path} 不存在")),
            }
        }
        other => Err(format!("不支持的断言类型：{other}")),
    }
}

fn default_op(kind: &str) -> &'static str {
    match kind {
        "durationMs" => "lte",
        "body" => "contains",
        "regex" => "matches",
        "jsonPath" => "exists",
        _ => "eq",
    }
}

fn lookup_header<'a>(headers: &'a HashMap<String, String>, name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

fn compare(
    op: &str,
    actual: &Value,
    spec: &AssertionSpec,
    _input: &AssertInput<'_>,
) -> Result<(), String> {
    let expected = spec.value.clone().unwrap_or(Value::Null);
    let pass = match op {
        "eq" | "==" | "equals" => value_eq(actual, &expected, spec.ignore_case),
        "ne" | "!=" | "notequals" => !value_eq(actual, &expected, spec.ignore_case),
        "in" => match &expected {
            Value::Array(items) => items
                .iter()
                .any(|item| value_eq(actual, item, spec.ignore_case)),
            _ => return Err("in 断言的 value 必须是数组".to_string()),
        },
        "notin" => match &expected {
            Value::Array(items) => !items
                .iter()
                .any(|item| value_eq(actual, item, spec.ignore_case)),
            _ => return Err("notIn 断言的 value 必须是数组".to_string()),
        },
        "contains" => value_contains(actual, &expected, spec.ignore_case),
        "notcontains" => !value_contains(actual, &expected, spec.ignore_case),
        "matches" => match (&actual, &expected) {
            (Value::String(text), Value::String(pattern)) => regex::RegexBuilder::new(pattern)
                .case_insensitive(spec.ignore_case)
                .build()
                .map(|regex| regex.is_match(text))
                .map_err(|e| format!("正则表达式无效：{e}"))?,
            _ => return Err("matches 断言要求实际值与 value 都是字符串".to_string()),
        },
        "exists" => !actual.is_null(),
        "notexists" => actual.is_null(),
        "lt" | "lte" | "gt" | "gte" => {
            let (Some(left), Some(right)) = (as_f64(actual), as_f64(&expected)) else {
                return Err(format!(
                    "数值比较需要两边都是数字：实际 {}，期望 {}",
                    display(actual),
                    display(&expected)
                ));
            };
            match op {
                "lt" => left < right,
                "lte" => left <= right,
                "gt" => left > right,
                _ => left >= right,
            }
        }
        other => return Err(format!("不支持的操作符：{other}")),
    };

    if pass {
        Ok(())
    } else {
        Err(format!("期望 {op} {}，实际 {}", display(&expected), display(actual)))
    }
}

fn as_f64(value: &Value) -> Option<f64> {
    match value {
        Value::Number(number) => number.as_f64(),
        Value::String(text) => text.trim().parse::<f64>().ok(),
        _ => None,
    }
}

fn value_eq(actual: &Value, expected: &Value, ignore_case: bool) -> bool {
    if let (Some(left), Some(right)) = (as_f64(actual), as_f64(expected)) {
        return (left - right).abs() < f64::EPSILON;
    }
    match (actual, expected) {
        (Value::String(left), Value::String(right)) => {
            if ignore_case {
                left.to_lowercase() == right.to_lowercase()
            } else {
                left == right
            }
        }
        (Value::Bool(left), Value::Bool(right)) => left == right,
        (Value::Null, Value::Null) => true,
        // 数字与字符串混用时按文本比较（状态码、耗时常见）
        (Value::Number(left), Value::String(right)) => left.to_string() == *right,
        (Value::String(left), Value::Number(right)) => *left == right.to_string(),
        _ => false,
    }
}

fn value_contains(actual: &Value, expected: &Value, ignore_case: bool) -> bool {
    match actual {
        Value::String(text) => {
            let needle = display(expected);
            if ignore_case {
                text.to_lowercase().contains(&needle.to_lowercase())
            } else {
                text.contains(&needle)
            }
        }
        Value::Array(items) => items.iter().any(|item| value_eq(item, expected, ignore_case)),
        _ => false,
    }
}

/// 迷你 JSONPath：支持 `$.a.b[0]`、`$["a"]`、`$[0]`，不支持通配与递归
pub fn resolve_path<'a>(root: &'a Value, path: &str) -> Option<&'a Value> {
    let path = path.trim();
    let rest = path.strip_prefix('$')?;
    let mut current = root;
    let mut chars = rest.chars().peekable();

    loop {
        match chars.peek() {
            None => return Some(current),
            Some('.') => {
                chars.next();
                let mut key = String::new();
                while let Some(ch) = chars.peek() {
                    if *ch == '.' || *ch == '[' {
                        break;
                    }
                    key.push(*ch);
                    chars.next();
                }
                if key.is_empty() {
                    return None;
                }
                current = current.get(&key)?;
            }
            Some('[') => {
                chars.next();
                let mut inner = String::new();
                let mut quoted = false;
                for ch in chars.by_ref() {
                    match ch {
                        '"' | '\'' => {
                            quoted = !quoted;
                        }
                        ']' if !quoted => break,
                        other => inner.push(other),
                    }
                }
                let inner = inner.trim();
                current = if let Ok(index) = inner.parse::<usize>() {
                    current.get(index)?
                } else {
                    current.get(inner)?
                };
            }
            Some(_) => return None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers() -> HashMap<String, String> {
        let mut map = HashMap::new();
        map.insert("content-type".to_string(), "application/json; charset=utf-8".to_string());
        map
    }

    fn input<'a>(headers: &'a HashMap<String, String>, body: &'a str) -> AssertInput<'a> {
        AssertInput {
            status: 200,
            duration_ms: 320,
            headers,
            body,
        }
    }

    #[test]
    fn status_and_duration_assertions() {
        let headers = headers();
        let specs = parse_specs(Some(
            r#"[{"type":"status","op":"in","value":[200,201]},
                {"type":"status","op":"eq","value":201},
                {"type":"durationMs","value":300}]"#,
        ))
        .unwrap();
        let outcomes = evaluate(&specs, &input(&headers, "{}"));
        assert!(outcomes[0].passed);
        assert!(!outcomes[1].passed);
        assert!(outcomes[1].message.contains("实际 200"));
        assert!(!outcomes[2].passed);
    }

    #[test]
    fn header_body_regex_and_jsonpath_assertions() {
        let headers = headers();
        let body = r#"{"code":0,"data":{"orderId":123,"items":[{"id":7}]}}"#;
        let specs = parse_specs(Some(
            r#"[{"type":"header","path":"Content-Type","op":"contains","value":"application/json"},
                {"type":"body","value":"orderId"},
                {"type":"regex","value":"orderId\"?:\\s*\\d+"},
                {"type":"jsonPath","path":"$.data.orderId","op":"eq","value":123},
                {"type":"jsonPath","path":"$.data.items[0].id","op":"eq","value":7},
                {"type":"jsonPath","path":"$.data.missing","op":"exists"}]"#,
        ))
        .unwrap();
        let outcomes = evaluate(&specs, &input(&headers, body));
        assert!(outcomes[0].passed, "{}", outcomes[0].message);
        assert!(outcomes[1].passed);
        assert!(outcomes[2].passed);
        assert!(outcomes[3].passed, "{}", outcomes[3].message);
        assert!(outcomes[4].passed);
        assert!(!outcomes[5].passed);
    }

    #[test]
    fn invalid_config_reports_error() {
        assert!(parse_specs(Some("not-json")).is_err());
        let headers = headers();
        let specs = parse_specs(Some(r#"[{"type":"nope"}]"#)).unwrap();
        let outcomes = evaluate(&specs, &input(&headers, ""));
        assert!(!outcomes[0].passed);
        assert!(outcomes[0].message.contains("不支持"));
    }

    #[test]
    fn resolve_path_supports_bracket_notation() {
        let value: Value = serde_json::json!({"a": {"b": [1, {"c": "x"}]}});
        assert_eq!(resolve_path(&value, "$.a.b[0]"), Some(&Value::from(1)));
        assert_eq!(
            resolve_path(&value, r#"$["a"]["b"][1].c"#),
            Some(&Value::from("x"))
        );
        assert!(resolve_path(&value, "$.a.missing").is_none());
    }
}
