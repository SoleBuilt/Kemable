use std::sync::{Arc, Once};
use std::time::Duration;

use async_trait::async_trait;
use futures_util::TryStreamExt;
use paboable_core::catalog::{ColumnInfo, ColumnMeta, DatabaseInfo, ObjectInfo, ObjectKind, SchemaInfo, TableRef};
use paboable_core::config::{ConnectionConfig, Target, TlsMode};
use paboable_core::connection::Connection;
use paboable_core::engine::EngineInfo;
use paboable_core::sink::{ExecOutcome, PUSH_BATCH_ROWS, ResultSink};
use paboable_core::{CallOptions, DriverError, ErrorCode, Result};
use tiberius::{AuthMethod, Client, EncryptionLevel, QueryItem, Row, SqlBrowser, ToSql};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio_util::compat::{Compat, TokioAsyncWriteCompatExt};

use crate::decode;
use crate::errors::{self, Phase};

type MssqlClient = Client<Compat<TcpStream>>;

const DEFAULT_PORT: u16 = 1433;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// How long the server has to acknowledge an attention before the session
/// is given up and opened again.
const CANCEL_GRACE: Duration = Duration::from_secs(5);

/// tiberius builds its TLS config with rustls' process-wide provider. Two
/// providers are linked into the app (ring, and aws-lc-rs through
/// tokio-rustls' defaults), so rustls cannot pick one by itself: name ring.
fn install_crypto_provider() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

pub struct MssqlConnection {
    config: tiberius::Config,
    named_instance: bool,
    /// `None` once a stop could not bring the session back; the next call opens a new one.
    editor: Mutex<Option<MssqlClient>>,
    catalog: Mutex<Option<MssqlClient>>,
    version: String,
    database: String,
}

impl MssqlConnection {
    pub async fn connect(config: &ConnectionConfig, options: CallOptions) -> Result<Self> {
        install_crypto_provider();
        let Target::Network {
            host,
            port,
            user,
            password,
            database,
            tls,
        } = &config.target
        else {
            return Err(DriverError::config("SQL Server connects to a host"));
        };
        let host = host.trim();
        if host.is_empty() {
            return Err(DriverError::config("the host is empty"));
        }

        let mut tds = tiberius::Config::new();
        let (server, instance) = match host.split_once('\\') {
            Some((server, instance)) => (server, Some(instance)),
            None => (host, None),
        };
        tds.host(server);
        if let Some(instance) = instance {
            tds.instance_name(instance);
        }
        if let Some(port) = port {
            tds.port(*port);
        } else if instance.is_none() {
            tds.port(DEFAULT_PORT);
        }
        if let Some(database) = database.as_deref().filter(|database| !database.is_empty()) {
            tds.database(database);
        }
        tds.application_name(
            config
                .options
                .get("application_name")
                .map_or("Paboable", String::as_str),
        );
        tds.authentication(match user.as_deref().filter(|user| !user.is_empty()) {
            Some(user) => AuthMethod::sql_server(user, password.expose()),
            None => windows_login()?,
        });
        match tls {
            TlsMode::Disable => tds.encryption(EncryptionLevel::NotSupported),
            // SQL Server always offers TLS, usually with its own self-signed
            // certificate: encrypt, and check nothing, as libpq's prefer and
            // require do.
            TlsMode::Prefer => {
                tds.encryption(EncryptionLevel::On);
                tds.trust_cert();
            }
            TlsMode::Require => {
                tds.encryption(EncryptionLevel::Required);
                tds.trust_cert();
            }
            TlsMode::VerifyFull => {
                tds.encryption(EncryptionLevel::Required);
                if let Some(ca) = config.options.get("sslrootcert").filter(|path| !path.is_empty()) {
                    tds.trust_cert_ca(ca);
                }
            }
        }

        options.check()?;
        let named_instance = instance.is_some() && port.is_none();
        let (editor, catalog) = options
            .run(async { tokio::try_join!(open(&tds, named_instance), open(&tds, named_instance)) })
            .await?;
        let mut connection = Self {
            config: tds,
            named_instance,
            editor: Mutex::new(Some(editor)),
            catalog: Mutex::new(Some(catalog)),
            version: String::new(),
            database: String::new(),
        };
        let rows = connection
            .catalog_rows(
                "select cast(serverproperty('ProductVersion') as nvarchar(128)), db_name()",
                &[],
                &options,
            )
            .await?;
        if let Some(row) = rows.first() {
            connection.version = text(row, 0)?.unwrap_or_default();
            connection.database = text(row, 1)?.unwrap_or_default();
        }
        Ok(connection)
    }

