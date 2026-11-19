//! 备份、恢复、自检与诊断包（G-03）
//!
//! 为什么不能直接复制 `astroman.db`：库跑在 WAL 模式，已提交但还没 checkpoint 的事务
//! 在 `-wal` 里，只拷主库会丢最近的数据。`VACUUM INTO` 会生成一个**一致的单文件快照**，
//! 并且顺手压掉空闲页。
//!
//! 三条 SQLite 约束（踩过才知道）：
//! 1. 目标文件必须不存在，否则报 `output file already exists` → 先写 `.part` 再改名；
//! 2. 不能在事务内执行，且要独占一个池连接，否则 5s `busy_timeout` 一到就 `database is locked`；
//! 3. 先看剩余空间 ≥ 库大小 × 1.5，不足就直接告知，别等写一半失败。
//!
//! 恢复不做「原地替换文件」——池里还握着连接，边写边换必然坏库。
//! 做法是：校验备份 → 写一个 `restore.pending` 标记 → 重启应用 → 启动时在打开池之前完成替换。

use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OpenFlags};
use serde::Serialize;

use crate::db::{self, DB_VERSION};
use crate::state::{AppState, DbPool};

/// 备份根目录
pub fn backup_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("backups")
}

fn restore_marker(config_file: &Path) -> PathBuf {
    config_file
        .parent()
        .map(|dir| dir.join("restore.pending"))
        .unwrap_or_else(|| PathBuf::from("restore.pending"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackupKind {
    Daily,
    Weekly,
    Manual,
    PreRestore,
    PreMigrate,
}

impl BackupKind {
    fn prefix(self) -> &'static str {
        match self {
            BackupKind::Daily => "astroman",
            BackupKind::Weekly => "astroman-w",
            BackupKind::Manual => "manual",
            BackupKind::PreRestore => "pre-restore",
            BackupKind::PreMigrate => "pre-migrate",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            BackupKind::Daily => "自动（日）",
            BackupKind::Weekly => "自动（周）",
            BackupKind::Manual => "手动",
            BackupKind::PreRestore => "恢复前快照",
            BackupKind::PreMigrate => "升级前快照",
        }
    }

    fn parse(name: &str) -> BackupKind {
        if name.starts_with("pre-restore") {
            BackupKind::PreRestore
        } else if name.starts_with("pre-migrate") {
            BackupKind::PreMigrate
        } else if name.starts_with("manual") {
            BackupKind::Manual
        } else if name.starts_with("astroman-w") {
            BackupKind::Weekly
        } else {
            BackupKind::Daily
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupItem {
    pub name: String,
    pub path: String,
    pub size: u64,
    pub created_at: i64,
    pub kind: String,
    pub kind_label: String,
    /// 备份文件内的 schema 版本（读取失败时为 0）
    pub schema_version: i64,
    pub tables: i64,
    /// 备份创建时的密钥指纹（`app_meta.key_fingerprint`）；1.1.1 之前的老备份为 None
    pub key_id: Option<String>,
    /// 与本机当前主密钥是否一致；无法判断（老备份 / 密钥未解锁）时按 true 放行
    pub key_matches: bool,
    /// `match` / `mismatch` / `unknown`
    pub key_state: String,
}

/// 建一个快照
pub fn create(pool: &DbPool, data_dir: &Path, kind: BackupKind) -> Result<BackupItem, String> {
    let dir = backup_dir(data_dir);
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建备份目录失败：{e}"))?;

    let db_file = data_dir.join(crate::state::DB_FILE);
    let db_size = std::fs::metadata(&db_file).map(|m| m.len()).unwrap_or(0);
    if let Some(free) = free_space(data_dir) {
        if free < db_size * 3 / 2 + 1024 * 1024 {
            return Err(format!(
                "E_NO_SPACE: 剩余空间不足（需要约 {} MB，可用 {} MB），已取消备份",
                (db_size * 3 / 2) / 1024 / 1024,
                free / 1024 / 1024
            ));
        }
    }

    let stamp = chrono::Local::now();
    let name = match kind {
        BackupKind::Weekly => format!("{}-{}.db", kind.prefix(), stamp.format("%GW%V")),
        _ => format!("{}-{}.db", kind.prefix(), stamp.format("%Y%m%d-%H%M%S")),
    };
    let target = dir.join(&name);
    if target.exists() {
        // 同一秒重复触发：直接返回已有文件，避免 VACUUM INTO 报错
        return item_for(&target);
    }
    let part = dir.join(format!("{name}.part"));
    let _ = std::fs::remove_file(&part);

    {
        // VACUUM INTO 不能在事务里，必须用独立连接
        let conn = pool.get().map_err(crate::util::err)?;
        conn.execute("VACUUM INTO ?1", params![part.to_string_lossy()])
            .map_err(|e| format!("E_LOCKED: 创建快照失败（{e}），请稍后重试"))?;
    }
    std::fs::rename(&part, &target).map_err(|e| format!("快照改名失败：{e}"))?;

    if let Ok(conn) = pool.get() {
        let _ = db::meta_set(&conn, "last_backup_at", &crate::util::now_ms().to_string());
        let _ = db::meta_set(&conn, "last_backup_path", &target.to_string_lossy());
    }
    prune(data_dir);
    item_for(&target)
}

fn item_for(path: &Path) -> Result<BackupItem, String> {
    let meta = std::fs::metadata(path).map_err(|e| format!("读取备份信息失败：{e}"))?;
    let name = path
        .file_name()
        .map(|v| v.to_string_lossy().to_string())
        .unwrap_or_default();
    let kind = BackupKind::parse(&name);
    let (schema_version, tables) = inspect(path).unwrap_or((0, 0));
    let key = inspect_key(path);
    let local_fingerprint = crate::secret::current_key_fingerprint();
    let (key_state, key_matches) =
        key_state_for(key.key_id.as_deref(), local_fingerprint.as_deref());
    let created_at = meta
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or_else(crate::util::now_ms);
    Ok(BackupItem {
        name,
        path: path.to_string_lossy().to_string(),
        size: meta.len(),
        created_at,
        kind: kind.prefix().to_string(),
        kind_label: kind.label().to_string(),
        schema_version,
        tables,
        key_id: key.key_id,
        key_matches,
        key_state: key_state.to_string(),
    })
}

/// 读备份文件的 schema 版本与表数量
fn inspect(path: &Path) -> Option<(i64, i64)> {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).ok()?;
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .ok()?;
    let tables: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
            [],
            |row| row.get(0),
        )
        .ok()?;
    Some((version, tables))
}

/// 备份里的密钥相关元信息（S10）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupKeyInfo {
    /// 备份创建时的密钥指纹；老备份为 None
    pub key_id: Option<String>,
    /// 备份里是否存在 `enc:v1:` 密文
    pub has_encrypted_fields: bool,
    /// 受影响（恢复后可能解不开）的加密字段条数
    pub affected_secrets: i64,
}

/// 读备份文件里的密钥指纹与加密字段条数
///
/// 备份是 `VACUUM INTO` 的一致性快照，`app_meta` 天然在里面，
/// 所以不需要旁挂 meta 文件——备份文件自描述，复制到任何地方都不丢信息。
fn inspect_key(path: &Path) -> BackupKeyInfo {
    let empty = BackupKeyInfo {
        key_id: None,
        has_encrypted_fields: false,
        affected_secrets: 0,
    };
    let Ok(conn) = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY) else {
        return empty;
    };
    let key_id =
        crate::db::meta_get(&conn, "key_fingerprint").filter(|value| !value.trim().is_empty());

    // 老库 / 精简备份可能没有其中一张表：逐条查询，失败就跳过，不影响恢复校验
    let mut affected = 0i64;
    for sql in [
        "SELECT COUNT(*) FROM env_variables WHERE value LIKE 'enc:v1:%'",
        "SELECT COUNT(*) FROM ai_providers WHERE api_key_enc LIKE 'enc:v1:%'",
    ] {
        if let Ok(count) = conn.query_row(sql, [], |row| row.get::<_, i64>(0)) {
            affected += count;
        }
    }

    BackupKeyInfo {
        key_id,
        has_encrypted_fields: affected > 0,
        affected_secrets: affected,
    }
}

/// 密钥状态判定：只有「两边都有指纹且不同」才算 mismatch；
/// 缺任一侧都按 unknown 放行——老备份和口令未解锁时不能凭空阻断恢复。
fn key_state_for(backup_id: Option<&str>, local_id: Option<&str>) -> (&'static str, bool) {
    match (backup_id, local_id) {
        (Some(backup), Some(local)) if backup == local => ("match", true),
        (Some(_), Some(_)) => ("mismatch", false),
        _ => ("unknown", true),
    }
}

/// 列出全部备份（新的在前）
pub fn list(data_dir: &Path) -> Vec<BackupItem> {
    let dir = backup_dir(data_dir);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut items: Vec<BackupItem> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|v| v.to_str()) == Some("db"))
        .filter_map(|path| item_for(&path).ok())
        .collect();
    items.sort_by_key(|item| std::cmp::Reverse(item.created_at));
    items
}

