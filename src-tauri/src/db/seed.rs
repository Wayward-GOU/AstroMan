use crate::models::KeyValueItem;
use crate::util::now_ms;
use rusqlite::{params, Connection, OptionalExtension};

const SAMPLE_PROJECT: &str = "示例项目";
const ENV_NAME: &str = "本地环境";
const FIRST_SCENARIO: &str = "01 基础请求";
const SAMPLE_REQUEST: &str = "示例请求";
const POOL_EXAMPLE_COLLECTION: &str = "07 数据池示例";
const SAMPLE_POOL_NAME: &str = "示例账号池（自定义列）";
const SAMPLE_POOL_PLAN: &str = "示例压测：自定义列数据池";
const SAMPLE_POOL_SEEDED_KEY: &str = "sample_pool_v20_seeded";
const SAMPLE_IMPORT_POOL_NAME: &str = "示例参数池（Excel 模板导入）";
const SAMPLE_IMPORT_PLAN: &str = "示例压测：Excel 导入池（逐请求轮换）";
const SAMPLE_POOL_FLOW_KEY: &str = "sample_pool_flow_v20_seeded";

struct SeedRequest {
    name: &'static str,
    method: &'static str,
    url: &'static str,
    headers: &'static [(&'static str, &'static str)],
    params: &'static [(&'static str, &'static str)],
    body_type: &'static str,
    body: &'static str,
    pre_script: &'static str,
    post_script: &'static str,
}

struct SeedCollection {
    name: &'static str,
    requests: &'static [SeedRequest],
}

const ACCEPT_JSON: &[(&str, &str)] = &[("Accept", "application/json")];

/// 数据池示例请求：演示「全部列同名注入」与「指定列映射」两种绑定
const POOL_EXAMPLE_REQUESTS: &[SeedRequest] = &[
    SeedRequest {
        name: "POST 池账号登录（全部列注入）",
        method: "POST",
        url: "{{base_url}}/post",
        headers: ACCEPT_JSON,
        params: &[],
        body_type: "json",
        body: r#"{
  "phone": "{{phone}}",
  "password": "{{password}}",
  "token": "{{token}}",
  "mappedToken": "{{demo_token}}",
  "note": "phone / password / token 来自示例账号池的同一行"
}"#,
        pre_script: "",
        post_script: "",
    },
    SeedRequest {
        name: "GET 指定列映射",
        method: "GET",
        url: "{{base_url}}/get",
        headers: ACCEPT_JSON,
        params: &[("mapped", "{{demo_token}}"), ("from", "pool")],
        body_type: "none",
        body: "",
        pre_script: "",
        post_script: "",
    },
];

/// 导入流程示例请求：三列都来自「下载模板 → 填表 → 导入」得到的参数池
const POOL_IMPORT_REQUESTS: &[SeedRequest] = &[SeedRequest {
    name: "GET Excel 导入列（同名注入）",
    method: "GET",
    url: "{{base_url}}/get",
    headers: ACCEPT_JSON,
    params: &[
        ("sku", "{{value}}"),
        ("region", "{{region}}"),
        ("level", "{{level}}"),
    ],
    body_type: "none",
    body: "",
    pre_script: "",
    post_script: "",
}];

