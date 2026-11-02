//! 主密钥与字段级加密（G-02）
//!
//! 现状问题（见技术方案 4.2）：环境变量的 `is_secret` 只控制前端输入框变星号，
//! 值仍然明文入库；AI Key 用 `SHA256("AstroMan::ai::provider::v1")` 做密钥，
//! 等于静态混淆——拿到库的人自己算一次就能解开。
//!
//! 本模块做三件事：
//! 1. 主密钥：优先进系统钥匙串（Windows 凭据管理器），可用口令模式（Argon2id，不落盘），
//!    最后才降级到文件——并且明确告诉用户「文件密钥与密文同目录，等于无加密」。
//!    钥匙串条目**按数据目录派生**，并且在 `key_check` / 指纹 / 抽样解密里校验，
//!    避免多份安装或测试库互相覆盖后静默解不开（v1.0.8 曾因共享条目踩坑）。
//! 2. 字段加密：`enc:v1:<base64(nonce12 || ciphertext)>`，AES-256-GCM，
//!    AAD 绑定「表名:列名」，把密文搬到别的列会直接解密失败。
//! 3. 迁移：把历史明文值就地加密，重复执行幂等。
//!
//! 全局单例是为了避免把 `SecretStore` 穿过几十个既有调用点；
//! 单元测试用 `SecretStore::from_key` 构造独立实例，互不影响，且测试构建
//! 永远不会写入系统钥匙串（`keyring_allowed`）。

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
/// 条目名前缀，实际条目为 `AstroMan/master-key/<数据目录指纹>`。
/// v1.0.8 之前所有安装共用不带后缀的这一个条目：任何一份新库（开发实例、测试库）
/// 首次启动都会把正在使用的密钥覆盖掉，因此改为按数据目录派生
const KEYRING_TARGET: &str = "AstroMan/master-key";
const KEYRING_USER: &str = "master-key";
const KEY_CHECK_AAD: &str = "app_meta:key_check";
const KEY_CHECK_TEXT: &str = "astroman-master-key-check";
/// 密钥指纹（sha256 前 8 字节）用于没有 key_check 的历史库做校验
const META_KEY_CHECK: &str = "key_check";
const META_KEY_FP: &str = "key_fingerprint";
/// CI / 排障用：设置后强制走本地密钥文件，绝不碰系统钥匙串
const DISABLE_KEYRING_ENV: &str = "ASTROMAN_DISABLE_KEYRING";

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
    /// 主密钥与库内密文不匹配（通常是钥匙串条目被其它实例覆盖后重新生成）
    pub key_mismatch: bool,
    pub key_mismatch_reason: Option<String>,
    /// 不匹配时，解不开的已加密值条数（用于提示用户重填）
    pub broken_secret_count: i64,
}

struct Inner {
    key: Option<[u8; KEY_LEN]>,
    source: KeySource,
    locked: Option<String>,
    keyring_available: bool,
    /// 密钥能读到但和库内密文对不上：值解不出来，但**仍保留密钥**，
    /// 这样用户重新填写密钥时能正常加密写回
    mismatch: Option<String>,
}

/// 主密钥持有者：加密只在这里发生，其它模块只拿到密文/明文
pub struct SecretStore {
    #[allow(dead_code)]
    data_dir: PathBuf,
    inner: parking_lot::RwLock<Inner>,
}

