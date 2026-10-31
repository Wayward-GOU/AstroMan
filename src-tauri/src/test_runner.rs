//! 套件执行引擎
//!
//! 批次一范围：HTTP 用例 + 声明式断言 + 运行落库 + 事件回调。引擎本身不依赖
//! Tauri（事件通过回调交给命令层），CLI 复用同一入口；Python / 压测用例在
//! 后续批次接入，未实现前按 `error` 记录，不会静默跳过。

use crate::commands::cookie::CookieJar;
use crate::commands::http::{self, load_vars_with_secrets, RecordOptions};
use crate::commands::request::find_request;
use crate::commands::test_case::load_cases;
use crate::commands::test_suite::{find_suite, load_datasets};
use crate::models::{
    HttpRequestDetail, HttpResult, KeyValueItem, RequestAuth, RunSummary, SendRequestPayload,
    SlaTargets, StressConfig, StressReport, TestCase, TestDataset, TestRun,
};
use crate::python_exec::{
    self, PythonAttachment, PythonInfo, PythonOutcome, PythonRequestOverride, PythonSpec,
};
use crate::state::AppState;
use crate::test_assert::{self, AssertInput, AssertionOutcome};
use crate::test_dataset::{self, DatasetRow};
use crate::util::{self, err, now_ms};
use rusqlite::{params, OptionalExtension};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const MAX_REQUEST_SNAPSHOT: usize = 256 * 1024;
const MAX_RESPONSE_SNAPSHOT: usize = 1024 * 1024;
const MAX_LOGS: usize = 64 * 1024;
/// 进度事件合帧间隔
const PROGRESS_INTERVAL: Duration = Duration::from_millis(200);

/// 套件配置（存于 `test_suites.config_json`）
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SuiteConfig {
    pub schema_version: Option<String>,
    pub fail_fast: bool,
    pub max_workers: u32,
    pub default_timeout_ms: Option<i64>,
    pub retries: i64,
    pub parallel_rows: bool,
    pub cookie_mode: Option<String>,
    pub strict_scripts: bool,
    pub suite_timeout_sec: Option<u64>,
    /// Python 钩子超时，默认 60s（用例剩余预算更小时取更小值）
    pub python_timeout_ms: Option<i64>,
    /// teardown 失败是否让整次运行失败，默认 true
    pub teardown_failure_fatal: bool,
    pub vars: HashMap<String, String>,
    /// 需要脱敏的变量名（本机变量还可通过 is_secret 标记）
    pub secrets: Vec<String>,
}

impl Default for SuiteConfig {
    fn default() -> Self {
        Self {
            schema_version: None,
            fail_fast: false,
            max_workers: 1,
            default_timeout_ms: Some(30_000),
            retries: 0,
            parallel_rows: false,
            cookie_mode: None,
            strict_scripts: false,
            suite_timeout_sec: Some(0),
            python_timeout_ms: Some(python_exec::DEFAULT_TIMEOUT_MS as i64),
            teardown_failure_fatal: true,
            vars: HashMap::new(),
            secrets: Vec::new(),
        }
    }
}

impl SuiteConfig {
    pub(crate) fn parse(raw: Option<&str>) -> Result<Self, String> {
        match raw.map(str::trim).filter(|text| !text.is_empty()) {
            Some(text) => serde_json::from_str(text).map_err(|e| format!("套件配置格式错误：{e}")),
            None => Ok(Self::default()),
        }
    }

    fn cookie_isolated(&self) -> bool {
        matches!(
            crate::commands::cookie::CookieMode::parse(self.cookie_mode.as_deref().unwrap_or("")),
            crate::commands::cookie::CookieMode::Isolated
        )
    }

    fn fail_fast_effective(&self) -> bool {
        self.fail_fast
    }

    fn suite_timeout(&self) -> Option<Duration> {
        self.suite_timeout_sec
            .filter(|secs| *secs > 0)
            .map(Duration::from_secs)
    }

    fn python_hook_timeout(&self) -> Duration {
        self.python_timeout_ms
            .filter(|value| *value > 0)
            .map(|value| Duration::from_millis(value.clamp(500, 600_000) as u64))
            .unwrap_or(Duration::from_millis(python_exec::DEFAULT_TIMEOUT_MS))
    }
}

/// 引擎事件：命令层负责 `app.emit(name, payload)`
#[derive(Debug, Clone)]
pub struct TestEvent {
    pub name: &'static str,
    pub payload: serde_json::Value,
}

pub type EventSink<'a> = dyn Fn(TestEvent) + Send + Sync + 'a;

pub struct RunOptions {
    pub trigger: String,
    pub env_id: Option<i64>,
    pub vars: HashMap<String, String>,
    pub secrets: HashMap<String, String>,
    pub fail_fast: Option<bool>,
    /// CLI `--tags`：为空表示不过滤；命中任意标签即执行
    pub tags: Vec<String>,
    /// CLI `--exclude-tags`：命中任意标签即跳过
    pub exclude_tags: Vec<String>,
    /// CLI `--timeout`：覆盖套件总超时（秒）
    pub timeout_sec: Option<u64>,
    /// CLI `--data`：用指定文件覆盖套件数据集
    pub dataset_override: Option<std::path::PathBuf>,
}

impl Default for RunOptions {
    fn default() -> Self {
        Self {
            trigger: "ui".to_string(),
            env_id: None,
            vars: HashMap::new(),
            secrets: HashMap::new(),
            fail_fast: None,
            tags: Vec::new(),
            exclude_tags: Vec::new(),
            timeout_sec: None,
            dataset_override: None,
        }
    }
}

/// 变量袋：合并各层变量并记录需要打码的明文
#[derive(Default)]
struct VarBag {
    values: HashMap<String, String>,
    secret_names: HashSet<String>,
    secret_values: Vec<String>,
}

impl VarBag {
    fn insert_flat(&mut self, vars: &HashMap<String, String>) {
        for (key, value) in vars {
            self.values.insert(key.clone(), value.clone());
        }
    }

    /// 运行时变量保留原始 JSON 类型，渲染时再转文本
    fn insert_typed(&mut self, vars: &HashMap<String, Value>) {
        for (key, value) in vars {
            self.values.insert(key.clone(), value_to_text(value));
        }
    }

    fn insert_dataset(&mut self, row: &DatasetRow) {
        for (key, value) in &row.flat {
            self.values.insert(key.clone(), value.clone());
        }
        for (key, value) in &row.values {
            self.values
                .entry(key.clone())
                .or_insert_with(|| value.to_string());
        }
    }

    fn insert_json(&mut self, raw: Option<&str>) {
        let Some(text) = raw.map(str::trim).filter(|text| !text.is_empty()) else {
            return;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
            return;
        };
        if let serde_json::Value::Object(map) = &value {
            for (key, child) in map {
                util::flatten_json(key, child, &mut self.values);
            }
        }
    }

    fn mark_secret(&mut self, name: &str) {
        self.secret_names.insert(name.to_string());
        if let Some(value) = self.values.get(name) {
            if !value.is_empty() {
                self.secret_values.push(value.clone());
            }
        }
    }

    fn render(&mut self, text: &str) -> String {
        util::render_tracked(text, &self.values, &mut self.secret_values)
    }

    fn mask(&self, text: &str) -> String {
        util::mask_values(text, &self.secret_values)
    }
}

/// 单个用例的执行结果（落库前的内存形态）
struct CaseOutcome {
    status: &'static str,
    attempts: i64,
    duration_ms: i64,
    http_duration_ms: i64,
    python_duration_ms: i64,
    request_snapshot: Option<String>,
    response_snapshot: Option<String>,
    assertions: Vec<AssertionOutcome>,
    error: Option<String>,
    logs: Vec<String>,
    python_version: Option<String>,
    script_hash: Option<String>,
    attachments: Vec<PythonAttachment>,
    /// kind=stress 时关联的压测报告
    report_id: Option<i64>,
    /// Python 结果里的 failFast：命中后中止后续用例
    stop_after: bool,
}

impl CaseOutcome {
    fn failed(status: &'static str, message: String) -> Self {
        Self {
            status,
            attempts: 0,
            duration_ms: 0,
            http_duration_ms: 0,
            python_duration_ms: 0,
            request_snapshot: None,
            response_snapshot: None,
            assertions: Vec::new(),
            error: Some(message),
            logs: Vec::new(),
            python_version: None,
            script_hash: None,
            attachments: Vec::new(),
            report_id: None,
            stop_after: false,
        }
    }
}

fn cap_text(text: &str, limit: usize) -> String {
    if text.len() <= limit {
        return text.to_string();
    }
    let mut end = limit;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n…（已截断，原始 {} 字节）", &text[..end], text.len())
}

// ---------------------------------------------------------------------------
// Python 钩子（T-11）
// ---------------------------------------------------------------------------

/// 本次运行可用的 Python 环境；未授权或不可用时只影响用到脚本的用例
#[derive(Clone)]
struct PythonEnv {
    info: Option<PythonInfo>,
    error: Option<String>,
}

struct PythonContext<'a> {
    env: &'a PythonEnv,
    run_id: i64,
    suite_id: i64,
    environment_id: Option<i64>,
    environment_name: Option<&'a str>,
    environment_vars: &'a HashMap<String, String>,
    cancel: &'a Arc<AtomicBool>,
    data_dir: &'a Path,
}

struct PythonCall<'a> {
    script: &'a str,
    phase: &'a str,
    case_id: i64,
    attempt: i64,
    work_dir: PathBuf,
    variables: HashMap<String, Value>,
    dataset: Option<Value>,
    request: Option<Value>,
    response: Option<Value>,
    assertions: Vec<Value>,
    timeout: Duration,
    business_env: HashMap<String, String>,
    attachment_budget: u64,
}

fn resolve_python(state: &AppState, suite: &crate::models::TestSuite) -> PythonEnv {
    let settings = state.settings();
    if !settings.python_enabled {
        return PythonEnv {
            info: None,
            error: Some(
                "PYTHON_NOT_AUTHORIZED：请在「设置 → 网络与安全」打开 Python 脚本开关，允许执行本地 Python 脚本后再运行"
                    .to_string(),
            ),
        };
    }
    // 套件可绑定独立解释器（例如某个项目专用的 venv），优先于全局设置
    let configured = suite
        .python_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(settings.python_path.trim());
    let check = python_exec::check(configured, true);
    match check.info {
        Some(info) => PythonEnv {
            info: Some(info),
            error: None,
        },
        None => PythonEnv {
            info: None,
            error: Some(format!("PYTHON_UNAVAILABLE：{}", check.message)),
        },
    }
}

async fn run_python(
    context: &PythonContext<'_>,
    call: PythonCall<'_>,
) -> Result<PythonOutcome, String> {
    let info = context.env.info.as_ref().ok_or_else(|| {
        context
            .env
            .error
            .clone()
            .unwrap_or_else(|| "PYTHON_UNAVAILABLE：Python 不可用".to_string())
    })?;
    std::fs::create_dir_all(&call.work_dir)
        .map_err(|error| format!("PYTHON_WORKDIR：创建工作目录失败：{error}"))?;
    let spec = PythonSpec {
        python: info.clone(),
        script: call.script.to_string(),
        work_dir: call.work_dir,
        run_id: context.run_id,
        suite_id: context.suite_id,
        case_id: call.case_id,
        phase: call.phase.to_string(),
        attempt: call.attempt,
        environment_id: context.environment_id,
        environment_name: context.environment_name.map(str::to_string),
        environment_vars: context.environment_vars.clone(),
        variables: call.variables,
        dataset: call.dataset,
        request: call.request,
        response: call.response,
        assertions: call.assertions,
        timeout: call.timeout,
        business_env: call.business_env,
        cancel: Some(context.cancel.clone()),
        attachment_budget: call.attachment_budget,
    };
    tokio::task::spawn_blocking(move || python_exec::run(spec))
        .await
        .map_err(|error| format!("PYTHON_TASK：执行任务异常：{error}"))
}

fn python_unavailable(message: String) -> PythonOutcome {
    PythonOutcome {
        error: Some(message),
        ..PythonOutcome::default()
    }
}

fn python_failure_status(outcome: &PythonOutcome) -> &'static str {
    if let Some(result) = &outcome.result {
        if result.ok == Some(false) {
            return "failed";
        }
    }
    "error"
}

fn python_case_status(
    outcome: &PythonOutcome,
    assertions: &[AssertionOutcome],
    skip: bool,
) -> &'static str {
    if skip {
        return "skipped";
    }
    if !outcome.ok {
        return python_failure_status(outcome);
    }
    if assertions.iter().any(|item| !item.passed) {
        "failed"
    } else {
        "passed"
    }
}

fn python_assertions(outcome: &PythonOutcome) -> Vec<AssertionOutcome> {
    outcome
        .result
        .as_ref()
        .map(|result| {
            result
                .assertions
                .iter()
                .map(|item| AssertionOutcome {
                    name: if item.name.trim().is_empty() {
                        "Python 断言".to_string()
                    } else {
                        item.name.clone()
                    },
                    passed: item.passed,
                    message: item.message.clone(),
                })
                .collect()
        })
        .unwrap_or_default()
}

