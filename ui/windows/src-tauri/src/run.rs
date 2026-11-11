//! A run: the statements of one script, executed in order on one server,
//! with what they return streamed to the UI.

use std::ops::Range;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use paboable_core::catalog::ColumnMeta;
use paboable_core::command::select_database;
use paboable_core::connection::Connection;
use paboable_core::sink::ResultSink;
use paboable_core::sql::{DatabaseChange, SqlGrammar, StatementKind, batch_queries, database_change};
use paboable_core::value::Value;
use paboable_core::{CallOptions, DriverError};
use paboable_store::ResultStore;
use serde::Serialize;
use tauri::ipc::Channel;
use tokio_util::sync::CancellationToken;

use crate::state::{AppState, OpenServer, ResultOrigin};

/// Row counts reach the UI at most this often. Rows arrive at wire speed into
/// the store; the UI only needs to know how many there are, about once a frame
/// or two, so a fast server never floods the webview with events.
const ROWS_EVENT_INTERVAL: Duration = Duration::from_millis(50);

/// How much of a statement's text an event carries, for the Messages tab.
const STATEMENT_PREVIEW_CHARS: usize = 200;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum RunEvent {
    StatementStarted {
        index: usize,
        text: String,
    },
    SetStarted {
        result: u64,
        statement: usize,
        columns: Vec<ColumnMeta>,
        /// The statement inside a SQL Server batch that made this set, as
        /// byte offsets into the batch's text; `None` when it cannot be told.
        part: Option<Part>,
    },
    /// The sets of `statement` did not match the statements its batch was
    /// read as (one failed, say): their `part`s are wrong, so drop them.
    PartsDropped {
        statement: usize,
    },
    Rows {
        result: u64,
        total: u64,
    },
    SetFinished {
        result: u64,
        total: u64,
        limited: bool,
    },
    CommandDone {
        statement: usize,
        rows_affected: Option<u64>,
    },
    Notice {
        statement: usize,
        text: String,
    },
    /// The statements after `statement` run in `database`: it was created
    /// and followed, or a directive or `USE` switched to it.
    DatabaseChanged {
        statement: usize,
        database: String,
    },
    StatementFailed {
        statement: usize,
        error: DriverError,
    },
    Finished {
        elapsed_ms: u64,
        cancelled: bool,
        failed: bool,
    },
}

/// Byte offsets into a statement's text.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Part {
    start: usize,
    end: usize,
}

struct OpenSet {
    id: u64,
    store: Arc<ResultStore>,
    last_sent: Option<Instant>,
}

/// The sink a driver writes into: each set becomes a store the grid reads by
/// id, and events tell the UI what changed.
struct RunSink {
    state: Arc<AppState>,
    channel: Channel<RunEvent>,
    statement: AtomicUsize,
    open: Mutex<Option<OpenSet>>,
    /// What a set started now came from; updated before each statement.
    origin: Mutex<ResultOrigin>,
    row_limit: Option<u64>,
    /// Rows the current statement returned, or affected, for the history.
    rows: Mutex<Option<i64>>,
    /// The statements of the current SQL Server batch that each make one set
    /// (`paboable_core::sql::batch_queries`), and how many sets it made so far.
    parts: Mutex<Option<Vec<Range<usize>>>>,
    sets_here: AtomicUsize,
}

impl RunSink {
    fn send(&self, event: RunEvent) {
        // A closed window drops the channel; the run then ends on its own.
        let _ = self.channel.send(event);
    }

    fn statement(&self) -> usize {
        self.statement.load(Ordering::Relaxed)
    }

    fn open(&self) -> std::sync::MutexGuard<'_, Option<OpenSet>> {
        self.open.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Ends a set a failed statement left open, so the UI shows the rows it had.
    fn close_open_set(&self) {
        let open = self.open().take();
        if let Some(set) = open {
            self.finish(set);
        }
    }

    fn finish(&self, set: OpenSet) {
        set.store.finish();
        let state = set.store.state();
        let mut rows = self.rows.lock().unwrap_or_else(PoisonError::into_inner);
        *rows = Some(rows.unwrap_or(0) + i64::try_from(state.total_rows).unwrap_or(i64::MAX));
        drop(rows);
        self.send(RunEvent::SetFinished {
            result: set.id,
            total: state.total_rows,
            limited: state.limited,
        });
    }
}

