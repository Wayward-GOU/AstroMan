mod backup;
mod capture;
pub mod cli;
mod commands;
mod db;
mod logging;
mod models;
mod net_probe;
mod netinfo;
mod notes;
mod prompts;
mod python_exec;
mod report_builder;
mod secret;
mod state;
mod test_assert;
mod test_dataset;
mod test_runner;
mod util;

use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Emitter, Manager,
};

fn show_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// 按屏幕缩放比例挑最贴合托盘槽位的图标尺寸，避免系统缩放把图标糊掉或显小。
/// Windows 托盘在 100% 缩放下是 16px、150% 是 24px、200% 是 32px。
fn tray_icon_bytes(scale: f64) -> &'static [u8] {
    if scale <= 1.1 {
        include_bytes!("../icons/tray-16.png")
    } else if scale <= 1.75 {
        include_bytes!("../icons/tray.png")
    } else {
        include_bytes!("../icons/tray@2x.png")
    }
}

/// 托盘菜单里暂停待办提醒，并同步托盘 tooltip
fn pause_notes(app: &tauri::AppHandle, minutes: i64) {
    let state = app.state::<state::AppState>();
    let mut settings = notes::get_settings(&state.pool);
    let now = util::now_ms();
    settings.pause_until = if minutes <= 0 {
        notes::start_of_tomorrow(now)
    } else {
        now + minutes * 60_000
    };
    if let Err(error) = notes::set_settings(&state.pool, &settings) {
        log::warn!("暂停待办提醒失败：{error}");
        return;
    }
    if let Some(tray) = app.tray_by_id("main-tray") {
        let _ = tray.set_tooltip(Some("AstroMan · 提醒已暂停"));
    }
    let _ = app.emit("notes:changed", serde_json::json!({ "reason": "settings" }));
}

/// 系统托盘：左键点击显示主窗口，右键菜单可显示或退出
fn setup_tray(app: &tauri::AppHandle) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "显示主窗口", true, None::<&str>)?;
    let open_notes = MenuItem::with_id(app, "open-notes", "打开记事本", true, None::<&str>)?;
    let quick = MenuItem::with_id(app, "notes-quick", "快速记录", true, None::<&str>)?;
    let pause_hour = MenuItem::with_id(
        app,
        "notes-pause-hour",
        "暂停提醒 1 小时",
        true,
        None::<&str>,
    )?;
    let pause_today = MenuItem::with_id(
        app,
        "notes-pause-today",
        "暂停提醒到今天结束",
        true,
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(app, "quit", "退出 AstroMan", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[&show, &open_notes, &quick, &pause_hour, &pause_today, &quit],
    )?;

    // 主窗口还没建好时退回主显示器缩放，保证始终能拿到接近托盘槽位的尺寸
    let scale = app
        .get_webview_window("main")
        .and_then(|window| window.scale_factor().ok())
        .or_else(|| {
            app.primary_monitor()
                .ok()
                .flatten()
                .map(|monitor| monitor.scale_factor())
        })
        .unwrap_or(1.0);

    TrayIconBuilder::with_id("main-tray")
        .icon(tauri::image::Image::from_bytes(tray_icon_bytes(scale))?)
        .tooltip("AstroMan")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main_window(app),
            "open-notes" => {
                show_main_window(app);
                let _ = app.emit("notes:open", ());
            }
            "notes-quick" => notes::quick::show(app),
            "notes-pause-hour" => pause_notes(app, 60),
            "notes-pause-today" => pause_notes(app, 0),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        })
        .build(app)?;

    Ok(())
}

