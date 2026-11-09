//! The commands the UI calls (`ui/windows/src/lib/ipc.ts` lists them with
//! their TypeScript types). Each one is async so it runs off the main thread.
//!
//! `conn` names an open server. `database` names one of its databases, for an
//! engine that has them; `None` means the database the connection opened on.

use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use paboable_core::catalog::{ColumnInfo, ColumnMeta, DatabaseInfo, ObjectInfo, SchemaInfo, TableRef};
use paboable_core::config::{ConnectionConfig, Secret, Target};
use paboable_core::engine::EngineInfo;
use paboable_core::sink::ResultSink;
use paboable_core::sql::{
    BrowseQuery, SortColumn, count_statement, page_statement, sorted_statement, split, statement_at,
};
use paboable_core::value::Value;
use paboable_core::{CallOptions, DriverError};
use paboable_storage::SavedConnection;
use paboable_storage::history::{HistoryEntry, HistoryQuery};
use paboable_storage::queries::SavedQuery;
use paboable_store::{CellRange, MAX_COPY_CELLS, SortKey, WindowRequest};
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
    /// The saved connection it was opened from, if any.
    saved_id: Option<String>,
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
    open_server(&state, config, title, None).await
}

async fn open_server(
    state: &AppState,
    config: ConnectionConfig,
    title: String,
    saved_id: Option<String>,
) -> CommandResult<OpenedConnection> {
    let driver = state
        .registry
        .get(&config.engine)
        .cloned()
        .ok_or_else(|| DriverError::unsupported(format!("no driver for {}", config.engine)))?;
    let first = driver
        .connect(&config, CallOptions::default().with_timeout(CONNECT_TIMEOUT))
        .await?;
    let server_version = first.server_version();
    let server = Arc::new(OpenServer::new(driver, config, first, title.clone()));
    let opened = OpenedConnection {
        id: 0,
        engine: server.engine.clone(),
        server_version,
        title,
        saved_id,
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
    let opened = open_server(&state, config, saved.name, Some(id.clone())).await?;
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
    row_limit: Option<u64>,
    record_history: bool,
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
        row_limit,
        record: record_history,
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

#[derive(Debug, Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SortRequestKey {
    column: usize,
    descending: bool,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum SortOutcome {
    /// Run `sql` in `database` with `row_limit`; its result is the sorted one.
    /// `statement` is the one it was written from, to run for "unsorted".
    Rerun {
        sql: String,
        statement: String,
        database: Option<String>,
        row_limit: Option<u64>,
    },
    /// The core sorted the rows held; `limited` says they are not every row.
    Sorted { limited: bool },
}

/// Sorts a result as `docs/decisions/0005-sorting-a-result.md` decides: its
/// statement again with ORDER BY when that is safe, else the rows it holds.
/// No keys clears a sort the core made.
#[tauri::command]
pub async fn sort_result(
    state: State<'_, Arc<AppState>>,
    result: u64,
    keys: Vec<SortRequestKey>,
) -> CommandResult<SortOutcome> {
    let store = state.result(result)?;
    if !keys.is_empty()
        && let Some(origin) = state.origin(result).filter(|origin| origin.alone)
    {
        let names: Vec<String> = store.columns().iter().map(|column| column.name.clone()).collect();
        let columns: Vec<SortColumn> = keys
            .iter()
            .map(|key| SortColumn {
                column: key.column,
                descending: key.descending,
            })
            .collect();
        if let Some(sql) = sorted_statement(&origin.statement, origin.grammar, &names, &columns) {
            return Ok(SortOutcome::Rerun {
                sql,
                statement: origin.statement,
                database: origin.database,
                row_limit: origin.row_limit,
            });
        }
    }
    let keys: Vec<SortKey> = keys
        .iter()
        .map(|key| SortKey {
            column: key.column,
            descending: key.descending,
        })
        .collect();
    // A big set takes a moment to sort: keep it off the async workers.
    let sorting = Arc::clone(&store);
    tauri::async_runtime::spawn_blocking(move || sorting.sort(&keys))
        .await
        .map_err(|error| DriverError::internal(error.to_string()))?;
    Ok(SortOutcome::Sorted {
        limited: store.state().limited,
    })
}

/// The query history, newest first, filtered by text and connection.
#[tauri::command]
pub async fn history_search(state: State<'_, Arc<AppState>>, query: HistoryQuery) -> CommandResult<Vec<HistoryEntry>> {
    let history = state
        .history
        .get()
        .cloned()
        .ok_or_else(|| DriverError::not_found("query history"))?;
    tauri::async_runtime::spawn_blocking(move || history.search(&query))
        .await
        .map_err(|error| DriverError::internal(error.to_string()))?
}

#[tauri::command]
pub async fn history_delete(state: State<'_, Arc<AppState>>, id: Option<i64>) -> CommandResult<()> {
    let history = state
        .history
        .get()
        .cloned()
        .ok_or_else(|| DriverError::not_found("query history"))?;
    match id {
        Some(id) => history.delete(id),
        None => history.clear(),
    }
}

/// The saved queries a connection shows: its own and the global ones.
#[tauri::command]
pub async fn list_queries(
    library: State<'_, Library>,
    connection_id: Option<String>,
) -> CommandResult<Vec<SavedQuery>> {
    library.with_queries(|queries| queries.list(connection_id.as_deref()))
}

#[tauri::command]
pub async fn save_query(library: State<'_, Library>, query: SavedQuery) -> CommandResult<SavedQuery> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX));
    library.with_queries(|queries| queries.save(query, now))
}

#[tauri::command]
pub async fn delete_query(library: State<'_, Library>, id: String) -> CommandResult<()> {
    library.with_queries(|queries| queries.delete(&id))
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

/// How long a table tab's row count may take before it gives up: a count
/// reads the whole table, and the page is already on screen without it.
const COUNT_TIMEOUT: Duration = Duration::from_secs(30);

/// The statements a table tab runs (docs/decisions/0006-table-browsing.md).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowseStatements {
    page: String,
    count: String,
}

#[tauri::command]
pub async fn browse_statements(
    state: State<'_, Arc<AppState>>,
    conn: u64,
    query: BrowseQuery,
) -> CommandResult<BrowseStatements> {
    let server = state.server(conn)?;
    let grammar = server
        .engine
        .sql_grammar
        .ok_or_else(|| DriverError::unsupported("this engine does not run SQL"))?;
    Ok(BrowseStatements {
        page: page_statement(grammar, &query)?,
        count: count_statement(grammar, &query)?,
    })
}

/// Runs a count statement and returns its one number.
#[tauri::command]
pub async fn count_rows(
    state: State<'_, Arc<AppState>>,
    conn: u64,
    database: Option<String>,
    sql: String,
) -> CommandResult<u64> {
    let session = state.server(conn)?.session(database.as_deref()).await?;
    let sink = Arc::new(CountSink::default());
    session
        .execute(
            &sql,
            Arc::clone(&sink) as Arc<dyn ResultSink>,
            CallOptions::default().with_timeout(COUNT_TIMEOUT),
        )
        .await?;
    let count = *sink.count.lock().unwrap_or_else(PoisonError::into_inner);
    count.ok_or_else(|| DriverError::internal("the count returned no number"))
}

/// Keeps the first value of a one-row result, as a count.
#[derive(Default)]
struct CountSink {
    count: Mutex<Option<u64>>,
}

impl ResultSink for CountSink {
    fn begin_set(&self, _columns: Vec<ColumnMeta>) {}

    fn push_rows(&self, values: &[Value]) -> bool {
        let count = match values.first() {
            Some(Value::Int(value)) => u64::try_from(*value).ok(),
            Some(Value::UInt(value)) => Some(*value),
            Some(Value::Decimal(text)) => text.parse().ok(),
            _ => None,
        };
        let mut slot = self.count.lock().unwrap_or_else(PoisonError::into_inner);
        if slot.is_none() {
            *slot = count;
        }
        false
    }

    fn end_set(&self) {}

    fn command_done(&self, _rows_affected: Option<u64>) {}

    fn notice(&self, _text: &str) {}
}