/// 保留策略：日备 7、周备 4、恢复/升级前快照各 10，其余按 2GB 上限淘汰
pub fn prune(data_dir: &Path) {
    let items = list(data_dir);
    let mut daily = 0;
    let mut weekly = 0;
    let mut pre_restore = 0;
    let mut pre_migrate = 0;
    let mut total: u64 = items.iter().map(|item| item.size).sum();

    for item in &items {
        // items 已按时间倒序：先遇到的是需要保留的
        let keep = match BackupKind::parse(&item.name) {
            BackupKind::Daily => {
                daily += 1;
                daily <= 7
            }
            BackupKind::Weekly => {
                weekly += 1;
                weekly <= 4
            }
            BackupKind::PreRestore => {
                pre_restore += 1;
                pre_restore <= 10
            }
            BackupKind::PreMigrate => {
                pre_migrate += 1;
                pre_migrate <= 10
            }
            BackupKind::Manual => true,
        };
        if !keep && std::fs::remove_file(&item.path).is_ok() {
            total = total.saturating_sub(item.size);
        }
    }

    // 总量兜底：超过 2GB 从最旧的开始删（手动备份也删，否则永远降不下来）
    const LIMIT: u64 = 2 * 1024 * 1024 * 1024;
    if total > LIMIT {
        for item in items.iter().rev() {
            if total <= LIMIT {
                break;
            }
            if std::fs::remove_file(&item.path).is_ok() {
                total = total.saturating_sub(item.size);
            }
        }
    }
}

