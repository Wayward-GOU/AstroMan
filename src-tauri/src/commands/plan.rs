use crate::state::AppState;
use crate::util::{err, now_ms};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::State;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanSummary {
    pub id: i64,
    pub project_id: i64,
    pub request_id: Option<i64>,
    pub request_name: Option<String>,
    pub name: String,
    pub updated_at: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestPlan {
    pub id: i64,
    pub project_id: i64,
    pub request_id: Option<i64>,
    pub name: String,
    pub config: Value,
    pub updated_at: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanInput {
    #[serde(default)]
    pub id: Option<i64>,
    pub project_id: i64,
    #[serde(default)]
    pub request_id: Option<i64>,
    pub name: String,
    #[serde(default)]
    pub config: Value,
}

#[tauri::command]
pub fn plan_list(
    state: State<'_, AppState>,
    project_id: i64,
) -> Result<Vec<PlanSummary>, String> {
    let conn = state.pool.get().map_err(err)?;
    let mut stmt = conn
        .prepare(
            "SELECT p.id, p.project_id, p.request_id, r.name, p.name, p.updated_at
             FROM test_plans p
             LEFT JOIN requests r ON r.id = p.request_id
             WHERE p.project_id = ?1
             ORDER BY p.updated_at DESC, p.id DESC",
        )
        .map_err(err)?;
    let rows = stmt
        .query_map(params![project_id], |row| {
            Ok(PlanSummary {
                id: row.get(0)?,
                project_id: row.get(1)?,
                request_id: row.get(2)?,
                request_name: row.get(3)?,
                name: row.get(4)?,
                updated_at: row.get(5)?,
            })
        })
        .map_err(err)?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(err)
}

#[tauri::command]
pub fn plan_get(state: State<'_, AppState>, id: i64) -> Result<TestPlan, String> {
    let conn = state.pool.get().map_err(err)?;
    conn.query_row(
        "SELECT id, project_id, request_id, name, config_json, updated_at
         FROM test_plans WHERE id = ?1",
        params![id],
        |row| {
            let raw: Option<String> = row.get(4)?;
            let config = raw
                .and_then(|text| serde_json::from_str::<Value>(&text).ok())
                .unwrap_or(Value::Null);
            Ok(TestPlan {
                id: row.get(0)?,
                project_id: row.get(1)?,
                request_id: row.get(2)?,
                name: row.get(3)?,
                config,
                updated_at: row.get(5)?,
            })
        },
    )
    .map_err(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => "压测计划不存在".to_string(),
        other => err(other),
    })
}

#[tauri::command]
pub fn plan_save(state: State<'_, AppState>, plan: PlanInput) -> Result<TestPlan, String> {
    let name = plan.name.trim();
    if name.is_empty() {
        return Err("请填写计划名称".to_string());
    }
    let config = serde_json::to_string(&plan.config).map_err(err)?;
    let now = now_ms();
    let conn = state.pool.get().map_err(err)?;

    let id = match plan.id {
        Some(id) => {
            conn.execute(
                "UPDATE test_plans SET request_id = ?1, name = ?2, config_json = ?3, updated_at = ?4
                 WHERE id = ?5",
                params![plan.request_id, name, config, now, id],
            )
            .map_err(err)?;
            id
        }
        None => {
            conn.execute(
                "INSERT INTO test_plans (project_id, request_id, name, config_json, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
                params![plan.project_id, plan.request_id, name, config, now],
            )
            .map_err(err)?;
            conn.last_insert_rowid()
        }
    };

    drop(conn);
    plan_get(state, id)
}

#[tauri::command]
pub fn plan_delete(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    let conn = state.pool.get().map_err(err)?;
    conn.execute("DELETE FROM test_plans WHERE id = ?1", params![id])
        .map_err(err)?;
    Ok(())
}
