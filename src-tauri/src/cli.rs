//! 无头 CLI（`astroman-cli`，文档 8）
//!
//! 与桌面端共用同一套 SQLite / 执行引擎 / 报告生成器，不启动窗口。
//! 退出码见文档 8.3：0 通过、1 断言失败、2 执行错误、3 配置错误、4 取消、5 库不可用。

use crate::commands::test_case::load_cases;
use crate::commands::test_preflight;
use crate::models::RunSummary;
use crate::report_builder;
use crate::state::AppState;
use crate::test_runner::{self, RunOptions, TestEvent};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// 退出码定义（文档 8.3）
pub const EXIT_OK: i32 = 0;
pub const EXIT_ASSERT_FAILED: i32 = 1;
pub const EXIT_RUN_ERROR: i32 = 2;
pub const EXIT_CONFIG: i32 = 3;
pub const EXIT_CANCELLED: i32 = 4;
pub const EXIT_DB: i32 = 5;

struct CliError {
    code: i32,
    message: String,
}

impl CliError {
    fn config(message: impl Into<String>) -> Self {
        Self {
            code: EXIT_CONFIG,
            message: message.into(),
        }
    }
    fn db(message: impl Into<String>) -> Self {
        Self {
            code: EXIT_DB,
            message: message.into(),
        }
    }
}

type CliResult<T> = Result<T, CliError>;

fn config<T>(value: Result<T, String>) -> CliResult<T> {
    value.map_err(CliError::config)
}

// ---------------------------------------------------------------------------
// 参数解析
// ---------------------------------------------------------------------------

const BOOL_FLAGS: [&str; 5] = ["fail-fast", "allow-prod", "json", "help", "strict"];

#[derive(Default)]
struct Args {
    positionals: Vec<String>,
    values: HashMap<String, Vec<String>>,
    flags: HashSet<String>,
}

impl Args {
    fn parse(tokens: &[String]) -> CliResult<Self> {
        let mut args = Args::default();
        let mut index = 0;
        while index < tokens.len() {
            let token = &tokens[index];
            if let Some(raw) = token.strip_prefix("--") {
                let (name, inline) = match raw.split_once('=') {
                    Some((name, value)) => (name.to_string(), Some(value.to_string())),
                    None => (raw.to_string(), None),
                };
                let name = name.to_ascii_lowercase();
                if BOOL_FLAGS.contains(&name.as_str()) {
                    args.flags.insert(name);
                    index += 1;
                    continue;
                }
                let value = match inline {
                    Some(value) => value,
                    None => {
                        let next = tokens.get(index + 1).cloned().unwrap_or_default();
                        if next.starts_with("--") || next.is_empty() {
                            return Err(CliError::config(format!("参数 --{name} 缺少值")));
                        }
                        index += 1;
                        next
                    }
                };
                args.values.entry(name).or_default().push(value);
                index += 1;
                continue;
            }
            if token.starts_with("-h") {
                args.flags.insert("help".into());
            } else {
                args.positionals.push(token.clone());
            }
            index += 1;
        }
        Ok(args)
    }

    fn value(&self, name: &str) -> Option<&str> {
        self.values
            .get(name)
            .and_then(|items| items.last())
            .map(String::as_str)
    }

    fn has(&self, name: &str) -> bool {
        self.flags.contains(name)
    }

    fn list(&self, name: &str) -> Vec<String> {
        self.value(name)
            .map(|text| {
                text.split(',')
                    .map(str::trim)
                    .filter(|item| !item.is_empty())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default()
    }
}

const HELP: &str = "AstroMan CLI（无窗口执行测试套件）

用法：
  astroman-cli test run       --suite <名称|ID> [--project <ID|名称>] [--env <名称|ID>]
                              [--env-file <path>] [--data <path>] [--tags a,b]
                              [--exclude-tags a,b] [--report html,json,junit,summary]
                              [--out <dir>] [--fail-fast] [--timeout <秒>]
                              [--allow-prod] [--json] [--db <path>]
  astroman-cli test list      [--suite <名称|ID>] [--project <ID|名称>] [--json] [--db <path>]
  astroman-cli test validate  --suite <名称|ID> [--project <ID|名称>] [--env <名称|ID>]
                              [--env-file <path>] [--json] [--db <path>]
  astroman-cli report export  --run <runId> [--report html,json,junit,summary]
                              [--out <dir>] [--db <path>]
  astroman-cli python check   [--json] [--db <path>]
  astroman-cli suite import   <pack.astromantest.json> [--project <ID|名称>]
                              [--on-conflict skip|overwrite|rename] [--db <path>]

退出码：0 通过 / 1 断言失败 / 2 执行错误 / 3 配置错误 / 4 取消 / 5 数据库不可用";

// ---------------------------------------------------------------------------
// 入口
// ---------------------------------------------------------------------------

pub fn main() -> i32 {
    let tokens: Vec<String> = std::env::args().skip(1).collect();
    let args = match Args::parse(&tokens) {
        Ok(args) => args,
        Err(error) => {
            eprintln!("{}", error.message);
            return error.code;
        }
    };
    if args.has("help") || args.positionals.is_empty() {
        println!("{HELP}");
        return if args.positionals.is_empty() && !args.has("help") {
            EXIT_CONFIG
        } else {
            EXIT_OK
        };
    }
    match dispatch(&args) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("{}", error.message);
            error.code
        }
    }
}

