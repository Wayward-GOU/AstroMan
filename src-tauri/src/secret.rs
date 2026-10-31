//! 主密钥与字段级加密（G-02）
//!
//! 现状问题（见技术方案 4.2）：环境变量的 `is_secret` 只控制前端输入框变星号，
//! 值仍然明文入库；AI Key 用 `SHA256("AstroMan::ai::provider::v1")` 做密钥，
//! 等于静态混淆——拿到库的人自己算一次就能解开。
//!
//! 本模块做三件事：
//! 1. 主密钥：优先进系统钥匙串（Windows 凭据管理器），可用口令模式（Argon2id，不落盘），
//!    最后才降级到文件——并且明确告诉用户「文件密钥与密文同目录，等于无加密」。
//! 2. 字段加密：`enc:v1:<base64(nonce12 || ciphertext)>`，AES-256-GCM，
//!    AAD 绑定「表名:列名」，把密文搬到别的列会直接解密失败。
//! 3. 迁移：把历史明文值就地加密，重复执行幂等。
//!
//! 全局单例是为了避免把 `SecretStore` 穿过几十个既有调用点；
//! 单元测试用 `SecretStore::from_key` 构造独立实例，互不影响。

use std::path::{Path, PathBuf};

use aes_gcm::aead::{Aead, Payload};
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use argon2::{Algorithm, Argon2, Params, Version};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::state::DbPool;
use crate::util::err;

/// 密文前缀，用于识别「已加密」与「历史明文」
pub const ENC_PREFIX: &str = "enc:v1:";
/// 前端回显掩码（与 env_list / ai_provider_list 一致）
pub const MASK: &str = "••••••";

const KEY_LEN: usize = 32;
const KEYRING_TARGET: &str = "AstroMan/master-key";
const KEYRING_USER: &str = "master-key";
const KEY_CHECK_AAD: &str = "app_meta:key_check";
const KEY_CHECK_TEXT: &str = "astroman-master-key-check";

/// 环境变量值的加密上下文
pub const AAD_ENV: &str = "env_variables:value";
/// AI Key 的加密上下文
pub const AAD_AI: &str = "ai_providers:api_key";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KeySource {
    /// 系统钥匙串（Windows 凭据管理器 / macOS 钥匙串）
    Keyring,
    /// 数据目录内的密钥文件：与密文同目录，只防误读，不构成安全边界
    File,
    /// 用户口令派生（Argon2id），不落盘任何密钥材料
    Passphrase,
}

impl KeySource {
    pub fn as_str(self) -> &'static str {
        match self {
            KeySource::Keyring => "keyring",
            KeySource::File => "file",
            KeySource::Passphrase => "passphrase",
        }
    }

    #[allow(dead_code)]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "keyring" => Some(KeySource::Keyring),
            "file" => Some(KeySource::File),
            "passphrase" => Some(KeySource::Passphrase),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            KeySource::Keyring => "系统钥匙串",
            KeySource::File => "本地密钥文件（降级）",
            KeySource::Passphrase => "口令模式",
        }
    }
}

/// 设置页展示的密钥状态
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretStatus {
    pub source: String,
    pub source_label: String,
    pub locked: bool,
    pub locked_reason: Option<String>,
    /// 当前平台是否成功访问过系统钥匙串
    pub keyring_available: bool,
    /// 是否处于「等同无加密」的降级模式
    pub degraded: bool,
    pub encrypted_env_count: i64,
    pub encrypted_ai_count: i64,
    pub key_created_at: Option<i64>,
}

struct Inner {
    key: Option<[u8; KEY_LEN]>,
    source: KeySource,
    locked: Option<String>,
    keyring_available: bool,
}

/// 主密钥持有者：加密只在这里发生，其它模块只拿到密文/明文
pub struct SecretStore {
    #[allow(dead_code)]
    data_dir: PathBuf,
    inner: parking_lot::RwLock<Inner>,
}

