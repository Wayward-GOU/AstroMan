use crate::models::{TestCase, TestCasePayload};
use crate::state::AppState;
use crate::util::{err, now_ms};
use rusqlite::{params, Connection, Row};
use std::collections::HashSet;
use tauri::State;

const CASE_COLUMNS: &str = "id, suite_id, name, kind, request_id, plan_id, dataset_id, depends_on, \
                            sort_order, enabled, timeout_ms, retries, retry_on_failure, \
                            continue_on_failure, tags, variables_json, assertions_json, \
                            pre_script, post_script, main_script, stress_json, created_at, updated_at";

const CASE_KINDS: [&str; 3] = ["request", "python", "stress"];

fn map_case(row: &Row<'_>) -> rusqlite::Result<TestCase> {
    Ok(TestCase {
        id: row.get(0)?,
        suite_id: row.get(1)?,
        name: row.get(2)?,
        kind: row.get(3)?,
        request_id: row.get(4)?,
        plan_id: row.get(5)?,
        dataset_id: row.get(6)?,
        depends_on: row.get(7)?,
        sort_order: row.get(8)?,
        enabled: row.get::<_, i64>(9)? != 0,
        timeout_ms: row.get(10)?,
        retries: row.get(11)?,
        retry_on_failure: row.get::<_, i64>(12)? != 0,
        continue_on_failure: row.get::<_, i64>(13)? != 0,
        tags: row.get(14)?,
        variables_json: row.get(15)?,
        assertions_json: row.get(16)?,
        pre_script: row.get(17)?,
        post_script: row.get(18)?,
        main_script: row.get(19)?,
        stress_json: row.get(20)?,
        created_at: row.get(21)?,
        updated_at: row.get(22)?,
    })
}

pub(crate) fn load_cases(conn: &Connection, suite_id: i64) -> Result<Vec<TestCase>, String> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {CASE_COLUMNS} FROM test_cases WHERE suite_id = ?1
             ORDER BY sort_order ASC, id ASC"
        ))
        .map_err(err)?;
    let rows = stmt.query_map(params![suite_id], map_case).map_err(err)?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(err)
}

fn find_case(conn: &Connection, id: i64) -> Result<TestCase, String> {
    conn.query_row(
        &format!("SELECT {CASE_COLUMNS} FROM test_cases WHERE id = ?1"),
        params![id],
        map_case,
    )
    .map_err(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => "测试用例不存在".to_string(),
        other => err(other),
    })
}

/// 依赖字段是逗号分隔的用例 ID；这里只做格式校验，环检测放到预检（需要整张套件的视图）
fn parse_depends(raw: Option<&str>, self_id: Option<i64>) -> Result<Option<String>, String> {
    let Some(text) = raw else { return Ok(None) };
    let mut ids: Vec<String> = Vec::new();
    let mut seen = HashSet::new();
    for token in text.split(',') {
        let token = token.trim();
        if token.is_empty() {
            continue;
        }
        let id: i64 = token
            .parse()
            .map_err(|_| format!("依赖项必须是用例 ID，收到：{token}"))?;
        if Some(id) == self_id {
            return Err("用例不能依赖自己".to_string());
        }
        if !seen.insert(id) {
            continue;
        }
        ids.push(id.to_string());
    }
    Ok(if ids.is_empty() {
        None
    } else {
        Some(ids.join(","))
    })
}

#[tauri::command]
pub fn test_case_list(state: State<'_, AppState>, suite_id: i64) -> Result<Vec<TestCase>, String> {
    let conn = state.pool.get().map_err(err)?;
    load_cases(&conn, suite_id)
}

#[tauri::command]
pub fn test_case_get(state: State<'_, AppState>, id: i64) -> Result<TestCase, String> {
    let conn = state.pool.get().map_err(err)?;
    find_case(&conn, id)
}

