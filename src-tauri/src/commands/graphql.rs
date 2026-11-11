//! GraphQL Schema 感知命令（G-10）：核心逻辑在 `services::graphql`。

use std::collections::HashMap;
use std::time::Duration;

use tauri::State;

use crate::models::KeyValueItem;
use crate::services::graphql::{
    self, GraphqlIntrospectPayload, GraphqlSchemaEntry, GraphqlSchemaMeta,
};
use crate::state::AppState;
use crate::util::{err, render};

fn render_headers(
    headers: &[KeyValueItem],
    vars: &HashMap<String, String>,
) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for item in headers {
        if !item.enabled {
            continue;
        }
        let key = render(item.key.trim(), vars);
        if key.is_empty() {
            continue;
        }
        out.push((key, render(&item.value, vars)));
    }
    out
}

#[tauri::command]
pub async fn graphql_introspect(
    state: State<'_, AppState>,
    payload: GraphqlIntrospectPayload,
) -> Result<GraphqlSchemaEntry, String> {
    let vars = match payload.env_id {
        Some(env_id) => {
            let conn = state.pool.get().map_err(err)?;
            crate::commands::http::load_vars(&conn, env_id)?
        }
        None => HashMap::new(),
    };
    let endpoint = render(payload.endpoint.trim(), &vars);
    let mut endpoint = endpoint.trim().to_string();
    if !endpoint.contains("://") {
        endpoint = format!("http://{endpoint}");
    }
    let headers = render_headers(&payload.headers, &vars);
    let timeout = Duration::from_millis(payload.timeout_ms.unwrap_or(30_000).clamp(500, 120_000));
    let value = graphql::introspect(&state.client(false), &endpoint, &headers, timeout).await?;
    let schema_json = serde_json::to_string(&value).map_err(err)?;
    let conn = state.pool.get().map_err(err)?;
    graphql::save(&conn, payload.project_id, &endpoint, &schema_json)
}

#[tauri::command]
pub fn graphql_schema_get(
    state: State<'_, AppState>,
    project_id: i64,
    endpoint: String,
) -> Result<Option<GraphqlSchemaEntry>, String> {
    let conn = state.pool.get().map_err(err)?;
    graphql::find(&conn, project_id, &endpoint)
}

#[tauri::command]
pub fn graphql_schema_list(
    state: State<'_, AppState>,
    project_id: i64,
) -> Result<Vec<GraphqlSchemaMeta>, String> {
    let conn = state.pool.get().map_err(err)?;
    graphql::list(&conn, project_id)
}

#[tauri::command]
pub fn graphql_schema_delete(
    state: State<'_, AppState>,
    project_id: i64,
    endpoint: String,
) -> Result<bool, String> {
    let conn = state.pool.get().map_err(err)?;
    graphql::delete(&conn, project_id, &endpoint)
}