impl SecretStore {
    /// 启动时初始化：读钥匙串 / 读文件 / 等待口令解锁；首次启动生成主密钥
    pub fn bootstrap(data_dir: &Path, pool: &DbPool) -> Result<Self, String> {
        let conn = pool.get().map_err(err)?;
        let source_raw = crate::db::meta_get(&conn, "key_source");
        drop(conn);

        let (key, source, locked, keyring_available) = match source_raw.as_deref() {
            Some("keyring") => match keyring_read() {
                Ok(key) => (Some(key), KeySource::Keyring, None, true),
                Err(reason) => {
                    log::warn!("读取系统钥匙串失败：{reason}");
                    (
                        None,
                        KeySource::Keyring,
                        Some(format!(
                            "E_KEYRING: 未能从系统钥匙串读取主密钥（{reason}）。\
可在「设置 → 安全」导入密钥包恢复，或改用口令模式重新设置"
                        )),
                        false,
                    )
                }
            },
            Some("file") => match read_file_key(data_dir) {
                Ok(key) => (Some(key), KeySource::File, None, false),
                Err(reason) => (
                    None,
                    KeySource::File,
                    Some(format!(
                        "E_KEYFILE: 密钥文件不可读（{reason}），请在「设置 → 安全」导入密钥包"
                    )),
                    false,
                ),
            },
            Some("passphrase") => (
                None,
                KeySource::Passphrase,
                Some(
                    "E_LOCKED: 当前为口令模式，请输入口令解锁密钥后才能读取已保存的密钥值"
                        .to_string(),
                ),
                false,
            ),
            // 首次启动（或 v1.0.7/1.0.8 老库）：生成新主密钥
            _ => {
                let key = random_key();
                let (source, keyring_available) = match keyring_write(&key) {
                    Ok(()) => (KeySource::Keyring, true),
                    Err(reason) => {
                        log::warn!("系统钥匙串不可用（{reason}），降级为本地密钥文件");
                        write_file_key(data_dir, &key)?;
                        (KeySource::File, false)
                    }
                };
                let conn = pool.get().map_err(err)?;
                crate::db::meta_set(&conn, "crypto_version", "1")?;
                crate::db::meta_set(&conn, "key_source", source.as_str())?;
                crate::db::meta_set(&conn, "key_created_at", &crate::util::now_ms().to_string())?;
                drop(conn);
                (Some(key), source, None, keyring_available)
            }
        };

        let store = Self {
            data_dir: data_dir.to_path_buf(),
            inner: parking_lot::RwLock::new(Inner {
                key,
                source,
                locked,
                keyring_available,
            }),
        };

        // 未锁定时顺手把历史明文加密（幂等，失败不阻断启动，但会记日志）
        if !store.is_locked() {
            match store.encrypt_plaintext(pool) {
                Ok(0) => {}
                Ok(count) => log::info!("已加密 {count} 条历史明文密钥值"),
                Err(error) => log::warn!("历史明文加密未完成：{error}"),
            }
        }
        Ok(store)
    }

    /// 测试 / 单测用：直接给定密钥
    #[cfg(test)]
    pub fn from_key(data_dir: PathBuf, key: [u8; KEY_LEN], source: KeySource) -> Self {
        Self {
            data_dir,
            inner: parking_lot::RwLock::new(Inner {
                key: Some(key),
                source,
                locked: None,
                keyring_available: matches!(source, KeySource::Keyring),
            }),
        }
    }

    pub fn is_locked(&self) -> bool {
        self.inner.read().key.is_none()
    }

    pub fn source(&self) -> KeySource {
        self.inner.read().source
    }

    fn current_key(&self) -> Result<[u8; KEY_LEN], String> {
        let inner = self.inner.read();
        inner.key.ok_or_else(|| {
            inner.locked.clone().unwrap_or_else(|| {
                "E_LOCKED: 主密钥尚未解锁，请先在「设置 → 安全」中解锁".to_string()
            })
        })
    }

    /// 加密；未解锁时返回可判定错误而不是写入明文
    pub fn encrypt(&self, aad: &str, plain: &str) -> Result<String, String> {
        let key = self.current_key()?;
        encrypt_with(&key, aad, plain)
    }

