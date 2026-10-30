//! 套件运行前预检
//!
//! 逐条列出会阻止运行的问题（缺接口 / 缺脚本 / 缺变量 / 依赖环）和需要提醒的
//! 跳过项（接口 JS 钩子、环境脚本、接口 assertion 字段）。`strictScripts=true`
//! 时跳过项升级为 error，避免「看起来通过、实际没校验」。

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use rusqlite::{params, OptionalExtension};
use tauri::State;

use crate::commands::cookie::CookieMode;
use crate::commands::http::load_vars_with_secrets;
use crate::commands::request::find_request;
use crate::commands::test_case::load_cases;
use crate::commands::test_suite::{find_suite, load_datasets};
use crate::models::{
    HttpRequestDetail, PreflightItem, PreflightReport, RequestAuth, StressConfig, TestCase,
    TestDataset,
};
use crate::state::AppState;
use crate::test_runner::SuiteConfig;
use crate::util::{self, err};

#[tauri::command]
pub fn test_preflight(
    state: State<'_, AppState>,
    suite_id: i64,
    env_id: Option<i64>,
) -> Result<PreflightReport, String> {
    run_preflight(state.inner(), suite_id, env_id)
}

/// 预检实现：命令与 CLI 共用
pub fn run_preflight(
    state: &AppState,
    suite_id: i64,
    env_id: Option<i64>,
) -> Result<PreflightReport, String> {
    run_preflight_with(state, suite_id, env_id, &HashMap::new(), &HashSet::new())
}

