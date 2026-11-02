PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;

-- 应用元信息（键值）：密钥来源、加密版本、上次备份/自检时间等
CREATE TABLE IF NOT EXISTS app_meta (
    key   TEXT PRIMARY KEY,
    value TEXT
);

-- 项目
CREATE TABLE IF NOT EXISTS projects (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    name        TEXT NOT NULL,
    description TEXT,
    color       TEXT,
    sort_order  INTEGER DEFAULT 0,
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL
);

-- 集合（文件夹，可无限嵌套）
CREATE TABLE IF NOT EXISTS collections (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id  INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    parent_id   INTEGER REFERENCES collections(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    sort_order  INTEGER DEFAULT 0,
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL
);

-- 请求
CREATE TABLE IF NOT EXISTS requests (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    collection_id INTEGER NOT NULL REFERENCES collections(id) ON DELETE CASCADE,
    name          TEXT NOT NULL,
    method        TEXT NOT NULL DEFAULT 'GET',
    url           TEXT NOT NULL,
    headers       TEXT,
    params        TEXT,
    body_type     TEXT DEFAULT 'none',
    body          TEXT,
    body_meta     TEXT,
    pre_script    TEXT,
    post_script   TEXT,
    assertions    TEXT,
    auth          TEXT,
    sort_order    INTEGER DEFAULT 0,
    created_at    INTEGER NOT NULL,
    updated_at    INTEGER NOT NULL
);

-- 环境
CREATE TABLE IF NOT EXISTS environments (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id  INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    is_active   INTEGER DEFAULT 0,
    script      TEXT,
    created_at  INTEGER NOT NULL
);

-- 环境变量
CREATE TABLE IF NOT EXISTS env_variables (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    env_id     INTEGER NOT NULL REFERENCES environments(id) ON DELETE CASCADE,
    "key"      TEXT NOT NULL,
    value      TEXT,
    is_secret  INTEGER DEFAULT 0,
    sort_order INTEGER DEFAULT 0
);

-- 压测计划
CREATE TABLE IF NOT EXISTS test_plans (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id      INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    request_id      INTEGER NOT NULL REFERENCES requests(id) ON DELETE CASCADE,
    name            TEXT NOT NULL,
    concurrent      INTEGER DEFAULT 10,
    duration_sec    INTEGER DEFAULT 60,
    ramp_up_sec     INTEGER DEFAULT 10,
    think_time_ms   INTEGER DEFAULT 0,
    timeout_ms      INTEGER DEFAULT 30000,
    params_config   TEXT,
    config_json     TEXT,
    created_at      INTEGER NOT NULL,
    updated_at      INTEGER NOT NULL DEFAULT 0
);

-- 压测报告
CREATE TABLE IF NOT EXISTS test_reports (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    plan_id        INTEGER,
    plan_name      TEXT NOT NULL,
    request_id     INTEGER,
    total_requests INTEGER DEFAULT 0,
    success_count  INTEGER DEFAULT 0,
    error_count    INTEGER DEFAULT 0,
    avg_rt         REAL DEFAULT 0,
    min_rt         REAL DEFAULT 0,
    max_rt         REAL DEFAULT 0,
    p50_rt         REAL DEFAULT 0,
    p75_rt         REAL DEFAULT 0,
    p90_rt         REAL DEFAULT 0,
    p95_rt         REAL DEFAULT 0,
    p99_rt         REAL DEFAULT 0,
    p999_rt        REAL DEFAULT 0,
    stddev_rt      REAL DEFAULT 0,
    ttfb_avg       REAL DEFAULT 0,
    ttfb_p95       REAL DEFAULT 0,
    apdex          REAL DEFAULT 0,
    bytes_total    INTEGER DEFAULT 0,
    max_qps        REAL DEFAULT 0,
    avg_qps        REAL DEFAULT 0,
    target_rps     REAL DEFAULT 0,
    error_rate     REAL DEFAULT 0,
    concurrency    INTEGER DEFAULT 0,
    status         TEXT DEFAULT 'completed',
    config_json    TEXT,
    stages_json    TEXT,
    codes_json     TEXT,
    histogram_json TEXT,
    error_types_json TEXT,
    scenario_text  TEXT,
    summary_text   TEXT,
    start_time     INTEGER,
    end_time       INTEGER,
    created_at     INTEGER NOT NULL
);

-- 报告采样（每秒一条）
CREATE TABLE IF NOT EXISTS report_samples (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    report_id     INTEGER NOT NULL REFERENCES test_reports(id) ON DELETE CASCADE,
    elapsed_sec   INTEGER NOT NULL,
    concurrent    INTEGER DEFAULT 0,
    qps           REAL DEFAULT 0,
    avg_rt        REAL DEFAULT 0,
    p50_rt        REAL DEFAULT 0,
    p95_rt        REAL DEFAULT 0,
    p99_rt        REAL DEFAULT 0,
    ttfb_avg      REAL DEFAULT 0,
    bytes         INTEGER DEFAULT 0,
    success_count INTEGER DEFAULT 0,
    error_count   INTEGER DEFAULT 0
);

-- 报告错误明细（最多保留前 500 条）
CREATE TABLE IF NOT EXISTS report_errors (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    report_id       INTEGER NOT NULL REFERENCES test_reports(id) ON DELETE CASCADE,
    elapsed_sec     INTEGER,
    error_type      TEXT,
    error_message   TEXT,
    request_url     TEXT,
    response_status INTEGER,
    response_body   TEXT
);

-- 报告请求明细（每次请求一条，最多保留前 50000 条）
CREATE TABLE IF NOT EXISTS report_requests (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    report_id     INTEGER NOT NULL REFERENCES test_reports(id) ON DELETE CASCADE,
    elapsed_ms    INTEGER DEFAULT 0,
    time_ms       INTEGER DEFAULT 0,
    method        TEXT,
    url           TEXT,
    status        INTEGER DEFAULT 0,
    latency_ms    INTEGER DEFAULT 0,
    ttfb_ms       INTEGER DEFAULT 0,
    bytes         INTEGER DEFAULT 0,
    success       INTEGER DEFAULT 1,
    body          TEXT,
    params        TEXT,
    req_headers   TEXT,
    res_headers   TEXT,
    res_body      TEXT,
    error_type    TEXT,
    error_message TEXT
);

CREATE INDEX IF NOT EXISTS idx_report_requests_report ON report_requests(report_id, id);

-- 请求历史（本地，最多保留最近 100000 条）
CREATE TABLE IF NOT EXISTS request_history (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    request_id   INTEGER,
    request_name TEXT,
    method       TEXT,
    url          TEXT,
    status_code  INTEGER,
    duration_ms  INTEGER,
    created_at   INTEGER NOT NULL
);

-- 单接口运行记录（每次发送的完整快照，最多保留最近 200 条/接口）
CREATE TABLE IF NOT EXISTS request_runs (
    id                INTEGER PRIMARY KEY AUTOINCREMENT,
    request_id        INTEGER NOT NULL REFERENCES requests(id) ON DELETE CASCADE,
    method            TEXT NOT NULL,
    url               TEXT NOT NULL,
    headers           TEXT,
    params            TEXT,
    body_type         TEXT DEFAULT 'none',
    body              TEXT,
    response_status   INTEGER DEFAULT 0,
    response_status_text TEXT,
    response_headers  TEXT,
    response_body     TEXT,
    duration_ms       INTEGER DEFAULT 0,
    size_bytes        INTEGER DEFAULT 0,
    error             TEXT,
    created_at        INTEGER NOT NULL
);

-- 测试套件（v1.0.8 自动化测试场景）
CREATE TABLE IF NOT EXISTS test_suites (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id      INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name            TEXT NOT NULL,
    description     TEXT,
    env_id          INTEGER REFERENCES environments(id) ON DELETE SET NULL,
    tags            TEXT,
    python_path     TEXT,
    config_json     TEXT,
    setup_script    TEXT,
    teardown_script TEXT,
    created_at      INTEGER NOT NULL,
    updated_at      INTEGER NOT NULL DEFAULT 0
);

-- 测试用例（request / python / stress）
CREATE TABLE IF NOT EXISTS test_cases (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    suite_id            INTEGER NOT NULL REFERENCES test_suites(id) ON DELETE CASCADE,
    name                TEXT NOT NULL,
    kind                TEXT NOT NULL DEFAULT 'request',
    request_id          INTEGER REFERENCES requests(id) ON DELETE SET NULL,
    plan_id             INTEGER REFERENCES test_plans(id) ON DELETE SET NULL,
    dataset_id          INTEGER REFERENCES test_datasets(id) ON DELETE SET NULL,
    depends_on          TEXT,
    sort_order          INTEGER DEFAULT 0,
    enabled             INTEGER DEFAULT 1,
    timeout_ms          INTEGER DEFAULT 30000,
    retries             INTEGER DEFAULT 0,
    retry_on_failure    INTEGER DEFAULT 0,
    continue_on_failure INTEGER DEFAULT 0,
    tags                TEXT,
    variables_json      TEXT,
    assertions_json     TEXT,
    pre_script          TEXT,
    post_script         TEXT,
    main_script         TEXT,
    stress_json         TEXT,
    created_at          INTEGER NOT NULL,
    updated_at          INTEGER NOT NULL DEFAULT 0
);

-- 数据集（CSV / JSON，path 相对 data_dir，内联时 content 优先）
CREATE TABLE IF NOT EXISTS test_datasets (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    suite_id    INTEGER NOT NULL REFERENCES test_suites(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    format      TEXT NOT NULL DEFAULT 'csv',
    path        TEXT,
    content     TEXT,
    row_count   INTEGER DEFAULT 0,
    sha256      TEXT,
    created_at  INTEGER NOT NULL
);

-- 套件运行
CREATE TABLE IF NOT EXISTS test_runs (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    suite_id       INTEGER NOT NULL REFERENCES test_suites(id) ON DELETE CASCADE,
    env_id         INTEGER,
    env_name       TEXT,
    trigger        TEXT NOT NULL DEFAULT 'ui',
    status         TEXT NOT NULL DEFAULT 'queued',
    total          INTEGER DEFAULT 0,
    passed         INTEGER DEFAULT 0,
    failed         INTEGER DEFAULT 0,
    skipped        INTEGER DEFAULT 0,
    errored        INTEGER DEFAULT 0,
    duration_ms    INTEGER DEFAULT 0,
    config_json    TEXT,
    summary_json   TEXT,
    app_version    TEXT,
    python_version TEXT,
    report_dir     TEXT,
    started_at     INTEGER,
    finished_at    INTEGER,
    created_at     INTEGER NOT NULL
);

-- 运行内用例结果
CREATE TABLE IF NOT EXISTS test_run_cases (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    run_id              INTEGER NOT NULL REFERENCES test_runs(id) ON DELETE CASCADE,
    case_id             INTEGER,
    name                TEXT NOT NULL,
    kind                TEXT NOT NULL,
    status              TEXT NOT NULL,
    attempts            INTEGER DEFAULT 1,
    duration_ms         INTEGER DEFAULT 0,
    http_duration_ms    INTEGER DEFAULT 0,
    python_duration_ms  INTEGER DEFAULT 0,
    request_snapshot    TEXT,
    response_snapshot   TEXT,
    assertions_total    INTEGER DEFAULT 0,
    assertions_failed   INTEGER DEFAULT 0,
    error               TEXT,
    logs                TEXT,
    python_version      TEXT,
    script_hash         TEXT,
    report_id           INTEGER,
    started_at          INTEGER,
    finished_at         INTEGER
);

-- 断言明细
CREATE TABLE IF NOT EXISTS test_run_assertions (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    run_case_id  INTEGER NOT NULL REFERENCES test_run_cases(id) ON DELETE CASCADE,
    name         TEXT NOT NULL,
    passed       INTEGER NOT NULL,
    message      TEXT,
    duration_ms  INTEGER DEFAULT 0
);

-- 附件与产物（报告 / 日志 / 脚本输出文件）
CREATE TABLE IF NOT EXISTS test_artifacts (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    run_id       INTEGER NOT NULL REFERENCES test_runs(id) ON DELETE CASCADE,
    run_case_id  INTEGER REFERENCES test_run_cases(id) ON DELETE CASCADE,
    kind         TEXT NOT NULL,
    name         TEXT NOT NULL,
    path         TEXT NOT NULL,
    size_bytes   INTEGER DEFAULT 0,
    created_at   INTEGER NOT NULL
);

-- 工具箱 · 记事本
CREATE TABLE IF NOT EXISTS notes (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id      INTEGER REFERENCES projects(id) ON DELETE SET NULL,
    ref_type        TEXT NOT NULL DEFAULT '',
    ref_id          INTEGER,
    title           TEXT NOT NULL DEFAULT '',
    content         TEXT NOT NULL DEFAULT '',
    kind            TEXT NOT NULL DEFAULT 'note',
    status          TEXT NOT NULL DEFAULT 'open',
    priority        INTEGER NOT NULL DEFAULT 1,
    tags            TEXT NOT NULL DEFAULT '',
    due_at          INTEGER,
    remind_at       INTEGER,
    remind_lead_min INTEGER NOT NULL DEFAULT 0,
    remind_repeat   TEXT NOT NULL DEFAULT 'none',
    snooze_until    INTEGER,
    notified_at     INTEGER,
    pinned          INTEGER NOT NULL DEFAULT 0,
    created_at      INTEGER NOT NULL,
    updated_at      INTEGER NOT NULL
);

-- AI Provider（api_key_enc 为 AES-GCM 密文，见 commands/ai.rs）
CREATE TABLE IF NOT EXISTS ai_providers (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    name          TEXT NOT NULL,
    kind          TEXT NOT NULL DEFAULT 'openai',
    base_url      TEXT NOT NULL,
    model         TEXT NOT NULL,
    api_key_enc   TEXT,
    extra_headers TEXT,
    temperature   REAL DEFAULT 0.3,
    max_tokens    INTEGER DEFAULT 0,
    timeout_secs  INTEGER DEFAULT 300,
    is_default    INTEGER DEFAULT 0,
    enabled       INTEGER DEFAULT 1,
    created_at    INTEGER NOT NULL,
    updated_at    INTEGER NOT NULL
);

-- AI 会话
CREATE TABLE IF NOT EXISTS ai_conversations (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    provider_id  INTEGER,
    scene        TEXT,
    prompt       TEXT,
    response     TEXT,
    token_input  INTEGER DEFAULT 0,
    token_output INTEGER DEFAULT 0,
    created_at   INTEGER NOT NULL
);

-- AI 会话（一次对话 = 一条会话，消息按 id 顺序追加）
CREATE TABLE IF NOT EXISTS ai_sessions (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    title         TEXT NOT NULL,
    provider_id   INTEGER,
    pinned        INTEGER NOT NULL DEFAULT 0,
    summary       TEXT,
    summary_until INTEGER DEFAULT 0,
    created_at    INTEGER NOT NULL,
    updated_at    INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS ai_messages (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id INTEGER NOT NULL,
    role       TEXT NOT NULL,
    content    TEXT NOT NULL,
    reasoning  TEXT,
    token_count INTEGER DEFAULT 0,
    created_at INTEGER NOT NULL
);

-- AI 工具调用流水（每次执行单独记录，供用户在界面上回看）
CREATE TABLE IF NOT EXISTS ai_tool_logs (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id    INTEGER,
    tool          TEXT NOT NULL,
    args          TEXT,
    ok            INTEGER NOT NULL DEFAULT 1,
    duration_ms   INTEGER NOT NULL DEFAULT 0,
    result_preview TEXT,
    round         INTEGER DEFAULT 0,
    trace_id      TEXT,
    timeout       INTEGER DEFAULT 0,
    created_at    INTEGER NOT NULL
);

-- AI 模型调用用量（每次对话一行，用于 token / 成功率统计）
CREATE TABLE IF NOT EXISTS ai_usage_logs (
    id                INTEGER PRIMARY KEY AUTOINCREMENT,
    provider_id       INTEGER,
    provider_name     TEXT,
    model             TEXT,
    prompt_tokens     INTEGER NOT NULL DEFAULT 0,
    completion_tokens INTEGER NOT NULL DEFAULT 0,
    ok                INTEGER NOT NULL DEFAULT 1,
    latency_ms        INTEGER NOT NULL DEFAULT 0,
    error             TEXT,
    request_preview   TEXT,
    created_at        INTEGER NOT NULL
);

-- 本机 Cookie（响应 Set-Cookie 自动写入，发送请求时按域自动携带）
CREATE TABLE IF NOT EXISTS cookies (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    domain     TEXT NOT NULL,
    path       TEXT NOT NULL DEFAULT '/',
    name       TEXT NOT NULL,
    value      TEXT NOT NULL,
    secure     INTEGER DEFAULT 0,
    http_only  INTEGER DEFAULT 0,
    expires_at INTEGER,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE(domain, path, name)
);

-- 索引
CREATE INDEX IF NOT EXISTS idx_collections_project ON collections(project_id, parent_id);
CREATE INDEX IF NOT EXISTS idx_requests_collection ON requests(collection_id);
CREATE INDEX IF NOT EXISTS idx_env_variables_env ON env_variables(env_id);
CREATE INDEX IF NOT EXISTS idx_samples_report ON report_samples(report_id, elapsed_sec);
CREATE INDEX IF NOT EXISTS idx_errors_report ON report_errors(report_id);
CREATE INDEX IF NOT EXISTS idx_history_created ON request_history(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_runs_request ON request_runs(request_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_reports_created ON test_reports(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_cookies_domain ON cookies(domain);
CREATE INDEX IF NOT EXISTS idx_ai_messages_session ON ai_messages(session_id, id);
CREATE INDEX IF NOT EXISTS idx_ai_tool_logs_session ON ai_tool_logs(session_id, id);
CREATE INDEX IF NOT EXISTS idx_ai_usage_provider ON ai_usage_logs(provider_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_test_cases_suite ON test_cases(suite_id, sort_order);
CREATE INDEX IF NOT EXISTS idx_suites_project ON test_suites(project_id);
CREATE INDEX IF NOT EXISTS idx_datasets_suite ON test_datasets(suite_id);
CREATE INDEX IF NOT EXISTS idx_runs_suite ON test_runs(suite_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_run_cases_run ON test_run_cases(run_id, id);
CREATE INDEX IF NOT EXISTS idx_run_assertions_case ON test_run_assertions(run_case_id, id);
CREATE INDEX IF NOT EXISTS idx_artifacts_run ON test_artifacts(run_id);
CREATE INDEX IF NOT EXISTS idx_notes_due ON notes(status, remind_at);
CREATE INDEX IF NOT EXISTS idx_notes_ref ON notes(ref_type, ref_id);
CREATE INDEX IF NOT EXISTS idx_notes_project ON notes(project_id, updated_at DESC);
-- idx_plans_project 依赖 updated_at 列，老库需先补列，因此放在 migrate() 中创建