#[tauri::command]
pub fn test_case_save(state: State<'_, AppState>, payload: TestCasePayload) -> Result<i64, String> {
    let name = payload.name.trim().to_string();
    if name.is_empty() {
        return Err("用例名称不能为空".to_string());
    }
    let kind = payload.kind.trim().to_ascii_lowercase();
    if !CASE_KINDS.contains(&kind.as_str()) {
        return Err("用例类型只支持 request / python / stress".to_string());
    }
    match kind.as_str() {
        "request" if payload.request_id.is_none() => {
            return Err("HTTP 用例必须绑定一个接口".to_string());
        }
        "python"
            if payload
                .main_script
                .as_deref()
                .unwrap_or("")
                .trim()
                .is_empty() =>
        {
            return Err("Python 用例必须有脚本内容".to_string());
        }
        "stress"
            if payload
                .stress_json
                .as_deref()
                .unwrap_or("")
                .trim()
                .is_empty() =>
        {
            return Err("压测用例必须有压测配置快照".to_string());
        }
        _ => {}
    }
    let timeout_ms = payload.timeout_ms.clamp(500, 24 * 60 * 60 * 1000);
    let retries = payload.retries.clamp(0, 10);
    let depends_on = parse_depends(payload.depends_on.as_deref(), payload.id)?;

    let conn = state.pool.get().map_err(err)?;
    let now = now_ms();

    let id = match payload.id {
        Some(id) => {
            find_case(&conn, id)?;
            conn.execute(
                "UPDATE test_cases
                 SET name = ?1, kind = ?2, request_id = ?3, plan_id = ?4, dataset_id = ?5,
                     depends_on = ?6, sort_order = ?7, enabled = ?8, timeout_ms = ?9,
                     retries = ?10, retry_on_failure = ?11, continue_on_failure = ?12,
                     tags = ?13, variables_json = ?14, assertions_json = ?15, pre_script = ?16,
                     post_script = ?17, main_script = ?18, stress_json = ?19, updated_at = ?20
                 WHERE id = ?21",
                params![
                    name,
                    kind,
                    payload.request_id,
                    payload.plan_id,
                    payload.dataset_id,
                    depends_on,
                    payload.sort_order,
                    payload.enabled as i64,
                    timeout_ms,
                    retries,
                    payload.retry_on_failure as i64,
                    payload.continue_on_failure as i64,
                    payload.tags,
                    payload.variables_json,
                    payload.assertions_json,
                    payload.pre_script,
                    payload.post_script,
                    payload.main_script,
                    payload.stress_json,
                    now,
                    id
                ],
            )
            .map_err(err)?;
            id
        }
        None => {
            // sort_order 未指定时追加到末尾，避免新用例全部挤在 0
            let sort_order = if payload.sort_order > 0 {
                payload.sort_order
            } else {
                conn.query_row(
                    "SELECT IFNULL(MAX(sort_order), 0) + 1 FROM test_cases WHERE suite_id = ?1",
                    params![payload.suite_id],
                    |row| row.get::<_, i64>(0),
                )
                .map_err(err)?
            };
            conn.execute(
                "INSERT INTO test_cases
                    (suite_id, name, kind, request_id, plan_id, dataset_id, depends_on,
                     sort_order, enabled, timeout_ms, retries, retry_on_failure,
                     continue_on_failure, tags, variables_json, assertions_json, pre_script,
                     post_script, main_script, stress_json, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15,
                         ?16, ?17, ?18, ?19, ?20, ?21, ?21)",
                params![
                    payload.suite_id,
                    name,
                    kind,
                    payload.request_id,
                    payload.plan_id,
                    payload.dataset_id,
                    depends_on,
                    sort_order,
                    payload.enabled as i64,
                    timeout_ms,
                    retries,
                    payload.retry_on_failure as i64,
                    payload.continue_on_failure as i64,
                    payload.tags,
                    payload.variables_json,
                    payload.assertions_json,
                    payload.pre_script,
                    payload.post_script,
                    payload.main_script,
                    payload.stress_json,
                    now
                ],
            )
            .map_err(|e| match e {
                rusqlite::Error::SqliteFailure(_, Some(message))
                    if message.contains("FOREIGN KEY") =>
                {
                    "所属套件、接口、计划或数据集不存在".to_string()
                }
                other => err(other),
            })?;
            conn.last_insert_rowid()
        }
    };
    Ok(id)
}

#[tauri::command]
pub fn test_case_delete(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    let conn = state.pool.get().map_err(err)?;
    conn.execute("DELETE FROM test_cases WHERE id = ?1", params![id])
        .map_err(err)?;
    Ok(())
}

/// 按传入顺序重排用例（拖拽排序）
#[tauri::command]
pub fn test_case_reorder(
    state: State<'_, AppState>,
    suite_id: i64,
    ids: Vec<i64>,
) -> Result<(), String> {
    let mut conn = state.pool.get().map_err(err)?;
    let tx = conn.transaction().map_err(err)?;
    for (index, id) in ids.iter().enumerate() {
        tx.execute(
            "UPDATE test_cases SET sort_order = ?1, updated_at = ?2 WHERE id = ?3 AND suite_id = ?4",
            params![index as i64 + 1, now_ms(), id, suite_id],
        )
        .map_err(err)?;
    }
    tx.commit().map_err(err)?;
    Ok(())
}
