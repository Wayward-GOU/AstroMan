use chrono::{Local, SecondsFormat};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

/// 单测用的临时目录
///
/// 优先放在工程的 `target/test-tmp` 下：部分沙箱环境会禁止写系统临时目录，
/// SQLite 会以 `attempt to write a readonly database` 报错，把测试带到坑里。
#[cfg(test)]
pub fn test_dir(tag: &str) -> std::path::PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let name = format!("{tag}-{}-{stamp}", std::process::id());
    let local = std::env::current_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
        .join("target")
        .join("test-tmp")
        .join(&name);
    if std::fs::create_dir_all(&local).is_ok() {
        return local;
    }
    let fallback = std::env::temp_dir().join(name);
    let _ = std::fs::create_dir_all(&fallback);
    fallback
}

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// 归一化集群变量策略：master_wins（默认）/ local_wins
pub fn normalize_var_policy(value: Option<&str>) -> &'static str {
    match value
        .map(|value| value.trim().to_ascii_lowercase())
        .as_deref()
    {
        Some("local_wins") => "local_wins",
        _ => "master_wins",
    }
}

/// 统一的错误转字符串，方便直接返回给前端展示。
///
/// SQLite 的写锁冲突统一收口成 `E_BUSY`：GUI 与 CLI 是独立进程，
/// 单实例插件只能挡住 GUI 多开，挡不住 CLI 同时写库。
pub fn err<E: std::fmt::Display>(e: E) -> String {
    let text = e.to_string();
    if is_db_busy(&text) {
        return format!("E_BUSY: 数据库正被其他进程写入，请关闭 GUI 或其他 CLI 后重试（{text}）");
    }
    text
}

/// 判断错误文本是否属于 SQLite 写锁冲突
pub fn is_db_busy(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("database is locked")
        || lower.contains("database table is locked")
        || lower.contains("sqlite_busy")
        || lower.contains("database schema is locked")
}

/// 创建不弹控制台窗口的子进程命令。
///
/// Windows 上 `reg`、`certutil`、`icacls` 这类控制台程序默认会闪一个黑窗；
/// 定时轮询证书状态时会被用户看成“狂弹 CMD”，统一加上 `CREATE_NO_WINDOW`。
#[cfg(windows)]
pub fn hidden_command(program: &str) -> std::process::Command {
    #[allow(unused_mut)]
    let mut command = std::process::Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

/// 去掉 JSON 文本里的注释，与 Postman 的 raw(JSON) 编辑器保持一致
///
/// 支持 `// 行注释` 与 `/* 块注释 */`，字符串内部的 `//`（例如 URL）不会被误删。
pub fn strip_json_comments(input: &str) -> String {
    let chars: Vec<char> = input.chars().collect();
    let mut out = String::with_capacity(input.len());
    let mut index = 0;
    let mut in_string = false;
    let mut escaped = false;

    while index < chars.len() {
        let current = chars[index];
        if in_string {
            out.push(current);
            if escaped {
                escaped = false;
            } else if current == '\\' {
                escaped = true;
            } else if current == '"' {
                in_string = false;
            }
            index += 1;
            continue;
        }
        if current == '"' {
            in_string = true;
            out.push(current);
            index += 1;
            continue;
        }
        if current == '/' && index + 1 < chars.len() {
            match chars[index + 1] {
                '/' => {
                    index += 2;
                    while index < chars.len() && chars[index] != '\n' {
                        index += 1;
                    }
                    continue;
                }
                '*' => {
                    index += 2;
                    while index + 1 < chars.len()
                        && !(chars[index] == '*' && chars[index + 1] == '/')
                    {
                        index += 1;
                    }
                    index = (index + 2).min(chars.len());
                    continue;
                }
                _ => {}
            }
        }
        out.push(current);
        index += 1;
    }

    out
}

/// 简单模板渲染：支持 {{var}} 与内置变量
///
/// 内置变量支持 `{{timestamp}}` 与 Postman 风格的 `{{$timestamp}}` 两种写法。
pub fn render(input: &str, vars: &HashMap<String, String>) -> String {
    let mut secrets = Vec::new();
    render_tracked(input, vars, &mut secrets)
}

/// 渲染并收集本次实际用到的敏感值（供日志 / 报告脱敏）
pub fn render_tracked(
    input: &str,
    vars: &HashMap<String, String>,
    used_secrets: &mut Vec<String>,
) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;

    while let Some(start) = rest.find("{{") {
        let Some(end_rel) = rest[start + 2..].find("}}") else {
            break;
        };
        let end = start + 2 + end_rel;
        out.push_str(&rest[..start]);
        let expr = rest[start + 2..end].trim();
        out.push_str(&resolve_expr(expr, vars, used_secrets));
        rest = &rest[end + 2..];
    }
    out.push_str(rest);
    out
}

