use std::collections::BTreeMap;
use std::future::poll_fn;
use std::pin::pin;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use async_trait::async_trait;
use futures_util::StreamExt;
use paboable_core::catalog::{ColumnInfo, ColumnMeta, DatabaseInfo, ObjectInfo, ObjectKind, SchemaInfo, TableRef};
use paboable_core::config::{ConnectionConfig, Target};
use paboable_core::connection::Connection;
use paboable_core::engine::EngineInfo;
use paboable_core::sink::{ExecOutcome, PUSH_BATCH_ROWS, ResultSink};
use paboable_core::value::Value;
use paboable_core::{CallOptions, DriverError, ErrorCode, Result};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::task::JoinHandle;
use tokio_postgres::types::{ToSql, Type};
use tokio_postgres::{AsyncMessage, CancelToken, Client, Config, NoTls, Row, RowStream};

use crate::ddl;
use crate::decode::{self, Raw};
use crate::errors::{self, Phase};
use crate::tls::{self, Tls};

const DEFAULT_PORT: u16 = 5432;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const KEEPALIVE_IDLE: Duration = Duration::from_secs(30);
/// How long a stopped statement may take to end after the cancel request
/// before the call gives up on it.
const CANCEL_GRACE: Duration = Duration::from_secs(5);

/// How the editor's statements are sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatementMode {
    /// Parse to a named statement, then execute it: a result with no rows
    /// still has its columns.
    Prepared,
    /// One unnamed statement per call, which a pooler in transaction mode
    /// (Supabase's on port 6543) accepts. A result with no rows shows no columns.
    Unnamed,
}

impl StatementMode {
    /// `statement_mode=prepared|unnamed` in the options wins; otherwise
    /// Supabase's transaction pooler gets `Unnamed`.
    pub fn for_target(host: &str, port: u16, options: &BTreeMap<String, String>) -> Self {
        match options.get("statement_mode").map(String::as_str) {
            Some("unnamed") => Self::Unnamed,
            Some("prepared") => Self::Prepared,
            _ if port == 6543 && host.to_ascii_lowercase().ends_with(".pooler.supabase.com") => Self::Unnamed,
            _ => Self::Prepared,
        }
    }
}

/// The sink of the statement running now, which the editor session's notices
/// reach.
type NoticeSlot = Arc<Mutex<Option<Arc<dyn ResultSink>>>>;

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

pub struct PostgresConnection {
    editor: Client,
    catalog: Client,
    cancel: CancelToken,
    tls: Tls,
    mode: StatementMode,
    version: String,
    database: String,
    notices: NoticeSlot,
    tasks: Vec<JoinHandle<()>>,
}

impl Drop for PostgresConnection {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}

impl PostgresConnection {
    pub async fn connect(config: &ConnectionConfig, options: CallOptions) -> Result<Self> {
        let Target::Network {
            host,
            port,
            user,
            password,
            database,
            tls,
        } = &config.target
        else {
            return Err(DriverError::config("PostgreSQL connects to a host"));
        };
        let host = host.trim();
        if host.is_empty() {
            return Err(DriverError::config("the host is empty"));
        }
        let port = port.unwrap_or(DEFAULT_PORT);
        let user = user.as_deref().filter(|user| !user.is_empty()).unwrap_or("postgres");
        let database = database
            .as_deref()
            .filter(|database| !database.is_empty())
            .unwrap_or(user);
        let (tls, ssl_mode) = tls::build(*tls, &config.options)?;

        let mut pg = Config::new();
        pg.host(host)
            .port(port)
            .user(user)
            .dbname(database)
            .application_name(
                config
                    .options
                    .get("application_name")
                    .map_or("Paboable", String::as_str),
            )
            .ssl_mode(ssl_mode)
            .connect_timeout(
                options
                    .remaining()
                    .map_or(CONNECT_TIMEOUT, |left| left.min(CONNECT_TIMEOUT)),
            )
            .keepalives(true)
            .keepalives_idle(KEEPALIVE_IDLE);
        if !password.is_empty() {
            pg.password(password.expose());
        }
        if let Some(startup) = config.options.get("options") {
            // Neon reads `endpoint=<id>` here when a client cannot send SNI.
            pg.options(startup);
        }

        options.check()?;
        let notices = NoticeSlot::default();
        let ((editor, editor_task), (catalog, catalog_task)) = options
            .run(async { tokio::try_join!(open(&pg, &tls, Some(notices.clone())), open(&pg, &tls, None)) })
            .await?;
        let mut connection = Self {
            cancel: editor.cancel_token(),
            editor,
            catalog,
            tls,
            mode: StatementMode::for_target(host, port, &config.options),
            version: String::new(),
            database: String::new(),
            notices,
            tasks: vec![editor_task, catalog_task],
        };
        let rows = connection
            .catalog_rows(
                "select current_setting('server_version'), current_database()::text",
                &[],
                &options,
            )
            .await?;
        if let Some(row) = rows.first() {
            connection.version = row.try_get(0).unwrap_or_default();
            connection.database = row.try_get(1).unwrap_or_default();
        }
        Ok(connection)
    }