    async fn catalog_rows(&self, sql: &str, params: &[&str], options: &CallOptions) -> Result<Vec<Row>> {
        options
            .run(async {
                let mut guard = self.catalog.lock().await;
                if guard.is_none() {
                    *guard = Some(open(&self.config, self.named_instance).await?);
                }
                let Some(client) = guard.as_mut() else {
                    return Err(DriverError::new(ErrorCode::Lost, "the catalog session is closed"));
                };
                let params: Vec<&dyn ToSql> = params.iter().map(|param| param as &dyn ToSql).collect();
                let result = async { client.query(sql, &params).await?.into_first_result().await }.await;
                result.map_err(|error| {
                    let error = errors::map(error, Some(sql), Phase::Run);
                    if error.code == ErrorCode::Lost {
                        *guard = None;
                    }
                    error
                })
            })
            .await
    }
}

#[cfg(windows)]
fn windows_login() -> Result<AuthMethod> {
    Ok(AuthMethod::Integrated)
}

#[cfg(not(windows))]
fn windows_login() -> Result<AuthMethod> {
    Err(DriverError::config(
        "a SQL Server login needs a user and a password here",
    ))
}

/// Opens one session within `CONNECT_TIMEOUT`; tiberius has no timeout of its
/// own, and a reopen after a failed stop must not hang a call either.
async fn open(config: &tiberius::Config, named_instance: bool) -> Result<MssqlClient> {
    tokio::time::timeout(CONNECT_TIMEOUT, open_now(config, named_instance))
        .await
        .map_err(|_| DriverError::new(ErrorCode::Timeout, "the server did not finish the login in time"))?
}

/// Opens one session, following an Azure SQL redirect when the server sends one.
async fn open_now(config: &tiberius::Config, named_instance: bool) -> Result<MssqlClient> {
    let map = |error| errors::map(error, None, Phase::Connect);
    let tcp = if named_instance {
        TcpStream::connect_named(config).await.map_err(map)?
    } else {
        TcpStream::connect(config.get_addr())
            .await
            .map_err(|error| DriverError::new(ErrorCode::Connect, error.to_string()))?
    };
    let _ = tcp.set_nodelay(true);
    match Client::connect(config.clone(), tcp.compat_write()).await {
        Ok(client) => Ok(client),
        Err(tiberius::error::Error::Routing { host, port }) => {
            let mut routed = config.clone();
            routed.host(&host);
            routed.port(port);
            let tcp = TcpStream::connect(routed.get_addr())
                .await
                .map_err(|error| DriverError::new(ErrorCode::Connect, error.to_string()))?;
            Client::connect(routed, tcp.compat_write()).await.map_err(map)
        }
        Err(error) => Err(map(error)),
    }
}

/// How a batch's stream ended.
enum Ended {
    Done(Result<ExecOutcome>),
    /// The sink took no more rows; the rest of the response is unread.
    Limited,
}

