//! G-06 加密异地备份：备份包加解密
//!
//! 设计要点：
//! - 口令由用户单独设置，**不复用环境变量主密钥**；Argon2id salt 写在包头，
//!   换机器 / 重装 / 主密钥轮换后，只要口令还在就能恢复；
//! - AES-256-GCM 分块流式加解密（默认 1 MB 一块），每块独立 nonce，
//!   适合几百 MB 的库文件又不必整包进内存；
//! - 包头（magic + JSON）作为 AAD 参与认证，任何篡改都会在解密时失败；
//! - 任何失败都会删除未完成的 `.part`，绝不留下半个备份。

use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::Path;

use aes_gcm::aead::{Aead, Payload};
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use argon2::{Algorithm, Argon2, Params, Version};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use serde::{Deserialize, Serialize};

pub const MAGIC: &[u8] = b"ASTROMANBK1\n";
pub const EXTENSION: &str = ".astroman.bak.enc";
const KEY_LEN: usize = 32;
const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 12;
const CHUNK_SIZE: usize = 1024 * 1024;
const MAX_HEADER: u64 = 64 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PackageHeader {
    version: u32,
    /// 原始文件名（仅用于提示，恢复时由用户指定目标名）
    original_name: String,
    created_at: i64,
    salt: String,
    chunk_size: usize,
}

/// 加密文件：`source` 通常是 `backup::create` 生成的 .db 快照
pub fn encrypt_file(source: &Path, target: &Path, passphrase: &str) -> Result<(), String> {
    let passphrase = checked_passphrase(passphrase)?;
    let salt = random_salt();
    let key = derive_key(&passphrase, &salt)?;

    let header = PackageHeader {
        version: 1,
        original_name: source
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| "astroman.db".to_string()),
        created_at: crate::util::now_ms(),
        salt: B64.encode(salt),
        chunk_size: CHUNK_SIZE,
    };
    let header_json = serde_json::to_vec(&header).map_err(crate::util::err)?;

    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("创建备份目录失败：{e}"))?;
    }
    let part = with_suffix(target, ".part");
    let _ = fs::remove_file(&part);

    let result = (|| -> Result<(), String> {
        let mut reader = BufReader::new(
            File::open(source).map_err(|e| format!("读取快照失败（{}）：{e}", source.display()))?,
        );
        let mut writer =
            BufWriter::new(File::create(&part).map_err(|e| format!("创建加密文件失败：{e}"))?);
        writer.write_all(MAGIC).map_err(io_err)?;
        writer.write_all(&header_json).map_err(io_err)?;
        writer.write_all(b"\n").map_err(io_err)?;

        let cipher = Aes256Gcm::new_from_slice(&key).map_err(crate::util::err)?;
        let mut buffer = vec![0u8; CHUNK_SIZE];
        loop {
            let read = reader.read(&mut buffer).map_err(io_err)?;
            if read == 0 {
                break;
            }
            let nonce = random_nonce();
            let ciphertext = cipher
                .encrypt(
                    Nonce::from_slice(&nonce),
                    Payload {
                        msg: &buffer[..read],
                        aad: &header_json,
                    },
                )
                .map_err(|_| "E_CRYPTO: 加密失败".to_string())?;
            writer.write_all(&nonce).map_err(io_err)?;
            let length =
                u32::try_from(ciphertext.len()).map_err(|_| "E_CRYPTO: 数据块过大".to_string())?;
            writer.write_all(&length.to_be_bytes()).map_err(io_err)?;
            writer.write_all(&ciphertext).map_err(io_err)?;
        }
        writer.flush().map_err(io_err)?;
        Ok(())
    })();

    if let Err(error) = result {
        let _ = fs::remove_file(&part);
        return Err(error);
    }
    fs::rename(&part, target).map_err(|e| format!("保存加密备份失败：{e}"))?;
    Ok(())
}

