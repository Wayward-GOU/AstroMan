use crate::models::{RunDetail, RunOverrides, RunSummary};
use crate::state::AppState;
use crate::test_runner::{self, RunOptions, TestEvent};
use crate::util::err;
use rusqlite::params;
use tauri::{AppHandle, Emitter, Manager, State};

/// 启动套件运行：先落 queued 记录返回 runId，再在后台执行
#[tauri::command]
pub async fn test_run_start(
    app: AppHandle,
    state: State<'_, AppState>,
    suite_id: i64,
    env_id: Option<i64>,
    overrides: Option<RunOverrides>,
    trigger: Option<String>,
) -> Result<i64, String> {
    let trigger = trigger
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "ui".to_string());
    let overrides = overrides.unwrap_or_default();
    let run_id = test_runner::create_queued_run(&state, suite_id, &trigger)?;

    let options = RunOptions {
        trigger,
        env_id: overrides.env_id.or(env_id),
        vars: overrides.vars.unwrap_or_default(),
        secrets: overrides.secrets.unwrap_or_default(),
        fail_fast: overrides.fail_fast,
        ..RunOptions::default()
    };

    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = handle.state::<AppState>();
        let emit = |event: TestEvent| {
            let _ = handle.emit(event.name, event.payload);
        };
        if let Err(error) = test_runner::run_suite(&state, suite_id, run_id, options, &emit).await {
            log::error!("套件运行失败（run {run_id}）：{error}");
            if let Ok(conn) = state.pool.get() {
                let _ = conn.execute(
                    "UPDATE test_runs SET status = 'error', finished_at = ?1,
                         summary_json = ?2 WHERE id = ?3",
                    params![
                        crate::util::now_ms(),
                        serde_json::json!({ "error": error }).to_string(),
                        run_id
                    ],
                );
            }
            state.test_release(run_id);
            let _ = handle.emit(
                "test:run:done",
                serde_json::json!({
                    "runId": run_id,
                    "status": "error",
                    "error": error,
                }),
            );
        }
    });

    Ok(run_id)
}

#[tauri::command]
pub fn test_run_stop(state: State<'_, AppState>, run_id: i64) -> Result<(), String> {
    if state.test_cancel_run(run_id) {
        Ok(())
    } else {
        Err("该运行已结束或不存在".to_string())
    }
}

#[tauri::command]
pub fn test_run_get(state: State<'_, AppState>, run_id: i64) -> Result<RunDetail, String> {
    let conn = state.pool.get().map_err(err)?;
    test_runner::load_run_detail(&conn, run_id)
}

#[tauri::command]
pub fn test_run_list(
    state: State<'_, AppState>,
    suite_id: Option<i64>,
    page: Option<u32>,
    size: Option<u32>,
) -> Result<Vec<RunSummary>, String> {
    let conn = state.pool.get().map_err(err)?;
    let size = size.unwrap_or(20).clamp(1, 200) as i64;
    let offset = (page.unwrap_or(1).max(1) - 1) as i64 * size;
    let mut stmt = conn
        .prepare(
            "SELECT r.id, r.suite_id, s.name, r.env_name, r.trigger, r.status, r.total,
                    r.passed, r.failed, r.skipped, r.errored, r.duration_ms, r.started_at,
                    r.finished_at, r.created_at
             FROM test_runs r
             LEFT JOIN test_suites s ON s.id = r.suite_id
             WHERE (?1 IS NULL OR r.suite_id = ?1)
             ORDER BY r.id DESC LIMIT ?2 OFFSET ?3",
        )
        .map_err(err)?;
    let rows = stmt
        .query_map(params![suite_id, size, offset], |row| {
            Ok(RunSummary {
                id: row.get(0)?,
                suite_id: row.get(1)?,
                suite_name: row.get(2)?,
                env_name: row.get(3)?,
                trigger: row.get(4)?,
                status: row.get(5)?,
                total: row.get(6)?,
                passed: row.get(7)?,
                failed: row.get(8)?,
                skipped: row.get(9)?,
                errored: row.get(10)?,
                duration_ms: row.get(11)?,
                started_at: row.get(12)?,
                finished_at: row.get(13)?,
                created_at: row.get(14)?,
            })
        })
        .map_err(err)?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(err)
}

/// 窗口重开 / 事件断线时恢复当前运行进度；没有运行中的记录返回 null
#[tauri::command]
pub fn test_run_current(
    state: State<'_, AppState>,
    suite_id: Option<i64>,
) -> Result<Option<RunDetail>, String> {
    let conn = state.pool.get().map_err(err)?;
    let run_id: Option<i64> = conn
        .query_row(
            "SELECT id FROM test_runs
             WHERE status IN ('queued', 'running') AND (?1 IS NULL OR suite_id = ?1)
             ORDER BY id DESC LIMIT 1",
            params![suite_id],
            |row| row.get(0),
        )
        .ok();
    match run_id {
        Some(id) => Ok(Some(test_runner::load_run_detail(&conn, id)?)),
        None => Ok(None),
    }
}
