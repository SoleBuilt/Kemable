//! SQLite. Every call runs on the blocking pool, since rusqlite is
//! synchronous; a cancel or a deadline interrupts the statement through
//! `sqlite3_interrupt` (`core/crates/drivers/sqlite/AGENTS.md`).

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use async_trait::async_trait;
use paboable_core::catalog::{ColumnInfo, ColumnMeta, IndexInfo, ObjectInfo, ObjectKind, SchemaInfo, TableRef};
use paboable_core::config::{ConnectionConfig, Target};
use paboable_core::connection::Connection;
use paboable_core::driver::Driver;
use paboable_core::engine::{DataModel, EngineInfo, TargetKind};
use paboable_core::sink::{ExecOutcome, PUSH_BATCH_ROWS, ResultSink};
use paboable_core::sql::{SqlGrammar, quote_ident};
use paboable_core::value::{Value, ValueKind};
use paboable_core::{CallOptions, DriverError, ErrorCode, Result};
use rusqlite::types::ValueRef;
use rusqlite::{InterruptHandle, OpenFlags};

pub static INFO: EngineInfo = EngineInfo {
    id: "sqlite",
    display_name: "SQLite",
    data_model: DataModel::Sql,
    targets: &[TargetKind::File],
    default_port: None,
    sql_grammar: Some(SqlGrammar::Sqlite),
    live_topics: &[],
    has_databases: false,
    tree_folders: paboable_core::engine::SQL_TREE_FOLDERS,
};

/// How long a statement waits on another process's lock before it fails.
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

pub struct SqliteDriver;

#[async_trait]
impl Driver for SqliteDriver {
    fn info(&self) -> &'static EngineInfo {
        &INFO
    }

    async fn connect(&self, config: &ConnectionConfig, options: CallOptions) -> Result<Arc<dyn Connection>> {
        let Target::File { path, read_only } = &config.target else {
            return Err(DriverError::config("SQLite opens a file"));
        };
        let path = path.clone();
        let read_only = *read_only;
        options.check()?;
        let connection = tokio::task::spawn_blocking(move || -> Result<SqliteConnection> {
            let flags = if read_only {
                OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI | OpenFlags::SQLITE_OPEN_NO_MUTEX
            } else {
                OpenFlags::SQLITE_OPEN_READ_WRITE
                    | OpenFlags::SQLITE_OPEN_CREATE
                    | OpenFlags::SQLITE_OPEN_URI
                    | OpenFlags::SQLITE_OPEN_NO_MUTEX
            };
            let conn = rusqlite::Connection::open_with_flags(&path, flags).map_err(map_open_error)?;
            conn.busy_timeout(BUSY_TIMEOUT).map_err(map_error)?;
            let version: String = conn
                .query_row("select sqlite_version()", [], |row| row.get(0))
                .map_err(map_error)?;
            Ok(SqliteConnection {
                interrupt: Arc::new(conn.get_interrupt_handle()),
                conn: Arc::new(Mutex::new(conn)),
                version,
            })
        })
        .await
        .map_err(join_error)??;
        // A cancel that came while the file opened closes it.
        options.check()?;
        Ok(Arc::new(connection))
    }
}

pub struct SqliteConnection {
    conn: Arc<Mutex<rusqlite::Connection>>,
    interrupt: Arc<InterruptHandle>,
    version: String,
}

impl SqliteConnection {
    /// Runs `work` on the blocking pool with the connection, interrupting it
    /// when the call is cancelled or passes its deadline.
    async fn blocking<T: Send + 'static>(
        &self,
        options: &CallOptions,
        work: impl FnOnce(&rusqlite::Connection) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        options.check()?;
        let conn = Arc::clone(&self.conn);
        let task = tokio::task::spawn_blocking(move || {
            let conn = conn.lock().unwrap_or_else(PoisonError::into_inner);
            work(&conn)
        });
        let interrupt = Arc::clone(&self.interrupt);
        let watcher = {
            let options = options.clone();
            tokio::spawn(async move {
                options.stopped().await;
                interrupt.interrupt();
            })
        };
        let result = task.await.map_err(join_error);
        watcher.abort();
        match result? {
            Err(error) if error.code == ErrorCode::Cancelled => Err(options.stop_reason()),
            other => other,
        }
    }
}