fn python_logs(phase: &str, attempt: i64, outcome: &PythonOutcome) -> Vec<String> {
    let tag = if attempt > 1 {
        format!("{phase}#{attempt}")
    } else {
        phase.to_string()
    };
    let mut logs = Vec::new();
    if let Some(result) = &outcome.result {
        for item in &result.logs {
            let level = if item.level.eq_ignore_ascii_case("info") || item.level.is_empty() {
                String::new()
            } else {
                format!("[{}]", item.level)
            };
            logs.push(format!("[{tag}] {level}{}", item.message));
        }
    }
    for line in outcome
        .stdout
        .lines()
        .filter(|line| !line.trim().is_empty())
    {
        logs.push(format!("[{tag}][stdout] {line}"));
    }
    for line in outcome
        .stderr
        .lines()
        .filter(|line| !line.trim().is_empty())
    {
        logs.push(format!("[{tag}][stderr] {line}"));
    }
    if outcome.output_truncated {
        logs.push(format!("[{tag}] stdout/stderr 超过 1MB，已截断"));
    }
    if let Some(result) = &outcome.result {
        for item in &result.attachments {
            logs.push(format!(
                "[{tag}] 附件 {}（{} 字节）",
                item.name,
                item.size.unwrap_or(0)
            ));
        }
    }
    logs
}

/// 把 Python 结果合并进变量、断言、日志与附件；返回 (skip, failFast)
#[allow(clippy::too_many_arguments)]
fn absorb_python(
    outcome: &PythonOutcome,
    phase: &str,
    attempt: i64,
    bag: &mut VarBag,
    runtime_vars: &mut HashMap<String, Value>,
    python_duration: &mut i64,
    logs: &mut Vec<String>,
    assertions: &mut Vec<AssertionOutcome>,
    attachments: &mut Vec<PythonAttachment>,
    python_version: &mut Option<String>,
    script_hash: &mut Option<String>,
) -> (bool, bool) {
    *python_duration += outcome.duration_ms;
    logs.extend(python_logs(phase, attempt, outcome));
    if !outcome.python_version.trim().is_empty() {
        *python_version = Some(outcome.python_version.clone());
    }
    if !outcome.script_hash.trim().is_empty() {
        *script_hash = Some(outcome.script_hash.clone());
    }
    let mut skip = false;
    let mut fail_fast = false;
    if let Some(result) = &outcome.result {
        for (key, value) in &result.variables {
            bag.values.insert(key.clone(), value_to_text(value));
            runtime_vars.insert(key.clone(), value.clone());
        }
        assertions.extend(python_assertions(outcome));
        attachments.extend(result.attachments.iter().cloned());
        skip = result.skip;
        fail_fast = result.fail_fast;
    }
    (skip, fail_fast)
}

fn value_to_text(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

fn native_variables(
    bag: &VarBag,
    row: Option<&DatasetRow>,
    runtime_vars: &HashMap<String, Value>,
) -> HashMap<String, Value> {
    let mut out: HashMap<String, Value> = bag
        .values
        .iter()
        .map(|(key, value)| (key.clone(), Value::String(value.clone())))
        .collect();
    if let Some(row) = row {
        // 数据集保留原始类型，覆盖摊平后的字符串值
        for (key, value) in &row.values {
            out.insert(key.clone(), value.clone());
        }
    }
    // Python 运行时变量优先级最高，且保留原始类型
    for (key, value) in runtime_vars {
        out.insert(key.clone(), value.clone());
    }
    out
}

fn dataset_context(row: Option<&DatasetRow>) -> Option<Value> {
    row.map(|row| {
        let columns: Vec<&String> = row.values.keys().collect();
        json!({
            "rowIndex": row.index,
            "columns": columns,
            "values": row.values,
        })
    })
}

fn items_to_object(items: &[KeyValueItem]) -> Value {
    let mut map = serde_json::Map::new();
    for item in items.iter().filter(|item| item.enabled) {
        map.insert(item.key.clone(), Value::String(item.value.clone()));
    }
    Value::Object(map)
}

fn request_context(request: &HttpRequestDetail) -> Value {
    json!({
        "method": request.method,
        "url": request.url,
        "headers": items_to_object(&request.headers),
        "params": items_to_object(&request.params),
        "bodyType": request.body_type,
        "body": request.body,
    })
}

fn apply_request_override(request: &mut HttpRequestDetail, over: &PythonRequestOverride) {
    if let Some(method) = over
        .method
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        request.method = method.to_ascii_uppercase();
    }
    if let Some(url) = over
        .url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        request.url = url.to_string();
    }
    if let Some(headers) = &over.headers {
        if let Some(items) = value_to_items(headers) {
            request.headers = items;
        }
    }
    if let Some(params) = &over.params {
        if let Some(items) = value_to_items(params) {
            request.params = items;
        }
    }
    if let Some(body_type) = over
        .body_type
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        request.body_type = body_type.to_string();
    }
    if let Some(body) = &over.body {
        request.body = value_to_text(body);
    }
}

fn value_to_items(value: &Value) -> Option<Vec<KeyValueItem>> {
    match value {
        Value::Object(map) => Some(
            map.iter()
                .map(|(key, value)| KeyValueItem {
                    key: key.clone(),
                    value: value_to_text(value),
                    enabled: true,
                })
                .collect(),
        ),
        Value::Array(items) => {
            let mut out = Vec::new();
            for item in items {
                match item {
                    Value::Object(map) => {
                        let key = map
                            .get("key")
                            .or_else(|| map.get("name"))
                            .map(value_to_text)
                            .unwrap_or_default();
                        if key.is_empty() {
                            continue;
                        }
                        let value = map.get("value").map(value_to_text).unwrap_or_default();
                        let enabled = map.get("enabled").and_then(Value::as_bool).unwrap_or(true);
                        out.push(KeyValueItem {
                            key,
                            value,
                            enabled,
                        });
                    }
                    other => {
                        let text = value_to_text(other);
                        if !text.is_empty() {
                            out.push(KeyValueItem {
                                key: text,
                                value: String::new(),
                                enabled: true,
                            });
                        }
                    }
                }
            }
            Some(out)
        }
        _ => None,
    }
}

fn case_work_dir(data_dir: &Path, run_id: i64, case_id: i64, row: Option<&DatasetRow>) -> PathBuf {
    let base = data_dir.join("test-runs").join(run_id.to_string());
    match row {
        Some(row) => base.join(format!("{case_id}-r{}", row.index + 1)),
        None => base.join(case_id.to_string()),
    }
}

fn suite_work_dir(data_dir: &Path, run_id: i64, phase: &str) -> PathBuf {
    data_dir
        .join("test-runs")
        .join(run_id.to_string())
        .join(phase)
}

/// 先落一条 queued 记录并返回 runId，便于 UI / CLI 立刻拿到句柄
pub fn create_queued_run(state: &AppState, suite_id: i64, trigger: &str) -> Result<i64, String> {
    let conn = state.pool.get().map_err(err)?;
    let running: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM test_runs WHERE suite_id = ?1 AND status IN ('queued', 'running')",
            params![suite_id],
            |row| row.get(0),
        )
        .map_err(err)?;
    if running > 0 {
        return Err("该套件已有正在运行的记录，请先停止或等待结束".to_string());
    }
    let now = now_ms();
    conn.execute(
        "INSERT INTO test_runs (suite_id, trigger, status, total, app_version, created_at)
         VALUES (?1, ?2, 'queued', 0, ?3, ?4)",
        params![suite_id, trigger, env!("CARGO_PKG_VERSION"), now],
    )
    .map_err(err)?;
    Ok(conn.last_insert_rowid())
}

