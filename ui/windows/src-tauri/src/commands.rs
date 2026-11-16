//! The commands the UI calls (`ui/windows/src/lib/ipc.ts` lists them with
//! their TypeScript types). Each one is async so it runs off the main thread.
//!
//! `conn` names an open server. `database` names one of its databases, for an
//! engine that has them; `None` means the database the connection opened on.

use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use kemable_core::catalog::{ColumnInfo, ColumnMeta, DatabaseInfo, ObjectInfo, SchemaInfo, TableRef};
use kemable_core::command::split_lines;
use kemable_core::config::{ConnectionConfig, Secret, Target};
use kemable_core::engine::{DataModel, EngineInfo};
use kemable_core::keys::{KeyDetail, KeyPage, KeyScan};
use kemable_core::registry::DriverRegistry;
use kemable_core::sink::ResultSink;
use kemable_core::sql::{
    BrowseQuery, Clause, CompletionItem, Maintenance, NewValue, ObjectAction, ResultFilter, ResultSource, ResultWhere,
    RowEdit, SortColumn, SqlGrammar, action_statements, completion_context, completions, count_statement,
    edit_statement, export_statement, keep_line_endings, maintenance_tasks, page_statement, readable_name,
    sorted_statement, split, statement_at, view_definition,
};
use kemable_core::text_file::{self, LineEnding, TextEncoding, TextFile};
use kemable_core::value::Value;
use kemable_core::{CallOptions, DriverError};
use kemable_storage::SavedConnection;
use kemable_storage::history::{HistoryEntry, HistoryQuery};
use kemable_storage::profiles::CredentialProfile;
use kemable_storage::queries::SavedQuery;
use kemable_store::{MAX_COPY_CELLS, ResultStore, RowSpan, SortKey, WindowRequest};
use serde::{Deserialize, Serialize};
use tauri::State;
use tauri::ipc::{Channel, Response};
use tokio_util::sync::CancellationToken;

use crate::data::{self, DataFolder};
use crate::library::Library;
use crate::run::{self, Run, RunEvent, Statement};
use crate::state::{AppState, CONNECT_TIMEOUT, OpenServer};

type CommandResult<T> = Result<T, DriverError>;

const METADATA_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenedConnection {
    pub(crate) id: u64,
    engine: EngineInfo,
    server_version: Option<String>,
    pub(crate) title: String,
    /// The saved connection it was opened from, if any.
    pub(crate) saved_id: Option<String>,
    /// The database the connection opened on, for an engine with databases.
    database: Option<String>,
    /// A Supabase project's URL, read from the host: its Live tab can listen
    /// to Realtime too (`docs/decisions/0025-supabase-realtime.md`).
    realtime_url: Option<String>,
    /// What names it across launches, as its history entries keep it.
    key: String,
}

#[tauri::command]
pub async fn engines(state: State<'_, Arc<AppState>>) -> CommandResult<Vec<EngineInfo>> {
    Ok(state.registry.engines().cloned().collect())
}

/// Reads a pasted connection URL into a config the connect form shows.
#[tauri::command]
pub async fn parse_connection_url(url: String) -> CommandResult<ConnectionConfig> {
    kemable_core::connection_url::parse(&url)
}

/// Opens a connection as typed. `profile_id` signs it in with a credential
/// profile's user and password instead of the ones in `config`.
#[tauri::command]
pub async fn connect(
    state: State<'_, Arc<AppState>>,
    library: State<'_, Library>,
    mut config: ConnectionConfig,
    profile_id: Option<String>,
) -> CommandResult<OpenedConnection> {
    sign_in_with(&library, &mut config, profile_id.as_deref())?;
    let title = title_of(&config.target);
    open_server(&state, config, title, None).await
}

/// Connects with the connect form's connection and saves it only once the
/// server has answered: a connection that cannot connect is never kept. The
/// login is the form's: the typed password, else a credential profile, else
/// the one kept for the connection being edited.
#[tauri::command]
pub async fn connect_and_save(
    state: State<'_, Arc<AppState>>,
    library: State<'_, Library>,
    connection: SavedConnection,
    password: Option<String>,
) -> CommandResult<OpenedConnection> {
    let mut config = connection.config.clone();
    let editing = (!connection.id.is_empty()).then(|| connection.id.clone());
    sign_in_as_form(&library, &mut config, editing, connection.profile_id.clone())?;
    let title = connection.name.clone();
    open_then_keep(&state, config, title, || {
        library.with(|library, vault| {
            let saved = library.save(connection, password.map(Secret::new), vault)?;
            // A failure to record the time is not worth failing the connection for.
            let _ = library.touch(&saved.id);
            Ok(saved)
        })
    })
    .await
}

/// Opens `config`, then keeps it with `keep`: nothing is kept when the server
/// does not answer, and a connection that cannot be kept is closed again.
pub(crate) async fn open_then_keep(
    state: &AppState,
    config: ConnectionConfig,
    title: String,
    keep: impl FnOnce() -> CommandResult<SavedConnection>,
) -> CommandResult<OpenedConnection> {
    let opened = open_server(state, config, title, None).await?;
    match keep() {
        Ok(saved) => Ok(OpenedConnection {
            saved_id: Some(saved.id),
            title: saved.name,
            ..opened
        }),
        Err(error) => {
            if let Some(server) = state.remove_server(opened.id) {
                server.close().await;
            }
            Err(error)
        }
    }
}

fn sign_in_with(library: &Library, config: &mut ConnectionConfig, profile_id: Option<&str>) -> CommandResult<()> {
    match profile_id {
        Some(id) => library.with(|library, vault| library.profiles().apply(id, vault, config)),
        None => Ok(()),
    }
}