#[async_trait]
impl Connection for SqliteConnection {
    fn engine(&self) -> &'static EngineInfo {
        &INFO
    }

    fn server_version(&self) -> Option<String> {
        Some(self.version.clone())
    }

    async fn ping(&self, options: CallOptions) -> Result<()> {
        self.blocking(&options, |conn| {
            conn.query_row("select 1", [], |_| Ok(())).map_err(map_error)
        })
        .await
    }

    async fn list_schemas(&self, options: CallOptions) -> Result<Vec<SchemaInfo>> {
        self.blocking(&options, |conn| {
            let mut statement = conn.prepare("pragma database_list").map_err(map_error)?;
            let names = statement
                .query_map([], |row| row.get::<_, String>(1))
                .map_err(map_error)?;
            names
                .map(|name| {
                    let name = name.map_err(map_error)?;
                    Ok(SchemaInfo {
                        is_default: name == "main",
                        name,
                    })
                })
                .collect()
        })
        .await
    }

    async fn list_objects(&self, schema: Option<&str>, options: CallOptions) -> Result<Vec<ObjectInfo>> {
        let schema = schema.unwrap_or("main").to_owned();
        self.blocking(&options, move |conn| {
            let sql = format!(
                "select type, name from {}.sqlite_schema \
                 where type in ('table', 'view', 'index', 'trigger') and name not like 'sqlite_%' \
                 order by case type when 'table' then 0 when 'view' then 1 when 'index' then 2 else 3 end, name",
                quote_ident(&schema)
            );
            let mut statement = conn.prepare(&sql).map_err(map_error)?;
            let rows = statement
                .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
                .map_err(map_error)?;
            rows.map(|row| {
                let (kind, name) = row.map_err(map_error)?;
                let kind = match kind.as_str() {
                    "table" => ObjectKind::Table,
                    "view" => ObjectKind::View,
                    "index" => ObjectKind::Index,
                    "trigger" => ObjectKind::Trigger,
                    _ => ObjectKind::Other,
                };
                Ok(ObjectInfo {
                    schema: Some(schema.clone()),
                    name,
                    kind,
                })
            })
            .collect()
        })
        .await
    }

    async fn fetch_columns(&self, table: &TableRef, options: CallOptions) -> Result<Vec<ColumnInfo>> {
        let schema = table.schema.clone().unwrap_or_else(|| "main".into());
        let name = table.name.clone();
        self.blocking(&options, move |conn| {
            let sql = format!("pragma {}.table_xinfo({})", quote_ident(&schema), quote_ident(&name));
            let mut statement = conn.prepare(&sql).map_err(map_error)?;
            let rows = statement
                .query_map([], |row| {
                    let primary_key: i64 = row.get(5)?;
                    // table_xinfo's `hidden`: 2 and 3 are generated columns.
                    let hidden: i64 = row.get(6)?;
                    Ok(ColumnInfo {
                        ordinal: u32::try_from(row.get::<_, i64>(0)?).unwrap_or(0),
                        name: row.get(1)?,
                        type_name: row.get(2)?,
                        nullable: row.get::<_, i64>(3)? == 0,
                        default: row.get(4)?,
                        primary_key: u32::try_from(primary_key).ok().filter(|&p| p > 0),
                        generated: hidden >= 2,
                    })
                })
                .map_err(map_error)?;
            let mut columns = rows.map(|row| row.map_err(map_error)).collect::<Result<Vec<_>>>()?;
            mark_rowid_alias(&mut columns);
            Ok(columns)
        })
        .await
    }

    async fn list_indexes(&self, table: &TableRef, options: CallOptions) -> Result<Vec<IndexInfo>> {
        let schema = table.schema.clone().unwrap_or_else(|| "main".into());
        let name = table.name.clone();
        self.blocking(&options, move |conn| {
            let list = format!("pragma {}.index_list({})", quote_ident(&schema), quote_ident(&name));
            let mut statement = conn.prepare(&list).map_err(map_error)?;
            // name, unique, origin (c: CREATE INDEX, u: UNIQUE, pk: PRIMARY KEY), partial
            let found = statement
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, i64>(4)?,
                    ))
                })
                .map_err(map_error)?
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(map_error)?;
            let mut indexes = Vec::with_capacity(found.len());
            for (index, unique, origin, partial) in found {
                let info = format!("pragma {}.index_xinfo({})", quote_ident(&schema), quote_ident(&index));
                let mut statement = conn.prepare(&info).map_err(map_error)?;
                // cid -2 is an expression; key 0 is the rowid the index carries, not a key.
                let parts = statement
                    .query_map([], |row| {
                        Ok((
                            row.get::<_, i64>(1)?,
                            row.get::<_, Option<String>>(2)?,
                            row.get::<_, i64>(3)?,
                            row.get::<_, i64>(5)?,
                        ))
                    })
                    .map_err(map_error)?
                    .collect::<std::result::Result<Vec<_>, _>>()
                    .map_err(map_error)?;
                let columns = parts
                    .into_iter()
                    .filter(|(_, _, _, key)| *key == 1)
                    .map(|(cid, column, descending, _)| {
                        let column = column.unwrap_or_else(|| {
                            if cid == -2 {
                                "(expression)".into()
                            } else {
                                "rowid".into()
                            }
                        });
                        if descending == 1 {
                            format!("{column} DESC")
                        } else {
                            column
                        }
                    })
                    .collect();
                let filter = if partial == 1 {
                    let sql: Option<String> = conn
                        .query_row(
                            &format!(
                                "select sql from {}.sqlite_schema where type = 'index' and name = ?1",
                                quote_ident(&schema)
                            ),
                            [&index],
                            |row| row.get(0),
                        )
                        .map_err(map_error)?;
                    sql.and_then(|sql| partial_filter(&sql))
                } else {
                    None
                };
                indexes.push(IndexInfo {
                    name: index,
                    columns,
                    included: Vec::new(),
                    unique: unique == 1,
                    primary: origin == "pk",
                    method: "btree".into(),
                    filter,
                });
            }
            indexes.sort_by_key(|index| (!index.primary, index.name.to_lowercase()));
            Ok(indexes)
        })
        .await
    }

    async fn object_ddl(&self, object: &ObjectInfo, options: CallOptions) -> Result<String> {
        let schema = object.schema.clone().unwrap_or_else(|| "main".into());
        let name = object.name.clone();
        self.blocking(&options, move |conn| {
            // SQLite keeps each object's CREATE statement as it was written; a
            // table's indexes and triggers follow it, since they go with it.
            let sql = format!(
                "select sql from {}.sqlite_schema                  where sql is not null and (name = ?1 or (tbl_name = ?1 and type in ('index', 'trigger') and name <> ?1))                  order by name <> ?1, type, name",
                quote_ident(&schema)
            );
            let mut statement = conn.prepare(&sql).map_err(map_error)?;
            let rows = statement
                .query_map([&name], |row| row.get::<_, String>(0))
                .map_err(map_error)?;
            let parts = rows
                .map(|row| row.map(|text| format!("{};", text.trim_end().trim_end_matches(';'))).map_err(map_error))
                .collect::<Result<Vec<_>>>()?;
            if parts.is_empty() {
                return Err(DriverError::not_found(format!("{name} in {schema}")));
            }
            Ok(parts.join("

"))
        })
        .await
    }

    async fn execute(&self, statement: &str, sink: Arc<dyn ResultSink>, options: CallOptions) -> Result<ExecOutcome> {
        let sql = statement.to_owned();
        self.blocking(&options, move |conn| run_statement(conn, &sql, sink.as_ref()))
            .await
    }

    async fn apply_edits(&self, statements: &[String], options: CallOptions) -> Result<()> {
        let statements = statements.to_vec();
        self.blocking(&options, move |conn| {
            // IMMEDIATE takes the write lock now, so a busy file fails before any row changes.
            conn.execute_batch("BEGIN IMMEDIATE").map_err(map_error)?;
            let applied = statements.iter().enumerate().try_for_each(|(index, statement)| {
                let changed = conn.execute(statement, []).map_err(map_error)?;
                if changed == 1 {
                    Ok(())
                } else {
                    Err(DriverError::row_gone(index, u64::try_from(changed).ok()))
                }
            });
            match applied {
                Ok(()) => conn.execute_batch("COMMIT").map_err(map_error),
                Err(error) => {
                    let _ = conn.execute_batch("ROLLBACK");
                    Err(error)
                }
            }
        })
        .await
    }

    async fn close(&self) -> Result<()> {
        // The file closes when the last handle drops.
        Ok(())
    }
}

fn run_statement(conn: &rusqlite::Connection, sql: &str, sink: &dyn ResultSink) -> Result<ExecOutcome> {
    let mut statement = conn.prepare(sql).map_err(map_error)?;
    let width = statement.column_count();
    if width == 0 {
        let changed = statement.raw_execute().map_err(map_error)?;
        sink.command_done(Some(changed as u64));
        return Ok(ExecOutcome::default());
    }

    let columns: Vec<ColumnMeta> = statement
        .columns()
        .map(|column| {
            let type_name = column.decl_type().unwrap_or_default().to_owned();
            ColumnMeta {
                name: column.name().to_owned(),
                kind: kind_of_declared(&type_name),
                type_name,
            }
        })
        .collect();
    sink.begin_set(columns);

    let mut rows = statement.raw_query();
    let mut batch = Vec::with_capacity(PUSH_BATCH_ROWS * width);
    let mut outcome = ExecOutcome::default();
    while let Some(row) = rows.next().map_err(map_error)? {
        for index in 0..width {
            batch.push(to_value(row.get_ref(index).map_err(map_error)?));
        }
        if batch.len() >= PUSH_BATCH_ROWS * width {
            if !sink.push_rows(&batch) {
                outcome.limited = true;
                break;
            }
            batch.clear();
        }
    }
    if !outcome.limited && !batch.is_empty() && !sink.push_rows(&batch) {
        outcome.limited = true;
    }
    sink.end_set();
    Ok(outcome)
}

fn to_value(value: ValueRef<'_>) -> Value {
    match value {
        ValueRef::Null => Value::Null,
        ValueRef::Integer(value) => Value::Int(value),
        ValueRef::Real(value) => Value::Float(value),
        ValueRef::Text(bytes) => Value::Text(String::from_utf8_lossy(bytes).into_owned()),
        ValueRef::Blob(bytes) => Value::Bytes(bytes.to_vec()),
    }
}

/// The kind a column's declared type suggests, by SQLite's affinity rules.
/// Each cell still carries the kind its value has.
fn kind_of_declared(type_name: &str) -> ValueKind {
    let upper = type_name.to_ascii_uppercase();
    if upper.contains("INT") {
        ValueKind::Int
    } else if upper.contains("CHAR") || upper.contains("CLOB") || upper.contains("TEXT") {
        ValueKind::Text
    } else if upper.contains("BLOB") {
        ValueKind::Bytes
    } else if upper.contains("REAL") || upper.contains("FLOA") || upper.contains("DOUB") {
        ValueKind::Float
    } else if upper.is_empty() {
        ValueKind::Other
    } else {
        ValueKind::Decimal
    }
}

fn map_open_error(error: rusqlite::Error) -> DriverError {
    let mapped = map_error(error);
    if mapped.code == ErrorCode::Query {
        DriverError {
            code: ErrorCode::Connect,
            ..mapped
        }
    } else {
        mapped
    }
}

fn map_error(error: rusqlite::Error) -> DriverError {
    use rusqlite::ffi::ErrorCode as Sqlite;
    match &error {
        rusqlite::Error::SqliteFailure(failure, message) => {
            let text = message.clone().unwrap_or_else(|| failure.to_string());
            let code = match failure.code {
                Sqlite::OperationInterrupted => ErrorCode::Cancelled,
                Sqlite::CannotOpen | Sqlite::NotADatabase => ErrorCode::Connect,
                Sqlite::PermissionDenied | Sqlite::ReadOnly | Sqlite::AuthorizationForStatementDenied => {
                    ErrorCode::Auth
                }
                Sqlite::SystemIoFailure | Sqlite::DiskFull => ErrorCode::Io,
                _ => ErrorCode::Query,
            };
            DriverError::new(code, text).with_engine_code(failure.extended_code.to_string())
        }
        rusqlite::Error::SqlInputError { msg, offset, .. } => {
            let error = DriverError::new(ErrorCode::Query, msg.clone());
            match usize::try_from(*offset) {
                Ok(offset) => error.with_position(offset),
                Err(_) => error,
            }
        }
        _ => DriverError::new(ErrorCode::Query, error.to_string()),
    }
}

fn join_error(error: tokio::task::JoinError) -> DriverError {
    DriverError::internal(format!("blocking task failed: {error}"))
}

#[cfg(test)]
mod tests;

/// A table's one `INTEGER PRIMARY KEY` column is its rowid: SQLite numbers
/// new rows itself, so the column counts as generated (D1 reads the same).
pub fn mark_rowid_alias(columns: &mut [ColumnInfo]) {
    let mut keys = columns.iter_mut().filter(|column| column.primary_key.is_some());
    if let (Some(only), None) = (keys.next(), keys.next())
        && only.type_name.eq_ignore_ascii_case("INTEGER")
    {
        only.generated = true;
    }
}

/// A partial index's condition: what follows the last WHERE of its CREATE INDEX.
pub fn partial_filter(sql: &str) -> Option<String> {
    let upper = sql.to_ascii_uppercase();
    let at = upper.rfind(" WHERE ")?;
    Some(sql[at + 7..].trim().trim_end_matches(';').trim().to_owned())
}