fn dispatch(args: &Args) -> CliResult<i32> {
    let first = args.positionals[0].as_str();
    let second = args.positionals.get(1).map(String::as_str).unwrap_or("");
    match (first, second) {
        ("test", "run") => cmd_test_run(args),
        ("test", "list") => cmd_test_list(args),
        ("test", "validate") => cmd_test_validate(args),
        ("report", "export") => cmd_report_export(args),
        ("python", "check") => cmd_python_check(args),
        ("suite", "import") => cmd_suite_import(args),
        _ => Err(CliError::config(format!(
            "未知命令：{} {}（用 --help 查看用法）",
            first, second
        ))),
    }
}

// ---------------------------------------------------------------------------
// 数据目录
// ---------------------------------------------------------------------------

fn home_dir() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn platform_dirs() -> (PathBuf, PathBuf) {
    // 与 Tauri app.path() 保持一致：data_dir / app_config_dir
    if cfg!(target_os = "windows") {
        let roaming = std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| home_dir().join("AppData/Roaming"));
        (roaming.join("AstroMan"), roaming.join("com.astroman.app"))
    } else if cfg!(target_os = "macos") {
        let base = home_dir().join("Library/Application Support");
        (base.join("AstroMan"), base.join("com.astroman.app"))
    } else {
        let data = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home_dir().join(".local/share"));
        let config = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home_dir().join(".config"));
        (data.join("AstroMan"), config.join("com.astroman.app"))
    }
}

struct Env {
    state: AppState,
}

impl Env {
    fn open(args: &Args) -> CliResult<Self> {
        let (default_data, default_config) = platform_dirs();
        let config_dir = std::env::var_os("ASTROMAN_CONFIG_DIR")
            .map(PathBuf::from)
            .unwrap_or(default_config);
        let data_dir = std::env::var_os("ASTROMAN_DATA_DIR")
            .map(PathBuf::from)
            .unwrap_or(default_data);
        let db_override = args.value("db").map(|text| PathBuf::from(text.trim()));

        let (data_dir, db_path) = match &db_override {
            Some(path) => {
                if path.as_os_str().is_empty() {
                    return Err(CliError::config("--db 不能为空"));
                }
                let parent = path
                    .parent()
                    .filter(|item| !item.as_os_str().is_empty())
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| PathBuf::from("."));
                (parent, path.clone())
            }
            None => {
                // storage.json 里的自定义数据目录优先（与桌面端一致）
                let config_file = config_dir.join("storage.json");
                let custom = crate::state::read_storage_config(&config_file);
                let dir = custom.unwrap_or(data_dir);
                let db = dir.join(crate::state::DB_FILE);
                (dir, db)
            }
        };

        check_db_version(&db_path)?;
        let state = AppState::headless_at(data_dir, config_dir, db_path).map_err(|error| {
            if error.to_ascii_lowercase().contains("locked")
                || error.to_ascii_lowercase().contains("busy")
            {
                CliError::db(format!("数据库被占用：{error}"))
            } else {
                CliError::config(error)
            }
        })?;
        Ok(Self { state })
    }
}

/// 库版本高于当前程序时直接退出 5（文档 8.3）
fn check_db_version(db_path: &Path) -> CliResult<()> {
    if !db_path.is_file() {
        return Ok(());
    }
    let conn = Connection::open_with_flags(db_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|error| CliError::db(format!("无法打开数据库：{error}")))?;
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(|error| CliError::db(format!("读取数据库版本失败：{error}")))?;
    if version > crate::db::DB_VERSION {
        return Err(CliError::db(format!(
            "数据库版本 {version} 高于当前程序支持的 {}，请升级 AstroMan",
            crate::db::DB_VERSION
        )));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 套件 / 环境解析
// ---------------------------------------------------------------------------

fn resolve_project(conn: &Connection, spec: Option<&str>) -> CliResult<Option<i64>> {
    let Some(spec) = spec.map(str::trim).filter(|item| !item.is_empty()) else {
        return Ok(None);
    };
    if let Ok(id) = spec.parse::<i64>() {
        let exists: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM projects WHERE id = ?1",
                params![id],
                |row| Ok(row.get::<_, i64>(0)? > 0),
            )
            .unwrap_or(false);
        if exists {
            return Ok(Some(id));
        }
    }
    let id: Option<i64> = conn
        .query_row(
            "SELECT id FROM projects WHERE name = ?1 ORDER BY id LIMIT 1",
            params![spec],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| CliError::config(crate::util::err(error)))?;
    match id {
        Some(id) => Ok(Some(id)),
        None => Err(CliError::config(format!("项目不存在：{spec}"))),
    }
}

fn resolve_suite(conn: &Connection, spec: &str, project: Option<i64>) -> CliResult<i64> {
    let spec = spec.trim();
    if spec.is_empty() {
        return Err(CliError::config("--suite 不能为空"));
    }
    if let Ok(id) = spec.parse::<i64>() {
        let exists: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM test_suites WHERE id = ?1 AND (?2 IS NULL OR project_id = ?2)",
                params![id, project],
                |row| Ok(row.get::<_, i64>(0)? > 0),
            )
            .unwrap_or(false);
        if exists {
            return Ok(id);
        }
    }
    let mut stmt = conn
        .prepare(
            "SELECT id FROM test_suites
             WHERE name = ?1 AND (?2 IS NULL OR project_id = ?2)
             ORDER BY id",
        )
        .map_err(|error| CliError::config(crate::util::err(error)))?;
    let ids: Vec<i64> = stmt
        .query_map(params![spec, project], |row| row.get(0))
        .map_err(|error| CliError::config(crate::util::err(error)))?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|error| CliError::config(crate::util::err(error)))?;
    match ids.len() {
        0 => Err(CliError::config(format!("套件不存在：{spec}"))),
        1 => Ok(ids[0]),
        _ => Err(CliError::config(format!(
            "套件名「{spec}」对应多个套件（{}），请用 --project 或 --suite <id> 指定",
            ids.iter()
                .map(|id| id.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ))),
    }
}

