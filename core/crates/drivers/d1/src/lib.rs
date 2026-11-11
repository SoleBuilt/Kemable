//! Cloudflare D1, a SQLite database behind Cloudflare's REST API
//! (`docs/decisions/0009-d1-over-http.md`).
//!
//! The target is `Target::Http { endpoint: <account ID>, token }`; the
//! database is the `database` option, a name or a UUID. Each database of the
//! account is one session, as PostgreSQL's are. A statement is one `POST
//! .../raw`: no session state survives between two, so each is its own
//! transaction. Stop closes the request; the statement may still finish on
//! Cloudflare's side.

mod api;

#[cfg(test)]
mod tests;

use std::sync::Arc;

use async_trait::async_trait;
use paboable_core::catalog::{ColumnInfo, ColumnMeta, DatabaseInfo, ObjectInfo, ObjectKind, SchemaInfo, TableRef};
use paboable_core::config::{ConnectionConfig, Target};
use paboable_core::connection::Connection;
use paboable_core::driver::Driver;
use paboable_core::engine::{DataModel, EngineInfo, TargetKind};
use paboable_core::sink::{ExecOutcome, PUSH_BATCH_ROWS, ResultSink};
use paboable_core::sql::{SqlGrammar, quote_ident};
use paboable_core::value::{Value, ValueKind};
use paboable_core::{CallOptions, DriverError, ErrorCode, Result};
use serde_json::Value as Json;

use crate::api::{Api, CLOUDFLARE_API, RawResult};

pub static INFO: EngineInfo = EngineInfo {
    id: "d1",
    display_name: "Cloudflare D1",
    data_model: DataModel::Sql,
    targets: &[TargetKind::Http],
    default_port: None,
    sql_grammar: Some(SqlGrammar::Sqlite),
    live: false,
    has_databases: true,
};

/// Fails, and with it the batch, unless the statement before it changed one row:
/// `json` refuses text that is not JSON.
const ROW_GUARD: &str = "SELECT CASE WHEN changes() = 1 THEN 1 ELSE json('paboable: the row is gone') END";
/// What SQLite says when the guard fails.
const ROW_GUARD_ERROR: &str = "malformed JSON";

pub struct D1Driver;

#[async_trait]
impl Driver for D1Driver {
    fn info(&self) -> &'static EngineInfo {
        &INFO
    }

    async fn connect(&self, config: &ConnectionConfig, options: CallOptions) -> Result<Arc<dyn Connection>> {
        let Target::Http { endpoint, token } = &config.target else {
            return Err(DriverError::config("D1 is reached through Cloudflare's API"));
        };
        let base = config.options.get("api_base").map_or(CLOUDFLARE_API, String::as_str);
        let api = Arc::new(Api::new(base, endpoint, token.clone())?);
        let wanted = config
            .options
            .get("database")
            .map(|name| name.trim().to_owned())
            .filter(|name| !name.is_empty());
        let database = match wanted {
            // The token is checked either way: by finding the database, or by listing them.
            Some(wanted) => Some(options.run(resolve(&api, &wanted)).await?),
            None => {
                options.run(api.databases(None, 1)).await?;
                None
            }
        };
        Ok(Arc::new(D1Connection { api, database }))
    }
}

/// The database `wanted` names, by UUID or by name.
async fn resolve(api: &Api, wanted: &str) -> Result<Database> {
    let looks_like_id = wanted.len() == 36 && wanted.chars().all(|ch| ch.is_ascii_hexdigit() || ch == '-');
    let (entries, _) = api.databases((!looks_like_id).then_some(wanted), 1).await?;
    let found = entries
        .into_iter()
        .find(|entry| entry.name == wanted || entry.uuid.eq_ignore_ascii_case(wanted));
    match found {
        Some(entry) => Ok(Database {
            id: entry.uuid,
            name: entry.name,
        }),
        // A UUID the list did not show (another page) is tried as it is.
        None if looks_like_id => Ok(Database {
            id: wanted.to_owned(),
            name: wanted.to_owned(),
        }),
        None => Err(DriverError::not_found(format!("D1 database {wanted}"))),
    }
}

struct Database {
    id: String,
    name: String,
}

pub struct D1Connection {
    api: Arc<Api>,
    /// `None` for the account's own session, which lists databases and runs nothing.
    database: Option<Database>,
}

impl D1Connection {
    fn database(&self) -> Result<&Database> {
        self.database
            .as_ref()
            .ok_or_else(|| DriverError::config("pick a D1 database first (Ctrl+K)"))
    }