    pub fn statement_mode(&self) -> StatementMode {
        self.mode
    }

    /// Asks the server to stop the editor session's statement. A failure is
    /// left alone: the statement then ends on its own or at the grace period.
    async fn send_cancel(&self) {
        let _ = match &self.tls {
            Tls::Plain => self.cancel.cancel_query(NoTls).await,
            Tls::Rustls(maker) => self.cancel.cancel_query(maker.clone()).await,
        };
    }

    async fn catalog_rows(&self, sql: &str, params: &[&str], options: &CallOptions) -> Result<Vec<Row>> {
        let typed: Vec<(&(dyn ToSql + Sync), Type)> = params
            .iter()
            .map(|param| (param as &(dyn ToSql + Sync), Type::TEXT))
            .collect();
        options
            .run(async {
                self.catalog
                    .query_typed(sql, &typed)
                    .await
                    .map_err(|error| errors::map(&error, Some(sql), Phase::Run))
            })
            .await
    }

    async fn run(&self, statement: &str, sink: &dyn ResultSink) -> Result<ExecOutcome> {
        let map = |error: tokio_postgres::Error| errors::map(&error, Some(statement), Phase::Run);
        match self.mode {
            StatementMode::Prepared => {
                let prepared = self.editor.prepare(statement).await.map_err(map)?;
                let columns: Vec<ColumnMeta> = prepared
                    .columns()
                    .iter()
                    .map(|column| decode::column_meta(column.name(), column.type_()))
                    .collect();
                let rows = self
                    .editor
                    .query_raw(&prepared, std::iter::empty::<&(dyn ToSql + Sync)>())
                    .await
                    .map_err(map)?;
                self.stream(rows, Some(columns), statement, sink).await
            }
            StatementMode::Unnamed => {
                let rows = self
                    .editor
                    .query_typed_raw(statement, std::iter::empty::<(&(dyn ToSql + Sync), Type)>())
                    .await
                    .map_err(map)?;
                self.stream(rows, None, statement, sink).await
            }
        }
    }