/// 解密文件；口令错误或数据被篡改返回 `E_CRYPTO`，且不会留下半成品
pub fn decrypt_file(source: &Path, target: &Path, passphrase: &str) -> Result<(), String> {
    let passphrase = checked_passphrase(passphrase)?;
    let (header, header_json, mut reader) = read_header(source)?;
    let salt = B64
        .decode(header.salt.as_bytes())
        .map_err(|_| "E_CRYPTO: 包头 salt 损坏".to_string())?;
    let key = derive_key(&passphrase, &salt)?;

    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("创建恢复目录失败：{e}"))?;
    }
    let part = with_suffix(target, ".part");
    let _ = fs::remove_file(&part);

    let result = (|| -> Result<(), String> {
        let cipher = Aes256Gcm::new_from_slice(&key).map_err(crate::util::err)?;
        let mut writer =
            BufWriter::new(File::create(&part).map_err(|e| format!("创建临时文件失败：{e}"))?);
        let chunk_limit = header.chunk_size.clamp(4096, 16 * 1024 * 1024);
        loop {
            let mut nonce = [0u8; NONCE_LEN];
            if !read_exact_or_eof(&mut reader, &mut nonce)? {
                break;
            }
            let mut length = [0u8; 4];
            reader
                .read_exact(&mut length)
                .map_err(|_| "E_CRYPTO: 备份文件被截断".to_string())?;
            let length = u32::from_be_bytes(length) as usize;
            if length > chunk_limit + 64 {
                return Err("E_CRYPTO: 备份分块长度异常".to_string());
            }
            let mut ciphertext = vec![0u8; length];
            reader
                .read_exact(&mut ciphertext)
                .map_err(|_| "E_CRYPTO: 备份文件被截断".to_string())?;
            let plain = cipher
                .decrypt(
                    Nonce::from_slice(&nonce),
                    Payload {
                        msg: &ciphertext,
                        aad: &header_json,
                    },
                )
                .map_err(|_| "E_CRYPTO: 解密失败（口令错误或备份被篡改）".to_string())?;
            writer.write_all(&plain).map_err(io_err)?;
        }
        writer.flush().map_err(io_err)?;
        Ok(())
    })();

    if let Err(error) = result {
        let _ = fs::remove_file(&part);
        return Err(error);
    }

    // 解出来的必须是 SQLite 库，否则拒绝覆盖本地库
    if !looks_like_sqlite(&part) {
        let _ = fs::remove_file(&part);
        return Err("E_CRYPTO: 解密结果不是有效的备份文件".to_string());
    }
    fs::rename(&part, target).map_err(|e| format!("写入恢复文件失败：{e}"))?;
    Ok(())
}

/// 只校验包头，不派生密钥（列目录 / 预览时使用）
pub fn is_encrypted(path: &Path) -> bool {
    let Ok(mut file) = File::open(path) else {
        return false;
    };
    let mut magic = vec![0u8; MAGIC.len()];
    file.read_exact(&mut magic).is_ok() && magic == MAGIC
}

fn read_header(source: &Path) -> Result<(PackageHeader, Vec<u8>, BufReader<File>), String> {
    let file =
        File::open(source).map_err(|e| format!("读取加密备份失败（{}）：{e}", source.display()))?;
    let mut reader = BufReader::new(file);
    let mut magic = vec![0u8; MAGIC.len()];
    reader
        .read_exact(&mut magic)
        .map_err(|_| "E_CRYPTO: 不是 AstroMan 加密备份".to_string())?;
    if magic != MAGIC {
        return Err("E_CRYPTO: 不是 AstroMan 加密备份".to_string());
    }
    let mut header_json = Vec::new();
    loop {
        let mut byte = [0u8; 1];
        reader
            .read_exact(&mut byte)
            .map_err(|_| "E_CRYPTO: 包头不完整".to_string())?;
        if byte[0] == b'\n' {
            break;
        }
        if header_json.len() as u64 >= MAX_HEADER {
            return Err("E_CRYPTO: 包头过长".to_string());
        }
        header_json.push(byte[0]);
    }
    let header: PackageHeader =
        serde_json::from_slice(&header_json).map_err(|_| "E_CRYPTO: 包头损坏".to_string())?;
    if header.version != 1 {
        return Err(format!("E_CRYPTO: 不支持的备份版本 {}", header.version));
    }
    Ok((header, header_json, reader))
}

fn checked_passphrase(passphrase: &str) -> Result<String, String> {
    let trimmed = passphrase.trim();
    if trimmed.chars().count() < 8 {
        return Err("口令至少 8 位（备份口令丢失将无法恢复，请妥善保存）".to_string());
    }
    Ok(trimmed.to_string())
}

/// Argon2id：19 MB / 3 轮 / 1 并行，与主密钥派生参数一致但 salt 独立
fn derive_key(passphrase: &str, salt: &[u8]) -> Result<[u8; KEY_LEN], String> {
    let params =
        Params::new(19 * 1024, 3, 1, Some(KEY_LEN)).map_err(|e| format!("KDF 参数无效：{e}"))?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut key = [0u8; KEY_LEN];
    argon
        .hash_password_into(passphrase.as_bytes(), salt, &mut key)
        .map_err(|e| format!("口令派生失败：{e}"))?;
    Ok(key)
}

fn random_salt() -> [u8; SALT_LEN] {
    let mut salt = [0u8; SALT_LEN];
    salt.copy_from_slice(uuid::Uuid::new_v4().as_bytes());
    salt
}

fn random_nonce() -> [u8; NONCE_LEN] {
    let raw = uuid::Uuid::new_v4().into_bytes();
    let mut nonce = [0u8; NONCE_LEN];
    nonce.copy_from_slice(&raw[..NONCE_LEN]);
    nonce
}

fn read_exact_or_eof(reader: &mut BufReader<File>, buffer: &mut [u8]) -> Result<bool, String> {
    match reader.read_exact(buffer) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => Ok(false),
        Err(error) => Err(format!("读取加密备份失败：{error}")),
    }
}

