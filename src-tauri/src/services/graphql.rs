//! GraphQL Schema 感知（G-10）
//!
//! 只做 introspection 拉取与缓存，不做完整语言服务：
//! - 结果按 (project_id, endpoint) 唯一保存；
//! - 请求头只在本次请求使用，敏感值不落库；
//! - 服务端关闭 introspection 时返回明确错误，前端降级为普通编辑器。

use std::time::Duration;

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::util::{err, now_ms};

/// introspection 结果上限，避免异常服务端把库撑爆
pub const MAX_SCHEMA_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GraphqlIntrospectPayload {
    pub project_id: i64,
    pub endpoint: String,
    #[serde(default)]
    pub headers: Vec<crate::models::KeyValueItem>,
    #[serde(default)]
    pub env_id: Option<i64>,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphqlSchemaEntry {
    pub id: i64,
    pub project_id: i64,
    pub endpoint: String,
    pub schema_json: String,
    pub fetched_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphqlSchemaMeta {
    pub id: i64,
    pub project_id: i64,
    pub endpoint: String,
    pub fetched_at: i64,
    pub bytes: usize,
}

/// 标准 introspection 查询（只取前端 Schema 浏览器需要的字段）
pub const INTROSPECTION_QUERY: &str = r#"query IntrospectionQuery {
  __schema {
    queryType { name }
    mutationType { name }
    subscriptionType { name }
    types {
      kind
      name
      description
      fields(includeDeprecated: true) {
        name
        description
        args {
          name
          description
          defaultValue
          type { ...TypeRef }
        }
        type { ...TypeRef }
        isDeprecated
        deprecationReason
      }
      inputFields {
        name
        description
        defaultValue
        type { ...TypeRef }
      }
      interfaces { kind name }
      enumValues(includeDeprecated: true) {
        name
        description
        isDeprecated
        deprecationReason
      }
      possibleTypes { kind name }
    }
  }
}

fragment TypeRef on __Type {
  kind
  name
  ofType {
    kind
    name
    ofType {
      kind
      name
      ofType {
        kind
        name
        ofType {
          kind
          name
        }
      }
    }
  }
}"#;

pub async fn introspect(
    client: &reqwest::Client,
    endpoint: &str,
    headers: &[(String, String)],
    timeout: Duration,
) -> Result<Value, String> {
    let endpoint = endpoint.trim();
    if endpoint.is_empty() {
        return Err("GraphQL Endpoint 不能为空".to_string());
    }
    let mut request = client
        .post(endpoint)
        .timeout(timeout)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json")
        .json(&json!({
            "query": INTROSPECTION_QUERY,
            "operationName": "IntrospectionQuery",
            "variables": null,
        }));
    for (name, value) in headers {
        let name = reqwest::header::HeaderName::from_bytes(name.as_bytes())
            .map_err(|_| format!("请求头不合法：{name}"))?;
        let value = reqwest::header::HeaderValue::from_str(value)
            .map_err(|_| format!("请求头值不合法：{name}"))?;
        request = request.header(name, value);
    }
    let response = request
        .send()
        .await
        .map_err(|error| format!("GraphQL introspection 请求失败：{error}"))?;
    let status = response.status();
    let text = response
        .text()
        .await
        .map_err(|error| format!("读取 GraphQL 响应失败：{error}"))?;
    let value: Value = serde_json::from_str(&text).map_err(|_| {
        let preview = crate::logging::redact(&text.chars().take(300).collect::<String>());
        if status.is_success() {
            format!("GraphQL 返回的不是 JSON：{preview}")
        } else {
            format!("GraphQL 响应 {}：{preview}", status.as_u16())
        }
    })?;
    if let Some(errors) = value.get("errors") {
        if value.pointer("/data/__schema").is_none() {
            return Err(format!(
                "E_INTROSPECTION: 服务端未返回 __schema（可能已关闭 introspection）：{}",
                crate::logging::redact(&errors.to_string().chars().take(300).collect::<String>())
            ));
        }
    }
    if value.pointer("/data/__schema").is_none() {
        return Err("E_INTROSPECTION: 响应缺少 data.__schema".to_string());
    }
    Ok(value)
}

pub fn save(
    conn: &Connection,
    project_id: i64,
    endpoint: &str,
    schema_json: &str,
) -> Result<GraphqlSchemaEntry, String> {
    if schema_json.len() > MAX_SCHEMA_BYTES {
        return Err(format!(
            "E_TOO_LARGE: Schema 超过 {} MB",
            MAX_SCHEMA_BYTES / 1024 / 1024
        ));
    }
    let now = now_ms();
    conn.execute(
        "INSERT INTO graphql_schemas (project_id, endpoint, schema_json, fetched_at)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(project_id, endpoint)
         DO UPDATE SET schema_json = excluded.schema_json, fetched_at = excluded.fetched_at",
        params![project_id, endpoint.trim(), schema_json, now],
    )
    .map_err(err)?;
    find(conn, project_id, endpoint)?.ok_or_else(|| "Schema 保存失败".to_string())
}

pub fn find(
    conn: &Connection,
    project_id: i64,
    endpoint: &str,
) -> Result<Option<GraphqlSchemaEntry>, String> {
    conn.query_row(
        "SELECT id, project_id, endpoint, schema_json, fetched_at
         FROM graphql_schemas WHERE project_id = ?1 AND endpoint = ?2",
        params![project_id, endpoint.trim()],
        |row| {
            Ok(GraphqlSchemaEntry {
                id: row.get(0)?,
                project_id: row.get(1)?,
                endpoint: row.get(2)?,
                schema_json: row.get(3)?,
                fetched_at: row.get(4)?,
            })
        },
    )
    .optional()
    .map_err(err)
}

pub fn list(conn: &Connection, project_id: i64) -> Result<Vec<GraphqlSchemaMeta>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, project_id, endpoint, fetched_at, LENGTH(schema_json)
             FROM graphql_schemas WHERE project_id = ?1 ORDER BY fetched_at DESC, id DESC",
        )
        .map_err(err)?;
    let rows = stmt
        .query_map(params![project_id], |row| {
            Ok(GraphqlSchemaMeta {
                id: row.get(0)?,
                project_id: row.get(1)?,
                endpoint: row.get(2)?,
                fetched_at: row.get(3)?,
                bytes: row.get::<_, i64>(4).unwrap_or(0) as usize,
            })
        })
        .map_err(err)?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(err)
}