const SCENARIOS: &[SeedCollection] = &[
    SeedCollection {
        name: "01 基础请求",
        requests: &[
            SeedRequest {
                name: "GET 查询参数",
                method: "GET",
                url: "{{base_url}}/get",
                headers: ACCEPT_JSON,
                params: &[("page", "1"), ("size", "10"), ("keyword", "astroman")],
                body_type: "none",
                body: "",
                pre_script: "",
                post_script: "",
            },
            SeedRequest {
                name: "POST 创建 JSON",
                method: "POST",
                url: "{{base_url}}/post",
                headers: ACCEPT_JSON,
                params: &[],
                body_type: "json",
                body: r#"{
  "name": "AstroMan",
  "owner": "{{username}}",
  "requestId": "{{uuid}}",
  "createdAt": "{{datetime}}"
}"#,
                pre_script: "",
                post_script: "",
            },
            SeedRequest {
                name: "PUT 更新资源",
                method: "PUT",
                url: "{{base_url}}/put",
                headers: ACCEPT_JSON,
                params: &[],
                body_type: "json",
                body: r#"{
  "id": {{user_id}},
  "status": "active",
  "updatedAt": "{{isoTime}}"
}"#,
                pre_script: "",
                post_script: "",
            },
            SeedRequest {
                name: "PATCH 局部更新",
                method: "PATCH",
                url: "{{base_url}}/patch",
                headers: ACCEPT_JSON,
                params: &[],
                body_type: "json",
                body: r#"{
  "nickname": "fink_{{randomString(4, lower)}}"
}"#,
                pre_script: "",
                post_script: "",
            },
            SeedRequest {
                name: "DELETE 删除资源",
                method: "DELETE",
                url: "{{base_url}}/delete",
                headers: ACCEPT_JSON,
                params: &[("id", "{{user_id}}")],
                body_type: "none",
                body: "",
                pre_script: "",
                post_script: "",
            },
            SeedRequest {
                name: "HEAD 探测接口",
                method: "HEAD",
                url: "{{base_url}}/get",
                headers: ACCEPT_JSON,
                params: &[],
                body_type: "none",
                body: "",
                pre_script: "",
                post_script: "",
            },
        ],
    },
    SeedCollection {
        name: "02 认证与请求头",
        requests: &[
            SeedRequest {
                name: "Bearer Token 认证",
                method: "GET",
                url: "{{base_url}}/bearer",
                headers: &[
                    ("Accept", "application/json"),
                    ("Authorization", "Bearer {{token}}"),
                ],
                params: &[],
                body_type: "none",
                body: "",
                pre_script: "",
                post_script: "",
            },
            SeedRequest {
                name: "API Key 认证",
                method: "GET",
                url: "{{base_url}}/headers",
                headers: &[("Accept", "application/json"), ("X-API-Key", "{{api_key}}")],
                params: &[],
                body_type: "none",
                body: "",
                pre_script: "",
                post_script: "",
            },
            SeedRequest {
                name: "自定义请求头",
                method: "GET",
                url: "{{base_url}}/headers",
                headers: &[
                    ("Accept", "application/json"),
                    ("X-Client", "AstroMan/1.0"),
                    ("X-Trace-Id", "{{uuid}}"),
                    ("Accept-Language", "zh-CN"),
                    ("User-Agent", "AstroMan-Agent"),
                ],
                params: &[],
                body_type: "none",
                body: "",
                pre_script: "",
                post_script: "",
            },
            SeedRequest {
                name: "401 未授权",
                method: "GET",
                url: "{{base_url}}/status/401",
                headers: ACCEPT_JSON,
                params: &[],
                body_type: "none",
                body: "",
                pre_script: "",
                post_script: "",
            },
        ],
    },
    SeedCollection {
        name: "03 表单与内容类型",
        requests: &[
            SeedRequest {
                name: "Form 表单提交",
                method: "POST",
                url: "{{base_url}}/post",
                headers: ACCEPT_JSON,
                params: &[],
                body_type: "form",
                body: "name=AstroMan&owner={{username}}&user_id={{user_id}}",
                pre_script: "",
                post_script: "",
            },
            SeedRequest {
                name: "Raw 纯文本",
                method: "POST",
                url: "{{base_url}}/post",
                headers: &[("Content-Type", "text/plain; charset=utf-8")],
                params: &[],
                body_type: "raw",
                body: "hello {{username}}, now = {{datetime}}",
                pre_script: "",
                post_script: "",
            },
            SeedRequest {
                name: "JSON 嵌套对象与数组",
                method: "POST",
                url: "{{base_url}}/anything",
                headers: ACCEPT_JSON,
                params: &[],
                body_type: "json",
                body: r#"{
  "orderId": "{{uuid}}",
  "amount": {{randomFloat(10, 100, 2)}},
  "channel": "api",
  "items": [
    { "sku": "A-001", "qty": {{randomInt(1, 5)}} },
    { "sku": "B-002", "qty": {{randomInt(1, 5)}} }
  ],
  "meta": { "client": "{{username}}", "createTime": "{{datetime}}" }
}"#,
                pre_script: "",
                post_script: "",
            },
            SeedRequest {
                name: "无 Body 的 POST",
                method: "POST",
                url: "{{base_url}}/post",
                headers: ACCEPT_JSON,
                params: &[],
                body_type: "none",
                body: "",
                pre_script: "",
                post_script: "",
            },
        ],
    },
    SeedCollection {
        name: "04 变量与脚本",
        requests: &[
            SeedRequest {
                name: "内置变量演示",
                method: "GET",
                url: "{{base_url}}/get",
                headers: &[
                    ("Accept", "application/json"),
                    ("X-Trace-Id", "{{uuidNoDash}}"),
                    ("X-At", "{{datetime}}"),
                ],
                params: &[
                    ("ts", "{{timestamp}}"),
                    ("sec", "{{timestampSec}}"),
                    ("iso", "{{isoTime}}"),
                    ("date", "{{date}}"),
                    ("uuid", "{{uuid}}"),
                    ("int", "{{randomInt(1, 100)}}"),
                    ("str", "{{randomString(10)}}"),
                    ("float", "{{randomFloat(1, 10, 3)}}"),
                    ("hex", "{{randomHex(8)}}"),
                    ("bool", "{{randomBool}}"),
                    ("email", "{{randomEmail}}"),
                    ("phone", "{{randomPhone}}"),
                    ("ip", "{{randomIPv4}}"),
                ],
                body_type: "none",
                body: "",
                pre_script: "",
                post_script: "",
            },
            SeedRequest {
                name: "前置脚本生成变量",
                method: "GET",
                url: "{{base_url}}/get",
                headers: ACCEPT_JSON,
                params: &[
                    ("traceId", "{{traceId}}"),
                    ("nonce", "{{nonce}}"),
                    ("sign", "{{sign}}"),
                ],
                body_type: "none",
                body: "",
                pre_script: r#"// 自写脚本：为运行时变量赋值，随后可在 URL / Header / Body 中用 {{变量名}} 引用
vars.set('traceId', 'trace-' + Date.now())
vars.set('nonce', Math.random().toString(36).slice(2, 10))
vars.set('sign', 'demo-' + vars.get('traceId'))

// 也可以直接改写本次请求
request.headers['X-Request-Id'] = vars.get('traceId')

console.log('traceId =', vars.get('traceId'))"#,
                post_script: "",
            },
            SeedRequest {
                name: "前置脚本改写请求",
                method: "GET",
                url: "{{base_url}}/get",
                headers: &[("Accept", "application/json")],
                params: &[("from", "seed")],
                body_type: "none",
                body: "",
                pre_script: r#"// 发送前动态拼接查询参数与请求头
request.params['dynamic'] = 'v-' + Date.now()
request.headers['X-Debug'] = 'on'

console.log('url =', request.url)"#,
                post_script: "",
            },
            SeedRequest {
                name: "后置脚本断言与提取",
                method: "GET",
                url: "{{base_url}}/json",
                headers: ACCEPT_JSON,
                params: &[],
                body_type: "none",
                body: "",
                pre_script: "",
                post_script: r#"test('状态码为 200', () => assert(response.status === 200, '实际状态码：' + response.status))
test('响应包含 slideshow', () => assert(response.json().slideshow, '缺少 slideshow 字段'))
test('耗时低于 3 秒', () => assert(response.durationMs < 3000, '实际耗时：' + response.durationMs + 'ms'))

// 把响应里的值写入运行时变量，后续请求可用 {{last_title}} 引用
vars.set('last_title', response.json().slideshow.title)
console.log('slideshow.title =', vars.get('last_title'))"#,
            },
            SeedRequest {
                name: "后置脚本提取 Token",
                method: "POST",
                url: "{{base_url}}/post",
                headers: ACCEPT_JSON,
                params: &[],
                body_type: "json",
                body: r#"{ "username": "{{username}}", "password": "demo" }"#,
                pre_script: "",
                post_script: r#"test('登录成功', () => assert(response.status === 200, '登录失败：' + response.status))

// 演示跨请求变量：模拟从响应中提取 Token，后面的「使用上一步 Token」可直接引用
vars.set('login_token', 'token-' + Date.now())
console.log('login_token 已写入运行时变量')"#,
            },
            SeedRequest {
                name: "使用上一步 Token",
                method: "GET",
                url: "{{base_url}}/bearer",
                headers: &[
                    ("Accept", "application/json"),
                    ("Authorization", "Bearer {{login_token}}"),
                ],
                params: &[],
                body_type: "none",
                body: "",
                pre_script: "",
                post_script: "",
            },
            SeedRequest {
                name: "断言失败示例",
                method: "GET",
                url: "{{base_url}}/status/500",
                headers: ACCEPT_JSON,
                params: &[],
                body_type: "none",
                body: "",
                pre_script: "",
                post_script: r#"test('接口应返回 200', () => assert(response.status === 200, '服务端返回 ' + response.status))"#,
            },
        ],
    },
    SeedCollection {
        name: "05 状态码与延迟",
        requests: &[
            SeedRequest {
                name: "201 创建成功",
                method: "POST",
                url: "{{base_url}}/status/201",
                headers: ACCEPT_JSON,
                params: &[],
                body_type: "none",
                body: "",
                pre_script: "",
                post_script: "",
            },
            SeedRequest {
                name: "400 参数错误",
                method: "GET",
                url: "{{base_url}}/status/400",
                headers: ACCEPT_JSON,
                params: &[],
                body_type: "none",
                body: "",
                pre_script: "",
                post_script: "",
            },
            SeedRequest {
                name: "500 服务异常",
                method: "GET",
                url: "{{base_url}}/status/500",
                headers: ACCEPT_JSON,
                params: &[],
                body_type: "none",
                body: "",
                pre_script: "",
                post_script: "",
            },
            SeedRequest {
                name: "延迟 2 秒",
                method: "GET",
                url: "{{base_url}}/delay/2",
                headers: ACCEPT_JSON,
                params: &[],
                body_type: "none",
                body: "",
                pre_script: "",
                post_script: "",
            },
            SeedRequest {
                name: "重定向 2 次",
                method: "GET",
                url: "{{base_url}}/redirect/2",
                headers: ACCEPT_JSON,
                params: &[],
                body_type: "none",
                body: "",
                pre_script: "",
                post_script: "",
            },
        ],
    },
    SeedCollection {
        name: "06 压测示例",
        requests: &[
            SeedRequest {
                name: "GET 基准接口",
                method: "GET",
                url: "{{base_url}}/get",
                headers: ACCEPT_JSON,
                params: &[("id", "{{randomInt(1, 1000)}}"), ("ts", "{{timestamp}}")],
                body_type: "none",
                body: "",
                pre_script: "",
                post_script: "",
            },
            SeedRequest {
                name: "POST 基准接口",
                method: "POST",
                url: "{{base_url}}/post",
                headers: ACCEPT_JSON,
                params: &[],
                body_type: "json",
                body: r#"{ "id": "{{uuid}}", "ts": {{timestamp}} }"#,
                pre_script: "",
                post_script: "",
            },
        ],
    },
    SeedCollection {
        name: POOL_EXAMPLE_COLLECTION,
        requests: POOL_EXAMPLE_REQUESTS,
    },
];