/// Streams every result set a batch returns into the sink.
async fn stream_batch(client: &mut MssqlClient, batch: &str, sink: &dyn ResultSink) -> Ended {
    let map = |error| errors::map(error, Some(batch), Phase::Run);
    let mut stream = match client.simple_query(batch).await {
        Ok(stream) => stream,
        Err(error) => return Ended::Done(Err(map(error))),
    };
    let mut width: Option<usize> = None;
    let mut sets = 0usize;
    let mut values = Vec::new();
    loop {
        let item = match stream.try_next().await {
            Ok(Some(item)) => item,
            Ok(None) => break,
            Err(error) => {
                if width.is_some() {
                    flush(sink, &mut values);
                    sink.end_set();
                }
                return Ended::Done(Err(map(error)));
            }
        };
        match item {
            QueryItem::Metadata(meta) => {
                if width.is_some() {
                    flush(sink, &mut values);
                    sink.end_set();
                }
                let columns: Vec<ColumnMeta> = meta.columns().iter().map(decode::column_meta).collect();
                width = Some(columns.len());
                sets += 1;
                sink.begin_set(columns);
            }
            QueryItem::Row(row) => {
                for (column, data) in row.cells() {
                    values.push(decode::decode(column.column_type(), data));
                }
                if values.len() >= PUSH_BATCH_ROWS * width.unwrap_or(1).max(1) {
                    if !sink.push_rows(&values) {
                        values.clear();
                        sink.end_set();
                        return Ended::Limited;
                    }
                    values.clear();
                }
            }
        }
    }
    if width.is_some() {
        if !values.is_empty() && !sink.push_rows(&values) {
            sink.end_set();
            return Ended::Limited;
        }
        sink.end_set();
    }
    if sets == 0 {
        // tiberius does not hand out the DONE tokens' row counts.
        sink.command_done(None);
    }
    Ended::Done(Ok(ExecOutcome::default()))
}

fn flush(sink: &dyn ResultSink, values: &mut Vec<paboable_core::value::Value>) {
    if !values.is_empty() {
        sink.push_rows(values);
        values.clear();
    }
}

/// Sends an attention and reads to its acknowledgement. A session that does
/// not come back clean in time is dropped, and the next call opens another.
async fn attention(guard: &mut Option<MssqlClient>) {
    let Some(client) = guard.as_mut() else { return };
    let clean = matches!(
        tokio::time::timeout(CANCEL_GRACE, client.cancel_query()).await,
        Ok(Ok(()))
    );
    if !clean {
        *guard = None;
    }
}

fn text(row: &Row, index: usize) -> Result<Option<String>> {
    row.try_get::<&str, _>(index)
        .map(|value| value.map(str::to_owned))
        .map_err(|error| DriverError::internal(error.to_string()))
}

fn number<'a, T: tiberius::FromSql<'a>>(row: &'a Row, index: usize) -> Result<Option<T>> {
    row.try_get::<T, _>(index)
        .map_err(|error| DriverError::internal(error.to_string()))
}

/// What this login may open and is online, user databases first.
const DATABASES: &str = "\
select name, cast(case when name = db_name() then 1 else 0 end as bit)
from sys.databases
where state = 0 and has_dbaccess(name) = 1
order by case when database_id <= 4 then 1 else 0 end, name";

/// Schemas that hold user objects: not the system ones, not the fixed roles' own.
const SCHEMAS: &str = "\
select s.name, cast(case when s.name = schema_name() then 1 else 0 end as bit)
from sys.schemas s
where s.name not in ('sys', 'INFORMATION_SCHEMA', 'guest') and s.name not like 'db[_]%'
order by s.name";

/// The kind of a `sys.objects.type`, in SSMS's folders: stored procedures,
/// table-valued and scalar-valued functions, each T-SQL or CLR.
pub(crate) fn object_kind(code: &str) -> ObjectKind {
    match code {
        "U" => ObjectKind::Table,
        "V" => ObjectKind::View,
        "SO" => ObjectKind::Sequence,
        "P" | "PC" => ObjectKind::Procedure,
        "IF" | "TF" | "FT" => ObjectKind::TableFunction,
        "FN" | "FS" => ObjectKind::Function,
        _ => ObjectKind::Other,
    }
}

