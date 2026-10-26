mod commands;
mod db;
mod models;
mod netinfo;
mod state;
mod util;

use tauri::Manager;

pub fn run() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let state = state::AppState::new(app.handle())?;
            log::info!("FinkMeter 数据目录: {:?}", state.data_dir);
            app.manage(state);
            app.manage(commands::winstats::WinStatsState::new());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::system::app_info,
            commands::system::workspace_stats,
            commands::project::project_list,
            commands::project::project_create,
            commands::project::project_update,
            commands::project::project_delete,
            commands::collection::collection_tree,
            commands::collection::collection_create,
            commands::collection::collection_rename,
            commands::collection::collection_delete,
            commands::request::request_list,
            commands::request::request_search,
            commands::request::request_options,
            commands::request::request_get,
            commands::request::request_save,
            commands::request::request_delete,
            commands::environment::env_list,
            commands::environment::env_create,
            commands::environment::env_activate,
            commands::environment::env_delete,
            commands::environment::env_var_save,
            commands::http::send_http_request,
            commands::history::history_list,
            commands::history::history_clear,
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
            // macOS 点击关闭按钮时只隐藏窗口，保留实例，便于从 Dock 再次打开
            #[cfg(target_os = "macos")]
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
            #[cfg(not(target_os = "macos"))]
            let _ = (window, event);
        })
        .build(tauri::generate_context!())
        .expect("启动 FinkMeter 失败")
        .run(|app_handle, event| {
            // macOS 点击 Dock 图标时把隐藏的主窗口重新显示出来
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { .. } = event {
                if let Some(window) = app_handle.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            #[cfg(not(target_os = "macos"))]
            let _ = (app_handle, event);
        });
}