/// 预检实现（带 CLI `--env-file` 注入的变量 / 密钥）
pub fn run_preflight_with(
    state: &AppState,
    suite_id: i64,
    env_id: Option<i64>,
    extra_vars: &HashMap<String, String>,
    extra_secrets: &HashSet<String>,
) -> Result<PreflightReport, String> {
    let conn = state.pool.get().map_err(err)?;
    let suite = find_suite(&conn, suite_id)?;
    let config = SuiteConfig::parse(suite.config_json.as_deref())?;
    let all_cases = load_cases(&conn, suite_id)?;
    let datasets = load_datasets(&conn, suite_id, true)?;
    let enabled: Vec<&TestCase> = all_cases.iter().filter(|case| case.enabled).collect();

    let mut builder = ReportBuilder::new(config.strict_scripts);

    let cookie_mode = {
        let raw = config.cookie_mode.clone().unwrap_or_else(|| "shared".into());
        let normalized = raw.trim().to_ascii_lowercase();
        if !matches!(normalized.as_str(), "" | "shared" | "isolated") {
            builder.warning(
                "COOKIE_MODE_INVALID",
                None,
                format!("未知的 cookieMode：{raw}，按 shared 处理"),
                None,
                false,
            );
        }
        match CookieMode::parse(&raw) {
            CookieMode::Isolated => "isolated".to_string(),
            CookieMode::Shared => "shared".to_string(),
        }
    };

    // 环境：变量、名称、脚本
    let env_id = env_id.or(suite.env_id);
    let mut env_vars: HashMap<String, String> = HashMap::new();
    let mut env_secrets: HashSet<String> = HashSet::new();
    let mut env_name: Option<String> = None;
    let mut env_script = String::new();
    if let Some(id) = env_id {
        let (vars, secrets) = load_vars_with_secrets(&conn, id)?;
        env_vars = vars;
        env_secrets = secrets;
        let row: Option<(String, Option<String>)> = conn
            .query_row(
                "SELECT name, script FROM environments WHERE id = ?1",
                params![id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(err)?;
        match row {
            Some((name, script)) => {
                env_name = Some(name);
                env_script = script.unwrap_or_default();
            }
            None => builder.error(
                "ENV_NOT_FOUND",
                None,
                format!("环境 #{id} 不存在"),
                None,
            ),
        }
    }
    // CLI `--env-file` 注入的密钥同样视为已提供
    env_secrets.extend(extra_secrets.iter().cloned());
    if !env_script.trim().is_empty() {
        builder.warning(
            "ENV_SCRIPT_SKIPPED",
            None,
            "目标环境带 JS 脚本，套件运行时会跳过（签名 / 加解密逻辑需迁移到 Python 钩子）"
                .to_string(),
            Some("套件设置 → 钩子，或环境详情中删除脚本后迁移"),
            true,
        );
    }

    if enabled.is_empty() {
        builder.error(
            "NO_CASES",
            None,
            "套件没有启用的用例，无法运行".to_string(),
            None,
        );
    }

    // Python 钩子：授权开关、解释器与版本（文档 4.6）
    let script_present =
        |text: &Option<String>| text.as_deref().map(str::trim).is_some_and(|value| !value.is_empty());
    let needs_python = enabled.iter().any(|case| {
        script_present(&case.main_script)
            || script_present(&case.pre_script)
            || script_present(&case.post_script)
    }) || script_present(&suite.setup_script)
        || script_present(&suite.teardown_script);
    if needs_python {
        let settings = state.settings();
        if !settings.python_enabled {
            builder.error(
                "PYTHON_NOT_AUTHORIZED",
                None,
                "套件包含 Python 脚本，但未开启「允许执行本地 Python 脚本」".to_string(),
                Some("设置 → 网络与安全 → 打开 Python 脚本开关"),
            );
        } else {
            let configured = suite
                .python_path
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or(settings.python_path.trim());
            let check = crate::python_exec::check(configured, true);
            if check.info.is_none() {
                builder.error(
                    "PYTHON_UNAVAILABLE",
                    None,
                    check.message,
                    Some("设置 → Python 解释器路径（留空自动探测）"),
                );
            }
        }
    }

    // 依赖环与未知依赖
    check_dependencies(&mut builder, &enabled);

    let mut checked_requests: HashSet<i64> = HashSet::new();

    for case in &enabled {
        let mut available: HashSet<String> = env_vars.keys().cloned().collect();
        available.extend(extra_vars.keys().cloned());
        available.extend(config.vars.keys().cloned());
        available.extend(case_variable_keys(&mut builder, case));
        if let Some(dataset) = case
            .dataset_id
            .and_then(|id| datasets.iter().find(|item| item.id == id))
        {
            available.extend(dataset_columns(dataset, &state.data_dir));
        } else if case.dataset_id.is_some() {
            builder.error(
                "DATASET_MISSING",
                Some(case),
                "用例引用的数据集不存在".to_string(),
                None,
            );
        }

        match case.kind.as_str() {
            "request" => {
                let Some(request_id) = case.request_id else {
                    builder.error(
                        "MISSING_REQUEST",
                        Some(case),
                        "请求用例未绑定接口".to_string(),
                        None,
                    );
                    continue;
                };
                let request = match find_request(&conn, request_id) {
                    Ok(request) => request,
                    Err(message) => {
                        builder.error("REQUEST_NOT_FOUND", Some(case), message, None);
                        continue;
                    }
                };
                if checked_requests.insert(request_id) {
                    check_skipped_scripts(&mut builder, case, &request);
                }
                check_request_variables(&mut builder, case, &request, &available, &env_secrets);
                check_case_assertions(&mut builder, case, &available, &env_secrets);
            }
            "python" => {
                if case
                    .main_script
                    .as_deref()
                    .map(str::trim)
                    .unwrap_or_default()
                    .is_empty()
                {
                    builder.error(
                        "MISSING_SCRIPT",
                        Some(case),
                        "Python 用例没有 main_script".to_string(),
                        Some("在用例编辑器中编写 case_main.py"),
                    );
                }
            }
            "stress" => {
                check_stress_case(&mut builder, case, &available, &env_secrets);
            }
            other => builder.error(
                "CASE_KIND_INVALID",
                Some(case),
                format!("不支持的用例类型：{other}"),
                None,
            ),
        }
    }

    Ok(builder.finish(
        suite_id,
        suite.name,
        env_id,
        env_name,
        cookie_mode,
        all_cases.len() as i64,
        enabled.len() as i64,
    ))
}

struct ReportBuilder {
    strict: bool,
    items: Vec<PreflightItem>,
    errors: i64,
    warnings: i64,
}

impl ReportBuilder {
    fn new(strict: bool) -> Self {
        Self {
            strict,
            items: Vec::new(),
            errors: 0,
            warnings: 0,
        }
    }

    fn error(
        &mut self,
        code: &str,
        case: Option<&TestCase>,
        message: String,
        hint: Option<&str>,
    ) {
        self.errors += 1;
        self.items.push(PreflightItem {
            level: "error".to_string(),
            code: code.to_string(),
            message,
            case_id: case.map(|item| item.id),
            case_name: case.map(|item| item.name.clone()),
            hint: hint.map(str::to_string),
        });
    }

    /// `upgrade_in_strict`：跳过类提醒在 strictScripts 下升级为 error
    fn warning(
        &mut self,
        code: &str,
        case: Option<&TestCase>,
        message: String,
        hint: Option<&str>,
        upgrade_in_strict: bool,
    ) {
        if upgrade_in_strict && self.strict {
            self.error(code, case, message, hint);
            return;
        }
        self.warnings += 1;
        self.items.push(PreflightItem {
            level: "warning".to_string(),
            code: code.to_string(),
            message,
            case_id: case.map(|item| item.id),
            case_name: case.map(|item| item.name.clone()),
            hint: hint.map(str::to_string),
        });
    }

    fn finish(
        self,
        suite_id: i64,
        suite_name: String,
        env_id: Option<i64>,
        env_name: Option<String>,
        cookie_mode: String,
        total_cases: i64,
        enabled_cases: i64,
    ) -> PreflightReport {
        PreflightReport {
            suite_id,
            suite_name,
            env_id,
            env_name,
            cookie_mode,
            strict_scripts: self.strict,
            total_cases,
            enabled_cases,
            errors: self.errors,
            warnings: self.warnings,
            runnable: self.errors == 0,
            items: self.items,
        }
    }
}

fn case_variable_keys(builder: &mut ReportBuilder, case: &TestCase) -> HashSet<String> {
    let mut keys = HashSet::new();
    let Some(text) = case
        .variables_json
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
    else {
        return keys;
    };
    match serde_json::from_str::<serde_json::Value>(text) {
        Ok(serde_json::Value::Object(map)) => {
            for (key, child) in map {
                let mut flat = HashMap::new();
                util::flatten_json(&key, &child, &mut flat);
                keys.extend(flat.into_keys());
            }
        }
        Ok(_) => builder.error(
            "CASE_VARIABLES_INVALID",
            Some(case),
            "用例变量必须是 JSON 对象".to_string(),
            None,
        ),
        Err(error) => builder.error(
            "CASE_VARIABLES_INVALID",
            Some(case),
            format!("用例变量不是合法 JSON：{error}"),
            None,
        ),
    }
    keys
}

fn check_dependencies(builder: &mut ReportBuilder, cases: &[&TestCase]) {
    let ids: HashSet<i64> = cases.iter().map(|case| case.id).collect();
    let mut edges: HashMap<i64, Vec<i64>> = HashMap::new();

    for case in cases {
        let mut deps = Vec::new();
        if let Some(raw) = case.depends_on.as_deref() {
            for token in raw.split(',') {
                let Ok(id) = token.trim().parse::<i64>() else {
                    continue;
                };
                if id == case.id {
                    builder.error(
                        "DEPENDENCY_SELF",
                        Some(case),
                        "用例不能依赖自身".to_string(),
                        None,
                    );
                    continue;
                }
                if !ids.contains(&id) {
                    builder.warning(
                        "DEPENDENCY_MISSING",
                        Some(case),
                        format!("依赖的用例 #{id} 不存在或未启用"),
                        None,
                        false,
                    );
                    continue;
                }
                deps.push(id);
            }
        }
        edges.insert(case.id, deps);
    }

    let mut state: HashMap<i64, u8> = HashMap::new();
    let mut stack: Vec<i64> = Vec::new();
    for case in cases {
        if let Some(cycle) = find_cycle(case.id, &edges, &mut state, &mut stack) {
            let names: Vec<String> = cycle
                .iter()
                .map(|id| {
                    cases
                        .iter()
                        .find(|case| case.id == *id)
                        .map(|case| format!("#{} {}", case.id, case.name))
                        .unwrap_or_else(|| format!("#{id}"))
                })
                .chain(std::iter::once(
                    cycle
                        .first()
                        .map(|id| format!("#{id}"))
                        .unwrap_or_default(),
                ))
                .collect();
            builder.error(
                "DEPENDENCY_CYCLE",
                None,
                format!("用例依赖成环：{}", names.join(" → ")),
                Some("打依赖环中的任意一条依赖"),
            );
            break;
        }
    }
}

fn find_cycle(
    node: i64,
    edges: &HashMap<i64, Vec<i64>>,
    state: &mut HashMap<i64, u8>,
    stack: &mut Vec<i64>,
) -> Option<Vec<i64>> {
    match state.get(&node).copied() {
        Some(1) => {
            let start = stack.iter().position(|id| *id == node).unwrap_or(0);
            return Some(stack[start..].to_vec());
        }
        Some(2) => return None,
        _ => {}
    }
    state.insert(node, 1);
    stack.push(node);
    if let Some(children) = edges.get(&node) {
        for &child in children {
            if state.get(&child).copied() == Some(1) {
                let start = stack.iter().position(|id| *id == child).unwrap_or(0);
                return Some(stack[start..].to_vec());
            }
            if let Some(cycle) = find_cycle(child, edges, state, stack) {
                return Some(cycle);
            }
        }
    }
    stack.pop();
    state.insert(node, 2);
    None
}

fn check_skipped_scripts(
    builder: &mut ReportBuilder,
    case: &TestCase,
    request: &HttpRequestDetail,
) {
    if !request.pre_script.trim().is_empty() || !request.post_script.trim().is_empty() {
        builder.warning(
            "JS_HOOK_SKIPPED",
            Some(case),
            format!(
                "接口『{}』配置了 JS 前置 / 后置脚本，套件运行时会跳过",
                request.name
            ),
            Some("把逻辑迁移到用例级 pre_script / post_script（Python）"),
            true,
        );
    }
    if !request.assertions.trim().is_empty() {
        builder.warning(
            "REQUEST_ASSERTIONS_SKIPPED",
            Some(case),
            format!(
                "接口『{}』的 assertion 字段不会执行，请在用例断言中重写",
                request.name
            ),
            Some("迁移到用例编辑器的声明式断言"),
            true,
        );
    }
}

fn check_request_variables(
    builder: &mut ReportBuilder,
    case: &TestCase,
    request: &HttpRequestDetail,
    available: &HashSet<String>,
    secrets: &HashSet<String>,
) {
    let mut placeholders = vec![request.url.clone()];
    for item in request.headers.iter().chain(request.params.iter()) {
        if !item.enabled {
            continue;
        }
        placeholders.push(item.key.clone());
        placeholders.push(item.value.clone());
    }
    if !request.body_type.is_empty() && request.body_type != "none" {
        placeholders.push(request.body.clone());
    }
    placeholders.extend(auth_texts(&request.auth).into_iter().map(str::to_string));

    check_missing_variables(builder, case, &placeholders, available, secrets);
}

fn check_case_assertions(
    builder: &mut ReportBuilder,
    case: &TestCase,
    available: &HashSet<String>,
    secrets: &HashSet<String>,
) {
    if let Some(text) = case.assertions_json.as_deref() {
        check_missing_variables(builder, case, &[text.to_string()], available, secrets);
    }
}

fn check_stress_case(
    builder: &mut ReportBuilder,
    case: &TestCase,
    available: &HashSet<String>,
    secrets: &HashSet<String>,
) {
    let Some(raw) = case
        .stress_json
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
    else {
        builder.error(
            "MISSING_STRESS_CONFIG",
            Some(case),
            "压测用例缺少内联 StressConfig".to_string(),
            Some("重新从压测页另存为用例，确保配置被内联"),
        );
        return;
    };
    let config: StressConfig = match serde_json::from_str(raw) {
        Ok(config) => config,
        Err(error) => {
            builder.error(
                "STRESS_CONFIG_INVALID",
                Some(case),
                format!("压测配置不是合法 JSON：{error}"),
                None,
            );
            return;
        }
    };
    if config.url.trim().is_empty() {
        builder.error(
            "STRESS_URL_EMPTY",
            Some(case),
            "压测配置缺少 URL".to_string(),
            None,
        );
    }

    let mut available = available.clone();
    available.extend(config.vars.keys().cloned());

    let mut placeholders = vec![config.url.clone()];
    for item in config.headers.iter().chain(config.params.iter()) {
        if !item.enabled {
            continue;
        }
        placeholders.push(item.key.clone());
        placeholders.push(item.value.clone());
    }
    if let Some(body) = config.body.as_deref() {
        placeholders.push(body.to_string());
    }
    if let Some(auth) = config.auth.as_ref() {
        placeholders.extend(auth_texts(auth).into_iter().map(str::to_string));
    }
    check_missing_variables(builder, case, &placeholders, &available, secrets);
}

fn auth_texts(auth: &RequestAuth) -> Vec<&str> {
    vec![
        &auth.token,
        &auth.username,
        &auth.password,
        &auth.key,
        &auth.value,
        &auth.token_url,
        &auth.client_id,
        &auth.client_secret,
        &auth.scope,
    ]
}

fn check_missing_variables(
    builder: &mut ReportBuilder,
    case: &TestCase,
    texts: &[String],
    available: &HashSet<String>,
    secrets: &HashSet<String>,
) {
    let mut missing: Vec<String> = Vec::new();
    let mut missing_secrets: Vec<String> = Vec::new();

    for text in texts {
        for name in util::collect_placeholders(text) {
            if let Some(secret) = name.strip_prefix("secret:") {
                if !available.contains(secret) && !secrets.contains(secret) {
                    missing_secrets.push(secret.to_string());
                }
                continue;
            }
            if util::is_builtin_placeholder(&name) || available.contains(&name) {
                continue;
            }
            let root = name.split('.').next().unwrap_or(&name);
            if available.contains(root) {
                continue;
            }
            missing.push(name);
        }
    }

    missing.sort();
    missing.dedup();
    missing_secrets.sort();
    missing_secrets.dedup();

    for name in missing {
        builder.error(
            "MISSING_VARIABLE",
            Some(case),
            format!("引用了未定义的变量 {{{{{name}}}}}"),
            Some("在环境变量 / 套件变量 / 数据集或运行参数中补充"),
        );
    }
    for secret in missing_secrets {
        builder.error(
            "MISSING_SECRET",
            Some(case),
            format!("未注入的密钥 secret:{secret}"),
            Some("通过 --env-file 或同名环境变量注入，禁止以空值运行"),
        );
    }
}

fn dataset_columns(dataset: &TestDataset, data_dir: &Path) -> Vec<String> {
    let table = match dataset.content.as_deref().filter(|text| !text.trim().is_empty()) {
        Some(content) => crate::test_dataset::parse(&dataset.format, content).ok(),
        None => dataset
            .path
            .as_deref()
            .map(str::trim)
            .filter(|path| !path.is_empty())
            .and_then(|path| {
                let full = if Path::new(path).is_absolute() {
                    PathBuf::from(path)
                } else {
                    data_dir.join(path)
                };
                crate::test_dataset::parse_file(&full).ok()
            }),
    };
    table.map(|table| table.columns).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn case(id: i64, name: &str, depends: Option<&str>) -> TestCase {
        TestCase {
            id,
            suite_id: 1,
            name: name.to_string(),
            kind: "request".to_string(),
            request_id: None,
            plan_id: None,
            dataset_id: None,
            depends_on: depends.map(str::to_string),
            sort_order: id,
            enabled: true,
            timeout_ms: 30_000,
            retries: 0,
            retry_on_failure: false,
            continue_on_failure: false,
            tags: None,
            variables_json: None,
            assertions_json: None,
            pre_script: None,
            post_script: None,
            main_script: None,
            stress_json: None,
            created_at: 0,
            updated_at: 0,
        }
    }

    #[test]
    fn detects_dependency_cycle() {
        let cases = vec![case(1, "a", Some("2")), case(2, "b", Some("1"))];
        let refs: Vec<&TestCase> = cases.iter().collect();
        let mut builder = ReportBuilder::new(false);
        check_dependencies(&mut builder, &refs);
        assert_eq!(builder.errors, 1);
        assert!(builder.items.iter().any(|item| item.code == "DEPENDENCY_CYCLE"));
    }

    #[test]
    fn missing_variable_skips_builtin_and_secret_forms() {
        let case = case(1, "a", None);
        let mut available = HashSet::new();
        available.insert("host".to_string());
        let texts = vec![
            "http://{{host}}/{{missing}}?t={{$timestamp}}&k={{secret:TOKEN}}".to_string(),
        ];
        let mut builder = ReportBuilder::new(false);
        check_missing_variables(&mut builder, &case, &texts, &available, &HashSet::new());
        assert_eq!(builder.errors, 2);
        assert!(builder.items.iter().any(|item| item.code == "MISSING_VARIABLE"));
        assert!(builder.items.iter().any(|item| item.code == "MISSING_SECRET"));
    }

    #[test]
    fn strict_mode_upgrades_skipped_scripts_to_error() {
        let mut builder = ReportBuilder::new(true);
        builder.warning(
            "JS_HOOK_SKIPPED",
            None,
            "skip".to_string(),
            None,
            true,
        );
        assert_eq!(builder.errors, 1);
        assert_eq!(builder.warnings, 0);
        assert!(!builder.finish(1, "s".into(), None, None, "shared".into(), 1, 1).runnable);
    }
}