const OBJECTS: &str = "\
select o.name, rtrim(o.type)
from sys.objects o join sys.schemas s on s.schema_id = o.schema_id
where s.name = @P1 and o.is_ms_shipped = 0 and o.type in ('U', 'V', 'SO', 'P', 'PC', 'FN', 'FS', 'IF', 'TF', 'FT')
order by case rtrim(o.type) when 'U' then 0 when 'V' then 1 when 'SO' then 2 when 'P' then 3 else 4 end, o.name";

const COLUMNS: &str = "\
select c.column_id, c.name,
       t.name + case
           when t.name in ('varchar', 'char', 'varbinary', 'binary')
               then '(' + case when c.max_length = -1 then 'max' else cast(c.max_length as varchar(10)) end + ')'
           when t.name in ('nvarchar', 'nchar')
               then '(' + case when c.max_length = -1 then 'max' else cast(c.max_length / 2 as varchar(10)) end + ')'
           when t.name in ('decimal', 'numeric')
               then '(' + cast(c.precision as varchar(10)) + ',' + cast(c.scale as varchar(10)) + ')'
           when t.name in ('datetime2', 'time', 'datetimeoffset') then '(' + cast(c.scale as varchar(10)) + ')'
           else '' end,
       c.is_nullable, dc.definition, cast(ic.key_ordinal as int)
from sys.columns c
join sys.types t on t.user_type_id = c.user_type_id
left join sys.default_constraints dc on dc.object_id = c.default_object_id
left join sys.indexes i on i.object_id = c.object_id and i.is_primary_key = 1
left join sys.index_columns ic on ic.object_id = i.object_id and ic.index_id = i.index_id and ic.column_id = c.column_id
where c.object_id = object_id(quotename(@P1) + '.' + quotename(@P2))
order by c.column_id";