/// 运行套件；返回落库后的运行摘要
pub async fn run_suite(
    state: &AppState,
    suite_id: i64,
    run_id: i64,
    options: RunOptions,
    emit: &EventSink<'_>,
) -> Result<RunSummary, String> {
    let (suite, suite_config) = {
        let conn = state.pool.get().map_err(err)?;
        let suite = find_suite(&conn, suite_id)?;
        let config = SuiteConfig::parse(suite.config_json.as_deref())?;
        (suite, config)
    };

    let env_id = options.env_id.or(suite.env_id);
    let (env_vars, env_secret_names, env_name) = match env_id {
        Some(id) => {
            let conn = state.pool.get().map_err(err)?;
            let (vars, secrets) = load_vars_with_secrets(&conn, id)?;
            let name: Option<String> = conn
                .query_row(
                    "SELECT name FROM environments WHERE id = ?1",
                    params![id],
                    |row| row.get(0),
                )
                .optional()
                .map_err(err)?;
            (vars, secrets, name)
        }
        None => (HashMap::new(), HashSet::new(), None),
    };

    // 用例、数据集与总步数（数据集行数决定 total）
    let (cases, datasets) = {
        let conn = state.pool.get().map_err(err)?;
        let cases: Vec<TestCase> = load_cases(&conn, suite_id)?
            .into_iter()
            .filter(|case| case.enabled)
            .filter(|case| tag_matches(case, &options.tags, &options.exclude_tags))
            .collect();
        let datasets = load_datasets(&conn, suite_id, true)?;
        (cases, datasets)
    };
    if cases.is_empty() {
        return Err("套件没有启用的用例（或全部被标签过滤）".to_string());
    }
    let mut dataset_map = prepare_datasets(&datasets, &state.data_dir)?;
    // CLI `--data`：用外部文件替换套件数据集（仅覆盖被用例引用的数据集）
    if let Some(path) = options.dataset_override.as_deref() {
        let table = test_dataset::parse_file(path)?;
        let mut applied = false;
        for case in &cases {
            if let Some(id) = case.dataset_id {
                dataset_map.insert(id, table.clone());
                applied = true;
            }
        }
        if !applied {
            return Err("套件用例没有引用数据集，无法用 --data 覆盖".to_string());
        }
    }
    // Cookie 隔离：以运行开始时的本机 Cookie 快照作为内存罐初始值，
    // 仅 isolated 用例读写它，运行结束随罐丢弃，不落库
    let cookie_jar = {
        let conn = state.pool.get().map_err(err)?;
        Arc::new(CookieJar::snapshot(&conn))
    };
    let total: i64 = cases
        .iter()
        .map(|case| dataset_rows(case, &dataset_map).len() as i64)
        .sum();

    let fail_fast = options
        .fail_fast
        .unwrap_or_else(|| suite_config.fail_fast_effective());
    let run_started_at = now_ms();
    let cancel = {
        let conn = state.pool.get().map_err(err)?;
        let changed = conn
            .execute(
                "UPDATE test_runs
                 SET env_id = ?1, env_name = ?2, trigger = ?3, status = 'running', total = ?4,
                     config_json = ?5, app_version = ?6, started_at = ?7
                 WHERE id = ?8 AND status IN ('queued', 'running')",
                params![
                    env_id,
                    env_name,
                    options.trigger,
                    total,
                    suite.config_json,
                    env!("CARGO_PKG_VERSION"),
                    run_started_at,
                    run_id
                ],
            )
            .map_err(err)?;
        if changed == 0 {
            return Err("运行记录不存在或已结束".to_string());
        }
        state.test_register(run_id)
    };

    emit(TestEvent {
        name: "test:run:start",
        payload: serde_json::json!({
            "runId": run_id,
            "suiteId": suite_id,
            "total": total,
            "startedAt": run_started_at,
        }),
    });

    let python_env = resolve_python(state, &suite);
    let python_context = PythonContext {
        env: &python_env,
        run_id,
        suite_id,
        environment_id: env_id,
        environment_name: env_name.as_deref(),
        environment_vars: &env_vars,
        cancel: &cancel,
        data_dir: &state.data_dir,
    };
    let secret_values =
        collect_secret_values(&suite_config, &env_vars, &env_secret_names, &options);
    let suite_timeout = options
        .timeout_sec
        .map(Duration::from_secs)
        .or_else(|| suite_config.suite_timeout());
    let run_clock = Instant::now();
    let mut counts = Counts::default();
    let mut executed: HashMap<i64, &'static str> = HashMap::new();
    let mut runtime_vars: HashMap<String, Value> = HashMap::new();
    let mut run_python_version: Option<String> = None;
    let mut setup_failure: Option<String> = None;
    let mut teardown_failure: Option<String> = None;
    let mut aborted_by_fail_fast: Option<String> = None;
    let mut last_progress = Instant::now();
    let mut index: i64 = 0;

    // 套件 setup：失败后所有用例记 skipped，teardown 仍会执行
    if let Some(script) = suite
        .setup_script
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
    {
        let mut bag = VarBag::default();
        bag.insert_flat(&env_vars);
        bag.insert_flat(&suite_config.vars);
        bag.insert_flat(&options.vars);
        mark_run_secrets(&mut bag, &suite_config, &env_secret_names, &options);

        let call = PythonCall {
            script,
            phase: "setup",
            case_id: 0,
            attempt: 1,
            work_dir: suite_work_dir(&state.data_dir, run_id, "setup"),
            variables: native_variables(&bag, None, &runtime_vars),
            dataset: None,
            request: None,
            response: None,
            assertions: Vec::new(),
            timeout: suite_config.python_hook_timeout(),
            business_env: bag.values.clone(),
            attachment_budget: python_exec::MAX_ATTACHMENTS_TOTAL,
        };
        let outcome = match run_python(&python_context, call).await {
            Ok(outcome) => outcome,
            Err(message) => python_unavailable(message),
        };
        let mut logs = Vec::new();
        let mut assertions = Vec::new();
        let mut attachments = Vec::new();
        let mut python_duration = 0i64;
        let mut python_version = None;
        let mut script_hash = None;
        let (skip, _) = absorb_python(
            &outcome,
            "setup",
            1,
            &mut bag,
            &mut runtime_vars,
            &mut python_duration,
            &mut logs,
            &mut assertions,
            &mut attachments,
            &mut python_version,
            &mut script_hash,
        );
        let status = python_case_status(&outcome, &assertions, skip);
        let error = if status == "passed" {
            None
        } else {
            Some(
                bag.mask(
                    &python_error_message(&outcome, &assertions)
                        .unwrap_or_else(|| "套件 setup 未通过".to_string()),
                ),
            )
        };
        let case_outcome = CaseOutcome {
            status,
            attempts: 1,
            duration_ms: outcome.duration_ms,
            http_duration_ms: 0,
            python_duration_ms: python_duration,
            request_snapshot: None,
            response_snapshot: None,
            assertions,
            error: error.clone(),
            logs,
            python_version: python_version.clone(),
            script_hash,
            attachments,
            report_id: None,
            stop_after: false,
        };
        let _ = persist_suite_script(state, run_id, "[setup] 套件初始化", &case_outcome, &bag);
        if let Some(version) = python_version {
            run_python_version = Some(version);
        }
        if status != "passed" {
            setup_failure = Some(format!(
                "SETUP_FAILED：{}",
                error.unwrap_or_else(|| "套件 setup 未通过".to_string())
            ));
        }
    }

    'cases: for case in &cases {
        let dependency_reason = dependency_block(case, &executed);
        for row in dataset_rows(case, &dataset_map) {
            index += 1;

            if let Some(reason) = &setup_failure {
                let outcome = CaseOutcome::failed("skipped", reason.clone());
                let case_id = persist_case(state, run_id, case, row, &outcome, &VarBag::default())?;
                emit_case_end(emit, run_id, case_id, case, &outcome);
                counts.add(outcome.status);
                continue;
            }

            if cancel.load(Ordering::SeqCst) {
                aborted_by_fail_fast = Some("CANCELLED".to_string());
                break 'cases;
            }
            if let Some(limit) = suite_timeout {
                if run_clock.elapsed() >= limit {
                    aborted_by_fail_fast = Some("SUITE_TIMEOUT".to_string());
                    break 'cases;
                }
            }

            if let Some(reason) = &dependency_reason {
                let outcome = CaseOutcome::failed("skipped", reason.clone());
                let case_id = persist_case(state, run_id, case, row, &outcome, &VarBag::default())?;
                emit_case_end(emit, run_id, case_id, case, &outcome);
                counts.add(outcome.status);
                continue;
            }

            let mut bag = VarBag::default();
            bag.insert_flat(&env_vars);
            bag.insert_flat(&suite_config.vars);
            bag.insert_flat(&options.vars);
            if let Some(row) = row {
                bag.insert_dataset(row);
            }
            bag.insert_json(case.variables_json.as_deref());
            // Python 运行时变量优先级最高，可覆盖静态变量与数据集
            bag.insert_typed(&runtime_vars);
            mark_run_secrets(&mut bag, &suite_config, &env_secret_names, &options);

            emit(TestEvent {
                name: "test:case:start",
                payload: serde_json::json!({
                    "runId": run_id,
                    "caseId": case.id,
                    "name": case.name,
                    "kind": case.kind,
                    "index": index,
                    "rowIndex": row.map(|item| item.index),
                }),
            });

            let isolated = case_cookie_isolated(case, &suite_config);
            let budget = case_timeout_budget_for(
                case,
                &suite_config,
                suite_timeout,
                run_clock,
                case.kind == "python",
            );
            let hook_budget = budget.min(suite_config.python_hook_timeout());
            let outcome = execute_case(
                state,
                case,
                &mut bag,
                env_id,
                &options.trigger,
                &cancel,
                budget,
                if isolated {
                    Some(cookie_jar.as_ref())
                } else {
                    None
                },
                &python_context,
                row,
                &mut runtime_vars,
                hook_budget,
            )
            .await;

            let run_case_pk = persist_case(state, run_id, case, row, &outcome, &bag)?;
            emit_case_end(emit, run_id, run_case_pk, case, &outcome);
            executed.insert(case.id, outcome.status);
            counts.add(outcome.status);
            if let Some(version) = outcome.python_version.clone() {
                run_python_version = Some(version);
            }

            if last_progress.elapsed() >= PROGRESS_INTERVAL {
                last_progress = Instant::now();
                emit_progress(emit, run_id, &counts, index, total);
            }

            if outcome.stop_after {
                aborted_by_fail_fast = Some("PYTHON_FAIL_FAST".to_string());
                break 'cases;
            }
            let stop = matches!(outcome.status, "failed" | "error")
                && fail_fast
                && !case.continue_on_failure;
            if stop {
                aborted_by_fail_fast = Some("FAIL_FAST".to_string());
                break 'cases;
            }
        }
    }

    // 剩余没跑到的用例按 skipped 记账，让 total 与实际一致
    let skipped_left = total - counts.total();
    if skipped_left > 0 {
        counts.skipped += skipped_left;
    }
    if matches!(aborted_by_fail_fast.as_deref(), Some("CANCELLED")) {
        counts.cancelled = true;
    }

    // 套件 teardown 无论成败都执行；用独立取消位，避免停止信号让它直接退出
    if let Some(script) = suite
        .teardown_script
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
    {
        let teardown_cancel = Arc::new(AtomicBool::new(false));
        let teardown_context = PythonContext {
            env: &python_env,
            run_id,
            suite_id,
            environment_id: env_id,
            environment_name: env_name.as_deref(),
            environment_vars: &env_vars,
            cancel: &teardown_cancel,
            data_dir: &state.data_dir,
        };
        let mut bag = VarBag::default();
        bag.insert_flat(&env_vars);
        bag.insert_flat(&suite_config.vars);
        bag.insert_flat(&options.vars);
        bag.insert_typed(&runtime_vars);
        mark_run_secrets(&mut bag, &suite_config, &env_secret_names, &options);

        let call = PythonCall {
            script,
            phase: "teardown",
            case_id: 0,
            attempt: 1,
            work_dir: suite_work_dir(&state.data_dir, run_id, "teardown"),
            variables: native_variables(&bag, None, &runtime_vars),
            dataset: None,
            request: None,
            response: None,
            assertions: Vec::new(),
            timeout: suite_config.python_hook_timeout(),
            business_env: bag.values.clone(),
            attachment_budget: python_exec::MAX_ATTACHMENTS_TOTAL,
        };
        let outcome = match run_python(&teardown_context, call).await {
            Ok(outcome) => outcome,
            Err(message) => python_unavailable(message),
        };
        let mut logs = Vec::new();
        let mut assertions = Vec::new();
        let mut attachments = Vec::new();
        let mut python_duration = 0i64;
        let mut python_version = None;
        let mut script_hash = None;
        let (skip, _) = absorb_python(
            &outcome,
            "teardown",
            1,
            &mut bag,
            &mut runtime_vars,
            &mut python_duration,
            &mut logs,
            &mut assertions,
            &mut attachments,
            &mut python_version,
            &mut script_hash,
        );
        let status = python_case_status(&outcome, &assertions, skip);
        let error = if status == "passed" {
            None
        } else {
            Some(
                bag.mask(
                    &python_error_message(&outcome, &assertions)
                        .unwrap_or_else(|| "套件 teardown 未通过".to_string()),
                ),
            )
        };
        let case_outcome = CaseOutcome {
            status,
            attempts: 1,
            duration_ms: outcome.duration_ms,
            http_duration_ms: 0,
            python_duration_ms: python_duration,
            request_snapshot: None,
            response_snapshot: None,
            assertions,
            error: error.clone(),
            logs,
            python_version: python_version.clone(),
            script_hash,
            attachments,
            report_id: None,
            stop_after: false,
        };
        let _ = persist_suite_script(state, run_id, "[teardown] 套件清理", &case_outcome, &bag);
        if let Some(version) = python_version {
            run_python_version = Some(version);
        }
        if status != "passed" {
            teardown_failure = Some(format!(
                "TEARDOWN_FAILED：{}",
                error.unwrap_or_else(|| "套件 teardown 未通过".to_string())
            ));
        }
    }

    let status = if counts.cancelled {
        "cancelled"
    } else if setup_failure.is_some() {
        "error"
    } else if (teardown_failure.is_some() && suite_config.teardown_failure_fatal)
        || counts.failed > 0
        || counts.errored > 0
    {
        // 拆解失败（致命）与用例失败对套件来说都是 failed，只是原因不同
        "failed"
    } else {
        "passed"
    };
    let aborted_by = if setup_failure.is_some() && aborted_by_fail_fast.is_none() {
        Some("SETUP_FAILED".to_string())
    } else {
        aborted_by_fail_fast.clone()
    };
    let duration = run_clock.elapsed().as_millis() as i64;
    let finished_at = now_ms();
    let summary_json = serde_json::json!({
        "abortedBy": aborted_by,
        "failFast": fail_fast,
        "cookieMode": suite_config.cookie_mode.clone().unwrap_or_else(|| "shared".into()),
        "setupFailed": setup_failure.is_some(),
        "setupError": setup_failure.as_deref().map(|text| util::mask_values(text, &secret_values)),
        "teardownFailure": teardown_failure.as_deref().map(|text| util::mask_values(text, &secret_values)),
        "teardownFailureFatal": suite_config.teardown_failure_fatal,
        "pythonVersion": run_python_version,
        // 审计：运行使用的解释器路径（文档 4.6）
        "pythonPath": python_env.info.as_ref().map(|info| info.display.clone()),
        "pythonError": python_env.error.as_deref().map(|text| util::mask_values(text, &secret_values)),
    })
    .to_string();

    {
        let conn = state.pool.get().map_err(err)?;
        conn.execute(
            "UPDATE test_runs
             SET status = ?1, passed = ?2, failed = ?3, skipped = ?4, errored = ?5,
                 duration_ms = ?6, finished_at = ?7, summary_json = ?8, python_version = ?9
             WHERE id = ?10",
            params![
                status,
                counts.passed,
                counts.failed,
                counts.skipped,
                counts.errored,
                duration,
                finished_at,
                summary_json,
                run_python_version,
                run_id
            ],
        )
        .map_err(err)?;
    }
    state.test_release(run_id);

    emit(TestEvent {
        name: "test:run:done",
        payload: serde_json::json!({
            "runId": run_id,
            "status": status,
            "durationMs": duration,
            "passed": counts.passed,
            "failed": counts.failed,
            "skipped": counts.skipped,
            "errored": counts.errored,
            "total": total,
        }),
    });

    Ok(RunSummary {
        id: run_id,
        suite_id,
        suite_name: Some(suite.name),
        env_name,
        trigger: options.trigger,
        status: status.to_string(),
        total,
        passed: counts.passed,
        failed: counts.failed,
        skipped: counts.skipped,
        errored: counts.errored,
        duration_ms: duration,
        started_at: Some(run_started_at),
        finished_at: Some(finished_at),
        created_at: run_started_at,
    })
}

#[derive(Default)]
struct Counts {
    passed: i64,
    failed: i64,
    skipped: i64,
    errored: i64,
    cancelled: bool,
}

impl Counts {
    fn total(&self) -> i64 {
        self.passed + self.failed + self.skipped + self.errored
    }

    fn add(&mut self, status: &str) {
        match status {
            "passed" => self.passed += 1,
            "failed" => self.failed += 1,
            "skipped" => self.skipped += 1,
            _ => self.errored += 1,
        }
    }
}

fn emit_case_end(
    emit: &EventSink<'_>,
    run_id: i64,
    run_case_id: i64,
    case: &TestCase,
    outcome: &CaseOutcome,
) {
    emit(TestEvent {
        name: "test:case:end",
        payload: serde_json::json!({
            "runId": run_id,
            "runCaseId": run_case_id,
            "caseId": case.id,
            "name": case.name,
            "kind": case.kind,
            "status": outcome.status,
            "durationMs": outcome.duration_ms,
            "assertionsTotal": outcome.assertions.len(),
            "assertionsFailed": outcome.assertions.iter().filter(|a| !a.passed).count(),
            "error": outcome.error,
            "reportId": outcome.report_id,
        }),
    });
}

fn emit_progress(emit: &EventSink<'_>, run_id: i64, counts: &Counts, current: i64, total: i64) {
    emit(TestEvent {
        name: "test:run:progress",
        payload: serde_json::json!({
            "runId": run_id,
            "passed": counts.passed,
            "failed": counts.failed,
            "skipped": counts.skipped,
            "errored": counts.errored,
            "current": current,
            "total": total,
        }),
    });
}

fn dependency_block(case: &TestCase, executed: &HashMap<i64, &'static str>) -> Option<String> {
    let raw = case.depends_on.as_deref()?.trim();
    if raw.is_empty() {
        return None;
    }
    let mut blocked = Vec::new();
    for token in raw.split(',') {
        let Ok(id) = token.trim().parse::<i64>() else {
            continue;
        };
        match executed.get(&id).copied() {
            Some("passed") => {}
            Some(status) => blocked.push(format!("#{id} {status}")),
            None => blocked.push(format!("#{id} 未执行")),
        }
    }
    if blocked.is_empty() {
        None
    } else {
        Some(format!("DEPENDENCY_FAILED：{}", blocked.join("，")))
    }
}

/// 用例是否使用隔离 Cookie：套件配置 isolated，或用例标签 `cookie:isolated` 覆盖
fn case_cookie_isolated(case: &TestCase, config: &SuiteConfig) -> bool {
    if config.cookie_isolated() {
        return true;
    }
    case.tags
        .as_deref()
        .map(|tags| {
            tags.split(',')
                .any(|tag| tag.trim().eq_ignore_ascii_case("cookie:isolated"))
        })
        .unwrap_or(false)
}

/// 用例级超时预算（文档 6.8）：Python 用例默认 60s，HTTP 沿用 `timeout_ms`；
/// 套件剩余时间更小时取更小值。
fn case_timeout_budget_for(
    case: &TestCase,
    _config: &SuiteConfig,
    suite_timeout: Option<Duration>,
    run_clock: Instant,
    is_python: bool,
) -> Duration {
    let mut configured = case.timeout_ms.max(500);
    // 库里默认值是 30s（HTTP 口径）；Python 用例没单独调过时按 60s 执行
    if is_python && configured == 30_000 {
        configured = 60_000;
    }
    let mut budget = Duration::from_millis(configured.min(600_000) as u64);
    if let Some(limit) = suite_timeout {
        let remaining = limit.saturating_sub(run_clock.elapsed());
        if remaining < budget {
            budget = remaining.max(Duration::from_millis(500));
        }
    }
    budget
}