fn resolve_expr(
    expr: &str,
    vars: &HashMap<String, String>,
    used_secrets: &mut Vec<String>,
) -> String {
    // {{secret:NAME}}：与普通变量取值一致，但会进入脱敏清单
    if let Some(secret_expr) = expr.strip_prefix("secret:") {
        let secret_expr = secret_expr.trim();
        let name = secret_expr.strip_prefix('$').unwrap_or(secret_expr);
        if let Some(value) = vars.get(secret_expr).or_else(|| vars.get(name)) {
            if !value.is_empty() {
                used_secrets.push(value.clone());
            }
            return value.clone();
        }
        return format!("{{{{secret:{secret_expr}}}}}");
    }

    // {{os:NAME}}：只认注入时写入的专用键，环境变量或数据池里的同名变量顶不掉
    // 系统值；专用键缺失说明该变量未进白名单或本机没有，按未解析处理。
    if let Some(os_expr) = expr.strip_prefix("os:") {
        let os_expr = os_expr.trim();
        let name = os_expr.strip_prefix('$').unwrap_or(os_expr);
        if let Some(value) = vars.get(&os_var_key(name)) {
            return value.clone();
        }
        return format!("{{{{os:{name}}}}}");
    }

    // {{faker:name}} / {{faker:int(1,100)}}：每次渲染重新取值（G-05）
    if let Some(faker_expr) = expr.strip_prefix("faker:") {
        if let Some(value) = crate::services::faker::runtime_value(faker_expr) {
            return value;
        }
        log::warn!("未知 faker 变量，保留原占位：{faker_expr}");
        return format!("{{{{faker:{faker_expr}}}}}");
    }

    let name = expr.strip_prefix('$').unwrap_or(expr);

    // 自定义变量优先，避免同名环境变量被内置变量覆盖
    if let Some(value) = vars.get(expr).or_else(|| vars.get(name)) {
        return value.clone();
    }

    if let Some(value) = resolve_time(name) {
        return value;
    }
    if let Some(value) = resolve_plain(name) {
        return value;
    }
    if let Some(value) = resolve_function(name) {
        return value;
    }

    format!("{{{{{expr}}}}}")
}

/// 与时间相关的内置变量，统一按本机时区输出
fn resolve_time(name: &str) -> Option<String> {
    let now = Local::now();
    Some(match name {
        "timestamp" | "timestampMs" => now.timestamp_millis().to_string(),
        "timestampSec" => now.timestamp().to_string(),
        "isoTime" | "isoTimestamp" => now.to_rfc3339_opts(SecondsFormat::Millis, false),
        "datetime" | "now" => now.format("%Y-%m-%d %H:%M:%S").to_string(),
        "date" | "today" => now.format("%Y-%m-%d").to_string(),
        "time" => now.format("%H:%M:%S").to_string(),
        "yyyyMMdd" => now.format("%Y%m%d").to_string(),
        "yyyyMMddHHmmss" => now.format("%Y%m%d%H%M%S").to_string(),
        _ => return None,
    })
}

/// 不需要参数的内置变量
fn resolve_plain(name: &str) -> Option<String> {
    Some(match name {
        "uuid" | "guid" => uuid::Uuid::new_v4().to_string(),
        "uuidNoDash" | "guidNoDash" | "uuidSimple" => uuid::Uuid::new_v4().simple().to_string(),
        "randomBool" => {
            if random_int(0, 1) == 1 {
                "true".to_string()
            } else {
                "false".to_string()
            }
        }
        "randomHex" => random_string(16, "hex"),
        "randomEmail" => format!("fink_{}@example.com", random_string(8, "hex")),
        "randomPhone" => format!("1{}{}", random_int(3, 9), random_string(9, "num")),
        "randomIPv4" => format!(
            "{}.{}.{}.{}",
            random_int(1, 254),
            random_int(0, 255),
            random_int(0, 255),
            random_int(1, 254)
        ),
        "randomInt" => random_int(1, 1000).to_string(),
        "randomString" => random_string(8, "alnum"),
        _ => return None,
    })
}

