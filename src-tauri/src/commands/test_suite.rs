use crate::commands::test_case::load_cases;
use crate::models::{
    SuiteDetail, SuiteSummary, TestDataset, TestDatasetPayload, TestSuite, TestSuitePayload,
};
use crate::state::AppState;
use crate::util::{err, now_ms};
use rusqlite::{params, Connection, OptionalExtension, Row};
use tauri::State;

const SUITE_COLUMNS: &str = "id, project_id, name, description, env_id, tags, python_path, \
                             config_json, setup_script, teardown_script, created_at, updated_at";

fn map_suite(row: &Row<'_>) -> rusqlite::Result<TestSuite> {
    Ok(TestSuite {
        id: row.get(0)?,
        project_id: row.get(1)?,
        name: row.get(2)?,
        description: row.get(3)?,
        env_id: row.get(4)?,
        tags: row.get(5)?,
        python_path: row.get(6)?,
        config_json: row.get(7)?,
        setup_script: row.get(8)?,
        teardown_script: row.get(9)?,
        created_at: row.get(10)?,
        updated_at: row.get(11)?,
    })
}

pub(crate) fn find_suite(conn: &Connection, id: i64) -> Result<TestSuite, String> {
    conn.query_row(
        &format!("SELECT {SUITE_COLUMNS} FROM test_suites WHERE id = ?1"),
        params![id],
        map_suite,
    )
    .map_err(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => "测试套件不存在".to_string(),
        other => err(other),
    })
}

/// 逗号分隔标签的匹配：带 `%` 的模糊或整词匹配都按包含处理
fn tag_filter(tag: &str) -> String {
    format!("%{}%", tag.trim().replace('%', "").replace('_', ""))
}

#[tauri::command]
pub fn test_suite_list(
    state: State<'_, AppState>,
    project_id: Option<i64>,
    keyword: Option<String>,
    tag: Option<String>,
) -> Result<Vec<SuiteSummary>, String> {
    let conn = state.pool.get().map_err(err)?;
    let keyword = keyword
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let tag = tag
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    let mut stmt = conn
        .prepare(
            "SELECT s.id, s.project_id, s.name, s.description, s.env_id, e.name, s.tags,
                    s.python_path, s.config_json,
                    (SELECT COUNT(*) FROM test_cases c WHERE c.suite_id = s.id),
                    s.created_at, s.updated_at
             FROM test_suites s
             LEFT JOIN environments e ON e.id = s.env_id
             WHERE (?1 IS NULL OR s.project_id = ?1)
               AND (?2 IS NULL OR s.name LIKE ?2 OR IFNULL(s.description, '') LIKE ?2)
               AND (?3 IS NULL OR IFNULL(s.tags, '') LIKE ?3)
             ORDER BY s.updated_at DESC, s.id DESC",
        )
        .map_err(err)?;

    let rows = stmt
        .query_map(
            params![
                project_id,
                keyword.map(|value| format!("%{value}%")),
                tag.map(|value| tag_filter(&value))
            ],
            |row| {
                Ok(SuiteSummary {
                    id: row.get(0)?,
                    project_id: row.get(1)?,
                    name: row.get(2)?,
                    description: row.get(3)?,
                    env_id: row.get(4)?,
                    env_name: row.get(5)?,
                    tags: row.get(6)?,
                    python_path: row.get(7)?,
                    config_json: row.get(8)?,
                    case_count: row.get(9)?,
                    created_at: row.get(10)?,
                    updated_at: row.get(11)?,
                })
            },
        )
        .map_err(err)?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(err)
}

#[tauri::command]
pub fn test_suite_get(state: State<'_, AppState>, id: i64) -> Result<SuiteDetail, String> {
    let conn = state.pool.get().map_err(err)?;
    let suite = find_suite(&conn, id)?;
    let cases = load_cases(&conn, id)?;
    let datasets = load_datasets(&conn, id, false)?;
    Ok(SuiteDetail {
        suite,
        cases,
        datasets,
    })
}

#[tauri::command]
pub fn test_suite_save(state: State<'_, AppState>, payload: TestSuitePayload) -> Result<i64, String> {
    let name = payload.name.trim().to_string();
    if name.is_empty() {
        return Err("套件名称不能为空".to_string());
    }
    let conn = state.pool.get().map_err(err)?;
    let now = now_ms();

    let id = match payload.id {
        Some(id) => {
            find_suite(&conn, id)?;
            conn.execute(
                "UPDATE test_suites
                 SET name = ?1, description = ?2, env_id = ?3, tags = ?4, python_path = ?5,
                     config_json = ?6, setup_script = ?7, teardown_script = ?8, updated_at = ?9
                 WHERE id = ?10",
                params![
                    name,
                    payload.description,
                    payload.env_id,
                    payload.tags,
                    payload.python_path,
                    payload.config_json,
                    payload.setup_script,
                    payload.teardown_script,
                    now,
                    id
                ],
            )
            .map_err(err)?;
            id
        }
        None => {
            conn.execute(
                "INSERT INTO test_suites
                    (project_id, name, description, env_id, tags, python_path, config_json,
                     setup_script, teardown_script, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)",
                params![
                    payload.project_id,
                    name,
                    payload.description,
                    payload.env_id,
                    payload.tags,
                    payload.python_path,
                    payload.config_json,
                    payload.setup_script,
                    payload.teardown_script,
                    now
                ],
            )
            .map_err(|e| match e {
                rusqlite::Error::SqliteFailure(_, Some(message))
                    if message.contains("FOREIGN KEY") =>
                {
                    "所属项目不存在".to_string()
                }
                other => err(other),
            })?;
            conn.last_insert_rowid()
        }
    };
    Ok(id)
}

