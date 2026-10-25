use std::time::Duration;

use crate::state::AppState;
use hmac::{Hmac, Mac};
use md5::Md5;
use sha1::Sha1;
use sha2::{Digest, Sha256, Sha512};
use tauri::State;

fn to_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

#[tauri::command]
pub fn tool_hash(text: String, algorithm: String) -> Result<String, String> {
    let algo = algorithm.to_lowercase();
    let digest = match algo.as_str() {
        "md5" => to_hex(&Md5::digest(text.as_bytes())),
        "sha1" | "sha-1" => to_hex(&Sha1::digest(text.as_bytes())),
        "sha256" | "sha-256" => to_hex(&Sha256::digest(text.as_bytes())),
        "sha512" | "sha-512" => to_hex(&Sha512::digest(text.as_bytes())),
        _ => return Err(format!("不支持的哈希算法：{algorithm}")),
    };
    Ok(digest)
}

/// 查询 IP 归属地信息（数据来源 ip9.com.cn）。
/// 不传 `ip` 时返回本机公网 IP，传入时查询指定 IP。
#[tauri::command]
pub async fn ip_lookup(
    state: State<'_, AppState>,
    ip: Option<String>,
) -> Result<serde_json::Value, String> {
    let target = ip
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    if let Some(value) = target.as_deref() {
        value
            .parse::<std::net::IpAddr>()
            .map_err(|_| format!("IP 地址格式不正确：{value}"))?;
    }

    let mut request = state
        .client(false)
        .get("https://ip9.com.cn/get")
        .header(reqwest::header::ACCEPT, "application/json, text/plain, */*")
        .timeout(Duration::from_secs(15));
    if let Some(value) = target.as_deref() {
        request = request.query(&[("ip", value)]);
    }

    let response = request
        .send()
        .await
        .map_err(|e| format!("IP 查询请求失败：{e}"))?;

    let status = response.status();
    if !status.is_success() {
        return Err(format!("IP 查询失败：HTTP {}", status.as_u16()));
    }

    let text = response
        .text()
        .await
        .map_err(|e| format!("读取 IP 查询结果失败：{e}"))?;
    let payload: serde_json::Value =
        serde_json::from_str(&text).map_err(|_| "IP 查询接口返回了无法解析的内容".to_string())?;

    let ret = payload
        .get("ret")
        .and_then(|value| value.as_i64())
        .unwrap_or_default();
    if ret != 200 {
        let reason = payload
            .get("msg")
            .and_then(|value| value.as_str())
            .unwrap_or("接口返回异常");
        return Err(format!("IP 查询失败：{reason}"));
    }

    payload
        .get("data")
        .cloned()
        .ok_or_else(|| "IP 查询接口未返回数据".to_string())
}

#[tauri::command]
pub fn tool_hmac(text: String, key: String, algorithm: String) -> Result<String, String> {
    let algo = algorithm.to_lowercase();
    let invalid = || "HMAC 密钥无效".to_string();
    let digest = match algo.as_str() {
        "md5" => {
            let mut mac = Hmac::<Md5>::new_from_slice(key.as_bytes()).map_err(|_| invalid())?;
            mac.update(text.as_bytes());
            to_hex(&mac.finalize().into_bytes())
        }
        "sha1" | "sha-1" => {
            let mut mac = Hmac::<Sha1>::new_from_slice(key.as_bytes()).map_err(|_| invalid())?;
            mac.update(text.as_bytes());
            to_hex(&mac.finalize().into_bytes())
        }
        "sha256" | "sha-256" => {
            let mut mac = Hmac::<Sha256>::new_from_slice(key.as_bytes()).map_err(|_| invalid())?;
            mac.update(text.as_bytes());
            to_hex(&mac.finalize().into_bytes())
        }
        "sha512" | "sha-512" => {
            let mut mac = Hmac::<Sha512>::new_from_slice(key.as_bytes()).map_err(|_| invalid())?;
            mac.update(text.as_bytes());
            to_hex(&mac.finalize().into_bytes())
        }
        _ => return Err(format!("不支持的 HMAC 算法：{algorithm}")),
    };
    Ok(digest)
}