    /// Streams rows into the sink in batches. `columns` is known up front for
    /// a prepared statement; an unnamed one learns them from its first row.
    async fn stream(
        &self,
        rows: RowStream,
        columns: Option<Vec<ColumnMeta>>,
        statement: &str,
        sink: &dyn ResultSink,
    ) -> Result<ExecOutcome> {
        let map = |error: tokio_postgres::Error| errors::map(&error, Some(statement), Phase::Run);
        let mut rows = pin!(rows);
        let mut width = None;
        if let Some(columns) = columns.filter(|columns| !columns.is_empty()) {
            width = Some(columns.len());
            sink.begin_set(columns);
        }

        let mut batch = Vec::new();
        let mut outcome = ExecOutcome::default();
        while let Some(row) = rows.next().await {
            let row = row.map_err(map)?;
            let columns = *width.get_or_insert_with(|| {
                let columns: Vec<ColumnMeta> = row
                    .columns()
                    .iter()
                    .map(|column| decode::column_meta(column.name(), column.type_()))
                    .collect();
                let count = columns.len();
                sink.begin_set(columns);
                count
            });
            for (index, column) in row.columns().iter().enumerate() {
                let raw: Option<Raw<'_>> = row.try_get(index).map_err(map)?;
                batch.push(raw.map_or(Value::Null, |Raw(bytes)| decode::decode(column.type_(), bytes)));
            }
            if batch.len() >= PUSH_BATCH_ROWS * columns.max(1) {
                if !sink.push_rows(&batch) {
                    outcome.limited = true;
                    break;
                }
                batch.clear();
            }
        }

        if outcome.limited {
            // Stop the server sending rows nobody keeps, and read to the end of
            // this statement before the next one starts, so the cancel cannot
            // reach it.
            self.send_cancel().await;
            while let Some(item) = rows.next().await {
                if item.is_err() {
                    break;
                }
            }
        } else if !batch.is_empty() && !sink.push_rows(&batch) {
            outcome.limited = true;
        }

        match width {
            Some(_) => sink.end_set(),
            None => sink.command_done(rows.rows_affected()),
        }
        Ok(outcome)
    }
}

async fn open(pg: &Config, tls: &Tls, notices: Option<NoticeSlot>) -> Result<(Client, JoinHandle<()>)> {
    let map = |error: tokio_postgres::Error| errors::map(&error, None, Phase::Connect);
    Ok(match tls {
        Tls::Plain => {
            let (client, connection) = pg.connect(NoTls).await.map_err(map)?;
            (client, tokio::spawn(drive(connection, notices)))
        }
        Tls::Rustls(maker) => {
            let (client, connection) = pg.connect(maker.clone()).await.map_err(map)?;
            (client, tokio::spawn(drive(connection, notices)))
        }
    })
}

/// Runs a session's socket, and hands the notices the server sends to the
/// sink of the statement running now.
async fn drive<S, T>(mut connection: tokio_postgres::Connection<S, T>, notices: Option<NoticeSlot>)
where
    S: AsyncRead + AsyncWrite + Unpin,
    T: AsyncRead + AsyncWrite + Unpin,
{
    while let Some(message) = poll_fn(|cx| connection.poll_message(cx)).await {
        match message {
            Ok(AsyncMessage::Notice(notice)) => {
                let sink = notices.as_ref().and_then(|slot| lock(slot).clone());
                if let Some(sink) = sink {
                    sink.notice(notice.message());
                }
            }
            Ok(_) => {}
            Err(_) => break,
        }
    }
}

/// What this login may open: not templates, accepting connections, and
/// granted CONNECT (directly, through a role, or through PUBLIC).
const DATABASES: &str = "\
select d.datname::text, d.datname = current_database()
from pg_database d
where not d.datistemplate and d.datallowconn and has_database_privilege(d.datname, 'CONNECT')
order by d.datname";

const SCHEMAS: &str = "\
select n.nspname::text, n.nspname = current_schema()
from pg_namespace n
where n.nspname !~ '^pg_' and n.nspname <> 'information_schema'
order by n.nspname";

const OBJECTS: &str = "\
select name, kind from (
    select c.relname::text as name, c.relkind::text as kind,
           case c.relkind when 'r' then 0 when 'p' then 0 when 'v' then 1 when 'm' then 2 when 'f' then 3 else 4 end as rank
    from pg_class c join pg_namespace n on n.oid = c.relnamespace
    where n.nspname = $1 and c.relkind in ('r', 'p', 'v', 'm', 'f', 'S') and not c.relispartition
    union all
    select p.proname || '(' || pg_get_function_identity_arguments(p.oid) || ')',
           case p.prokind when 'p' then 'P' else 'F' end, 5
    from pg_proc p join pg_namespace n on n.oid = p.pronamespace
    where n.nspname = $1 and p.prokind in ('f', 'p')
) objects
order by rank, name";

