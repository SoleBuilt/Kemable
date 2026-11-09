//! The commands the UI calls (`ui/windows/src/lib/ipc.ts` lists them with
//! their TypeScript types). Each one is async so it runs off the main thread.
//!
//! `conn` names an open server. `database` names one of its databases, for an
//! engine that has them; `None` means the database the connection opened on.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use paboable_core::catalog::{ColumnInfo, DatabaseInfo, ObjectInfo, SchemaInfo, TableRef};
use paboable_core::config::{ConnectionConfig, Secret, Target};
use paboable_core::engine::EngineInfo;
use paboable_core::sql::{split, statement_at};
use paboable_core::{CallOptions, DriverError};
use paboable_storage::SavedConnection;
use paboable_store::{CellRange, MAX_COPY_CELLS, WindowRequest};
use serde::Serialize;
use tauri::State;
use tauri::ipc::{Channel, Response};
use tokio_util::sync::CancellationToken;

use crate::library::Library;
use crate::run::{self, Run, RunEvent, Statement};
use crate::state::{AppState, CONNECT_TIMEOUT, OpenServer};

type CommandResult<T> = Result<T, DriverError>;

const METADATA_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenedConnection {
    id: u64,
    engine: EngineInfo,
    server_version: Option<String>,
    title: String,
    /// The database the connection opened on, for an engine with databases.
    database: Option<String>,
}

#[tauri::command]
pub async fn engines(state: State<'_, Arc<AppState>>) -> CommandResult<Vec<EngineInfo>> {
    Ok(state.registry.engines().cloned().collect())
}

/// Reads a pasted connection URL into a config the connect form shows.
#[tauri::command]
pub async fn parse_connection_url(url: String) -> CommandResult<ConnectionConfig> {
    paboable_core::connection_url::parse(&url)
}

#[tauri::command]
pub async fn connect(state: State<'_, Arc<AppState>>, config: ConnectionConfig) -> CommandResult<OpenedConnection> {
    let title = title_of(&config.target);
    open_server(&state, config, title).await
}

async fn open_server(state: &AppState, config: ConnectionConfig, title: String) -> CommandResult<OpenedConnection> {
    let driver = state
        .registry
        .get(&config.engine)
        .cloned()
        .ok_or_else(|| DriverError::unsupported(format!("no driver for {}", config.engine)))?;
    let first = driver
        .connect(&config, CallOptions::default().with_timeout(CONNECT_TIMEOUT))
        .await?;
    let server_version = first.server_version();
    let server = Arc::new(OpenServer::new(driver, config, first));
    let opened = OpenedConnection {
        id: 0,
        engine: server.engine.clone(),
        server_version,
        title,
        database: server.engine.has_databases.then(|| server.home.clone()),
    };
    let id = state.add_server(server);
    Ok(OpenedConnection { id, ..opened })
}

/// The saved connections, in the order they were added.
#[tauri::command]
pub async fn list_saved(library: State<'_, Library>) -> CommandResult<Vec<SavedConnection>> {
    library.with(|library, _| library.list())
}

/// Saves a connection (a new one when its id is empty). `password`, when
/// given, replaces the kept one; the file never holds it.
#[tauri::command]
pub async fn save_connection(
    library: State<'_, Library>,
    connection: SavedConnection,
    password: Option<String>,
) -> CommandResult<SavedConnection> {
    library.with(|library, vault| library.save(connection, password.map(Secret::new), vault))
}

#[tauri::command]
pub async fn delete_saved(library: State<'_, Library>, id: String) -> CommandResult<()> {
    library.with(|library, vault| library.delete(&id, vault))
}

/// Opens a saved connection. `password` is for one saved without its
/// password, asked for at connect time and not kept.
#[tauri::command]
pub async fn connect_saved(
    state: State<'_, Arc<AppState>>,
    library: State<'_, Library>,
    id: String,
    password: Option<String>,
) -> CommandResult<OpenedConnection> {
    let (saved, mut config) = library.with(|library, vault| library.resolve(&id, vault))?;
    if let Some(password) = password
        && let Target::Network { password: target, .. } = &mut config.target
    {
        *target = Secret::new(password);
    }
    let opened = open_server(&state, config, saved.name).await?;
    // A failure to record the time is not worth failing the connection for.
    let _ = library.with(|library, _| library.touch(&id));
    Ok(opened)
}

fn title_of(target: &Target) -> String {
    match target {
        Target::File { path, .. } if path == Path::new(":memory:") => "In memory".into(),
        Target::File { path, .. } => path.file_name().map_or_else(
            || path.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        ),
        Target::Network { host, port, .. } => match port {
            Some(port) => format!("{host}:{port}"),
            None => host.clone(),
        },
        Target::Http { endpoint, .. } => endpoint.clone(),
    }
}