impl ResultSink for RunSink {
    fn begin_set(&self, columns: Vec<ColumnMeta>) {
        self.close_open_set();
        let store = Arc::new(ResultStore::new(columns.clone(), self.row_limit));
        let origin = self.origin.lock().unwrap_or_else(PoisonError::into_inner).clone();
        let id = self.state.add_result(Arc::clone(&store), Some(origin));
        let made = self.sets_here.fetch_add(1, Ordering::Relaxed);
        let part = self
            .parts
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .and_then(|parts| parts.get(made))
            .map(|range| Part {
                start: range.start,
                end: range.end,
            });
        self.send(RunEvent::SetStarted {
            result: id,
            statement: self.statement(),
            columns,
            part,
        });
        *self.open() = Some(OpenSet {
            id,
            store,
            last_sent: None,
        });
    }

    fn push_rows(&self, values: &[Value]) -> bool {
        let mut open = self.open();
        let Some(set) = open.as_mut() else { return false };
        let more = set.store.push_rows(values);
        if set.last_sent.is_none_or(|sent| sent.elapsed() >= ROWS_EVENT_INTERVAL) {
            set.last_sent = Some(Instant::now());
            let total = set.store.state().total_rows;
            self.send(RunEvent::Rows { result: set.id, total });
        }
        more
    }

    fn end_set(&self) {
        self.close_open_set();
    }

    fn command_done(&self, rows_affected: Option<u64>) {
        if let Some(affected) = rows_affected {
            *self.rows.lock().unwrap_or_else(PoisonError::into_inner) =
                Some(i64::try_from(affected).unwrap_or(i64::MAX));
        }
        self.send(RunEvent::CommandDone {
            statement: self.statement(),
            rows_affected,
        });
    }

    fn notice(&self, text: &str) {
        self.send(RunEvent::Notice {
            statement: self.statement(),
            text: text.to_owned(),
        });
    }
}

pub struct Statement {
    pub text: String,
    pub kind: StatementKind,
}

pub struct Run {
    pub id: u64,
    pub server: Arc<OpenServer>,
    /// The database the run starts in; `None` for the home database.
    pub database: Option<String>,
    /// How the text was split; `None` for a console of commands (Redis).
    pub grammar: Option<SqlGrammar>,
    pub statements: Vec<Statement>,
    /// After `CREATE DATABASE x` succeeds, run the rest of the script in `x`.
    pub follow_created_database: bool,
    /// The most rows a set keeps; `None` keeps every row.
    pub row_limit: Option<u64>,
    /// Record each statement in the query history (a run from the editor).
    pub record: bool,
    pub cancel: CancellationToken,
    pub channel: Channel<RunEvent>,
}

/// Executes each statement in turn. The first failure stops the rest, as the
/// engines' own clients do.
pub async fn execute(state: Arc<AppState>, run: Run) {
    let started = Instant::now();
    let sink = Arc::new(RunSink {
        state: Arc::clone(&state),
        channel: run.channel.clone(),
        statement: AtomicUsize::new(0),
        open: Mutex::new(None),
        origin: Mutex::new(ResultOrigin {
            database: run.database.clone(),
            grammar: run.grammar,
            statement: String::new(),
            alone: run.statements.len() == 1,
            row_limit: run.row_limit,
        }),
        row_limit: run.row_limit,
        rows: Mutex::new(None),
        parts: Mutex::new(None),
        sets_here: AtomicUsize::new(0),
    });
    let mut database = run.database.clone();
    let mut failed = false;

    for (index, statement) in run.statements.iter().enumerate() {
        if run.cancel.is_cancelled() {
            break;
        }
        sink.statement.store(index, Ordering::Relaxed);
        let text = statement.text.as_str();
        {
            let mut origin = sink.origin.lock().unwrap_or_else(PoisonError::into_inner);
            origin.statement = text.to_owned();
            origin.database = database.clone();
        }
        sink.send(RunEvent::StatementStarted {
            index,
            text: text.chars().take(STATEMENT_PREVIEW_CHARS).collect(),
        });

        *sink.rows.lock().unwrap_or_else(PoisonError::into_inner) = None;
        // A batch's results come back in order; reading the batch tells which statement made each.
        let parts = (run.grammar == Some(SqlGrammar::SqlServer) && statement.kind == StatementKind::Sql)
            .then(|| batch_queries(text))
            .flatten();
        *sink.parts.lock().unwrap_or_else(PoisonError::into_inner) = parts;
        sink.sets_here.store(0, Ordering::Relaxed);
        let statement_started = Instant::now();
        let ran_in = database.clone();
        let outcome = match statement.kind {
            StatementKind::Directive => directive(&run, text, &mut database),
            StatementKind::Sql => {
                // The server refuses to drop a database someone is connected
                // to; when that someone is this app, step out of it first.
                if run.server.engine.has_databases
                    && let Some(grammar) = run.grammar
                    && let Some(DatabaseChange::Dropped(name)) = database_change(text, grammar)
                    && database.as_deref() != Some(name.as_str())
                {
                    run.server.close_session(&name).await;
                }
                let shared: Arc<dyn ResultSink> = sink.clone();
                statement_in(&run, text, &database, shared, run.cancel.child_token())
                    .await
                    .map(|()| follow(&run, text, &mut database))
            }
        };
        let made = sink.sets_here.load(Ordering::Relaxed);
        let expected = sink
            .parts
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .map(Vec::len);
        if expected.is_some_and(|expected| made > 0 && made != expected) {
            sink.send(RunEvent::PartsDropped { statement: index });
        }
        if run.record && statement.kind == StatementKind::Sql {
            let rows = *sink.rows.lock().unwrap_or_else(PoisonError::into_inner);
            record(
                &state,
                &run,
                text,
                ran_in,
                statement_started,
                rows,
                outcome.as_ref().err(),
            );
        }
        match outcome {
            Ok(Some(target)) => {
                // Open the session now, so a database that cannot be reached
                // fails here, where the switch is, and not on the next statement.
                if let Err(error) = run.server.session(Some(&target)).await {
                    sink.send(RunEvent::StatementFailed {
                        statement: index,
                        error,
                    });
                    failed = true;
                    break;
                }
                sink.send(RunEvent::DatabaseChanged {
                    statement: index,
                    database: target,
                });
            }
            Ok(None) => {}
            Err(error) => {
                sink.close_open_set();
                sink.send(RunEvent::StatementFailed {
                    statement: index,
                    error,
                });
                failed = true;
                break;
            }
        }
    }

    state.remove_run(run.id);
    sink.send(RunEvent::Finished {
        elapsed_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        cancelled: run.cancel.is_cancelled(),
        failed,
    });
}

