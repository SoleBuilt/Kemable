//! The commands the UI calls (`ui/windows/src/lib/ipc.ts` lists them with
//! their TypeScript types). Each one is async so it runs off the main thread.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use paboable_core::catalog::{ColumnInfo, ObjectInfo, SchemaInfo, TableRef};
use paboable_core::config::{ConnectionConfig, Target};
use paboable_core::engine::EngineInfo;
use paboable_core::sql::{split, statement_at};
use paboable_core::{CallOptions, DriverError};
use paboable_store::WindowRequest;
use serde::Serialize;
use tauri::State;
use tauri::ipc::{Channel, Response};
use tokio_util::sync::CancellationToken;

use crate::run::{self, Run, RunEvent};
use crate::state::AppState;

type CommandResult<T> = Result<T, DriverError>;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const METADATA_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenedConnection {
    id: u64,
    engine: EngineInfo,
    server_version: Option<String>,
    title: String,
}

#[tauri::command]
pub async fn engines(state: State<'_, Arc<AppState>>) -> CommandResult<Vec<EngineInfo>> {
    Ok(state.registry.engines().cloned().collect())
}

#[tauri::command]
pub async fn connect(state: State<'_, Arc<AppState>>, config: ConnectionConfig) -> CommandResult<OpenedConnection> {
    let driver = state
        .registry
        .get(&config.engine)
        .cloned()
        .ok_or_else(|| DriverError::unsupported(format!("no driver for {}", config.engine)))?;
    let connection = driver
        .connect(&config, CallOptions::default().with_timeout(CONNECT_TIMEOUT))
        .await?;
    let opened = OpenedConnection {
        id: 0,
        engine: connection.engine().clone(),
        server_version: connection.server_version(),
        title: title_of(&config.target),
    };
    let id = state.add_connection(connection);
    Ok(OpenedConnection { id, ..opened })
}

fn title_of(target: &Target) -> String {
    match target {
        Target::File { path, .. } if path == Path::new(":memory:") => "In memory".into(),
        Target::File { path, .. } => path.file_name().map_or_else(
            || path.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        ),
        Target::Network { host, database, .. } => match database {
            Some(database) => format!("{database} on {host}"),
            None => host.clone(),
        },
        Target::Http { endpoint, .. } => endpoint.clone(),
    }
}

#[tauri::command]
pub async fn disconnect(state: State<'_, Arc<AppState>>, conn: u64) -> CommandResult<()> {
    match state.remove_connection(conn) {
        Some(connection) => connection.close().await,
        None => Ok(()),
    }
}

fn metadata_options() -> CallOptions {
    CallOptions::default().with_timeout(METADATA_TIMEOUT)
}

#[tauri::command]
pub async fn list_schemas(state: State<'_, Arc<AppState>>, conn: u64) -> CommandResult<Vec<SchemaInfo>> {
    state.connection(conn)?.list_schemas(metadata_options()).await
}

#[tauri::command]
pub async fn list_objects(
    state: State<'_, Arc<AppState>>,
    conn: u64,
    schema: Option<String>,
) -> CommandResult<Vec<ObjectInfo>> {
    state
        .connection(conn)?
        .list_objects(schema.as_deref(), metadata_options())
        .await
}

#[tauri::command]
pub async fn fetch_columns(
    state: State<'_, Arc<AppState>>,
    conn: u64,
    table: TableRef,
) -> CommandResult<Vec<ColumnInfo>> {
    state.connection(conn)?.fetch_columns(&table, metadata_options()).await
}

/// Starts a run and returns its id at once; its events follow on `on_event`.
/// With `cursor` (a UTF-8 byte offset into `sql`) only the statement under it
/// runs, else every statement.
#[tauri::command]
pub async fn run_query(
    state: State<'_, Arc<AppState>>,
    conn: u64,
    sql: String,
    cursor: Option<usize>,
    on_event: Channel<RunEvent>,
) -> CommandResult<u64> {
    let connection = state.connection(conn)?;
    let grammar = connection
        .engine()
        .sql_grammar
        .ok_or_else(|| DriverError::unsupported("this engine does not run SQL"))?;
    let spans = split(&sql, grammar);
    let chosen = match cursor {
        Some(offset) => statement_at(&spans, offset)
            .map(|index| &spans[index..=index])
            .unwrap_or_default(),
        None => &spans[..],
    };
    let statements: Vec<String> = chosen.iter().map(|span| span.text(&sql).to_owned()).collect();

    let cancel = CancellationToken::new();
    let id = state.add_run(cancel.clone());
    let run = Run {
        id,
        connection,
        statements,
        cancel,
        channel: on_event,
    };
    tauri::async_runtime::spawn(run::execute(Arc::clone(&state), run));
    Ok(id)
}

#[tauri::command]
pub async fn cancel_run(state: State<'_, Arc<AppState>>, run: u64) -> CommandResult<()> {
    state.cancel_run(run);
    Ok(())
}

/// One packed window, sent as raw bytes: the webview gets an ArrayBuffer and
/// nothing is serialized as JSON (`docs/decisions/0002-result-windows.md`).
#[tauri::command]
pub async fn fetch_window(
    state: State<'_, Arc<AppState>>,
    result: u64,
    first_row: u64,
    row_count: u32,
    first_column: u32,
    column_count: u32,
    max_chars: u32,
) -> CommandResult<Response> {
    let store = state.result(result)?;
    let bytes = store.fetch_window(WindowRequest {
        first_row,
        row_count,
        first_column,
        column_count,
        max_chars,
    });
    Ok(Response::new(bytes))
}

#[tauri::command]
pub async fn cell_text(
    state: State<'_, Arc<AppState>>,
    result: u64,
    row: u64,
    column: usize,
) -> CommandResult<Option<String>> {
    state
        .result(result)?
        .cell_text(row, column)
        .ok_or_else(|| DriverError::not_found("cell"))
}

#[tauri::command]
pub async fn release_result(state: State<'_, Arc<AppState>>, result: u64) -> CommandResult<()> {
    state.remove_result(result);
    Ok(())
}
