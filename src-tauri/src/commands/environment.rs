use crate::models::{EnvVariable, Environment};
use crate::state::AppState;
use crate::util::{err, now_ms};
use rusqlite::{params, Connection, Row};
use tauri::State;

fn map_variable(row: &Row<'_>) -> rusqlite::Result<EnvVariable> {
    Ok(EnvVariable {
        id: Some(row.get(0)?),
        key: row.get(1)?,
        value: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
        is_secret: row.get::<_, i64>(3)? != 0,
        sort_order: row.get(4)?,
    })
}

fn load_variables(conn: &Connection, env_id: i64) -> Result<Vec<EnvVariable>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, \"key\", value, is_secret, sort_order
             FROM env_variables WHERE env_id = ?1 ORDER BY sort_order ASC, id ASC",
        )
        .map_err(err)?;
    let rows = stmt.query_map(params![env_id], map_variable).map_err(err)?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(err)
}

#[tauri::command]
pub fn env_list(state: State<'_, AppState>, project_id: i64) -> Result<Vec<Environment>, String> {
    let conn = state.pool.get().map_err(err)?;
    let mut stmt = conn
        .prepare(
            "SELECT id, project_id, name, is_active, COALESCE(script, '')
             FROM environments WHERE project_id = ?1 ORDER BY id ASC",
        )
        .map_err(err)?;
    let rows = stmt
        .query_map(params![project_id], |row| {
            Ok(Environment {
                id: row.get(0)?,
                project_id: row.get(1)?,
                name: row.get(2)?,
                is_active: row.get::<_, i64>(3)? != 0,
                script: row.get(4)?,
                variables: Vec::new(),
            })
        })
        .map_err(err)?;

    let mut list = Vec::new();
    for item in rows {
        let mut env = item.map_err(err)?;
        env.variables = load_variables(&conn, env.id)?;
        list.push(env);
    }
    Ok(list)
}

#[tauri::command]
pub fn env_create(
    state: State<'_, AppState>,
    project_id: i64,
    name: String,
) -> Result<Environment, String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("环境名称不能为空".to_string());
    }

    let conn = state.pool.get().map_err(err)?;
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM environments WHERE project_id = ?1",
            params![project_id],
            |row| row.get(0),
        )
        .map_err(err)?;
    let is_active = count == 0;

    conn.execute(
        "INSERT INTO environments (project_id, name, is_active, script, created_at)
         VALUES (?1, ?2, ?3, '', ?4)",
        params![project_id, name, if is_active { 1 } else { 0 }, now_ms()],
    )
    .map_err(err)?;

    Ok(Environment {
        id: conn.last_insert_rowid(),
        project_id,
        name,
        is_active,
        script: String::new(),
        variables: Vec::new(),
    })
}

/// 保存环境脚本：每次发送请求前执行，可用 vars.set() 生成运行时变量
#[tauri::command]
pub fn env_script_save(state: State<'_, AppState>, id: i64, script: String) -> Result<(), String> {
    let conn = state.pool.get().map_err(err)?;
    let changed = conn
        .execute(
            "UPDATE environments SET script = ?1 WHERE id = ?2",
            params![script, id],
        )
        .map_err(err)?;
    if changed == 0 {
        return Err("环境不存在".to_string());
    }
    Ok(())
}

#[tauri::command]
pub fn env_activate(
    state: State<'_, AppState>,
    project_id: i64,
    id: i64,
) -> Result<(), String> {
    let conn = state.pool.get().map_err(err)?;

    let owner: i64 = conn
        .query_row(
            "SELECT project_id FROM environments WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => "环境不存在".to_string(),
            other => err(other),
        })?;
    if owner != project_id {
        return Err("环境不属于当前项目".to_string());
    }

    conn.execute(
        "UPDATE environments SET is_active = CASE WHEN id = ?1 THEN 1 ELSE 0 END
         WHERE project_id = ?2",
        params![id, project_id],
    )
    .map_err(err)?;
    Ok(())
}

#[tauri::command]
pub fn env_delete(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    let conn = state.pool.get().map_err(err)?;

    let project_id: i64 = match conn.query_row(
        "SELECT project_id FROM environments WHERE id = ?1",
        params![id],
        |row| row.get(0),
    ) {
        Ok(value) => value,
        Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(()),
        Err(e) => return Err(err(e)),
    };

    conn.execute("DELETE FROM environments WHERE id = ?1", params![id])
        .map_err(err)?;

    let active: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM environments WHERE project_id = ?1 AND is_active = 1",
            params![project_id],
            |row| row.get(0),
        )
        .map_err(err)?;
    if active == 0 {
        conn.execute(
            "UPDATE environments SET is_active = 1
             WHERE id = (SELECT id FROM environments WHERE project_id = ?1 ORDER BY id ASC LIMIT 1)",
            params![project_id],
        )
        .map_err(err)?;
    }

    Ok(())
}

#[tauri::command]
pub fn env_var_save(
    state: State<'_, AppState>,
    env_id: i64,
    variables: Vec<EnvVariable>,
) -> Result<(), String> {
    let mut conn = state.pool.get().map_err(err)?;
    let tx = conn.transaction().map_err(err)?;

    tx.execute("DELETE FROM env_variables WHERE env_id = ?1", params![env_id])
        .map_err(err)?;

    {
        let mut stmt = tx
            .prepare(
                "INSERT INTO env_variables (env_id, \"key\", value, is_secret, sort_order)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
            )
            .map_err(err)?;

        for (index, item) in variables.iter().enumerate() {
            let key = item.key.trim();
            if key.is_empty() {
                continue;
            }
            stmt.execute(params![
                env_id,
                key,
                item.value,
                if item.is_secret { 1 } else { 0 },
                index as i64
            ])
            .map_err(err)?;
        }
    }

    tx.commit().map_err(err)?;
    Ok(())
}