async fn open_server(
    state: &AppState,
    mut config: ConnectionConfig,
    title: String,
    saved_id: Option<String>,
) -> CommandResult<OpenedConnection> {
    let driver = state
        .registry
        .get(&config.engine)
        .cloned()
        .ok_or_else(|| DriverError::unsupported(format!("no driver for {}", config.engine)))?;
    let tunnel = tunnel_for(state, &mut config).await?;
    let first = driver
        .connect(&config, CallOptions::default().with_timeout(CONNECT_TIMEOUT))
        .await?;
    let server_version = first.server_version();
    let key = connection_key(&config, &title, saved_id.as_deref());
    let server = Arc::new(
        OpenServer::new(driver, config, first, title.clone())
            .with_tunnel(tunnel)
            .with_key(key.clone()),
    );
    let opened = OpenedConnection {
        id: 0,
        engine: server.engine.clone(),
        server_version,
        title,
        saved_id,
        database: server.engine.has_databases.then(|| server.home.clone()),
        realtime_url: server.realtime_url(),
        key,
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

/// A script file bigger than this is not opened: the editor is for scripts, not dumps.
const MAX_SCRIPT_BYTES: u64 = 64 * 1024 * 1024;

fn file_error(what: &str, path: &str, error: impl std::fmt::Display) -> DriverError {
    DriverError::new(kemable_core::ErrorCode::Io, format!("{what} {path}: {error}"))
}

/// Reads a `.sql` file for a query page (`docs/decisions/0016-sql-files.md`):
/// its text, and the encoding and line endings a save writes back.
#[tauri::command]
pub async fn read_text_file(path: String) -> CommandResult<TextFile> {
    tauri::async_runtime::spawn_blocking(move || {
        let size = std::fs::metadata(&path)
            .map_err(|error| file_error("cannot read", &path, error))?
            .len();
        if size > MAX_SCRIPT_BYTES {
            return Err(DriverError::unsupported(format!(
                "{path} is {size} bytes; scripts up to {MAX_SCRIPT_BYTES} open"
            )));
        }
        let bytes = std::fs::read(&path).map_err(|error| file_error("cannot read", &path, error))?;
        text_file::decode(&bytes)
    })
    .await
    .map_err(|error| DriverError::internal(error.to_string()))?
}

/// Writes a query page's text to `path` in `encoding` with `line_ending`,
/// through a temporary file and a rename, so a failed save leaves the old file.
/// Resolves to the file's stamp once written, which a later [`file_stamps`]
/// compares with to tell a change made outside the app.
#[tauri::command]
pub async fn write_text_file(
    path: String,
    text: String,
    encoding: TextEncoding,
    line_ending: LineEnding,
) -> CommandResult<Option<FileStamp>> {
    tauri::async_runtime::spawn_blocking(move || {
        let bytes = text_file::encode(&text, encoding, line_ending);
        let target = Path::new(&path);
        let temporary = target.with_extension("kemable-save.tmp");
        std::fs::write(&temporary, bytes).map_err(|error| file_error("cannot write", &path, error))?;
        std::fs::rename(&temporary, target).map_err(|error| {
            let _ = std::fs::remove_file(&temporary);
            file_error("cannot replace", &path, error)
        })?;
        Ok(FileStamp::of(target))
    })
    .await
    .map_err(|error| DriverError::internal(error.to_string()))?
}

/// When a file was last written and how long it is: what tells that it
/// changed on disk without reading it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileStamp {
    /// Nanoseconds since the Unix epoch, as text: more than a JavaScript number holds.
    pub modified: String,
    pub size: u64,
}

impl FileStamp {
    /// `None` when the file is gone or cannot be read.
    pub fn of(path: &Path) -> Option<Self> {
        let metadata = std::fs::metadata(path).ok().filter(std::fs::Metadata::is_file)?;
        let modified = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |since| since.as_nanos());
        Some(Self {
            modified: modified.to_string(),
            size: metadata.len(),
        })
    }
}

/// The stamp of each of `paths`, `None` for one that is gone: a query page
/// asks for its file's when the window comes back to the front, so a file
/// changed outside the app is noticed (`docs/decisions/0016-sql-files.md`).
/// Only metadata is read, never the files.
#[tauri::command]
pub async fn file_stamps(paths: Vec<String>) -> CommandResult<Vec<Option<FileStamp>>> {
    tauri::async_runtime::spawn_blocking(move || paths.iter().map(|path| FileStamp::of(Path::new(path))).collect())
        .await
        .map_err(|error| DriverError::internal(error.to_string()))
}

/// The `.sql` files under a folder linked into the sidebar, folders first
/// (`docs/decisions/0016-sql-files.md`).
#[tauri::command]
pub async fn list_sql_folder(path: String) -> CommandResult<crate::folders::SqlFolder> {
    tauri::async_runtime::spawn_blocking(move || crate::folders::list(Path::new(&path)))
        .await
        .map_err(|error| DriverError::internal(error.to_string()))?
}

/// The credential profiles, by name. Their passwords stay in the vault.
#[tauri::command]
pub async fn list_profiles(library: State<'_, Library>) -> CommandResult<Vec<CredentialProfile>> {
    library.with(|library, _| library.profiles().list())
}

/// Saves a credential profile (a new one when its id is empty). `password`,
/// when given, replaces the kept one.
#[tauri::command]
pub async fn save_profile(
    library: State<'_, Library>,
    profile: CredentialProfile,
    password: Option<String>,
) -> CommandResult<CredentialProfile> {
    library.with(|library, vault| library.profiles().save(profile, password.map(Secret::new), vault))
}

/// Deletes a credential profile; resolves to how many saved connections used it.
#[tauri::command]
pub async fn delete_profile(library: State<'_, Library>, id: String) -> CommandResult<usize> {
    library.with(|library, vault| library.delete_profile(&id, vault))
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
    let (saved, mut config, moved) = library.with(|library, vault| library.resolve(&id, vault))?;
    // A kept secret is sent only where it was kept for (docs/decisions/0033-bound-passwords.md).
    if moved.ssh {
        return Err(moved_error(kemable_core::ErrorCode::SshSecretMoved, &saved.config));
    }
    if moved.password && password.is_none() {
        return Err(moved_error(kemable_core::ErrorCode::SecretMoved, &saved.config));
    }
    let typed = password.clone().filter(|_| moved.password);
    if let Some(password) = password {
        match &mut config.target {
            Target::Network { password: target, .. } => *target = Secret::new(password),
            Target::Http { token, .. } => *token = Secret::new(password),
            Target::File { .. } => {}
        }
    }
    let opened = open_server(&state, config, saved.name, Some(id.clone())).await?;
    // Typed again after the connection moved: kept for where it points now.
    if let Some(typed) = typed {
        library.with(|library, vault| library.keep_password(&id, &Secret::new(typed), vault))?;
    }
    // A failure to record the time is not worth failing the connection for.
    let _ = library.with(|library, _| library.touch(&id));
    Ok(opened)
}

/// What Test in the connect form learned: the server answered and the login worked.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestedConnection {
    server_version: Option<String>,
    /// How many databases this login may see, for an engine with databases.
    databases: Option<usize>,
}

/// Connects as the connect form would, says what the server is, and closes
/// again: nothing stays open. `saved_id` lends a saved connection's kept
/// password or token when the form's field is blank, as Connect does;
/// `profile_id` signs in with a credential profile instead.
#[tauri::command]
pub async fn test_connection(
    state: State<'_, Arc<AppState>>,
    library: State<'_, Library>,
    mut config: ConnectionConfig,
    saved_id: Option<String>,
    profile_id: Option<String>,
) -> CommandResult<TestedConnection> {
    sign_in_as_form(&library, &mut config, saved_id, profile_id)?;
    let _tunnel = tunnel_for(&state, &mut config).await?;
    try_connection(&state.registry, &config).await
}

/// Opens the SSH tunnel `config` asks for, if any, and points the config at
/// its local end (`docs/decisions/0024-ssh-tunnels.md`). Whatever is opened
/// from the config goes through it for as long as the tunnel is kept.
pub(crate) async fn tunnel_for(
    state: &AppState,
    config: &mut ConnectionConfig,
) -> CommandResult<Option<kemable_tunnel::Tunnel>> {
    let Some(ssh) = config.ssh.clone() else {
        return Ok(None);
    };
    let Target::Network { host, port, .. } = &config.target else {
        return Err(DriverError::config("an SSH tunnel reaches a host and a port"));
    };
    let port = port
        .or_else(|| {
            state
                .registry
                .get(&config.engine)
                .and_then(|driver| driver.info().default_port)
        })
        .ok_or_else(|| DriverError::config("the port is needed to go through SSH"))?;
    let tunnel = kemable_tunnel::open(
        &ssh,
        host.trim(),
        port,
        state.host_keys(),
        &CallOptions::default().with_timeout(CONNECT_TIMEOUT),
    )
    .await?;
    config.dial = Some(tunnel.local());
    Ok(Some(tunnel))
}