    /// 解密：非密文（历史明文）原样返回；解密失败返回空串并记日志
    ///
    /// 调用方多为「渲染请求」路径，失败时返回空串比 panic / 500 安全：
    /// 用户会看到请求因缺少值而失败，同时日志里有明确原因。
    pub fn decrypt(&self, aad: &str, stored: &str) -> String {
        match self.decrypt_checked(aad, stored) {
            Ok(value) => value,
            Err(error) => {
                log::warn!("解密失败（{aad}）：{error}");
                String::new()
            }
        }
    }

    pub fn decrypt_checked(&self, aad: &str, stored: &str) -> Result<String, String> {
        if stored.is_empty() || !stored.starts_with(ENC_PREFIX) {
            return Ok(stored.to_string());
        }
        let key = self.current_key()?;
        decrypt_with(&key, aad, stored)
    }

    pub fn status(&self, pool: &DbPool) -> SecretStatus {
        let inner = self.inner.read();
        let (env_count, ai_count, created) = match pool.get() {
            Ok(conn) => (
                conn.query_row(
                    "SELECT COUNT(*) FROM env_variables WHERE is_secret = 1 AND value LIKE 'enc:v1:%'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap_or(0),
                conn.query_row(
                    "SELECT COUNT(*) FROM ai_providers WHERE api_key_enc LIKE 'enc:v1:%'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap_or(0),
                crate::db::meta_get(&conn, "key_created_at").and_then(|v| v.parse::<i64>().ok()),
            ),
            Err(_) => (0, 0, None),
        };
        SecretStatus {
            source: inner.source.as_str().to_string(),
            source_label: inner.source.label().to_string(),
            locked: inner.key.is_none(),
            locked_reason: inner.locked.clone(),
            keyring_available: inner.keyring_available,
            degraded: inner.source == KeySource::File,
            encrypted_env_count: env_count,
            encrypted_ai_count: ai_count,
            key_created_at: created,
        }
    }

    /// 把历史明文加密（幂等）：环境变量中 is_secret=1 的值 + AI Key
    pub fn encrypt_plaintext(&self, pool: &DbPool) -> Result<usize, String> {
        let key = self.current_key()?;
        let mut conn = pool.get().map_err(err)?;
        let tx = conn.transaction().map_err(err)?;
        let mut count = 0usize;

        {
            let mut stmt = tx
                .prepare(
                    "SELECT id, value FROM env_variables
                     WHERE is_secret = 1 AND value IS NOT NULL AND value <> ''",
                )
                .map_err(err)?;
            let rows = stmt
                .query_map([], |row| {
                    Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(err)?;
            let mut updates: Vec<(i64, String)> = Vec::new();
            for row in rows {
                let (id, value) = row.map_err(err)?;
                if value.starts_with(ENC_PREFIX) {
                    continue;
                }
                updates.push((id, encrypt_with(&key, AAD_ENV, &value)?));
            }
            drop(stmt);
            for (id, value) in updates {
                tx.execute(
                    "UPDATE env_variables SET value = ?1 WHERE id = ?2",
                    params![value, id],
                )
                .map_err(err)?;
                count += 1;
            }
        }

        {
            let mut stmt = tx
                .prepare("SELECT id, COALESCE(api_key_enc, '') FROM ai_providers")
                .map_err(err)?;
            let rows = stmt
                .query_map([], |row| {
                    Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(err)?;
            let mut updates: Vec<(i64, String)> = Vec::new();
            for row in rows {
                let (id, stored) = row.map_err(err)?;
                if stored.is_empty() || stored.starts_with(ENC_PREFIX) {
                    continue;
                }
                // v1:<b64> 是 v1.0.7 的静态混淆格式，先按老方式解开
                let plain = legacy_ai_decrypt(&stored);
                if plain.is_empty() {
                    continue;
                }
                updates.push((id, encrypt_with(&key, AAD_AI, &plain)?));
            }
            drop(stmt);
            for (id, value) in updates {
                tx.execute(
                    "UPDATE ai_providers SET api_key_enc = ?1 WHERE id = ?2",
                    params![value, id],
                )
                .map_err(err)?;
                count += 1;
            }
        }

        tx.commit().map_err(err)?;
        Ok(count)
    }

    /// 切换到口令模式：用口令派生新主密钥，并把已有密钥值整体重加密
    pub fn set_passphrase(&self, pool: &DbPool, passphrase: &str) -> Result<(), String> {
        let passphrase = checked_passphrase(passphrase)?;
        let old_key = self.current_key()?;
        let salt = random_salt();
        let new_key = derive_key(&passphrase, &salt)?;

        self.rotate(pool, &old_key, &new_key)?;

        let conn = pool.get().map_err(err)?;
        crate::db::meta_set(&conn, "kdf_salt", &B64.encode(salt))?;
        crate::db::meta_set(
            &conn,
            "key_check",
            &encrypt_with(&new_key, KEY_CHECK_AAD, KEY_CHECK_TEXT)?,
        )?;
        crate::db::meta_set(&conn, "key_source", KeySource::Passphrase.as_str())?;
        drop(conn);

        let mut inner = self.inner.write();
        inner.key = Some(new_key);
        inner.source = KeySource::Passphrase;
        inner.locked = None;
        Ok(())
    }

    /// 口令解锁
    pub fn unlock(&self, pool: &DbPool, passphrase: &str) -> Result<(), String> {
        let conn = pool.get().map_err(err)?;
        let salt = crate::db::meta_get(&conn, "kdf_salt")
            .ok_or_else(|| "E_LOCKED: 未找到口令盐值，请重新设置口令模式".to_string())?;
        let check = crate::db::meta_get(&conn, "key_check").unwrap_or_default();
        drop(conn);

        let salt = B64
            .decode(salt.as_bytes())
            .map_err(|_| "E_LOCKED: 口令盐值损坏，请重新设置口令模式".to_string())?;
        let key = derive_key(passphrase, &salt)?;
        if !check.is_empty() {
            match decrypt_with(&key, KEY_CHECK_AAD, &check) {
                Ok(text) if text == KEY_CHECK_TEXT => {}
                _ => {
                    return Err(
                        "E_LOCKED: 口令不正确，请重试（忘记口令可从密钥包恢复或重置）".to_string(),
                    )
                }
            }
        }

        {
            let mut inner = self.inner.write();
            inner.key = Some(key);
            inner.locked = None;
        }
        // 解锁后补做历史明文加密
        let _ = self.encrypt_plaintext(pool);
        Ok(())
    }

    /// 导出密钥包：用用户口令重新派生一把钥匙加密主密钥，落盘文件
    pub fn export_keypack(&self, path: &Path, passphrase: &str) -> Result<(), String> {
        let passphrase = checked_passphrase(passphrase)?;
        let key = self.current_key()?;
        let salt = random_salt();
        let wrap = derive_key(&passphrase, &salt)?;
        let nonce = random_nonce();
        let cipher = Aes256Gcm::new_from_slice(&wrap).map_err(err)?;
        let ciphertext = cipher
            .encrypt(Nonce::from_slice(&nonce), key.as_slice())
            .map_err(|_| "密钥包加密失败".to_string())?;
        let pack = KeyPack {
            app: "AstroMan".to_string(),
            version: 1,
            salt: B64.encode(salt),
            nonce: B64.encode(nonce),
            data: B64.encode(ciphertext),
            created_at: crate::util::now_ms(),
            source: self.source().as_str().to_string(),
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(err)?;
        }
        let text = serde_json::to_string_pretty(&pack).map_err(err)?;
        std::fs::write(path, text).map_err(|e| format!("写入密钥包失败（{}）：{e}", path.display()))
    }

    /// 导入密钥包：解开后替换当前主密钥，并把库内密钥值整体重加密
    pub fn import_keypack(
        &self,
        pool: &DbPool,
        path: &Path,
        passphrase: &str,
    ) -> Result<(), String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("读取密钥包失败（{}）：{e}", path.display()))?;
        let pack: KeyPack =
            serde_json::from_str(&text).map_err(|e| format!("密钥包格式不正确：{e}"))?;
        if pack.app != "AstroMan" || pack.version != 1 {
            return Err("密钥包版本不受支持，请使用同一版本导出的文件".to_string());
        }
        let salt = B64.decode(pack.salt.as_bytes()).map_err(err)?;
        let nonce = B64.decode(pack.nonce.as_bytes()).map_err(err)?;
        let data = B64.decode(pack.data.as_bytes()).map_err(err)?;
        let wrap = derive_key(passphrase, &salt)?;
        let cipher = Aes256Gcm::new_from_slice(&wrap).map_err(err)?;
        let plain = cipher
            .decrypt(Nonce::from_slice(&nonce), data.as_slice())
            .map_err(|_| "E_LOCKED: 密钥包口令不正确".to_string())?;
        if plain.len() != KEY_LEN {
            return Err("密钥包内容已损坏".to_string());
        }
        let mut new_key = [0u8; KEY_LEN];
        new_key.copy_from_slice(&plain);

        // 未锁定时可以把现有值重加密到新密钥；锁定态（密钥丢失）直接沿用包内密钥
        if let Ok(old_key) = self.current_key() {
            if old_key != new_key {
                self.rotate(pool, &old_key, &new_key)?;
            }
        }
        {
            let mut inner = self.inner.write();
            inner.key = Some(new_key);
            inner.locked = None;
        }
        Ok(())
    }

    /// 重置密钥：清空全部已加密的密钥值（不是删除密钥本身）
    pub fn reset_values(&self, pool: &DbPool) -> Result<usize, String> {
        let conn = pool.get().map_err(err)?;
        let env = conn
            .execute(
                "UPDATE env_variables SET value = '' WHERE is_secret = 1 AND value LIKE 'enc:v1:%'",
                [],
            )
            .map_err(err)?;
        let ai = conn
            .execute(
                "UPDATE ai_providers SET api_key_enc = '' WHERE api_key_enc LIKE 'enc:v1:%'",
                [],
            )
            .map_err(err)?;
        Ok(env + ai)
    }

    /// 用新密钥重写所有已加密值
    fn rotate(
        &self,
        pool: &DbPool,
        old_key: &[u8; KEY_LEN],
        new_key: &[u8; KEY_LEN],
    ) -> Result<usize, String> {
        let mut conn = pool.get().map_err(err)?;
        let tx = conn.transaction().map_err(err)?;
        let mut count = 0usize;

        {
            let mut stmt = tx
                .prepare("SELECT id, value FROM env_variables WHERE is_secret = 1 AND value LIKE 'enc:v1:%'")
                .map_err(err)?;
            let rows = stmt
                .query_map([], |row| {
                    Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(err)?;
            let mut updates = Vec::new();
            for row in rows {
                let (id, value) = row.map_err(err)?;
                let plain = decrypt_with(old_key, AAD_ENV, &value)?;
                updates.push((id, encrypt_with(new_key, AAD_ENV, &plain)?));
            }
            drop(stmt);
            for (id, value) in updates {
                tx.execute(
                    "UPDATE env_variables SET value = ?1 WHERE id = ?2",
                    params![value, id],
                )
                .map_err(err)?;
                count += 1;
            }
        }

        {
            let mut stmt = tx
                .prepare(
                    "SELECT id, api_key_enc FROM ai_providers WHERE api_key_enc LIKE 'enc:v1:%'",
                )
                .map_err(err)?;
            let rows = stmt
                .query_map([], |row| {
                    Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(err)?;
            let mut updates = Vec::new();
            for row in rows {
                let (id, value) = row.map_err(err)?;
                let plain = decrypt_with(old_key, AAD_AI, &value)?;
                updates.push((id, encrypt_with(new_key, AAD_AI, &plain)?));
            }
            drop(stmt);
            for (id, value) in updates {
                tx.execute(
                    "UPDATE ai_providers SET api_key_enc = ?1 WHERE id = ?2",
                    params![value, id],
                )
                .map_err(err)?;
                count += 1;
            }
        }

        tx.commit().map_err(err)?;
        Ok(count)
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyPack {
    pub app: String,
    pub version: u32,
    pub salt: String,
    pub nonce: String,
    pub data: String,
    pub created_at: i64,
    pub source: String,
}

/* ---------- 全局单例 ---------- */

static STORE: std::sync::OnceLock<SecretStore> = std::sync::OnceLock::new();

pub fn install(store: SecretStore) {
    // 重复安装（测试中多次初始化）时保留首次实例，避免运行中的引用失效
    let _ = STORE.set(store);
}

pub fn store() -> Option<&'static SecretStore> {
    STORE.get()
}

/// 单元测试用：装一个固定的测试密钥（只装一次）
#[cfg(test)]
pub fn install_for_test() {
    if STORE.get().is_none() {
        let _ = STORE.set(SecretStore::from_key(
            std::env::temp_dir(),
            [5u8; KEY_LEN],
            KeySource::File,
        ));
    }
}

/// 加密（未安装密钥库时返回错误，绝不落明文）
pub fn encrypt(aad: &str, plain: &str) -> Result<String, String> {
    match store() {
        Some(store) => store.encrypt(aad, plain),
        None => Err("E_LOCKED: 密钥模块尚未初始化".to_string()),
    }
}

/// 解密：未安装 / 未解锁 / 非密文时都返回原值或空串，调用方无需判空
pub fn decrypt(aad: &str, stored: &str) -> String {
    match store() {
        Some(store) => store.decrypt(aad, stored),
        None => stored.to_string(),
    }
}

/* ---------- 加解密原语 ---------- */

fn encrypt_with(key: &[u8; KEY_LEN], aad: &str, plain: &str) -> Result<String, String> {
    let cipher = Aes256Gcm::new_from_slice(key).map_err(err)?;
    let nonce = random_nonce();
    let ciphertext = cipher
        .encrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: plain.as_bytes(),
                aad: aad.as_bytes(),
            },
        )
        .map_err(|_| "加密失败".to_string())?;
    let mut packed = nonce.to_vec();
    packed.extend_from_slice(&ciphertext);
    Ok(format!("{ENC_PREFIX}{}", B64.encode(packed)))
}

fn decrypt_with(key: &[u8; KEY_LEN], aad: &str, stored: &str) -> Result<String, String> {
    let body = stored
        .strip_prefix(ENC_PREFIX)
        .ok_or_else(|| "不是密文格式".to_string())?;
    let packed = B64
        .decode(body.as_bytes())
        .map_err(|_| "密文损坏".to_string())?;
    if packed.len() <= 12 {
        return Err("密文长度不足".to_string());
    }
    let (nonce, ciphertext) = packed.split_at(12);
    let cipher = Aes256Gcm::new_from_slice(key).map_err(err)?;
    let plain = cipher
        .decrypt(
            Nonce::from_slice(nonce),
            Payload {
                msg: ciphertext,
                aad: aad.as_bytes(),
            },
        )
        .map_err(|_| "解密失败（密钥不匹配或数据被篡改）".to_string())?;
    String::from_utf8(plain).map_err(err)
}

/// v1.0.7 的 AI Key 格式：`v1:<base64(nonce||ct)>`，密钥是固定字符串的 SHA-256
pub fn legacy_ai_decrypt(stored: &str) -> String {
    use sha2::{Digest, Sha256};
    let Some(body) = stored.strip_prefix("v1:") else {
        return String::new();
    };
    let Ok(packed) = B64.decode(body.as_bytes()) else {
        return String::new();
    };
    if packed.len() <= 12 {
        return String::new();
    }
    let mut hasher = Sha256::new();
    hasher.update(b"AstroMan::ai::provider::v1");
    let key = hasher.finalize();
    let (nonce, ciphertext) = packed.split_at(12);
    let Ok(cipher) = Aes256Gcm::new_from_slice(&key) else {
        return String::new();
    };
    match cipher.decrypt(Nonce::from_slice(nonce), ciphertext) {
        Ok(plain) => String::from_utf8(plain).unwrap_or_default(),
        Err(_) => String::new(),
    }
}

fn random_key() -> [u8; KEY_LEN] {
    let mut key = [0u8; KEY_LEN];
    // 两段 UUID v4 拼接：16 字节随机 × 2，避免引入 rand 依赖
    key[..16].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
    key[16..].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
    key
}

fn random_nonce() -> [u8; 12] {
    let raw = uuid::Uuid::new_v4().into_bytes();
    let mut nonce = [0u8; 12];
    nonce.copy_from_slice(&raw[..12]);
    nonce
}

fn random_salt() -> [u8; 16] {
    let mut salt = [0u8; 16];
    salt.copy_from_slice(uuid::Uuid::new_v4().as_bytes());
    salt
}

fn checked_passphrase(passphrase: &str) -> Result<String, String> {
    let trimmed = passphrase.trim();
    if trimmed.chars().count() < 8 {
        return Err("口令至少 8 位（建议 12 位以上，含字母与数字）".to_string());
    }
    Ok(trimmed.to_string())
}

/// Argon2id 派生：19MB / 3 轮 / 1 并行，单次约 60–120ms
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

/* ---------- 系统钥匙串 ---------- */

fn key_file(data_dir: &Path) -> PathBuf {
    data_dir.join("keys").join("master.key")
}

fn read_file_key(data_dir: &Path) -> Result<[u8; KEY_LEN], String> {
    let path = key_file(data_dir);
    let text =
        std::fs::read_to_string(&path).map_err(|e| format!("{} 不可读：{e}", path.display()))?;
    let raw = B64
        .decode(text.trim().as_bytes())
        .map_err(|_| "密钥文件内容损坏".to_string())?;
    if raw.len() != KEY_LEN {
        return Err("密钥文件长度不正确".to_string());
    }
    let mut key = [0u8; KEY_LEN];
    key.copy_from_slice(&raw);
    Ok(key)
}

fn write_file_key(data_dir: &Path, key: &[u8; KEY_LEN]) -> Result<(), String> {
    let path = key_file(data_dir);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(err)?;
    }
    std::fs::write(&path, B64.encode(key)).map_err(|e| format!("写入密钥文件失败：{e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

#[cfg(windows)]
fn keyring_read() -> Result<[u8; KEY_LEN], String> {
    use windows_sys::Win32::Security::Credentials::{
        CredFree, CredReadW, CREDENTIALW, CRED_TYPE_GENERIC,
    };

    let target: Vec<u16> = KEYRING_TARGET
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let mut ptr: *mut CREDENTIALW = std::ptr::null_mut();
    let ok = unsafe { CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut ptr) };
    if ok == 0 || ptr.is_null() {
        return Err("凭据管理器中未找到 AstroMan 主密钥".to_string());
    }
    let result = unsafe {
        let cred = &*ptr;
        let blob =
            std::slice::from_raw_parts(cred.CredentialBlob, cred.CredentialBlobSize as usize);
        B64.decode(blob).map_err(|_| "钥匙串内容损坏".to_string())
    };
    unsafe { CredFree(ptr as *const core::ffi::c_void) };
    let raw = result?;
    if raw.len() != KEY_LEN {
        return Err("钥匙串中的密钥长度不正确".to_string());
    }
    let mut key = [0u8; KEY_LEN];
    key.copy_from_slice(&raw);
    Ok(key)
}

#[cfg(windows)]
fn keyring_write(key: &[u8; KEY_LEN]) -> Result<(), String> {
    use windows_sys::Win32::Security::Credentials::{
        CredWriteW, CREDENTIALW, CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC,
    };

    let mut target: Vec<u16> = KEYRING_TARGET
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let mut user: Vec<u16> = KEYRING_USER
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let mut blob = B64.encode(key).into_bytes();
    let credential = CREDENTIALW {
        Flags: 0,
        Type: CRED_TYPE_GENERIC,
        TargetName: target.as_mut_ptr(),
        Comment: std::ptr::null_mut(),
        LastWritten: windows_sys::Win32::Foundation::FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        },
        CredentialBlobSize: blob.len() as u32,
        CredentialBlob: blob.as_mut_ptr(),
        Persist: CRED_PERSIST_LOCAL_MACHINE,
        AttributeCount: 0,
        Attributes: std::ptr::null_mut(),
        TargetAlias: std::ptr::null_mut(),
        UserName: user.as_mut_ptr(),
    };
    let ok = unsafe { CredWriteW(&credential, 0) };
    if ok == 0 {
        return Err("写入系统凭据管理器失败".to_string());
    }
    Ok(())
}

#[cfg(not(windows))]
fn keyring_read() -> Result<[u8; KEY_LEN], String> {
    Err("当前平台暂未接入系统钥匙串".to_string())
}

#[cfg(not(windows))]
fn keyring_write(_key: &[u8; KEY_LEN]) -> Result<(), String> {
    Err("当前平台暂未接入系统钥匙串".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> SecretStore {
        SecretStore::from_key(std::env::temp_dir(), [7u8; KEY_LEN], KeySource::File)
    }

    #[test]
    fn encrypt_roundtrip_and_prefix() {
        let store = store();
        let cipher = store.encrypt(AAD_ENV, "secret-value").unwrap();
        assert!(cipher.starts_with(ENC_PREFIX));
        assert!(!cipher.contains("secret-value"));
        assert_eq!(
            store.decrypt_checked(AAD_ENV, &cipher).unwrap(),
            "secret-value"
        );
    }

    #[test]
    fn aad_binding_rejects_moved_ciphertext() {
        let store = store();
        let cipher = store.encrypt(AAD_ENV, "token").unwrap();
        // 同一把密钥、不同上下文必须失败
        assert!(store.decrypt_checked(AAD_AI, &cipher).is_err());
    }

    #[test]
    fn plaintext_passes_through_for_legacy_rows() {
        let store = store();
        assert_eq!(
            store.decrypt_checked(AAD_ENV, "plain-old").unwrap(),
            "plain-old"
        );
    }

    #[test]
    fn wrong_key_fails_instead_of_returning_garbage() {
        let a = SecretStore::from_key(std::env::temp_dir(), [1u8; KEY_LEN], KeySource::File);
        let b = SecretStore::from_key(std::env::temp_dir(), [2u8; KEY_LEN], KeySource::File);
        let cipher = a.encrypt(AAD_ENV, "value").unwrap();
        assert!(b.decrypt_checked(AAD_ENV, &cipher).is_err());
    }

    #[test]
    fn passphrase_derivation_is_stable_and_salt_sensitive() {
        let salt = [9u8; 16];
        let k1 = derive_key("correct horse battery", &salt).unwrap();
        let k2 = derive_key("correct horse battery", &salt).unwrap();
        let k3 = derive_key("correct horse battery", &[8u8; 16]).unwrap();
        assert_eq!(k1, k2);
        assert_ne!(k1, k3);
    }

    #[test]
    fn legacy_ai_format_still_decrypts() {
        use aes_gcm::aead::Aead;
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(b"AstroMan::ai::provider::v1");
        let key = hasher.finalize();
        let nonce = [3u8; 12];
        let cipher = Aes256Gcm::new_from_slice(&key).unwrap();
        let ct = cipher
            .encrypt(Nonce::from_slice(&nonce), b"sk-old".as_ref())
            .unwrap();
        let mut packed = nonce.to_vec();
        packed.extend_from_slice(&ct);
        let stored = format!("v1:{}", B64.encode(packed));
        assert_eq!(legacy_ai_decrypt(&stored), "sk-old");
    }
}