fn looks_like_sqlite(path: &Path) -> bool {
    let Ok(mut file) = File::open(path) else {
        return false;
    };
    let mut header = [0u8; 16];
    file.read_exact(&mut header).is_ok() && &header == b"SQLite format 3\0"
}

fn with_suffix(path: &Path, suffix: &str) -> std::path::PathBuf {
    let mut name = path
        .file_name()
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_else(|| "backup".to_string());
    name.push_str(suffix);
    path.with_file_name(name)
}

fn io_err(error: std::io::Error) -> String {
    format!("读写备份失败：{error}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 造一个「像 SQLite」的多分块文件：解密后的库头校验才会通过
    fn sample(path: &Path, size: usize) {
        let mut data = Vec::with_capacity(size);
        data.extend_from_slice(b"SQLite format 3\0");
        let mut seed = 7u64;
        while data.len() < size {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            data.push(seed as u8);
        }
        fs::write(path, data).unwrap();
    }

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let dir = crate::util::test_dir(&format!("backup-crypto-{tag}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn roundtrip_keeps_content_across_chunks() {
        let dir = temp_dir("roundtrip");
        let source = dir.join("astroman.db");
        // 跨两个分块（1 MB + 余量），验证分块循环
        sample(&source, CHUNK_SIZE + 4096);
        let target = dir.join("astroman.db.astroman.bak.enc");
        encrypt_file(&source, &target, "correct horse battery").unwrap();
        assert!(is_encrypted(&target));

        let restored = dir.join("restored.db");
        decrypt_file(&target, &restored, "correct horse battery").unwrap();
        assert_eq!(
            fs::read(&source).unwrap(),
            fs::read(&restored).unwrap(),
            "解密后内容应与原库一致"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn wrong_passphrase_leaves_no_output() {
        let dir = temp_dir("wrong-pass");
        let source = dir.join("astroman.db");
        sample(&source, 8192);
        let target = dir.join("astroman.db.astroman.bak.enc");
        encrypt_file(&source, &target, "right-passphrase").unwrap();

        let restored = dir.join("restored.db");
        let error = decrypt_file(&target, &restored, "wrong-passphrase").unwrap_err();
        assert!(error.starts_with("E_CRYPTO"), "实际：{error}");
        assert!(!restored.exists(), "口令错误不能留下半成品");
        assert!(!with_suffix(&restored, ".part").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn tampered_ciphertext_is_rejected() {
        let dir = temp_dir("tamper");
        let source = dir.join("astroman.db");
        sample(&source, 8192);
        let target = dir.join("astroman.db.astroman.bak.enc");
        encrypt_file(&source, &target, "right-passphrase").unwrap();

        // 翻转最后一字节：GCM 认证失败
        let mut data = fs::read(&target).unwrap();
        let last = data.len() - 1;
        data[last] ^= 0x01;
        fs::write(&target, data).unwrap();

        let restored = dir.join("restored.db");
        let error = decrypt_file(&target, &restored, "right-passphrase").unwrap_err();
        assert!(error.starts_with("E_CRYPTO"), "实际：{error}");
        assert!(!restored.exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn salt_is_unique_and_header_has_no_passphrase() {
        let dir = temp_dir("salt");
        let source = dir.join("astroman.db");
        sample(&source, 4096);
        let first = dir.join(format!("first{EXTENSION}"));
        let second = dir.join(format!("second{EXTENSION}"));
        encrypt_file(&source, &first, "top-secret-pass").unwrap();
        encrypt_file(&source, &second, "top-secret-pass").unwrap();

        let header_one = read_header(&first).unwrap().0;
        let header_two = read_header(&second).unwrap().0;
        assert_ne!(header_one.salt, header_two.salt, "每次备份 salt 必须独立");

        let raw = fs::read(&first).unwrap();
        let header_end = raw
            .iter()
            .enumerate()
            .skip(MAGIC.len())
            .find(|(_, byte)| **byte == b'\n')
            .map(|(index, _)| index)
            .unwrap();
        let header_text = String::from_utf8_lossy(&raw[..=header_end]);
        assert!(!header_text.contains("top-secret-pass"), "包头不能出现口令");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn short_passphrase_and_plain_file_are_rejected() {
        let dir = temp_dir("guard");
        let source = dir.join("astroman.db");
        sample(&source, 4096);
        let error = encrypt_file(&source, &dir.join("x.enc"), "short").unwrap_err();
        assert!(error.contains("至少 8 位"), "实际：{error}");

        let plain = dir.join("plain.enc");
        fs::write(&plain, b"not an encrypted backup").unwrap();
        assert!(!is_encrypted(&plain));
        let error = decrypt_file(&plain, &dir.join("y.db"), "long-enough-pass").unwrap_err();
        assert_eq!(error, "E_CRYPTO: 不是 AstroMan 加密备份");
        let _ = fs::remove_dir_all(&dir);
    }
}