fn resolve_env(conn: &Connection, project_id: i64, spec: Option<&str>) -> CliResult<Option<i64>> {
    let Some(spec) = spec.map(str::trim).filter(|item| !item.is_empty()) else {
        return Ok(None);
    };
    if let Ok(id) = spec.parse::<i64>() {
        let exists: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM environments WHERE id = ?1 AND project_id = ?2",
                params![id, project_id],
                |row| Ok(row.get::<_, i64>(0)? > 0),
            )
            .unwrap_or(false);
        if exists {
            return Ok(Some(id));
        }
    }
    let ids: Vec<(i64, String)> = conn
        .prepare("SELECT id, name FROM environments WHERE project_id = ?1 ORDER BY id")
        .map_err(|error| CliError::config(crate::util::err(error)))?
        .query_map(params![project_id], |row| Ok((row.get(0)?, row.get(1)?)))
        .map_err(|error| CliError::config(crate::util::err(error)))?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|error| CliError::config(crate::util::err(error)))?;
    let hits: Vec<i64> = ids
        .into_iter()
        .filter(|(_, name)| name == spec)
        .map(|(id, _)| id)
        .collect();
    match hits.len() {
        0 => Err(CliError::config(format!("环境不存在：{spec}"))),
        1 => Ok(Some(hits[0])),
        _ => Err(CliError::config(format!(
            "环境名「{spec}」不唯一，请改用 --env <id>"
        ))),
    }
}

