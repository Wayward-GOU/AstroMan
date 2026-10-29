use chrono::{Local, SecondsFormat};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// 统一的错误转字符串，方便直接返回给前端展示
pub fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

/// 去掉 JSON 文本里的注释，与 Postman 的 raw(JSON) 编辑器保持一致
///
/// 支持 `// 行注释` 与 `/* 块注释 */`，字符串内部的 `//`（例如 URL）不会被误删。
pub fn strip_json_comments(input: &str) -> String {
    let chars: Vec<char> = input.chars().collect();
    let mut out = String::with_capacity(input.len());
    let mut index = 0;
    let mut in_string = false;
    let mut escaped = false;

    while index < chars.len() {
        let current = chars[index];
        if in_string {
            out.push(current);
            if escaped {
                escaped = false;
            } else if current == '\\' {
                escaped = true;
            } else if current == '"' {
                in_string = false;
            }
            index += 1;
            continue;
        }
        if current == '"' {
            in_string = true;
            out.push(current);
            index += 1;
            continue;
        }
        if current == '/' && index + 1 < chars.len() {
            match chars[index + 1] {
                '/' => {
                    index += 2;
                    while index < chars.len() && chars[index] != '\n' {
                        index += 1;
                    }
                    continue;
                }
                '*' => {
                    index += 2;
                    while index + 1 < chars.len() && !(chars[index] == '*' && chars[index + 1] == '/') {
                        index += 1;
                    }
                    index = (index + 2).min(chars.len());
                    continue;
                }
                _ => {}
            }
        }
        out.push(current);
        index += 1;
    }

    out
}

/// 简单模板渲染：支持 {{var}} 与内置变量
///
/// 内置变量支持 `{{timestamp}}` 与 Postman 风格的 `{{$timestamp}}` 两种写法。
pub fn render(input: &str, vars: &HashMap<String, String>) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;

    while let Some(start) = rest.find("{{") {
        let Some(end_rel) = rest[start + 2..].find("}}") else {
            break;
        };
        let end = start + 2 + end_rel;
        out.push_str(&rest[..start]);
        let expr = rest[start + 2..end].trim();
        out.push_str(&resolve_expr(expr, vars));
        rest = &rest[end + 2..];
    }
    out.push_str(rest);
    out
}

fn resolve_expr(expr: &str, vars: &HashMap<String, String>) -> String {
    let name = expr.strip_prefix('$').unwrap_or(expr);

    // 自定义变量优先，避免同名环境变量被内置变量覆盖
    if let Some(value) = vars.get(expr).or_else(|| vars.get(name)) {
        return value.clone();
    }

    if let Some(value) = resolve_time(name) {
        return value;
    }
    if let Some(value) = resolve_plain(name) {
        return value;
    }
    if let Some(value) = resolve_function(name) {
        return value;
    }

    format!("{{{{{expr}}}}}")
}

/// 与时间相关的内置变量，统一按本机时区输出
fn resolve_time(name: &str) -> Option<String> {
    let now = Local::now();
    Some(match name {
        "timestamp" | "timestampMs" => now.timestamp_millis().to_string(),
        "timestampSec" => now.timestamp().to_string(),
        "isoTime" | "isoTimestamp" => now.to_rfc3339_opts(SecondsFormat::Millis, false),
        "datetime" | "now" => now.format("%Y-%m-%d %H:%M:%S").to_string(),
        "date" | "today" => now.format("%Y-%m-%d").to_string(),
        "time" => now.format("%H:%M:%S").to_string(),
        "yyyyMMdd" => now.format("%Y%m%d").to_string(),
        "yyyyMMddHHmmss" => now.format("%Y%m%d%H%M%S").to_string(),
        _ => return None,
    })
}

/// 不需要参数的内置变量
fn resolve_plain(name: &str) -> Option<String> {
    Some(match name {
        "uuid" | "guid" => uuid::Uuid::new_v4().to_string(),
        "uuidNoDash" | "guidNoDash" | "uuidSimple" => uuid::Uuid::new_v4().simple().to_string(),
        "randomBool" => {
            if random_int(0, 1) == 1 {
                "true".to_string()
            } else {
                "false".to_string()
            }
        }
        "randomHex" => random_string(16, "hex"),
        "randomEmail" => format!("fink_{}@example.com", random_string(8, "hex")),
        "randomPhone" => format!("1{}{}", random_int(3, 9), random_string(9, "num")),
        "randomIPv4" => format!(
            "{}.{}.{}.{}",
            random_int(1, 254),
            random_int(0, 255),
            random_int(0, 255),
            random_int(1, 254)
        ),
        "randomInt" => random_int(1, 1000).to_string(),
        "randomString" => random_string(8, "alnum"),
        _ => return None,
    })
}

/// 带参数的函数型变量
fn resolve_function(name: &str) -> Option<String> {
    if let Some(inner) = call_args(name, "randomInt") {
        let args = int_args(inner)?;
        return Some(match args.as_slice() {
            [max] => random_int(0, *max).to_string(),
            [min, max] => random_int(*min, *max).to_string(),
            _ => return None,
        });
    }

    if let Some(inner) = call_args(name, "randomFloat") {
        let raw: Vec<&str> = inner.split(',').map(str::trim).collect();
        let min = raw.first()?.parse::<f64>().ok()?;
        let max = raw.get(1)?.parse::<f64>().ok()?;
        let decimals = match raw.get(2) {
            Some(value) => value.parse::<usize>().ok()?.min(6),
            None => 2,
        };
        return Some(random_float(min, max, decimals));
    }

    if let Some(inner) = call_args(name, "randomString") {
        let raw: Vec<&str> = inner.split(',').map(str::trim).collect();
        let len = raw.first()?.parse::<usize>().ok()?;
        let pool = raw.get(1).copied().unwrap_or("alnum");
        return Some(random_string(len, pool));
    }

    if let Some(inner) = call_args(name, "randomHex") {
        let len = inner.trim().parse::<usize>().ok()?;
        return Some(random_string(len, "hex"));
    }

    if let Some(inner) = call_args(name, "randomNumber") {
        let len = inner.trim().parse::<usize>().ok()?;
        return Some(random_string(len, "num"));
    }

    None
}

