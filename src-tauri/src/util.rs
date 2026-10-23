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

/// 简单模板渲染：支持 {{var}} 与少量内置函数
pub fn render(input: &str, vars: &std::collections::HashMap<String, String>) -> String {
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

fn resolve_expr(expr: &str, vars: &std::collections::HashMap<String, String>) -> String {
    match expr {
        "timestamp" => return now_ms().to_string(),
        "uuid" => return uuid::Uuid::new_v4().to_string(),
        _ => {}
    }

    if let Some(inner) = expr.strip_prefix("randomInt(").and_then(|s| s.strip_suffix(')')) {
        let parts: Vec<&str> = inner.split(',').map(|s| s.trim()).collect();
        if parts.len() == 2 {
            if let (Ok(min), Ok(max)) = (parts[0].parse::<i64>(), parts[1].parse::<i64>()) {
                if max >= min {
                    let span = (max - min + 1) as u64;
                    let seed = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map(|d| d.subsec_nanos() as u64)
                        .unwrap_or(1);
                    return (min + (seed % span) as i64).to_string();
                }
            }
        }
    }

    if let Some(inner) = expr.strip_prefix("randomString(").and_then(|s| s.strip_suffix(')')) {
        if let Ok(len) = inner.trim().parse::<usize>() {
            let raw = uuid::Uuid::new_v4().simple().to_string();
            return raw.chars().cycle().take(len.max(1)).collect();
        }
    }

    if let Some(value) = vars.get(expr) {
        return value.clone();
    }

    format!("{{{{{expr}}}}}")
}