#[async_trait]
impl Connection for MssqlConnection {
    fn engine(&self) -> &'static EngineInfo {
        &crate::INFO
    }

    fn server_version(&self) -> Option<String> {
        Some(self.version.clone())
    }

    fn current_database(&self) -> Option<String> {
        Some(self.database.clone())
    }

    async fn ping(&self, options: CallOptions) -> Result<()> {
        self.catalog_rows("select 1", &[], &options).await.map(drop)
    }

    async fn list_databases(&self, options: CallOptions) -> Result<Vec<DatabaseInfo>> {
        let rows = self.catalog_rows(DATABASES, &[], &options).await?;
        rows.iter()
            .map(|row| {
                Ok(DatabaseInfo {
                    name: text(row, 0)?.unwrap_or_default(),
                    is_current: number::<bool>(row, 1)?.unwrap_or(false),
                })
            })
            .collect()
    }

    async fn list_schemas(&self, options: CallOptions) -> Result<Vec<SchemaInfo>> {
        let rows = self.catalog_rows(SCHEMAS, &[], &options).await?;
        rows.iter()
            .map(|row| {
                Ok(SchemaInfo {
                    name: text(row, 0)?.unwrap_or_default(),
                    is_default: number::<bool>(row, 1)?.unwrap_or(false),
                })
            })
            .collect()
    }

    async fn list_objects(&self, schema: Option<&str>, options: CallOptions) -> Result<Vec<ObjectInfo>> {
        let schema = schema.unwrap_or("dbo");
        let rows = self.catalog_rows(OBJECTS, &[schema], &options).await?;
        rows.iter()
            .map(|row| {
                let kind = object_kind(text(row, 1)?.as_deref().unwrap_or_default());
                Ok(ObjectInfo {
                    schema: Some(schema.to_owned()),
                    name: text(row, 0)?.unwrap_or_default(),
                    kind,
                })
            })
            .collect()
    }

    async fn fetch_columns(&self, table: &TableRef, options: CallOptions) -> Result<Vec<ColumnInfo>> {
        let schema = table.schema.as_deref().unwrap_or("dbo");
        let rows = self.catalog_rows(COLUMNS, &[schema, &table.name], &options).await?;
        rows.iter()
            .map(|row| {
                Ok(ColumnInfo {
                    ordinal: number::<i32>(row, 0)?.and_then(|n| u32::try_from(n).ok()).unwrap_or(0),
                    name: text(row, 1)?.unwrap_or_default(),
                    type_name: text(row, 2)?.unwrap_or_default(),
                    nullable: number::<bool>(row, 3)?.unwrap_or(true),
                    default: text(row, 4)?,
                    primary_key: number::<i32>(row, 5)?.and_then(|n| u32::try_from(n).ok()),
                })
            })
            .collect()
    }

    async fn execute(&self, batch: &str, sink: Arc<dyn ResultSink>, options: CallOptions) -> Result<ExecOutcome> {
        options.check()?;
        let mut guard = self.editor.lock().await;
        if guard.is_none() {
            *guard = Some(open(&self.config, self.named_instance).await?);
        }
        let ended = {
            let Some(client) = guard.as_mut() else {
                return Err(DriverError::new(ErrorCode::Lost, "the editor session is closed"));
            };
            tokio::select! {
                ended = stream_batch(client, batch, sink.as_ref()) => Some(ended),
                () = options.stopped() => None,
            }
        };
        match ended {
            Some(Ended::Done(Ok(outcome))) => Ok(outcome),
            Some(Ended::Done(Err(error))) => {
                if error.code == ErrorCode::Lost {
                    *guard = None;
                }
                Err(error)
            }
            Some(Ended::Limited) => {
                attention(&mut guard).await;
                Ok(ExecOutcome { limited: true })
            }
            None => {
                attention(&mut guard).await;
                Err(options.stop_reason())
            }
        }
    }

    /// One batch: tiberius does not hand out row counts, so the batch checks
    /// `@@ROWCOUNT` after each statement and throws, and `XACT_ABORT` with the
    /// CATCH rolls everything back, a cancel's attention included.
    async fn apply_edits(&self, statements: &[String], options: CallOptions) -> Result<()> {
        let batch = edit_batch(statements);
        match self.execute(&batch, Arc::new(Discard), options).await {
            Ok(_) => Ok(()),
            Err(error) if error.engine_code.as_deref() == Some(ROW_GONE_ERROR) => {
                let index = error.message.trim().parse::<usize>().unwrap_or(0);
                Err(DriverError::row_gone(index, None))
            }
            Err(error) => Err(error),
        }
    }

    async fn close(&self) -> Result<()> {
        for slot in [&self.editor, &self.catalog] {
            if let Some(client) = slot.lock().await.take() {
                let _ = client.close().await;
            }
        }
        Ok(())
    }
}

/// The error number an edit's batch throws when a statement changes another number of rows than one.
const ROW_GONE_ERROR: &str = "50001";

/// An edit's statements as one all-or-nothing batch; the error's message is the statement's index.
fn edit_batch(statements: &[String]) -> String {
    let mut batch = String::from("SET XACT_ABORT ON;\nBEGIN TRY\nBEGIN TRANSACTION;\n");
    for (index, statement) in statements.iter().enumerate() {
        batch.push_str(statement);
        batch.push_str(&format!(
            ";\nIF @@ROWCOUNT <> 1 THROW {ROW_GONE_ERROR}, N'{index}', 1;\n"
        ));
    }
    batch.push_str(
        "COMMIT TRANSACTION;\nEND TRY\nBEGIN CATCH\nIF @@TRANCOUNT > 0 ROLLBACK TRANSACTION;\nTHROW;\nEND CATCH",
    );
    batch
}

/// A sink for a batch whose rows nobody reads.
struct Discard;

impl ResultSink for Discard {
    fn begin_set(&self, _columns: Vec<ColumnMeta>) {}

    fn push_rows(&self, _values: &[paboable_core::value::Value]) -> bool {
        true
    }

    fn end_set(&self) {}

    fn command_done(&self, _rows_affected: Option<u64>) {}

    fn notice(&self, _text: &str) {}
}
