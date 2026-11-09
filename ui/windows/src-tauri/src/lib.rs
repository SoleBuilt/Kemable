//! The Windows app's Rust side: it names every driver, holds what is open,
//! and turns the UI's commands into core calls. It formats nothing and words
//! nothing; the core formats values and the UI words errors.

mod commands;
mod run;
mod state;

use std::sync::Arc;

use paboable_core::registry::DriverRegistry;
use paboable_driver_postgres::PostgresDriver;
use paboable_driver_sqlite::SqliteDriver;

use crate::state::AppState;

fn registry() -> DriverRegistry {
    DriverRegistry::new()
        .with(Arc::new(SqliteDriver))
        .with(Arc::new(PostgresDriver))
}

pub fn run() {
    let state = Arc::new(AppState::new(registry()));
    let result = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            commands::engines,
            commands::parse_connection_url,
            commands::connect,
            commands::disconnect,
            commands::list_schemas,
            commands::list_objects,
            commands::fetch_columns,
            commands::run_query,
            commands::cancel_run,
            commands::fetch_window,
            commands::cell_text,
            commands::release_result,
        ])
        .run(tauri::generate_context!());
    if let Err(error) = result {
        eprintln!("Paboable could not start: {error}");
        std::process::exit(1);
    }
}
