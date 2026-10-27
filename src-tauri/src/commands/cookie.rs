use crate::models::CookieItem;
use crate::state::AppState;
use crate::util::{err, now_ms};
use rusqlite::{params, Connection};
use tauri::State;

/// 解析后的 Cookie
struct ParsedCookie {
    domain: String,
    path: String,
    name: String,
    value: String,
    secure: bool,
    http_only: bool,
    expires_at: Option<i64>,
}

/// 解析单条 Set-Cookie，失败时返回 None（不影响请求结果）
fn parse_set_cookie(raw: &str, host: &str) -> Option<ParsedCookie> {
    let mut parts = raw.split(';');
    let pair = parts.next()?.trim();
    let (name, value) = pair.split_once('=')?;
    let name = name.trim();
    if name.is_empty() {
        return None;
    }

    let mut domain = host.to_string();
    let mut path = default_path(name, host);
    let mut secure = false;
    let mut http_only = false;
    let mut expires_at = None;
    let mut max_age: Option<i64> = None;

    for attr in parts {
        let attr = attr.trim();
        if attr.is_empty() {
            continue;
        }
        let (key, attr_value) = match attr.split_once('=') {
            Some((k, v)) => (k.trim(), v.trim()),
            None => (attr, ""),
        };
        match key.to_ascii_lowercase().as_str() {
            "domain" => {
                let value = attr_value.trim_start_matches('.').to_ascii_lowercase();
                if !value.is_empty() {
                    domain = value;
                }
            }
            "path" => {
                if attr_value.starts_with('/') {
                    path = attr_value.to_string();
                }
            }
            "secure" => secure = true,
            "httponly" => http_only = true,
            "max-age" => {
                if let Ok(seconds) = attr_value.parse::<i64>() {
                    max_age = Some(if seconds <= 0 { 0 } else { now_ms() + seconds * 1000 });
                }
            }
            "expires" => {
                if let Ok(time) = chrono::DateTime::parse_from_rfc2822(attr_value) {
                    expires_at = Some(time.timestamp_millis());
                } else if let Ok(time) = chrono::DateTime::parse_from_rfc3339(attr_value) {
                    expires_at = Some(time.timestamp_millis());
                }
            }
            _ => {}
        }
    }

    Some(ParsedCookie {
        // 未指定 Domain 时只对当前主机生效
        domain: if domain.is_empty() { host.to_string() } else { domain },
        path,
        name: name.to_string(),
        value: value.to_string(),
        secure,
        http_only,
        expires_at: max_age.or(expires_at),
    })
}

/// 按 RFC 6265 的默认路径规则推导 Cookie 路径
fn default_path(_name: &str, _host: &str) -> String {
    "/".to_string()
}

fn host_matches(host: &str, domain: &str) -> bool {
    let host = host.to_ascii_lowercase();
    let domain = domain.trim_start_matches('.').to_ascii_lowercase();
    host == domain || host.ends_with(&format!(".{domain}"))
}

fn path_matches(request_path: &str, cookie_path: &str) -> bool {
    if cookie_path == "/" || request_path == cookie_path {
        return true;
    }
    request_path.starts_with(cookie_path)
}

/// 为一次请求挑选需要携带的 Cookie，返回 `name=value` 列表
pub fn cookies_for_url(conn: &Connection, url: &reqwest::Url) -> Vec<String> {
    let Some(host) = url.host_str() else {
        return Vec::new();
    };
    let request_path = url.path();
    let secure_request = url.scheme() == "https";
    let now = now_ms();

    let mut stmt = match conn.prepare("SELECT domain, path, name, value, secure, expires_at FROM cookies") {
        Ok(stmt) => stmt,
        Err(_) => return Vec::new(),
    };
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, i64>(4).unwrap_or(0) != 0,
            row.get::<_, Option<i64>>(5)?,
        ))
    });

    let mut cookies = Vec::new();
    let mut expired = Vec::new();
    if let Ok(rows) = rows {
        for row in rows.flatten() {
            let (domain, path, name, value, secure, expires_at) = row;
            if let Some(at) = expires_at {
                if at <= now {
                    expired.push((domain.clone(), path.clone(), name.clone()));
                    continue;
                }
            }
            if !host_matches(host, &domain) {
                continue;
            }
            if !path_matches(request_path, &path) {
                continue;
            }
            if secure && !secure_request {
                continue;
            }
            cookies.push(format!("{name}={value}"));
        }
    }

    for (domain, path, name) in expired {
        let _ = conn.execute(
            "DELETE FROM cookies WHERE domain = ?1 AND path = ?2 AND name = ?3",
            params![domain, path, name],
        );
    }

    cookies
}