/// 带参数的函数型变量
fn resolve_function(name: &str) -> Option<String> {
    if let Some(inner) = call_args(name, "randomInt") {
        let args = int_args(inner)?;
        return Some(match args.as_slice() {
            [max] => random_int(0, *max).to_string(),
            [min, max] => random_int(*min, *max).to_string(),
            _ => return None,
        });
    }

    if let Some(inner) = call_args(name, "randomFloat") {
        let raw: Vec<&str> = inner.split(',').map(str::trim).collect();
        let min = raw.first()?.parse::<f64>().ok()?;
        let max = raw.get(1)?.parse::<f64>().ok()?;
        let decimals = match raw.get(2) {
            Some(value) => value.parse::<usize>().ok()?.min(6),
            None => 2,
        };
        return Some(random_float(min, max, decimals));
    }

    if let Some(inner) = call_args(name, "randomString") {
        let raw: Vec<&str> = inner.split(',').map(str::trim).collect();
        let len = raw.first()?.parse::<usize>().ok()?;
        let pool = raw.get(1).copied().unwrap_or("alnum");
        return Some(random_string(len, pool));
    }

    if let Some(inner) = call_args(name, "randomHex") {
        let len = inner.trim().parse::<usize>().ok()?;
        return Some(random_string(len, "hex"));
    }

    if let Some(inner) = call_args(name, "randomNumber") {
        let len = inner.trim().parse::<usize>().ok()?;
        return Some(random_string(len, "num"));
    }

    None
}

/// 把 JSON 对象摊平成 `a.b.c` 形式的字符串变量，供 `{{a.b.c}}` 渲染
///
/// 顶层键同时保留原始 JSON 文本，`{{user}}` 仍可拿到完整对象。
pub fn flatten_json(prefix: &str, value: &serde_json::Value, out: &mut HashMap<String, String>) {
    match value {
        serde_json::Value::Object(map) => {
            if !prefix.is_empty() {
                out.insert(prefix.to_string(), value.to_string());
            }
            for (key, child) in map {
                let path = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                flatten_json(&path, child, out);
            }
        }
        serde_json::Value::Array(items) => {
            if !prefix.is_empty() {
                out.insert(prefix.to_string(), value.to_string());
            }
            for (index, child) in items.iter().enumerate() {
                let path = if prefix.is_empty() {
                    index.to_string()
                } else {
                    format!("{prefix}.{index}")
                };
                flatten_json(&path, child, out);
            }
        }
        serde_json::Value::String(text) => {
            out.insert(prefix.to_string(), text.clone());
        }
        serde_json::Value::Null => {
            out.insert(prefix.to_string(), String::new());
        }
        other => {
            out.insert(prefix.to_string(), other.to_string());
        }
    }
}

/// 日志 / 报告脱敏：把敏感值替换成掩码，空值不处理避免全量替换
pub fn mask_values(text: &str, secrets: &[String]) -> String {
    let mut out = text.to_string();
    for secret in secrets {
        if secret.len() >= 3 {
            out = out.replace(secret.as_str(), "******");
        }
    }
    out
}

fn call_args<'a>(expr: &'a str, name: &str) -> Option<&'a str> {
    expr.strip_prefix(name)?
        .strip_prefix('(')?
        .strip_suffix(')')
}

/// 收集模板中的变量名（`{{name}}` / `{{$name}}`），用于运行前预检
pub fn collect_placeholders(input: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut rest = input;
    while let Some(start) = rest.find("{{") {
        let Some(end_rel) = rest[start + 2..].find("}}") else {
            break;
        };
        let expr = rest[start + 2..start + 2 + end_rel].trim();
        let name = expr.strip_prefix('$').unwrap_or(expr);
        if !name.is_empty() {
            names.push(name.to_string());
        }
        rest = &rest[start + 2 + end_rel + 2..];
    }
    names
}

/// 是否为内置变量或函数型占位符（这些不需要用户提供）
pub fn is_builtin_placeholder(name: &str) -> bool {
    if name.starts_with("secret:") || name.starts_with("faker:") || name.contains('(') {
        return true;
    }
    resolve_time(name).is_some()
        || matches!(
            name,
            "uuid"
                | "guid"
                | "uuidNoDash"
                | "guidNoDash"
                | "uuidSimple"
                | "randomBool"
                | "randomHex"
                | "randomEmail"
                | "randomPhone"
                | "randomIPv4"
                | "randomInt"
                | "randomString"
        )
}