#[tauri::command]
pub fn test_suite_delete(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    let conn = state.pool.get().map_err(err)?;
    conn.execute("DELETE FROM test_suites WHERE id = ?1", params![id])
        .map_err(err)?;
    Ok(())
}

// ---------- 数据集 ----------

fn map_dataset(row: &Row<'_>) -> rusqlite::Result<TestDataset> {
    Ok(TestDataset {
        id: row.get(0)?,
        suite_id: row.get(1)?,
        name: row.get(2)?,
        format: row.get(3)?,
        path: row.get(4)?,
        content: row.get(5)?,
        row_count: row.get(6)?,
        sha256: row.get(7)?,
        created_at: row.get(8)?,
    })
}

/// `with_content = false` 时不取内容，避免列表接口返回大段 CSV
pub(crate) fn load_datasets(
    conn: &Connection,
    suite_id: i64,
    with_content: bool,
) -> Result<Vec<TestDataset>, String> {
    let content = if with_content { "content" } else { "NULL" };
    let mut stmt = conn
        .prepare(&format!(
            "SELECT id, suite_id, name, format, path, {content}, row_count, sha256, created_at
             FROM test_datasets WHERE suite_id = ?1 ORDER BY id ASC"
        ))
        .map_err(err)?;
    let rows = stmt.query_map(params![suite_id], map_dataset).map_err(err)?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(err)
}

fn find_dataset(conn: &Connection, id: i64) -> Result<TestDataset, String> {
    conn.query_row(
        "SELECT id, suite_id, name, format, path, content, row_count, sha256, created_at
         FROM test_datasets WHERE id = ?1",
        params![id],
        map_dataset,
    )
    .map_err(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => "数据集不存在".to_string(),
        other => err(other),
    })
}

#[tauri::command]
pub fn test_dataset_list(
    state: State<'_, AppState>,
    suite_id: i64,
) -> Result<Vec<TestDataset>, String> {
    let conn = state.pool.get().map_err(err)?;
    load_datasets(&conn, suite_id, false)
}

#[tauri::command]
pub fn test_dataset_get(state: State<'_, AppState>, id: i64) -> Result<TestDataset, String> {
    let conn = state.pool.get().map_err(err)?;
    find_dataset(&conn, id)
}

#[tauri::command]
pub fn test_dataset_save(
    state: State<'_, AppState>,
    payload: TestDatasetPayload,
) -> Result<i64, String> {
    let name = payload.name.trim().to_string();
    if name.is_empty() {
        return Err("数据集名称不能为空".to_string());
    }
    let format = payload.format.trim().to_ascii_lowercase();
    if format != "csv" && format != "json" {
        return Err("数据集格式只支持 csv / json".to_string());
    }
    if payload.path.as_deref().unwrap_or("").trim().is_empty()
        && payload.content.as_deref().unwrap_or("").trim().is_empty()
    {
        return Err("数据集需要文件路径或内容".to_string());
    }

    let conn = state.pool.get().map_err(err)?;
    let now = now_ms();
    let path = payload.path.map(|value| value.trim().to_string());
    let sha256 = payload
        .content
        .as_deref()
        .map(|text| crate::commands::tools::sha256_hex(text.as_bytes()));

    let id = match payload.id {
        Some(id) => {
            find_dataset(&conn, id)?;
            conn.execute(
                "UPDATE test_datasets
                 SET name = ?1, format = ?2, path = ?3, content = ?4, sha256 = ?5
                 WHERE id = ?6",
                params![name, format, path, payload.content, sha256, id],
            )
            .map_err(err)?;
            id
        }
        None => {
            conn.execute(
                "INSERT INTO test_datasets (suite_id, name, format, path, content, sha256, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    payload.suite_id,
                    name,
                    format,
                    path,
                    payload.content,
                    sha256,
                    now
                ],
            )
            .map_err(|e| match e {
                rusqlite::Error::SqliteFailure(_, Some(message))
                    if message.contains("FOREIGN KEY") =>
                {
                    "所属套件不存在".to_string()
                }
                other => err(other),
            })?;
            conn.last_insert_rowid()
        }
    };
    Ok(id)
}

#[tauri::command]
pub fn test_dataset_delete(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    let conn = state.pool.get().map_err(err)?;
    let path: Option<String> = conn
        .query_row(
            "SELECT path FROM test_datasets WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )
        .optional()
        .map_err(err)?
        .flatten();
    conn.execute("DELETE FROM test_datasets WHERE id = ?1", params![id])
        .map_err(err)?;
    // 复制到数据目录下的数据集文件随记录一起删除；外部路径不动
    if let Some(relative) = path {
        if relative.starts_with("test-datasets/") {
            let full = state.data_dir.join(&relative);
            if let Err(error) = std::fs::remove_file(&full) {
                log::warn!("删除数据集文件失败（{}）：{error}", full.display());
            }
        }
    }
    Ok(())
}
