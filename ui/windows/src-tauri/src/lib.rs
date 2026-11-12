//! The Windows app's Rust side: it names every driver, holds what is open,
//! and turns the UI's commands into core calls. It formats nothing and words
//! nothing; the core formats values and the UI words errors.

mod catalog;
mod commands;
mod data;
mod export;
mod jobs;
mod library;
mod run;
mod state;
mod window;

#[cfg(test)]
mod tests;

use std::sync::Arc;

use paboable_core::registry::DriverRegistry;
use paboable_driver_d1::D1Driver;
use paboable_driver_mssql::MssqlDriver;
use paboable_driver_postgres::PostgresDriver;
use paboable_driver_redis::RedisDriver;
use paboable_driver_sqlite::SqliteDriver;
use tauri::Manager;

use crate::data::DataFolder;
use crate::library::Library;
use crate::state::AppState;

fn registry() -> DriverRegistry {
    DriverRegistry::new()
        .with(Arc::new(SqliteDriver))
        .with(Arc::new(PostgresDriver))
        .with(Arc::new(MssqlDriver))
        .with(Arc::new(RedisDriver))
        .with(Arc::new(D1Driver))
}

pub fn run() {
    let state = Arc::new(AppState::new(registry()));
    let result = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(state)
        .setup(|app| {
            // Everything the app remembers is in one folder: `data` beside the app when
            // the installer made it, else %APPDATA%\<identifier>: paboable.app installed,
            // dev.paboable.app from `pnpm tauri dev` (ADR 0019).
            let profile = app.path().app_data_dir()?;
            let folder = DataFolder::find(profile.clone());
            folder.carry_over(&profile);
            std::fs::create_dir_all(&folder.path)?;
            let old_webview = app.path().app_local_data_dir()?.join("EBWebView");
            let (webview, import_local_storage) = data::webview_for_launch(&folder, &old_webview);
            app.manage(Library::in_folder(&folder.path));
            // Without a history the app still runs; the History tab says so.
            if let Ok(history) = paboable_storage::history::QueryHistory::open(&folder.path) {
                let _ = app.state::<Arc<AppState>>().history.set(Arc::new(history));
            }
            app.manage(folder.clone());
            window::open(app, &folder, webview, import_local_storage)?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::engines,
            commands::parse_connection_url,
            commands::list_saved,
            commands::save_connection,
            commands::delete_saved,
            commands::result_source,
            commands::object_ddl,
            commands::list_indexes,
            commands::qualified_name,
            commands::export_table,
            commands::preview_csv,
            commands::import_plan,
            commands::import_csv,
            commands::generate_plan,
            commands::generate_rows,
            commands::object_comment,
            commands::view_definition_of,
            commands::maintenance_tasks_of,
            commands::object_action,
            commands::run_statements,
            commands::read_text_file,
            commands::write_text_file,
            commands::list_profiles,
            commands::save_profile,
            commands::delete_profile,
            commands::connect_saved,
            commands::connect,
            commands::test_connection,
            commands::list_form_databases,
            commands::disconnect,
            commands::list_databases,
            commands::list_schemas,
            commands::list_objects,
            commands::fetch_columns,
            commands::run_query,
            commands::cancel_run,
            commands::sort_result,
            commands::filter_result,
            commands::result_counts,
            commands::result_where,
            commands::history_search,
            commands::history_delete,
            commands::list_queries,
            commands::save_query,
            commands::delete_query,
            commands::fetch_window,
            commands::cell_text,
            commands::copy_range,
            commands::row_values,
            commands::release_result,
            commands::browse_statements,
            commands::count_rows,
            commands::complete,
            commands::forget_catalog,
            commands::scan_keys,
            commands::read_key,
            commands::edit_preview,
            commands::apply_edits,
            commands::set_mica,
            commands::read_pages,
            commands::write_pages,
            commands::write_prefs,
            commands::import_local_storage,
            commands::open_data_folder,
        ])
        .run(tauri::generate_context!());
    if let Err(error) = result {
        eprintln!("Paboable could not start: {error}");
        std::process::exit(1);
    }
}