pub fn delete(conn: &Connection, project_id: i64, endpoint: &str) -> Result<bool, String> {
    let affected = conn
        .execute(
            "DELETE FROM graphql_schemas WHERE project_id = ?1 AND endpoint = ?2",
            params![project_id, endpoint.trim()],
        )
        .map_err(err)?;
    Ok(affected > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use r2d2_sqlite::SqliteConnectionManager;

    fn pool(tag: &str) -> crate::state::DbPool {
        let dir = crate::util::test_dir(tag);
        let manager = SqliteConnectionManager::file(dir.join("astroman.db"));
        let pool = r2d2::Pool::builder().max_size(2).build(manager).unwrap();
        db::init(&pool).unwrap();
        pool
    }

    fn project(conn: &Connection) -> i64 {
        conn.execute(
            "INSERT INTO projects (name, created_at, updated_at) VALUES ('P', 0, 0)",
            [],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    #[test]
    fn saves_and_updates_schema_by_endpoint() {
        let pool = pool("graphql_schema_save");
        let conn = pool.get().unwrap();
        let project = project(&conn);
        let first = save(&conn, project, "https://example.com/graphql", "{\"a\":1}").unwrap();
        assert_eq!(first.project_id, project);
        let second = save(&conn, project, "https://example.com/graphql", "{\"a\":2}").unwrap();
        assert_eq!(first.id, second.id);
        assert_eq!(second.schema_json, "{\"a\":2}");
        assert_eq!(list(&conn, project).unwrap().len(), 1);
    }

    #[test]
    fn rejects_oversized_schema() {
        let pool = pool("graphql_schema_large");
        let conn = pool.get().unwrap();
        let project = project(&conn);
        let huge = "x".repeat(MAX_SCHEMA_BYTES + 1);
        assert!(save(&conn, project, "https://example.com/graphql", &huge)
            .unwrap_err()
            .contains("E_TOO_LARGE"));
    }

    #[test]
    fn deletes_by_endpoint() {
        let pool = pool("graphql_schema_delete");
        let conn = pool.get().unwrap();
        let project = project(&conn);
        save(&conn, project, "https://example.com/graphql", "{}").unwrap();
        assert!(delete(&conn, project, "https://example.com/graphql").unwrap());
        assert!(find(&conn, project, "https://example.com/graphql")
            .unwrap()
            .is_none());
    }
}