const COLUMNS: &str = "\
select a.attnum::int4, a.attname::text, format_type(a.atttypid, a.atttypmod), not a.attnotnull,
       pg_get_expr(d.adbin, d.adrelid),
       -- indkey is an int2vector, whose subscripts start at 0, so count with ordinality.
       (select k.position::int4
          from pg_index i, unnest(i.indkey::int2[]) with ordinality as k(attnum, position)
         where i.indrelid = c.oid and i.indisprimary and k.attnum = a.attnum)
from pg_attribute a
join pg_class c on c.oid = a.attrelid
join pg_namespace n on n.oid = c.relnamespace
left join pg_attrdef d on d.adrelid = a.attrelid and d.adnum = a.attnum
where n.nspname = coalesce(nullif($1, ''), current_schema()) and c.relname = $2
  and a.attnum > 0 and not a.attisdropped
order by a.attnum";

fn read<'a, T: tokio_postgres::types::FromSql<'a>>(row: &'a Row, index: usize) -> Result<T> {
    row.try_get(index)
        .map_err(|error| DriverError::new(ErrorCode::Internal, error.to_string()))
}

fn missing(object: &ObjectInfo) -> DriverError {
    match &object.schema {
        Some(schema) => DriverError::not_found(format!("{schema}.{}", object.name)),
        None => DriverError::not_found(object.name.clone()),
    }
}

fn unscripted(object: &ObjectInfo) -> DriverError {
    DriverError::unsupported(format!("{:?} objects have no DDL here", object.kind))
}

fn ddl_column(row: &Row) -> Result<ddl::Column> {
    let identity: String = read(row, 4)?;
    let generated: String = read(row, 5)?;
    Ok(ddl::Column {
        name: read(row, 0)?,
        type_name: read(row, 1)?,
        collation: read(row, 2)?,
        fill: ddl::Fill::from_catalog(&identity, &generated, read(row, 3)?),
        not_null: read(row, 6)?,
        comment: read(row, 7)?,
    })
}

fn texts(rows: &[Row]) -> Result<Vec<String>> {
    rows.iter().map(|row| read(row, 0)).collect()
}

impl PostgresConnection {
    async fn relation_ddl(&self, row: &Row, options: &CallOptions) -> Result<String> {
        let oid: String = read(row, 0)?;
        let kind: String = read(row, 1)?;
        let schema: String = read(row, 2)?;
        let name: String = read(row, 3)?;
        let unlogged = read::<String>(row, 4)? == "u";
        let reloptions: Vec<String> = read::<Option<Vec<String>>>(row, 5)?.unwrap_or_default();
        let comment: Option<String> = read(row, 13)?;
        let definition = || -> Result<String> { Ok(read::<Option<String>>(row, 14)?.unwrap_or_default()) };
        match kind.as_str() {
            "v" => Ok(ddl::view_ddl(&schema, &name, &reloptions, &definition()?)),
            "m" => {
                let indexes = texts(&self.catalog_rows(ddl::INDEXES, &[&oid], options).await?)?;
                Ok(ddl::materialized_view_ddl(
                    &schema,
                    &name,
                    &reloptions,
                    &definition()?,
                    read(row, 12)?,
                    &indexes,
                    comment.as_deref(),
                ))
            }
            "S" => {
                let rows = self.catalog_rows(ddl::SEQUENCE, &[&oid], options).await?;
                let sequence = rows
                    .first()
                    .ok_or_else(|| DriverError::not_found(format!("{schema}.{name}")))?;
                let sequence = ddl::Sequence {
                    type_name: read(sequence, 0)?,
                    increment: read(sequence, 1)?,
                    min: read(sequence, 2)?,
                    max: read(sequence, 3)?,
                    start: read(sequence, 4)?,
                    cache: read(sequence, 5)?,
                    cycle: read(sequence, 6)?,
                };
                Ok(ddl::sequence_ddl(&schema, &name, unlogged, &sequence))
            }
            _ => {
                let parent_schema: Option<String> = read(row, 7)?;
                let parent: Option<String> = read(row, 8)?;
                let bound: Option<String> = read(row, 9)?;
                let server: Option<String> = read(row, 10)?;
                let server_options: Option<Vec<String>> = read(row, 11)?;
                let table = ddl::Table {
                    unlogged,
                    options: reloptions,
                    partition_key: read(row, 6)?,
                    partition_of: match (parent_schema, parent, bound) {
                        (Some(parent_schema), Some(parent), Some(bound)) => Some((parent_schema, parent, bound)),
                        _ => None,
                    },
                    server: server.map(|server| (server, server_options.unwrap_or_default())),
                    comment,
                    schema,
                    name,
                };
                let oid = [oid.as_str()];
                // One catalog session pipelines the three reads.
                let (columns, constraints, indexes) = tokio::try_join!(
                    self.catalog_rows(ddl::COLUMNS, &oid, options),
                    self.catalog_rows(ddl::CONSTRAINTS, &oid, options),
                    self.catalog_rows(ddl::INDEXES, &oid, options),
                )?;
                let columns = columns.iter().map(ddl_column).collect::<Result<Vec<_>>>()?;
                let constraints = constraints
                    .iter()
                    .map(|row| Ok((read(row, 0)?, read(row, 1)?)))
                    .collect::<Result<Vec<_>>>()?;
                Ok(ddl::table_ddl(&table, &columns, &constraints, &texts(&indexes)?))
            }
        }
    }
}