/// 生产环境守卫：名称含 prod / 生产 时默认拒绝（文档 8.2 --allow-prod）
fn guard_prod(conn: &Connection, env_id: Option<i64>, allow: bool) -> CliResult<()> {
    if allow {
        return Ok(());
    }
    let Some(env_id) = env_id else {
        return Ok(());
    };
    let name: Option<String> = conn
        .query_row(
            "SELECT name FROM environments WHERE id = ?1",
            params![env_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| CliError::config(crate::util::err(error)))?;
    let Some(name) = name else { return Ok(()) };
    let lower = name.to_ascii_lowercase();
    if lower.contains("prod") || name.contains("生产") {
        return Err(CliError::config(format!(
            "环境「{name}」疑似生产环境，如确认要执行请加 --allow-prod"
        )));
    }
    Ok(())
}

#[derive(Default)]
struct EnvFile {
    vars: HashMap<String, String>,
    secrets: HashMap<String, String>,
}

fn load_env_file(path: Option<&str>) -> CliResult<EnvFile> {
    let Some(path) = path.map(str::trim).filter(|item| !item.is_empty()) else {
        return Ok(EnvFile::default());
    };
    let text = std::fs::read_to_string(path)
        .map_err(|error| CliError::config(format!("读取 --env-file 失败：{error}")))?;
    let value: Value = serde_json::from_str(&text)
        .map_err(|error| CliError::config(format!("--env-file 不是合法 JSON：{error}")))?;
    let mut file = EnvFile::default();
    let Value::Object(map) = value else {
        return Err(CliError::config("--env-file 顶层必须是 JSON 对象"));
    };
    for (key, item) in map {
        match key.as_str() {
            "vars" => {
                if let Value::Object(vars) = item {
                    for (name, value) in vars {
                        file.vars.insert(name, value_to_text(value));
                    }
                }
            }
            "secrets" => match item {
                Value::Object(items) => {
                    for (name, value) in items {
                        let raw = value_to_text(value);
                        // 支持 "$ENV:NAME" 与空值→读同名环境变量
                        let resolved = if let Some(env_name) = raw.strip_prefix("$ENV:") {
                            std::env::var(env_name).unwrap_or_default()
                        } else if raw.is_empty() {
                            std::env::var(&name).ok().unwrap_or_default()
                        } else {
                            raw
                        };
                        file.secrets.insert(name, resolved);
                    }
                }
                Value::Array(items) => {
                    for item in items {
                        if let Some(name) = item.as_str() {
                            let value = std::env::var(name).ok().unwrap_or_default();
                            file.secrets.insert(name.to_string(), value);
                        }
                    }
                }
                _ => {}
            },
            _ => {
                file.vars.insert(key, value_to_text(item));
            }
        }
    }
    Ok(file)
}

fn value_to_text(value: Value) -> String {
    match value {
        Value::String(text) => text,
        other => other.to_string(),
    }
}

// ---------------------------------------------------------------------------
// 命令实现
// ---------------------------------------------------------------------------

fn cmd_test_run(args: &Args) -> CliResult<i32> {
    let spec = args
        .value("suite")
        .ok_or_else(|| CliError::config("缺少 --suite"))?;
    let env = Env::open(args)?;
    let state = env.state;
    let suite_id = {
        let conn = state
            .pool
            .get()
            .map_err(|error| CliError::db(crate::util::err(error)))?;
        let project = resolve_project(&conn, args.value("project"))?;
        resolve_suite(&conn, spec, project)?
    };
    let suite = {
        let conn = state
            .pool
            .get()
            .map_err(|error| CliError::db(crate::util::err(error)))?;
        crate::commands::test_suite::find_suite(&conn, suite_id).map_err(CliError::config)?
    };
    let (env_id, env_name) = {
        let conn = state
            .pool
            .get()
            .map_err(|error| CliError::db(crate::util::err(error)))?;
        let env_id = resolve_env(&conn, suite.project_id, args.value("env"))?.or(suite.env_id);
        guard_prod(&conn, env_id, args.has("allow-prod"))?;
        let name: Option<String> = match env_id {
            Some(id) => conn
                .query_row(
                    "SELECT name FROM environments WHERE id = ?1",
                    params![id],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|error| CliError::config(crate::util::err(error)))?,
            None => None,
        };
        (env_id, name)
    };
    let env_file = load_env_file(args.value("env-file"))?;

    // 运行前预检：错误直接按配置错误退出（文档 8.3 优先级 3）
    let preflight = config(test_preflight::run_preflight_with(
        &state,
        suite_id,
        env_id,
        &env_file.vars,
        &env_file.secrets.keys().cloned().collect::<HashSet<_>>(),
    ))?;
    for item in &preflight.items {
        let line = format!(
            "[{}] {}{}",
            item.code,
            item.case_name
                .as_deref()
                .map(|name| format!("{name}："))
                .unwrap_or_default(),
            item.message
        );
        if item.level == "error" {
            eprintln!("错误 {line}");
        } else {
            eprintln!("提醒 {line}");
        }
    }
    if !preflight.runnable {
        return Err(CliError::config(format!(
            "预检未通过（{} 个错误），已取消运行",
            preflight.errors
        )));
    }

    let dataset = args
        .value("data")
        .map(|text| PathBuf::from(text.trim()))
        .map(|path| {
            if path.is_file() {
                Ok(path)
            } else {
                Err(CliError::config(format!(
                    "--data 文件不存在：{}",
                    path.display()
                )))
            }
        })
        .transpose()?;

    let options = RunOptions {
        trigger: "cli".to_string(),
        env_id,
        vars: env_file.vars.clone(),
        secrets: env_file.secrets.clone(),
        fail_fast: args.has("fail-fast").then_some(true),
        tags: args.list("tags"),
        exclude_tags: args.list("exclude-tags"),
        timeout_sec: args
            .value("timeout")
            .map(|text| {
                text.parse::<u64>()
                    .map_err(|_| CliError::config(format!("--timeout 不是整数：{text}")))
            })
            .transpose()?
            .filter(|secs| *secs > 0),
        dataset_override: dataset,
    };

    let run_id = config(test_runner::create_queued_run(&state, suite_id, "cli"))?;
    let machine = args.has("json");
    let sink = |event: TestEvent| {
        if machine || event.name != "test:case:end" && event.name != "test:run:progress" {
            return;
        }
        let payload = &event.payload;
        let name = payload
            .get("caseName")
            .and_then(Value::as_str)
            .or_else(|| payload.get("name").and_then(Value::as_str));
        let status = payload.get("status").and_then(Value::as_str).unwrap_or("");
        if let Some(name) = name {
            eprintln!("  {name} · {status}");
        }
    };
    let summary = match tauri::async_runtime::block_on(test_runner::run_suite(
        &state, suite_id, run_id, options, &sink,
    )) {
        Ok(summary) => summary,
        Err(error) => {
            // 与桌面端一致：把运行记录标为 error，便于事后在库里查到
            if let Ok(conn) = state.pool.get() {
                let _ = conn.execute(
                    "UPDATE test_runs SET status = 'error', finished_at = ?1, summary_json = ?2
                     WHERE id = ?3",
                    params![
                        crate::util::now_ms(),
                        json!({ "error": error }).to_string(),
                        run_id
                    ],
                );
            }
            state.test_release(run_id);
            let code = if error.contains("取消") || error.contains("CANCELLED") {
                EXIT_CANCELLED
            } else {
                EXIT_RUN_ERROR
            };
            return Err(CliError {
                code,
                message: format!("运行失败：{error}"),
            });
        }
    };

    // 报告：--report 为空时按 CLI 默认 html,json,junit
    let mut formats = args.list("report");
    if formats.is_empty() && !args.values.contains_key("report") {
        formats = vec!["html".into(), "json".into(), "junit".into()];
    }
    let out_dir = args.value("out").map(|text| PathBuf::from(text.trim()));
    let mut report_dir: Option<String> = None;
    if !formats.is_empty() {
        let report = config(report_builder::generate(
            &state,
            run_id,
            &formats,
            out_dir.as_deref(),
        ))?;
        report_dir = Some(report.dir.clone());
        if !machine {
            println!("报告：{}", report.dir);
        }
    }

    if machine {
        let payload = json!({
            "runId": run_id,
            "suiteId": suite_id,
            "suite": suite.name,
            "env": env_name,
            "status": summary.status,
            "total": summary.total,
            "passed": summary.passed,
            "failed": summary.failed,
            "skipped": summary.skipped,
            "errored": summary.errored,
            "durationMs": summary.duration_ms,
            "reportDir": report_dir.as_deref().map(|dir| dir.replace('\\', "/")),
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&payload).unwrap_or_default()
        );
    } else {
        print_summary(&summary);
    }
    Ok(exit_code_for(&summary))
}

fn exit_code_for(summary: &RunSummary) -> i32 {
    if summary.errored > 0 {
        EXIT_RUN_ERROR
    } else if summary.failed > 0 {
        EXIT_ASSERT_FAILED
    } else if summary.status == "error" {
        EXIT_RUN_ERROR
    } else {
        EXIT_OK
    }
}

fn print_summary(summary: &RunSummary) {
    println!(
        "运行 #{} · {} · {}\n状态：{} · 共 {} 个用例，通过 {}，失败 {}，跳过 {}，错误 {} · {} ms",
        summary.id,
        summary.suite_name.as_deref().unwrap_or("-"),
        summary.env_name.as_deref().unwrap_or("无环境"),
        summary.status,
        summary.total,
        summary.passed,
        summary.failed,
        summary.skipped,
        summary.errored,
        summary.duration_ms
    );
}

fn cmd_test_list(args: &Args) -> CliResult<i32> {
    let env = Env::open(args)?;
    let state = env.state;
    let conn = state
        .pool
        .get()
        .map_err(|error| CliError::db(crate::util::err(error)))?;
    let machine = args.has("json");
    let project = resolve_project(&conn, args.value("project"))?;

    if let Some(spec) = args.value("suite") {
        let suite_id = resolve_suite(&conn, spec, project)?;
        let suite =
            crate::commands::test_suite::find_suite(&conn, suite_id).map_err(CliError::config)?;
        let cases = load_cases(&conn, suite_id).map_err(CliError::config)?;
        if machine {
            let rows: Vec<Value> = cases
                .iter()
                .map(|case| {
                    json!({
                        "id": case.id,
                        "name": case.name,
                        "kind": case.kind,
                        "enabled": case.enabled,
                        "tags": case.tags,
                        "dependsOn": case.depends_on,
                        "timeoutMs": case.timeout_ms,
                        "retries": case.retries,
                    })
                })
                .collect();
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "suiteId": suite_id,
                    "suite": suite.name,
                    "cases": rows,
                }))
                .unwrap_or_default()
            );
        } else {
            println!(
                "套件 #{} {}（{} 个用例）",
                suite.id,
                suite.name,
                cases.len()
            );
            for case in cases {
                println!(
                    "  #{} [{}] {}{}{}",
                    case.id,
                    case.kind,
                    case.name,
                    if case.enabled { "" } else { "（已禁用）" },
                    case.tags
                        .as_deref()
                        .filter(|text| !text.trim().is_empty())
                        .map(|text| format!(" · {text}"))
                        .unwrap_or_default()
                );
            }
        }
        return Ok(EXIT_OK);
    }

    let rows: Vec<(i64, String, i64, Option<String>, i64)> = conn
        .prepare(
            "SELECT s.id, s.name, s.project_id, p.name,
                    (SELECT COUNT(*) FROM test_cases c WHERE c.suite_id = s.id)
             FROM test_suites s LEFT JOIN projects p ON p.id = s.project_id
             WHERE (?1 IS NULL OR s.project_id = ?1)
             ORDER BY s.id",
        )
        .map_err(|error| CliError::config(crate::util::err(error)))?
        .query_map(params![project], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
            ))
        })
        .map_err(|error| CliError::config(crate::util::err(error)))?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|error| CliError::config(crate::util::err(error)))?;
    if machine {
        let items: Vec<Value> = rows
            .iter()
            .map(|(id, name, project_id, project_name, cases)| {
                json!({
                    "id": id,
                    "name": name,
                    "projectId": project_id,
                    "project": project_name,
                    "cases": cases,
                })
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({ "suites": items })).unwrap_or_default()
        );
    } else if rows.is_empty() {
        println!("没有匹配的套件");
    } else {
        for (id, name, _, project_name, cases) in rows {
            println!(
                "#{id} {name} · {} · {cases} 个用例",
                project_name.unwrap_or_else(|| "未命名项目".into())
            );
        }
    }
    Ok(EXIT_OK)
}