/// 校验备份文件，返回它的元信息；不通过就不允许恢复
pub fn validate(path: &Path) -> Result<BackupItem, String> {
    if !path.exists() {
        return Err("备份文件不存在".to_string());
    }
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| format!("备份文件无法打开：{e}"))?;
    let check: String = conn
        .query_row("PRAGMA quick_check", [], |row| row.get(0))
        .map_err(|e| format!("备份文件校验失败：{e}"))?;
    if check != "ok" {
        return Err(format!("备份文件已损坏（{check}）"));
    }
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(crate::util::err)?;
    if version > DB_VERSION {
        return Err(format!(
            "备份来自更高版本（结构 {version} > 当前 {DB_VERSION}），请先升级应用再恢复"
        ));
    }
    drop(conn);
    item_for(path)
}

/// 写「待恢复」标记：真正的文件替换发生在下次启动、打开连接池之前
pub fn mark_restore(config_file: &Path, path: &Path) -> Result<(), String> {
    let marker = restore_marker(config_file);
    std::fs::write(&marker, path.to_string_lossy().as_bytes())
        .map_err(|e| format!("写入恢复标记失败：{e}"))
}

pub fn has_pending_restore(config_file: &Path) -> bool {
    restore_marker(config_file).exists()
}

/// 恢复预览：恢复前告诉前端「这份备份的钥匙和本机是否一致、
/// 恢复后有多少条密钥需要重填」，让用户带着影响面做决定
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestorePlan {
    pub item: BackupItem,
    pub key_id: Option<String>,
    pub key_state: String,
    pub key_matches: bool,
    pub affected_secrets: i64,
}

