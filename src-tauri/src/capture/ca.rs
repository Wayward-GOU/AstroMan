//! 本地根证书：生成、加载、指纹、导出。
//!
//! 私钥是长期资产，直接落盘并把权限收紧到当前用户；不走 `secret.rs` 的口令库，
//! 否则“未解锁就不能抓包”会变成新的启动故障点。

use hudsucker::certificate_authority::RcgenAuthority;
use hudsucker::rcgen::Issuer;
use hudsucker::rcgen::{
    date_time_ymd, BasicConstraints, CertificateParams, DnType, IsCa, KeyPair, KeyUsagePurpose,
};
use serde::{Deserialize, Serialize};
use sha1::Sha1;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

const CERT_FILE: &str = "astroman-ca.crt";
const KEY_FILE: &str = "astroman-ca.key";
const META_FILE: &str = "astroman-ca.json";

#[derive(Debug, Clone)]
pub struct CaMaterial {
    pub cert_pem: String,
    pub key_pem: String,
    pub fingerprint: String,
    pub created_at: i64,
    pub expires_at: i64,
    #[allow(dead_code)]
    pub cert_path: PathBuf,
    #[allow(dead_code)]
    pub key_path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaInfo {
    pub exists: bool,
    /// SHA-256 指纹（冒号分隔大写）
    pub fingerprint: String,
    /// SHA-1 指纹（Windows 证书管理器里显示的「指纹」，无分隔大写）
    pub thumbprint: String,
    pub created_at: i64,
    pub expires_at: i64,
    pub installed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CaMeta {
    fingerprint: String,
    created_at: i64,
    expires_at: i64,
}

pub fn certs_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("certs")
}

fn paths(data_dir: &Path) -> (PathBuf, PathBuf, PathBuf) {
    let dir = certs_dir(data_dir);
    (dir.join(CERT_FILE), dir.join(KEY_FILE), dir.join(META_FILE))
}

/// 读取已有证书；不存在或损坏时生成。
pub fn ensure(data_dir: &Path) -> Result<CaMaterial, String> {
    let (cert_path, key_path, meta_path) = paths(data_dir);
    if cert_path.is_file() && key_path.is_file() {
        return load(&cert_path, &key_path, &meta_path);
    }
    generate(data_dir, false)
}

/// 生成新证书。`force=true` 会覆盖旧证书，调用方必须先提示用户重新安装。
pub fn generate(data_dir: &Path, force: bool) -> Result<CaMaterial, String> {
    let (cert_path, key_path, meta_path) = paths(data_dir);
    if !force && cert_path.is_file() && key_path.is_file() {
        return load(&cert_path, &key_path, &meta_path);
    }
    if let Some(parent) = cert_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("E_CA: 创建证书目录失败 {e}"))?;
    }

    let mut params = CertificateParams::new(Vec::<String>::new())
        .map_err(|e| format!("E_CA: 初始化证书参数失败 {e}"))?;
    params
        .distinguished_name
        .push(DnType::CommonName, "AstroMan Root CA");
    params
        .distinguished_name
        .push(DnType::OrganizationName, "AstroMan");
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params.key_usages = vec![
        KeyUsagePurpose::KeyCertSign,
        KeyUsagePurpose::CrlSign,
        KeyUsagePurpose::DigitalSignature,
    ];
    // rcgen 0.14 没有 Validity，用固定时间边界，避免再引入 time 依赖
    params.not_before = date_time_ymd(2026, 1, 1);
    params.not_after = date_time_ymd(2036, 1, 1);

    let key = KeyPair::generate().map_err(|e| format!("E_CA: 生成私钥失败 {e}"))?;
    let cert = params
        .self_signed(&key)
        .map_err(|e| format!("E_CA: 签发证书失败 {e}"))?;

    let cert_pem = cert.pem();
    let key_pem = key.serialize_pem();
    let fingerprint = fingerprint_der(cert.der());
    let created_at = crate::util::now_ms();
    let expires_at = expiry_ms();

    std::fs::write(&cert_path, cert_pem.as_bytes())
        .map_err(|e| format!("E_CA: 写入证书失败 {e}"))?;
    std::fs::write(&key_path, key_pem.as_bytes()).map_err(|e| format!("E_CA: 写入私钥失败 {e}"))?;
    restrict_key(&key_path);

    let meta = CaMeta {
        fingerprint: fingerprint.clone(),
        created_at,
        expires_at,
    };
    write_meta(&meta_path, &meta)?;

    log::info!(
        "已生成抓包根证书：{}（SHA-256 {fingerprint}）",
        cert_path.display()
    );
    Ok(CaMaterial {
        cert_pem,
        key_pem,
        fingerprint,
        created_at,
        expires_at,
        cert_path,
        key_path,
    })
}