/// Trusts the key an SSH server showed when it was unknown, after the user
/// saw its fingerprint; the next connection through it goes on.
#[tauri::command]
pub async fn trust_ssh_host(state: State<'_, Arc<AppState>>, host: String, port: Option<u16>) -> CommandResult<()> {
    state.host_keys().trust(
        host.trim(),
        port.unwrap_or(kemable_core::config::SshTunnel::DEFAULT_PORT),
    )
}

/// Signs `config` in as the connect form would connect: with a credential
/// profile, or with a saved connection's kept secret when the field is blank.
fn sign_in_as_form(
    library: &Library,
    config: &mut ConnectionConfig,
    saved_id: Option<String>,
    profile_id: Option<String>,
) -> CommandResult<()> {
    // The SSH secret is the connection's even with a profile: a blank one is the kept one.
    if let Some(id) = &saved_id
        && config.ssh.as_ref().is_some_and(|ssh| ssh.secret().is_empty())
    {
        let (saved, kept, moved) = library.with(|library, vault| library.resolve(id, vault))?;
        if moved.ssh {
            return Err(moved_error(kemable_core::ErrorCode::SshSecretMoved, &saved.config));
        }
        if let (Some(ssh), Some(kept)) = (config.ssh.as_mut(), kept.ssh)
            && std::mem::discriminant(&ssh.auth) == std::mem::discriminant(&kept.auth)
        {
            ssh.set_secret(kept.secret().clone());
        }
    }
    if profile_id.is_some() {
        sign_in_with(library, config, profile_id.as_deref())?;
    } else if let Some(id) = saved_id
        && secret_is_blank(&config.target)
    {
        let (saved, kept, moved) = library.with(|library, vault| library.resolve(&id, vault))?;
        if moved.password {
            return Err(moved_error(kemable_core::ErrorCode::SecretMoved, &saved.config));
        }
        lend_secret(&mut config.target, kept.target);
    }
    Ok(())
}

/// The databases the connect form's login may see, for its Database picker:
/// connects as the form would, lists them, and closes again.
#[tauri::command]
pub async fn list_form_databases(
    state: State<'_, Arc<AppState>>,
    library: State<'_, Library>,
    mut config: ConnectionConfig,
    saved_id: Option<String>,
    profile_id: Option<String>,
) -> CommandResult<Vec<String>> {
    sign_in_as_form(&library, &mut config, saved_id, profile_id)?;
    let _tunnel = tunnel_for(&state, &mut config).await?;
    form_databases(&state.registry, &config).await
}

pub(crate) async fn form_databases(registry: &DriverRegistry, config: &ConnectionConfig) -> CommandResult<Vec<String>> {
    let driver = registry
        .get(&config.engine)
        .cloned()
        .ok_or_else(|| DriverError::unsupported(format!("no driver for {}", config.engine)))?;
    let connection = driver
        .connect(config, CallOptions::default().with_timeout(CONNECT_TIMEOUT))
        .await?;
    let listed = connection.list_databases(metadata_options()).await;
    // A close that fails leaves nothing the user can act on.
    let _ = connection.close().await;
    Ok(listed?.into_iter().map(|database| database.name).collect())
}

pub(crate) async fn try_connection(
    registry: &DriverRegistry,
    config: &ConnectionConfig,
) -> CommandResult<TestedConnection> {
    let driver = registry
        .get(&config.engine)
        .cloned()
        .ok_or_else(|| DriverError::unsupported(format!("no driver for {}", config.engine)))?;
    let connection = driver
        .connect(config, CallOptions::default().with_timeout(CONNECT_TIMEOUT))
        .await?;
    // The count is a bonus: a login that may connect but not list still passes.
    let databases = if connection.engine().has_databases {
        connection
            .list_databases(metadata_options())
            .await
            .ok()
            .map(|list| list.len())
    } else {
        None
    };
    let tested = TestedConnection {
        server_version: connection.server_version(),
        databases,
    };
    // A close that fails leaves nothing the user can act on.
    let _ = connection.close().await;
    Ok(tested)
}

/// Names a connection across launches: a saved one by its id, a SQLite file by its
/// path, anything else by its engine and title (as `connectionKey` in pages.ts).
pub(crate) fn connection_key(config: &ConnectionConfig, title: &str, saved_id: Option<&str>) -> String {
    match (saved_id, &config.target) {
        (Some(id), _) => format!("saved:{id}"),
        (None, Target::File { path, .. }) => format!("file:{}", path.display()),
        (None, _) => format!("{}:{title}", config.engine),
    }
}

/// The error for a kept secret left out: its message names where the connection points now.
fn moved_error(code: kemable_core::ErrorCode, config: &ConnectionConfig) -> DriverError {
    let through = config
        .ssh
        .as_ref()
        .map(|ssh| format!(" through {}@{}:{}", ssh.user, ssh.host, ssh.port()))
        .unwrap_or_default();
    let user = match &config.target {
        Target::Network { user: Some(user), .. } if !user.is_empty() => format!("{user}@"),
        _ => String::new(),
    };
    DriverError::new(code, format!("{user}{}{through}", title_of(&config.target)))
}

fn secret_is_blank(target: &Target) -> bool {
    match target {
        Target::Network { password, .. } => password.is_empty(),
        Target::Http { token, .. } => token.is_empty(),
        Target::File { .. } => false,
    }
}

fn lend_secret(target: &mut Target, from: Target) {
    match (target, from) {
        (Target::Network { password, .. }, Target::Network { password: kept, .. }) => *password = kept,
        (Target::Http { token, .. }, Target::Http { token: kept, .. }) => *token = kept,
        _ => {}
    }
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
    state.catalog.forget(conn);
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
        .read(None, |session| async move {
            session.list_databases(metadata_options()).await
        })
        .await
}

#[tauri::command]
pub async fn list_schemas(
    state: State<'_, Arc<AppState>>,
    conn: u64,
    database: Option<String>,
) -> CommandResult<Vec<SchemaInfo>> {
    state
        .server(conn)?
        .read(database.as_deref(), |session| async move {
            session.list_schemas(metadata_options()).await
        })
        .await
}

#[tauri::command]
pub async fn list_objects(
    state: State<'_, Arc<AppState>>,
    conn: u64,
    database: Option<String>,
    schema: Option<String>,
) -> CommandResult<Vec<ObjectInfo>> {
    let schema = schema.as_deref();
    state
        .server(conn)?
        .read(database.as_deref(), |session| async move {
            session.list_objects(schema, metadata_options()).await
        })
        .await
}