/// 名字像密钥的变量：只做打码，不阻止渲染（文档 6.2）
fn looks_secret(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    [
        "token",
        "secret",
        "password",
        "passwd",
        "apikey",
        "api_key",
        "credential",
        "authorization",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn mark_run_secrets(
    bag: &mut VarBag,
    config: &SuiteConfig,
    env_secret_names: &HashSet<String>,
    options: &RunOptions,
) {
    for name in &config.secrets {
        bag.mark_secret(name);
    }
    for name in env_secret_names {
        bag.mark_secret(name);
    }
    for name in options.secrets.keys() {
        bag.mark_secret(name);
    }
    let heuristic: Vec<String> = bag
        .values
        .keys()
        .filter(|name| looks_secret(name))
        .cloned()
        .collect();
    for name in heuristic {
        bag.mark_secret(&name);
    }
}

fn push_secret(values: &mut Vec<String>, value: &str) {
    let trimmed = value.trim();
    // 太短的值（如 "1"）做反向替换会把正常内容打烂
    if trimmed.len() >= 4 && !values.iter().any(|item| item == trimmed) {
        values.push(trimmed.to_string());
    }
}

/// 本次运行需要从落库文本中反向替换掉的密钥明文
fn collect_secret_values(
    config: &SuiteConfig,
    env_vars: &HashMap<String, String>,
    env_secret_names: &HashSet<String>,
    options: &RunOptions,
) -> Vec<String> {
    let mut values = Vec::new();
    for name in &config.secrets {
        if let Some(value) = env_vars.get(name) {
            push_secret(&mut values, value);
        }
    }
    for name in env_secret_names {
        if let Some(value) = env_vars.get(name) {
            push_secret(&mut values, value);
        }
    }
    for value in options.secrets.values() {
        push_secret(&mut values, value);
    }
    for (name, value) in env_vars {
        if looks_secret(name) {
            push_secret(&mut values, value);
        }
    }
    // 长值优先替换，避免短值先命中导致长密钥残留
    values.sort_by_key(|value| std::cmp::Reverse(value.len()));
    values
}

/// `tags` 既支持逗号分隔，也兼容 JSON 数组写法
fn case_tags(case: &TestCase) -> Vec<String> {
    let Some(raw) = case
        .tags
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
    else {
        return Vec::new();
    };
    if raw.starts_with('[') {
        if let Ok(Value::Array(items)) = serde_json::from_str::<Value>(raw) {
            return items
                .into_iter()
                .filter_map(|item| item.as_str().map(str::trim).map(str::to_string))
                .filter(|item| !item.is_empty())
                .collect();
        }
    }
    raw.split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_string)
        .collect()
}

/// 标签过滤：include 为空不限制；include 命中其一即保留；exclude 命中即剔除
fn tag_matches(case: &TestCase, include: &[String], exclude: &[String]) -> bool {
    if include.is_empty() && exclude.is_empty() {
        return true;
    }
    let tags = case_tags(case);
    if !exclude.is_empty() && tags.iter().any(|tag| exclude.contains(tag)) {
        return false;
    }
    if include.is_empty() {
        return true;
    }
    tags.iter().any(|tag| include.contains(tag))
}

fn dataset_rows<'a>(
    case: &TestCase,
    datasets: &'a HashMap<i64, DatasetTable>,
) -> Vec<Option<&'a DatasetRow>> {
    match case.dataset_id.and_then(|id| datasets.get(&id)) {
        Some(table) if !table.is_empty() => table.rows.iter().map(Some).collect(),
        _ => vec![None],
    }
}

type DatasetTable = test_dataset::DatasetTable;

fn prepare_datasets(
    datasets: &[TestDataset],
    data_dir: &std::path::Path,
) -> Result<HashMap<i64, DatasetTable>, String> {
    let mut map = HashMap::new();
    for dataset in datasets {
        let table = match dataset.content.as_deref() {
            Some(content) if !content.trim().is_empty() => {
                test_dataset::parse(&dataset.format, content)?
            }
            _ => match dataset
                .path
                .as_deref()
                .map(str::trim)
                .filter(|p| !p.is_empty())
            {
                Some(path) => {
                    let full = if std::path::Path::new(path).is_absolute() {
                        std::path::PathBuf::from(path)
                    } else {
                        data_dir.join(path)
                    };
                    test_dataset::parse_file(&full)?
                }
                None => continue,
            },
        };
        map.insert(dataset.id, table);
    }
    Ok(map)
}

#[allow(clippy::too_many_arguments)]
async fn execute_case(
    state: &AppState,
    case: &TestCase,
    bag: &mut VarBag,
    env_id: Option<i64>,
    trigger: &str,
    cancel: &Arc<AtomicBool>,
    budget: Duration,
    cookie_jar: Option<&CookieJar>,
    python: &PythonContext<'_>,
    row: Option<&DatasetRow>,
    runtime_vars: &mut HashMap<String, Value>,
    hook_budget: Duration,
) -> CaseOutcome {
    if cancel.load(Ordering::SeqCst) {
        return CaseOutcome::failed("skipped", "运行已停止".to_string());
    }
    match case.kind.as_str() {
        "request" => {
            execute_http_case(
                state,
                case,
                bag,
                env_id,
                trigger,
                budget,
                cookie_jar,
                python,
                row,
                runtime_vars,
                hook_budget,
            )
            .await
        }
        "python" => execute_python_case(case, bag, python, row, runtime_vars, hook_budget).await,
        "stress" => {
            execute_stress_case(state, case, bag, env_id, cancel, cookie_jar.is_some()).await
        }
        other => CaseOutcome::failed("error", format!("不支持的用例类型：{other}")),
    }
}

/// 基线报告的核心指标（SLA 回归对比用）
struct StressBaseline {
    id: i64,
    p95: f64,
    p99: f64,
    avg_qps: f64,
    error_rate: f64,
}

fn load_stress_baseline(state: &AppState, id: i64) -> Result<StressBaseline, String> {
    let conn = state.pool.get().map_err(err)?;
    conn.query_row(
        "SELECT id, p95_rt, p99_rt, avg_qps, error_rate FROM test_reports WHERE id = ?1",
        params![id],
        |row| {
            Ok(StressBaseline {
                id: row.get(0)?,
                p95: row.get(1)?,
                p99: row.get(2)?,
                avg_qps: row.get(3)?,
                error_rate: row.get(4)?,
            })
        },
    )
    .map_err(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => format!("基线报告 #{id} 不存在"),
        other => err(other),
    })
}

fn upper_bound_assertion(name: &str, actual: f64, limit: f64, unit: &str) -> AssertionOutcome {
    AssertionOutcome {
        name: format!("{name} ≤ {limit:.1}{unit}"),
        passed: actual <= limit,
        message: format!("实际 {actual:.1}{unit}"),
    }
}

/// SLA + 基线门禁：四项指标逐项判定，任一超标即用例 failed
fn stress_gate_assertions(
    report: &StressReport,
    targets: Option<&SlaTargets>,
    baseline: Option<&StressBaseline>,
) -> Vec<AssertionOutcome> {
    let mut out = vec![AssertionOutcome {
        name: "压测完成（请求数 > 0）".to_string(),
        passed: report.total_requests > 0,
        message: format!("共 {} 次请求", report.total_requests),
    }];
    if let Some(targets) = targets {
        if let Some(limit) = targets.p95_ms.filter(|value| *value > 0.0) {
            out.push(upper_bound_assertion(
                "SLA · P95",
                report.p95_rt,
                limit,
                "ms",
            ));
        }
        if let Some(limit) = targets.p99_ms.filter(|value| *value > 0.0) {
            out.push(upper_bound_assertion(
                "SLA · P99",
                report.p99_rt,
                limit,
                "ms",
            ));
        }
        if let Some(limit) = targets.error_rate.filter(|value| *value >= 0.0) {
            out.push(upper_bound_assertion(
                "SLA · 错误率",
                report.error_rate,
                limit,
                "%",
            ));
        }
        if let Some(limit) = targets.min_rps.filter(|value| *value > 0.0) {
            out.push(AssertionOutcome {
                name: format!("SLA · 平均 RPS ≥ {limit:.1}"),
                passed: report.avg_qps >= limit,
                message: format!("实际 {:.1}", report.avg_qps),
            });
        }
    }
    if let Some(base) = baseline {
        // 基线报告中对应指标为 0 时无法比较（例如没有成功请求），跳过避免误伤
        if base.p95 > 0.0 {
            out.push(upper_bound_assertion(
                &format!("基线 · P95（报告 #{} 的 1.2 倍）", base.id),
                report.p95_rt,
                base.p95 * 1.2,
                "ms",
            ));
        }
        if base.p99 > 0.0 {
            out.push(upper_bound_assertion(
                &format!("基线 · P99（报告 #{} 的 1.2 倍）", base.id),
                report.p99_rt,
                base.p99 * 1.2,
                "ms",
            ));
        }
        if base.avg_qps > 0.0 {
            let limit = base.avg_qps * 0.8;
            out.push(AssertionOutcome {
                name: format!("基线 · 平均 RPS ≥ {limit:.1}（报告 #{} 的 80%）", base.id),
                passed: report.avg_qps >= limit,
                message: format!("实际 {:.1}", report.avg_qps),
            });
        }
        let limit = base.error_rate + 0.5;
        out.push(upper_bound_assertion(
            &format!("基线 · 错误率（报告 #{} + 0.5%）", base.id),
            report.error_rate,
            limit,
            "%",
        ));
    }
    out
}

fn stress_error_summary(assertions: &[AssertionOutcome]) -> Option<String> {
    let failed: Vec<String> = assertions
        .iter()
        .filter(|item| !item.passed)
        .map(|item| format!("{}（{}）", item.name, item.message))
        .collect();
    if failed.is_empty() {
        None
    } else {
        Some(format!("压测门禁未通过：{}", failed.join("；")))
    }
}

/// `kind=stress` 用例：内联 StressConfig 快照，跑完复用压测报告并做 SLA / 基线判定
async fn execute_stress_case(
    state: &AppState,
    case: &TestCase,
    bag: &mut VarBag,
    env_id: Option<i64>,
    cancel: &Arc<AtomicBool>,
    cookie_isolated: bool,
) -> CaseOutcome {
    let raw = case.stress_json.as_deref().map(str::trim).unwrap_or("");
    if raw.is_empty() {
        return CaseOutcome::failed(
            "error",
            "STRESS_CONFIG_MISSING：压测用例缺少内联配置（stress_json）".to_string(),
        );
    }
    let value: Value = match serde_json::from_str(raw) {
        Ok(value) => value,
        Err(error) => {
            return CaseOutcome::failed("error", format!("压测配置格式错误：{error}"));
        }
    };
    let sla_override: Option<SlaTargets> = value
        .get("slaOverride")
        .and_then(|item| serde_json::from_value(item.clone()).ok());
    let baseline_id = value
        .get("baselineReportId")
        .and_then(Value::as_i64)
        .filter(|id| *id > 0);
    let wait_timeout = value
        .get("waitTimeoutSec")
        .and_then(Value::as_u64)
        .filter(|secs| *secs > 0)
        .map(|secs| secs.min(3600));
    let mut config: StressConfig = match serde_json::from_value(value) {
        Ok(config) => config,
        Err(error) => {
            return CaseOutcome::failed("error", format!("压测配置解析失败：{error}"));
        }
    };
    if let Some(targets) = sla_override {
        config.sla = Some(targets);
    }
    config.env_id = env_id;
    // 环境 / 数据集 / 运行期变量统一进入压测渲染；运行期变量优先
    {
        let mut vars = bag.values.clone();
        for (key, value) in &config.vars {
            if !key.trim().is_empty() {
                vars.insert(key.clone(), value.clone());
            }
        }
        config.vars = vars;
    }
    let mut logs = Vec::new();
    if cookie_isolated {
        config.cookie_enabled = Some(false);
        logs.push("Cookie 隔离模式：压测不读取本机 Cookie".to_string());
    }
    let gate_targets = config.sla.clone();
    let baseline = match baseline_id {
        Some(id) => match load_stress_baseline(state, id) {
            Ok(base) => Some(base),
            Err(error) => return CaseOutcome::failed("error", error),
        },
        None => None,
    };
    let request_snapshot = cap_text(
        &json!({
            "method": config.method,
            "url": bag.mask(&config.url),
            "concurrent": config.concurrent,
            "durationSec": config.duration_sec,
            "stages": config.stages,
            "sla": config.sla,
            "baselineReportId": baseline_id,
        })
        .to_string(),
        MAX_REQUEST_SNAPSHOT,
    );

    // 与手工压测互斥：等待前一个压测结束，最多等到 waitTimeoutSec
    if let Some(secs) = wait_timeout {
        let deadline = Instant::now() + Duration::from_secs(secs);
        while state.stress_running.load(Ordering::SeqCst) && Instant::now() < deadline {
            if cancel.load(Ordering::SeqCst) {
                return CaseOutcome::failed("skipped", "运行已停止".to_string());
            }
            tokio::time::sleep(Duration::from_millis(300)).await;
        }
    }
    if cancel.load(Ordering::SeqCst) {
        return CaseOutcome::failed("skipped", "运行已停止".to_string());
    }

    let started = Instant::now();
    let outcome = match crate::commands::stress::run_stress_case(state, config, Some(cancel)).await
    {
        Ok(outcome) => outcome,
        Err(error) => return CaseOutcome::failed("error", format!("STRESS_FAILED：{error}")),
    };
    let duration_ms = started.elapsed().as_millis() as i64;
    let report = &outcome.report;
    logs.push(format!(
        "压测{}：请求 {}，成功 {}，错误 {}，平均 RPS {:.1}，P95 {:.1}ms，P99 {:.1}ms，错误率 {:.2}%",
        if report.status == "stopped" { "已停止" } else { "完成" },
        report.total_requests,
        report.success_count,
        report.error_count,
        report.avg_qps,
        report.p95_rt,
        report.p99_rt,
        report.error_rate
    ));
    if let Some(id) = outcome.report_id {
        logs.push(format!("压测报告 #{id} 已保存"));
    }
    if let Some(message) = &outcome.message {
        logs.push(message.clone());
    }

    let assertions = stress_gate_assertions(report, gate_targets.as_ref(), baseline.as_ref());
    let stopped = cancel.load(Ordering::SeqCst) || report.status == "stopped";
    let gate_error = stress_error_summary(&assertions);
    let status = if stopped {
        "skipped"
    } else if gate_error.is_some() {
        "failed"
    } else {
        "passed"
    };
    let error = if stopped {
        Some("运行已停止，压测已中断".to_string())
    } else {
        gate_error
    };

    CaseOutcome {
        status,
        attempts: 1,
        duration_ms,
        http_duration_ms: 0,
        python_duration_ms: 0,
        request_snapshot: Some(request_snapshot),
        response_snapshot: None,
        assertions,
        error,
        logs,
        python_version: None,
        script_hash: None,
        attachments: Vec::new(),
        report_id: outcome.report_id,
        stop_after: false,
    }
}