const ENV_VARS: &[(&str, &str)] = &[
    ("base_url", "https://httpbin.org"),
    ("token", "demo-token"),
    ("api_key", "demo-api-key"),
    ("user_id", "1001"),
    ("username", "astroman"),
];

const ENV_SCRIPT: &str = r#"// 环境脚本：每次发送请求前执行，可为环境变量赋值
// 在这里赋值的变量会写入本次请求的运行时变量，不会改动下方环境变量表
vars.set('request_time', new Date().toISOString())
vars.set('client_version', '1.0.2')
vars.set('run_id', 'run-' + Date.now())"#;

/* ---------------- 示例测试套件 ---------------- */

const SAMPLE_SUITE: &str = "示例回归（httpbin）";
const SAMPLE_SUITE_CONFIG: &str = r#"{"failFast":false,"maxWorkers":1,"retries":0,"parallelRows":false,"cookieMode":"shared","strictScripts":false,"suiteTimeoutSec":120,"teardownFailureFatal":true,"vars":{}}"#;

struct SeedCase {
    request_name: &'static str,
    name: &'static str,
    assertions: &'static str,
    tags: &'static str,
}

const SAMPLE_CASES: &[SeedCase] = &[
    SeedCase {
        request_name: "GET 查询参数",
        name: "查询参数回显",
        assertions: r#"[{"type":"status","op":"eq","value":200,"name":"状态码 200"},{"type":"jsonPath","path":"$.args.keyword","op":"eq","value":"astroman","name":"keyword 回显"}]"#,
        tags: "smoke,demo",
    },
    SeedCase {
        request_name: "POST 创建 JSON",
        name: "创建 JSON 并回显",
        assertions: r#"[{"type":"status","op":"eq","value":200,"name":"状态码 200"},{"type":"jsonPath","path":"$.json.name","op":"eq","value":"AstroMan","name":"name 回显"},{"type":"durationMs","op":"lte","value":10000,"name":"10 秒内返回"}]"#,
        tags: "smoke,demo",
    },
    SeedCase {
        request_name: "Bearer Token 认证",
        name: "Bearer 认证通过",
        assertions: r#"[{"type":"status","op":"eq","value":200,"name":"状态码 200"},{"type":"jsonPath","path":"$.authenticated","op":"eq","value":true,"name":"认证成功"}]"#,
        tags: "auth,demo",
    },
];