#[tauri::command]
pub async fn fetch_columns(
    state: State<'_, Arc<AppState>>,
    conn: u64,
    database: Option<String>,
    table: TableRef,
) -> CommandResult<Vec<ColumnInfo>> {
    let table = &table;
    state
        .server(conn)?
        .read(database.as_deref(), |session| async move {
            session.fetch_columns(table, metadata_options()).await
        })
        .await
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
    let grammar = server.engine.sql_grammar;
    let spans = match grammar {
        Some(grammar) => split(&sql, grammar),
        None if server.engine.data_model == DataModel::KeyValue => split_lines(&sql),
        None => return Err(DriverError::unsupported("this engine has no console")),
    };
    let chosen = match cursor {
        Some(offset) => statement_at(&spans, offset)
            .map(|index| &spans[index..=index])
            .unwrap_or_default(),
        None => &spans[..],
    };
    let chosen = run::statements_to_run(&sql, chosen.to_vec(), grammar, follow_created_database);
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
/// While a filter is on, always the rows held, the filtered ones
/// (`docs/decisions/0011-filtering-a-result.md`). No keys clears a sort the
/// core made.
#[tauri::command]
pub async fn sort_result(
    state: State<'_, Arc<AppState>>,
    result: u64,
    keys: Vec<SortRequestKey>,
) -> CommandResult<SortOutcome> {
    let store = state.result(result)?;
    if !keys.is_empty()
        && !store.state().filtered
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
        if let Some(grammar) = origin.grammar
            && let Some(sql) = sorted_statement(&origin.statement, grammar, &names, &columns)
        {
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

// --- The object tree's menu (docs/decisions/0017-tree-actions.md) ---------------

/// The statement that makes `object` as the server holds it now (Show DDL, Copy DDL).
#[tauri::command]
pub async fn object_ddl(
    state: State<'_, Arc<AppState>>,
    conn: u64,
    database: Option<String>,
    object: ObjectInfo,
) -> CommandResult<String> {
    let object = &object;
    state
        .server(conn)?
        .read(database.as_deref(), |session| async move {
            session.object_ddl(object, metadata_options()).await
        })
        .await
}

/// The comment on `object`, for Edit Comment.
#[tauri::command]
pub async fn object_comment(
    state: State<'_, Arc<AppState>>,
    conn: u64,
    database: Option<String>,
    object: ObjectInfo,
) -> CommandResult<Option<String>> {
    let object = &object;
    state
        .server(conn)?
        .read(database.as_deref(), |session| async move {
            session.object_comment(object, metadata_options()).await
        })
        .await
}

/// A view's definition as the statement that replaces it, to edit on a query page.
#[tauri::command]
pub async fn view_definition_of(
    state: State<'_, Arc<AppState>>,
    conn: u64,
    database: Option<String>,
    object: ObjectInfo,
) -> CommandResult<String> {
    let server = state.server(conn)?;
    let grammar = grammar_of(&server)?;
    let object = &object;
    let ddl = server
        .read(database.as_deref(), |session| async move {
            session.object_ddl(object, metadata_options()).await
        })
        .await?;
    Ok(view_definition(grammar, object, &ddl))
}

/// The maintenance tasks the connection's engine offers, in the menu's order.
#[tauri::command]
pub async fn maintenance_tasks_of(state: State<'_, Arc<AppState>>, conn: u64) -> CommandResult<Vec<Maintenance>> {
    let server = state.server(conn)?;
    Ok(maintenance_tasks(grammar_of(&server)?).to_vec())
}

/// The statements `action` would run on `object`, to show before they run.
#[tauri::command]
pub async fn object_action(
    state: State<'_, Arc<AppState>>,
    conn: u64,
    object: ObjectInfo,
    action: ObjectAction,
) -> CommandResult<Vec<String>> {
    let server = state.server(conn)?;
    action_statements(grammar_of(&server)?, &object, &action)
}

/// Runs the statements an object action showed, in order, each on its own
/// (VACUUM may not run inside a transaction); the first failure stops the
/// rest. What completion read of the catalog is forgotten, since it changed.
#[tauri::command]
pub async fn run_statements(
    state: State<'_, Arc<AppState>>,
    conn: u64,
    database: Option<String>,
    statements: Vec<String>,
) -> CommandResult<()> {
    let server = state.server(conn)?;
    let session = server.session(database.as_deref()).await?;
    let sink: Arc<dyn ResultSink> = Arc::new(Discard);
    let outcome = async {
        for statement in &statements {
            session
                .execute(
                    statement,
                    Arc::clone(&sink),
                    CallOptions::default().with_timeout(ACTION_TIMEOUT),
                )
                .await?;
        }
        Ok(())
    }
    .await;
    if let Err(error) = &outcome {
        server.recover(database.as_deref(), &session, error).await;
    }
    state.catalog.forget(conn);
    outcome
}

/// A maintenance task on a big table takes a while; past this it is stopped.
const ACTION_TIMEOUT: Duration = Duration::from_secs(30 * 60);

fn grammar_of(server: &OpenServer) -> CommandResult<SqlGrammar> {
    server
        .engine
        .sql_grammar
        .ok_or_else(|| DriverError::unsupported("this engine has no SQL"))
}

/// A table's indexes, the primary key's first, for Show Structure.
#[tauri::command]
pub async fn list_indexes(
    state: State<'_, Arc<AppState>>,
    conn: u64,
    database: Option<String>,
    table: TableRef,
) -> CommandResult<Vec<kemable_core::catalog::IndexInfo>> {
    let table = &table;
    state
        .server(conn)?
        .read(database.as_deref(), |session| async move {
            session.list_indexes(table, metadata_options()).await
        })
        .await
}

/// An object's name as a person pastes it into a statement, quoted only where
/// it must be (Copy Qualified Name).
#[tauri::command]
pub async fn qualified_name(state: State<'_, Arc<AppState>>, conn: u64, object: ObjectInfo) -> CommandResult<String> {
    let server = state.server(conn)?;
    Ok(readable_name(
        grammar_of(&server)?,
        object.schema.as_deref(),
        &object.name,
    ))
}

/// Starts writing a table's rows to `path` (every row `query`'s filters
/// match, in key order); returns at once with an id `cancel_run` stops, and
/// reports on `on_event` (`docs/decisions/0018-export-import-generate.md`).
#[tauri::command]
pub async fn export_table(
    state: State<'_, Arc<AppState>>,
    conn: u64,
    database: Option<String>,
    query: BrowseQuery,
    options: kemable_core::export::ExportOptions,
    path: String,
    on_event: Channel<crate::jobs::JobEvent>,
) -> CommandResult<u64> {
    let server = state.server(conn)?;
    let grammar = grammar_of(&server)?;
    let statement = export_statement(grammar, &query)?;
    let session = server.session(database.as_deref()).await?;
    let cancel = CancellationToken::new();
    let id = state.add_run(cancel.clone());
    let export = crate::export::Export {
        id,
        session,
        statement,
        grammar,
        table: query.table,
        options,
        path: path.into(),
        cancel,
        channel: on_event,
    };
    tauri::async_runtime::spawn(crate::export::run(Arc::clone(&state), export));
    Ok(id)
}

/// A CSV file's separator, header, first rows and the shape of its values, for Import.
#[tauri::command]
pub async fn preview_csv(
    state: State<'_, Arc<AppState>>,
    conn: u64,
    path: String,
    delimiter: Option<char>,
    header: bool,
) -> CommandResult<crate::jobs::CsvPreview> {
    let server = state.server(conn)?;
    let grammar = grammar_of(&server)?;
    tauri::async_runtime::spawn_blocking(move || {
        crate::jobs::read_csv(&path).map(|text| crate::jobs::preview(&text, delimiter, header, grammar))
    })
    .await
    .map_err(|error| DriverError::internal(error.to_string()))?
}

/// What an import will do: where each field goes, the CREATE TABLE of a new
/// table, and the first INSERT, before anything runs.
#[tauri::command]
pub async fn import_plan(
    state: State<'_, Arc<AppState>>,
    conn: u64,
    database: Option<String>,
    path: String,
    options: crate::jobs::CsvOptions,
    target: crate::jobs::ImportTarget,
) -> CommandResult<crate::jobs::ImportPlan> {
    use kemable_core::import::{CsvReader, create_statement};
    let server = state.server(conn)?;
    let grammar = grammar_of(&server)?;
    let columns = match &target {
        crate::jobs::ImportTarget::Existing { table } => {
            server
                .read(database.as_deref(), |session| async move {
                    session.fetch_columns(table, metadata_options()).await
                })
                .await?
        }
        crate::jobs::ImportTarget::New { .. } => Vec::new(),
    };
    let text = tauri::async_runtime::spawn_blocking(move || crate::jobs::read_csv(&path))
        .await
        .map_err(|error| DriverError::internal(error.to_string()))??;
    let read = crate::jobs::preview(&text, Some(options.delimiter), options.header, grammar);
    let targets = crate::jobs::import_columns(&target, &read.header, &columns);
    let mut reader = CsvReader::new(&text, options.delimiter);
    if options.header {
        reader.next_fields();
    }
    let records = std::iter::from_fn(move || reader.next_fields());
    let first_insert = if targets.iter().any(Option::is_some) {
        crate::jobs::insert_batches(
            grammar,
            target.table(),
            &targets,
            records.take(5),
            options.empty_is_null,
            5,
        )
        .next()
        .transpose()?
        .map(|(sql, _)| sql)
    } else {
        None
    };
    let create = match &target {
        crate::jobs::ImportTarget::New { table, columns } => Some(create_statement(grammar, table, columns)?),
        crate::jobs::ImportTarget::Existing { .. } => None,
    };
    Ok(crate::jobs::ImportPlan {
        mapping: targets.into_iter().map(|target| target.map(|(name, _)| name)).collect(),
        create,
        first_insert,
        records: read.records,
    })
}

/// Starts an import; returns at once with an id `cancel_run` stops.
#[tauri::command]
pub async fn import_csv(
    state: State<'_, Arc<AppState>>,
    conn: u64,
    database: Option<String>,
    path: String,
    options: crate::jobs::CsvOptions,
    target: crate::jobs::ImportTarget,
    on_event: Channel<crate::jobs::JobEvent>,
) -> CommandResult<u64> {
    let server = state.server(conn)?;
    let grammar = grammar_of(&server)?;
    let session = server.session(database.as_deref()).await?;
    let columns = match &target {
        crate::jobs::ImportTarget::Existing { table } => session.fetch_columns(table, metadata_options()).await?,
        crate::jobs::ImportTarget::New { .. } => Vec::new(),
    };
    let text = tauri::async_runtime::spawn_blocking(move || crate::jobs::read_csv(&path))
        .await
        .map_err(|error| DriverError::internal(error.to_string()))??;
    let cancel = CancellationToken::new();
    let id = state.add_run(cancel.clone());
    let job = crate::jobs::ImportJob {
        id,
        session,
        grammar,
        // D1 runs each statement on its own: it has no transaction to roll back.
        transactional: server.engine.id != "d1",
        text,
        options,
        target,
        columns,
        cancel,
        channel: on_event,
    };
    state.catalog.forget(conn);
    tauri::async_runtime::spawn(crate::jobs::run_import(Arc::clone(&state), job));
    Ok(id)
}

/// What Generate Rows will insert into `table`: the columns it fills, the key
/// it counts up and from where, and the first five rows.
#[tauri::command]
pub async fn generate_plan(
    state: State<'_, Arc<AppState>>,
    conn: u64,
    database: Option<String>,
    table: TableRef,
    seed: u64,
) -> CommandResult<crate::jobs::GeneratePlan> {
    let server = state.server(conn)?;
    let grammar = grammar_of(&server)?;
    let session = server.session(database.as_deref()).await?;
    let columns = session.fetch_columns(&table, metadata_options()).await?;
    let start = match kemable_core::generate::counted_key(&columns) {
        Some(key) => crate::jobs::key_start(&session, grammar, &table, &key.name).await?,
        None => 1,
    };
    Ok(crate::jobs::plan(grammar, &table, &columns, start, seed))
}

/// Starts inserting `count` made-up rows; returns at once with an id `cancel_run` stops.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn generate_rows(
    state: State<'_, Arc<AppState>>,
    conn: u64,
    database: Option<String>,
    table: TableRef,
    count: u64,
    seed: u64,
    key_start: i64,
    on_event: Channel<crate::jobs::JobEvent>,
) -> CommandResult<u64> {
    let server = state.server(conn)?;
    let grammar = grammar_of(&server)?;
    let session = server.session(database.as_deref()).await?;
    let columns = session.fetch_columns(&table, metadata_options()).await?;
    let cancel = CancellationToken::new();
    let id = state.add_run(cancel.clone());
    let job = crate::jobs::GenerateJob {
        id,
        session,
        grammar,
        transactional: server.engine.id != "d1",
        table,
        columns,
        count,
        seed,
        key_start,
        cancel,
        channel: on_event,
    };
    tauri::async_runtime::spawn(crate::jobs::run_generate(Arc::clone(&state), job));
    Ok(id)
}

/// A sink for statements whose rows nobody reads (DBCC's report, say).
struct Discard;

impl ResultSink for Discard {
    fn begin_set(&self, _columns: Vec<ColumnMeta>) {}
    fn push_rows(&self, _values: &[Value]) -> bool {
        false
    }
    fn end_set(&self) {}
    fn command_done(&self, _rows_affected: Option<u64>) {}
    fn notice(&self, _text: &str) {}
}

/// The one table result `result` comes from, for "Open <table> to edit"
/// (`docs/decisions/0011-filtering-a-result.md`): its database, and the
/// statement's `WHERE` as filters when they can hold it. `None` when the
/// statement is not a plain `SELECT` from one table.
#[tauri::command]
pub async fn result_source(state: State<'_, Arc<AppState>>, result: u64) -> CommandResult<Option<ResultSource>> {
    Ok(source_of(&state, result))
}

pub(crate) fn source_of(state: &AppState, result: u64) -> Option<ResultSource> {
    let origin = state.origin(result)?;
    let grammar = origin.grammar?;
    let statement = origin.part.as_deref().unwrap_or(&origin.statement);
    kemable_core::sql::result_source(statement, grammar).map(|mut source| {
        source.database = source.database.or(origin.database);
        source
    })
}

/// What a result shows under its filter
/// (`docs/decisions/0011-filtering-a-result.md`).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResultCounts {
    /// The rows the filter keeps; every row held when no filter is on.
    shown_rows: u64,
    /// The rows the result holds.
    total_rows: u64,
    filtered: bool,
    /// The result stopped at its row limit: the filter covers only the rows loaded.
    limited: bool,
}

impl ResultCounts {
    fn of(store: &ResultStore) -> Self {
        let state = store.state();
        Self {
            shown_rows: state.shown_rows,
            total_rows: state.total_rows,
            filtered: state.filtered,
            limited: state.limited,
        }
    }
}

/// Filters the rows a result holds, in the core, never by running its
/// statement again. No filters clears the filter; a core sort stays.
#[tauri::command]
pub async fn filter_result(
    state: State<'_, Arc<AppState>>,
    result: u64,
    filters: Vec<ResultFilter>,
    match_any: bool,
) -> CommandResult<ResultCounts> {
    let store = state.result(result)?;
    // A million rows take a moment to test: keep them off the async workers.
    let filtering = Arc::clone(&store);
    tauri::async_runtime::spawn_blocking(move || filtering.filter(&filters, match_any))
        .await
        .map_err(|error| DriverError::internal(error.to_string()))??;
    Ok(ResultCounts::of(&store))
}

/// The counts of a result while it streams under a filter: run events count
/// the rows held, not the rows shown.
#[tauri::command]
pub async fn result_counts(state: State<'_, Arc<AppState>>, result: u64) -> CommandResult<ResultCounts> {
    let store = state.result(result)?;
    Ok(ResultCounts::of(&store))
}

/// A result's filter as a `WHERE` for the connection's engine, to copy;
/// Kemable never runs it.
#[tauri::command]
pub async fn result_where(
    state: State<'_, Arc<AppState>>,
    conn: u64,
    result: u64,
    filters: Vec<ResultFilter>,
    match_any: bool,
) -> CommandResult<ResultWhere> {
    let grammar = state
        .server(conn)?
        .engine
        .sql_grammar
        .ok_or_else(|| DriverError::unsupported("this engine does not run SQL"))?;
    let names: Vec<String> = state
        .result(result)?
        .columns()
        .iter()
        .map(|column| column.name.clone())
        .collect();
    kemable_core::sql::result_where(grammar, &names, &filters, match_any)
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

/// Starts a Live tab's listening in `database` and returns at once with an id
/// `cancel_run` stops (`docs/decisions/0020-live-tab.md`). `channels` and
/// `patterns` are what the user typed; the core reads the names in them.
/// Events arrive on `on_event` until one of type `finished`.
#[tauri::command]
pub async fn start_live(
    state: State<'_, Arc<AppState>>,
    library: State<'_, Library>,
    conn: u64,
    database: Option<String>,
    request: LiveRequest,
    on_event: Channel<crate::live::LiveEvent>,
) -> CommandResult<u64> {
    use kemable_core::live::LiveTopicKind;
    let server = state.server(conn)?;
    let texts: Vec<(LiveTopicKind, &str)> = request
        .topics
        .iter()
        .map(|typed| (typed.kind, typed.text.as_str()))
        .collect();
    let topics = kemable_core::live::topics_of(&texts);
    if topics.is_empty() {
        return Err(DriverError::config("nothing to listen to"));
    }
    let through_realtime = |kind: LiveTopicKind| matches!(kind, LiveTopicKind::Table | LiveTopicKind::Broadcast);
    let realtime = if topics.iter().any(|topic| through_realtime(topic.kind)) {
        // Supabase Realtime: a socket of its own, not a session (ADR 0025), and nothing else on it.
        if !topics.iter().all(|topic| through_realtime(topic.kind)) {
            return Err(DriverError::config("Realtime tables and LISTEN channels listen apart"));
        }
        let url = server
            .realtime_url()
            .ok_or_else(|| DriverError::unsupported("Realtime is a Supabase project's"))?;
        let key = realtime_key(&library, &url, request.api_key)?;
        Some((url, key))
    } else {
        if topics
            .iter()
            .any(|topic| !server.engine.live_topics.contains(&topic.kind))
        {
            return Err(DriverError::unsupported(
                "this engine does not listen to that kind of topic",
            ));
        }
        None
    };
    let cancel = server.closing.child_token();
    let id = state.add_run(cancel.clone());
    let live = crate::live::Live {
        id,
        server,
        database,
        topics,
        realtime,
        max_events: crate::live::MAX_EVENTS,
        cancel,
        channel: on_event,
    };
    tauri::async_runtime::spawn(crate::live::listen(Arc::clone(&state), live));
    Ok(id)
}

/// What a Live page asks to listen to: the text typed for each kind of topic.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveRequest {
    topics: Vec<TypedTopics>,
    /// Supabase Realtime's API key typed now; none uses the one kept for the project.
    #[serde(default)]
    api_key: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct TypedTopics {
    kind: kemable_core::live::LiveTopicKind,
    text: String,
}

/// The API key Realtime signs in with: the one typed, which is then kept in
/// Windows Credential Manager for the project, or the one kept.
fn realtime_key(library: &Library, url: &str, typed: Option<String>) -> CommandResult<Secret> {
    let name = format!("realtime/{}/key", url.trim_start_matches("https://"));
    library.with(|_, vault| match typed.filter(|key| !key.trim().is_empty()) {
        Some(key) => {
            let key = Secret::new(key.trim());
            vault.set(&name, &key)?;
            Ok(key)
        }
        None => vault
            .get(&name)?
            .ok_or_else(|| DriverError::config("Realtime needs the project's API key")),
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
    rows: Vec<RowSpanArg>,
    first_column: usize,
    column_count: usize,
    with_headers: bool,
) -> CommandResult<String> {
    let rows: Vec<RowSpan> = rows
        .into_iter()
        .map(|span| RowSpan {
            first_row: span.first_row,
            row_count: span.row_count,
        })
        .collect();
    state
        .result(result)?
        .copy_tsv_rows(&rows, first_column, column_count, with_headers)
        .ok_or_else(|| DriverError::unsupported(format!("copying more than {MAX_COPY_CELLS} cells at once")))
}

/// Rows of a selection: one span for a rectangle, one per run of rows picked with Ctrl+click.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RowSpanArg {
    first_row: u64,
    row_count: u64,
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
    let sql = &sql;
    let sink = Arc::new(CountSink::default());
    let counting = Arc::clone(&sink);
    // A count only reads, so a lost session is opened again and asked once more.
    state
        .server(conn)?
        .read(database.as_deref(), |session| {
            let sink = Arc::clone(&counting) as Arc<dyn ResultSink>;
            async move {
                session
                    .execute(sql, sink, CallOptions::default().with_timeout(COUNT_TIMEOUT))
                    .await
            }
        })
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

    fn row_limit(&self) -> Option<u64> {
        Some(1)
    }

    fn end_set(&self) {}

    fn command_done(&self, _rows_affected: Option<u64>) {}

    fn notice(&self, _text: &str) {}
}

/// What the editor offers at a cursor (docs/decisions/0007-autocomplete.md).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompletionReply {
    /// The UTF-8 byte offset where the word the items replace starts.
    from: usize,
    items: Vec<CompletionItem>,
}

/// The names to offer at byte `cursor` of `text`; `None` inside a string or a comment.
#[tauri::command]
pub async fn complete(
    state: State<'_, Arc<AppState>>,
    conn: u64,
    database: Option<String>,
    text: String,
    cursor: usize,
) -> CommandResult<Option<CompletionReply>> {
    let server = state.server(conn)?;
    let Some(grammar) = server.engine.sql_grammar else {
        return Ok(None);
    };
    let Some(context) = completion_context(&text, cursor, grammar) else {
        return Ok(None);
    };
    let from = context.from;
    if matches!(context.clause, Clause::Start | Clause::AfterTable | Clause::Other) {
        return Ok(Some(CompletionReply {
            from,
            items: Vec::new(),
        }));
    }
    let session = server.session(database.as_deref()).await?;
    let view = state.catalog.view(conn, database, session, &context).await;
    Ok(Some(CompletionReply {
        from,
        items: completions(&context, &view, grammar),
    }))
}

/// Forgets what completion read of a server's catalog, after a change to it.
#[tauri::command]
pub async fn forget_catalog(state: State<'_, Arc<AppState>>, conn: u64) -> CommandResult<()> {
    state.catalog.forget(conn);
    Ok(())
}

/// One step of a scan over a key-value server's keys
/// (docs/decisions/0008-key-value-engines.md).
#[tauri::command]
pub async fn scan_keys(
    state: State<'_, Arc<AppState>>,
    conn: u64,
    database: Option<String>,
    scan: KeyScan,
) -> CommandResult<KeyPage> {
    let scan = &scan;
    state
        .server(conn)?
        .read(database.as_deref(), |session| async move {
            session.scan_keys(scan, metadata_options()).await
        })
        .await
}

/// A key read into a result set for the grid.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyRead {
    /// The result set, when the key's kind has rows to show.
    result: Option<u64>,
    columns: Vec<ColumnMeta>,
    detail: KeyDetail,
    total_rows: u64,
    limited: bool,
    /// The set stopped at the memory budget holding about this many bytes.
    stopped_at_bytes: Option<u64>,
    /// What the driver said beside the rows.
    notices: Vec<String>,
}

#[tauri::command]
pub async fn read_key(
    state: State<'_, Arc<AppState>>,
    conn: u64,
    database: Option<String>,
    key: String,
    row_limit: Option<u64>,
) -> CommandResult<KeyRead> {
    let key = &key;
    let sink = Arc::new(KeySink {
        state: Arc::clone(&state),
        row_limit,
        store: Mutex::new(None),
        notices: Mutex::new(Vec::new()),
    });
    let reading = Arc::clone(&sink);
    let detail = state
        .server(conn)?
        .read(database.as_deref(), |session| {
            let sink = Arc::clone(&reading) as Arc<dyn ResultSink>;
            async move {
                session
                    .read_key(key, sink, CallOptions::default().with_timeout(COUNT_TIMEOUT))
                    .await
            }
        })
        .await?;
    let store = sink.store.lock().unwrap_or_else(PoisonError::into_inner).take();
    let notices = std::mem::take(&mut *sink.notices.lock().unwrap_or_else(PoisonError::into_inner));
    Ok(match store {
        Some(store) => {
            store.finish();
            let shown = store.state();
            let columns = store.columns().to_vec();
            KeyRead {
                result: Some(state.add_result(store, None)),
                columns,
                detail,
                total_rows: shown.total_rows,
                limited: shown.limited,
                stopped_at_bytes: shown.memory_limited.then_some(shown.bytes),
                notices,
            }
        }
        None => KeyRead {
            result: None,
            columns: Vec::new(),
            detail,
            total_rows: 0,
            limited: false,
            stopped_at_bytes: None,
            notices,
        },
    })
}

/// Keeps the one set a key read gives.
struct KeySink {
    state: Arc<AppState>,
    row_limit: Option<u64>,
    store: Mutex<Option<Arc<ResultStore>>>,
    notices: Mutex<Vec<String>>,
}

impl ResultSink for KeySink {
    fn begin_set(&self, columns: Vec<ColumnMeta>) {
        *self.store.lock().unwrap_or_else(PoisonError::into_inner) =
            Some(Arc::new(self.state.new_store(columns, self.row_limit)));
    }

    fn push_rows(&self, values: &[Value]) -> bool {
        self.store
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .is_some_and(|store| store.push_rows(values))
    }

    fn row_limit(&self) -> Option<u64> {
        self.row_limit
    }

    fn byte_limit(&self) -> Option<u64> {
        self.state.result_bytes
    }

    fn end_set(&self) {}

    fn command_done(&self, _rows_affected: Option<u64>) {}

    fn notice(&self, text: &str) {
        self.notices
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(text.to_owned());
    }
}

/// A table tab's pending edits (docs/decisions/0010-editing-rows.md). Rows
/// are rows of `result`, the page the tab shows; columns are its columns.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditRequest {
    pub result: u64,
    pub table: TableRef,
    /// The primary key's columns, which name each row.
    pub key: Vec<String>,
    #[serde(default)]
    pub updates: Vec<RowUpdate>,
    #[serde(default)]
    pub deletes: Vec<u64>,
    #[serde(default)]
    pub inserts: Vec<Vec<CellEdit>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RowUpdate {
    pub row: u64,
    pub cells: Vec<CellEdit>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CellEdit {
    pub column: usize,
    pub value: NewValue,
}

/// The statements that save `request`, in the order they run: deletes,
/// updates, inserts, so a key freed by a delete can be taken by the rest.
pub fn edit_statements(state: &AppState, grammar: SqlGrammar, request: &EditRequest) -> CommandResult<Vec<String>> {
    let store = state.result(request.result)?;
    let names: Vec<String> = store.columns().iter().map(|column| column.name.clone()).collect();
    let key: Vec<(usize, &String)> = request
        .key
        .iter()
        .map(|name| {
            names
                .iter()
                .position(|column| column == name)
                .map(|index| (index, name))
                .ok_or_else(|| DriverError::config(format!("the key column {name} is not in the page")))
        })
        .collect::<CommandResult<_>>()?;
    let key_of = |row: u64| {
        key.iter()
            .map(|(index, name)| {
                let value = store
                    .cell(row, *index)
                    .ok_or_else(|| DriverError::not_found(format!("row {row}")))?
                    .into_value();
                Ok(((*name).clone(), value))
            })
            .collect::<CommandResult<Vec<_>>>()
    };
    // Each value goes with its column, so the core writes it for the column's type.
    let cells = |cells: &[CellEdit]| {
        cells
            .iter()
            .map(|cell| {
                let column = store
                    .columns()
                    .get(cell.column)
                    .cloned()
                    .ok_or_else(|| DriverError::config(format!("no column {}", cell.column)))?;
                Ok((column, cell.value.clone()))
            })
            .collect::<CommandResult<Vec<_>>>()
    };

    let mut edits = Vec::new();
    for row in &request.deletes {
        edits.push(RowEdit::Delete { key: key_of(*row)? });
    }
    for update in &request.updates {
        let mut set = cells(&update.cells)?;
        for ((_, value), cell) in set.iter_mut().zip(&update.cells) {
            if let NewValue::Text(text) = value
                && let Some(Some(original)) = store.cell_text(update.row, cell.column)
            {
                *text = keep_line_endings(&original, text);
            }
        }
        edits.push(RowEdit::Update {
            key: key_of(update.row)?,
            set,
        });
    }
    for insert in &request.inserts {
        edits.push(RowEdit::Insert { values: cells(insert)? });
    }
    edits
        .iter()
        .map(|edit| edit_statement(grammar, &request.table, edit))
        .collect()
}

/// The statements a Save would run, for the preview.
#[tauri::command]
pub async fn edit_preview(
    state: State<'_, Arc<AppState>>,
    conn: u64,
    request: EditRequest,
) -> CommandResult<Vec<String>> {
    let server = state.server(conn)?;
    let grammar = server
        .engine
        .sql_grammar
        .ok_or_else(|| DriverError::unsupported("this engine has no tables to edit"))?;
    edit_statements(&state, grammar, &request)
}

/// Runs the previewed statements, all or none.
#[tauri::command]
pub async fn apply_edits(
    state: State<'_, Arc<AppState>>,
    conn: u64,
    database: Option<String>,
    statements: Vec<String>,
) -> CommandResult<()> {
    let server = state.server(conn)?;
    let session = server.session(database.as_deref()).await?;
    let saved = session
        .apply_edits(&statements, CallOptions::default().with_timeout(EDIT_TIMEOUT))
        .await;
    // Never saved twice: a lost session is only dropped, and the user saves again.
    if let Err(error) = &saved {
        server.recover(database.as_deref(), &session, error).await;
    }
    saved
}

/// How long saving edits may take: each statement names one row by its key.
const EDIT_TIMEOUT: Duration = Duration::from_secs(60);

/// Puts Mica behind the window (Settings › Mica background, ADR 0012), or takes it away, and says whether Mica is on.
///
/// The window is transparent, so the UI makes its chrome see-through only
/// when this returns `true`: on Windows 10 Mica cannot be applied, and a
/// see-through chrome would show the desktop. Tauri's own `set_effects`
/// drops that error, which is why the app calls `window-vibrancy` itself.
/// `dark` tints the backdrop to match the theme the UI shows.
#[tauri::command]
pub async fn set_mica(window: tauri::WebviewWindow, on: bool, dark: bool) -> CommandResult<bool> {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let target = window.clone();
    // DWM attributes belong to the thread that owns the window.
    window
        .run_on_main_thread(move || {
            let applied = if on {
                window_vibrancy::apply_mica(&target, Some(dark)).is_ok()
            } else {
                // Clearing what is not there is not an error worth showing.
                let _ = window_vibrancy::clear_mica(&target);
                false
            };
            let _ = sender.send(applied);
        })
        .map_err(|error| DriverError::internal(format!("the window did not answer: {error}")))?;
    receiver
        .await
        .map_err(|_| DriverError::internal("the window closed before Mica was set"))
}

// --- What the app remembers (docs/decisions/0019-data-folder.md) ------------

async fn in_background<T: Send + 'static>(
    work: impl FnOnce() -> CommandResult<T> + Send + 'static,
) -> CommandResult<T> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|error| DriverError::internal(error.to_string()))?
}