fn load(cert_path: &Path, key_path: &Path, meta_path: &Path) -> Result<CaMaterial, String> {
    let cert_pem =
        std::fs::read_to_string(cert_path).map_err(|e| format!("E_CA: 读取证书失败 {e}"))?;
    let key_pem =
        std::fs::read_to_string(key_path).map_err(|e| format!("E_CA: 读取私钥失败 {e}"))?;

    // 先确认这对证书/私钥能用于 RcgenAuthority，损坏时给出明确错误
    let key_pair = KeyPair::from_pem(&key_pem).map_err(|e| format!("E_CA: 私钥无效 {e}"))?;
    Issuer::from_ca_cert_pem(&cert_pem, key_pair).map_err(|e| format!("E_CA: 证书无效 {e}"))?;

    let der = der_from_pem(&cert_pem)?;
    let fingerprint = fingerprint_der(&der);
    let meta = read_meta(meta_path).unwrap_or_else(|| CaMeta {
        fingerprint: fingerprint.clone(),
        created_at: file_created_ms(cert_path),
        expires_at: expiry_ms(),
    });
    if meta.fingerprint != fingerprint {
        log::warn!("抓包根证书指纹与元数据不一致，以实际证书为准");
    }

    Ok(CaMaterial {
        cert_pem,
        key_pem,
        fingerprint,
        created_at: meta.created_at,
        expires_at: meta.expires_at,
        cert_path: cert_path.to_path_buf(),
        key_path: key_path.to_path_buf(),
    })
}

pub fn info(data_dir: &Path) -> CaInfo {
    match ensure(data_dir) {
        Ok(material) => CaInfo {
            exists: true,
            fingerprint: material.fingerprint,
            thumbprint: thumbprint_pem(&material.cert_pem).unwrap_or_default(),
            created_at: material.created_at,
            expires_at: material.expires_at,
            installed: is_trusted_pem(&material.cert_pem),
        },
        Err(error) => {
            log::warn!("读取抓包根证书失败：{error}");
            CaInfo {
                exists: false,
                fingerprint: String::new(),
                thumbprint: String::new(),
                created_at: 0,
                expires_at: 0,
                installed: false,
            }
        }
    }
}

/// 把当前 CA 装进「当前用户 → 受信任的根证书颁发机构」。
/// Windows 用 `certutil -addstore -user Root`，不需要管理员权限。
pub fn install(data_dir: &Path) -> Result<CaInfo, String> {
    let material = ensure(data_dir)?;
    install_file(&material.cert_path)?;
    Ok(info(data_dir))
}

