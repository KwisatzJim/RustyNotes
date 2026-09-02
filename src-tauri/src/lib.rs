// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/

mod commands;
mod db;
mod settings;
mod auth;
mod notes_api;
mod import;
mod refresh;
mod resolution;
mod upload;
mod create_remote;
mod export;
mod markdown_import;
mod recovery;
mod sync_status;
mod trash;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(auth::LoginState::default())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            commands::get_notes,
            commands::create_note,
            commands::update_note,
            commands::delete_note,
            trash::list_trashed_notes,
            trash::restore_note,
            settings::get_server_url,
            settings::save_server_url,
            auth::begin_login,
            auth::poll_login,
            auth::cancel_login,
            auth::get_login_status,
            notes_api::check_notes_connection,
            notes_api::import_server_notes,
            notes_api::refresh_server_notes,
            refresh::list_refresh_conflicts,
            refresh::get_refresh_conflict,
            resolution::resolve_conflict,
            upload::upload_note,
            create_remote::is_local_only_note,
            create_remote::create_server_note,
            export::export_note,
            markdown_import::import_markdown,
            recovery::list_creation_recoveries,
            recovery::get_creation_recovery,
            recovery::recover_creation,
            sync_status::get_note_sync_status,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
