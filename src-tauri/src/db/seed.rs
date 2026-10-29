use crate::models::KeyValueItem;
use crate::util::now_ms;
use rusqlite::{params, Connection, OptionalExtension};

const SAMPLE_PROJECT: &str = "示例项目";
const ENV_NAME: &str = "本地环境";
const FIRST_SCENARIO: &str = "01 基础请求";
const SAMPLE_REQUEST: &str = "示例请求";

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
                headers: &[
                    ("Accept", "application/json"),
                    ("X-API-Key", "{{api_key}}"),
                ],
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
                    request_index as i64,
                    now
                ],
            )
            .map_err(|e| e.to_string())?;
        }
    }
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