impl SecretStore {
    /// 启动时初始化：读钥匙串 / 读文件 / 等待口令解锁；首次启动生成主密钥
    ///
    /// 拿到密钥后会先校验「这把钥匙是不是加密这批密文的那把」（`key_check` /
    /// 指纹 / 抽样解密）。对不上时**不静默清空**，而是记一条明确告警并置
    /// `keyMismatch`，让设置页能提示用户重填；否则整库密钥值会悄悄变空。
    pub fn bootstrap(data_dir: &Path, pool: &DbPool) -> Result<Self, String> {
        let conn = pool.get().map_err(err)?;
        let source_raw = crate::db::meta_get(&conn, "key_source");
        drop(conn);

        let target = keyring_target(data_dir);
        let (key, source, locked, keyring_available) = match source_raw.as_deref() {
            Some("keyring") => match read_key_any(&target) {
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
                let (source, keyring_available) = if keyring_allowed() {
                    match keyring_write(&target, &key) {
                        Ok(()) => (KeySource::Keyring, true),
                        Err(reason) => {
                            log::warn!("系统钥匙串不可用（{reason}），降级为本地密钥文件");
                            write_file_key(data_dir, &key)?;
                            (KeySource::File, false)
                        }
                    }
                } else {
                    // 测试 / CI：只写数据目录内的密钥文件，绝不碰用户真实凭据
                    write_file_key(data_dir, &key)?;
                    (KeySource::File, false)
                };
                let conn = pool.get().map_err(err)?;
                crate::db::meta_set(&conn, "crypto_version", "1")?;
                crate::db::meta_set(&conn, "key_source", source.as_str())?;
                crate::db::meta_set(&conn, "key_created_at", &crate::util::now_ms().to_string())?;
                drop(conn);
                (Some(key), source, None, keyring_available)
            }
        };

        // 有密钥就校验一次：能读到但解不开，说明这把钥匙不是加密数据的那把
        let mismatch = match key {
            Some(ref key) => match verify_key(pool, key) {
                Ok(()) => None,
                Err(reason) => {
                    log::warn!("{reason}");
                    Some(reason)
                }
            },
            None => None,
        };

        let store = Self {
            data_dir: data_dir.to_path_buf(),
            inner: parking_lot::RwLock::new(Inner {
                key,
                source,
                locked,
                keyring_available,
                mismatch,
            }),
        };

        // 未锁定且密钥可信时，顺手把历史明文加密（幂等，失败不阻断启动）
        // 密钥不可信时绝不做这一步，否则等于用错钥匙把明文锁死
        if !store.is_locked() && store.mismatch().is_none() {
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
                mismatch: None,
            }),
        }
    }

    pub fn is_locked(&self) -> bool {
        self.inner.read().key.is_none()
    }

    /// 密钥可读但与库内密文不匹配时的说明（None 表示正常）
    pub fn mismatch(&self) -> Option<String> {
        self.inner.read().mismatch.clone()
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
    /// 已知密钥不匹配时不再逐条刷日志（启动时已经告警过一次）。
    pub fn decrypt(&self, aad: &str, stored: &str) -> String {
        if self.mismatch().is_some() {
            return String::new();
        }
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
        let mismatch = inner.mismatch.clone();
        let broken = if mismatch.is_some() {
            count_broken(pool, inner.key.as_ref()).unwrap_or(0)
        } else {
            0
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
            key_mismatch: mismatch.is_some(),
            key_mismatch_reason: mismatch,
            broken_secret_count: broken,
        }
    }

    /// 把历史明文加密（幂等）：环境变量中 is_secret=1 的值 + AI Key
    pub fn encrypt_plaintext(&self, pool: &DbPool) -> Result<usize, String> {
        if let Some(reason) = self.mismatch() {
            // 钥匙不对时加密明文＝用错钥匙把数据锁死，宁可不动
            return Err(reason);
        }
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
                    // 解不开就别装作迁移成功：这条值已经丢了，明确告诉用户
                    log::warn!(
                        "AI 服务商 #{id} 的旧格式密钥（v1:）无法解密，已跳过迁移；\
请在「设置 → AI」重新填写该服务商的 Key"
                    );
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

        // 密钥已不匹配时旧值本来就解不开，重加密必然失败；
        // 此时直接换成新密钥，让用户能重新开始（旧值已在别处提示重填）
        if self.mismatch().is_none() {
            self.rotate(pool, &old_key, &new_key)?;
        } else {
            log::warn!("当前主密钥与库内密文不匹配，跳过重加密，直接切换到口令模式");
        }

        let conn = pool.get().map_err(err)?;
        crate::db::meta_set(&conn, "kdf_salt", &B64.encode(salt))?;
        write_key_meta(&conn, &new_key)?;
        crate::db::meta_set(&conn, "key_source", KeySource::Passphrase.as_str())?;
        drop(conn);

        let mut inner = self.inner.write();
        inner.key = Some(new_key);
        inner.source = KeySource::Passphrase;
        inner.locked = None;
        inner.mismatch = None;
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
            inner.mismatch = None;
        }
        // 解锁成功后刷新校验信息，并把历史明文补加密（幂等）
        if let Ok(conn) = pool.get() {
            let _ = write_key_meta(&conn, &key);
        }
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

        // 未锁定时可以把现有值重加密到新密钥；锁定态（密钥丢失）直接沿用包内密钥。
        // 已知密钥不匹配时不能重加密——旧值本来就解不开，强行 rotate 会整体失败，
        // 而「导入密钥包」正是这种情况下的恢复手段，必须能走通。
        let reusable = self.mismatch().is_none();
        if let Ok(old_key) = self.current_key() {
            if old_key != new_key {
                if reusable {
                    self.rotate(pool, &old_key, &new_key)?;
                } else {
                    log::warn!("当前主密钥与库内密文不匹配，跳过重加密，直接采用导入的密钥");
                }
            }
        }
        {
            let mut inner = self.inner.write();
            inner.key = Some(new_key);
            inner.locked = None;
            inner.mismatch = None;
        }
        if let Ok(conn) = pool.get() {
            write_key_meta(&conn, &new_key)?;
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
        // 值都清掉了，就不该再报「解不开」；下次启动校验也没有密文可比对
        if env + ai > 0 {
            self.inner.write().mismatch = None;
        }
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

/* ---------- 密钥校验：确认「这把钥匙」就是加密这批密文的那把 ---------- */

fn mismatch_reason() -> String {
    "E_KEY_MISMATCH: 当前主密钥与库内已保存的密钥值不匹配（通常是钥匙串条目被\
其它实例覆盖或重置过）。已保存的 Key 无法解出，请在「设置 → 安全」导入密钥包，\
或在各服务商里重新填写一次"
        .to_string()
}

/// 带校验信息写入：`key_check`（能直接验明文）+ 密钥指纹（老库兜底）
fn write_key_meta(conn: &rusqlite::Connection, key: &[u8; KEY_LEN]) -> Result<(), String> {
    crate::db::meta_set(
        conn,
        META_KEY_CHECK,
        &encrypt_with(key, KEY_CHECK_AAD, KEY_CHECK_TEXT)?,
    )?;
    crate::db::meta_set(conn, META_KEY_FP, &key_fingerprint(key))?;
    Ok(())
}

fn key_fingerprint(key: &[u8; KEY_LEN]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(key);
    hasher.finalize()[..8]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// 取若干条已加密样本（AAD 必须一起带上，否则解不开）
fn enc_samples(conn: &rusqlite::Connection, limit: i64) -> Vec<(&'static str, String)> {
    let mut samples = Vec::new();
    if let Ok(mut stmt) = conn.prepare(
        "SELECT value FROM env_variables
         WHERE is_secret = 1 AND value LIKE 'enc:v1:%' LIMIT ?1",
    ) {
        if let Ok(rows) = stmt.query_map([limit], |row| row.get::<_, String>(0)) {
            samples.extend(rows.filter_map(Result::ok).map(|value| (AAD_ENV, value)));
        }
    }
    if let Ok(mut stmt) = conn.prepare(
        "SELECT api_key_enc FROM ai_providers
         WHERE api_key_enc LIKE 'enc:v1:%' LIMIT ?1",
    ) {
        if let Ok(rows) = stmt.query_map([limit], |row| row.get::<_, String>(0)) {
            samples.extend(rows.filter_map(Result::ok).map(|value| (AAD_AI, value)));
        }
    }
    samples
}

/// 校验主密钥：有 `key_check` 用它，没有指纹就比指纹，再没有就抽样解密。
/// 三种信息都缺（全新库 / 纯明文老库）时视为通过，并顺手补写校验信息。
///
/// 补写校验信息属于「附带好处」，失败只记日志——写不进去不能阻断启动。
fn verify_key(pool: &DbPool, key: &[u8; KEY_LEN]) -> Result<(), String> {
    let conn = pool.get().map_err(err)?;
    if let Some(check) = crate::db::meta_get(&conn, META_KEY_CHECK) {
        return match decrypt_with(key, KEY_CHECK_AAD, &check) {
            Ok(text) if text == KEY_CHECK_TEXT => Ok(()),
            _ => Err(mismatch_reason()),
        };
    }
    if let Some(fingerprint) = crate::db::meta_get(&conn, META_KEY_FP) {
        if fingerprint != key_fingerprint(key) {
            return Err(mismatch_reason());
        }
        if let Err(error) = write_key_meta(&conn, key) {
            log::warn!("补写密钥校验信息失败（不影响使用）：{error}");
        }
        return Ok(());
    }

    let samples = enc_samples(&conn, 5);
    if !samples.is_empty()
        && !samples
            .iter()
            .any(|(aad, value)| decrypt_with(key, aad, value).is_ok())
    {
        // 有密文但一条都解不开：钥匙不对。这里绝不能写 key_check，
        // 否则下次启动会把这把错钥匙当成「已验证」，问题被永久掩盖
        return Err(mismatch_reason());
    }
    if let Err(error) = write_key_meta(&conn, key) {
        log::warn!("补写密钥校验信息失败（不影响使用）：{error}");
    }
    Ok(())
}

/// 统计解不开的已加密值条数（状态页展示用）
fn count_broken(pool: &DbPool, key: Option<&[u8; KEY_LEN]>) -> Result<i64, String> {
    let Some(key) = key else {
        return Ok(0);
    };
    let conn = pool.get().map_err(err)?;
    let mut broken = 0i64;
    for (aad, value) in enc_samples(&conn, 500) {
        if decrypt_with(key, aad, &value).is_err() {
            broken += 1;
        }
    }
    Ok(broken)
}

/* ---------- 钥匙串条目命名 ---------- */

/// 数据目录归一化：大小写与末尾分隔符在不同来源下不一致，先统一再哈希
fn normalize_path(path: &Path) -> String {
    let text = path.to_string_lossy().replace('/', "\\");
    let trimmed = text.trim_end_matches('\\');
    trimmed.to_lowercase()
}

/// 每个数据目录一个钥匙串条目：开发实例、测试库、第二份安装不会互相覆盖
fn keyring_target(data_dir: &Path) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(normalize_path(data_dir).as_bytes());
    let digest = hasher.finalize();
    let suffix: String = digest[..8].iter().map(|b| format!("{b:02x}")).collect();
    format!("{KEYRING_TARGET}/{suffix}")
}

/// 测试构建与 CI 一律不碰系统钥匙串，避免把正在使用的密钥覆盖掉
fn keyring_allowed() -> bool {
    if cfg!(test) {
        return false;
    }
    if let Some(value) = std::env::var_os(DISABLE_KEYRING_ENV) {
        let text = value.to_string_lossy();
        if !text.is_empty() && text != "0" && !text.eq_ignore_ascii_case("false") {
            return false;
        }
    }
    true
}

/// 先读本目录条目；老版本只有共享条目，读到后顺手迁移过来
fn read_key_any(target: &str) -> Result<[u8; KEY_LEN], String> {
    match keyring_read(target) {
        Ok(key) => Ok(key),
        Err(primary) => {
            // 老版本只有不带后缀的共享条目，读到后迁移到本目录专属条目
            let legacy = keyring_read(KEYRING_TARGET).map_err(|legacy_error| {
                format!("{primary}；旧共享条目也不可用（{legacy_error}）")
            })?;
            match keyring_write(target, &legacy) {
                Ok(()) => log::info!("已把主密钥迁移到本数据目录专属的钥匙串条目"),
                Err(error) => log::warn!("迁移钥匙串条目失败（不影响本次启动）：{error}"),
            }
            Ok(legacy)
        }
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
fn keyring_read(target_name: &str) -> Result<[u8; KEY_LEN], String> {
    use windows_sys::Win32::Security::Credentials::{
        CredFree, CredReadW, CREDENTIALW, CRED_TYPE_GENERIC,
    };

    let target: Vec<u16> = target_name
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let mut ptr: *mut CREDENTIALW = std::ptr::null_mut();
    let ok = unsafe { CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut ptr) };
    if ok == 0 || ptr.is_null() {
        return Err(format!("凭据管理器中未找到 {target_name}"));
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
fn keyring_write(target_name: &str, key: &[u8; KEY_LEN]) -> Result<(), String> {
    use windows_sys::Win32::Security::Credentials::{
        CredWriteW, CREDENTIALW, CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC,
    };

    let mut target: Vec<u16> = target_name
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
fn keyring_read(_target_name: &str) -> Result<[u8; KEY_LEN], String> {
    Err("当前平台暂未接入系统钥匙串".to_string())
}

#[cfg(not(windows))]
fn keyring_write(_target_name: &str, _key: &[u8; KEY_LEN]) -> Result<(), String> {
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

    #[test]
    fn keyring_target_is_per_data_dir() {
        let a = keyring_target(Path::new("D:/AstroMan/data"));
        let b = keyring_target(Path::new("D:/AstroMan/data-alt"));
        assert_ne!(a, b, "不同数据目录必须拿到不同条目，否则会互相覆盖主密钥");
        assert!(a.starts_with(KEYRING_TARGET));
        // 大小写与末尾分隔符不应影响结果，否则升级后读不到原条目
        let again = keyring_target(Path::new("d:\\astroman\\DATA\\"));
        assert_eq!(a, again);
    }

    #[test]
    fn keyring_is_never_touched_by_tests() {
        // 这条断言保护真实安装：测试库里 bootstrap 一旦写钥匙串，
        // 就会把用户正在使用的主密钥覆盖掉（v1.0.8 曾发生）
        assert!(!keyring_allowed());
    }

    #[test]
    fn fingerprint_is_short_and_key_dependent() {
        let a = key_fingerprint(&[1u8; KEY_LEN]);
        let b = key_fingerprint(&[2u8; KEY_LEN]);
        assert_eq!(a.len(), 16);
        assert_ne!(a, b);
        assert_eq!(a, key_fingerprint(&[1u8; KEY_LEN]));
    }

    #[test]
    fn mismatch_keeps_key_usable_for_new_writes() {
        // 密钥不匹配时仍要能加密（否则用户连重填 Key 都做不到）
        let store = SecretStore::from_key(std::env::temp_dir(), [4u8; KEY_LEN], KeySource::File);
        store.inner.write().mismatch = Some(mismatch_reason());
        assert!(store.mismatch().is_some());
        let cipher = store.encrypt(AAD_AI, "sk-new").unwrap();
        assert!(cipher.starts_with(ENC_PREFIX));
        // 已知不匹配时不再逐条打印解密告警，直接返回空串
        assert_eq!(store.decrypt(AAD_AI, &cipher), "");
    }
}