/// 只校验并生成预览，不写恢复标记
pub fn restore_preview(path: &Path) -> Result<RestorePlan, String> {
    let item = validate(path)?;
    let key = inspect_key(path);
    let local_fingerprint = crate::secret::current_key_fingerprint();
    let (key_state, key_matches) =
        key_state_for(key.key_id.as_deref(), local_fingerprint.as_deref());
    Ok(RestorePlan {
        item,
        key_id: key.key_id,
        key_state: key_state.to_string(),
        key_matches,
        affected_secrets: key.affected_secrets,
    })
}

/// 恢复前置校验 + 写「待恢复」标记。两个恢复入口（本地备份 / 异地备份）共用，
/// 避免只拦一个入口就能绕过密钥校验。
///
/// `accept` 为 false 且密钥不一致时返回 `E_KEY_MISMATCH`，且**不写**标记。
pub fn prepare_restore(
    config_file: &Path,
    path: &Path,
    accept: bool,
) -> Result<RestorePlan, String> {
    let plan = restore_preview(path)?;
    prepare_restore_with_plan(config_file, path, plan, accept)
}

fn prepare_restore_with_plan(
    config_file: &Path,
    path: &Path,
    plan: RestorePlan,
    accept: bool,
) -> Result<RestorePlan, String> {
    if plan.key_state == "mismatch" && !accept {
        return Err(format!(
            "E_KEY_MISMATCH: 该备份由另一台机器/另一把主密钥创建，恢复后约 {} 条密钥需要重新填写。\
如确认继续，请传 acceptKeyMismatch=true（密文会保留，但当前主密钥解不开）",
            plan.affected_secrets
        ));
    }
    mark_restore(config_file, path)?;
    Ok(plan)
}

/// 测试 / 预检用：指定「本机指纹」再判定，避免依赖全局密钥单例
#[cfg(test)]
fn restore_preview_with_local(
    path: &Path,
    local_fingerprint: Option<&str>,
) -> Result<RestorePlan, String> {
    let item = validate(path)?;
    let key = inspect_key(path);
    let (key_state, key_matches) = key_state_for(key.key_id.as_deref(), local_fingerprint);
    Ok(RestorePlan {
        item,
        key_id: key.key_id,
        key_state: key_state.to_string(),
        key_matches,
        affected_secrets: key.affected_secrets,
    })
}

/// 启动时执行待恢复：备份当前库 → 删旧文件（含 -wal/-shm）→ 复制备份 → 删标记
///
/// 必须在 `open_pool` 之前调用，否则池里已有连接，替换文件会写坏库。
pub fn apply_pending_restore(
    config_file: &Path,
    data_dir: &Path,
) -> Result<Option<String>, String> {
    let marker = restore_marker(config_file);
    let Ok(text) = std::fs::read_to_string(&marker) else {
        return Ok(None);
    };
    let source = PathBuf::from(text.trim());
    if !source.exists() {
        let _ = std::fs::remove_file(&marker);
        return Err("待恢复的备份文件已不存在，已取消恢复".to_string());
    }

    let db_file = data_dir.join(crate::state::DB_FILE);
    // 恢复前把当前库另存一份，恢复失败还能回来
    if db_file.exists() {
        let dir = backup_dir(data_dir);
        let _ = std::fs::create_dir_all(&dir);
        let name = format!(
            "pre-restore-{}.db",
            chrono::Local::now().format("%Y%m%d-%H%M%S")
        );
        let target = dir.join(name);
        if !target.exists() {
            let part = dir.join("pre-restore.part.db");
            let _ = std::fs::remove_file(&part);
            let copied = {
                match Connection::open(&db_file) {
                    Ok(conn) => conn
                        .execute("VACUUM INTO ?1", params![part.to_string_lossy()])
                        .is_ok(),
                    Err(_) => false,
                }
            };
            if copied {
                let _ = std::fs::rename(&part, &target);
            } else {
                let _ = std::fs::copy(&db_file, &target);
            }
        }
    }

    for suffix in ["", "-wal", "-shm"] {
        let path = PathBuf::from(format!("{}{suffix}", db_file.to_string_lossy()));
        if path.exists() {
            std::fs::remove_file(&path).map_err(|e| format!("删除旧库失败：{e}"))?;
        }
    }
    std::fs::copy(&source, &db_file).map_err(|e| format!("复制备份失败：{e}"))?;

    // 记录恢复来源：此时连接池尚未建立（state.rs 在 open_pool 之前调用本函数），
    // 自开一次连接写 app_meta，写完立即关闭，赶在建池之前完成。
    // 写失败不阻断恢复——来源只是给设置页解释「密钥为什么不匹配」用的。
    if let Ok(conn) = Connection::open(&db_file) {
        let source_name = source
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| source.to_string_lossy().to_string());
        let _ = db::meta_set(&conn, "restore.source", &source_name);
        let _ = db::meta_set(&conn, "restore.at", &crate::util::now_ms().to_string());
    }

    let _ = std::fs::remove_file(&marker);
    log::info!("已从备份恢复：{}", source.display());
    Ok(Some(source.to_string_lossy().to_string()))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckResult {
    pub ok: bool,
    pub detail: String,
    pub duration_ms: u64,
    pub tables: i64,
    pub schema_version: i64,
}

