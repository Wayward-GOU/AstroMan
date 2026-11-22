//! 参数 / Token 池（v1.1.2 W6）
//!
//! 负责池的读取、导入与运行期切片。池值只在准备阶段进入内存，
//! 报告与日志只保存池名与统计，不保存 token 明文。

use std::collections::{HashMap, HashSet};

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::models::KeyValueItem;
use crate::services::cluster::protocol::PoolItem;

/// 结构化池错误码（第三批 11）：前端按前缀识别并给出针对性引导
pub const E_POOL_EMPTY: &str = "POOL_EMPTY";
pub const E_POOL_BINDING_INVALID: &str = "POOL_BINDING_INVALID";
/// 同一个变量被多个池绑定：运行期后写入者会静默覆盖先写入者，按配置错误拦下
///
/// 命名与技术方案 `docs/v1.1.2-tech-plan.md` 的池问题码表保持一致。
pub const E_POOL_VAR_CONFLICT: &str = "POOL_VAR_CONFLICT";
pub const E_POOL_SHARD_EMPTY: &str = "POOL_SHARD_EMPTY";
pub const E_POOL_SLICE_GAP: &str = "POOL_SLICE_GAP";
pub const E_POOL_SLICE_TOO_LARGE: &str = "POOL_SLICE_TOO_LARGE";
pub const E_POOL_REFRESH_FAILED: &str = "POOL_REFRESH_FAILED";

/// 单个池允许的最大列数（v20）
pub const MAX_POOL_COLUMNS: usize = 24;
/// 单个列名的最大字符数（v20）
pub const MAX_POOL_COLUMN_LEN: usize = 32;

/// 池默认列：token 池为 token/account，其他为 value
pub fn default_columns(kind: &str) -> Vec<String> {
    if kind.eq_ignore_ascii_case("token") {
        vec!["token".to_string(), "account".to_string()]
    } else {
        vec!["value".to_string()]
    }
}

/// 列名是否合法：字母或下划线开头，后续只能是字母、数字、下划线
fn is_valid_column_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) if first.is_ascii_alphabetic() || first == '_' => {}
        _ => return false,
    }
    chars.all(|item| item.is_ascii_alphanumeric() || item == '_')
}

/// 校验并规范化列名：trim、去重、限制数量与字符集
pub fn normalize_columns(input: &[String]) -> Result<Vec<String>, String> {
    let mut result: Vec<String> = Vec::new();
    for (index, raw) in input.iter().enumerate() {
        let name = raw.trim();
        if name.is_empty() {
            return Err(format!("第 {} 列名称为空", index + 1));
        }
        if name.chars().count() > MAX_POOL_COLUMN_LEN {
            return Err(format!("列「{name}」超过 {MAX_POOL_COLUMN_LEN} 个字符"));
        }
        if !is_valid_column_name(name) {
            return Err(format!(
                "列「{name}」不合法：只能包含字母、数字、下划线，且不能以数字开头"
            ));
        }
        if !result.iter().any(|item| item == name) {
            result.push(name.to_string());
        }
    }
    if result.is_empty() {
        return Err("至少需要 1 列".to_string());
    }
    if result.len() > MAX_POOL_COLUMNS {
        return Err(format!("最多支持 {MAX_POOL_COLUMNS} 列"));
    }
    Ok(result)
}

/// 旧池兼容列：v20 之前固定为 值 + 账号
pub const LEGACY_COLUMNS: [&str; 2] = ["value", "account"];

/// 解析 columns_json；为空时按旧版固定列处理，解析失败时回退到池默认列
pub fn parse_columns_json(raw: &str, kind: &str) -> Vec<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed == "[]" {
        return LEGACY_COLUMNS.iter().map(|item| item.to_string()).collect();
    }
    match serde_json::from_str::<Vec<String>>(trimmed) {
        Ok(list) if !list.is_empty() => {
            normalize_columns(&list).unwrap_or_else(|_| default_columns(kind))
        }
        _ => default_columns(kind),
    }
}