#[async_trait]
impl Connection for PostgresConnection {
    fn engine(&self) -> &'static EngineInfo {
        &crate::INFO
    }

    fn server_version(&self) -> Option<String> {
        Some(self.version.clone())
    }

    fn current_database(&self) -> Option<String> {
        Some(self.database.clone())
    }

    async fn list_databases(&self, options: CallOptions) -> Result<Vec<DatabaseInfo>> {
        let rows = self.catalog_rows(DATABASES, &[], &options).await?;
        rows.iter()
            .map(|row| {
                Ok(DatabaseInfo {
                    name: read(row, 0)?,
                    is_current: read(row, 1)?,
                })
            })
            .collect()
    }

    async fn ping(&self, options: CallOptions) -> Result<()> {
        if self.editor.is_closed() {
            return Err(DriverError::new(ErrorCode::Lost, "the editor session is closed"));
        }
        self.catalog_rows("select 1", &[], &options).await.map(drop)
    }

    async fn list_schemas(&self, options: CallOptions) -> Result<Vec<SchemaInfo>> {
        let rows = self.catalog_rows(SCHEMAS, &[], &options).await?;
        rows.iter()
            .map(|row| {
                Ok(SchemaInfo {
                    name: read(row, 0)?,
                    is_default: read(row, 1)?,
                })
            })
            .collect()
    }

    async fn list_objects(&self, schema: Option<&str>, options: CallOptions) -> Result<Vec<ObjectInfo>> {
        let schema = schema.unwrap_or("public");
        let rows = self.catalog_rows(OBJECTS, &[schema], &options).await?;
        rows.iter()
            .map(|row| {
                let kind: String = read(row, 1)?;
                let kind = match kind.as_str() {
                    "r" | "p" | "f" => ObjectKind::Table,
                    "v" => ObjectKind::View,
                    "m" => ObjectKind::MaterializedView,
                    "S" => ObjectKind::Sequence,
                    "F" => ObjectKind::Function,
                    "P" => ObjectKind::Procedure,
                    _ => ObjectKind::Other,
                };
                Ok(ObjectInfo {
                    schema: Some(schema.to_owned()),
                    name: read(row, 0)?,
                    kind,
                })
            })
            .collect()
    }

    async fn fetch_columns(&self, table: &TableRef, options: CallOptions) -> Result<Vec<ColumnInfo>> {
        let schema = table.schema.as_deref().unwrap_or("");
        let rows = self.catalog_rows(COLUMNS, &[schema, &table.name], &options).await?;
        rows.iter()
            .map(|row| {
                let ordinal: i32 = read(row, 0)?;
                let primary_key: Option<i32> = read(row, 5)?;
                Ok(ColumnInfo {
                    ordinal: u32::try_from(ordinal).unwrap_or(0),
                    name: read(row, 1)?,
                    type_name: read(row, 2)?,
                    nullable: read(row, 3)?,
                    default: read(row, 4)?,
                    primary_key: primary_key.and_then(|position| u32::try_from(position).ok()),
                })
            })
            .collect()
    }

    async fn object_ddl(&self, object: &ObjectInfo, options: CallOptions) -> Result<String> {
        let schema = object.schema.as_deref().unwrap_or("");
        if let Some(kinds) = ddl::prokinds(object.kind) {
            let rows = self
                .catalog_rows(ddl::ROUTINES, &[schema, &object.name, kinds], &options)
                .await?;
            if rows.is_empty() {
                return Err(missing(object));
            }
            return Ok(ddl::routines_ddl(&texts(&rows)?));
        }
        let kinds = ddl::relkinds(object.kind).ok_or_else(|| unscripted(object))?;
        let rows = self
            .catalog_rows(ddl::RELATION, &[schema, &object.name, kinds], &options)
            .await?;
        let row = rows.first().ok_or_else(|| missing(object))?;
        self.relation_ddl(row, &options).await
    }

    async fn object_comment(&self, object: &ObjectInfo, options: CallOptions) -> Result<Option<String>> {
        let schema = object.schema.as_deref().unwrap_or("");
        let (sql, kinds) = match (ddl::relkinds(object.kind), ddl::prokinds(object.kind)) {
            (Some(kinds), _) => (ddl::RELATION_COMMENT, kinds),
            (None, Some(kinds)) => (ddl::ROUTINE_COMMENT, kinds),
            (None, None) => return Err(unscripted(object)),
        };
        let rows = self.catalog_rows(sql, &[schema, &object.name, kinds], &options).await?;
        if rows.is_empty() {
            return Err(missing(object));
        }
        // Overloads under one bare name: the first that has a comment.
        for row in &rows {
            if let Some(comment) = read::<Option<String>>(row, 0)? {
                return Ok(Some(comment));
            }
        }
        Ok(None)
    }

    async fn execute(&self, statement: &str, sink: Arc<dyn ResultSink>, options: CallOptions) -> Result<ExecOutcome> {
        options.check()?;
        *lock(&self.notices) = Some(Arc::clone(&sink));
        let stop = async {
            options.stopped().await;
            self.send_cancel().await;
            tokio::time::sleep(CANCEL_GRACE).await;
        };
        let result = tokio::select! {
            result = self.run(statement, sink.as_ref()) => result,
            () = stop => Err(options.stop_reason()),
        };
        *lock(&self.notices) = None;
        match result {
            // The server reports our own cancel, or the deadline's, as 57014.
            Err(error) if error.code == ErrorCode::Cancelled && (options.is_cancelled() || options.is_expired()) => {
                Err(options.stop_reason())
            }
            other => other,
        }
    }

    async fn apply_edits(&self, statements: &[String], options: CallOptions) -> Result<()> {
        options.check()?;
        // Simple queries: they report each command's count, and Supabase's pooler takes them.
        let work = async {
            self.editor
                .batch_execute("BEGIN")
                .await
                .map_err(|error| errors::map(&error, None, Phase::Run))?;
            for (index, statement) in statements.iter().enumerate() {
                let messages = self
                    .editor
                    .simple_query(statement)
                    .await
                    .map_err(|error| errors::map(&error, Some(statement), Phase::Run))?;
                let changed = messages.iter().find_map(|message| match message {
                    tokio_postgres::SimpleQueryMessage::CommandComplete(count) => Some(*count),
                    _ => None,
                });
                if changed != Some(1) {
                    return Err(DriverError::row_gone(index, changed));
                }
            }
            self.editor
                .batch_execute("COMMIT")
                .await
                .map_err(|error| errors::map(&error, None, Phase::Run))
        };
        let result = options.run(work).await;
        if result.is_err() {
            // After a failure, a cancel or a deadline the transaction may still
            // be open on the editor session; it must not outlive the edit.
            let _ = self.editor.batch_execute("ROLLBACK").await;
        }
        result
    }

    async fn close(&self) -> Result<()> {
        for task in &self.tasks {
            task.abort();
        }
        Ok(())
    }
}