    async fn query(&self, sql: &str, options: &CallOptions) -> Result<Vec<RawResult>> {
        let database = self.database()?;
        options.run(self.api.raw(&database.id, sql)).await
    }

    /// The rows of a catalog query, as JSON values.
    async fn rows(&self, sql: &str, options: &CallOptions) -> Result<Vec<Vec<Json>>> {
        let mut results = self.query(sql, options).await?;
        Ok(results
            .pop()
            .and_then(|result| result.results)
            .map(|rows| rows.rows)
            .unwrap_or_default())
    }
}

/// A JSON value from D1 as a `Value`: SQLite's integer, real, text, NULL, and
/// a blob as D1 writes it, an array of byte numbers.
fn value(json: Json) -> Value {
    match json {
        Json::Null => Value::Null,
        Json::Bool(value) => Value::Bool(value),
        Json::Number(number) => {
            if let Some(value) = number.as_i64() {
                Value::Int(value)
            } else if let Some(value) = number.as_u64() {
                Value::UInt(value)
            } else {
                Value::Float(number.as_f64().unwrap_or(f64::NAN))
            }
        }
        Json::String(text) => Value::Text(text),
        Json::Array(items) => {
            let bytes: Option<Vec<u8>> = items
                .iter()
                .map(|item| item.as_u64().and_then(|byte| u8::try_from(byte).ok()))
                .collect();
            match bytes {
                Some(bytes) => Value::Bytes(bytes),
                None => Value::Json(Json::Array(items).to_string()),
            }
        }
        Json::Object(map) => Value::Json(Json::Object(map).to_string()),
    }
}

fn text(json: &Json) -> String {
    match json {
        Json::String(text) => text.clone(),
        Json::Null => String::new(),
        other => other.to_string(),
    }
}

