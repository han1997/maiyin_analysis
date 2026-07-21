mod analysis;
mod commands;
mod error;
mod exporter;
mod importer;
mod model;
mod storage;

use commands::AppState;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let storage_root = app.path().app_data_dir()?;
            let state = AppState::open(storage_root)
                .map_err(|error| std::io::Error::other(error.to_string()))?;
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::bootstrap_workspace,
            commands::import_paths,
            commands::import_folders,
            commands::load_session,
            commands::merge_sessions,
            commands::delete_session,
            commands::clear_workspace,
            commands::reanalyze,
            commands::get_person_detail,
            commands::get_imported_records,
            commands::export_result,
            commands::set_storage_directory,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run maiyin analysis");
}