fn call_args<'a>(expr: &'a str, name: &str) -> Option<&'a str> {
    expr.strip_prefix(name)?
        .strip_prefix('(')?
        .strip_suffix(')')
}

fn int_args(inner: &str) -> Option<Vec<i64>> {
    inner
        .split(',')
        .map(|part| part.trim().parse::<i64>().ok())
        .collect()
}

/// 用 UUID 作为熵源，避免额外引入随机数依赖
fn entropy() -> u128 {
    uuid::Uuid::new_v4().as_u128()
}

fn random_int(min: i64, max: i64) -> i64 {
    if max <= min {
        return min;
    }
    let span = (max as i128 - min as i128 + 1) as u128;
    (min as i128 + (entropy() % span) as i128) as i64
}

fn random_float(min: f64, max: f64, decimals: usize) -> String {
    let ratio = entropy() as f64 / u128::MAX as f64;
    let value = if max > min {
        min + (max - min) * ratio
    } else {
        min
    };
    format!("{value:.decimals$}")
}

fn random_string(len: usize, pool: &str) -> String {
    const ALNUM: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    const ALPHA: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

    let chars: Vec<char> = match pool.trim().to_ascii_lowercase().as_str() {
        "num" | "number" | "digit" | "digits" => "0123456789".chars().collect(),
        "hex" => "0123456789abcdef".chars().collect(),
        "lower" => "abcdefghijklmnopqrstuvwxyz".chars().collect(),
        "upper" => "ABCDEFGHIJKLMNOPQRSTUVWXYZ".chars().collect(),
        "alpha" | "letter" | "letters" => ALPHA.chars().collect(),
        _ => ALNUM.chars().collect(),
    };

    let len = len.clamp(1, 4096);
    let mut out = String::with_capacity(len);
    for _ in 0..len {
        out.push(chars[(entropy() % chars.len() as u128) as usize]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
            .collect()
    }

    #[test]
    fn strip_comments_keeps_urls_inside_strings() {
        let raw = "{\n  // 行注释\n  \"url\": \"https://example.com/a//b\",\n  /* 块注释 */\n  \"n\": 1\n}";
        let clean = strip_json_comments(raw);
        assert!(clean.contains("https://example.com/a//b"));
        assert!(!clean.contains("行注释"));
        assert!(!clean.contains("块注释"));
        assert!(serde_json::from_str::<serde_json::Value>(clean.trim()).is_ok());
    }

    #[test]
    fn strip_comments_handles_escaped_quotes_and_unclosed_block() {
        let raw = "{\"a\":\"say \\\"hi\\\" // 不是注释\",\"b\":1 /* 未闭合";
        let clean = strip_json_comments(raw);
        assert!(clean.contains("say \\\"hi\\\" // 不是注释"));
        assert!(!clean.contains("未闭合"));
    }

    #[test]
    fn render_replaces_custom_vars_and_keeps_unknown() {
        let map = vars(&[("host", "api.local"), ("token", "abc")]);
        assert_eq!(
            render("http://{{host}}/v1?t={{token}}", &map),
            "http://api.local/v1?t=abc"
        );
        assert_eq!(render("{{ missing }}", &map), "{{missing}}");
    }

    #[test]
    fn render_supports_dollar_prefixed_builtins() {
        let map = HashMap::new();
        let out = render("{{$uuid}}", &map);
        assert_eq!(out.len(), 36);
        assert!(out.contains('-'));
        let ts = render("{{$timestamp}}", &map);
        assert!(ts.parse::<i64>().unwrap() > 1_600_000_000_000);
        let simple = render("{{uuidNoDash}}", &map);
        assert_eq!(simple.len(), 32);
        assert!(!simple.contains('-'));
    }

    #[test]
    fn render_custom_var_wins_over_builtin() {
        let map = vars(&[("uuid", "fixed")]);
        assert_eq!(render("{{uuid}}", &map), "fixed");
    }

    #[test]
    fn render_function_vars_respect_bounds() {
        let map = HashMap::new();
        for _ in 0..20 {
            let value = render("{{randomInt(5,9)}}", &map);
            let parsed: i64 = value.parse().unwrap();
            assert!((5..=9).contains(&parsed));
        }
        let hex = render("{{randomHex(8)}}", &map);
        assert_eq!(hex.len(), 8);
        assert!(hex.chars().all(|c| c.is_ascii_hexdigit()));
        let digits = render("{{randomNumber(6)}}", &map);
        assert_eq!(digits.len(), 6);
        assert!(digits.chars().all(|c| c.is_ascii_digit()));
        let floats = render("{{randomFloat(1,2,3)}}", &map);
        let parsed: f64 = floats.parse().unwrap();
        assert!((1.0..=2.0).contains(&parsed));
        assert_eq!(floats.split('.').last().unwrap().len(), 3);
    }

    #[test]
    fn random_string_clamps_length_and_pool() {
        assert_eq!(random_string(0, "hex").len(), 1);
        assert_eq!(random_string(5000, "hex").len(), 4096);
        let upper = random_string(16, "upper");
        assert!(upper.chars().all(|c| c.is_ascii_uppercase()));
    }

    #[test]
    fn random_int_handles_degenerate_range() {
        assert_eq!(random_int(3, 3), 3);
        assert_eq!(random_int(5, 2), 5);
    }
}