fn cmd_test_validate(args: &Args) -> CliResult<i32> {
    let spec = args
        .value("suite")
        .ok_or_else(|| CliError::config("缺少 --suite"))?;
    let env = Env::open(args)?;
    let state = env.state;
    let (suite_id, project_id) = {
        let conn = state
            .pool
            .get()
            .map_err(|error| CliError::db(crate::util::err(error)))?;
        let project = resolve_project(&conn, args.value("project"))?;
        let suite_id = resolve_suite(&conn, spec, project)?;
        let suite =
            crate::commands::test_suite::find_suite(&conn, suite_id).map_err(CliError::config)?;
        (suite_id, suite.project_id)
    };
    let (env_id, _) = {
        let conn = state
            .pool
            .get()
            .map_err(|error| CliError::db(crate::util::err(error)))?;
        (
            resolve_env(&conn, project_id, args.value("env"))?,
            None::<String>,
        )
    };
    let env_file = load_env_file(args.value("env-file"))?;
    let report = config(test_preflight::run_preflight_with(
        &state,
        suite_id,
        env_id,
        &env_file.vars,
        &env_file.secrets.keys().cloned().collect::<HashSet<_>>(),
    ))?;
    if args.has("json") {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "suiteId": report.suite_id,
                "suite": report.suite_name,
                "env": report.env_name,
                "runnable": report.runnable,
                "errors": report.errors,
                "warnings": report.warnings,
                "items": report.items.iter().map(|item| json!({
                    "level": item.level,
                    "code": item.code,
                    "caseId": item.case_id,
                    "case": item.case_name,
                    "message": item.message,
                    "hint": item.hint,
                })).collect::<Vec<_>>(),
            }))
            .unwrap_or_default()
        );
    } else {
        println!(
            "套件 #{} {} · 环境 {}\n结果：{}（错误 {}，提醒 {}）",
            report.suite_id,
            report.suite_name,
            report.env_name.as_deref().unwrap_or("无"),
            if report.runnable {
                "可以运行"
            } else {
                "不可运行"
            },
            report.errors,
            report.warnings
        );
        for item in &report.items {
            println!(
                "  [{}] {}{}",
                item.code,
                item.case_name
                    .as_deref()
                    .map(|name| format!("{name}："))
                    .unwrap_or_default(),
                item.message
            );
        }
    }
    Ok(if report.runnable {
        EXIT_OK
    } else {
        EXIT_CONFIG
    })
}