/// The query pages kept for a connection, or null.
#[tauri::command]
pub async fn read_pages(folder: State<'_, DataFolder>, key: String) -> CommandResult<Option<serde_json::Value>> {
    let folder = folder.inner().clone();
    in_background(move || folder.pages(&key)).await
}

/// Keeps a connection's query pages; resolves once they are on disk.
#[tauri::command]
pub async fn write_pages(folder: State<'_, DataFolder>, key: String, kept: serde_json::Value) -> CommandResult<()> {
    let folder = folder.inner().clone();
    in_background(move || {
        folder
            .write_pages(&key, kept)
            .map_err(|error| data::io_error("cannot keep the pages", error))
    })
    .await
}

/// Keeps the UI's preferences; resolves once they are on disk.
#[tauri::command]
pub async fn write_prefs(folder: State<'_, DataFolder>, prefs: serde_json::Value) -> CommandResult<()> {
    let folder = folder.inner().clone();
    in_background(move || {
        folder
            .write_prefs(&prefs)
            .map_err(|error| data::io_error("cannot keep the settings", error))
    })
    .await
}

/// Opens the data folder in Explorer.
#[tauri::command]
pub async fn open_data_folder(folder: State<'_, DataFolder>) -> CommandResult<()> {
    std::process::Command::new("explorer")
        .arg(&folder.path)
        .spawn()
        .map(|_| ())
        .map_err(|error| data::io_error("cannot open the folder", error))
}