#[async_trait]
impl Connection for D1Connection {
    fn engine(&self) -> &'static EngineInfo {
        &INFO
    }

    fn server_version(&self) -> Option<String> {
        None
    }

    fn current_database(&self) -> Option<String> {
        self.database.as_ref().map(|database| database.name.clone())
    }

    async fn ping(&self, options: CallOptions) -> Result<()> {
        options.run(self.api.databases(None, 1)).await.map(drop)
    }

    async fn list_databases(&self, options: CallOptions) -> Result<Vec<DatabaseInfo>> {
        let current = self.current_database();
        let mut names = Vec::new();
        for page in 1..=100 {
            let (entries, more) = options.run(self.api.databases(None, page)).await?;
            names.extend(entries.into_iter().map(|entry| entry.name));
            if !more {
                break;
            }
        }
        names.sort();
        Ok(names
            .into_iter()
            .map(|name| DatabaseInfo {
                is_current: current.as_deref() == Some(name.as_str()),
                name,
            })
            .collect())
    }

    async fn list_schemas(&self, _options: CallOptions) -> Result<Vec<SchemaInfo>> {
        Ok(vec![SchemaInfo {
            name: "main".into(),
            is_default: true,
        }])
    }

    /// D1's own tables (`_cf_KV` and the like) cannot be read, so they are left out.
    async fn list_objects(&self, _schema: Option<&str>, options: CallOptions) -> Result<Vec<ObjectInfo>> {
        let sql = "select type, name from sqlite_schema \
                   where type in ('table', 'view', 'index', 'trigger') \
                   and name not like 'sqlite_%' and name not like '\\_cf\\_%' escape '\\' \
                   order by case type when 'table' then 0 when 'view' then 1 when 'index' then 2 else 3 end, name";
        let rows = self.rows(sql, &options).await?;
        Ok(rows
            .iter()
            .map(|row| {
                let kind = match row.first().map(text).as_deref() {
                    Some("table") => ObjectKind::Table,
                    Some("view") => ObjectKind::View,
                    Some("index") => ObjectKind::Index,
                    Some("trigger") => ObjectKind::Trigger,
                    _ => ObjectKind::Other,
                };
                ObjectInfo {
                    schema: Some("main".into()),
                    name: row.get(1).map(text).unwrap_or_default(),
                    kind,
                }
            })
            .collect())
    }

    async fn fetch_columns(&self, table: &TableRef, options: CallOptions) -> Result<Vec<ColumnInfo>> {
        let sql = format!("PRAGMA table_info({})", quote_ident(&table.name));
        let rows = self.rows(&sql, &options).await?;
        Ok(rows
            .iter()
            .map(|row| {
                let number = |index: usize| row.get(index).and_then(Json::as_i64).unwrap_or(0);
                ColumnInfo {
                    ordinal: u32::try_from(number(0) + 1).unwrap_or(0),
                    name: row.get(1).map(text).unwrap_or_default(),
                    type_name: row.get(2).map(text).unwrap_or_default(),
                    nullable: number(3) == 0,
                    default: row.get(4).filter(|value| !value.is_null()).map(text),
                    primary_key: u32::try_from(number(5)).ok().filter(|position| *position > 0),
                }
            })
            .collect())
    }

    /// The CREATE statements D1 keeps, as SQLite does: the object's own, and
    /// for a table its indexes and triggers after it.
    async fn object_ddl(&self, object: &ObjectInfo, options: CallOptions) -> Result<String> {
        let name = text_literal(&object.name);
        let sql = format!(
            "select sql from sqlite_schema where sql is not null              and (name = {name} or (tbl_name = {name} and type in ('index', 'trigger') and name <> {name}))              order by name <> {name}, type, name"
        );
        let rows = self.rows(&sql, &options).await?;
        let parts: Vec<String> = rows
            .iter()
            .filter_map(|row| row.first().map(text))
            .map(|create| format!("{};", create.trim_end().trim_end_matches(';')))
            .collect();
        if parts.is_empty() {
            return Err(DriverError::not_found(object.name.clone()));
        }
        Ok(parts.join(
            "

",
        ))
    }

    async fn execute(&self, statement: &str, sink: Arc<dyn ResultSink>, options: CallOptions) -> Result<ExecOutcome> {
        let results = self.query(statement, &options).await?;
        let mut outcome = ExecOutcome::default();
        for result in results {
            if let Some(meta) = &result.meta {
                sink.notice(&format!(
                    "D1: {} rows read, {} written, {:.1} ms",
                    meta.rows_read.unwrap_or(0),
                    meta.rows_written.unwrap_or(0),
                    meta.duration.unwrap_or(0.0)
                ));
            }
            let rows = result.results.unwrap_or_default();
            if rows.columns.is_empty() {
                let changes = result.meta.as_ref().and_then(|meta| meta.changes);
                sink.command_done(changes);
                continue;
            }
            // D1 sends no column types: each column takes the kind of its first value.
            let columns = rows
                .columns
                .iter()
                .enumerate()
                .map(|(index, name)| ColumnMeta {
                    name: name.clone(),
                    type_name: String::new(),
                    kind: rows
                        .rows
                        .iter()
                        .filter_map(|row| row.get(index))
                        .find(|cell| !cell.is_null())
                        .map_or(ValueKind::Null, |cell| value(cell.clone()).kind()),
                })
                .collect();
            sink.begin_set(columns);
            let width = rows.columns.len();
            let mut batch = Vec::with_capacity(width * PUSH_BATCH_ROWS.min(rows.rows.len().max(1)));
            for row in rows.rows {
                let mut cells = row.into_iter();
                batch.extend((0..width).map(|_| value(cells.next().unwrap_or(Json::Null))));
                if batch.len() >= width * PUSH_BATCH_ROWS {
                    if !sink.push_rows(&batch) {
                        outcome.limited = true;
                        batch.clear();
                        break;
                    }
                    batch.clear();
                }
            }
            if !batch.is_empty() && !sink.push_rows(&batch) {
                outcome.limited = true;
            }
            sink.end_set();
        }
        Ok(outcome)
    }

    /// One batch, which D1 runs as one transaction. D1 cannot be asked to roll
    /// back from outside, so a guard after each statement fails the batch when
    /// the statement changed another number of rows than one.
    async fn apply_edits(&self, statements: &[String], options: CallOptions) -> Result<()> {
        let database = self.database()?;
        let mut batch = Vec::with_capacity(statements.len() * 2);
        for statement in statements {
            batch.push(statement.clone());
            batch.push(ROW_GUARD.to_owned());
        }
        match options.run(self.api.batch(&database.id, &batch)).await {
            Ok(_) => Ok(()),
            Err(error) if error.message.contains(ROW_GUARD_ERROR) => Err(DriverError::new(
                ErrorCode::RowGone,
                "a statement changed another number of rows than one",
            )),
            Err(error) => Err(error),
        }
    }

    async fn close(&self) -> Result<()> {
        Ok(())
    }
}

/// Text as a SQLite string literal: in single quotes, its quotes doubled.
/// D1's query endpoint here takes no parameters, so a name goes in this way.
fn text_literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}