fn python_error_message(
    outcome: &PythonOutcome,
    assertions: &[AssertionOutcome],
) -> Option<String> {
    if let Some(error) = outcome
        .error
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
    {
        return Some(error.to_string());
    }
    let failed: Vec<String> = assertions
        .iter()
        .filter(|item| !item.passed)
        .map(|item| format!("{}（{}）", item.name, item.message))
        .collect();
    if failed.is_empty() {
        None
    } else {
        Some(format!(
            "{} 条断言未通过：{}",
            failed.len(),
            failed.join("；")
        ))
    }
}

fn response_context(result: &HttpResult) -> Value {
    json!({
        "status": result.status,
        "statusText": result.status_text,
        "headers": result.headers,
        "body": result.body,
        "durationMs": result.duration_ms,
        "sizeBytes": result.size_bytes,
        "truncated": result.truncated,
        "error": result.error,
    })
}

fn snapshot_python_request(bag: &VarBag, over: &PythonRequestOverride) -> String {
    let value = json!({
        "method": over.method,
        "url": over.url.as_deref().map(|url| bag.mask(url)),
        "headers": over.headers,
        "params": over.params,
        "bodyType": over.body_type,
        "body": over.body.as_ref().map(value_to_text).map(|text| bag.mask(&text)),
    });
    cap_text(&value.to_string(), MAX_REQUEST_SNAPSHOT)
}

/// `kind=python` 的用例：只跑 main 脚本，不经过引擎的 HTTP 通道
async fn execute_python_case(
    case: &TestCase,
    bag: &mut VarBag,
    python: &PythonContext<'_>,
    row: Option<&DatasetRow>,
    runtime_vars: &mut HashMap<String, Value>,
    hook_budget: Duration,
) -> CaseOutcome {
    let script = match case
        .main_script
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
    {
        Some(script) => script,
        None => {
            return CaseOutcome::failed(
                "error",
                "PYTHON_NOT_CONFIGURED：Python 用例缺少 mainScript".to_string(),
            )
        }
    };
    let call = PythonCall {
        script,
        phase: "main",
        case_id: case.id,
        attempt: 1,
        work_dir: case_work_dir(python.data_dir, python.run_id, case.id, row),
        variables: native_variables(bag, row, runtime_vars),
        dataset: dataset_context(row),
        request: None,
        response: None,
        assertions: Vec::new(),
        timeout: hook_budget,
        business_env: bag.values.clone(),
        attachment_budget: python_exec::MAX_ATTACHMENTS_TOTAL,
    };
    let outcome = match run_python(python, call).await {
        Ok(outcome) => outcome,
        Err(message) => python_unavailable(message),
    };

    let mut logs = Vec::new();
    let mut assertions = Vec::new();
    let mut attachments = Vec::new();
    let mut python_duration = 0i64;
    let mut python_version = None;
    let mut script_hash = None;
    let (skip, fail_fast) = absorb_python(
        &outcome,
        "main",
        1,
        bag,
        runtime_vars,
        &mut python_duration,
        &mut logs,
        &mut assertions,
        &mut attachments,
        &mut python_version,
        &mut script_hash,
    );
    let status = python_case_status(&outcome, &assertions, skip);
    let error = if matches!(status, "passed" | "skipped") {
        None
    } else {
        python_error_message(&outcome, &assertions)
    };
    let request_snapshot = outcome
        .result
        .as_ref()
        .and_then(|result| result.request.as_ref())
        .map(|over| snapshot_python_request(bag, over));

    CaseOutcome {
        status,
        attempts: 1,
        duration_ms: outcome.duration_ms,
        http_duration_ms: 0,
        python_duration_ms: python_duration,
        request_snapshot,
        response_snapshot: None,
        assertions,
        error,
        logs,
        python_version,
        script_hash,
        attachments,
        report_id: None,
        stop_after: fail_fast,
    }
}

/// HTTP 用例：pre 钩子（可改请求）→ 发请求 → 声明式断言 → post 钩子
#[allow(clippy::too_many_arguments)]
async fn execute_http_case(
    state: &AppState,
    case: &TestCase,
    bag: &mut VarBag,
    env_id: Option<i64>,
    trigger: &str,
    budget: Duration,
    cookie_jar: Option<&CookieJar>,
    python: &PythonContext<'_>,
    row: Option<&DatasetRow>,
    runtime_vars: &mut HashMap<String, Value>,
    hook_budget: Duration,
) -> CaseOutcome {
    let request_id = match case.request_id {
        Some(id) => id,
        None => return CaseOutcome::failed("error", "用例未绑定接口".to_string()),
    };
    let base_request = {
        let conn = match state.pool.get() {
            Ok(conn) => conn,
            Err(error) => return CaseOutcome::failed("error", err(error)),
        };
        match find_request(&conn, request_id) {
            Ok(request) => request,
            Err(message) => return CaseOutcome::failed("error", message),
        }
    };

    let assertions = match test_assert::parse_specs(case.assertions_json.as_deref()) {
        Ok(specs) => specs,
        Err(message) => return CaseOutcome::failed("error", message),
    };
    let pre_script = case
        .pre_script
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty());
    let post_script = case
        .post_script
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty());

    let retries = case.retries.clamp(0, 3);
    let case_clock = Instant::now();
    let mut attempts = 0;
    let mut http_duration = 0i64;
    let mut python_duration = 0i64;
    let mut logs: Vec<String> = Vec::new();
    let mut all_assertions: Vec<AssertionOutcome> = Vec::new();
    let mut attachments: Vec<PythonAttachment> = Vec::new();
    let mut python_version = None;
    let mut script_hash = None;
    let mut stop_after = false;
    let mut status: &'static str = "error";
    let mut error: Option<String> = None;
    let mut request_snapshot: Option<String> = None;
    let mut response_snapshot: Option<String> = None;

    'attempts: for attempt in 1..=retries + 1 {
        attempts = attempt;
        let mut request = base_request.clone();

        if let Some(script) = pre_script {
            let call = PythonCall {
                script,
                phase: "pre",
                case_id: case.id,
                attempt,
                work_dir: case_work_dir(python.data_dir, python.run_id, case.id, row),
                variables: native_variables(bag, row, runtime_vars),
                dataset: dataset_context(row),
                request: Some(request_context(&request)),
                response: None,
                assertions: Vec::new(),
                timeout: hook_budget,
                business_env: bag.values.clone(),
                attachment_budget: python_exec::MAX_ATTACHMENTS_TOTAL,
            };
            let outcome = match run_python(python, call).await {
                Ok(outcome) => outcome,
                Err(message) => python_unavailable(message),
            };
            let mut pre_assertions = Vec::new();
            let (skip, fail_fast) = absorb_python(
                &outcome,
                "pre",
                attempt,
                bag,
                runtime_vars,
                &mut python_duration,
                &mut logs,
                &mut pre_assertions,
                &mut attachments,
                &mut python_version,
                &mut script_hash,
            );
            all_assertions.extend(pre_assertions.clone());
            stop_after |= fail_fast;
            if skip {
                status = "skipped";
                error = None;
                break 'attempts;
            }
            let pre_status = python_case_status(&outcome, &pre_assertions, false);
            if pre_status != "passed" {
                // 前置脚本失败一律记 error（文档 4.2），重试由 retryOnFailure 决定
                status = "error";
                error = Some(format!(
                    "PYTHON_PRE_FAILED：{}",
                    python_error_message(&outcome, &pre_assertions)
                        .unwrap_or_else(|| "前置脚本未通过".to_string())
                ));
                if !(case.retry_on_failure && retries > 0 && attempt <= retries) {
                    break 'attempts;
                }
                continue;
            }
            if let Some(result) = &outcome.result {
                if let Some(over) = &result.request {
                    apply_request_override(&mut request, over);
                    request_snapshot = Some(snapshot_python_request(bag, over));
                }
            }
        }

        let payload = match build_payload(&request, case, bag, env_id, budget) {
            Ok(payload) => payload,
            Err(message) => {
                status = "error";
                error = Some(message);
                break 'attempts;
            }
        };
        request_snapshot = Some(snapshot_request(&payload, bag));

        let started = Instant::now();
        let result = http::execute_with(
            state,
            payload,
            RecordOptions::suite(if trigger == "cli" { "cli" } else { "suite" }),
            cookie_jar,
        )
        .await;
        http_duration += started.elapsed().as_millis() as i64;

        let result = match result {
            Ok(result) => result,
            Err(message) => {
                status = "error";
                error = Some(message.clone());
                logs.push(format!("第 {attempt} 次请求失败：{message}"));
                let retryable = case.retry_on_failure && retries > 0 && attempt <= retries;
                if retryable {
                    continue;
                }
                break 'attempts;
            }
        };

        let assertion_input = AssertInput {
            status: result.status,
            duration_ms: result.duration_ms,
            headers: &result.headers,
            body: &result.body,
        };
        let outcomes = test_assert::evaluate(&assertions, &assertion_input);
        let failed = outcomes.iter().filter(|item| !item.passed).count();
        all_assertions.extend(outcomes.clone());

        response_snapshot = Some(cap_text(
            &bag.mask(&serde_json::to_string(&result).unwrap_or_default()),
            MAX_RESPONSE_SNAPSHOT,
        ));

        status = if let Some(error_text) = result.error.as_deref() {
            error = Some(error_text.to_string());
            "error"
        } else if failed > 0 {
            error = Some(format!(
                "{failed} 条断言未通过：{}",
                outcomes
                    .iter()
                    .filter(|item| !item.passed)
                    .map(|item| format!("{}（{}）", item.name, item.message))
                    .collect::<Vec<_>>()
                    .join("；")
            ));
            "failed"
        } else {
            error = None;
            "passed"
        };

        logs.push(format!(
            "{} {} -> {} {}ms",
            request.method.to_uppercase(),
            bag.mask(&request.url),
            result.status,
            result.duration_ms
        ));
        if result.truncated {
            logs.push("响应体超过 5MB，已截断".to_string());
        }
        if let Some(error_text) = result.error.as_deref() {
            logs.push(format!("请求错误：{error_text}"));
        }

        if let Some(script) = post_script {
            let before = all_assertions.len();
            let call = PythonCall {
                script,
                phase: "post",
                case_id: case.id,
                attempt,
                work_dir: case_work_dir(python.data_dir, python.run_id, case.id, row),
                variables: native_variables(bag, row, runtime_vars),
                dataset: dataset_context(row),
                request: Some(request_context(&request)),
                response: Some(response_context(&result)),
                assertions: outcomes
                    .iter()
                    .map(|item| {
                        json!({
                            "name": item.name,
                            "passed": item.passed,
                            "message": item.message,
                        })
                    })
                    .collect(),
                timeout: hook_budget,
                business_env: bag.values.clone(),
                attachment_budget: python_exec::MAX_ATTACHMENTS_TOTAL,
            };
            let outcome = match run_python(python, call).await {
                Ok(outcome) => outcome,
                Err(message) => python_unavailable(message),
            };
            let (skip, fail_fast) = absorb_python(
                &outcome,
                "post",
                attempt,
                bag,
                runtime_vars,
                &mut python_duration,
                &mut logs,
                &mut all_assertions,
                &mut attachments,
                &mut python_version,
                &mut script_hash,
            );
            stop_after |= fail_fast;
            let post_assertions = &all_assertions[before..];
            let post_status = python_case_status(&outcome, post_assertions, skip);
            if skip && status != "failed" {
                status = "skipped";
                error = None;
            } else if post_status != "passed" {
                // 后置脚本断言不通过 / ok=false → failed；进程级失败 → error
                status = post_status;
                error = python_error_message(&outcome, post_assertions);
            }
        }

        let retryable = status == "failed" || status == "error";
        let allow_retry = case.retry_on_failure && retries > 0 && attempt <= retries;
        if !retryable || !allow_retry {
            break 'attempts;
        }
    }

    CaseOutcome {
        status,
        attempts: attempts.max(1),
        duration_ms: case_clock.elapsed().as_millis() as i64,
        http_duration_ms: http_duration,
        python_duration_ms: python_duration,
        request_snapshot,
        response_snapshot,
        assertions: all_assertions,
        error,
        logs,
        python_version,
        script_hash,
        attachments,
        report_id: None,
        stop_after,
    }
}