// --- The terms (docs/decisions/0019-data-folder.md) ---------------------------

/// Where the terms and the privacy policy are read: Kemable's own site, on its
/// Cloudflare Pages address until a domain is bought (docs/decisions/0030-website.md).
const TERMS_OF_SERVICE: &str = "https://kemable.pages.dev/terms";
const PRIVACY_POLICY: &str = "https://kemable.pages.dev/privacy";

/// Opens the terms (`which`: "terms") or the privacy policy ("privacy") in the
/// browser. Only these two: the UI never hands a web address to the system.
#[tauri::command]
pub async fn open_terms(which: String) -> CommandResult<()> {
    let url = match which.as_str() {
        "terms" => TERMS_OF_SERVICE,
        "privacy" => PRIVACY_POLICY,
        _ => return Err(no_page(&which)),
    };
    open_in_browser(url)
}

// --- About (docs/decisions/0023-licensing.md) ---------------------------------

/// Where Kemable's source is: whoever has the app can read, build and share
/// the code it is built from (AGPL-3.0, section 6).
const SOURCE_CODE: &str = "https://github.com/SoleBuilt/Kemable";
/// The AGPL's text, installed beside the app (`bundle.resources` in tauri.conf.json,
/// from the repository's `LICENSE`), so it reads offline and before the source is public.
pub(crate) const LICENSE_FILE: &str = "LICENSE.txt";
/// The libraries and fonts inside the app with their licenses, installed beside
/// it (`bundle.resources` in tauri.conf.json, written by scripts/licenses.mjs).
pub(crate) const NOTICES: &str = "THIRD-PARTY-NOTICES.md";

