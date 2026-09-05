// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/

mod auth;
mod backup;
mod backup_preview;
mod commands;
mod create_remote;
mod db;
mod export;
mod import;
mod markdown_import;
mod notes_api;
mod recovery;
mod refresh;
mod resolution;
mod restore;
mod settings;
mod sync_status;
mod trash;
mod upload;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(auth::LoginState::default())
        .manage(backup_preview::PreviewState::default())
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
            backup::backup_local_data,
            backup::open_local_data_folder,
            backup_preview::preview_local_backup,
            restore::restore_local_backup,
            markdown_import::import_markdown,
            recovery::list_creation_recoveries,
            recovery::get_creation_recovery,
            recovery::recover_creation,
            sync_status::get_note_sync_status,
            sync_status::list_local_changes,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
