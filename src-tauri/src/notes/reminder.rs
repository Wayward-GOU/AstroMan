//! 提醒调度：应用内 30 秒轮询一次，只发应用内提醒卡片，不再调用系统通知。
//!
//! 暂停、免打扰、提醒关闭时整轮跳过且不写库，结束后只补发一条聚合提醒。

use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

use chrono::{Local, TimeZone};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::state::AppState;
use crate::util::{err, now_ms};

const FIRST_DELAY_SECS: u64 = 8;
const TICK_SECS: u64 = 30;
const CLEANUP_INTERVAL_MS: i64 = 24 * 60 * 60 * 1000;
/// 孤儿图片累计释放超过这个体积才补 VACUUM，避免每次清理都重写整库
const VACUUM_THRESHOLD: i64 = 32 * 1024 * 1024;

static LAST_CLEANUP: AtomicI64 = AtomicI64::new(0);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ReminderItem {
    id: i64,
    title: String,
    due_at: Option<i64>,
    overdue: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ReminderEvent {
    items: Vec<ReminderItem>,
    fired_at: i64,
}

pub fn spawn(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(FIRST_DELAY_SECS)).await;
        loop {
            tick(&app).await;
            tokio::time::sleep(Duration::from_secs(TICK_SECS)).await;
        }
    });
}

async fn tick(app: &AppHandle) {
    let now = now_ms();
    // 每日清理与提醒开关无关：软删除到期就该物理回收
    let cleanup_handle = app.clone();
    let _ = tauri::async_runtime::spawn_blocking(move || maybe_cleanup(&cleanup_handle, now)).await;

    let state = app.state::<AppState>();
    let settings = crate::notes::get_settings(&state.pool);
    if !settings.notify_enabled
        || crate::notes::pause_active(&settings, now)
        || crate::notes::is_quiet_now(&settings, now)
    {
        return;
    }

    let pool = state.pool.clone();
    let due =
        match tauri::async_runtime::spawn_blocking(move || crate::notes::due_notes(&pool, now))
            .await
        {
            Ok(Ok(list)) => list,
            Ok(Err(error)) => {
                log::warn!("查询到期待办失败：{error}");
                return;
            }
            Err(error) => {
                log::warn!("提醒查询任务失败：{error}");
                return;
            }
        };
    if due.is_empty() {
        return;
    }

    let pool = state.pool.clone();
    let batch = due.clone();
    match tauri::async_runtime::spawn_blocking(move || crate::notes::mark_fired(&pool, &batch, now))
        .await
    {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            log::warn!("标记提醒已送达失败：{error}");
            return;
        }
        Err(error) => {
            log::warn!("标记提醒任务失败：{error}");
            return;
        }
    }

    // 标记送达会清空 snooze_until / 推进重复周期，通知前端把「已稍后」等展示同步刷新
    let changed = serde_json::json!({ "reason": "update", "id": null });
    if let Err(error) = app.emit("notes:changed", changed) {
        log::warn!("发送待办变更事件失败：{error}");
    }

    // 只保留应用内卡片：窗口藏在托盘或最小化时先拉起来，否则卡片没有可见容器
    ensure_window_for_reminder(app);
    update_tray_tooltip(app, due[0].title.as_str(), now);
    request_attention(app);

    let event = ReminderEvent {
        items: due
            .iter()
            .map(|note| ReminderItem {
                id: note.id,
                title: note.title.clone(),
                due_at: note.due_at,
                overdue: note.due_at.map(|due_at| due_at < now).unwrap_or(false),
            })
            .collect(),
        fired_at: now,
    };
    if let Err(error) = app.emit("notes:reminder", event) {
        log::warn!("发送提醒事件失败：{error}");
    }
}

fn ensure_window_for_reminder(app: &AppHandle) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let hidden = !window.is_visible().unwrap_or(true);
    let minimized = window.is_minimized().unwrap_or(false);
    if hidden || minimized {
        let _ = window.show();
        let _ = window.unminimize();
    }
}

fn update_tray_tooltip(app: &AppHandle, title: &str, now: i64) {
    let Some(tray) = app.tray_by_id("main-tray") else {
        return;
    };
    let clock = Local
        .timestamp_millis_opt(now)
        .single()
        .map(|dt| dt.format("%H:%M").to_string());
    let text = match clock {
        Some(clock) => format!("{clock} {title}"),
        None => title.to_string(),
    };
    let _ = tray.set_tooltip(Some(text));
}

fn request_attention(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        if !window.is_focused().unwrap_or(true) {
            let _ = window.request_user_attention(Some(tauri::UserAttentionType::Informational));
        }
    }
}

fn maybe_cleanup(app: &AppHandle, now: i64) {
    let last = LAST_CLEANUP.load(Ordering::SeqCst);
    if last != 0 && now - last < CLEANUP_INTERVAL_MS {
        return;
    }
    LAST_CLEANUP.store(now, Ordering::SeqCst);

    let state = app.state::<AppState>();
    match crate::notes::cleanup_deleted(&state.pool) {
        Ok(result) => {
            if result.deleted_notes > 0 || result.freed_bytes > 0 {
                log::info!(
                    "待办清理：物理删除 {} 条，回收图片 {} 字节",
                    result.deleted_notes,
                    result.freed_bytes
                );
            }
            if result.freed_bytes > VACUUM_THRESHOLD {
                let _guard = state.heavy_ops.lock();
                match state.pool.get() {
                    Ok(conn) => {
                        if let Err(error) = conn.execute_batch("VACUUM") {
                            log::warn!("回收数据库空间失败：{}", err(error));
                        }
                    }
                    Err(error) => log::warn!("VACUUM 取连接失败：{}", err(error)),
                }
            }
        }
        Err(error) => log::warn!("清理已删除待办失败：{error}"),
    }
}