/// 将响应中的 Set-Cookie 写入本机 Cookie 库
pub fn store_response_cookies(conn: &Connection, url: &reqwest::Url, raw_cookies: &[String]) {
    let Some(host) = url.host_str() else {
        return;
    };
    let now = now_ms();
    for raw in raw_cookies {
        let Some(cookie) = parse_set_cookie(raw, host) else {
            continue;
        };
        let is_expired = cookie.expires_at.map(|at| at <= now).unwrap_or(false);
        if is_expired {
            let _ = conn.execute(
                "DELETE FROM cookies WHERE domain = ?1 AND path = ?2 AND name = ?3",
                params![cookie.domain, cookie.path, cookie.name],
            );
            continue;
        }
        let _ = conn.execute(
            "INSERT INTO cookies
                (domain, path, name, value, secure, http_only, expires_at, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)
             ON CONFLICT(domain, path, name) DO UPDATE SET
                value = excluded.value,
                secure = excluded.secure,
                http_only = excluded.http_only,
                expires_at = excluded.expires_at,
                updated_at = excluded.updated_at",
            params![
                cookie.domain,
                cookie.path,
                cookie.name,
                cookie.value,
                cookie.secure as i64,
                cookie.http_only as i64,
                cookie.expires_at,
                now
            ],
        );
    }
}

#[tauri::command]
pub fn cookie_list(state: State<'_, AppState>) -> Result<Vec<CookieItem>, String> {
    let conn = state.pool.get().map_err(err)?;
    let mut stmt = conn
        .prepare(
            "SELECT id, domain, path, name, value, secure, http_only, expires_at, updated_at
             FROM cookies ORDER BY domain ASC, path ASC, name ASC",
        )
        .map_err(err)?;
    let rows = stmt
        .query_map([], |row| {
            Ok(CookieItem {
                id: row.get(0)?,
                domain: row.get(1)?,
                path: row.get(2)?,
                name: row.get(3)?,
                value: row.get(4)?,
                secure: row.get::<_, i64>(5).unwrap_or(0) != 0,
                http_only: row.get::<_, i64>(6).unwrap_or(0) != 0,
                expires_at: row.get(7)?,
                updated_at: row.get(8)?,
            })
        })
        .map_err(err)?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(err)
}

#[tauri::command]
pub fn cookie_save(
    state: State<'_, AppState>,
    domain: String,
    path: Option<String>,
    name: String,
    value: String,
    expires_at: Option<i64>,
) -> Result<(), String> {
    let domain = domain.trim().trim_start_matches('.').to_ascii_lowercase();
    let name = name.trim().to_string();
    if domain.is_empty() {
        return Err("Cookie 域名不能为空".to_string());
    }
    if name.is_empty() {
        return Err("Cookie 名称不能为空".to_string());
    }
    let path = path.unwrap_or_else(|| "/".to_string());
    let path = if path.starts_with('/') { path } else { format!("/{path}") };
    let now = now_ms();
    let conn = state.pool.get().map_err(err)?;
    conn.execute(
        "INSERT INTO cookies
            (domain, path, name, value, expires_at, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)
         ON CONFLICT(domain, path, name) DO UPDATE SET
            value = excluded.value,
            expires_at = excluded.expires_at,
            updated_at = excluded.updated_at",
        params![domain, path, name, value, expires_at, now],
    )
    .map_err(err)?;
    Ok(())
}

#[tauri::command]
pub fn cookie_delete(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    let conn = state.pool.get().map_err(err)?;
    conn.execute("DELETE FROM cookies WHERE id = ?1", params![id])
        .map_err(err)?;
    Ok(())
}

#[tauri::command]
pub fn cookie_clear(state: State<'_, AppState>, domain: Option<String>) -> Result<u32, String> {
    let conn = state.pool.get().map_err(err)?;
    let removed = match domain {
        Some(domain) if !domain.trim().is_empty() => conn.execute(
            "DELETE FROM cookies WHERE domain = ?1",
            params![domain.trim().trim_start_matches('.').to_ascii_lowercase()],
        ),
        _ => conn.execute("DELETE FROM cookies", []),
    }
    .map_err(err)?;
    Ok(removed as u32)
}