/// 给示例项目补一个可直接运行的回归套件（幂等，老库升级也会补）
pub fn ensure_sample_suite(conn: &Connection) -> Result<(), String> {
    let project_id: Option<i64> = conn
        .query_row(
            "SELECT id FROM projects WHERE name = ?1 ORDER BY id ASC LIMIT 1",
            params![SAMPLE_PROJECT],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let Some(project_id) = project_id else {
        return Ok(());
    };

    // 示例项目里已经有套件（无论是内置的还是用户自建），都不再追加，避免删了又回来
    let exists: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM test_suites WHERE project_id = ?1",
            params![project_id],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    if exists > 0 {
        return Ok(());
    }

    let env_id: Option<i64> = conn
        .query_row(
            "SELECT id FROM environments WHERE project_id = ?1 AND name = ?2
             ORDER BY is_active DESC, id ASC LIMIT 1",
            params![project_id, ENV_NAME],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;

    // 只挂到仍然存在的示例接口上；接口都被删掉时不再生成空套件
    let mut matched: Vec<(&SeedCase, i64)> = Vec::new();
    for case in SAMPLE_CASES {
        let request_id: Option<i64> = conn
            .query_row(
                "SELECT r.id FROM requests r
                 JOIN collections c ON c.id = r.collection_id
                 WHERE c.project_id = ?1 AND r.name = ?2
                 ORDER BY r.id LIMIT 1",
                params![project_id, case.request_name],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        if let Some(request_id) = request_id {
            matched.push((case, request_id));
        }
    }
    if matched.is_empty() {
        return Ok(());
    }

    let now = now_ms();
    conn.execute(
        "INSERT INTO test_suites
            (project_id, name, description, env_id, tags, config_json, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, 'demo', ?5, ?6, ?6)",
        params![
            project_id,
            SAMPLE_SUITE,
            "内置示例：对 httpbin 的查询参数、JSON 回显与 Bearer 认证做断言，可直接运行",
            env_id,
            SAMPLE_SUITE_CONFIG,
            now
        ],
    )
    .map_err(|e| e.to_string())?;
    let suite_id = conn.last_insert_rowid();

    for (index, (case, request_id)) in matched.iter().enumerate() {
        conn.execute(
            "INSERT INTO test_cases
                (suite_id, name, kind, request_id, sort_order, enabled, timeout_ms, tags,
                 assertions_json, created_at, updated_at)
             VALUES (?1, ?2, 'request', ?3, ?4, 1, 30000, ?5, ?6, ?7, ?7)",
            params![
                suite_id,
                case.name,
                request_id,
                index as i64,
                case.tags,
                case.assertions,
                now
            ],
        )
        .map_err(|e| e.to_string())?;
    }

    Ok(())
}

/// 首次建库时写入示例项目
pub fn seed(conn: &Connection) -> Result<(), String> {
    let project_id = ensure_project(conn)?;
    ensure_env(conn, project_id)?;
    insert_scenarios(conn, project_id)?;
    Ok(())
}

/// 旧版本数据库升级：示例项目仍是初始状态时，补齐场景集合、接口与环境变量
pub fn upgrade_sample(conn: &Connection) -> Result<(), String> {
    let project_id: Option<i64> = conn
        .query_row(
            "SELECT id FROM projects WHERE name = ?1 ORDER BY id ASC LIMIT 1",
            params![SAMPLE_PROJECT],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;

    let Some(project_id) = project_id else {
        return Ok(());
    };

    let scenario_exists: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM collections WHERE project_id = ?1 AND name = ?2",
            params![project_id, FIRST_SCENARIO],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    if scenario_exists > 0 {
        return Ok(());
    }

    let sample_exists: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM requests WHERE name = ?1 AND collection_id IN (
                SELECT id FROM collections WHERE project_id = ?2
            )",
            params![SAMPLE_REQUEST, project_id],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;

    let request_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM requests WHERE collection_id IN (
                SELECT id FROM collections WHERE project_id = ?1
            )",
            params![project_id],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;

    // 用户已经自己改过示例项目时，不再追加
    if sample_exists == 0 || request_count > 3 {
        return Ok(());
    }

    insert_scenarios(conn, project_id)?;
    ensure_env(conn, project_id)?;
    Ok(())
}

fn ensure_project(conn: &Connection) -> Result<i64, String> {
    let existing: Option<i64> = conn
        .query_row(
            "SELECT id FROM projects WHERE name = ?1 ORDER BY id ASC LIMIT 1",
            params![SAMPLE_PROJECT],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;

    if let Some(id) = existing {
        return Ok(id);
    }

    let now = now_ms();
    conn.execute(
        "INSERT INTO projects (name, description, color, sort_order, created_at, updated_at)
         VALUES (?1, ?2, ?3, 0, ?4, ?4)",
        params![
            SAMPLE_PROJECT,
            "内置示例：覆盖基础请求、认证、表单、变量脚本、状态码与压测场景",
            "#3b7ddd",
            now
        ],
    )
    .map_err(|e| e.to_string())?;

    Ok(conn.last_insert_rowid())
}

fn ensure_env(conn: &Connection, project_id: i64) -> Result<(), String> {
    let env_id: Option<i64> = conn
        .query_row(
            "SELECT id FROM environments WHERE project_id = ?1 AND name = ?2
             ORDER BY is_active DESC, id ASC LIMIT 1",
            params![project_id, ENV_NAME],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;

    let env_id = match env_id {
        Some(id) => id,
        None => {
            let now = now_ms();
            conn.execute(
                "INSERT INTO environments (project_id, name, is_active, script, created_at)
                 VALUES (?1, ?2, 1, ?3, ?4)",
                params![project_id, ENV_NAME, ENV_SCRIPT, now],
            )
            .map_err(|e| e.to_string())?;
            conn.last_insert_rowid()
        }
    };

    for (index, (key, value)) in ENV_VARS.iter().enumerate() {
        let exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM env_variables WHERE env_id = ?1 AND \"key\" = ?2",
                params![env_id, key],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())?;

        if exists == 0 {
            conn.execute(
                "INSERT INTO env_variables (env_id, \"key\", value, is_secret, sort_order)
                 VALUES (?1, ?2, ?3, 0, ?4)",
                params![env_id, key, value, index as i64],
            )
            .map_err(|e| e.to_string())?;
        }
    }

    Ok(())
}

fn insert_scenarios(conn: &Connection, project_id: i64) -> Result<(), String> {
    let now = now_ms();
    for (collection_index, scenario) in SCENARIOS.iter().enumerate() {
        conn.execute(
            "INSERT INTO collections (project_id, parent_id, name, sort_order, created_at, updated_at)
             VALUES (?1, NULL, ?2, ?3, ?4, ?4)",
            params![project_id, scenario.name, collection_index as i64, now],
        )
        .map_err(|e| e.to_string())?;

        let collection_id = conn.last_insert_rowid();
        for (request_index, request) in scenario.requests.iter().enumerate() {
            insert_request(conn, collection_id, request, request_index as i64, now)?;
        }
    }
    Ok(())
}

fn insert_request(
    conn: &Connection,
    collection_id: i64,
    request: &SeedRequest,
    sort_order: i64,
    now: i64,
) -> Result<(), String> {
    let headers = serde_json::to_string(&to_items(request.headers)).map_err(|e| e.to_string())?;
    let query = serde_json::to_string(&to_items(request.params)).map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO requests (
            collection_id, name, method, url, headers, params, body_type, body,
            pre_script, post_script, assertions, sort_order, created_at, updated_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, '[]', ?11, ?12, ?12)",
        params![
            collection_id,
            request.name,
            request.method,
            request.url,
            headers,
            query,
            request.body_type,
            request.body,
            request.pre_script,
            request.post_script,
            sort_order,
            now
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// 数据池示例：自定义列池 + 示例请求 + 示例压测计划。
///
/// 只追加一次（app_meta 标记），新库与老库升级都能看到；用户删除后不会再次复活。
pub fn ensure_sample_pool_examples(conn: &Connection) -> Result<(), String> {
    if crate::db::meta_get(conn, SAMPLE_POOL_SEEDED_KEY).is_some() {
        // 已经写过基础示例：仍要检查后面追加的导入 / 轮换流程示例
        return ensure_sample_pool_flow_examples(conn);
    }

    let project_id: Option<i64> = conn
        .query_row(
            "SELECT id FROM projects WHERE name = ?1 ORDER BY id ASC LIMIT 1",
            params![SAMPLE_PROJECT],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let Some(project_id) = project_id else {
        mark_sample_pool_seeded(conn)?;
        return ensure_sample_pool_flow_examples(conn);
    };

    let now = now_ms();
    // 1) 示例集合与请求（老库可能还没有 07 集合）
    let collection_id: i64 = match conn
        .query_row(
            "SELECT id FROM collections WHERE project_id = ?1 AND name = ?2 ORDER BY id ASC LIMIT 1",
            params![project_id, POOL_EXAMPLE_COLLECTION],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?
    {
        Some(id) => id,
        None => {
            conn.execute(
                "INSERT INTO collections (project_id, parent_id, name, sort_order, created_at, updated_at)
                 VALUES (?1, NULL, ?2, ?3, ?4, ?4)",
                params![project_id, POOL_EXAMPLE_COLLECTION, (SCENARIOS.len() - 1) as i64, now],
            )
            .map_err(|e| e.to_string())?;
            conn.last_insert_rowid()
        }
    };
    for (index, request) in POOL_EXAMPLE_REQUESTS.iter().enumerate() {
        let exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM requests WHERE collection_id = ?1 AND name = ?2",
                params![collection_id, request.name],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())?;
        if exists == 0 {
            insert_request(conn, collection_id, request, index as i64, now)?;
        }
    }

    // 2) 自定义列示例池：phone / password / token 三列，敏感展示
    insert_sample_pool(
        conn,
        SAMPLE_POOL_NAME,
        "parameter",
        true,
        "内置示例：phone / password / token 三列；绑定「全部列（同名注入）」即可在请求里用 {{phone}}、{{password}}、{{token}}",
        &["phone", "password", "token"],
        &[
            &[
                ("phone", "13800000001"),
                ("password", "demo-pass-1"),
                ("token", "demo-token-1"),
            ],
            &[
                ("phone", "13800000002"),
                ("password", "demo-pass-2"),
                ("token", "demo-token-2"),
            ],
            &[
                ("phone", "13800000003"),
                ("password", "demo-pass-3"),
                ("token", "demo-token-3"),
            ],
        ],
        now,
    )?;

    // 3) 示例压测计划：绑定「全部列」+ 指定列 token → demo_token
    insert_sample_plan(
        conn,
        project_id,
        collection_id,
        SAMPLE_POOL_PLAN,
        POOL_EXAMPLE_REQUESTS[0].name,
        serde_json::json!([
            {
                "pool": SAMPLE_POOL_NAME,
                "target": "",
                "mode": "per_vu",
                "refreshOn401": false,
                "column": "*"
            },
            {
                "pool": SAMPLE_POOL_NAME,
                "target": "demo_token",
                "mode": "per_vu",
                "refreshOn401": false,
                "column": "token"
            }
        ]),
        5,
        15,
        3,
        now,
    )?;

    mark_sample_pool_seeded(conn)?;
    // 导入 / 逐请求轮换示例单独标记，已升到 v20 的老库也能补上
    ensure_sample_pool_flow_examples(conn)
}

fn mark_sample_pool_seeded(conn: &Connection) -> Result<(), String> {
    crate::db::meta_set(conn, SAMPLE_POOL_SEEDED_KEY, "1")
}

/// 导入与逐请求轮换示例：Excel 模板导入得到的参数池 + 每请求轮换的压测计划。
///
/// 单独用 app_meta 标记，保证已经升到 v20 的库也能补上；用户删除后不再复活。
pub fn ensure_sample_pool_flow_examples(conn: &Connection) -> Result<(), String> {
    if crate::db::meta_get(conn, SAMPLE_POOL_FLOW_KEY).is_some() {
        return Ok(());
    }
    let project_id: Option<i64> = conn
        .query_row(
            "SELECT id FROM projects WHERE name = ?1 ORDER BY id ASC LIMIT 1",
            params![SAMPLE_PROJECT],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let Some(project_id) = project_id else {
        return crate::db::meta_set(conn, SAMPLE_POOL_FLOW_KEY, "1");
    };
    let collection_id: Option<i64> = conn
        .query_row(
            "SELECT id FROM collections WHERE project_id = ?1 AND name = ?2 ORDER BY id ASC LIMIT 1",
            params![project_id, POOL_EXAMPLE_COLLECTION],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let Some(collection_id) = collection_id else {
        return crate::db::meta_set(conn, SAMPLE_POOL_FLOW_KEY, "1");
    };

    let now = now_ms();
    // 1) 请求：三列都来自导入池，演示「列名即变量名」
    for (index, request) in POOL_IMPORT_REQUESTS.iter().enumerate() {
        let exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM requests WHERE collection_id = ?1 AND name = ?2",
                params![collection_id, request.name],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())?;
        if exists == 0 {
            insert_request(
                conn,
                collection_id,
                request,
                (POOL_EXAMPLE_REQUESTS.len() + index) as i64,
                now,
            )?;
        }
    }

    // 2) 导入池：等价于下载模板后按 value / region / level 填表导入
    insert_sample_pool(
        conn,
        SAMPLE_IMPORT_POOL_NAME,
        "parameter",
        false,
        "内置示例：等同于「下载 Excel 模板 → 填 value / region / level 三列 → 导入」；绑定「全部列（同名注入）」后，请求里可直接用 {{value}}、{{region}}、{{level}}",
        &["value", "region", "level"],
        &[
            &[
                ("value", "sku-1001"),
                ("region", "cn-east"),
                ("level", "gold"),
            ],
            &[
                ("value", "sku-1002"),
                ("region", "cn-north"),
                ("level", "silver"),
            ],
            &[
                ("value", "sku-1003"),
                ("region", "cn-south"),
                ("level", "gold"),
            ],
            &[
                ("value", "sku-1004"),
                ("region", "cn-west"),
                ("level", "bronze"),
            ],
        ],
        now,
    )?;

    // 3) 计划：逐请求轮换，所有请求共用同一池但每条换一行
    insert_sample_plan(
        conn,
        project_id,
        collection_id,
        SAMPLE_IMPORT_PLAN,
        POOL_IMPORT_REQUESTS[0].name,
        serde_json::json!([{
            "pool": SAMPLE_IMPORT_POOL_NAME,
            "target": "",
            "mode": "per_request",
            "refreshOn401": false,
            "column": "*"
        }]),
        8,
        20,
        5,
        now,
    )?;

    crate::db::meta_set(conn, SAMPLE_POOL_FLOW_KEY, "1")
}

/// 写入一个自定义列示例池；同名池已存在时跳过，避免覆盖用户数据
fn insert_sample_pool(
    conn: &Connection,
    name: &str,
    kind: &str,
    sensitive: bool,
    description: &str,
    columns: &[&str],
    rows: &[&[(&str, &str)]],
    now: i64,
) -> Result<(), String> {
    let exists: Option<i64> = conn
        .query_row(
            "SELECT id FROM data_pools WHERE name = ?1",
            params![name],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    if exists.is_some() {
        return Ok(());
    }
    conn.execute(
        "INSERT INTO data_pools
            (name, kind, sensitive, description, refresh_mode, refresh_json, columns_json, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, 'none', '{}', ?5, ?6, ?6)",
        params![
            name,
            kind,
            sensitive as i64,
            description,
            serde_json::json!(columns).to_string(),
            now
        ],
    )
    .map_err(|e| e.to_string())?;
    let pool_id = conn.last_insert_rowid();
    for row in rows {
        let value = row.first().map(|(_, value)| *value).unwrap_or_default();
        let data = row
            .iter()
            .map(|(key, value)| (key.to_string(), serde_json::json!(value)))
            .collect::<serde_json::Map<String, serde_json::Value>>();
        conn.execute(
            "INSERT INTO data_pool_items
                (pool_id, value, account, weight, enabled, created_at, expires_at, cooldown_until, fail_count, last_used_at, data_json)
             VALUES (?1, ?2, NULL, 1, 1, ?3, 0, 0, 0, 0, ?4)",
            params![
                pool_id,
                value,
                now,
                serde_json::Value::Object(data).to_string()
            ],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// 写入一个引用示例请求的压测计划；同名计划已存在时跳过
#[allow(clippy::too_many_arguments)]
fn insert_sample_plan(
    conn: &Connection,
    project_id: i64,
    collection_id: i64,
    plan_name: &str,
    request_name: &str,
    bindings: serde_json::Value,
    concurrent: i64,
    duration_sec: i64,
    ramp_up_sec: i64,
    now: i64,
) -> Result<(), String> {
    let exists: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM test_plans WHERE project_id = ?1 AND name = ?2",
            params![project_id, plan_name],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    if exists > 0 {
        return Ok(());
    }
    let target: Option<(i64, String, String)> = conn
        .query_row(
            "SELECT id, method, url FROM requests WHERE collection_id = ?1 AND name = ?2",
            params![collection_id, request_name],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let Some((request_id, method, url)) = target else {
        return Ok(());
    };
    let env: Option<(i64, String)> = conn
        .query_row(
            "SELECT id, name FROM environments
             WHERE project_id = ?1 AND is_active = 1 ORDER BY id ASC LIMIT 1",
            params![project_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let mut config = serde_json::json!({
        "schemaVersion": 2,
        "settings": {
            "mode": "fixed",
            "recordDetail": "auto",
            "captureMode": "auto",
            "topN": 200,
            "slowThresholdMs": 0,
            "concurrent": concurrent,
            "durationSec": duration_sec,
            "rampUpSec": ramp_up_sec,
            "thinkTimeMs": 0,
            "timeoutMs": 30000,
            "connectTimeoutMs": 10000,
            "followRedirects": true,
            "insecure": false,
            "poolBindings": bindings
        },
        "stages": [],
        "target": {
            "requestId": request_id,
            "method": method,
            "urlTemplate": url,
            "resolvedUrl": url.replace("{{base_url}}", "https://httpbin.org"),
            "capturedAt": now,
        }
    });
    if let Some((env_id, env_name)) = env {
        config["envId"] = serde_json::json!(env_id);
        config["envName"] = serde_json::json!(env_name);
    }
    conn.execute(
        "INSERT INTO test_plans
            (project_id, request_id, name, concurrent, duration_sec, ramp_up_sec,
             think_time_ms, timeout_ms, config_json, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, 30000, ?7, ?8, ?8)",
        params![
            project_id,
            request_id,
            plan_name,
            concurrent,
            duration_sec,
            ramp_up_sec,
            config.to_string(),
            now
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn to_items(pairs: &[(&str, &str)]) -> Vec<KeyValueItem> {
    pairs
        .iter()
        .map(|(key, value)| KeyValueItem {
            key: key.to_string(),
            value: value.to_string(),
            enabled: true,
        })
        .collect()
}
