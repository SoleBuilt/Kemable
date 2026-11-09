//! The query history: every statement the user ran from the editor, kept in
//! `history.db` (SQLite) in the app's folder, newest first.
//!
//! - A statement that carries a password (`CREATE USER ... PASSWORD '...'`,
//!   `ALTER LOGIN ... WITH PASSWORD = '...'`) is kept as a note, never as text.
//! - The history keeps at most [`MAX_ENTRIES`]; older ones go as new ones come.
//! - Runs the app writes itself (a sort, opening a table) are not recorded.

use std::path::Path;
use std::sync::{Mutex, MutexGuard, PoisonError};

use paboable_core::{DriverError, ErrorCode, Result};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

pub const MAX_ENTRIES: i64 = 20_000;

/// What the text of a statement holding a password is replaced with.
pub const WITHHELD: &str = "-- not kept: this statement holds a password";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Outcome {
    Succeeded,
    Failed,
    Cancelled,
}

impl Outcome {
    fn as_str(self) -> &'static str {
        match self {
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    fn parse(text: &str) -> Self {
        match text {
            "failed" => Self::Failed,
            "cancelled" => Self::Cancelled,
            _ => Self::Succeeded,
        }
    }
}

/// One statement that ran.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    /// Zero for an entry not recorded yet.
    #[serde(default)]
    pub id: i64,
    /// Milliseconds since the Unix epoch.
    pub ran_at: i64,
    /// The connection's title, or its saved name.
    pub connection: String,
    pub engine: String,
    pub database: Option<String>,
    pub statement: String,
    pub duration_ms: i64,
    /// Rows returned, or rows affected when the engine says.
    pub rows: Option<i64>,
    pub outcome: Outcome,
    pub error: Option<String>,
}

/// What to look for: text in the statement, one connection, a page.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryQuery {
    #[serde(default)]
    pub text: String,
    pub connection: Option<String>,
    /// Entries older than this id; none for the newest.
    pub before: Option<i64>,
    pub limit: Option<u32>,
}

pub struct QueryHistory {
    connection: Mutex<Connection>,
}

fn sql_error(error: rusqlite::Error) -> DriverError {
    DriverError::new(ErrorCode::Io, format!("query history: {error}"))
}

/// Whether a statement may carry a password, by the words engines use for one.
pub fn holds_password(statement: &str) -> bool {
    let upper = statement.to_ascii_uppercase();
    ["PASSWORD", "IDENTIFIED BY", "SECRET "]
        .iter()
        .any(|word| upper.contains(word))
        && (statement.contains('\'') || statement.contains('"') || statement.contains('$'))
}

impl QueryHistory {
    /// Opens `history.db` in `folder`, creating the folder and the file.
    pub fn open(folder: &Path) -> Result<Self> {
        std::fs::create_dir_all(folder)
            .map_err(|error| DriverError::new(ErrorCode::Io, format!("cannot create {}: {error}", folder.display())))?;
        Self::with(Connection::open(folder.join("history.db")).map_err(sql_error)?)
    }

    /// A history in memory, for tests.
    pub fn in_memory() -> Result<Self> {
        Self::with(Connection::open_in_memory().map_err(sql_error)?)
    }

    fn with(connection: Connection) -> Result<Self> {
        connection
            .execute_batch(
                "pragma journal_mode = wal;
                 create table if not exists entries (
                     id integer primary key,
                     ran_at integer not null,
                     connection text not null,
                     engine text not null,
                     database text,
                     statement text not null,
                     duration_ms integer not null,
                     rows integer,
                     outcome text not null,
                     error text
                 );
                 create index if not exists entries_by_connection on entries (connection, id);",
            )
            .map_err(sql_error)?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    fn lock(&self) -> MutexGuard<'_, Connection> {
        self.connection.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Records one statement and returns its id. A statement holding a
    /// password is kept as [`WITHHELD`].
    pub fn record(&self, entry: &HistoryEntry) -> Result<i64> {
        let statement = if holds_password(&entry.statement) {
            WITHHELD
        } else {
            entry.statement.as_str()
        };
        let connection = self.lock();
        connection
            .execute(
                "insert into entries (ran_at, connection, engine, database, statement, duration_ms, rows, outcome, error)
                 values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    entry.ran_at,
                    entry.connection,
                    entry.engine,
                    entry.database,
                    statement,
                    entry.duration_ms,
                    entry.rows,
                    entry.outcome.as_str(),
                    entry.error,
                ],
            )
            .map_err(sql_error)?;
        let id = connection.last_insert_rowid();
        // Trim now and then, not on every insert.
        if id % 500 == 0 {
            connection
                .execute("delete from entries where id <= ?1", params![id - MAX_ENTRIES])
                .map_err(sql_error)?;
        }
        Ok(id)
    }

    /// Newest first, filtered by text (any case) and connection.
    pub fn search(&self, query: &HistoryQuery) -> Result<Vec<HistoryEntry>> {
        let limit = i64::from(query.limit.unwrap_or(200).clamp(1, 1000));
        let pattern = format!(
            "%{}%",
            query.text.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")
        );
        let connection = self.lock();
        let mut statement = connection
            .prepare(
                "select id, ran_at, connection, engine, database, statement, duration_ms, rows, outcome, error
                 from entries
                 where (?1 = '%%' or statement like ?1 escape '\\')
                   and (?2 is null or connection = ?2)
                   and (?3 is null or id < ?3)
                 order by id desc
                 limit ?4",
            )
            .map_err(sql_error)?;
        let rows = statement
            .query_map(params![pattern, query.connection, query.before, limit], |row| {
                Ok(HistoryEntry {
                    id: row.get(0)?,
                    ran_at: row.get(1)?,
                    connection: row.get(2)?,
                    engine: row.get(3)?,
                    database: row.get(4)?,
                    statement: row.get(5)?,
                    duration_ms: row.get(6)?,
                    rows: row.get(7)?,
                    outcome: Outcome::parse(&row.get::<_, String>(8)?),
                    error: row.get(9)?,
                })
            })
            .map_err(sql_error)?;
        rows.map(|row| row.map_err(sql_error)).collect()
    }

    pub fn delete(&self, id: i64) -> Result<()> {
        self.lock()
            .execute("delete from entries where id = ?1", params![id])
            .map(drop)
            .map_err(sql_error)
    }

    pub fn clear(&self) -> Result<()> {
        self.lock()
            .execute("delete from entries", [])
            .map(drop)
            .map_err(sql_error)
    }

    pub fn len(&self) -> Result<i64> {
        self.lock()
            .query_row("select count(*) from entries", [], |row| row.get(0))
            .optional()
            .map(|count| count.unwrap_or(0))
            .map_err(sql_error)
    }

    pub fn is_empty(&self) -> Result<bool> {
        Ok(self.len()? == 0)
    }
}