/// 按名称读取池列；池不存在时报错
pub fn pool_columns(conn: &Connection, name: &str) -> Result<Vec<String>, String> {
    let name = name.trim();
    let row = conn
        .query_row(
            "SELECT kind, columns_json FROM data_pools WHERE name = ?1",
            params![name],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()
        .map_err(|error| format!("读取池列失败：{error}"))?;
    let (kind, raw) = row.ok_or_else(|| format!("{E_POOL_BINDING_INVALID}: 池「{name}」不存在"))?;
    Ok(parse_columns_json(&raw, &kind))
}

/// 按 id 读取池列；池不存在时报错
pub fn pool_columns_by_id(conn: &Connection, pool_id: i64) -> Result<Vec<String>, String> {
    let row = conn
        .query_row(
            "SELECT kind, columns_json FROM data_pools WHERE id = ?1",
            params![pool_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()
        .map_err(|error| format!("读取池列失败：{error}"))?;
    let (kind, raw) = row.ok_or_else(|| format!("池不存在：{pool_id}"))?;
    Ok(parse_columns_json(&raw, &kind))
}

/// 解析整行 data_json；空或非法时返回空表
fn parse_data_json(raw: &str) -> HashMap<String, String> {
    serde_json::from_str::<HashMap<String, String>>(raw.trim()).unwrap_or_default()
}

/// 按池列合成整行数据：首列取 value，account 列取 account
fn synthesize_data(
    columns: &[String],
    value: &str,
    account: Option<&str>,
) -> HashMap<String, String> {
    let mut data = HashMap::new();
    for column in columns {
        data.insert(column.clone(), String::new());
    }
    if let Some(first) = columns.first() {
        data.insert(first.clone(), value.to_string());
    }
    if columns.iter().any(|column| column == "account") {
        data.insert(
            "account".to_string(),
            account.unwrap_or_default().to_string(),
        );
    }
    data
}

/// Token 刷新配置（8.6）：`mode = http` 时按配置调用刷新接口
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PoolRefreshConfig {
    /// none / http
    pub mode: String,
    /// 默认 POST
    pub method: String,
    /// 刷新地址，支持环境变量占位符
    pub url: String,
    /// 请求头
    pub headers: Vec<KeyValueItem>,
    /// 请求体，支持 `{{token}}` / `{{refreshToken}}` / `{{account}}`
    pub body: String,
    /// 响应 JSON 路径，如 `$.data.accessToken`
    pub token_path: String,
    /// 响应 JSON 中的过期时间路径，如 `$.data.expiresIn`（秒或毫秒）
    pub expires_path: String,
    /// 响应缺少过期时间时的兜底有效期（秒），0 = 不过期
    pub expires_in_sec: i64,
}

impl PoolRefreshConfig {
    /// 是否启用 HTTP 刷新
    pub fn is_http(&self) -> bool {
        self.mode.eq_ignore_ascii_case("http") && !self.url.trim().is_empty()
    }
}

/// 池摘要（列表页）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PoolRow {
    pub id: i64,
    pub name: String,
    /// parameter / token
    pub kind: String,
    /// 默认脱敏展示
    pub sensitive: bool,
    pub description: String,
    pub item_count: i64,
    pub enabled_count: i64,
    /// 健康度分项（管理页列表展示）
    pub active_count: i64,
    pub invalid_count: i64,
    pub expired_count: i64,
    pub cooling_count: i64,
    pub last_used_at: i64,
    pub created_at: i64,
    pub updated_at: i64,
    /// 自定义列（v20）
    pub columns: Vec<String>,
}

/// 池健康度汇总
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PoolHealth {
    pub total: i64,
    pub active: i64,
    pub invalid: i64,
    pub expired: i64,
    pub cooling: i64,
    pub disabled: i64,
}

/// 管理页条目视图：敏感池的 value 始终是掩码
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PoolItemView {
    pub id: i64,
    /// 在池内的序号（从 1 开始，按页偏移）
    pub index: i64,
    pub value: String,
    /// value 是否为掩码
    pub masked: bool,
    pub account: Option<String>,
    pub weight: f64,
    /// 整行数据（v20）：敏感池已逐值脱敏
    pub data: HashMap<String, String>,
    pub enabled: bool,
    pub status: String,
    pub expires_at: i64,
    pub fail_count: i64,
    pub last_used_at: i64,
    pub created_at: i64,
}

/// 管理页分页结果
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PoolItemsPage {
    pub items: Vec<PoolItemView>,
    pub total: i64,
    pub page: i64,
    pub page_size: i64,
    pub health: PoolHealth,
}

/// 池条目（管理页）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PoolItemRow {
    pub id: i64,
    pub value: String,
    pub account: Option<String>,
    pub weight: f64,
    /// 整行数据（v20）
    pub data: HashMap<String, String>,
    pub enabled: bool,
    pub created_at: i64,
}

/// 单条明文（含整行数据），仅管理页二次确认后返回
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RevealedItem {
    pub id: i64,
    pub value: String,
    pub data: HashMap<String, String>,
}

pub fn now_ms() -> i64 {
    crate::util::now_ms()
}

/// 池列表（含健康度分项）
pub fn list_pools(conn: &Connection) -> Result<Vec<PoolRow>, String> {
    let now = now_ms();
    let mut stmt = conn
        .prepare(
            "SELECT p.id, p.name, p.kind, p.sensitive, p.description, p.created_at, p.updated_at,
                    COUNT(i.id) AS item_count,
                    COALESCE(SUM(CASE WHEN i.enabled = 1 THEN 1 ELSE 0 END), 0) AS enabled_count,
                    COALESCE(SUM(CASE WHEN i.enabled = 1 AND TRIM(i.value) <> '' AND i.fail_count < 5
                        AND (i.expires_at = 0 OR i.expires_at > ?1)
                        AND (i.cooldown_until = 0 OR i.cooldown_until <= ?1) THEN 1 ELSE 0 END), 0) AS active_count,
                    COALESCE(SUM(CASE WHEN i.enabled = 1 AND (TRIM(i.value) = '' OR i.fail_count >= 5)
                        THEN 1 ELSE 0 END), 0) AS invalid_count,
                    COALESCE(SUM(CASE WHEN i.enabled = 1 AND TRIM(i.value) <> '' AND i.fail_count < 5
                        AND i.expires_at > 0 AND i.expires_at <= ?1 THEN 1 ELSE 0 END), 0) AS expired_count,
                    COALESCE(SUM(CASE WHEN i.enabled = 1 AND TRIM(i.value) <> '' AND i.fail_count < 5
                        AND (i.expires_at = 0 OR i.expires_at > ?1)
                        AND i.cooldown_until > ?1 THEN 1 ELSE 0 END), 0) AS cooling_count,
                    COALESCE(MAX(i.last_used_at), 0) AS last_used_at,
                    p.columns_json
             FROM data_pools p
             LEFT JOIN data_pool_items i ON i.pool_id = p.id
             GROUP BY p.id
             ORDER BY p.updated_at DESC, p.id DESC",
        )
        .map_err(|error| format!("读取池列表失败：{error}"))?;
    let rows = stmt
        .query_map(params![now], |row| {
            let kind: String = row.get(2)?;
            let raw_columns: String = row.get(14)?;
            Ok(PoolRow {
                id: row.get(0)?,
                name: row.get(1)?,
                kind: kind.clone(),
                sensitive: row.get::<_, i64>(3)? != 0,
                description: row.get(4)?,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
                item_count: row.get(7)?,
                enabled_count: row.get(8)?,
                active_count: row.get(9)?,
                invalid_count: row.get(10)?,
                expired_count: row.get(11)?,
                cooling_count: row.get(12)?,
                last_used_at: row.get(13)?,
                columns: parse_columns_json(&raw_columns, &kind),
            })
        })
        .map_err(|error| format!("读取池列表失败：{error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("读取池列表失败：{error}"))?;
    Ok(rows)
}

/// 按名称查池 id
pub fn pool_id_by_name(conn: &Connection, name: &str) -> Result<Option<i64>, String> {
    conn.query_row(
        "SELECT id FROM data_pools WHERE name = ?1",
        params![name.trim()],
        |row| row.get(0),
    )
    .optional()
    .map_err(|error| format!("查询池失败：{error}"))
}

/// 池类型（parameter / token）；不存在时按 parameter
pub fn pool_kind(conn: &Connection, name: &str) -> String {
    conn.query_row(
        "SELECT kind FROM data_pools WHERE name = ?1",
        params![name.trim()],
        |row| row.get::<_, String>(0),
    )
    .optional()
    .ok()
    .flatten()
    .unwrap_or_else(|| "parameter".to_string())
}

/// 读取指定池的全部启用条目
pub fn load_items(conn: &Connection, name: &str) -> Result<Vec<PoolItem>, String> {
    let Some(pool_id) = pool_id_by_name(conn, name)? else {
        return Ok(Vec::new());
    };
    let columns = pool_columns(conn, name)?;
    let mut stmt = conn
        .prepare(
            "SELECT id, value, account, weight, expires_at, data_json FROM data_pool_items
             WHERE pool_id = ?1 AND enabled = 1 ORDER BY id ASC",
        )
        .map_err(|error| format!("读取池条目失败：{error}"))?;
    let rows = stmt
        .query_map(params![pool_id], |row| {
            let value: String = row.get(1)?;
            let account: Option<String> = row.get(2)?;
            let raw_data: String = row.get(5)?;
            let mut data = parse_data_json(&raw_data);
            if data.is_empty() {
                data = synthesize_data(&columns, &value, account.as_deref());
            }
            Ok(PoolItem {
                id: row.get(0)?,
                value,
                account,
                weight: row.get(3)?,
                expires_at: row.get(4)?,
                data,
            })
        })
        .map_err(|error| format!("读取池条目失败：{error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("读取池条目失败：{error}"))?;
    Ok(rows)
}

/// 读取池的 Token 刷新配置（仅返回启用 http 刷新的池）
pub fn pool_refresh_configs(
    conn: &Connection,
    names: &[String],
) -> Result<HashMap<String, PoolRefreshConfig>, String> {
    let mut result = HashMap::new();
    for name in names {
        let name = name.trim();
        if name.is_empty() {
            continue;
        }
        let row = conn
            .query_row(
                "SELECT refresh_mode, refresh_json FROM data_pools WHERE name = ?1",
                params![name],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()
            .map_err(|error| format!("读取池刷新配置失败：{error}"))?;
        let Some((mode, raw)) = row else {
            continue;
        };
        if !mode.eq_ignore_ascii_case("http") {
            continue;
        }
        let mut config: PoolRefreshConfig = serde_json::from_str(&raw).unwrap_or_default();
        config.mode = mode;
        if config.method.trim().is_empty() {
            config.method = "POST".to_string();
        }
        result.insert(name.to_string(), config);
    }
    Ok(result)
}

/// 读取池的 Token 刷新配置（管理页回显；未配置返回 `mode = none`）
pub fn load_refresh_config(conn: &Connection, name: &str) -> Result<PoolRefreshConfig, String> {
    let row = conn
        .query_row(
            "SELECT refresh_mode, refresh_json FROM data_pools WHERE name = ?1",
            params![name.trim()],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()
        .map_err(|error| format!("读取池刷新配置失败：{error}"))?;
    let Some((mode, raw)) = row else {
        return Err(format!("{E_POOL_BINDING_INVALID}: 池「{name}」不存在"));
    };
    let mut config: PoolRefreshConfig = serde_json::from_str(&raw).unwrap_or_default();
    config.mode = if mode.eq_ignore_ascii_case("http") {
        "http".to_string()
    } else {
        "none".to_string()
    };
    Ok(config)
}

/// 保存池的 Token 刷新配置
pub fn save_refresh_config(
    conn: &Connection,
    name: &str,
    config: &PoolRefreshConfig,
) -> Result<(), String> {
    let mode = if config.is_http() { "http" } else { "none" };
    let raw =
        serde_json::to_string(config).map_err(|error| format!("序列化刷新配置失败：{error}"))?;
    let changed = conn
        .execute(
            "UPDATE data_pools SET refresh_mode = ?1, refresh_json = ?2, updated_at = ?3
             WHERE name = ?4",
            params![mode, raw, now_ms(), name.trim()],
        )
        .map_err(|error| format!("保存刷新配置失败：{error}"))?;
    if changed == 0 {
        return Err(format!("{E_POOL_BINDING_INVALID}: 池「{name}」不存在"));
    }
    Ok(())
}

/// 按绑定批量读取池条目
pub fn items_for_bindings(
    conn: &Connection,
    bindings: &[crate::models::PoolBinding],
) -> Result<HashMap<String, Vec<PoolItem>>, String> {
    let mut result = HashMap::new();
    for binding in bindings {
        if binding.pool.trim().is_empty() {
            continue;
        }
        let items = load_items(conn, binding.pool.trim())?;
        result.insert(binding.pool.trim().to_string(), items);
    }
    Ok(result)
}

/// 池列表（管理页，带条目）
pub fn pool_items(conn: &Connection, pool_id: i64) -> Result<Vec<PoolItemRow>, String> {
    let columns = pool_columns_by_id(conn, pool_id)?;
    let mut stmt = conn
        .prepare(
            "SELECT id, value, account, weight, enabled, created_at, data_json
             FROM data_pool_items WHERE pool_id = ?1 ORDER BY id ASC",
        )
        .map_err(|error| format!("读取池条目失败：{error}"))?;
    let rows = stmt
        .query_map(params![pool_id], |row| {
            let value: String = row.get(1)?;
            let account: Option<String> = row.get(2)?;
            let raw_data: String = row.get(6)?;
            let mut data = parse_data_json(&raw_data);
            if data.is_empty() {
                data = synthesize_data(&columns, &value, account.as_deref());
            }
            Ok(PoolItemRow {
                id: row.get(0)?,
                value,
                account,
                weight: row.get(3)?,
                data,
                enabled: row.get::<_, i64>(4)? != 0,
                created_at: row.get(5)?,
            })
        })
        .map_err(|error| format!("读取池条目失败：{error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("读取池条目失败：{error}"))?;
    Ok(rows)
}

/// 派生状态的子查询：管理页分页、健康度与状态筛选共用同一套口径
const ITEM_STATUS_SOURCE: &str = "SELECT id, value, account, weight, enabled, created_at,
        expires_at, cooldown_until, fail_count, last_used_at, data_json,
        CASE
            WHEN enabled = 0 THEN 'disabled'
            WHEN TRIM(value) = '' OR fail_count >= 5 THEN 'invalid'
            WHEN expires_at > 0 AND expires_at <= ?2 THEN 'expired'
            WHEN cooldown_until > ?2 THEN 'cooling'
            ELSE 'active'
        END AS status
    FROM data_pool_items WHERE pool_id = ?1";

/// 池是否默认脱敏；池不存在时报错
pub fn pool_sensitive(conn: &Connection, pool_id: i64) -> Result<bool, String> {
    conn.query_row(
        "SELECT sensitive FROM data_pools WHERE id = ?1",
        params![pool_id],
        |row| row.get::<_, i64>(0),
    )
    .optional()
    .map_err(|error| format!("查询池失败：{error}"))?
    .map(|value| value != 0)
    .ok_or_else(|| "池不存在".to_string())
}

/// 统计池健康度
fn health_for_pool(conn: &Connection, pool_id: i64) -> Result<PoolHealth, String> {
    let now = now_ms();
    let sql = format!("SELECT status, COUNT(*) FROM ({ITEM_STATUS_SOURCE}) GROUP BY status");
    let mut stmt = conn
        .prepare(&sql)
        .map_err(|error| format!("读取池健康度失败：{error}"))?;
    let rows = stmt
        .query_map(params![pool_id, now], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
        .map_err(|error| format!("读取池健康度失败：{error}"))?;
    let mut health = PoolHealth::default();
    for row in rows {
        let (status, count) = row.map_err(|error| format!("读取池健康度失败：{error}"))?;
        health.total += count;
        match status.as_str() {
            "active" => health.active = count,
            "invalid" => health.invalid = count,
            "expired" => health.expired = count,
            "cooling" => health.cooling = count,
            "disabled" => health.disabled = count,
            _ => {}
        }
    }
    Ok(health)
}

/// 管理页分页查询：只返回掩码（`sensitive = false` 的池才返回原值）
pub fn pool_items_page(
    conn: &Connection,
    pool_id: i64,
    page: i64,
    page_size: i64,
    search: &str,
    status: &str,
    sensitive: bool,
) -> Result<PoolItemsPage, String> {
    let now = now_ms();
    let page = page.max(1);
    let page_size = page_size.clamp(1, 200);
    let offset = (page - 1) * page_size;
    let status = status.trim();
    let search = search.trim();
    let pattern = format!("%{search}%");
    let columns = pool_columns_by_id(conn, pool_id)?;
    let filter = "WHERE (?3 = '' OR status = ?3)
        AND (?4 = '' OR value LIKE ?5 OR IFNULL(account, '') LIKE ?5 OR data_json LIKE ?5)";

    let count_sql = format!("SELECT COUNT(*) FROM ({ITEM_STATUS_SOURCE}) {filter}");
    let total: i64 = conn
        .query_row(
            &count_sql,
            params![pool_id, now, status, search, pattern],
            |row| row.get(0),
        )
        .map_err(|error| format!("读取池条目失败：{error}"))?;

    let list_sql = format!(
        "SELECT id, value, account, weight, enabled, status, expires_at, fail_count, last_used_at, created_at, data_json
         FROM ({ITEM_STATUS_SOURCE}) {filter}
         ORDER BY id ASC LIMIT ?6 OFFSET ?7"
    );
    let mut stmt = conn
        .prepare(&list_sql)
        .map_err(|error| format!("读取池条目失败：{error}"))?;
    let rows = stmt
        .query_map(
            params![pool_id, now, status, search, pattern, page_size, offset],
            |row| {
                let value: String = row.get(1)?;
                let account: Option<String> = row.get(2)?;
                let mut data = parse_data_json(&row.get::<_, String>(10)?);
                // 旧池没有 data_json：按 value/account 合成整行，保证动态列可见
                if data.is_empty() {
                    data = synthesize_data(&columns, &value, account.as_deref());
                }
                if sensitive {
                    for item in data.values_mut() {
                        *item = crate::util::mask_secret_value(item);
                    }
                }
                Ok(PoolItemView {
                    id: row.get(0)?,
                    index: 0,
                    masked: sensitive,
                    value: if sensitive {
                        crate::util::mask_secret_value(&value)
                    } else {
                        value
                    },
                    account,
                    weight: row.get(3)?,
                    data,
                    enabled: row.get::<_, i64>(4)? != 0,
                    status: row.get(5)?,
                    expires_at: row.get(6)?,
                    fail_count: row.get(7)?,
                    last_used_at: row.get(8)?,
                    created_at: row.get(9)?,
                })
            },
        )
        .map_err(|error| format!("读取池条目失败：{error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("读取池条目失败：{error}"))?;
    let items = rows
        .into_iter()
        .enumerate()
        .map(|(index, mut item)| {
            item.index = offset + index as i64 + 1;
            item
        })
        .collect();
    Ok(PoolItemsPage {
        items,
        total,
        page,
        page_size,
        health: health_for_pool(conn, pool_id)?,
    })
}

/// 单条明文；仅管理页二次确认后调用
pub fn reveal_item(conn: &Connection, item_id: i64) -> Result<RevealedItem, String> {
    let row = conn
        .query_row(
            "SELECT id, value, account, data_json, pool_id FROM data_pool_items WHERE id = ?1",
            params![item_id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            },
        )
        .optional()
        .map_err(|error| format!("读取条目失败：{error}"))?
        .ok_or_else(|| "条目不存在".to_string())?;
    let (id, value, account, raw_data, pool_id) = row;
    let mut data = parse_data_json(&raw_data);
    if data.is_empty() {
        let columns = pool_columns_by_id(conn, pool_id)?;
        data = synthesize_data(&columns, &value, account.as_deref());
    }
    Ok(RevealedItem { id, value, data })
}

/// 启用 / 停用条目
pub fn set_item_enabled(conn: &Connection, item_id: i64, enabled: bool) -> Result<(), String> {
    let pool_id: i64 = conn
        .query_row(
            "SELECT pool_id FROM data_pool_items WHERE id = ?1",
            params![item_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| format!("查询条目失败：{error}"))?
        .ok_or_else(|| "条目不存在".to_string())?;
    conn.execute(
        "UPDATE data_pool_items SET enabled = ?1 WHERE id = ?2",
        params![enabled as i64, item_id],
    )
    .map_err(|error| format!("更新条目状态失败：{error}"))?;
    touch_pool(conn, pool_id)?;
    Ok(())
}

/// 清空池条目（保留池本身）
pub fn clear_items(conn: &Connection, pool_id: i64) -> Result<usize, String> {
    let deleted = conn
        .execute(
            "DELETE FROM data_pool_items WHERE pool_id = ?1",
            params![pool_id],
        )
        .map_err(|error| format!("清空池条目失败：{error}"))?;
    touch_pool(conn, pool_id)?;
    Ok(deleted)
}

/// 刷新健康度：清理已到期的冷却并重新统计
pub fn refresh_health(conn: &Connection, pool_id: i64) -> Result<PoolHealth, String> {
    let now = now_ms();
    conn.execute(
        "UPDATE data_pool_items SET cooldown_until = 0
         WHERE pool_id = ?1 AND cooldown_until > 0 AND cooldown_until <= ?2",
        params![pool_id, now],
    )
    .map_err(|error| format!("刷新健康度失败：{error}"))?;
    health_for_pool(conn, pool_id)
}

/// 更新池的 updated_at，让列表按最近变更排序
fn touch_pool(conn: &Connection, pool_id: i64) -> Result<(), String> {
    conn.execute(
        "UPDATE data_pools SET updated_at = ?1 WHERE id = ?2",
        params![now_ms(), pool_id],
    )
    .map_err(|error| format!("更新池时间失败：{error}"))?;
    Ok(())
}

/// 解析导入文本：支持 JSON 数组、CSV（value,account,weight）与逐行文本
#[allow(dead_code)]
pub fn parse_import(text: &str) -> Result<Vec<(String, Option<String>, f64)>, String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err("导入内容为空".to_string());
    }
    if trimmed.starts_with('[') {
        let value: serde_json::Value =
            serde_json::from_str(trimmed).map_err(|error| format!("JSON 解析失败：{error}"))?;
        let array = value
            .as_array()
            .ok_or_else(|| "JSON 顶层必须是数组".to_string())?;
        let mut items = Vec::new();
        for entry in array {
            match entry {
                serde_json::Value::String(text) => {
                    items.push((text.clone(), None, 1.0));
                }
                serde_json::Value::Object(map) => {
                    let value = map
                        .get("value")
                        .or_else(|| map.get("token"))
                        .and_then(|item| item.as_str())
                        .ok_or_else(|| "JSON 对象缺少 value 字段".to_string())?
                        .to_string();
                    let account = map
                        .get("account")
                        .and_then(|item| item.as_str())
                        .map(str::to_string);
                    let weight = map
                        .get("weight")
                        .and_then(|item| item.as_f64())
                        .unwrap_or(1.0);
                    items.push((value, account, weight));
                }
                _ => return Err("JSON 数组元素必须是字符串或对象".to_string()),
            }
        }
        return Ok(items);
    }
    let mut items = Vec::new();
    for line in trimmed.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // 只按英文逗号切分且最多 3 段，避免破坏 token 内部的逗号
        let parts: Vec<&str> = line.splitn(3, ',').map(str::trim).collect();
        let value = parts.first().copied().unwrap_or_default();
        if value.is_empty() {
            continue;
        }
        let account = parts
            .get(1)
            .filter(|item| !item.is_empty())
            .map(|item| item.to_string());
        let weight = parts
            .get(2)
            .and_then(|item| item.parse::<f64>().ok())
            .filter(|value| *value > 0.0)
            .unwrap_or(1.0);
        items.push((value.to_string(), account, weight));
    }
    if items.is_empty() {
        return Err("没有解析到有效条目".to_string());
    }
    Ok(items)
}

/// 导入行：整行数据 + 权重（v20）
#[derive(Debug, Clone)]
pub struct PoolImportRow {
    pub data: HashMap<String, String>,
    pub weight: f64,
}

/// 单事务批量写入条目；value 始终镜像首列
pub fn insert_rows(
    conn: &Connection,
    pool_id: i64,
    columns: &[String],
    rows: &[PoolImportRow],
) -> Result<usize, String> {
    let first = columns.first().cloned().unwrap_or_default();
    let has_account = columns.iter().any(|column| column == "account");
    let now = now_ms();
    let tx = conn
        .unchecked_transaction()
        .map_err(|error| format!("开启导入事务失败：{error}"))?;
    {
        let mut stmt = tx
            .prepare(
                "INSERT INTO data_pool_items (pool_id, value, account, weight, enabled, created_at, data_json)
                 VALUES (?1, ?2, ?3, ?4, 1, ?5, ?6)",
            )
            .map_err(|error| format!("准备写入池条目失败：{error}"))?;
        for row in rows {
            let value = row.data.get(&first).cloned().unwrap_or_default();
            let account = if has_account {
                row.data.get("account").cloned()
            } else {
                None
            };
            let raw_data = serde_json::to_string(&row.data)
                .map_err(|error| format!("序列化整行数据失败：{error}"))?;
            stmt.execute(params![pool_id, value, account, row.weight, now, raw_data])
                .map_err(|error| format!("写入池条目失败：{error}"))?;
        }
    }
    tx.execute(
        "UPDATE data_pools SET updated_at = ?1 WHERE id = ?2",
        params![now, pool_id],
    )
    .map_err(|error| format!("更新池时间失败：{error}"))?;
    tx.commit()
        .map_err(|error| format!("提交导入事务失败：{error}"))?;
    Ok(rows.len())
}

/// JSON 值转字符串（数字/布尔一并支持）
fn json_value_to_string(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(text) => Some(text.clone()),
        serde_json::Value::Number(number) => Some(number.to_string()),
        serde_json::Value::Bool(flag) => Some(flag.to_string()),
        _ => None,
    }
}

/// 导入条目；返回新增条数。按池列解析 JSON 数组或逗号分隔文本（v20）
pub fn import_items(conn: &Connection, pool_id: i64, text: &str) -> Result<usize, String> {
    let columns = pool_columns_by_id(conn, pool_id)?;
    let first = columns
        .first()
        .cloned()
        .ok_or_else(|| "池没有可导入的列".to_string())?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err("导入内容为空".to_string());
    }
    // 旧池（从未保存过列定义）保留「值,账号,权重」文本格式，避免破坏历史用法
    let legacy = conn
        .query_row(
            "SELECT columns_json FROM data_pools WHERE id = ?1",
            params![pool_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|error| format!("读取池列定义失败：{error}"))?
        .map(|raw| {
            let raw = raw.trim();
            raw.is_empty() || raw == "[]"
        })
        .unwrap_or(false);
    if legacy && !trimmed.starts_with('[') {
        let parsed = parse_import(trimmed)?;
        let has_account = columns.iter().any(|column| column == "account");
        let rows: Vec<PoolImportRow> = parsed
            .into_iter()
            .map(|(value, account, weight)| {
                let mut data = HashMap::new();
                for column in &columns {
                    data.insert(column.clone(), String::new());
                }
                data.insert(first.clone(), value);
                if has_account {
                    if let Some(account) = account {
                        data.insert("account".to_string(), account);
                    }
                }
                PoolImportRow { data, weight }
            })
            .collect();
        return insert_rows(conn, pool_id, &columns, &rows);
    }
    let mut rows: Vec<PoolImportRow> = Vec::new();
    if trimmed.starts_with('[') {
        let value: serde_json::Value =
            serde_json::from_str(trimmed).map_err(|error| format!("JSON 解析失败：{error}"))?;
        let array = value
            .as_array()
            .ok_or_else(|| "JSON 顶层必须是数组".to_string())?;
        for (index, entry) in array.iter().enumerate() {
            match entry {
                serde_json::Value::String(text) => {
                    let mut data = HashMap::new();
                    for column in &columns {
                        data.insert(column.clone(), String::new());
                    }
                    data.insert(first.clone(), text.clone());
                    rows.push(PoolImportRow { data, weight: 1.0 });
                }
                serde_json::Value::Object(map) => {
                    let mut data = HashMap::new();
                    for column in &columns {
                        let text = map
                            .get(column)
                            .and_then(json_value_to_string)
                            .unwrap_or_default();
                        data.insert(column.clone(), text);
                    }
                    if data
                        .get(&first)
                        .map(|item| item.trim().is_empty())
                        .unwrap_or(true)
                    {
                        return Err(format!("第 {} 个对象缺少首列「{}」的值", index + 1, first));
                    }
                    let weight = map
                        .get("weight")
                        .and_then(|item| item.as_f64())
                        .unwrap_or(1.0);
                    rows.push(PoolImportRow { data, weight });
                }
                _ => return Err("JSON 数组元素必须是字符串或对象".to_string()),
            }
        }
    } else {
        let header = columns.join(",");
        let mut first_line = true;
        for line in trimmed.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if first_line {
                first_line = false;
                if line == header {
                    continue;
                }
            }
            let parts: Vec<&str> = line.splitn(columns.len(), ',').map(str::trim).collect();
            let value = parts.first().copied().unwrap_or_default();
            if value.is_empty() {
                continue;
            }
            let mut data = HashMap::new();
            for (index, column) in columns.iter().enumerate() {
                data.insert(
                    column.clone(),
                    parts.get(index).copied().unwrap_or_default().to_string(),
                );
            }
            rows.push(PoolImportRow { data, weight: 1.0 });
        }
    }
    if rows.is_empty() {
        return Err("没有解析到有效条目".to_string());
    }
    insert_rows(conn, pool_id, &columns, &rows)
}

/// 列变更（v20）：`from` 为空表示新增列，否则表示由旧列改名而来
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PoolColumnChange {
    #[serde(default)]
    pub from: Option<String>,
    pub name: String,
}

/// 保存池列定义并迁移存量数据；返回新列
pub fn save_columns(
    conn: &Connection,
    pool_id: i64,
    changes: &[PoolColumnChange],
) -> Result<Vec<String>, String> {
    let old_columns = pool_columns_by_id(conn, pool_id)?;
    let names: Vec<String> = changes.iter().map(|change| change.name.clone()).collect();
    let new_columns = normalize_columns(&names)?;
    // normalize_columns 会静默去重；列迁移不能静默丢列，重复名直接报错
    if new_columns.len() != changes.len() {
        return Err("列名不能重复".to_string());
    }

    let mut referenced: HashSet<String> = HashSet::new();
    for change in changes {
        if let Some(from) = change
            .from
            .as_deref()
            .map(str::trim)
            .filter(|item| !item.is_empty())
        {
            if !old_columns.iter().any(|column| column == from) {
                return Err(format!("原列「{from}」不存在"));
            }
            if !referenced.insert(from.to_string()) {
                return Err(format!("原列「{from}」被重复引用"));
            }
        }
    }
    // 同名保留的列不能同时又作为另一个改名的来源，否则数据归属不明确
    for change in changes {
        let has_from = change
            .from
            .as_deref()
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .is_some();
        if !has_from && referenced.contains(change.name.trim()) {
            return Err(format!(
                "列「{}」同时被保留和重命名，请调整列定义",
                change.name.trim()
            ));
        }
    }

    let items = {
        let mut stmt = conn
            .prepare("SELECT id, value, account, data_json FROM data_pool_items WHERE pool_id = ?1")
            .map_err(|error| format!("读取池条目失败：{error}"))?;
        let rows = stmt
            .query_map(params![pool_id], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })
            .map_err(|error| format!("读取池条目失败：{error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("读取池条目失败：{error}"))?;
        rows
    };

    let tx = conn
        .unchecked_transaction()
        .map_err(|error| format!("开启列迁移事务失败：{error}"))?;
    {
        let mut update = tx
            .prepare(
                "UPDATE data_pool_items SET data_json = ?1, value = ?2, account = ?3 WHERE id = ?4",
            )
            .map_err(|error| format!("准备更新池条目失败：{error}"))?;
        for (id, value, account, raw_data) in items {
            let mut old_data = parse_data_json(&raw_data);
            if old_data.is_empty() {
                old_data = synthesize_data(&old_columns, &value, account.as_deref());
            }
            let mut new_data = HashMap::new();
            for column in &new_columns {
                let renamed = changes.iter().find(|change| {
                    change.name.trim() == column
                        && change
                            .from
                            .as_deref()
                            .map(str::trim)
                            .filter(|item| !item.is_empty())
                            .is_some()
                });
                let next_value = if let Some(change) = renamed {
                    let from = change.from.as_deref().map(str::trim).unwrap_or_default();
                    old_data.get(from).cloned().unwrap_or_default()
                } else {
                    old_data.get(column).cloned().unwrap_or_default()
                };
                new_data.insert(column.clone(), next_value);
            }
            let new_value = new_columns
                .first()
                .and_then(|column| new_data.get(column))
                .cloned()
                .unwrap_or_default();
            let new_account = if new_columns.iter().any(|column| column == "account") {
                new_data.get("account").cloned()
            } else {
                None
            };
            let raw_new = serde_json::to_string(&new_data)
                .map_err(|error| format!("序列化整行数据失败：{error}"))?;
            update
                .execute(params![raw_new, new_value, new_account, id])
                .map_err(|error| format!("更新池条目失败：{error}"))?;
        }
    }
    let raw_columns =
        serde_json::to_string(&new_columns).map_err(|error| format!("序列化池列失败：{error}"))?;
    tx.execute(
        "UPDATE data_pools SET columns_json = ?1, updated_at = ?2 WHERE id = ?3",
        params![raw_columns, now_ms(), pool_id],
    )
    .map_err(|error| format!("保存池列失败：{error}"))?;
    tx.commit()
        .map_err(|error| format!("提交列迁移事务失败：{error}"))?;
    Ok(new_columns)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn setup_pool_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE data_pools (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                kind TEXT NOT NULL DEFAULT 'token',
                sensitive INTEGER NOT NULL DEFAULT 1,
                description TEXT,
                refresh_mode TEXT NOT NULL DEFAULT 'none',
                refresh_json TEXT NOT NULL DEFAULT '{}',
                columns_json TEXT NOT NULL DEFAULT '[]',
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );
            CREATE TABLE data_pool_items (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                pool_id INTEGER NOT NULL,
                value TEXT NOT NULL,
                account TEXT,
                weight REAL NOT NULL DEFAULT 1,
                enabled INTEGER NOT NULL DEFAULT 1,
                created_at INTEGER NOT NULL,
                expires_at INTEGER NOT NULL DEFAULT 0,
                cooldown_until INTEGER NOT NULL DEFAULT 0,
                fail_count INTEGER NOT NULL DEFAULT 0,
                last_used_at INTEGER NOT NULL DEFAULT 0,
                data_json TEXT NOT NULL DEFAULT '{}'
            );",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO data_pools (name, kind, sensitive, created_at, updated_at)
             VALUES ('tokens', 'token', 1, 1, 1)",
            [],
        )
        .unwrap();
        // 1 可用 / 2 空值失效 / 3 已过期 / 4 冷却中 / 5 已停用
        let now = now_ms();
        let items = [
            ("token-active", 0_i64, 0_i64, 1_i64),
            ("   ", 0, 0, 1),
            ("token-expired", now - 1000, 0, 1),
            ("token-cooling", 0, now + 60_000, 1),
            ("token-disabled", 0, 0, 0),
        ];
        for (value, expires_at, cooldown_until, enabled) in items {
            conn.execute(
                "INSERT INTO data_pool_items (pool_id, value, enabled, created_at, expires_at, cooldown_until)
                 VALUES (1, ?1, ?2, ?3, ?4, ?5)",
                params![value, enabled, now, expires_at, cooldown_until],
            )
            .unwrap();
        }
        conn
    }

    #[test]
    fn refresh_config_roundtrip() {
        let conn = setup_pool_db();
        let config = PoolRefreshConfig {
            mode: "http".to_string(),
            method: "POST".to_string(),
            url: "https://example.com/oauth/token".to_string(),
            headers: Vec::new(),
            body: r#"{"refresh_token":"{{refreshToken}}"}"#.to_string(),
            token_path: "$.data.accessToken".to_string(),
            expires_path: "$.data.expiresIn".to_string(),
            expires_in_sec: 60,
        };
        save_refresh_config(&conn, "tokens", &config).unwrap();
        let loaded = load_refresh_config(&conn, "tokens").unwrap();
        assert_eq!(loaded.mode, "http");
        assert_eq!(loaded.token_path, "$.data.accessToken");
        assert!(loaded.is_http());
        let map = pool_refresh_configs(&conn, &["tokens".to_string()]).unwrap();
        assert_eq!(map.len(), 1);
    }

    #[test]
    fn parse_plain_lines_and_csv() {
        let items = parse_import("abc\nxyz,acc1,2").unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[1].1.as_deref(), Some("acc1"));
        assert_eq!(items[1].2, 2.0);
    }

    #[test]
    fn parse_json_array() {
        let items = parse_import(r#"[{"token":"t1","account":"a"}, "t2"]"#).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[1].0, "t2");
    }

    #[test]
    fn items_page_masks_and_filters_by_status() {
        let conn = setup_pool_db();
        let all = pool_items_page(&conn, 1, 1, 50, "", "", true).unwrap();
        assert_eq!(all.total, 5);
        assert_eq!(all.health.active, 1);
        assert_eq!(all.health.invalid, 1);
        assert_eq!(all.health.expired, 1);
        assert_eq!(all.health.cooling, 1);
        assert_eq!(all.health.disabled, 1);
        // 敏感池：列表接口不下发明文
        assert!(all.items.iter().all(|item| item.masked));
        assert!(all
            .items
            .iter()
            .all(|item| !item.value.contains("token-active")));

        let cooling = pool_items_page(&conn, 1, 1, 50, "", "cooling", true).unwrap();
        assert_eq!(cooling.total, 1);
        assert_eq!(cooling.items[0].status, "cooling");

        let disabled = pool_items_page(&conn, 1, 1, 50, "", "disabled", true).unwrap();
        assert_eq!(disabled.total, 1);
        assert!(!disabled.items[0].enabled);

        let searched = pool_items_page(&conn, 1, 1, 50, "expired", "", true).unwrap();
        assert_eq!(searched.total, 1);
    }

    #[test]
    fn item_toggle_and_clear_update_pool() {
        let conn = setup_pool_db();
        set_item_enabled(&conn, 1, false).unwrap();
        let after = pool_items_page(&conn, 1, 1, 50, "", "disabled", true).unwrap();
        assert_eq!(after.total, 2);
        let cleared = clear_items(&conn, 1).unwrap();
        assert_eq!(cleared, 5);
        let empty = pool_items_page(&conn, 1, 1, 50, "", "", true).unwrap();
        assert_eq!(empty.total, 0);
    }

    /// 追加一个指定列定义的池
    fn add_pool(conn: &Connection, name: &str, kind: &str, columns: &[&str]) -> i64 {
        let raw = serde_json::to_string(columns).unwrap();
        conn.execute(
            "INSERT INTO data_pools (name, kind, sensitive, columns_json, created_at, updated_at)
             VALUES (?1, ?2, 1, ?3, 1, 1)",
            params![name, kind, raw],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    #[test]
    fn column_validation_and_defaults() {
        assert_eq!(default_columns("token"), vec!["token", "account"]);
        assert_eq!(default_columns("parameter"), vec!["value"]);
        assert_eq!(
            normalize_columns(&[
                " token ".to_string(),
                "account".to_string(),
                "token".to_string()
            ])
            .unwrap(),
            vec!["token", "account"]
        );
        assert!(normalize_columns(&[]).is_err());
        assert!(normalize_columns(&["1abc".to_string()]).is_err());
        assert!(normalize_columns(&["a b".to_string()]).is_err());
        let too_many: Vec<String> = (0..(MAX_POOL_COLUMNS + 1))
            .map(|index| format!("c{index}"))
            .collect();
        assert!(normalize_columns(&too_many).is_err());
        // 旧池（空列定义）按 v20 之前的固定列兼容
        assert_eq!(parse_columns_json("", "token"), vec!["value", "account"]);
        assert_eq!(parse_columns_json("not-json", "parameter"), vec!["value"]);
        assert_eq!(
            parse_columns_json("[]", "parameter"),
            vec!["value", "account"]
        );
    }

    #[test]
    fn save_columns_renames_migrates_and_mirrors() {
        let conn = setup_pool_db();
        // 自定义列池；写入一条带整行数据的条目
        let pool_id = add_pool(&conn, "custom", "token", &["token", "account"]);
        conn.execute(
            "INSERT INTO data_pool_items (pool_id, value, account, weight, enabled, created_at, data_json)
             VALUES (?1, 't1', 'a1', 1, 1, 1, '{\"token\":\"t1\",\"account\":\"a1\"}')",
            params![pool_id],
        )
        .unwrap();
        let columns = save_columns(
            &conn,
            pool_id,
            &[
                PoolColumnChange {
                    from: Some("token".to_string()),
                    name: "token".to_string(),
                },
                PoolColumnChange {
                    from: None,
                    name: "nick".to_string(),
                },
            ],
        )
        .unwrap();
        assert_eq!(columns, vec!["token", "nick"]);
        let rows = pool_items(&conn, pool_id).unwrap();
        let row = rows.last().unwrap();
        assert_eq!(row.value, "t1");
        assert_eq!(row.data.get("token").map(String::as_str), Some("t1"));
        assert_eq!(row.data.get("nick").map(String::as_str), Some(""));
        assert_eq!(row.account, None);

        // 互换列名：token/account -> account/token
        let pool_id = add_pool(&conn, "swap", "token", &["token", "account"]);
        conn.execute(
            "INSERT INTO data_pool_items (pool_id, value, account, weight, enabled, created_at, data_json)
             VALUES (?1, 't1', 'a1', 1, 1, 1, '{\"token\":\"t1\",\"account\":\"a1\"}')",
            params![pool_id],
        )
        .unwrap();
        let swapped = save_columns(
            &conn,
            pool_id,
            &[
                PoolColumnChange {
                    from: Some("token".to_string()),
                    name: "account".to_string(),
                },
                PoolColumnChange {
                    from: Some("account".to_string()),
                    name: "token".to_string(),
                },
            ],
        )
        .unwrap();
        assert_eq!(swapped, vec!["account", "token"]);
        let row = pool_items(&conn, pool_id).unwrap().pop().unwrap();
        assert_eq!(row.data.get("account").map(String::as_str), Some("t1"));
        assert_eq!(row.data.get("token").map(String::as_str), Some("a1"));
        assert_eq!(row.value, "t1");
        // account 字段始终镜像 data["account"]，互换后即旧 token 值
        assert_eq!(row.account.as_deref(), Some("t1"));

        // 删除列：只保留 token（account 未被引用即视为删除）
        let deleted = save_columns(
            &conn,
            pool_id,
            &[PoolColumnChange {
                from: None,
                name: "token".to_string(),
            }],
        )
        .unwrap();
        assert_eq!(deleted, vec!["token"]);
        let row = pool_items(&conn, pool_id).unwrap().pop().unwrap();
        assert_eq!(row.data.len(), 1);
        assert_eq!(row.account, None);

        // 删除旧 account 的同时把 token 改名为 account：允许复用被删除列的名字
        let pool_id = add_pool(&conn, "reuse", "token", &["token", "account"]);
        let reused = save_columns(
            &conn,
            pool_id,
            &[PoolColumnChange {
                from: Some("token".to_string()),
                name: "account".to_string(),
            }],
        )
        .unwrap();
        assert_eq!(reused, vec!["account"]);
        // 两个改名指向同一个新列名时报冲突
        let pool_id = add_pool(&conn, "conflict", "token", &["token", "account"]);
        let conflict = save_columns(
            &conn,
            pool_id,
            &[
                PoolColumnChange {
                    from: Some("token".to_string()),
                    name: "merged".to_string(),
                },
                PoolColumnChange {
                    from: Some("account".to_string()),
                    name: "merged".to_string(),
                },
            ],
        );
        assert!(conflict.is_err());
    }

    #[test]
    fn insert_rows_mirrors_first_column() {
        let conn = setup_pool_db();
        let pool_id = add_pool(&conn, "texts", "parameter", &["value", "note"]);
        let mut data = HashMap::new();
        data.insert("value".to_string(), "v1".to_string());
        data.insert("note".to_string(), "n1".to_string());
        let inserted = insert_rows(
            &conn,
            pool_id,
            &["value".to_string(), "note".to_string()],
            &[PoolImportRow { data, weight: 2.0 }],
        )
        .unwrap();
        assert_eq!(inserted, 1);
        let (value, weight, raw): (String, f64, String) = conn
            .query_row(
                "SELECT value, weight, data_json FROM data_pool_items WHERE pool_id = ?1",
                params![pool_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(value, "v1");
        assert_eq!(weight, 2.0);
        let parsed: HashMap<String, String> = serde_json::from_str(&raw).unwrap();
        assert_eq!(parsed.get("value").map(String::as_str), Some("v1"));
        assert_eq!(parsed.get("note").map(String::as_str), Some("n1"));
    }

    #[test]
    fn import_items_parses_text_and_json_by_columns() {
        let conn = setup_pool_db();
        let pool_id = add_pool(&conn, "imports", "token", &["token", "account"]);
        let text = "token,account\n# 注释\n\nt1,a1\nt2,a2\n,ignored";
        let inserted = import_items(&conn, pool_id, text).unwrap();
        assert_eq!(inserted, 2);
        let rows = pool_items(&conn, pool_id).unwrap();
        assert_eq!(rows[0].value, "t1");
        assert_eq!(rows[0].account.as_deref(), Some("a1"));
        assert_eq!(rows[1].data.get("account").map(String::as_str), Some("a2"));

        let json = r#"[{"token":"t3","account":"a3","extra":"x"},{"token":"t4"}]"#;
        let inserted = import_items(&conn, pool_id, json).unwrap();
        assert_eq!(inserted, 2);
        let rows = pool_items(&conn, pool_id).unwrap();
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[2].value, "t3");
        assert!(!rows[2].data.contains_key("extra"));
        assert_eq!(rows[3].data.get("account").map(String::as_str), Some(""));
        // 缺少首列值的对象报错
        assert!(import_items(&conn, pool_id, r#"[{"account":"a"}]"#).is_err());
    }
}

/// 12.5 性能验收（手动执行，默认测试集跳过）
///
/// `cargo test --lib perf_pool_preview_100k -- --ignored --nocapture`
#[cfg(test)]
mod perf_tests {
    use super::*;
    use rusqlite::{params, Connection};
    use std::path::PathBuf;
    use std::time::Instant;

    /// 建一个带 `count` 条 token 的临时文件库（与 12.5 场景一致：落盘而非内存库）
    fn temp_pool(tag: &str, count: usize) -> (PathBuf, Connection) {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|value| value.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!(
            "astroman-perf-pool-{tag}-{}-{stamp}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let conn = Connection::open(dir.join("pool.db")).unwrap();
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             CREATE TABLE data_pools (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 name TEXT NOT NULL,
                 kind TEXT NOT NULL DEFAULT 'token',
                 sensitive INTEGER NOT NULL DEFAULT 1,
                 description TEXT,
                 refresh_mode TEXT NOT NULL DEFAULT 'none',
                 refresh_json TEXT NOT NULL DEFAULT '{}',
                 columns_json TEXT NOT NULL DEFAULT '[]',
                 created_at INTEGER NOT NULL,
                 updated_at INTEGER NOT NULL
             );
             CREATE TABLE data_pool_items (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 pool_id INTEGER NOT NULL,
                 value TEXT NOT NULL,
                 account TEXT,
                 weight REAL NOT NULL DEFAULT 1,
                 enabled INTEGER NOT NULL DEFAULT 1,
                 created_at INTEGER NOT NULL,
                 expires_at INTEGER NOT NULL DEFAULT 0,
                 cooldown_until INTEGER NOT NULL DEFAULT 0,
                 fail_count INTEGER NOT NULL DEFAULT 0,
                 last_used_at INTEGER NOT NULL DEFAULT 0,
                 data_json TEXT NOT NULL DEFAULT '{}'
             );
             INSERT INTO data_pools (name, kind, sensitive, created_at, updated_at)
             VALUES ('perf', 'token', 1, 1, 1);
             BEGIN;",
        )
        .unwrap();
        {
            let mut stmt = conn
                .prepare(
                    "INSERT INTO data_pool_items (pool_id, value, created_at)
                     VALUES (1, ?1, 1)",
                )
                .unwrap();
            for index in 0..count {
                stmt.execute(params![format!("token-{index:06}")]).unwrap();
            }
        }
        conn.execute_batch("COMMIT").unwrap();
        (dir, conn)
    }

    /// 当前进程 RSS（KB）；没有 `ps` 的平台返回 0
    fn rss_kb() -> u64 {
        std::process::Command::new("ps")
            .args(["-o", "rss=", "-p", &std::process::id().to_string()])
            .output()
            .ok()
            .and_then(|output| String::from_utf8(output.stdout).ok())
            .and_then(|text| text.trim().parse().ok())
            .unwrap_or(0)
    }

    /// 12.5：单条 reveal（单行读取）< 50ms
    #[test]
    #[ignore = "性能验收，需手动执行"]
    fn perf_pool_reveal_one() {
        let (dir, conn) = temp_pool("reveal", 100_000);
        let started = Instant::now();
        let (_pool_id, value) = {
            let item = reveal_item(&conn, 50_000).unwrap();
            (item.id, item.value)
        };
        let elapsed = started.elapsed();
        println!("perf_pool_reveal_one: {} bytes in {elapsed:?}", value.len());
        assert!(
            elapsed < std::time::Duration::from_millis(50),
            "单条 reveal 耗时 {elapsed:?}"
        );
        drop(conn);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 12.5：10,000 token 载入内存 < 128MB
    #[test]
    #[ignore = "性能验收，需手动执行"]
    fn perf_pool_memory_10k() {
        let (dir, conn) = temp_pool("memory", 10_000);
        let before = rss_kb();
        let items = load_items(&conn, "perf").unwrap();
        let after = rss_kb();
        std::hint::black_box(&items);
        let delta_mb = after.saturating_sub(before) as f64 / 1024.0;
        println!(
            "perf_pool_memory_10k: {} items, rss +{delta_mb:.1} MB",
            items.len()
        );
        assert_eq!(items.len(), 10_000);
        if before > 0 {
            assert!(
                delta_mb < 128.0,
                "10,000 token 占用 {delta_mb:.1} MB，超过 128MB 目标"
            );
        }
        drop(items);
        drop(conn);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[ignore = "性能验收，需手动执行"]
    fn perf_pool_preview_100k() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|value| value.as_nanos())
            .unwrap_or(0);
        let dir =
            std::env::temp_dir().join(format!("astroman-perf-pool-{}-{stamp}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let conn = Connection::open(dir.join("pool.db")).unwrap();
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             CREATE TABLE data_pools (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 name TEXT NOT NULL,
                 kind TEXT NOT NULL DEFAULT 'token',
                 sensitive INTEGER NOT NULL DEFAULT 1,
                 description TEXT,
                 refresh_mode TEXT NOT NULL DEFAULT 'none',
                 refresh_json TEXT NOT NULL DEFAULT '{}',
                 columns_json TEXT NOT NULL DEFAULT '[]',
                 created_at INTEGER NOT NULL,
                 updated_at INTEGER NOT NULL
             );
             CREATE TABLE data_pool_items (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 pool_id INTEGER NOT NULL,
                 value TEXT NOT NULL,
                 account TEXT,
                 weight REAL NOT NULL DEFAULT 1,
                 enabled INTEGER NOT NULL DEFAULT 1,
                 created_at INTEGER NOT NULL,
                 expires_at INTEGER NOT NULL DEFAULT 0,
                 cooldown_until INTEGER NOT NULL DEFAULT 0,
                 fail_count INTEGER NOT NULL DEFAULT 0,
                 last_used_at INTEGER NOT NULL DEFAULT 0,
                 data_json TEXT NOT NULL DEFAULT '{}'
             );
             INSERT INTO data_pools (name, kind, sensitive, created_at, updated_at)
             VALUES ('perf', 'token', 1, 1, 1);",
        )
        .unwrap();
        // 10 万条：单事务批量写入，避免影响被测读取
        conn.execute_batch("BEGIN").unwrap();
        {
            let mut stmt = conn
                .prepare(
                    "INSERT INTO data_pool_items (pool_id, value, created_at)
                     VALUES (1, ?1, 1)",
                )
                .unwrap();
            for index in 0..100_000 {
                stmt.execute(params![format!("token-{index:06}")]).unwrap();
            }
        }
        conn.execute_batch("COMMIT").unwrap();

        let started = Instant::now();
        let items = load_items(&conn, "perf").unwrap();
        let elapsed = started.elapsed();
        println!(
            "perf_pool_preview_100k: {} items in {elapsed:?}",
            items.len()
        );
        assert_eq!(items.len(), 100_000);
        assert!(
            elapsed < std::time::Duration::from_millis(300),
            "预检读取 10 万条耗时 {elapsed:?}，超过 300ms 目标"
        );
        drop(conn);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
