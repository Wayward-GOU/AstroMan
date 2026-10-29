mod commands;
mod db;
mod models;
mod netinfo;
mod prompts;
mod state;
mod util;

use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager,
};

fn show_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// 系统托盘：左键点击显示主窗口，右键菜单可显示或退出
fn setup_tray(app: &tauri::AppHandle) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "显示主窗口", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出 AstroMan", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;

    TrayIconBuilder::with_id("main-tray")
        .icon(tauri::image::Image::from_bytes(include_bytes!(
            "../icons/tray.png"
        ))?)
        .tooltip("AstroMan")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main_window(app),
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
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    tauri::Builder::default()
        // 单实例必须最先注册：重复启动时不再开新进程，直接唤出已有窗口
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            show_main_window(app);
        }))
        .plugin(tauri_plugin_dialog::init())
        // 记住窗口尺寸/位置/最大化状态，下次启动原样恢复
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .setup(|app| {
            let state = state::AppState::new(app.handle())?;
            log::info!("AstroMan 数据目录: {:?}", state.data_dir);
            app.manage(state);
            app.manage(commands::winstats::WinStatsState::new());
            setup_tray(app.handle())?;
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
            commands::request::request_delete,
            commands::request::request_reorder,
            commands::environment::env_list,
            commands::environment::env_create,
            commands::environment::env_activate,
            commands::environment::env_delete,
            commands::environment::env_var_save,
            commands::environment::env_script_save,
            commands::http::send_http_request,
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
            commands::files::file_read_text,
            commands::files::file_exists,
            commands::files::file_write_text,
            commands::history::history_list,
            commands::history::history_stats,
            commands::history::data_cleanup,
            commands::run::run_list,
            commands::run::run_clear,
            commands::stress::stress_start,
            commands::stress::stress_stop,
            commands::report::report_list,
            commands::report::report_get,
            commands::report::report_delete,
            commands::report::report_export,
            commands::report::report_write_file,
            commands::report::report_requests,
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
            commands::winstats::winstats_processes,
            commands::storage::storage_info,
            commands::storage::storage_set,
            commands::storage::storage_reset,
            commands::storage::storage_open,
            commands::storage::app_restart,
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