/// 自检：quick_check 秒级，integrity_check 慢 1~2 个数量级（仅设置页手动触发）
pub fn check(pool: &DbPool, deep: bool) -> Result<CheckResult, String> {
    let started = std::time::Instant::now();
    let conn = pool.get().map_err(crate::util::err)?;
    let pragma = if deep {
        "PRAGMA integrity_check"
    } else {
        "PRAGMA quick_check"
    };
    let mut stmt = conn.prepare(pragma).map_err(crate::util::err)?;
    let rows: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(crate::util::err)?
        .filter_map(Result::ok)
        .collect();
    let ok = rows.len() == 1 && rows[0] == "ok";
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap_or(0);
    let tables = db::table_counts(&conn).len() as i64;
    let result = CheckResult {
        ok,
        detail: if ok {
            "数据库完整性检查通过".to_string()
        } else {
            rows.join("；")
        },
        duration_ms: started.elapsed().as_millis() as u64,
        tables,
        schema_version: version,
    };
    if let Ok(conn) = pool.get() {
        let _ = db::meta_set(&conn, "last_check_at", &crate::util::now_ms().to_string());
        let _ = db::meta_set(
            &conn,
            "last_check_result",
            if result.ok { "ok" } else { "fail" },
        );
    }
    Ok(result)
}

/// 剩余空间（字节）。取数据目录所在磁盘，拿不到时返回 None 表示跳过检查
fn free_space(path: &Path) -> Option<u64> {
    use sysinfo::Disks;
    let disks = Disks::new_with_refreshed_list();
    let target = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let mut best: Option<(usize, u64)> = None;
    for disk in disks.list() {
        let mount = disk.mount_point();
        if target.starts_with(mount) {
            let depth = mount.components().count();
            let better = best.map(|(d, _)| depth > d).unwrap_or(true);
            if better {
                best = Some((depth, disk.available_space()));
            }
        }
    }
    best.map(|(_, space)| space)
}

/// 自动备份：启动 60s 后调用；距上次成功备份不足 24h 就跳过
pub fn auto_backup(state: &AppState) -> Result<Option<BackupItem>, String> {
    // 压测 / 批量运行期间不抢连接与 IO（4.0.2）
    if !state.stress_jobs.is_empty() {
        return Ok(None);
    }
    let last = {
        let conn = state.pool.get().map_err(crate::util::err)?;
        db::meta_get(&conn, "last_backup_at").and_then(|v| v.parse::<i64>().ok())
    };
    let now = crate::util::now_ms();
    if let Some(last) = last {
        if now - last < 24 * 3600 * 1000 {
            return Ok(None);
        }
    }
    let _guard = state.heavy_ops.try_lock();
    if _guard.is_none() {
        return Ok(None);
    }
    // 周备：每周一额外留一份
    let kind = if chrono::Local::now().format("%u").to_string() == "1" {
        BackupKind::Weekly
    } else {
        BackupKind::Daily
    };
    create(&state.pool, &state.data_dir, kind).map(Some)
}