#[cfg(windows)]
fn install_file(path: &Path) -> Result<(), String> {
    let output = crate::util::hidden_command("certutil")
        .args(["-user", "-f", "-addstore", "Root"])
        .arg(path)
        .output()
        .map_err(|e| format!("E_CA_INSTALL: 调用 certutil 失败 {e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "E_CA_INSTALL: 安装证书失败 {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

#[cfg(not(windows))]
fn install_file(_path: &Path) -> Result<(), String> {
    Err("E_CA_INSTALL: 当前平台暂不支持一键安装，请手动导入证书".into())
}

pub fn export(data_dir: &Path, target: &Path) -> Result<(), String> {
    let material = ensure(data_dir)?;
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("E_CA: 创建导出目录失败 {e}"))?;
    }
    std::fs::write(target, material.cert_pem.as_bytes())
        .map_err(|e| format!("E_CA: 导出证书失败 {e}"))
}

pub fn authority(material: &CaMaterial) -> Result<RcgenAuthority, String> {
    let key_pair =
        KeyPair::from_pem(&material.key_pem).map_err(|e| format!("E_CA: 私钥无效 {e}"))?;
    let issuer = Issuer::from_ca_cert_pem(&material.cert_pem, key_pair)
        .map_err(|e| format!("E_CA: 证书无效 {e}"))?;
    Ok(RcgenAuthority::new(
        issuer,
        1024,
        hudsucker::rustls::crypto::aws_lc_rs::default_provider(),
    ))
}

fn write_meta(path: &Path, meta: &CaMeta) -> Result<(), String> {
    let text = serde_json::to_string_pretty(meta)
        .map_err(|e| format!("E_CA: 序列化证书元数据失败 {e}"))?;
    std::fs::write(path, text).map_err(|e| format!("E_CA: 写入证书元数据失败 {e}"))
}

fn read_meta(path: &Path) -> Option<CaMeta> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

fn expiry_ms() -> i64 {
    use chrono::{TimeZone, Utc};
    Utc.with_ymd_and_hms(2036, 1, 1, 0, 0, 0)
        .single()
        .map(|value| value.timestamp_millis())
        .unwrap_or(0)
}

fn file_created_ms(path: &Path) -> i64 {
    std::fs::metadata(path)
        .and_then(|meta| meta.created().or_else(|_| meta.modified()))
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

fn fingerprint_der(der: &[u8]) -> String {
    let digest = Sha256::digest(der);
    digest
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(":")
}

fn thumbprint_der(der: &[u8]) -> String {
    let digest = Sha1::digest(der);
    digest
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join("")
}

fn thumbprint_pem(pem: &str) -> Result<String, String> {
    Ok(thumbprint_der(&der_from_pem(pem)?))
}

fn is_trusted_pem(pem: &str) -> bool {
    match thumbprint_pem(pem) {
        Ok(thumbprint) => in_root_store(&thumbprint),
        Err(error) => {
            log::warn!("计算证书指纹失败：{error}");
            false
        }
    }
}

/// Windows 证书管理器的「指纹」就是 SHA-1；依次查当前用户、本机的 Root 库。
#[cfg(windows)]
fn in_root_store(thumbprint: &str) -> bool {
    for root in ["HKCU", "HKLM"] {
        let key =
            format!(r"{root}\Software\Microsoft\SystemCertificates\Root\Certificates\{thumbprint}");
        let found = crate::util::hidden_command("reg")
            .args(["query", &key])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        if matches!(found, Ok(status) if status.success()) {
            return true;
        }
    }
    // 某些根证书会进 ProtectedRoots，注册表里看不到，再用 certutil 兜底
    let user = crate::util::hidden_command("certutil")
        .args(["-user", "-store", "Root", thumbprint])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    if matches!(user, Ok(status) if status.success()) {
        return true;
    }
    let machine = crate::util::hidden_command("certutil")
        .args(["-store", "Root", thumbprint])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    matches!(machine, Ok(status) if status.success())
}

#[cfg(not(windows))]
fn in_root_store(_thumbprint: &str) -> bool {
    // macOS / Linux 的信任库差异很大，后续单独接入；当前明确返回未安装
    false
}

fn der_from_pem(pem: &str) -> Result<Vec<u8>, String> {
    use base64::Engine;
    let body: String = pem
        .lines()
        .filter(|line| !line.trim_start().starts_with("-----"))
        .collect::<Vec<_>>()
        .join("");
    base64::engine::general_purpose::STANDARD
        .decode(body.trim())
        .map_err(|e| format!("E_CA: 证书 PEM 解码失败 {e}"))
}

#[cfg(unix)]
fn restrict_key(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Err(error) = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)) {
        log::warn!("设置 CA 私钥权限失败：{error}");
    }
}

#[cfg(windows)]
fn restrict_key(path: &Path) {
    let Ok(user) = std::env::var("USERNAME") else {
        log::warn!("未能读取 USERNAME，跳过 CA 私钥 ACL 收紧");
        return;
    };
    let output = crate::util::hidden_command("icacls")
        .arg(path)
        .args(["/inheritance:r"])
        .arg("/grant:r")
        .arg(format!("{user}:F"))
        .output();
    match output {
        Ok(result) if result.status.success() => {}
        Ok(result) => log::warn!(
            "收紧 CA 私钥 ACL 失败：{}",
            String::from_utf8_lossy(&result.stderr).trim()
        ),
        Err(error) => log::warn!("调用 icacls 失败：{error}"),
    }
}
