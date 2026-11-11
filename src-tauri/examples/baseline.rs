//! W0 性能基线：不依赖第三方基准框架，跑真实实现并打印耗时。
//!
//! 用法：
//!   cargo run --release --features bench --example baseline
//!   cargo run --release --features bench --example baseline -- --check
//!
//! 默认只记录（技术方案 9.5：先记录两个版本再设阻断）；`--check` 时低于目标线
//! 才返回 0，方便以后接 CI。

use std::collections::HashMap;
use std::time::Instant;

use astroman_lib::bench_api;
use astroman_lib::bench_api::{BenchDocsCache, BenchDocsPreparePayload};
use rusqlite::{params, Connection};
use serde_json::json;

const SCHEMA: &str = include_str!("../src/db/schema.sql");

fn main() {
    let check = std::env::args().any(|arg| arg == "--check");
    println!("AstroMan W0 性能基线（单次耗时，取多轮最小值）");
    println!("------------------------------------------------------------");

    let conn = seed(500);
    let mut failed = false;

    let ms = bench("openapi_build (500)", 500.0, 3, || {
        bench_api::openapi_spec(&conn, 1).expect("openapi build")
    });
    failed |= ms > 500.0;

    let cache = BenchDocsCache::new();
    let payload = BenchDocsPreparePayload {
        scope: "project".to_string(),
        target_id: 1,
        environment_id: None,
        options: Default::default(),
    };
    let ms = bench("docs_render (200)", 800.0, 3, || {
        bench_api::docs_render(&conn, &cache, &payload).expect("docs render")
    });
    failed |= ms > 800.0;

    seed_revisions(&conn);
    let ms = bench("revision_diff", 50.0, 5, || {
        bench_api::revision_diff(&conn, 1, 1, 2).expect("revision diff")
    });
    failed |= ms > 50.0;

    let body = tracked_body(1024 * 1024);
    let vars = tracked_vars();
    let ms = bench("render_tracked (1MB)", 20.0, 3, || {
        bench_api::render_tracked(&body, &vars)
    });
    failed |= ms > 20.0;

    println!("------------------------------------------------------------");
    if check && failed {
        eprintln!("有指标超过目标线");
        std::process::exit(1);
    }
    println!("完成（记录模式，不阻断）");
}

fn bench<T>(name: &str, target_ms: f64, runs: usize, mut task: impl FnMut() -> T) -> f64 {
    let _ = task(); // 预热：连接、码表、SQLite 页缓存
    let mut best = f64::MAX;
    for _ in 0..runs {
        let started = Instant::now();
        let _ = task();
        best = best.min(started.elapsed().as_secs_f64() * 1000.0);
    }
    let verdict = if best <= target_ms { "PASS" } else { "SLOW" };
    println!("{name:<24}{best:>10.2} ms   目标 < {target_ms:>6.1} ms   {verdict}");
    best
}

fn open_in_memory() -> Connection {
    let conn = Connection::open_in_memory().expect("open memory db");
    conn.execute_batch(SCHEMA).expect("schema");
    conn
}

fn seed(requests: usize) -> Connection {
    let conn = open_in_memory();
    let now = 1_700_000_000_000_i64;
    conn.execute(
        "INSERT INTO projects (id, name, description, created_at, updated_at)
         VALUES (1, 'bench', '性能基线', ?1, ?1)",
        params![now],
    )
    .expect("project");
    conn.execute(
        "INSERT INTO collections (id, project_id, name, created_at, updated_at)
         VALUES (1, 1, 'api', ?1, ?1)",
        params![now],
    )
    .expect("collection");

    let entity = json!({
        "version": 1,
        "updatedAt": now,
        "hiddenPaths": [],
        "root": {
            "name": "$",
            "type": "object",
            "itemType": "",
            "required": false,
            "description": "",
            "manual": false,
            "stale": false,
            "children": [
                {
                    "name": "id", "type": "integer", "itemType": "", "required": true,
                    "description": "", "manual": false, "stale": false, "children": []
                },
                {
                    "name": "name", "type": "string", "itemType": "", "required": false,
                    "description": "", "manual": false, "stale": false, "children": []
                }
            ]
        }
    })
    .to_string();

    let tx = conn.unchecked_transaction().expect("tx");
    {
        let mut stmt = tx
            .prepare(
                "INSERT INTO requests
                 (collection_id, name, method, url, headers, params, body_type, body,
                  request_entity, response_entity, created_at, updated_at)
                 VALUES (1, ?1, ?2, ?3, '[]', '[]', 'json', ?4, ?5, ?5, ?6, ?6)",
            )
            .expect("prepare insert");
        for index in 0..requests {
            let name = format!("接口 {index}");
            let method = if index % 4 == 0 { "POST" } else { "GET" };
            let url = format!("http://localhost/api/v1/resource/{index}");
            let body = r#"{"name":"demo","value":1}"#;
            stmt.execute(params![name, method, url, body, entity, now])
                .expect("insert request");
        }
    }
    tx.commit().expect("commit");
    conn
}

fn seed_revisions(conn: &Connection) {
    let now = 1_700_000_000_000_i64;
    let make = |value: &str| {
        json!({
            "name": "bench",
            "method": "POST",
            "url": "http://localhost/api/v1/resource",
            "headers": r#"[{"key":"Content-Type","value":"application/json","enabled":true}]"#,
            "params": "[]",
            "bodyType": "json",
            "body": format!(r#"{{"value":"{value}"}}"#),
            "bodyMeta": "",
            "preScript": "",
            "postScript": "",
            "assertions": "[]",
            "auth": "",
            "requestEntity": null,
            "responseEntity": null
        })
        .to_string()
    };
    for (revision, value) in [(1_i64, "one"), (2, "two")] {
        conn.execute(
            "INSERT INTO request_revisions (request_id, revision, snapshot_json, source, created_at)
             VALUES (1, ?1, ?2, 'save', ?3)",
            params![revision, make(value), now],
        )
        .expect("revision");
    }
}

fn tracked_body(size: usize) -> String {
    let unit = "{\"id\":{{id}},\"name\":\"{{name}}\",\"token\":\"{{token}}\"}\n";
    let mut body = String::with_capacity(size + unit.len());
    while body.len() < size {
        body.push_str(unit);
    }
    body
}

fn tracked_vars() -> HashMap<String, String> {
    let mut vars = HashMap::new();
    vars.insert("id".to_string(), "42".to_string());
    vars.insert("name".to_string(), "demo".to_string());
    vars.insert("token".to_string(), "secret".to_string());
    vars
}
