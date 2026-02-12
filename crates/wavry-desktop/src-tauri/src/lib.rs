pub mod auth;
pub mod client_manager;
pub mod commands;
pub mod media_utils;
pub mod secure_storage;
pub mod state;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            commands::greet,
            commands::get_pcvr_status,
            commands::set_cc_config,
            commands::get_cc_stats,
            commands::register,
            commands::login_full,
            commands::set_signaling_token,
            commands::start_session,
            commands::stop_session,
            commands::send_file_transfer_command,
            commands::list_monitors,
            commands::connect_via_id,
            commands::start_host,
            commands::stop_host,
            commands::save_secure_token,
            commands::load_secure_token,
            commands::delete_secure_token,
            commands::save_secure_data,
            commands::load_secure_data,
            commands::delete_secure_data,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