fn cmd_report_export(args: &Args) -> CliResult<i32> {
    let run_id = args
        .value("run")
        .ok_or_else(|| CliError::config("缺少 --run <runId>"))?
        .parse::<i64>()
        .map_err(|_| CliError::config("--run 必须是数字"))?;
    let env = Env::open(args)?;
    let state = env.state;
    let mut formats = args.list("report");
    if formats.is_empty() {
        formats = vec!["html".into(), "json".into(), "junit".into()];
    }
    let out_dir = args.value("out").map(|text| PathBuf::from(text.trim()));
    let report = config(report_builder::generate(
        &state,
        run_id,
        &formats,
        out_dir.as_deref(),
    ))?;
    println!("已导出运行 #{run_id} 的报告：{}", report.dir);
    for file in &report.files {
        println!("  {file}");
    }
    Ok(EXIT_OK)
}

fn cmd_python_check(args: &Args) -> CliResult<i32> {
    let env = Env::open(args)?;
    let state = env.state;
    let settings = state.settings();
    let check = crate::python_exec::check(settings.python_path.trim(), settings.python_enabled);
    if args.has("json") {
        println!(
            "{}",
            serde_json::to_string_pretty(&check).unwrap_or_default()
        );
    } else {
        println!(
            "授权：{}\n可用：{}\n{}",
            if check.enabled {
                "已允许执行 Python"
            } else {
                "未授权（设置 → 网络与安全 → Python 脚本）"
            },
            if check.available { "是" } else { "否" },
            check.message
        );
        if let Some(info) = &check.info {
            println!("解释器：{}（Python {}）", info.display, info.version);
        }
        if let Some(path) = &check.runtime_path {
            println!("运行时：{path}");
        }
    }
    // 未授权或不可用时按配置错误退出，便于 CI 提前拦截
    Ok(if check.info.is_some() {
        EXIT_OK
    } else {
        EXIT_CONFIG
    })
}