#[tauri::command]
pub async fn disconnect(state: State<'_, Arc<AppState>>, conn: u64) -> CommandResult<()> {
    if let Some(server) = state.remove_server(conn) {
        server.close().await;
    }
    Ok(())
}

fn metadata_options() -> CallOptions {
    CallOptions::default().with_timeout(METADATA_TIMEOUT)
}

#[tauri::command]
pub async fn list_databases(state: State<'_, Arc<AppState>>, conn: u64) -> CommandResult<Vec<DatabaseInfo>> {
    state
        .server(conn)?
        .session(None)
        .await?
        .list_databases(metadata_options())
        .await
}

#[tauri::command]
pub async fn list_schemas(
    state: State<'_, Arc<AppState>>,
    conn: u64,
    database: Option<String>,
) -> CommandResult<Vec<SchemaInfo>> {
    let session = state.server(conn)?.session(database.as_deref()).await?;
    session.list_schemas(metadata_options()).await
}

#[tauri::command]
pub async fn list_objects(
    state: State<'_, Arc<AppState>>,
    conn: u64,
    database: Option<String>,
    schema: Option<String>,
) -> CommandResult<Vec<ObjectInfo>> {
    let session = state.server(conn)?.session(database.as_deref()).await?;
    session.list_objects(schema.as_deref(), metadata_options()).await
}

#[tauri::command]
pub async fn fetch_columns(
    state: State<'_, Arc<AppState>>,
    conn: u64,
    database: Option<String>,
    table: TableRef,
) -> CommandResult<Vec<ColumnInfo>> {
    let session = state.server(conn)?.session(database.as_deref()).await?;
    session.fetch_columns(&table, metadata_options()).await
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct ByteRange {
    start: usize,
    end: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunStarted {
    run: u64,
    /// The UTF-8 byte ranges of the statements this run sends, so the editor
    /// can show what ran.
    ranges: Vec<ByteRange>,
    /// How many statements the whole text holds.
    script_statements: usize,
}

/// Starts a run and returns at once; its events follow on `on_event`. With
/// `cursor` (a UTF-8 byte offset into `sql`) only the statement under it runs,
/// else every statement.
#[tauri::command]
#[allow(clippy::too_many_arguments)] // Each is one field of the UI's request.
pub async fn run_query(
    state: State<'_, Arc<AppState>>,
    conn: u64,
    database: Option<String>,
    sql: String,
    cursor: Option<usize>,
    follow_created_database: bool,
    on_event: Channel<RunEvent>,
) -> CommandResult<RunStarted> {
    let server = state.server(conn)?;
    let grammar = server
        .engine
        .sql_grammar
        .ok_or_else(|| DriverError::unsupported("this engine does not run SQL"))?;
    let spans = split(&sql, grammar);
    let chosen = match cursor {
        Some(offset) => statement_at(&spans, offset)
            .map(|index| &spans[index..=index])
            .unwrap_or_default(),
        None => &spans[..],
    };
    let statements = chosen
        .iter()
        .map(|span| Statement {
            text: span.text(&sql).to_owned(),
            kind: span.kind,
        })
        .collect();
    let ranges = chosen
        .iter()
        .map(|span| ByteRange {
            start: span.start,
            end: span.end,
        })
        .collect();

    let cancel = CancellationToken::new();
    let id = state.add_run(cancel.clone());
    let run = Run {
        id,
        server,
        database,
        grammar,
        statements,
        follow_created_database,
        cancel,
        channel: on_event,
    };
    tauri::async_runtime::spawn(run::execute(Arc::clone(&state), run));
    Ok(RunStarted {
        run: id,
        ranges,
        script_statements: spans.len(),
    })
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

/// A rectangle of cells as tab-separated text for the clipboard, built in
/// the store under one lock. Refused past `MAX_COPY_CELLS`.
#[tauri::command]
pub async fn copy_range(
    state: State<'_, Arc<AppState>>,
    result: u64,
    first_row: u64,
    row_count: u64,
    first_column: usize,
    column_count: usize,
    with_headers: bool,
) -> CommandResult<String> {
    let range = CellRange {
        first_row,
        row_count,
        first_column,
        column_count,
    };
    state
        .result(result)?
        .copy_tsv(range, with_headers)
        .ok_or_else(|| DriverError::unsupported(format!("copying more than {MAX_COPY_CELLS} cells at once")))
}

/// Every value of one row, for the detail pane.
#[tauri::command]
pub async fn row_values(state: State<'_, Arc<AppState>>, result: u64, row: u64) -> CommandResult<Vec<Option<String>>> {
    state
        .result(result)?
        .row_texts(row)
        .ok_or_else(|| DriverError::not_found("row"))
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
