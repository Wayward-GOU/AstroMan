mod commands;
mod db;
mod models;
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
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::system::app_info,
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
            commands::curl::curl_parse,
            commands::transfer::project_export,
            commands::transfer::project_import,
            commands::tools::tool_hash,
            commands::tools::tool_hmac,
        ])
        .run(tauri::generate_context!())
        .expect("启动 FinkMeter 失败");
}
