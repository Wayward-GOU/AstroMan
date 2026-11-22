//! EnvBundle 加解密
//!
//! 主从通过 Hello/HelloAck 交换 X25519 公钥，之后用 HKDF-SHA256 派生
//! 32 字节会话密钥，对 `{"vars":{...},"secrets":{...}}` 做 AES-256-GCM 加密。
//! 密钥只存在内存，断线即失效；明文不落盘。

use aes_gcm::aead::{Aead, Payload};
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;
use hkdf::Hkdf;
use sha2::{Digest, Sha256};
use x25519_dalek::{PublicKey, StaticSecret};

use super::protocol::{EnvBundle, ENV_BUNDLE_VERSION};

const HKDF_INFO: &[u8] = b"astroman-env-bundle-v1";

/// 生成会话密钥对，返回私钥与 base64 公钥
pub fn generate_keypair() -> (StaticSecret, String) {
    let secret = StaticSecret::random();
    let public = B64.encode(PublicKey::from(&secret).as_bytes());
    (secret, public)
}

/// 解析 base64 公钥（32 字节）
pub fn decode_public(raw: &str) -> Result<[u8; 32], String> {
    let bytes = B64
        .decode(raw.trim())
        .map_err(|error| format!("公钥格式错误：{error}"))?;
    bytes
        .try_into()
        .map_err(|_| "公钥长度必须为 32 字节".to_string())
}

/// 派生 AES-256-GCM 密钥
fn shared_key(secret: &StaticSecret, peer: &[u8; 32]) -> [u8; 32] {
    let peer = PublicKey::from(*peer);
    let shared = secret.diffie_hellman(&peer);
    let hkdf = Hkdf::<Sha256>::new(None, shared.as_bytes());
    let mut key = [0u8; 32];
    // 32 字节输出不会超过 HKDF-SHA256 的长度上限
    let _ = hkdf.expand(HKDF_INFO, &mut key);
    key
}

/// 明文摘要（hex），用于解密后校验
pub fn digest_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// 加密环境变量包；返回 (nonce, payload, digest)
pub fn encrypt_bundle(
    secret: &StaticSecret,
    peer: &[u8; 32],
    plaintext: &[u8],
) -> Result<(String, String, String), String> {
    let key = shared_key(secret, peer);
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|error| format!("初始化加密器失败：{error}"))?;
    let digest = digest_hex(plaintext);
    // nonce 由会话密钥与明文摘要派生：同一会话内不同明文得到不同 nonce
    let mut nonce_bytes = [0u8; 12];
    let mut hasher = Sha256::new();
    hasher.update(key);
    hasher.update(digest.as_bytes());
    nonce_bytes.copy_from_slice(&hasher.finalize()[..12]);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, Payload { msg: plaintext, aad: HKDF_INFO })
        .map_err(|_| "环境变量包加密失败".to_string())?;
    Ok((B64.encode(nonce_bytes), B64.encode(ciphertext), digest))
}

/// 解密环境变量包并校验摘要
pub fn decrypt_bundle(
    secret: &StaticSecret,
    peer: &[u8; 32],
    bundle: &EnvBundle,
) -> Result<Vec<u8>, String> {
    if bundle.version != ENV_BUNDLE_VERSION {
        return Err(format!("EnvBundle 版本不支持：{}", bundle.version));
    }
    let key = shared_key(secret, peer);
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|error| format!("初始化解密器失败：{error}"))?;
    let nonce_bytes = B64
        .decode(bundle.nonce.trim())
        .map_err(|error| format!("nonce 格式错误：{error}"))?;
    if nonce_bytes.len() != 12 {
        return Err("nonce 长度必须为 12 字节".to_string());
    }
    let ciphertext = B64
        .decode(bundle.payload.trim())
        .map_err(|error| format!("密文格式错误：{error}"))?;
    let plaintext = cipher
        .decrypt(
            Nonce::from_slice(&nonce_bytes),
            Payload { msg: &ciphertext, aad: HKDF_INFO },
        )
        .map_err(|_| "环境变量包解密失败：主从会话密钥不一致或数据被篡改".to_string())?;
    let actual = digest_hex(&plaintext);
    if actual != bundle.digest {
        return Err("环境变量包摘要校验失败".to_string());
    }
    Ok(plaintext)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundle_roundtrip_between_two_parties() {
        let (master_secret, master_public) = generate_keypair();
        let (slave_secret, slave_public) = generate_keypair();
        let master_peer = decode_public(&slave_public).unwrap();
        let slave_peer = decode_public(&master_public).unwrap();
        let plaintext = br#"{"vars":{"base_url":"http://127.0.0.1"},"secrets":{"TOKEN":"abc"}}"#;
        let (nonce, payload, digest) =
            encrypt_bundle(&master_secret, &master_peer, plaintext).unwrap();
        let bundle = EnvBundle {
            version: ENV_BUNDLE_VERSION,
            env_name: Some("测试".to_string()),
            nonce,
            payload,
            digest,
            secret_names: vec!["TOKEN".to_string()],
        };
        let decoded = decrypt_bundle(&slave_secret, &slave_peer, &bundle).unwrap();
        assert_eq!(decoded, plaintext);
    }

    #[test]
    fn tampered_bundle_is_rejected() {
        let (master_secret, _) = generate_keypair();
        let (slave_secret, slave_public) = generate_keypair();
        let peer = decode_public(&slave_public).unwrap();
        let (nonce, payload, digest) = encrypt_bundle(&master_secret, &peer, b"hello").unwrap();
        let bundle = EnvBundle {
            version: ENV_BUNDLE_VERSION,
            env_name: None,
            nonce,
            payload,
            digest: format!("{digest}0"),
            secret_names: Vec::new(),
        };
        assert!(decrypt_bundle(&slave_secret, &peer, &bundle).is_err());
    }
}