/// Opens the source (`which`: "source") in the browser, or the license
/// ("license") or the third-party notices ("notices") in Notepad, which every
/// Windows has and which reads Markdown as the plain text it is.
#[tauri::command]
pub async fn open_about(app: tauri::AppHandle, which: String) -> CommandResult<()> {
    match which.as_str() {
        "source" => open_in_browser(SOURCE_CODE),
        "license" => open_installed(&app, LICENSE_FILE),
        "notices" => open_installed(&app, NOTICES),
        _ => Err(no_page(&which)),
    }
}

/// Opens a file installed beside the app in Notepad.
fn open_installed(app: &tauri::AppHandle, name: &str) -> CommandResult<()> {
    use tauri::Manager;
    let path = app
        .path()
        .resource_dir()
        .map_err(|error| data::io_error("cannot find the app's folder", error))?
        .join(name);
    if !path.is_file() {
        return Err(DriverError::new(
            kemable_core::ErrorCode::Io,
            format!("{} is not there", path.display()),
        ));
    }
    std::process::Command::new("notepad")
        .arg(&path)
        .spawn()
        .map(|_| ())
        .map_err(|error| data::io_error(&format!("cannot open {}", path.display()), error))
}

fn no_page(which: &str) -> DriverError {
    DriverError::new(kemable_core::ErrorCode::ConfigInvalid, format!("no page named {which}"))
}

/// Only addresses named in this file reach here: the UI never hands one to the system.
fn open_in_browser(url: &str) -> CommandResult<()> {
    std::process::Command::new("rundll32")
        .args(["url.dll,FileProtocolHandler", url])
        .spawn()
        .map(|_| ())
        .map_err(|error| data::io_error("cannot open the browser", error))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_files_stamp_moves_when_it_is_written_and_is_none_once_it_is_gone() {
        let folder = std::env::temp_dir().join(format!("kemable-stamps-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join("script.sql");
        std::fs::write(&path, "select 1;").unwrap();
        let first = FileStamp::of(&path).unwrap();
        assert_eq!(first.size, 9);
        assert_eq!(
            FileStamp::of(&path),
            Some(first.clone()),
            "reading the stamp changes nothing"
        );
        std::fs::write(&path, "select 12;").unwrap();
        let second = FileStamp::of(&path).unwrap();
        assert_ne!(second, first, "a write of another length is another stamp");
        std::fs::remove_file(&path).unwrap();
        assert_eq!(FileStamp::of(&path), None);
        assert_eq!(FileStamp::of(&folder), None, "a folder is not a file");
        std::fs::remove_dir_all(&folder).unwrap();
    }
}