/* ---------- v1.1.2 占位符分类 ---------- */

/// 占位符类型：决定它是否需要用户提供、由谁生成
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaceholderKind {
    /// `{{host}}`：需要用户/环境提供
    User,
    /// `{{secret:TOKEN}}`：需要 TOKEN，输出时登记脱敏
    Secret,
    /// `{{os:USER}}`：从本机 OS 环境读取（需白名单）
    Os,
    /// `{{pool:token}}`：由数据池运行期注入
    Pool,
    /// `{{faker:name}}`：各节点本地生成
    Faker,
    /// `{{randomInt(1,9)}}`：各节点本地生成
    Function,
    /// `{{$timestamp}}` / `{{uuid}}`：各节点本地生成
    Builtin,
}

/// 把 `{{...}}` 中的表达式分类，并返回归一化后的变量名
///
/// 例：`secret:TOKEN` → (Secret, "TOKEN")；`os:USER` → (Os, "USER")；
/// `pool:token` → (Pool, "token")；`$timestamp` → (Builtin, "timestamp")。
pub fn classify_placeholder(expr: &str) -> (PlaceholderKind, String) {
    let trimmed = expr.trim();
    let bare = trimmed.strip_prefix('$').unwrap_or(trimmed);
    if let Some(name) = bare.strip_prefix("secret:") {
        return (PlaceholderKind::Secret, name.trim().to_string());
    }
    if let Some(name) = bare.strip_prefix("os:") {
        return (PlaceholderKind::Os, name.trim().to_string());
    }
    if let Some(name) = bare.strip_prefix("pool:") {
        return (PlaceholderKind::Pool, name.trim().to_string());
    }
    if bare.starts_with("faker:") {
        return (PlaceholderKind::Faker, bare.to_string());
    }
    if bare.contains('(') {
        return (PlaceholderKind::Function, bare.to_string());
    }
    if is_builtin_placeholder(bare) {
        return (PlaceholderKind::Builtin, bare.to_string());
    }
    (PlaceholderKind::User, bare.to_string())
}

/// 一段模板里出现的变量分类结果
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RequiredVars {
    pub user: Vec<String>,
    pub secret: Vec<String>,
    pub os: Vec<String>,
    pub pools: Vec<String>,
}

impl RequiredVars {
    fn push_unique(target: &mut Vec<String>, value: String) {
        if !value.is_empty() && !target.contains(&value) {
            target.push(value);
        }
    }

    fn merge(&mut self, other: RequiredVars) {
        for item in other.user {
            Self::push_unique(&mut self.user, item);
        }
        for item in other.secret {
            Self::push_unique(&mut self.secret, item);
        }
        for item in other.os {
            Self::push_unique(&mut self.os, item);
        }
        for item in other.pools {
            Self::push_unique(&mut self.pools, item);
        }
    }
}

/// 收集压测配置里所有可能含占位符的文本：URL、Query、Header、Body、认证字段
///
/// 变量体检、开跑前预检与集群下发共用这一份收集规则，避免各处漏字段导致
/// 「界面上说有值、跑起来报缺失」。
pub fn config_texts(config: &crate::models::StressConfig) -> Vec<String> {
    let mut texts = vec![config.url.trim().to_string()];
    for item in config.params.iter().chain(config.headers.iter()) {
        if !item.enabled {
            continue;
        }
        texts.push(item.key.trim().to_string());
        texts.push(item.value.clone());
    }
    if let Some(body) = &config.body {
        texts.push(body.clone());
    }
    if let Some(auth) = &config.auth {
        for value in [
            &auth.token,
            &auth.username,
            &auth.password,
            &auth.key,
            &auth.value,
            &auth.token_url,
            &auth.client_id,
            &auth.client_secret,
            &auth.scope,
        ] {
            texts.push(value.clone());
        }
    }
    // 多接口：每个接口的 URL / Query / Header / Body / 认证都可能引用变量
    for request in &config.requests {
        texts.push(request.url.trim().to_string());
        for item in request.params.iter().chain(request.headers.iter()) {
            if !item.enabled {
                continue;
            }
            texts.push(item.key.trim().to_string());
            texts.push(item.value.clone());
        }
        if let Some(body) = &request.body {
            texts.push(body.clone());
        }
        if let Some(auth) = &request.auth {
            for value in [
                &auth.token,
                &auth.username,
                &auth.password,
                &auth.key,
                &auth.value,
                &auth.token_url,
                &auth.client_id,
                &auth.client_secret,
                &auth.scope,
            ] {
                texts.push(value.clone());
            }
        }
    }
    texts
}