/// 套件包导入（文档 8.6 的最小实现：suite + cases + datasets）
fn cmd_suite_import(args: &Args) -> CliResult<i32> {
    let path = args
        .positionals
        .get(2)
        .cloned()
        .or_else(|| args.value("file").map(str::to_string))
        .ok_or_else(|| {
            CliError::config("缺少套件包路径：astroman-cli suite import <pack.astromantest.json>")
        })?;
    let env = Env::open(args)?;
    let state = env.state;
    let text = std::fs::read_to_string(&path)
        .map_err(|error| CliError::config(format!("读取套件包失败：{error}")))?;
    let pack: Value = serde_json::from_str(&text)
        .map_err(|error| CliError::config(format!("套件包不是合法 JSON：{error}")))?;
    let suite = pack
        .get("suite")
        .and_then(Value::as_object)
        .ok_or_else(|| CliError::config("套件包缺少 suite 节点"))?;
    let suite_name = suite
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .ok_or_else(|| CliError::config("套件包缺少 suite.name"))?;
    let on_conflict = args.value("on-conflict").unwrap_or("rename");
    if !matches!(on_conflict, "skip" | "overwrite" | "rename") {
        return Err(CliError::config(
            "--on-conflict 仅支持 skip / overwrite / rename",
        ));
    }

    // 套件包带 Python 脚本等同于引入任意代码：未授权时直接拒绝导入（文档 8.6）
    let mut scripts: Vec<String> = Vec::new();
    for key in ["setupScript", "teardownScript"] {
        if text_field(suite, key).is_some_and(|text| !text.trim().is_empty()) {
            scripts.push(format!("suite.{key}"));
        }
    }
    if let Some(cases) = pack.get("cases").and_then(Value::as_array) {
        for (index, item) in cases.iter().enumerate() {
            let Some(map) = item.as_object() else {
                continue;
            };
            let name = text_field(map, "name").unwrap_or_else(|| format!("#{}", index + 1));
            for key in ["preScript", "postScript", "mainScript"] {
                if text_field(map, key).is_some_and(|text| !text.trim().is_empty()) {
                    scripts.push(format!("{name}.{key}"));
                }
            }
        }
    }
    if !scripts.is_empty() {
        if !state.settings().python_enabled {
            return Err(CliError::config(format!(
                "套件包包含 {} 个 Python 脚本，但本机未开启「允许执行本地 Python 脚本」，已拒绝导入\n脚本清单：{}",
                scripts.len(),
                scripts.join("、")
            )));
        }
        eprintln!(
            "注意：套件包包含 {} 个 Python 脚本，导入后将随套件执行：{}",
            scripts.len(),
            scripts.join("、")
        );
    }

    let conn = state
        .pool
        .get()
        .map_err(|error| CliError::db(crate::util::err(error)))?;
    let project_id = match resolve_project(&conn, args.value("project")) {
        Ok(Some(id)) => id,
        Ok(None) => ensure_project(&conn, "导入套件")?,
        Err(_) => ensure_project(&conn, args.value("project").unwrap_or("导入套件"))?,
    };
    // 同名冲突处理
    let existing: Option<i64> = conn
        .query_row(
            "SELECT id FROM test_suites WHERE project_id = ?1 AND name = ?2 ORDER BY id LIMIT 1",
            params![project_id, suite_name],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| CliError::config(crate::util::err(error)))?;
    let final_name = match (existing, on_conflict) {
        (None, _) => suite_name.to_string(),
        (Some(_), "skip") => {
            println!("套件「{suite_name}」已存在，按 --on-conflict skip 跳过");
            return Ok(EXIT_OK);
        }
        (Some(id), "overwrite") => {
            conn.execute("DELETE FROM test_suites WHERE id = ?1", params![id])
                .map_err(|error| CliError::config(crate::util::err(error)))?;
            suite_name.to_string()
        }
        (Some(_), _) => unique_name(&conn, project_id, suite_name),
    };

    let suite_id = insert_pack(&conn, project_id, &final_name, suite, &pack)?;
    println!("已导入套件 #{suite_id} {final_name}");
    Ok(EXIT_OK)
}

fn ensure_project(conn: &Connection, name: &str) -> CliResult<i64> {
    let name = name.trim();
    let existing: Option<i64> = conn
        .query_row(
            "SELECT id FROM projects WHERE name = ?1 ORDER BY id LIMIT 1",
            params![name],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| CliError::config(crate::util::err(error)))?;
    if let Some(id) = existing {
        return Ok(id);
    }
    let now = crate::util::now_ms();
    conn.execute(
        "INSERT INTO projects (name, created_at, updated_at) VALUES (?1, ?2, ?2)",
        params![name, now],
    )
    .map_err(|error| CliError::config(crate::util::err(error)))?;
    Ok(conn.last_insert_rowid())
}

fn unique_name(conn: &Connection, project_id: i64, base: &str) -> String {
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let mut candidate = format!("{base}-{stamp}");
    let mut index = 1;
    while conn
        .query_row(
            "SELECT COUNT(*) FROM test_suites WHERE project_id = ?1 AND name = ?2",
            params![project_id, candidate],
            |row| Ok(row.get::<_, i64>(0)? > 0),
        )
        .unwrap_or(false)
    {
        index += 1;
        candidate = format!("{base}-{stamp}-{index}");
    }
    candidate
}

fn text_field(map: &serde_json::Map<String, Value>, key: &str) -> Option<String> {
    map.get(key).and_then(Value::as_str).map(str::to_string)
}