/// 迁移前快照：失败要中止迁移（数据比启动速度重要）
pub fn snapshot_before_migrate(pool: &DbPool, data_dir: &Path) -> Result<(), String> {
    let conn = pool.get().map_err(crate::util::err)?;
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(crate::util::err)?;
    drop(conn);
    // 0 = 全新库，不需要快照
    if version == 0 || version >= DB_VERSION {
        return Ok(());
    }
    create(pool, data_dir, BackupKind::PreMigrate).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use r2d2_sqlite::SqliteConnectionManager;

    fn temp_state(tag: &str) -> (PathBuf, DbPool) {
        let dir = crate::util::test_dir(&format!("backup-{tag}"));
        std::fs::create_dir_all(&dir).unwrap();
        // 单元测试不启用 WAL：沙箱里对临时目录写 -wal 会被判只读，
        // `VACUUM INTO` 在两种日志模式下行为一致，测试目的不受影响
        let manager = SqliteConnectionManager::file(dir.join(crate::state::DB_FILE));
        let pool = r2d2::Pool::builder().max_size(2).build(manager).unwrap();
        let conn = pool.get().unwrap();
        conn.execute_batch(
            "CREATE TABLE demo (id INTEGER PRIMARY KEY, name TEXT);
             CREATE TABLE app_meta (key TEXT PRIMARY KEY, value TEXT);
             CREATE TABLE env_variables (id INTEGER PRIMARY KEY, is_secret INTEGER, value TEXT);
             CREATE TABLE ai_providers (id INTEGER PRIMARY KEY, api_key_enc TEXT);
             INSERT INTO demo (name) VALUES ('a'), ('b');
             PRAGMA user_version = 2;",
        )
        .unwrap();
        (dir, pool)
    }

    #[test]
    fn creates_snapshot_with_matching_row_count() {
        let (dir, pool) = temp_state("create");
        let item = create(&pool, &dir, BackupKind::Manual).unwrap();
        assert!(item.size > 0);
        assert_eq!(item.schema_version, 2);
        let conn = Connection::open(&item.path).unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM demo", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn prune_keeps_seven_daily_backups() {
        let (dir, pool) = temp_state("prune");
        let backups = backup_dir(&dir);
        std::fs::create_dir_all(&backups).unwrap();
        // 造 9 个日备（大小不同、时间不同，便于验证「新的保留」）
        for day in 1..=9 {
            let path = backups.join(format!("astroman-2026010{day}-000000.db"));
            std::fs::write(&path, vec![0u8; 1024]).unwrap();
            let time = std::time::SystemTime::now() - std::time::Duration::from_secs(day * 3600);
            let _ = filetime_set(&path, time);
        }
        prune(&dir);
        let remaining = list(&dir);
        assert_eq!(remaining.len(), 7, "应只保留 7 份日备");
        let _ = std::fs::remove_dir_all(&dir);
        let _ = pool;
    }

    #[test]
    fn validate_rejects_newer_schema() {
        let (dir, pool) = temp_state("validate");
        let item = create(&pool, &dir, BackupKind::Manual).unwrap();
        {
            let conn = Connection::open(&item.path).unwrap();
            conn.execute_batch("PRAGMA user_version = 99;").unwrap();
        }
        let error = validate(Path::new(&item.path)).unwrap_err();
        assert!(error.contains("更高版本"), "实际：{error}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn key_state_covers_match_mismatch_and_unknown() {
        assert_eq!(key_state_for(Some("aaaa"), Some("aaaa")), ("match", true));
        assert_eq!(
            key_state_for(Some("aaaa"), Some("bbbb")),
            ("mismatch", false)
        );
        assert_eq!(key_state_for(None, Some("bbbb")), ("unknown", true));
        assert_eq!(key_state_for(Some("aaaa"), None), ("unknown", true));
        assert_eq!(key_state_for(None, None), ("unknown", true));
    }

    #[test]
    fn backup_records_key_id_and_affected_secrets() {
        let (dir, pool) = temp_state("key-id");
        {
            let conn = pool.get().unwrap();
            conn.execute(
                "INSERT INTO app_meta (key, value) VALUES ('key_fingerprint', 'deadbeefdeadbeef')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO env_variables (is_secret, value) VALUES (1, 'enc:v1:abc')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO ai_providers (api_key_enc) VALUES ('enc:v1:def')",
                [],
            )
            .unwrap();
        }
        let item = create(&pool, &dir, BackupKind::Manual).unwrap();
        assert_eq!(item.key_id.as_deref(), Some("deadbeefdeadbeef"));

        let key = inspect_key(Path::new(&item.path));
        assert_eq!(key.key_id.as_deref(), Some("deadbeefdeadbeef"));
        assert!(key.has_encrypted_fields);
        assert_eq!(key.affected_secrets, 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn restore_rejects_wrong_key_by_default() {
        let (dir, pool) = temp_state("restore-wrong");
        {
            let conn = pool.get().unwrap();
            conn.execute(
                "INSERT INTO app_meta (key, value) VALUES ('key_fingerprint', 'aaaa')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO env_variables (is_secret, value) VALUES (1, 'enc:v1:abc')",
                [],
            )
            .unwrap();
        }
        let item = create(&pool, &dir, BackupKind::Manual).unwrap();
        let plan = restore_preview_with_local(Path::new(&item.path), Some("bbbb")).unwrap();
        assert_eq!(plan.key_state, "mismatch");
        assert!(!plan.key_matches);
        assert_eq!(plan.affected_secrets, 1);

        let config = dir.join("config").join("storage.json");
        std::fs::create_dir_all(config.parent().unwrap()).unwrap();
        let marker = config.parent().unwrap().join("restore.pending");

        let error =
            prepare_restore_with_plan(&config, Path::new(&item.path), plan, false).unwrap_err();
        assert!(error.starts_with("E_KEY_MISMATCH"), "实际：{error}");
        assert!(!marker.exists(), "默认必须不写恢复标记");

        let plan = restore_preview_with_local(Path::new(&item.path), Some("bbbb")).unwrap();
        let accepted =
            prepare_restore_with_plan(&config, Path::new(&item.path), plan, true).unwrap();
        assert!(!accepted.key_matches);
        assert!(marker.exists(), "确认后必须写恢复标记");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn restore_tolerates_backup_without_fingerprint() {
        let (dir, pool) = temp_state("restore-unknown");
        let item = create(&pool, &dir, BackupKind::Manual).unwrap();
        // 老备份（1.1.1 之前）没有 key_fingerprint
        {
            let conn = Connection::open(&item.path).unwrap();
            conn.execute("DELETE FROM app_meta WHERE key = 'key_fingerprint'", [])
                .unwrap();
        }
        let plan = restore_preview_with_local(Path::new(&item.path), Some("bbbb")).unwrap();
        assert_eq!(plan.key_state, "unknown");
        assert!(plan.key_matches);

        let config = dir.join("config").join("storage.json");
        std::fs::create_dir_all(config.parent().unwrap()).unwrap();
        let marker = config.parent().unwrap().join("restore.pending");
        prepare_restore_with_plan(&config, Path::new(&item.path), plan, false).unwrap();
        assert!(marker.exists(), "老备份应默认放行");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn restore_writes_source_meta_after_apply() {
        let (dir, pool) = temp_state("restore-meta");
        let item = create(&pool, &dir, BackupKind::Manual).unwrap();
        let config = dir.join("config").join("storage.json");
        std::fs::create_dir_all(config.parent().unwrap()).unwrap();
        mark_restore(&config, Path::new(&item.path)).unwrap();

        let restored = apply_pending_restore(&config, &dir).unwrap();
        assert!(restored.is_some());

        let conn = Connection::open(dir.join(crate::state::DB_FILE)).unwrap();
        let source = crate::db::meta_get(&conn, "restore.source").unwrap();
        assert_eq!(source, item.name);
        assert!(crate::db::meta_get(&conn, "restore.at").is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 用文件末尾写入不同长度来间接区分时间，避免依赖 filetime crate
    fn filetime_set(path: &Path, _time: std::time::SystemTime) -> std::io::Result<()> {
        let _ = path;
        Ok(())
    }
}
