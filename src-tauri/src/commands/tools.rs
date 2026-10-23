use hmac::{Hmac, Mac};
use md5::Md5;
use sha1::Sha1;
use sha2::{Digest, Sha256, Sha512};

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