/// 扫描一段文本中的全部 `{{...}}`，按类型归类
pub fn required_vars_in_text(input: &str) -> RequiredVars {
    let mut result = RequiredVars::default();
    let mut rest = input;
    while let Some(start) = rest.find("{{") {
        let Some(end_rel) = rest[start + 2..].find("}}") else {
            break;
        };
        let expr = rest[start + 2..start + 2 + end_rel].trim();
        let (kind, name) = classify_placeholder(expr);
        match kind {
            PlaceholderKind::User => RequiredVars::push_unique(&mut result.user, name),
            PlaceholderKind::Secret => RequiredVars::push_unique(&mut result.secret, name),
            PlaceholderKind::Os => RequiredVars::push_unique(&mut result.os, name),
            PlaceholderKind::Pool => RequiredVars::push_unique(&mut result.pools, name),
            _ => {}
        }
        rest = &rest[start + 2 + end_rel + 2..];
    }
    result
}

/// 合并多段模板的变量分类
pub fn required_vars_in_texts<'a>(texts: impl IntoIterator<Item = &'a str>) -> RequiredVars {
    let mut result = RequiredVars::default();
    for text in texts {
        result.merge(required_vars_in_text(text));
    }
    result
}

/// 递归扫描 JSON 中所有字符串值，得到变量分类
pub fn required_vars_in_json(value: &serde_json::Value) -> RequiredVars {
    let mut result = RequiredVars::default();
    fn walk(value: &serde_json::Value, result: &mut RequiredVars) {
        match value {
            serde_json::Value::String(text) => result.merge(required_vars_in_text(text)),
            serde_json::Value::Array(items) => {
                for item in items {
                    walk(item, result);
                }
            }
            serde_json::Value::Object(map) => {
                for item in map.values() {
                    walk(item, result);
                }
            }
            _ => {}
        }
    }
    walk(value, &mut result);
    result
}

/// 操作系统变量在运行期变量表里的专用键前缀
///
/// `{{os:NAME}}` 的语义是「本机系统值」，不能被环境变量或数据池里的同名变量顶掉，
/// 因此注入时把系统值写到这个前缀下，`resolve_expr` 只认这个键。
/// 前缀含双下划线且带冒号，不会与合法的池列名（字母 / 下划线开头，仅字母数字下划线）冲突。
pub const OS_VAR_PREFIX: &str = "__os__:";

/// 操作系统变量的专用键名
pub fn os_var_key(name: &str) -> String {
    format!("{OS_VAR_PREFIX}{name}")
}

/// 判断 OS 环境变量是否在允许清单内
///
/// 规则：逗号分隔；`*` 允许全部；`PREFIX*` 前缀匹配；其余按名称精确匹配（大小写敏感）。
pub fn os_var_allowed(allow: &str, name: &str) -> bool {
    allow.split(',').any(|item| {
        let item = item.trim();
        if item.is_empty() {
            return false;
        }
        if item == "*" {
            return true;
        }
        match item.strip_suffix('*') {
            Some(prefix) => !prefix.is_empty() && name.starts_with(prefix),
            None => item == name,
        }
    })
}

/// 敏感值掩码：长 token 保留前 6 后 4，短值只保留前 2
pub fn mask_secret_value(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    let len = chars.len();
    if len == 0 {
        return "***".to_string();
    }
    if len > 16 {
        let head: String = chars[..6].iter().collect();
        let tail: String = chars[len - 4..].iter().collect();
        return format!("{head}...{tail}");
    }
    if len > 4 {
        let head: String = chars[..2].iter().collect();
        return format!("{head}***");
    }
    "***".to_string()
}

fn int_args(inner: &str) -> Option<Vec<i64>> {
    inner
        .split(',')
        .map(|part| part.trim().parse::<i64>().ok())
        .collect()
}

