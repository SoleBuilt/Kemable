//! The Windows app's Rust side: it names every driver, holds what is open,
//! and turns the UI's commands into core calls. It formats nothing and words
//! nothing; the core formats values and the UI words errors.

mod catalog;
mod commands;
mod library;
mod run;
mod state;

#[cfg(test)]
mod tests;

use std::sync::Arc;

use paboable_core::registry::DriverRegistry;
use paboable_driver_mssql::MssqlDriver;
use paboable_driver_postgres::PostgresDriver;
use paboable_driver_sqlite::SqliteDriver;
use tauri::Manager;

use crate::library::Library;
use crate::state::AppState;

fn registry() -> DriverRegistry {
    DriverRegistry::new()
        .with(Arc::new(SqliteDriver))
        .with(Arc::new(PostgresDriver))
        .with(Arc::new(MssqlDriver))
}

pub fn run() {
    let state = Arc::new(AppState::new(registry()));
    let result = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(state)
        .setup(|app| {
            // %APPDATA%\dev.paboable.app, the folder Windows keeps per user.
            let folder = app.path().app_data_dir()?;
            app.manage(Library::in_folder(&folder));
            // Without a history the app still runs; the History tab says so.
            if let Ok(history) = paboable_storage::history::QueryHistory::open(&folder) {
                let _ = app.state::<Arc<AppState>>().history.set(Arc::new(history));
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::engines,
            commands::parse_connection_url,
            commands::list_saved,
            commands::save_connection,
            commands::delete_saved,
            commands::connect_saved,
            commands::connect,
            commands::disconnect,
            commands::list_databases,
            commands::list_schemas,
            commands::list_objects,
            commands::fetch_columns,
            commands::run_query,
            commands::cancel_run,
            commands::sort_result,
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
        ])
        .run(tauri::generate_context!());
    if let Err(error) = result {
        eprintln!("Paboable could not start: {error}");
        std::process::exit(1);
    }
}