fn build_payload(
    request: &HttpRequestDetail,
    case: &TestCase,
    bag: &mut VarBag,
    env_id: Option<i64>,
    budget: Duration,
) -> Result<SendRequestPayload, String> {
    let url = bag.render(&request.url);
    let headers = request
        .headers
        .iter()
        .filter(|item| item.enabled)
        .map(|item| KeyValueItem {
            key: bag.render(&item.key),
            value: bag.render(&item.value),
            enabled: true,
        })
        .collect();
    let params = request
        .params
        .iter()
        .filter(|item| item.enabled)
        .map(|item| KeyValueItem {
            key: bag.render(&item.key),
            value: bag.render(&item.value),
            enabled: true,
        })
        .collect();
    let body = request.body.as_str();
    let body = if body.is_empty() {
        None
    } else {
        Some(bag.render(body))
    };
    let auth = render_auth(&request.auth, bag);

    Ok(SendRequestPayload {
        request_id: Some(request.id.unwrap_or(0)).filter(|id| *id > 0),
        name: Some(case.name.clone()),
        method: request.method.clone(),
        url,
        headers,
        params,
        body_type: request.body_type.clone(),
        body,
        env_id,
        insecure: false,
        timeout_ms: budget.as_millis().max(500) as u64,
        auth: Some(auth),
        cookie_enabled: Some(true),
        origin: Some("suite".to_string()),
        request_key: None,
        protocol: None,
        follow_redirects: None,
        connect_timeout_ms: None,
        retry: None,
    })
}

fn render_auth(auth: &RequestAuth, bag: &mut VarBag) -> RequestAuth {
    RequestAuth {
        kind: auth.kind.clone(),
        token: bag.render(&auth.token),
        username: bag.render(&auth.username),
        password: bag.render(&auth.password),
        key: bag.render(&auth.key),
        value: bag.render(&auth.value),
        add_to: auth.add_to.clone(),
        token_url: bag.render(&auth.token_url),
        client_id: bag.render(&auth.client_id),
        client_secret: bag.render(&auth.client_secret),
        scope: bag.render(&auth.scope),
    }
}

fn snapshot_request(payload: &SendRequestPayload, bag: &VarBag) -> String {
    let snapshot = serde_json::json!({
        "method": payload.method,
        "url": bag.mask(&payload.url),
        "headers": payload.headers.iter().map(|item| {
            let name = item.key.to_ascii_lowercase();
            let value = if name == "authorization" || name == "cookie" {
                "******".to_string()
            } else {
                bag.mask(&item.value)
            };
            serde_json::json!({ "key": item.key, "value": value })
        }).collect::<Vec<_>>(),
        "params": payload.params.iter().map(|item| {
            serde_json::json!({ "key": item.key, "value": bag.mask(&item.value) })
        }).collect::<Vec<_>>(),
        "bodyType": payload.body_type,
        "body": payload.body.as_deref().map(|text| bag.mask(text)).unwrap_or_default(),
    });
    cap_text(&snapshot.to_string(), MAX_REQUEST_SNAPSHOT)
}

fn persist_case(
    state: &AppState,
    run_id: i64,
    case: &TestCase,
    row: Option<&DatasetRow>,
    outcome: &CaseOutcome,
    bag: &VarBag,
) -> Result<i64, String> {
    let conn = state.pool.get().map_err(err)?;
    let now = now_ms();
    let name = match row {
        Some(item) => format!("{} [第 {} 行]", case.name, item.index + 1),
        None => case.name.clone(),
    };
    let logs = cap_text(&bag.mask(&outcome.logs.join("\n")), MAX_LOGS);
    let error = outcome.error.as_deref().map(|text| bag.mask(text));
    conn.execute(
        "INSERT INTO test_run_cases
            (run_id, case_id, name, kind, status, attempts, duration_ms, http_duration_ms,
             python_duration_ms, request_snapshot, response_snapshot, assertions_total,
             assertions_failed, error, logs, python_version, script_hash, report_id,
             started_at, finished_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17,
                 ?18, ?19, ?19)",
        params![
            run_id,
            case.id,
            name,
            case.kind,
            outcome.status,
            outcome.attempts,
            outcome.duration_ms,
            outcome.http_duration_ms,
            outcome.python_duration_ms,
            outcome.request_snapshot,
            outcome.response_snapshot,
            outcome.assertions.len() as i64,
            outcome
                .assertions
                .iter()
                .filter(|item| !item.passed)
                .count() as i64,
            error,
            logs,
            outcome.python_version,
            outcome.script_hash,
            outcome.report_id,
            now
        ],
    )
    .map_err(err)?;
    let run_case_id = conn.last_insert_rowid();

    insert_assertions(&conn, run_case_id, &outcome.assertions, bag)?;
    let run_root = state.data_dir.join("test-runs").join(run_id.to_string());
    insert_artifacts(
        &conn,
        run_id,
        Some(run_case_id),
        &outcome.attachments,
        &run_root,
        now,
    )?;
    Ok(run_case_id)
}

fn insert_assertions(
    conn: &rusqlite::Connection,
    run_case_id: i64,
    assertions: &[AssertionOutcome],
    bag: &VarBag,
) -> Result<(), String> {
    for assertion in assertions {
        conn.execute(
            "INSERT INTO test_run_assertions (run_case_id, name, passed, message, duration_ms)
             VALUES (?1, ?2, ?3, ?4, 0)",
            params![
                run_case_id,
                bag.mask(&assertion.name),
                assertion.passed as i64,
                if assertion.passed {
                    None
                } else {
                    Some(bag.mask(&assertion.message))
                }
            ],
        )
        .map_err(err)?;
    }
    Ok(())
}

fn insert_artifacts(
    conn: &rusqlite::Connection,
    run_id: i64,
    run_case_id: Option<i64>,
    attachments: &[PythonAttachment],
    run_root: &Path,
    created_at: i64,
) -> Result<(), String> {
    for item in attachments {
        // 路径统一存相对本次运行目录（test-runs/<runId>），自定义数据目录搬家后仍可用
        let path = Path::new(&item.path)
            .strip_prefix(run_root)
            .map(|path| path.to_string_lossy().replace('\\', "/"))
            .unwrap_or_else(|_| item.path.clone());
        conn.execute(
            "INSERT INTO test_artifacts
                (run_id, run_case_id, kind, name, path, size_bytes, created_at)
             VALUES (?1, ?2, 'attachment', ?3, ?4, ?5, ?6)",
            params![
                run_id,
                run_case_id,
                item.name,
                path,
                item.size.unwrap_or(0) as i64,
                created_at
            ],
        )
        .map_err(err)?;
    }
    Ok(())
}

/// 套件 setup / teardown 的独立结果：`case_id` 为空，不计入用例总数
fn persist_suite_script(
    state: &AppState,
    run_id: i64,
    name: &str,
    outcome: &CaseOutcome,
    bag: &VarBag,
) -> Result<i64, String> {
    let conn = state.pool.get().map_err(err)?;
    let now = now_ms();
    let logs = cap_text(&bag.mask(&outcome.logs.join("\n")), MAX_LOGS);
    let error = outcome.error.as_deref().map(|text| bag.mask(text));
    conn.execute(
        "INSERT INTO test_run_cases
            (run_id, case_id, name, kind, status, attempts, duration_ms, http_duration_ms,
             python_duration_ms, assertions_total, assertions_failed, error, logs,
             python_version, script_hash, started_at, finished_at)
         VALUES (?1, NULL, ?2, 'python', ?3, ?4, ?5, 0, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?13)",
        params![
            run_id,
            name,
            outcome.status,
            outcome.attempts,
            outcome.duration_ms,
            outcome.python_duration_ms,
            outcome.assertions.len() as i64,
            outcome
                .assertions
                .iter()
                .filter(|item| !item.passed)
                .count() as i64,
            error,
            logs,
            outcome.python_version,
            outcome.script_hash,
            now
        ],
    )
    .map_err(err)?;
    let run_case_id = conn.last_insert_rowid();
    insert_assertions(&conn, run_case_id, &outcome.assertions, bag)?;
    let run_root = state.data_dir.join("test-runs").join(run_id.to_string());
    insert_artifacts(
        &conn,
        run_id,
        Some(run_case_id),
        &outcome.attachments,
        &run_root,
        now,
    )?;
    Ok(run_case_id)
}