fn insert_pack(
    conn: &Connection,
    project_id: i64,
    name: &str,
    suite: &serde_json::Map<String, Value>,
    pack: &Value,
) -> CliResult<i64> {
    let now = crate::util::now_ms();
    let tags = suite.get("tags").and_then(Value::as_array).map(|items| {
        items
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(",")
    });
    let config = suite
        .get("config")
        .map(|value| value.to_string())
        .filter(|text| text != "null");
    conn.execute(
        "INSERT INTO test_suites
            (project_id, name, tags, config_json, setup_script, teardown_script, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
        params![
            project_id,
            name,
            tags,
            config,
            text_field(suite, "setupScript"),
            text_field(suite, "teardownScript"),
            now
        ],
    )
    .map_err(|error| CliError::config(crate::util::err(error)))?;
    let suite_id = conn.last_insert_rowid();

    // 数据集：先导入，用例按 name 引用
    let mut dataset_ids: HashMap<String, i64> = HashMap::new();
    if let Some(items) = pack.get("datasets").and_then(Value::as_array) {
        for item in items {
            let Some(map) = item.as_object() else {
                continue;
            };
            let Some(dataset_name) = text_field(map, "name") else {
                continue;
            };
            let format = text_field(map, "format").unwrap_or_else(|| "json".into());
            let content = map
                .get("inline")
                .map(|value| value.to_string())
                .or_else(|| text_field(map, "content"));
            let path = text_field(map, "path");
            let rows = map
                .get("inline")
                .and_then(Value::as_array)
                .map(|items| items.len() as i64)
                .unwrap_or(0);
            conn.execute(
                "INSERT INTO test_datasets
                    (suite_id, name, format, path, content, row_count, sha256, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    suite_id,
                    dataset_name,
                    format,
                    path,
                    content,
                    rows,
                    text_field(map, "sha256"),
                    now
                ],
            )
            .map_err(|error| CliError::config(crate::util::err(error)))?;
            dataset_ids.insert(dataset_name.clone(), conn.last_insert_rowid());
        }
    }

    // request 用例的快照落到项目下的集合里，保证不依赖目标机器接口库
    let collection_id = ensure_collection(conn, project_id, name)?;

    let mut order: i64 = 0;
    if let Some(cases) = pack.get("cases").and_then(Value::as_array) {
        for item in cases {
            let Some(map) = item.as_object() else {
                continue;
            };
            order += 1;
            let case_name = text_field(map, "name").unwrap_or_else(|| format!("用例 {order}"));
            let kind = text_field(map, "kind").unwrap_or_else(|| "request".into());
            let timeout_ms = map
                .get("timeoutMs")
                .and_then(Value::as_i64)
                .unwrap_or(30_000);
            let retries = map.get("retries").and_then(Value::as_i64).unwrap_or(0);
            let depends = map.get("dependsOn").and_then(Value::as_array).map(|items| {
                items
                    .iter()
                    .filter_map(|value| value.as_str().map(str::to_string))
                    .collect::<Vec<_>>()
                    .join(",")
            });
            let dataset_id =
                text_field(map, "dataset").and_then(|key| dataset_ids.get(&key).copied());
            let assertions = map.get("assertions").map(|value| value.to_string());
            let request_id = if kind == "request" {
                match map.get("request").and_then(Value::as_object) {
                    Some(request) => Some(insert_pack_request(
                        conn,
                        collection_id,
                        &case_name,
                        request,
                        order,
                    )?),
                    None => None,
                }
            } else {
                None
            };
            let stress_json = map.get("stress").map(|value| value.to_string());
            conn.execute(
                "INSERT INTO test_cases
                    (suite_id, name, kind, request_id, dataset_id, depends_on, sort_order, enabled,
                     timeout_ms, retries, tags, variables_json, assertions_json, pre_script,
                     post_script, main_script, stress_json, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 1, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?17)",
                params![
                    suite_id,
                    case_name,
                    kind,
                    request_id,
                    dataset_id,
                    depends,
                    order,
                    timeout_ms,
                    retries,
                    map.get("tags")
                        .and_then(Value::as_array)
                        .map(|items| items
                            .iter()
                            .filter_map(Value::as_str)
                            .collect::<Vec<_>>()
                            .join(",")),
                    map.get("variables").map(|value| value.to_string()),
                    assertions,
                    text_field(map, "preScript"),
                    text_field(map, "postScript"),
                    text_field(map, "mainScript"),
                    stress_json,
                    now
                ],
            )
            .map_err(|error| CliError::config(crate::util::err(error)))?;
        }
    }
    Ok(suite_id)
}

fn ensure_collection(conn: &Connection, project_id: i64, name: &str) -> CliResult<i64> {
    let collection_name = format!("套件包 · {name}");
    let existing: Option<i64> = conn
        .query_row(
            "SELECT id FROM collections WHERE project_id = ?1 AND name = ?2 ORDER BY id LIMIT 1",
            params![project_id, collection_name],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| CliError::config(crate::util::err(error)))?;
    if let Some(id) = existing {
        return Ok(id);
    }
    let now = crate::util::now_ms();
    conn.execute(
        "INSERT INTO collections (project_id, name, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?3)",
        params![project_id, collection_name, now],
    )
    .map_err(|error| CliError::config(crate::util::err(error)))?;
    Ok(conn.last_insert_rowid())
}

fn insert_pack_request(
    conn: &Connection,
    collection_id: i64,
    name: &str,
    request: &serde_json::Map<String, Value>,
    order: i64,
) -> CliResult<i64> {
    let now = crate::util::now_ms();
    let headers = request.get("headers").map(|value| match value {
        Value::Object(map) => {
            let items: Vec<Value> = map
                .iter()
                .map(|(key, value)| json!({ "key": key, "value": value_to_text(value.clone()), "enabled": true }))
                .collect();
            Value::Array(items)
        }
        other => other.clone(),
    });
    conn.execute(
        "INSERT INTO requests
            (collection_id, name, method, url, headers, params, body_type, body,
             pre_script, post_script, assertions, auth, sort_order, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, '', '', '', ?9, ?10, ?11, ?11)",
        params![
            collection_id,
            name,
            text_field(request, "method").unwrap_or_else(|| "GET".into()),
            text_field(request, "url").unwrap_or_default(),
            headers.map(|value| value.to_string()),
            request.get("params").map(|value| value.to_string()),
            text_field(request, "bodyType").unwrap_or_else(|| "none".into()),
            text_field(request, "body"),
            request.get("auth").map(|value| value.to_string()),
            order,
            now
        ],
    )
    .map_err(|error| CliError::config(crate::util::err(error)))?;
    Ok(conn.last_insert_rowid())
}
