//! A run: the statements of one script, executed in order on one server,
//! with what they return streamed to the UI.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use paboable_core::catalog::ColumnMeta;
use paboable_core::connection::Connection;
use paboable_core::sink::ResultSink;
use paboable_core::sql::{DatabaseChange, SqlGrammar, StatementKind, database_change};
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
        self.send(RunEvent::SetStarted {
            result: id,
            statement: self.statement(),
            columns,
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
    pub grammar: SqlGrammar,
    pub statements: Vec<Statement>,
    /// After `CREATE DATABASE x` succeeds, run the rest of the script in `x`.
    pub follow_created_database: bool,
    /// The most rows a set keeps; `None` keeps every row.
    pub row_limit: Option<u64>,
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

        let outcome = match statement.kind {
            StatementKind::Directive => directive(&run.server, text, &mut database),
            StatementKind::Sql => {
                // The server refuses to drop a database someone is connected
                // to; when that someone is this app, step out of it first.
                if run.server.engine.has_databases
                    && let Some(DatabaseChange::Dropped(name)) = database_change(text, run.grammar)
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
    let target = match database_change(text, run.grammar)? {
        DatabaseChange::Created(name) if run.follow_created_database => name,
        DatabaseChange::Used(name) => name,
        _ => return None,
    };
    *database = Some(target.clone());
    Some(target)
}

/// A client command. `\c name` moves the run to `name`; any other is refused,
/// since nothing would run it.
fn directive(server: &OpenServer, text: &str, database: &mut Option<String>) -> Result<Option<String>, DriverError> {
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
