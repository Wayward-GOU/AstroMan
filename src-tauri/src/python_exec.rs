//! Python 子进程执行器（T-09）
//!
//! 协议见文档 4.3：Rust 侧写 `context.json` 并启动 `python <script>`，脚本写
//! `result.json`。本模块只负责进程生命周期、协议校验与安全边界（输出上限、
//! 附件路径、环境变量白名单），用例语义由 `test_runner` 处理。

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub const PROTOCOL_VERSION: &str = "1.0";
pub const DEFAULT_TIMEOUT_MS: u64 = 60_000;
pub const MAX_OUTPUT_BYTES: usize = 1024 * 1024;
pub const MAX_ATTACHMENT_BYTES: u64 = 10 * 1024 * 1024;
/// result.json 上限：防止脚本写出超大结果拖垮解析与落库
pub const MAX_RESULT_BYTES: u64 = 5 * 1024 * 1024;
pub const MAX_ATTACHMENTS_TOTAL: u64 = 100 * 1024 * 1024;
pub const MIN_PYTHON_MINOR: u64 = 9;
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

// ---------------------------------------------------------------------------
// 解释器探测
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PythonInfo {
    pub program: String,
    pub launcher_args: Vec<String>,
    pub display: String,
    pub version: String,
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PythonCheck {
    /// 是否已显式授权执行本地脚本
    pub enabled: bool,
    pub available: bool,
    pub info: Option<PythonInfo>,
    pub message: String,
    /// 探测失败的候选命令，便于用户排查
    pub tried: Vec<String>,
    /// 随包 `astroman_runtime.py` 的路径，未打包时为 null
    pub runtime_path: Option<String>,
}

/// 按「设置项 → python3 → python → py -3」顺序探测解释器。
///
/// 设置项为空时走自动探测；设置项无效时继续尝试默认候选，避免一个错误路径
/// 让整个功能不可用。返回值同时带 `enabled`，由 UI 决定是提示授权还是提示安装。
pub fn check(configured: &str, enabled: bool) -> PythonCheck {
    let mut tried = Vec::new();
    let mut too_old = Vec::new();
    let mut found: Option<PythonInfo> = None;

    for (program, args) in candidates(configured) {
        let display = display_command(&program, &args);
        match probe(&program, &args) {
            Some((major, minor, patch)) => {
                if major == 3 && minor >= MIN_PYTHON_MINOR {
                    found = Some(PythonInfo {
                        program,
                        launcher_args: args,
                        display,
                        version: format!("{major}.{minor}.{patch}"),
                        major,
                        minor,
                        patch,
                    });
                    break;
                }
                too_old.push(format!(
                    "{display}（Python {major}.{minor}.{patch}，要求 ≥ 3.9）"
                ));
            }
            None => tried.push(display),
        }
    }

    let available = found.is_some();
    let message = match (&found, enabled) {
        (Some(info), true) => format!("Python {} 已就绪（{}）", info.version, info.display),
        (Some(info), false) => format!(
            "已找到 Python {}（{}），但尚未授权执行本地 Python 脚本",
            info.version, info.display
        ),
        (None, _) if !too_old.is_empty() => format!(
            "Python 版本过低：{}；请安装 Python ≥ 3.9 或在设置中填写解释器路径",
            too_old.join("，")
        ),
        (None, _) => {
            let suffix = if tried.is_empty() {
                String::new()
            } else {
                format!("（已尝试：{}）", tried.join("，"))
            };
            format!(
                "未找到可用的 Python 解释器{suffix}；请安装 Python ≥ 3.9 或在设置中填写解释器路径"
            )
        }
    };

    PythonCheck {
        enabled,
        available,
        info: found,
        message,
        tried,
        runtime_path: runtime_module_path().map(|path| path.display().to_string()),
    }
}

fn candidates(configured: &str) -> Vec<(String, Vec<String>)> {
    let mut out: Vec<(String, Vec<String>)> = Vec::new();
    let mut push = |program: &str, args: &[&str]| {
        let entry = (
            program.to_string(),
            args.iter().map(|item| item.to_string()).collect(),
        );
        if !out.iter().any(|(p, a)| *p == entry.0 && *a == entry.1) {
            out.push(entry);
        }
    };

    let configured = configured.trim();
    if !configured.is_empty() {
        push(configured, &[]);
    }
    push("python3", &[]);
    push("python", &[]);
    #[cfg(windows)]
    push("py", &["-3"]);

    out
}

fn display_command(program: &str, args: &[String]) -> String {
    if args.is_empty() {
        program.to_string()
    } else {
        format!("{} {}", program, args.join(" "))
    }
}

fn probe(program: &str, args: &[String]) -> Option<(u64, u64, u64)> {
    let mut command = Command::new(program);
    command.args(args).arg("--version");
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    apply_no_window(&mut command);

    let mut child = command.spawn().ok()?;
    let deadline = Instant::now() + PROBE_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }

    let mut text = String::new();
    if let Some(mut stdout) = child.stdout.take() {
        let _ = stdout.read_to_string(&mut text);
    }
    if let Some(mut stderr) = child.stderr.take() {
        let mut extra = String::new();
        let _ = stderr.read_to_string(&mut extra);
        text.push_str(&extra);
    }
    parse_version(&text)
}

fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    let start = text.find("Python ")? + "Python ".len();
    let token: String = text[start..]
        .chars()
        .take_while(|ch| ch.is_ascii_digit() || *ch == '.')
        .collect();
    let mut parts = token.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next().and_then(|item| item.parse().ok()).unwrap_or(0);
    Some((major, minor, patch))
}

/// 随包 `astroman_runtime.py` 的位置：开发态取仓库资源目录，安装态取 exe 附近。
pub fn runtime_module_path() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("ASTROMAN_RUNTIME_DIR") {
        let candidate = PathBuf::from(dir).join("astroman_runtime.py");
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    let mut candidates = Vec::new();
    candidates.push(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("resources")
            .join("python")
            .join("astroman_runtime.py"),
    );
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(
                dir.join("resources")
                    .join("python")
                    .join("astroman_runtime.py"),
            );
            candidates.push(dir.join("python").join("astroman_runtime.py"));
            candidates.push(
                dir.join("..")
                    .join("Resources")
                    .join("python")
                    .join("astroman_runtime.py"),
            );
        }
    }
    candidates.into_iter().find(|path| path.is_file())
}

// ---------------------------------------------------------------------------
// 执行
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct PythonSpec {
    pub python: PythonInfo,
    pub script: String,
    pub work_dir: PathBuf,
    pub run_id: i64,
    pub suite_id: i64,
    pub case_id: i64,
    /// setup / pre / post / main / teardown
    pub phase: String,
    pub attempt: i64,
    pub environment_id: Option<i64>,
    pub environment_name: Option<String>,
    pub environment_vars: HashMap<String, String>,
    pub variables: HashMap<String, Value>,
    pub dataset: Option<Value>,
    pub request: Option<Value>,
    pub response: Option<Value>,
    pub assertions: Vec<Value>,
    pub timeout: Duration,
    /// 传给子进程的业务变量（仅合法环境变量名，且不覆盖系统关键变量）
    pub business_env: HashMap<String, String>,
    pub cancel: Option<Arc<AtomicBool>>,
    /// 本次用例可用的附件总预算（单次运行 100MB 由调用方递减）
    pub attachment_budget: u64,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PythonResult {
    pub ok: Option<bool>,
    pub variables: HashMap<String, Value>,
    pub request: Option<PythonRequestOverride>,
    pub assertions: Vec<PythonAssertion>,
    pub logs: Vec<PythonLog>,
    pub attachments: Vec<PythonAttachment>,
    pub skip: bool,
    pub fail_fast: bool,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PythonRequestOverride {
    pub method: Option<String>,
    pub url: Option<String>,
    pub headers: Option<Value>,
    pub params: Option<Value>,
    pub body_type: Option<String>,
    /// 允许字符串，也允许直接返回 JSON 对象（会序列化成文本）
    pub body: Option<Value>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PythonAssertion {
    pub name: String,
    pub passed: bool,
    pub message: String,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PythonLog {
    pub level: String,
    pub message: String,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PythonAttachment {
    pub name: String,
    pub path: String,
    pub mime: String,
    /// 解析后的字节数（由 Rust 侧补齐）
    pub size: Option<u64>,
    /// 相对 workDir 的路径，落库与报告使用
    pub relative_path: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PythonOutcome {
    pub ok: bool,
    pub timed_out: bool,
    pub cancelled: bool,
    pub exit_code: Option<i32>,
    pub duration_ms: i64,
    pub python_version: String,
    pub script_hash: String,
    pub stdout: String,
    pub stderr: String,
    pub output_truncated: bool,
    pub result: Option<PythonResult>,
    pub error: Option<String>,
}

/// 同步执行一个 Python 脚本（调用方负责放到阻塞线程）。
pub fn run(spec: PythonSpec) -> PythonOutcome {
    let started = Instant::now();
    let mut outcome = PythonOutcome {
        python_version: spec.python.version.clone(),
        script_hash: script_hash(&spec.script),
        ..PythonOutcome::default()
    };

    let work_dir = match spec.work_dir.canonicalize() {
        Ok(path) => path,
        Err(error) => {
            outcome.error = Some(format!("PYTHON_WORKDIR：工作目录不可用：{error}"));
            return outcome;
        }
    };
    let scripts_dir = work_dir.join("scripts");
    if let Err(error) = std::fs::create_dir_all(&scripts_dir) {
        outcome.error = Some(format!("PYTHON_WORKDIR：创建工作目录失败：{error}"));
        return outcome;
    }

    let script_path = scripts_dir.join(format!("{}.py", sanitize_phase(&spec.phase)));
    if let Err(error) = std::fs::write(&script_path, spec.script.as_bytes()) {
        outcome.error = Some(format!("PYTHON_SCRIPT：写入脚本失败：{error}"));
        return outcome;
    }
    let context_file = work_dir.join("context.json");
    let result_file = work_dir.join("result.json");
    let context = build_context(&spec, &work_dir, &context_file, &result_file);
    if let Err(error) = std::fs::write(
        &context_file,
        serde_json::to_string_pretty(&context).unwrap_or_default(),
    ) {
        outcome.error = Some(format!("PYTHON_CONTEXT：写入 context.json 失败：{error}"));
        return outcome;
    }
    let _ = std::fs::remove_file(&result_file);

    let mut command = Command::new(&spec.python.program);
    command.args(&spec.python.launcher_args);
    command.arg(&script_path);
    command.current_dir(&work_dir);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    apply_environment(&mut command, &spec, &work_dir, &context_file, &result_file);
    apply_no_window(&mut command);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // 独立进程组，超时/取消时能整组回收，避免派生进程变孤儿
        unsafe {
            command.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
    }

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            outcome.error = Some(format!(
                "PYTHON_SPAWN_FAILED：无法启动 {}（{error}）",
                spec.python.display
            ));
            outcome.duration_ms = started.elapsed().as_millis() as i64;
            return outcome;
        }
    };

    let stdout_pipe = child.stdout.take();
    let stderr_pipe = child.stderr.take();
    let stdout_reader =
        stdout_pipe.map(|pipe| std::thread::spawn(move || read_capped(pipe, MAX_OUTPUT_BYTES)));
    let stderr_reader =
        stderr_pipe.map(|pipe| std::thread::spawn(move || read_capped(pipe, MAX_OUTPUT_BYTES)));

    let deadline = started + spec.timeout;
    let mut status = None;
    loop {
        if spec
            .cancel
            .as_ref()
            .map(|flag| flag.load(Ordering::SeqCst))
            .unwrap_or(false)
        {
            outcome.cancelled = true;
            kill_tree(&mut child);
            let _ = child.wait();
            break;
        }
        match child.try_wait() {
            Ok(Some(exit)) => {
                status = Some(exit);
                break;
            }
            Ok(None) => {
                if Instant::now() >= deadline {
                    outcome.timed_out = true;
                    kill_tree(&mut child);
                    let _ = child.wait();
                    break;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(error) => {
                outcome.error = Some(format!("PYTHON_WAIT：等待进程失败：{error}"));
                kill_tree(&mut child);
                let _ = child.wait();
                break;
            }
        }
    }

    let (stdout, stdout_truncated) = stdout_reader
        .and_then(|handle| handle.join().ok())
        .unwrap_or_default();
    let (stderr, stderr_truncated) = stderr_reader
        .and_then(|handle| handle.join().ok())
        .unwrap_or_default();
    outcome.stdout = stdout;
    outcome.stderr = stderr;
    outcome.output_truncated = stdout_truncated || stderr_truncated;
    outcome.duration_ms = started.elapsed().as_millis() as i64;
    outcome.exit_code = status.and_then(|exit| exit.code());

    if outcome.cancelled {
        outcome.error = Some("运行已取消".to_string());
        return outcome;
    }
    if outcome.timed_out {
        outcome.error = Some(format!(
            "PYTHON_TIMEOUT：脚本超过 {} ms 未结束，已结束进程树",
            spec.timeout.as_millis()
        ));
        return outcome;
    }
    if outcome.error.is_some() {
        return outcome;
    }
    if let Some(exit) = status {
        if !exit.success() {
            let mut message = match exit.code() {
                Some(code) => format!("PYTHON_EXIT：脚本退出码 {code}"),
                None => "PYTHON_EXIT：脚本被信号终止".to_string(),
            };
            if !outcome.stderr.trim().is_empty() {
                message.push_str(&format!("；stderr：{}", tail(&outcome.stderr, 400)));
            }
            outcome.error = Some(message);
            outcome.result = read_result(&result_file).ok();
            return outcome;
        }
    }

    let mut result = match read_result(&result_file) {
        Ok(result) => result,
        Err(message) => {
            outcome.error = Some(message);
            return outcome;
        }
    };

    if let Err(message) =
        resolve_attachments(&mut result.attachments, &work_dir, spec.attachment_budget)
    {
        outcome.error = Some(format!("PYTHON_ATTACHMENT_INVALID：{message}"));
        outcome.result = Some(result);
        return outcome;
    }

    match result.ok {
        Some(true) => outcome.ok = true,
        Some(false) => {
            let message = result
                .logs
                .iter()
                .rev()
                .find(|item| item.level.eq_ignore_ascii_case("error"))
                .map(|item| item.message.clone())
                .unwrap_or_else(|| "脚本返回 ok=false".to_string());
            outcome.error = Some(message);
        }
        None => {
            outcome.error =
                Some("PYTHON_RESULT_INVALID：result.json 缺少 ok 字段（布尔值）".to_string());
        }
    }
    outcome.result = Some(result);
    outcome
}

fn build_context(
    spec: &PythonSpec,
    work_dir: &Path,
    context_file: &Path,
    result_file: &Path,
) -> Value {
    let environment = json!({
        "id": spec.environment_id,
        "name": spec.environment_name,
        "vars": spec.environment_vars,
    });
    let limits = json!({
        "timeoutMs": spec.timeout.as_millis() as u64,
        "maxOutputBytes": MAX_OUTPUT_BYTES,
        "maxAttachmentBytes": MAX_ATTACHMENT_BYTES,
    });
    let paths = json!({
        "workDir": work_dir.display().to_string(),
        "contextFile": context_file.display().to_string(),
        "resultFile": result_file.display().to_string(),
    });
    json!({
        "protocolVersion": PROTOCOL_VERSION,
        "runId": spec.run_id,
        "suiteId": spec.suite_id,
        "caseId": spec.case_id,
        "phase": spec.phase,
        "attempt": spec.attempt,
        "environment": environment,
        "variables": spec.variables,
        "dataset": spec.dataset,
        "request": spec.request,
        "response": spec.response,
        "assertions": spec.assertions,
        "limits": limits,
        "paths": paths,
    })
}

fn read_result(path: &Path) -> Result<PythonResult, String> {
    // 结果文件同样有大小上限：脚本可能被篡改或误写超大内容
    if let Ok(metadata) = std::fs::metadata(path) {
        if metadata.len() > MAX_RESULT_BYTES {
            return Err(format!(
                "PYTHON_RESULT_TOO_LARGE：result.json 超过 {} MB 上限",
                MAX_RESULT_BYTES / (1024 * 1024)
            ));
        }
    }
    let text = std::fs::read_to_string(path).map_err(|_| {
        "PYTHON_RESULT_MISSING：脚本未写出 result.json（可用 astroman_runtime.emit() 写入）"
            .to_string()
    })?;
    let text = text.trim_start_matches('\u{feff}');
    serde_json::from_str::<PythonResult>(text)
        .map_err(|error| format!("PYTHON_RESULT_INVALID：result.json 不是合法结果：{error}"))
}

fn resolve_attachments(
    attachments: &mut [PythonAttachment],
    work_dir: &Path,
    budget: u64,
) -> Result<(), String> {
    let mut used = 0u64;
    for item in attachments.iter_mut() {
        let raw = item.path.trim();
        if raw.is_empty() {
            return Err(format!("附件 {} 缺少 path", display_name(item)));
        }
        let candidate = if Path::new(raw).is_absolute() {
            PathBuf::from(raw)
        } else {
            work_dir.join(raw)
        };
        let resolved = candidate
            .canonicalize()
            .map_err(|error| format!("附件 {} 不存在或不可读：{error}", display_name(item)))?;
        if !resolved.starts_with(work_dir) {
            return Err(format!(
                "附件 {} 路径越界（必须位于工作目录内）：{}",
                display_name(item),
                resolved.display()
            ));
        }
        let metadata = std::fs::metadata(&resolved)
            .map_err(|error| format!("附件 {} 无法读取：{error}", display_name(item)))?;
        if !metadata.is_file() {
            return Err(format!("附件 {} 不是文件", display_name(item)));
        }
        if metadata.len() > MAX_ATTACHMENT_BYTES {
            return Err(format!(
                "附件 {} 超过单文件上限 {}MB",
                display_name(item),
                MAX_ATTACHMENT_BYTES / 1024 / 1024
            ));
        }
        used = used.saturating_add(metadata.len());
        if used > budget {
            return Err(format!(
                "本次运行附件总量超过 {}MB 上限",
                budget / 1024 / 1024
            ));
        }
        if item.name.trim().is_empty() {
            item.name = resolved
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_else(|| "attachment".to_string());
        }
        if item.mime.trim().is_empty() {
            item.mime = guess_mime(&item.name).to_string();
        }
        item.relative_path = resolved
            .strip_prefix(work_dir)
            .ok()
            .map(|path| path.to_string_lossy().replace('\\', "/"));
        item.path = resolved.display().to_string();
        item.size = Some(metadata.len());
    }
    Ok(())
}

fn display_name(item: &PythonAttachment) -> String {
    if item.name.trim().is_empty() {
        item.path.clone()
    } else {
        item.name.clone()
    }
}

fn guess_mime(name: &str) -> &'static str {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".json") {
        "application/json"
    } else if lower.ends_with(".csv") {
        "text/csv"
    } else if lower.ends_with(".html") || lower.ends_with(".htm") {
        "text/html"
    } else if lower.ends_with(".xml") {
        "application/xml"
    } else if lower.ends_with(".png") {
        "image/png"
    } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        "image/jpeg"
    } else if lower.ends_with(".pdf") {
        "application/pdf"
    } else if lower.ends_with(".log") || lower.ends_with(".txt") {
        "text/plain"
    } else {
        "application/octet-stream"
    }
}

fn tail(text: &str, limit: usize) -> String {
    let count = text.chars().count();
    if count <= limit {
        return text.trim().to_string();
    }
    let skip = count - limit;
    format!("…{}", text.chars().skip(skip).collect::<String>())
}

fn sanitize_phase(phase: &str) -> String {
    let cleaned: String = phase
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
        .collect();
    if cleaned.is_empty() {
        "case".to_string()
    } else {
        cleaned
    }
}

pub fn script_hash(script: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(script.as_bytes());
    let digest = hasher.finalize();
    let mut out = String::with_capacity(7 + digest.len() * 2);
    out.push_str("sha256:");
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

// ---------------------------------------------------------------------------
// 进程与环境
// ---------------------------------------------------------------------------

fn read_capped<R: Read>(mut reader: R, limit: usize) -> (String, bool) {
    let mut buffer = Vec::with_capacity(8192);
    let mut chunk = [0u8; 8192];
    let mut truncated = false;
    loop {
        match reader.read(&mut chunk) {
            Ok(0) => break,
            Ok(read) => {
                if buffer.len() < limit {
                    let remain = limit - buffer.len();
                    buffer.extend_from_slice(&chunk[..read.min(remain)]);
                }
                if buffer.len() >= limit {
                    truncated = true;
                }
            }
            Err(_) => break,
        }
    }
    (String::from_utf8_lossy(&buffer).into_owned(), truncated)
}

fn kill_tree(child: &mut Child) {
    let pid = child.id();
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        use std::os::windows::process::CommandExt;
        let mut command = Command::new("taskkill");
        command
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW);
        let _ = command.status();
    }
    #[cfg(unix)]
    {
        // 子进程已通过 setsid 成为进程组组长，负号表示整组信号
        unsafe {
            libc::kill(-(pid as i32), libc::SIGKILL);
        }
    }
    let _ = child.kill();
}

fn apply_no_window(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    {
        let _ = command;
    }
}

fn apply_environment(
    command: &mut Command,
    spec: &PythonSpec,
    work_dir: &Path,
    context_file: &Path,
    result_file: &Path,
) {
    command.env_clear();
    for (key, value) in std::env::vars_os() {
        let name = key.to_string_lossy();
        if allowed_system_env(&name) {
            command.env(&key, value);
        }
    }
    for (key, value) in &spec.business_env {
        if valid_env_name(key) && !reserved_env(key) {
            command.env(key, value);
        }
    }

    command.env("PYTHONUTF8", "1");
    command.env("PYTHONIOENCODING", "utf-8");
    command.env("PYTHONDONTWRITEBYTECODE", "1");
    if let Some(module) = runtime_module_path() {
        if let Some(dir) = module.parent() {
            command.env("PYTHONPATH", dir);
        }
    }
    command.env("ASTROMAN_PROTOCOL_VERSION", PROTOCOL_VERSION);
    command.env("ASTROMAN_CONTEXT", context_file);
    command.env("ASTROMAN_RESULT", result_file);
    command.env("ASTROMAN_WORKDIR", work_dir);
    command.env("ASTROMAN_PHASE", &spec.phase);
    command.env("ASTROMAN_RUN_ID", spec.run_id.to_string());
    command.env("ASTROMAN_SUITE_ID", spec.suite_id.to_string());
    command.env("ASTROMAN_CASE_ID", spec.case_id.to_string());
}

fn allowed_system_env(name: &str) -> bool {
    const ALLOW: [&str; 24] = [
        "PATH",
        "HOME",
        "LANG",
        "LC_ALL",
        "LC_CTYPE",
        "TMPDIR",
        "TEMP",
        "TMP",
        "SYSTEMROOT",
        "WINDIR",
        "USERPROFILE",
        "APPDATA",
        "LOCALAPPDATA",
        "PROGRAMDATA",
        "PROGRAMFILES",
        "PROGRAMFILES(X86)",
        "COMSPEC",
        "PATHEXT",
        "USERNAME",
        "USER",
        "SHELL",
        "TERM",
        "SYSTEMDRIVE",
        "HOMEDRIVE",
    ];
    ALLOW.iter().any(|item| name.eq_ignore_ascii_case(item))
        || name.eq_ignore_ascii_case("HOMEPATH")
}

fn reserved_env(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    allowed_system_env(name)
        || upper.starts_with("ASTROMAN_")
        || matches!(
            upper.as_str(),
            "PYTHONPATH"
                | "PYTHONHOME"
                | "PYTHONSTARTUP"
                | "PYTHONEXECUTABLE"
                | "PYTHONUTF8"
                | "PYTHONIOENCODING"
        )
}

fn valid_env_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(ch) if ch.is_ascii_alphabetic() || ch == '_' => {}
        _ => return false,
    }
    chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::SystemTime;

    fn temp_dir(tag: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|value| value.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!(
            "astroman-python-{tag}-{}-{stamp}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn python_or_skip() -> Option<PythonInfo> {
        let check = check("", true);
        check.info
    }

    fn spec(info: &PythonInfo, dir: &Path, script: &str) -> PythonSpec {
        PythonSpec {
            python: info.clone(),
            script: script.to_string(),
            work_dir: dir.to_path_buf(),
            run_id: 1,
            suite_id: 2,
            case_id: 3,
            phase: "main".to_string(),
            attempt: 1,
            environment_id: None,
            environment_name: Some("test".to_string()),
            environment_vars: HashMap::new(),
            variables: HashMap::new(),
            dataset: None,
            request: None,
            response: None,
            assertions: Vec::new(),
            timeout: Duration::from_secs(10),
            business_env: HashMap::new(),
            cancel: None,
            attachment_budget: MAX_ATTACHMENTS_TOTAL,
        }
    }

    #[test]
    fn detects_python_and_reports_version() {
        let Some(info) = python_or_skip() else {
            return;
        };
        assert!(info.major == 3 && info.minor >= MIN_PYTHON_MINOR);
        assert!(info.version.split('.').count() == 3);
        assert!(!info.display.is_empty());
    }

    #[test]
    fn runs_script_and_reads_result() {
        let Some(info) = python_or_skip() else {
            return;
        };
        let dir = temp_dir("ok");
        let script = r#"
import json, os
import astroman_runtime as ar
ctx = ar.load_context()
ar.assert_eq(ctx["phase"], "main", "phase 为 main")
ar.assert_true(os.environ.get("DEMO_VALUE") == "42", "业务变量已注入")
ar.log("hello")
out = os.path.join(os.environ["ASTROMAN_WORKDIR"], "out.json")
json.dump({"x": 1}, open(out, "w", encoding="utf-8"))
ar.attach("out.json", out)
ar.emit(variables={"orderId": 42})
"#;
        let mut spec = spec(&info, &dir, script);
        spec.business_env
            .insert("DEMO_VALUE".to_string(), "42".to_string());
        let outcome = run(spec);
        assert!(outcome.ok, "{:?} {}", outcome.error, outcome.stderr);
        let result = outcome.result.expect("result");
        assert_eq!(result.variables.get("orderId"), Some(&Value::from(42)));
        assert!(result.assertions.iter().all(|item| item.passed));
        assert_eq!(result.logs[0].message, "hello");
        assert_eq!(result.attachments.len(), 1);
        assert_eq!(
            result.attachments[0].relative_path.as_deref(),
            Some("out.json")
        );
        assert!(result.attachments[0].size.unwrap() > 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn non_zero_exit_is_error() {
        let Some(info) = python_or_skip() else {
            return;
        };
        let dir = temp_dir("exit");
        let outcome = run(spec(&info, &dir, "raise SystemExit(2)\n"));
        assert!(!outcome.ok);
        assert!(outcome.error.unwrap_or_default().contains("退出码 2"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn timeout_kills_process_tree() {
        let Some(info) = python_or_skip() else {
            return;
        };
        let dir = temp_dir("timeout");
        let mut spec = spec(&info, &dir, "while True:\n    pass\n");
        spec.timeout = Duration::from_millis(600);
        let started = Instant::now();
        let outcome = run(spec);
        assert!(outcome.timed_out, "{}", outcome.stderr);
        assert!(outcome.error.unwrap_or_default().contains("PYTHON_TIMEOUT"));
        assert!(started.elapsed() < Duration::from_secs(10));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_attachment_outside_workdir() {
        let Some(info) = python_or_skip() else {
            return;
        };
        let dir = temp_dir("attach");
        let outside = dir.parent().unwrap().join("astroman-outside.txt");
        std::fs::write(&outside, b"x").unwrap();
        // 绕过运行时助手的越界检查，直接写 result.json，验证 Rust 侧仍会拒绝
        let script = format!(
            "import astroman_runtime as ar\nar.emit(attachments=[{{'name': 'bad.txt', 'path': {outside:?}}}])\n"
        );
        let outcome = run(spec(&info, &dir, &script));
        assert!(!outcome.ok);
        let message = outcome.error.unwrap_or_default();
        assert!(message.contains("PYTHON_ATTACHMENT_INVALID"), "{message}");
        assert!(message.contains("越界"), "{message}");
        let _ = std::fs::remove_file(&outside);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn truncates_oversized_output() {
        let Some(info) = python_or_skip() else {
            return;
        };
        let dir = temp_dir("output");
        let script = "\
import astroman_runtime as ar
import sys
sys.stdout.write('x' * (2 * 1024 * 1024))
sys.stdout.flush()
ar.emit()
";
        let outcome = run(spec(&info, &dir, script));
        assert!(outcome.output_truncated);
        assert!(outcome.stdout.len() <= MAX_OUTPUT_BYTES);
        assert!(outcome.ok, "{:?}", outcome.error);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_oversized_result_file() {
        let Some(info) = python_or_skip() else {
            return;
        };
        let dir = temp_dir("result-large");
        // 绕开 emit：直接写一个超过 5MB 的 result.json
        let script = "\
import json, os
path = os.environ['ASTROMAN_RESULT']
with open(path, 'w', encoding='utf-8') as handle:
    handle.write('{\"ok\": true, \"logs\": [{\"level\": \"info\", \"message\": \"' + 'x' * (6 * 1024 * 1024) + '\"}]}')
";
        let outcome = run(spec(&info, &dir, script));
        assert!(!outcome.ok);
        let message = outcome.error.unwrap_or_default();
        assert!(message.contains("PYTHON_RESULT_TOO_LARGE"), "{message}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn script_hash_is_stable() {
        let first = script_hash("print(1)\n");
        let second = script_hash("print(1)\n");
        let other = script_hash("print(2)\n");
        assert_eq!(first, second);
        assert_ne!(first, other);
        assert!(first.starts_with("sha256:"));
        assert_eq!(first.len(), "sha256:".len() + 64);
    }
}
