use serde::Serialize;

use crate::models::KeyValueItem;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CurlParsed {
    pub name: String,
    pub method: String,
    pub url: String,
    pub headers: Vec<KeyValueItem>,
    pub params: Vec<KeyValueItem>,
    pub body_type: String,
    pub body: String,
    pub warnings: Vec<String>,
}

fn looks_like_json(value: &str) -> bool {
    let trimmed = value.trim_start();
    trimmed.starts_with('{') || trimmed.starts_with('[')
}

fn base64_encode(input: &[u8]) -> String {
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        out.push(TABLE[((b0 >> 2) & 0x3F) as usize] as char);
        out.push(TABLE[(((b0 & 0x03) << 4) | (b1 >> 4)) as usize] as char);
        if chunk.len() > 1 {
            out.push(TABLE[(((b1 & 0x0F) << 2) | (b2 >> 6)) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(TABLE[(b2 & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

/// 去掉包裹 URL 的 Markdown 反引号，例如 '`https://example.com`'
fn clean_url(raw: &str) -> String {
    raw.trim().trim_matches('`').trim().to_string()
}

/// curl --data-urlencode 的编码规则：仅保留 unreserved 字符，空格编码为 %20
fn urlencode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(byte as char)
            }
            b' ' => out.push_str("%20"),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn tokenize(input: &str) -> Vec<String> {
    let mut tokens: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut has_token = false;
    let mut quote: Option<char> = None;
    let mut chars = input.chars().peekable();

    while let Some(c) = chars.next() {
        match quote {
            Some(q) => {
                if c == q {
                    quote = None;
                } else if c == '\\' && q == '"' {
                    if let Some(&next) = chars.peek() {
                        if next == '"' || next == '\\' || next == '$' || next == '`' {
                            chars.next();
                            current.push(next);
                        } else {
                            current.push(c);
                        }
                    } else {
                        current.push(c);
                    }
                } else {
                    current.push(c);
                }
            }
            None => match c {
                '\'' | '"' => {
                    quote = Some(c);
                    has_token = true;
                }
                '\\' => match chars.peek() {
                    Some(&'\n') => {
                        chars.next();
                    }
                    Some(&'\r') => {
                        chars.next();
                        if chars.peek() == Some(&'\n') {
                            chars.next();
                        }
                    }
                    Some(&next) => {
                        chars.next();
                        current.push(next);
                        has_token = true;
                    }
                    None => current.push(c),
                },
                _ if c.is_whitespace() => {
                    if has_token {
                        tokens.push(std::mem::take(&mut current));
                        has_token = false;
                    }
                }
                _ => {
                    current.push(c);
                    has_token = true;
                }
            },
        }
    }

    if has_token {
        tokens.push(current);
    }
    tokens
}

fn normalize(command: &str) -> String {
    command
        .replace("^\r\n", " ")
        .replace("^\n", " ")
        .replace("`\r\n", " ")
        .replace("`\n", " ")
        .replace("\\\r\n", " ")
        .replace("\\\n", " ")
        .trim()
        .to_string()
}

fn header_item(raw: &str) -> Option<KeyValueItem> {
    let (key, value) = raw.split_once(':')?;
    let key = key.trim();
    if key.is_empty() || key.starts_with(':') {
        return None;
    }
    Some(KeyValueItem {
        key: key.to_string(),
        value: value.trim().to_string(),
        enabled: true,
    })
}

fn collect_data(
    value: &str,
    pairs: &mut Vec<(String, String, bool)>,
    warnings: &mut Vec<String>,
) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    if looks_like_json(trimmed) {
        return Some(value.to_string());
    }
    if trimmed.contains('=') && !trimmed.contains('\n') {
        for chunk in trimmed.split('&') {
            if chunk.is_empty() {
                continue;
            }
            match chunk.split_once('=') {
                Some((k, v)) => pairs.push((k.to_string(), v.to_string(), false)),
                None => pairs.push((chunk.to_string(), String::new(), false)),
            }
        }
        return None;
    }
    if trimmed.contains('\n') {
        warnings.push("请求体包含换行，已按原始文本保留".to_string());
    }
    Some(value.to_string())
}

fn suggest_name(method: &str, url: &str) -> String {
    let without_query = url.split(['?', '#']).next().unwrap_or(url);
    let rest = without_query
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(without_query);
    let path = rest.find('/').map(|i| &rest[i..]).unwrap_or("/");
    let name = format!("{method} {path}");
    if name.chars().count() > 44 {
        name.chars().take(44).collect::<String>() + "…"
    } else {
        name
    }
}

#[tauri::command]
pub fn curl_parse(text: String) -> Result<CurlParsed, String> {
    let command = normalize(&text);
    if command.is_empty() {
        return Err("请粘贴 cURL 命令".to_string());
    }

    let tokens = tokenize(&command);
    if tokens.is_empty() {
        return Err("请粘贴 cURL 命令".to_string());
    }
    let head = tokens[0].to_lowercase();
    if !head.starts_with("curl") {
        return Err("命令需要以 curl 开头".to_string());
    }

    let mut method: Option<String> = None;
    let mut url: Option<String> = None;
    let mut headers: Vec<KeyValueItem> = Vec::new();
    // 第三项标记该字段是否由 --data-urlencode 提供（发送前需要 URL 编码）
    let mut pairs: Vec<(String, String, bool)> = Vec::new();
    let mut raw_body: Option<String> = None;
    let mut json_mode = false;
    let mut get_mode = false;
    let mut form_mode = false;
    let mut urlencode_mode = false;
    let mut warnings: Vec<String> = Vec::new();
    let mut user: Option<String> = None;
    let mut cookie: Option<String> = None;
    let mut user_agent: Option<String> = None;

    let mut iter = tokens.into_iter().skip(1).peekable();
    while let Some(token) = iter.next() {
        match token.as_str() {
            "-X" | "--request" => {
                if let Some(value) = iter.next() {
                    method = Some(value.trim().to_uppercase());
                }
            }
            "-H" | "--header" => {
                if let Some(value) = iter.next() {
                    match header_item(&value) {
                        Some(item) => headers.push(item),
                        None => warnings.push(format!("已忽略无法识别的 Header：{value}")),
                    }
                }
            }
            "-d" | "--data" | "--data-raw" | "--data-ascii" | "--data-binary"
            | "--data-urlencode" => {
                if let Some(value) = iter.next() {
                    if token == "--data-urlencode" {
                        urlencode_mode = true;
                        match value.split_once('=') {
                            Some((k, v)) => pairs.push((k.to_string(), v.to_string(), true)),
                            None => pairs.push((value.clone(), String::new(), true)),
                        }
                    } else if let Some(body) = collect_data(&value, &mut pairs, &mut warnings) {
                        raw_body = Some(body);
                    }
                }
            }
            "--json" => {
                json_mode = true;
                if let Some(value) = iter.next() {
                    raw_body = Some(value);
                }
            }
            "-F" | "--form" => {
                form_mode = true;
                if let Some(value) = iter.next() {
                    match value.split_once('=') {
                        Some((k, v)) => {
                            if v.starts_with('@') || v.starts_with('<') {
                                warnings.push(format!(
                                    "已忽略文件字段 {k}（暂不支持 multipart 文件上传）"
                                ));
                            } else {
                                pairs.push((k.to_string(), v.to_string(), false));
                            }
                        }
                        None => pairs.push((value.clone(), String::new(), false)),
                    }
                }
            }
            "-G" | "--get" => get_mode = true,
            "-I" | "--head" => method = Some("HEAD".to_string()),
            "-u" | "--user" => user = iter.next(),
            "-b" | "--cookie" => cookie = iter.next(),
            "-A" | "--user-agent" => user_agent = iter.next(),
            "--url" => url = iter.next().map(|value| clean_url(&value)),
            "-f"
            | "--fail"
            | "--fail-with-body"
            | "-s"
            | "--silent"
            | "-S"
            | "--show-error"
            | "-sS"
            | "-L"
            | "--location"
            | "-k"
            | "--insecure"
            | "-i"
            | "--include"
            | "-v"
            | "--verbose"
            | "--compressed"
            | "--no-buffer"
            | "-N"
            | "--http1.1"
            | "--http2"
            | "--raw"
            | "--path-as-is"
            | "--no-progress-meter"
            | "--globoff"
            | "-g" => {}
            other => {
                if other.starts_with('-') {
                    warnings.push(format!("已忽略参数 {other}"));
                    if let Some(next) = iter.peek() {
                        if !next.starts_with('-') {
                            iter.next();
                        }
                    }
                } else if url.is_none() {
                    url = Some(clean_url(other));
                }
            }
        }
    }

    let url = url.ok_or_else(|| "没有在命令中找到 URL".to_string())?;

    if let Some(agent) = user_agent {
        if !headers
            .iter()
            .any(|h| h.key.eq_ignore_ascii_case("user-agent"))
        {
            headers.push(KeyValueItem {
                key: "User-Agent".to_string(),
                value: agent,
                enabled: true,
            });
        }
    }

    if let Some(cookie) = cookie {
        if !headers.iter().any(|h| h.key.eq_ignore_ascii_case("cookie")) {
            headers.push(KeyValueItem {
                key: "Cookie".to_string(),
                value: cookie,
                enabled: true,
            });
        }
    }

    if let Some(user) = user {
        if !headers
            .iter()
            .any(|h| h.key.eq_ignore_ascii_case("authorization"))
        {
            headers.push(KeyValueItem {
                key: "Authorization".to_string(),
                value: format!("Basic {}", base64_encode(user.as_bytes())),
                enabled: true,
            });
        }
    }

    let mut params: Vec<KeyValueItem> = Vec::new();
    let mut body = String::new();

    if get_mode {
        if raw_body.is_some() {
            warnings.push("-G 模式下的 -d 内容无法直接转换为查询参数，已放入 Body".to_string());
        }
        params = pairs
            .drain(..)
            .map(|(key, value, _)| KeyValueItem {
                key,
                value,
                enabled: true,
            })
            .collect();
    } else {
        if !pairs.is_empty() {
            let joined = pairs
                .iter()
                .map(|(k, v, encode)| {
                    if *encode {
                        format!("{k}={}", urlencode(v))
                    } else {
                        format!("{k}={v}")
                    }
                })
                .collect::<Vec<_>>()
                .join("&");
            if let Some(existing) = raw_body {
                warnings.push("同时存在 -d 与表单字段，已合并到 Body".to_string());
                body = format!("{existing}&{joined}");
            } else {
                body = joined;
            }
        } else if let Some(existing) = raw_body {
            body = existing;
        }
    }

    let content_type = headers
        .iter()
        .find(|h| h.key.eq_ignore_ascii_case("content-type"))
        .map(|h| h.value.to_lowercase());

    // -G 或没有 body 都按 GET 处理，两者走的是同一个分支
    let method = method.unwrap_or_else(|| {
        if get_mode || body.is_empty() {
            "GET".to_string()
        } else {
            "POST".to_string()
        }
    });

    let body_type = if body.is_empty() {
        "none"
    } else if json_mode
        || looks_like_json(&body)
        || content_type
            .as_deref()
            .map(|v| v.contains("json"))
            .unwrap_or(false)
    {
        "json"
    } else if form_mode
        || urlencode_mode
        || content_type
            .as_deref()
            .map(|v| v.contains("x-www-form-urlencoded"))
            .unwrap_or(false)
    {
        "form"
    } else {
        "raw"
    };

    if json_mode
        && !headers
            .iter()
            .any(|h| h.key.eq_ignore_ascii_case("content-type"))
    {
        headers.push(KeyValueItem {
            key: "Content-Type".to_string(),
            value: "application/json".to_string(),
            enabled: true,
        });
    }

    if form_mode {
        warnings.push("multipart 表单已转为 application/x-www-form-urlencoded".to_string());
    }

    let name = suggest_name(&method, &url);

    Ok(CurlParsed {
        name,
        method,
        url,
        headers,
        params,
        body_type: body_type.to_string(),
        body,
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imports_urlencoded_form_and_strips_backtick_wrapped_url() {
        let command = "curl --location --request POST '`https://winesmpxc1.cofco.com/uat/login/base/user/apilogin`' \\\n--data-urlencode 'clientId=api' \\\n--data-urlencode 'password=y>arg12312'";
        let parsed = curl_parse(command.to_string()).expect("应能解析");
        assert_eq!(parsed.method, "POST");
        assert_eq!(
            parsed.url,
            "https://winesmpxc1.cofco.com/uat/login/base/user/apilogin"
        );
        assert_eq!(parsed.body_type, "form");
        assert_eq!(parsed.body, "clientId=api&password=y%3Earg12312");
        assert!(parsed.params.is_empty());
    }

    #[test]
    fn get_mode_keeps_data_urlencode_values_raw_for_query_params() {
        let command = "curl -G 'https://api.example.com/search' --data-urlencode 'q=a b&c'";
        let parsed = curl_parse(command.to_string()).expect("应能解析");
        assert_eq!(parsed.method, "GET");
        assert_eq!(parsed.body_type, "none");
        assert_eq!(parsed.params.len(), 1);
        assert_eq!(parsed.params[0].key, "q");
        // -G 的编码交给发送侧统一处理，这里必须保留原文，避免双重编码
        assert_eq!(parsed.params[0].value, "a b&c");
    }
}