/// 读取运行详情（命令与 CLI 共用）
pub fn load_run_detail(
    conn: &rusqlite::Connection,
    run_id: i64,
) -> Result<crate::models::RunDetail, String> {
    let run = conn
        .query_row(
            "SELECT id, suite_id, env_id, env_name, trigger, status, total, passed, failed,
                    skipped, errored, duration_ms, config_json, summary_json, app_version,
                    python_version, report_dir, started_at, finished_at, created_at
             FROM test_runs WHERE id = ?1",
            params![run_id],
            |row| {
                Ok(TestRun {
                    id: row.get(0)?,
                    suite_id: row.get(1)?,
                    env_id: row.get(2)?,
                    env_name: row.get(3)?,
                    trigger: row.get(4)?,
                    status: row.get(5)?,
                    total: row.get(6)?,
                    passed: row.get(7)?,
                    failed: row.get(8)?,
                    skipped: row.get(9)?,
                    errored: row.get(10)?,
                    duration_ms: row.get(11)?,
                    config_json: row.get(12)?,
                    summary_json: row.get(13)?,
                    app_version: row.get(14)?,
                    python_version: row.get(15)?,
                    report_dir: row.get(16)?,
                    started_at: row.get(17)?,
                    finished_at: row.get(18)?,
                    created_at: row.get(19)?,
                })
            },
        )
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => "运行记录不存在".to_string(),
            other => err(other),
        })?;

    let mut stmt = conn
        .prepare(
            "SELECT id, run_id, case_id, name, kind, status, attempts, duration_ms,
                    http_duration_ms, python_duration_ms, request_snapshot, response_snapshot,
                    assertions_total, assertions_failed, error, logs, python_version,
                    script_hash, report_id, started_at, finished_at
             FROM test_run_cases WHERE run_id = ?1 ORDER BY id ASC",
        )
        .map_err(err)?;
    let cases = stmt
        .query_map(params![run_id], |row| {
            Ok(crate::models::TestRunCase {
                id: row.get(0)?,
                run_id: row.get(1)?,
                case_id: row.get(2)?,
                name: row.get(3)?,
                kind: row.get(4)?,
                status: row.get(5)?,
                attempts: row.get(6)?,
                duration_ms: row.get(7)?,
                http_duration_ms: row.get(8)?,
                python_duration_ms: row.get(9)?,
                request_snapshot: row.get(10)?,
                response_snapshot: row.get(11)?,
                assertions_total: row.get(12)?,
                assertions_failed: row.get(13)?,
                error: row.get(14)?,
                logs: row.get(15)?,
                python_version: row.get(16)?,
                script_hash: row.get(17)?,
                report_id: row.get(18)?,
                started_at: row.get(19)?,
                finished_at: row.get(20)?,
            })
        })
        .map_err(err)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(err)?;

    let mut stmt = conn
        .prepare(
            "SELECT a.id, a.run_case_id, a.name, a.passed, a.message, a.duration_ms
             FROM test_run_assertions a
             JOIN test_run_cases c ON c.id = a.run_case_id
             WHERE c.run_id = ?1 ORDER BY a.id ASC",
        )
        .map_err(err)?;
    let assertions = stmt
        .query_map(params![run_id], |row| {
            Ok(crate::models::TestRunAssertion {
                id: row.get(0)?,
                run_case_id: row.get(1)?,
                name: row.get(2)?,
                passed: row.get::<_, i64>(3)? != 0,
                message: row.get(4)?,
                duration_ms: row.get(5)?,
            })
        })
        .map_err(err)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(err)?;

    Ok(crate::models::RunDetail {
        run,
        cases,
        assertions,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::path::PathBuf;

    /// 起一个只回固定响应的本地服务，避免测试依赖外网
    fn spawn_server(body: &'static str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut stream = stream;
                let mut buffer = [0u8; 4096];
                let _ = stream.read(&mut buffer);
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(response.as_bytes());
            }
        });
        format!("http://{addr}/api/orders")
    }

    fn temp_state(tag: &str) -> (PathBuf, AppState) {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let root = std::env::temp_dir().join(format!(
            "astroman-runner-{tag}-{}-{stamp}",
            std::process::id()
        ));
        let data = root.join("data");
        let config = root.join("config");
        let state = AppState::headless(data, config).unwrap();
        (root, state)
    }

    fn seed_suite(state: &AppState, url: &str, assertions: &str) -> i64 {
        let conn = state.pool.get().unwrap();
        let now = util::now_ms();
        conn.execute(
            "INSERT INTO projects (name, created_at, updated_at) VALUES ('p', ?1, ?1)",
            params![now],
        )
        .unwrap();
        let project_id = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO collections (project_id, name, created_at, updated_at)
             VALUES (?1, 'c', ?2, ?2)",
            params![project_id, now],
        )
        .unwrap();
        let collection_id = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO requests (collection_id, name, method, url, created_at, updated_at)
             VALUES (?1, 'r', 'GET', ?2, ?3, ?3)",
            params![collection_id, url, now],
        )
        .unwrap();
        let request_id = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO test_suites (project_id, name, config_json, created_at, updated_at)
             VALUES (?1, 's', '{\"failFast\":true}', ?2, ?2)",
            params![project_id, now],
        )
        .unwrap();
        let suite_id = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO test_cases
                (suite_id, name, kind, request_id, sort_order, assertions_json, created_at, updated_at)
             VALUES (?1, '下单', 'request', ?2, 1, ?3, ?4, ?4)",
            params![suite_id, request_id, assertions, now],
        )
        .unwrap();
        suite_id
    }

    #[test]
    fn runs_http_suite_and_persists_results() {
        let url = spawn_server(r#"{"code":0,"data":{"orderId":42}}"#);
        let (root, state) = temp_state("pass");
        let suite_id = seed_suite(
            &state,
            &url,
            r#"[{"type":"status","op":"in","value":[200]},
                {"type":"jsonPath","path":"$.data.orderId","op":"eq","value":42},
                {"type":"durationMs","value":5000}]"#,
        );

        let run_id = create_queued_run(&state, suite_id, "ui").unwrap();
        let summary = tauri::async_runtime::block_on(run_suite(
            &state,
            suite_id,
            run_id,
            RunOptions::default(),
            &|_| {},
        ))
        .unwrap();

        assert_eq!(summary.status, "passed", "{summary:?}");
        assert_eq!(summary.total, 1);
        assert_eq!(summary.passed, 1);

        let conn = state.pool.get().unwrap();
        let detail = load_run_detail(&conn, run_id).unwrap();
        assert_eq!(detail.cases.len(), 1);
        assert_eq!(detail.cases[0].status, "passed");
        assert_eq!(detail.cases[0].assertions_total, 3);
        assert_eq!(detail.cases[0].assertions_failed, 0);
        assert_eq!(detail.assertions.len(), 3);
        assert!(detail.assertions.iter().all(|item| item.passed));
        assert!(detail.cases[0]
            .response_snapshot
            .as_deref()
            .unwrap()
            .contains("orderId"));
        // 套件运行不写单接口历史与快照（T-33）
        let history: i64 = conn
            .query_row("SELECT COUNT(*) FROM request_history", [], |row| row.get(0))
            .unwrap();
        let runs: i64 = conn
            .query_row("SELECT COUNT(*) FROM request_runs", [], |row| row.get(0))
            .unwrap();
        assert_eq!(history, 0);
        assert_eq!(runs, 0);
        drop(conn);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 回显请求 Cookie 并下发 Set-Cookie 的服务
    fn spawn_cookie_server() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut stream = stream;
                let mut buffer = [0u8; 8192];
                let read = stream.read(&mut buffer).unwrap_or(0);
                let request = String::from_utf8_lossy(&buffer[..read]).into_owned();
                let cookie = request
                    .lines()
                    .find_map(|line| {
                        let lower = line.to_ascii_lowercase();
                        lower
                            .starts_with("cookie:")
                            .then(|| line["cookie:".len()..].trim().to_string())
                    })
                    .unwrap_or_default();
                let body = serde_json::json!({ "cookie": cookie }).to_string();
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nSet-Cookie: sid=abc; Path=/\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(response.as_bytes());
            }
        });
        format!("http://{addr}/api/orders")
    }

    #[test]
    fn isolated_cookie_mode_keeps_session_in_memory_only() {
        let url = spawn_cookie_server();
        let (root, state) = temp_state("cookie");
        let suite_id = seed_suite(
            &state,
            &url,
            r#"[{"type":"status","op":"in","value":[200]}]"#,
        );
        {
            let conn = state.pool.get().unwrap();
            let now = util::now_ms();
            // 套件切换到 isolated，并追加一个读取 Cookie 的用例
            conn.execute(
                "UPDATE test_suites SET config_json = '{\"failFast\":true,\"cookieMode\":\"isolated\"}' WHERE id = ?1",
                params![suite_id],
            )
            .unwrap();
            let request_id: i64 = conn
                .query_row(
                    "SELECT id FROM requests WHERE url = ?1 LIMIT 1",
                    params![url],
                    |row| row.get(0),
                )
                .unwrap();
            conn.execute(
                "INSERT INTO test_cases
                    (suite_id, name, kind, request_id, sort_order, assertions_json, created_at, updated_at)
                 VALUES (?1, '校验会话', 'request', ?2, 2, ?3, ?4, ?4)",
                params![
                    suite_id,
                    request_id,
                    r#"[{"type":"jsonPath","path":"$.cookie","op":"contains","value":"pre=1"},
                        {"type":"jsonPath","path":"$.cookie","op":"contains","value":"sid=abc"}]"#,
                    now
                ],
            )
            .unwrap();
            // 运行开始前的本机 Cookie 应进入内存罐初始值
            conn.execute(
                "INSERT INTO cookies (domain, path, name, value, created_at, updated_at)
                 VALUES ('127.0.0.1', '/', 'pre', '1', ?1, ?1)",
                params![now],
            )
            .unwrap();
        }

        let run_id = create_queued_run(&state, suite_id, "ui").unwrap();
        let summary = tauri::async_runtime::block_on(run_suite(
            &state,
            suite_id,
            run_id,
            RunOptions::default(),
            &|_| {},
        ))
        .unwrap();
        {
            let conn = state.pool.get().unwrap();
            let detail = load_run_detail(&conn, run_id).unwrap();
            let rows: Vec<_> = detail
                .cases
                .iter()
                .map(|case| (&case.name, &case.status, &case.error, &case.logs))
                .collect();
            assert_eq!(summary.status, "passed", "{summary:?} {rows:?}");
        }

        // isolated：Set-Cookie 只留在内存罐，不回写本机 Cookie 库
        let conn = state.pool.get().unwrap();
        let names: Vec<String> = {
            let mut stmt = conn
                .prepare("SELECT name FROM cookies ORDER BY name")
                .unwrap();
            stmt.query_map([], |row| row.get(0))
                .unwrap()
                .flatten()
                .collect()
        };
        assert_eq!(names, vec!["pre".to_string()]);
        drop(conn);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn failing_assertion_marks_case_failed_and_stops_on_fail_fast() {
        let url = spawn_server(r#"{"code":7}"#);
        let (root, state) = temp_state("fail");
        let suite_id = seed_suite(
            &state,
            &url,
            r#"[{"type":"jsonPath","path":"$.code","op":"eq","value":0,"name":"业务码为 0"}]"#,
        );
        {
            // 追加第二个用例，failFast 生效时应被跳过
            let conn = state.pool.get().unwrap();
            let now = util::now_ms();
            let request_id: i64 = conn
                .query_row("SELECT id FROM requests LIMIT 1", [], |row| row.get(0))
                .unwrap();
            conn.execute(
                "INSERT INTO test_cases
                    (suite_id, name, kind, request_id, sort_order, created_at, updated_at)
                 VALUES (?1, '后续用例', 'request', ?2, 2, ?3, ?3)",
                params![suite_id, request_id, now],
            )
            .unwrap();
        }

        let run_id = create_queued_run(&state, suite_id, "cli").unwrap();
        let summary = tauri::async_runtime::block_on(run_suite(
            &state,
            suite_id,
            run_id,
            RunOptions {
                trigger: "cli".to_string(),
                ..RunOptions::default()
            },
            &|_| {},
        ))
        .unwrap();

        assert_eq!(summary.status, "failed");
        assert_eq!(summary.failed, 1);
        assert_eq!(summary.skipped, 1);
        let conn = state.pool.get().unwrap();
        let detail = load_run_detail(&conn, run_id).unwrap();
        assert_eq!(detail.cases.len(), 1);
        assert_eq!(detail.cases[0].assertions_failed, 1);
        let message = detail.assertions[0].message.clone().unwrap_or_default();
        assert!(message.contains("实际 7"), "{message}");
        drop(conn);
        let _ = std::fs::remove_dir_all(&root);
    }

    // -----------------------------------------------------------------------
    // Python 钩子（T-11）
    // -----------------------------------------------------------------------

    fn enable_python(state: &AppState) {
        let mut settings = state.settings();
        settings.python_enabled = true;
        state.update_settings(settings).unwrap();
    }

    fn python_ready() -> bool {
        python_exec::check("", true).info.is_some()
    }

    fn seed_python_suite(
        state: &AppState,
        setup: Option<&str>,
        teardown: Option<&str>,
        cases: &[(&str, &str, Option<i64>)],
    ) -> i64 {
        let conn = state.pool.get().unwrap();
        let now = util::now_ms();
        conn.execute(
            "INSERT INTO projects (name, created_at, updated_at) VALUES ('p', ?1, ?1)",
            params![now],
        )
        .unwrap();
        let project_id = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO test_suites
                (project_id, name, setup_script, teardown_script, config_json, created_at, updated_at)
             VALUES (?1, 's', ?2, ?3, '{\"failFast\":true}', ?4, ?4)",
            params![project_id, setup, teardown, now],
        )
        .unwrap();
        let suite_id = conn.last_insert_rowid();
        for (index, (name, script, timeout)) in cases.iter().enumerate() {
            conn.execute(
                "INSERT INTO test_cases
                    (suite_id, name, kind, main_script, sort_order, timeout_ms, created_at, updated_at)
                 VALUES (?1, ?2, 'python', ?3, ?4, ?5, ?6, ?6)",
                params![
                    suite_id,
                    name,
                    script,
                    index as i64 + 1,
                    timeout.unwrap_or(30_000),
                    now
                ],
            )
            .unwrap();
        }
        suite_id
    }

    fn run_now(state: &AppState, suite_id: i64, trigger: &str) -> (i64, RunSummary) {
        let run_id = create_queued_run(state, suite_id, trigger).unwrap();
        let summary = tauri::async_runtime::block_on(run_suite(
            state,
            suite_id,
            run_id,
            RunOptions {
                trigger: trigger.to_string(),
                ..RunOptions::default()
            },
            &|_| {},
        ))
        .unwrap();
        (run_id, summary)
    }

    #[test]
    fn python_case_runs_and_shares_runtime_variables() {
        if !python_ready() {
            return;
        }
        let (root, state) = temp_state("py-main");
        enable_python(&state);
        let suite_id = seed_python_suite(
            &state,
            None,
            None,
            &[
                (
                    "生成订单",
                    "import astroman_runtime as ar\n\
                     ar.log('生成订单')\n\
                     ar.attach('out.json', b'{\"ok\": 1}')\n\
                     ar.emit(variables={'orderId': 42})\n",
                    None,
                ),
                (
                    "校验变量",
                    "import astroman_runtime as ar\n\
                     ctx = ar.load_context()\n\
                     ar.assert_eq(ctx['variables'].get('orderId'), 42, '拿不到上一步变量')\n\
                     ar.emit()\n",
                    None,
                ),
            ],
        );
        let (run_id, summary) = run_now(&state, suite_id, "ui");
        assert_eq!(summary.status, "passed", "{summary:?}");
        assert_eq!(summary.passed, 2);

        let conn = state.pool.get().unwrap();
        let detail = load_run_detail(&conn, run_id).unwrap();
        assert!(
            detail.cases.iter().all(|case| case.status == "passed"),
            "{:?}",
            detail.cases
        );
        let logs = detail.cases[0].logs.clone().unwrap_or_default();
        assert!(logs.contains("[main] 生成订单"), "{logs}");
        let version = detail.cases[0].python_version.clone().unwrap_or_default();
        assert!(version.starts_with("3."), "{version}");
        let script_hash = detail.cases[0].script_hash.clone().unwrap_or_default();
        assert!(script_hash.starts_with("sha256:"), "{script_hash}");
        let artifacts: i64 = conn
            .query_row("SELECT COUNT(*) FROM test_artifacts", [], |row| row.get(0))
            .unwrap();
        assert_eq!(artifacts, 1);
        drop(conn);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn python_requires_explicit_authorization() {
        let (root, state) = temp_state("py-auth");
        let suite_id = seed_python_suite(
            &state,
            None,
            None,
            &[("脚本用例", "print('should not run')\n", None)],
        );
        let (run_id, summary) = run_now(&state, suite_id, "ui");
        assert_eq!(summary.status, "failed");
        assert_eq!(summary.errored, 1);
        let conn = state.pool.get().unwrap();
        let detail = load_run_detail(&conn, run_id).unwrap();
        let error = detail.cases[0].error.clone().unwrap_or_default();
        assert!(error.contains("PYTHON_NOT_AUTHORIZED"), "{error}");
        drop(conn);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn suite_setup_failure_skips_cases_and_still_runs_teardown() {
        if !python_ready() {
            return;
        }
        let (root, state) = temp_state("py-setup");
        enable_python(&state);
        let suite_id = seed_python_suite(
            &state,
            Some(
                "import astroman_runtime as ar\n\
                 ar.emit(ok=False, logs=[{'level': 'error', 'message': '数据库连不上'}])\n",
            ),
            Some("import astroman_runtime as ar\nar.emit(variables={'cleaned': True})\n"),
            &[("用例一", "import astroman_runtime as ar\nar.emit()\n", None)],
        );
        let (run_id, summary) = run_now(&state, suite_id, "ui");
        assert_eq!(summary.status, "error", "{summary:?}");
        assert_eq!(summary.skipped, 1);

        let conn = state.pool.get().unwrap();
        let detail = load_run_detail(&conn, run_id).unwrap();
        let names: Vec<_> = detail
            .cases
            .iter()
            .map(|case| (case.name.as_str(), case.status.as_str()))
            .collect();
        assert!(names.contains(&("用例一", "skipped")), "{names:?}");
        assert!(
            names.contains(&("[teardown] 套件清理", "passed")),
            "{names:?}"
        );
        let setup = detail
            .cases
            .iter()
            .find(|case| case.name == "[setup] 套件初始化")
            .expect("setup 结果应落库");
        assert_eq!(setup.status, "failed");
        let error = setup.error.clone().unwrap_or_default();
        assert!(error.contains("数据库连不上"), "{error}");
        drop(conn);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn teardown_failure_marks_run_failed_unless_not_fatal() {
        if !python_ready() {
            return;
        }
        let (root, state) = temp_state("py-teardown");
        enable_python(&state);
        let suite_id = seed_python_suite(
            &state,
            None,
            Some("import astroman_runtime as ar\nar.emit(ok=False)\n"),
            &[("用例一", "import astroman_runtime as ar\nar.emit()\n", None)],
        );
        let (_, summary) = run_now(&state, suite_id, "ui");
        assert_eq!(summary.status, "failed", "{summary:?}");
        assert_eq!(summary.passed, 1);

        // teardownFailureFatal=false 时降级为警告，运行状态不变
        {
            let conn = state.pool.get().unwrap();
            conn.execute(
                "UPDATE test_suites SET config_json = '{\"failFast\":true,\"teardownFailureFatal\":false}' WHERE id = ?1",
                params![suite_id],
            )
            .unwrap();
        }
        let (_, summary) = run_now(&state, suite_id, "ui");
        assert_eq!(summary.status, "passed", "{summary:?}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn python_hooks_wrap_http_case() {
        if !python_ready() {
            return;
        }
        // 回显请求行与自定义头的服务
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut stream = stream;
                let mut buffer = [0u8; 8192];
                let read = stream.read(&mut buffer).unwrap_or(0);
                let request = String::from_utf8_lossy(&buffer[..read]).into_owned();
                let path = request
                    .lines()
                    .next()
                    .and_then(|line| line.split_whitespace().nth(1))
                    .unwrap_or("")
                    .to_string();
                let pre = request
                    .lines()
                    .find(|line| line.to_ascii_lowercase().starts_with("x-pre:"))
                    .map(|line| line["x-pre:".len()..].trim().to_string())
                    .unwrap_or_default();
                let body = serde_json::json!({ "path": path, "pre": pre }).to_string();
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(response.as_bytes());
            }
        });
        let url = format!("http://{addr}/api/orders");

        let (root, state) = temp_state("py-hooks");
        enable_python(&state);
        let suite_id = seed_suite(
            &state,
            &url,
            r#"[{"type":"jsonPath","path":"$.pre","op":"eq","value":"1"}]"#,
        );
        {
            let conn = state.pool.get().unwrap();
            conn.execute(
                "UPDATE test_cases SET pre_script = ?1, post_script = ?2 WHERE suite_id = ?3",
                params![
                    "import astroman_runtime as ar\n\
                     ar.emit(variables={'token': 'abc'}, request={'headers': {'x-pre': '1'}})\n",
                    "import astroman_runtime as ar\n\
                     ctx = ar.load_context()\n\
                     ar.assert_eq(ctx['response']['status'], 200, '后置读不到响应')\n\
                     ar.assert_eq(ctx['variables'].get('token'), 'abc', '后置读不到变量')\n\
                     ar.emit(variables={'post': 'done'})\n",
                    suite_id
                ],
            )
            .unwrap();
        }
        let (run_id, summary) = run_now(&state, suite_id, "ui");
        assert_eq!(summary.status, "passed", "{summary:?}");

        let conn = state.pool.get().unwrap();
        let detail = load_run_detail(&conn, run_id).unwrap();
        // 1 条声明式 + 2 条后置断言
        assert_eq!(detail.cases[0].assertions_total, 3);
        assert_eq!(detail.cases[0].assertions_failed, 0);
        assert!(detail.cases[0].python_duration_ms > 0);
        drop(conn);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn python_post_assertion_failure_marks_case_failed() {
        if !python_ready() {
            return;
        }
        let url = spawn_server(r#"{"code":0}"#);
        let (root, state) = temp_state("py-post-fail");
        enable_python(&state);
        let suite_id = seed_suite(&state, &url, r#"[{"type":"status","value":200}]"#);
        {
            let conn = state.pool.get().unwrap();
            conn.execute(
                "UPDATE test_cases SET post_script = ?1 WHERE suite_id = ?2",
                params![
                    "import astroman_runtime as ar\n\
                     ar.assert_true(False, '后置断言失败')\n\
                     ar.emit()\n",
                    suite_id
                ],
            )
            .unwrap();
        }
        let (run_id, summary) = run_now(&state, suite_id, "ui");
        assert_eq!(summary.status, "failed", "{summary:?}");
        assert_eq!(summary.failed, 1);

        let conn = state.pool.get().unwrap();
        let detail = load_run_detail(&conn, run_id).unwrap();
        assert_eq!(detail.cases[0].status, "failed");
        let error = detail.cases[0].error.clone().unwrap_or_default();
        assert!(error.contains("后置断言失败"), "{error}");
        drop(conn);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn python_skip_and_timeout_are_recorded() {
        if !python_ready() {
            return;
        }
        let (root, state) = temp_state("py-skip-timeout");
        enable_python(&state);
        let suite_id = seed_python_suite(
            &state,
            None,
            None,
            &[
                (
                    "跳过用例",
                    "import astroman_runtime as ar\nar.emit(skip=True)\n",
                    None,
                ),
                ("超时用例", "while True:\n    pass\n", Some(1000_i64)),
            ],
        );
        let (run_id, summary) = run_now(&state, suite_id, "ui");
        // 跳过不算失败，超时算 error
        assert_eq!(summary.status, "failed", "{summary:?}");
        assert_eq!(summary.skipped, 1);
        assert_eq!(summary.errored, 1);

        let conn = state.pool.get().unwrap();
        let detail = load_run_detail(&conn, run_id).unwrap();
        assert_eq!(detail.cases[0].status, "skipped");
        assert_eq!(detail.cases[1].status, "error");
        let error = detail.cases[1].error.clone().unwrap_or_default();
        assert!(error.contains("PYTHON_TIMEOUT"), "{error}");
        drop(conn);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn secret_variables_are_masked_in_logs() {
        if !python_ready() {
            return;
        }
        let (root, state) = temp_state("py-mask");
        enable_python(&state);
        let suite_id = seed_python_suite(
            &state,
            None,
            None,
            &[(
                "打印密钥",
                "import astroman_runtime as ar\n\
                 ctx = ar.load_context()\n\
                 ar.log('password is %s' % ctx['variables'].get('password'))\n\
                 ar.emit()\n",
                None,
            )],
        );
        {
            let conn = state.pool.get().unwrap();
            conn.execute(
                "UPDATE test_cases SET variables_json = '{\"password\":\"p@ssw0rd-123\"}' WHERE suite_id = ?1",
                params![suite_id],
            )
            .unwrap();
        }
        let (run_id, _) = run_now(&state, suite_id, "ui");
        let conn = state.pool.get().unwrap();
        let detail = load_run_detail(&conn, run_id).unwrap();
        let logs = detail.cases[0].logs.clone().unwrap_or_default();
        assert!(!logs.contains("p@ssw0rd-123"), "{logs}");
        assert!(logs.contains("******") || logs.contains("***"), "{logs}");
        drop(conn);
        let _ = std::fs::remove_dir_all(&root);
    }

    // -----------------------------------------------------------------------
    // CLI 运行参数（T-17）
    // -----------------------------------------------------------------------

    fn case_fixture(name: &str) -> TestCase {
        TestCase {
            id: 1,
            suite_id: 1,
            name: name.to_string(),
            kind: "request".to_string(),
            request_id: None,
            plan_id: None,
            dataset_id: None,
            depends_on: None,
            sort_order: 1,
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
    fn tag_filter_parses_comma_and_json_forms() {
        let mut case = case_fixture("a");
        case.tags = Some("smoke, order".to_string());
        assert_eq!(case_tags(&case), vec!["smoke", "order"]);
        case.tags = Some("[\"smoke\",\"order\"]".to_string());
        assert_eq!(case_tags(&case), vec!["smoke", "order"]);

        let include = vec!["smoke".to_string()];
        let exclude = vec!["slow".to_string()];
        assert!(tag_matches(&case, &include, &exclude));
        assert!(!tag_matches(&case, &["slow".to_string()], &[]));
        assert!(!tag_matches(&case, &[], &["order".to_string()]));
        assert!(tag_matches(&case, &[], &[]));
    }

    #[test]
    fn tag_filter_skips_unmatched_cases() {
        let url = spawn_server(r#"{"code":0}"#);
        let (root, state) = temp_state("tags");
        let suite_id = seed_suite(&state, &url, r#"[{"type":"status","op":"eq","value":200}]"#);
        {
            let conn = state.pool.get().unwrap();
            conn.execute(
                "UPDATE test_cases SET tags = 'slow' WHERE suite_id = ?1",
                params![suite_id],
            )
            .unwrap();
        }
        let error = tauri::async_runtime::block_on(run_suite(
            &state,
            suite_id,
            create_queued_run(&state, suite_id, "cli").unwrap(),
            RunOptions {
                trigger: "cli".to_string(),
                tags: vec!["smoke".to_string()],
                ..RunOptions::default()
            },
            &|_| {},
        ))
        .unwrap_err();
        assert!(error.contains("标签过滤"), "{error}");
        let conn = state.pool.get().unwrap();
        let status: String = conn
            .query_row(
                "SELECT status FROM test_runs ORDER BY id DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(status, "queued");
        drop(conn);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn stress_case_runs_and_persists_report_with_sla() {
        let url = spawn_server(r#"{"ok":true}"#);
        let (root, state) = temp_state("stress-case");
        let suite_id = seed_suite(&state, &url, "[]");
        let now = util::now_ms();
        {
            let conn = state.pool.get().unwrap();
            conn.execute(
                "DELETE FROM test_cases WHERE suite_id = ?1",
                params![suite_id],
            )
            .unwrap();
            let stress = json!({
                "name": "压测用例",
                "method": "GET",
                "url": url,
                "concurrent": 2,
                "durationSec": 1,
                "timeoutMs": 2000,
                "sla": { "p95Ms": 5000, "p99Ms": 5000, "errorRate": 100, "minRps": 0 }
            })
            .to_string();
            conn.execute(
                "INSERT INTO test_cases
                    (suite_id, name, kind, sort_order, stress_json, created_at, updated_at)
                 VALUES (?1, '压测用例', 'stress', 1, ?2, ?3, ?3)",
                params![suite_id, stress, now],
            )
            .unwrap();
        }
        let run_id = create_queued_run(&state, suite_id, "ui").unwrap();
        let summary = tauri::async_runtime::block_on(run_suite(
            &state,
            suite_id,
            run_id,
            RunOptions::default(),
            &|_| {},
        ))
        .unwrap();
        assert_eq!(summary.status, "passed", "{summary:?}");
        let conn = state.pool.get().unwrap();
        let (case_status, report_id): (String, Option<i64>) = conn
            .query_row(
                "SELECT status, report_id FROM test_run_cases WHERE run_id = ?1",
                params![run_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(case_status, "passed");
        let report_id = report_id.expect("压测用例应关联报告");
        let requests: i64 = conn
            .query_row(
                "SELECT total_requests FROM test_reports WHERE id = ?1",
                params![report_id],
                |row| row.get(0),
            )
            .unwrap();
        assert!(requests > 0, "压测应产生真实请求");
        drop(conn);
        let _ = std::fs::remove_dir_all(&root);
    }

    fn stress_report_fixture(p95: f64, p99: f64, avg_qps: f64, error_rate: f64) -> StressReport {
        StressReport {
            id: 0,
            plan_name: "fixture".to_string(),
            request_id: None,
            total_requests: 100,
            success_count: 100,
            error_count: 0,
            avg_rt: p95,
            min_rt: 1.0,
            max_rt: p99,
            p50_rt: p95 / 2.0,
            p75_rt: p95,
            p90_rt: p95,
            p95_rt: p95,
            p99_rt: p99,
            p999_rt: p99,
            stddev_rt: 1.0,
            ttfb_avg: 1.0,
            ttfb_p95: 1.0,
            apdex: 1.0,
            bytes_total: 1,
            max_qps: avg_qps,
            avg_qps,
            target_rps: 0.0,
            error_rate,
            concurrency: 10,
            status: "completed".to_string(),
            start_time: Some(0),
            end_time: Some(1000),
            created_at: 1000,
        }
    }

    #[test]
    fn stress_sla_gate_flags_each_metric() {
        let targets = SlaTargets {
            p95_ms: Some(500.0),
            p99_ms: Some(1000.0),
            error_rate: Some(0.5),
            min_rps: Some(50.0),
        };
        let bad = stress_report_fixture(612.0, 1200.0, 40.0, 1.2);
        let assertions = stress_gate_assertions(&bad, Some(&targets), None);
        let failed: Vec<&str> = assertions
            .iter()
            .filter(|item| !item.passed)
            .map(|item| item.name.as_str())
            .collect();
        assert_eq!(failed.len(), 4, "{failed:?}");
        assert!(stress_error_summary(&assertions)
            .unwrap()
            .contains("压测门禁未通过"));

        let good = stress_report_fixture(420.0, 800.0, 60.0, 0.0);
        let assertions = stress_gate_assertions(&good, Some(&targets), None);
        assert!(assertions.iter().all(|item| item.passed));
        assert!(stress_error_summary(&assertions).is_none());
    }

    #[test]
    fn stress_baseline_regression_uses_documented_thresholds() {
        let baseline = StressBaseline {
            id: 7,
            p95: 500.0,
            p99: 900.0,
            avg_qps: 100.0,
            error_rate: 0.2,
        };
        // P95 612 > 500×1.2=600，RPS 70 < 100×0.8=80，错误率 1.0 > 0.2+0.5
        let regressed = stress_report_fixture(612.0, 900.0, 70.0, 1.0);
        let assertions = stress_gate_assertions(&regressed, None, Some(&baseline));
        let failed: Vec<&str> = assertions
            .iter()
            .filter(|item| !item.passed)
            .map(|item| item.name.as_str())
            .collect();
        assert_eq!(failed.len(), 3, "{failed:?}");

        let stable = stress_report_fixture(550.0, 950.0, 90.0, 0.4);
        let assertions = stress_gate_assertions(&stable, None, Some(&baseline));
        assert!(assertions.iter().all(|item| item.passed), "{assertions:?}");
    }
}