async fn statement_in(
    run: &Run,
    text: &str,
    database: &Option<String>,
    sink: Arc<dyn ResultSink>,
    cancel: CancellationToken,
) -> Result<(), DriverError> {
    let session: Arc<dyn Connection> = run.server.session(database.as_deref()).await?;
    session.execute(text, sink, CallOptions::new(cancel)).await.map(drop)
}

/// The database a successful statement moves the run to, if any.
fn follow(run: &Run, text: &str, database: &mut Option<String>) -> Option<String> {
    if !run.server.engine.has_databases {
        return None;
    }
    let target = match database_change(text, run.grammar?)? {
        DatabaseChange::Created(name) if run.follow_created_database => name,
        DatabaseChange::Used(name) => name,
        _ => return None,
    };
    *database = Some(target.clone());
    Some(target)
}

/// A client command. `\c name` moves the run to `name`; any other is refused,
/// since nothing would run it.
fn directive(run: &Run, text: &str, database: &mut Option<String>) -> Result<Option<String>, DriverError> {
    let server = &run.server;
    // A console's `SELECT n` switches as psql's `c` does: the session for n runs what follows.
    if run.grammar.is_none() {
        let target =
            select_database(text).ok_or_else(|| DriverError::unsupported(format!("{text} is not a directive")))?;
        *database = Some(target.clone());
        return Ok(Some(target));
    }
    match database_change(text, SqlGrammar::Postgres) {
        Some(DatabaseChange::Connect(name)) if server.engine.has_databases => {
            *database = Some(name.clone());
            Ok(Some(name))
        }
        _ => Err(DriverError::unsupported(format!(
            "{text} is a psql command Paboable does not run"
        ))),
    }
}

/// Adds one statement to the query history, off the async workers. A history
/// that cannot be written is not a reason to fail the run.
fn record(
    state: &AppState,
    run: &Run,
    statement: &str,
    database: Option<String>,
    started: Instant,
    rows: Option<i64>,
    error: Option<&DriverError>,
) {
    use paboable_storage::history::{HistoryEntry, Outcome};
    let Some(history) = state.history.get().cloned() else {
        return;
    };
    let ran_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX));
    let entry = HistoryEntry {
        id: 0,
        ran_at,
        connection: run.server.title.clone(),
        engine: run.server.engine.id.to_owned(),
        database,
        statement: statement.to_owned(),
        duration_ms: i64::try_from(started.elapsed().as_millis()).unwrap_or(i64::MAX),
        rows,
        outcome: match error {
            None => Outcome::Succeeded,
            Some(error) if error.code == paboable_core::ErrorCode::Cancelled => Outcome::Cancelled,
            Some(_) => Outcome::Failed,
        },
        error: error.map(|error| error.message.clone()),
    };
    tauri::async_runtime::spawn_blocking(move || {
        let _ = history.record(&entry);
    });
}