pub fn run() {
    tauri::Builder::default()
        // 单实例必须最先注册：重复启动时不再开新进程，直接唤出已有窗口
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            show_main_window(app);
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        // 记住窗口尺寸/位置/最大化状态，下次启动原样恢复；
        // 快速记录窗口每次居中弹出，不参与状态记忆
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_denylist(&["quick-capture"])
                .build(),
        )
        .setup(|app| {
            // 日志落盘：数据目录确定后才能建写入器，因此放在 setup 里初始化
            let data_dir = state::resolve_data_dir(app.handle());
            env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
                .target(env_logger::Target::Pipe(Box::new(
                    logging::RollingWriter::new(data_dir.join("logs")),
                )))
                .init();
            log::info!("AstroMan 启动，数据目录：{}", data_dir.display());

            let state = state::AppState::new(app.handle())?;
            app.manage(state);

            // 启动后台任务：响应文件清理、自动备份（60s 后）、quick_check 自检
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                {
                    let state = handle.state::<state::AppState>();
                    let cwd = state.data_dir.clone();
                    tauri::async_runtime::spawn_blocking(move || {
                        commands::files::cleanup_responses(&cwd);
                    });
                }

                tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                // 压测期间不抢连接：最多退让 5 次（每次 60s）
                for _ in 0..5 {
                    if !handle
                        .state::<state::AppState>()
                        .stress_running
                        .load(std::sync::atomic::Ordering::SeqCst)
                    {
                        break;
                    }
                    tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                }

                let state = handle.state::<state::AppState>();
                if state
                    .stress_running
                    .load(std::sync::atomic::Ordering::SeqCst)
                {
                    log::info!("压测仍在运行，本次自动备份跳过");
                    return;
                }
                if let Err(error) = backup::check(&state.pool, false) {
                    log::warn!("启动自检失败：{error}");
                }
                match backup::auto_backup(&state) {
                    Ok(Some(item)) => log::info!("自动备份完成：{}", item.name),
                    Ok(None) => {}
                    Err(error) => log::warn!("自动备份失败：{error}"),
                }
            });

            app.manage(commands::winstats::WinStatsState::new());
            setup_tray(app.handle())?;

            // 待办：恢复全局快捷键并启动提醒调度（隐藏到托盘后仍然生效）
            {
                let state = app.state::<state::AppState>();
                let settings = notes::get_settings(&state.pool);
                if let Err(error) =
                    notes::quick::sync_shortcut(app.handle(), settings.global_shortcut)
                {
                    log::warn!("{error}");
                    if settings.global_shortcut {
                        let mut fallback = settings.clone();
                        fallback.global_shortcut = false;
                        let _ = notes::set_settings(&state.pool, &fallback);
                    }
                }
            }
            notes::reminder::spawn(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::system::app_info,
            commands::system::workspace_stats,
            commands::system::dashboard_metrics,
            commands::project::project_list,
            commands::project::project_create,
            commands::project::project_update,
            commands::project::project_delete,
            commands::collection::collection_tree,
            commands::collection::collection_create,
            commands::collection::collection_rename,
            commands::collection::collection_delete,
            commands::collection::collection_reorder,
            commands::request::request_list,
            commands::request::request_search,
            commands::request::request_options,
            commands::request::request_get,
            commands::request::request_save,
            commands::request::request_save_quick,
            commands::request::request_delete,
            commands::request::request_reorder,
            commands::environment::env_list,
            commands::environment::env_create,
            commands::environment::env_activate,
            commands::environment::env_delete,
            commands::environment::env_plain_vars,
            commands::environment::env_var_save,
            commands::environment::env_script_save,
            commands::http::send_http_request,
            commands::http::http_cancel,
            commands::backup::backup_list,
            commands::backup::backup_create,
            commands::backup::backup_restore,
            commands::backup::backup_cancel_restore,
            commands::backup::backup_delete,
            commands::backup::backup_dir_path,
            commands::backup::db_check,
            commands::backup::diagnostics_export,
            commands::security::secrets_status,
            commands::security::secrets_unlock,
            commands::security::secrets_set_passphrase,
            commands::security::secrets_export,
            commands::security::secrets_import,
            commands::security::secrets_reset,
            commands::security::secrets_inspect_pack,
            commands::security::secrets_clear_cache,
            commands::ai::ai_context_meta,
            commands::ai::ai_provider_list,
            commands::ai::ai_provider_save,
            commands::ai::ai_provider_delete,
            commands::ai::ai_provider_test,
            commands::ai::ai_chat,
            commands::ai::ai_chat_cancel,
            commands::ai::ai_execute,
            commands::ai::ai_models_list,
            commands::ai::ai_session_list,
            commands::ai::ai_session_create,
            commands::ai::ai_session_rename,
            commands::ai::ai_session_delete,
            commands::ai::ai_session_pin,
            commands::ai::ai_session_summary_save,
            commands::ai::ai_message_list,
            commands::ai::ai_message_add,
            commands::ai::ai_message_clear,
            commands::ai::ai_message_delete_from,
            commands::ai::ai_message_delete,
            commands::ai::ai_tool_log_add,
            commands::ai::ai_tool_log_list,
            commands::ai::ai_tool_log_stats,
            commands::ai::ai_tool_log_clear,
            commands::ai::ai_usage_stats,
            commands::ai::ai_usage_logs,
            commands::ai::ai_usage_clear,
            commands::oauth::oauth2_token,
            commands::cookie::cookie_list,
            commands::cookie::cookie_save,
            commands::cookie::cookie_delete,
            commands::cookie::cookie_clear,
            commands::settings::settings_get,
            commands::settings::settings_set,
            commands::plan::plan_list,
            commands::plan::plan_get,
            commands::plan::plan_save,
            commands::plan::plan_delete,
            commands::test_suite::test_suite_list,
            commands::test_suite::test_suite_get,
            commands::test_suite::test_suite_save,
            commands::test_suite::test_suite_delete,
            commands::test_pack::test_suite_export,
            commands::test_pack::test_suite_import,
            commands::test_suite::test_dataset_list,
            commands::test_suite::test_dataset_get,
            commands::test_suite::test_dataset_save,
            commands::test_suite::test_dataset_delete,
            commands::test_case::test_case_list,
            commands::test_case::test_case_get,
            commands::test_case::test_case_save,
            commands::test_case::test_case_delete,
            commands::test_case::test_case_reorder,
            commands::test_preflight::test_preflight,
            commands::python::python_check,
            commands::test_run::test_run_start,
            commands::test_run::test_run_stop,
            commands::test_run::test_run_get,
            commands::test_run::test_run_list,
            commands::test_run::test_run_current,
            commands::test_report::test_report_generate,
            commands::test_report::test_artifact_list,
            commands::test_report::test_artifact_export,
            commands::files::file_read_text,
            commands::files::file_exists,
            commands::files::file_write_text,
            commands::files::file_write_binary,
            commands::files::file_copy,
            commands::files::response_file_path,
            commands::history::history_list,
            commands::history::history_stats,
            commands::history::data_cleanup,
            commands::run::run_list,
            commands::run::run_get,
            commands::run::run_clear,
            commands::stress::stress_start,
            commands::stress::stress_stop,
            commands::report::report_list,
            commands::report::report_get,
            commands::report::report_delete,
            commands::report::report_export,
            commands::report::report_write_file,
            commands::report::report_requests,
            commands::report::report_update_texts,
            commands::report::report_clear_details,
            commands::report::report_export_detail_csv,
            commands::curl::curl_parse,
            commands::transfer::project_export,
            commands::transfer::project_import,
            commands::transfer::openapi_export,
            commands::transfer::openapi_import,
            commands::update::update_check,
            commands::update::update_download,
            commands::update::open_external,
            commands::importer::api_import,
            commands::tools::tool_hash,
            commands::tools::tool_hmac,
            commands::tools::ip_lookup,
            commands::winstats::winstats_snapshot,
            commands::notes::notes_list,
            commands::notes::notes_get,
            commands::notes::notes_create,
            commands::notes::notes_update,
            commands::notes::notes_toggle,
            commands::notes::notes_snooze,
            commands::notes::notes_delete,
            commands::notes::notes_restore,
            commands::notes::notes_clear_done,
            commands::notes::notes_counts,
            commands::notes::notes_tags,
            commands::notes::notes_settings_get,
            commands::notes::notes_settings_set,
            commands::notes::notes_quick_show,
            commands::notes::notes_attachment_add,
            commands::notes::notes_attachment_list,
            commands::notes::notes_attachment_data,
            commands::notes::notes_attachment_remove,
            commands::notes::notes_attachment_save,
            commands::notes::notes_extract_image,
            commands::notes::notes_extract_cancel,
            commands::notes::notes_create_from_image,
            commands::winstats::winstats_processes,
            commands::winstats::winstats_process_kill,
            commands::storage::storage_info,
            commands::storage::storage_set,
            commands::storage::storage_reset,
            commands::storage::storage_open,
            commands::storage::app_restart,
            commands::capture::capture_start,
            commands::capture::capture_stop,
            commands::capture::capture_status,
            commands::capture::capture_flows,
            commands::capture::capture_flow_detail,
            commands::capture::capture_flow_clear,
            commands::capture::capture_lan_ips,
            commands::capture::capture_ca_info,
            commands::capture::capture_ca_generate,
            commands::capture::capture_ca_install,
            commands::capture::capture_ca_export,
            commands::capture::capture_sysproxy_enable,
            commands::capture::capture_sysproxy_disable,
            commands::capture::capture_repeat,
            commands::capture::capture_export_har,
            commands::capture::capture_export_zip,
            commands::capture::capture_save_body,
            commands::capture::capture_intercept_state,
            commands::capture::capture_breakpoint_set_rules,
            commands::capture::capture_map_set_rules,
            commands::capture::capture_remote_set_rules,
            commands::capture::capture_weak_set,
            commands::capture::capture_breakpoint_resolve,
            commands::capture::capture_breakpoint_release_all,
        ])
        .on_window_event(|window, event| {
            // 关闭主窗口时隐藏到托盘，从托盘菜单「退出 AstroMan」才会真正结束进程
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .build(tauri::generate_context!())
        .expect("启动 AstroMan 失败")
        .run(|app_handle, event| {
            // macOS 点击 Dock 图标时把隐藏的主窗口重新显示出来
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { .. } = event {
                show_main_window(app_handle);
            }
            #[cfg(not(target_os = "macos"))]
            let _ = (app_handle, event);
        });
}