/// 用 UUID 作为熵源，避免额外引入随机数依赖
fn entropy() -> u128 {
    uuid::Uuid::new_v4().as_u128()
}

fn random_int(min: i64, max: i64) -> i64 {
    if max <= min {
        return min;
    }
    let span = (max as i128 - min as i128 + 1) as u128;
    (min as i128 + (entropy() % span) as i128) as i64
}

fn random_float(min: f64, max: f64, decimals: usize) -> String {
    let ratio = entropy() as f64 / u128::MAX as f64;
    let value = if max > min {
        min + (max - min) * ratio
    } else {
        min
    };
    format!("{value:.decimals$}")
}

fn random_string(len: usize, pool: &str) -> String {
    const ALNUM: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    const ALPHA: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

    let chars: Vec<char> = match pool.trim().to_ascii_lowercase().as_str() {
        "num" | "number" | "digit" | "digits" => "0123456789".chars().collect(),
        "hex" => "0123456789abcdef".chars().collect(),
        "lower" => "abcdefghijklmnopqrstuvwxyz".chars().collect(),
        "upper" => "ABCDEFGHIJKLMNOPQRSTUVWXYZ".chars().collect(),
        "alpha" | "letter" | "letters" => ALPHA.chars().collect(),
        _ => ALNUM.chars().collect(),
    };

    let len = len.clamp(1, 4096);
    let mut out = String::with_capacity(len);
    for _ in 0..len {
        out.push(chars[(entropy() % chars.len() as u128) as usize]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_sqlite_busy_to_error_code() {
        let text = err("database is locked");
        assert!(text.starts_with("E_BUSY:"), "{text}");
        assert!(text.contains("关闭 GUI"));
    }

    #[test]
    fn keeps_normal_errors_unchanged() {
        assert_eq!(err("文件不存在"), "文件不存在");
    }

    fn vars(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
            .collect()
    }

    #[test]
    fn strip_comments_keeps_urls_inside_strings() {
        let raw = "{\n  // 行注释\n  \"url\": \"https://example.com/a//b\",\n  /* 块注释 */\n  \"n\": 1\n}";
        let clean = strip_json_comments(raw);
        assert!(clean.contains("https://example.com/a//b"));
        assert!(!clean.contains("行注释"));
        assert!(!clean.contains("块注释"));
        assert!(serde_json::from_str::<serde_json::Value>(clean.trim()).is_ok());
    }

    #[test]
    fn strip_comments_handles_escaped_quotes_and_unclosed_block() {
        let raw = "{\"a\":\"say \\\"hi\\\" // 不是注释\",\"b\":1 /* 未闭合";
        let clean = strip_json_comments(raw);
        assert!(clean.contains("say \\\"hi\\\" // 不是注释"));
        assert!(!clean.contains("未闭合"));
    }

    #[test]
    fn render_replaces_custom_vars_and_keeps_unknown() {
        let map = vars(&[("host", "api.local"), ("token", "abc")]);
        assert_eq!(
            render("http://{{host}}/v1?t={{token}}", &map),
            "http://api.local/v1?t=abc"
        );
        assert_eq!(render("{{ missing }}", &map), "{{missing}}");
    }

    #[test]
    fn render_supports_dollar_prefixed_builtins() {
        let map = HashMap::new();
        let out = render("{{$uuid}}", &map);
        assert_eq!(out.len(), 36);
        assert!(out.contains('-'));
        let ts = render("{{$timestamp}}", &map);
        assert!(ts.parse::<i64>().unwrap() > 1_600_000_000_000);
        let simple = render("{{uuidNoDash}}", &map);
        assert_eq!(simple.len(), 32);
        assert!(!simple.contains('-'));
    }

    #[test]
    fn os_placeholder_reads_dedicated_key_only() {
        // 数据池 / 环境变量的同名变量不得顶掉系统值
        let mut map = vars(&[("USER", "from-pool")]);
        map.insert(os_var_key("USER"), "system-user".to_string());
        assert_eq!(render("{{os:USER}}", &map), "system-user");
        assert_eq!(render("{{USER}}", &map), "from-pool");
        // 没有专用键时按未解析保留，绝不回退到普通变量
        assert_eq!(
            render("{{os:USER}}", &vars(&[("USER", "from-pool")])),
            "{{os:USER}}"
        );
    }

    #[test]
    fn render_custom_var_wins_over_builtin() {
        let map = vars(&[("uuid", "fixed")]);
        assert_eq!(render("{{uuid}}", &map), "fixed");
    }

    #[test]
    fn render_function_vars_respect_bounds() {
        let map = HashMap::new();
        for _ in 0..20 {
            let value = render("{{randomInt(5,9)}}", &map);
            let parsed: i64 = value.parse().unwrap();
            assert!((5..=9).contains(&parsed));
        }
        let hex = render("{{randomHex(8)}}", &map);
        assert_eq!(hex.len(), 8);
        assert!(hex.chars().all(|c| c.is_ascii_hexdigit()));
        let digits = render("{{randomNumber(6)}}", &map);
        assert_eq!(digits.len(), 6);
        assert!(digits.chars().all(|c| c.is_ascii_digit()));
        let floats = render("{{randomFloat(1,2,3)}}", &map);
        let parsed: f64 = floats.parse().unwrap();
        assert!((1.0..=2.0).contains(&parsed));
        assert_eq!(floats.split('.').next_back().unwrap().len(), 3);
    }

    #[test]
    fn random_string_clamps_length_and_pool() {
        assert_eq!(random_string(0, "hex").len(), 1);
        assert_eq!(random_string(5000, "hex").len(), 4096);
        let upper = random_string(16, "upper");
        assert!(upper.chars().all(|c| c.is_ascii_uppercase()));
    }

    #[test]
    fn render_supports_nested_paths_via_flatten() {
        let mut map = HashMap::new();
        let data = serde_json::json!({
            "user": {"id": 7, "name": "astro", "tags": ["a", "b"]},
            "empty": null
        });
        flatten_json("", &data, &mut map);
        assert_eq!(render("{{user.id}}/{{user.name}}", &map), "7/astro");
        assert_eq!(render("{{user.tags.1}}", &map), "b");
        assert_eq!(render("{{empty}}", &map), "");
        assert!(render("{{user}}", &map).contains("\"id\":7"));
    }

    #[test]
    fn render_secret_prefix_collects_values() {
        let map = vars(&[("token", "s3cr3t-value")]);
        let mut used = Vec::new();
        assert_eq!(
            render_tracked("Bearer {{secret:token}}", &map, &mut used),
            "Bearer s3cr3t-value"
        );
        assert_eq!(used, vec!["s3cr3t-value".to_string()]);
        assert_eq!(mask_values("Bearer s3cr3t-value", &used), "Bearer ******");
        // 未定义的 secret 保留占位，方便发现拼写错误
        let mut none = Vec::new();
        assert_eq!(
            render_tracked("{{secret:nope}}", &map, &mut none),
            "{{secret:nope}}"
        );
    }

    #[test]
    fn random_int_handles_degenerate_range() {
        assert_eq!(random_int(3, 3), 3);
        assert_eq!(random_int(5, 2), 5);
    }

    #[test]
    fn collect_placeholders_handles_builtin_and_secret_forms() {
        let names =
            collect_placeholders("{{ host }}/{{$uuid}}?t={{secret:TOKEN}}&x={{randomInt(1,3)}}");
        assert_eq!(
            names,
            vec![
                "host".to_string(),
                "uuid".to_string(),
                "secret:TOKEN".to_string(),
                "randomInt(1,3)".to_string()
            ]
        );
        assert!(is_builtin_placeholder("uuid"));
        assert!(is_builtin_placeholder("timestamp"));
        assert!(is_builtin_placeholder("secret:TOKEN"));
        assert!(is_builtin_placeholder("faker:email"));
        assert!(is_builtin_placeholder("randomInt(1,3)"));
        assert!(!is_builtin_placeholder("host"));
    }

    #[test]
    fn render_expands_faker_and_keeps_unknown() {
        let map = HashMap::new();
        let email = render("{{faker:email}}", &map);
        assert!(email.contains('@'));
        assert!(!email.contains("{{"));
        let value: i64 = render("{{faker:int(2,4)}}", &map).parse().unwrap();
        assert!((2..=4).contains(&value));
        // 拼写错误保留原样，提醒用户而不是悄悄变成空串
        assert_eq!(render("{{faker:nope}}", &map), "{{faker:nope}}");
        // 自定义变量不抢占 faker 命名空间
        let map = vars(&[("faker:email", "fixed")]);
        assert!(render("{{faker:email}}", &map).contains('@'));
    }
}
