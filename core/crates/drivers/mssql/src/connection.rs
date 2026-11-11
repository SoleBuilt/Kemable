use std::sync::{Arc, Once};
use std::time::Duration;

use async_trait::async_trait;
use futures_util::TryStreamExt;
use paboable_core::catalog::{
    ColumnInfo, ColumnMeta, DatabaseInfo, IndexInfo, ObjectInfo, ObjectKind, SchemaInfo, TableRef,
};
use paboable_core::config::{ConnectionConfig, Target, TlsMode};
use paboable_core::connection::Connection;
use paboable_core::engine::EngineInfo;
use paboable_core::sink::{ExecOutcome, PUSH_BATCH_ROWS, ResultSink};
use paboable_core::{CallOptions, DriverError, ErrorCode, Result};
use tiberius::{AuthMethod, Client, EncryptionLevel, QueryItem, Row, SqlBrowser, ToSql};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio_util::compat::{Compat, TokioAsyncWriteCompatExt};

use crate::ddl;
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
        let sets = self.catalog_sets(sql, params, options).await?;
        Ok(sets.into_iter().next().unwrap_or_default())
    }

    /// Every result set of one catalog batch, in order; a set with no rows is
    /// an empty `Vec`, so a set's place in the batch is its index.
    async fn catalog_sets(&self, sql: &str, params: &[&str], options: &CallOptions) -> Result<Vec<Vec<Row>>> {
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
                let result = async { client.query(sql, &params).await?.into_results().await }.await;
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

/// The last column says whether the server fills the column. A GENERATED
/// ALWAYS column is read through `COLUMNPROPERTY`, not
/// `sys.columns.generated_always_type`: servers before 2016 lack that column
/// and would refuse the whole query, where the property is only `NULL`.
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
       c.is_nullable, dc.definition, cast(ic.key_ordinal as int),
       cast(case when c.is_identity = 1 or c.is_computed = 1 or type_name(c.system_type_id) = 'timestamp'
                   or isnull(columnproperty(c.object_id, c.name, 'GeneratedAlwaysType'), 0) <> 0
            then 1 else 0 end as bit)
from sys.columns c
join sys.types t on t.user_type_id = c.user_type_id
left join sys.default_constraints dc on dc.object_id = c.default_object_id
left join sys.indexes i on i.object_id = c.object_id and i.is_primary_key = 1
left join sys.index_columns ic on ic.object_id = i.object_id and ic.index_id = i.index_id and ic.column_id = c.column_id
where c.object_id = object_id(quotename(@P1) + '.' + quotename(@P2))
order by c.column_id";

/// Whether the object exists, its text, and whether that text is encrypted.
/// `@P1` is `[schema].[name]`.
const DEFINITION: &str = "\
select cast(case when object_id(@P1) is null then 0 else 1 end as bit),
       object_definition(object_id(@P1)),
       cast(isnull(objectproperty(object_id(@P1), 'IsEncrypted'), 0) as bit)";

/// Everything a table's script needs, in one round trip. The result sets, in
/// order: the database's collation, columns, primary and unique keys, checks,
/// foreign keys, other rowstore indexes, descriptions, columnstore indexes,
/// their `ORDER` columns, XML indexes, spatial indexes. `@P1` is `[schema].[name]`.
///
/// `column_store_order_ordinal` exists from SQL Server 2022; a query naming it
/// fails to compile on an older server, so it runs as dynamic SQL only where
/// the column is there, and the set is empty elsewhere.
const TABLE: &str = "\
declare @id int = object_id(@P1);
select cast(databasepropertyex(db_name(), 'Collation') as nvarchar(128));
select c.name, t.name, case when t.is_user_defined = 1 then schema_name(t.schema_id) end,
       cast(c.max_length as int), cast(c.precision as int), cast(c.scale as int), c.is_nullable, c.collation_name,
       cast(ic.seed_value as nvarchar(64)), cast(ic.increment_value as nvarchar(64)),
       cc.definition, cc.is_persisted, dc.name, dc.definition
from sys.columns c
join sys.types t on t.user_type_id = c.user_type_id
left join sys.identity_columns ic on ic.object_id = c.object_id and ic.column_id = c.column_id
left join sys.computed_columns cc on cc.object_id = c.object_id and cc.column_id = c.column_id
left join sys.default_constraints dc on dc.object_id = c.default_object_id
where c.object_id = @id
order by c.column_id;
select kc.name, cast(case when kc.type = 'PK' then 1 else 0 end as bit),
       cast(case when i.type = 1 then 1 else 0 end as bit), c.name, ic.is_descending_key
from sys.key_constraints kc
join sys.indexes i on i.object_id = kc.parent_object_id and i.index_id = kc.unique_index_id
join sys.index_columns ic on ic.object_id = i.object_id and ic.index_id = i.index_id and ic.key_ordinal > 0
join sys.columns c on c.object_id = ic.object_id and c.column_id = ic.column_id
where kc.parent_object_id = @id
order by case when kc.type = 'PK' then 0 else 1 end, kc.name, ic.key_ordinal;
select name, definition
from sys.check_constraints
where parent_object_id = @id
order by name;
select fk.name, schema_name(rt.schema_id), rt.name, pc.name, rc.name,
       fk.delete_referential_action_desc, fk.update_referential_action_desc
from sys.foreign_keys fk
join sys.foreign_key_columns fkc on fkc.constraint_object_id = fk.object_id
join sys.columns pc on pc.object_id = fkc.parent_object_id and pc.column_id = fkc.parent_column_id
join sys.columns rc on rc.object_id = fkc.referenced_object_id and rc.column_id = fkc.referenced_column_id
join sys.objects rt on rt.object_id = fk.referenced_object_id
where fk.parent_object_id = @id
order by fk.name, fkc.constraint_column_id;
select i.name, i.is_unique, cast(case when i.type = 1 then 1 else 0 end as bit), i.filter_definition,
       c.name, ic.is_descending_key, ic.is_included_column
from sys.indexes i
join sys.index_columns ic on ic.object_id = i.object_id and ic.index_id = i.index_id
join sys.columns c on c.object_id = ic.object_id and c.column_id = ic.column_id
where i.object_id = @id and i.is_primary_key = 0 and i.is_unique_constraint = 0
  and i.type in (1, 2) and i.is_hypothetical = 0
  and (ic.key_ordinal > 0 or ic.is_included_column = 1)
order by i.name, ic.is_included_column, ic.key_ordinal, ic.index_column_id;
select c.name, cast(ep.value as nvarchar(max))
from sys.extended_properties ep
left join sys.columns c on c.object_id = ep.major_id and c.column_id = ep.minor_id
where ep.class = 1 and ep.major_id = @id and ep.name = N'MS_Description'
  and (ep.minor_id = 0 or c.column_id is not null)
order by ep.minor_id;
select i.name, cast(case when i.type = 5 then 1 else 0 end as bit), i.filter_definition, c.name
from sys.indexes i
left join sys.index_columns ic on ic.object_id = i.object_id and ic.index_id = i.index_id
     and i.type = 6 and ic.is_included_column = 1
left join sys.columns c on c.object_id = ic.object_id and c.column_id = ic.column_id
where i.object_id = @id and i.type in (5, 6) and i.is_hypothetical = 0
order by i.name, ic.index_column_id;
if exists (select 1 from sys.all_columns
           where object_id = object_id(N'sys.index_columns') and name = N'column_store_order_ordinal')
    exec sys.sp_executesql N'select i.name, c.name
from sys.indexes i
join sys.index_columns ic on ic.object_id = i.object_id and ic.index_id = i.index_id
join sys.columns c on c.object_id = ic.object_id and c.column_id = ic.column_id
where i.object_id = @id and i.type in (5, 6) and ic.column_store_order_ordinal > 0
order by i.name, ic.column_store_order_ordinal', N'@id int', @id;
else
    select cast(null as sysname), cast(null as sysname) where 1 = 0;
select xi.name, c.name, cast(xi.secondary_type as nvarchar(1)), p.name
from sys.xml_indexes xi
join sys.index_columns ic on ic.object_id = xi.object_id and ic.index_id = xi.index_id
join sys.columns c on c.object_id = ic.object_id and c.column_id = ic.column_id
left join sys.xml_indexes p on p.object_id = xi.object_id and p.index_id = xi.using_xml_index_id
where xi.object_id = @id and xi.xml_index_type in (0, 1) and xi.is_hypothetical = 0
order by case when xi.secondary_type is null then 0 else 1 end, xi.name;
select si.name, c.name, cast(st.tessellation_scheme as nvarchar(60)),
       st.bounding_box_xmin, st.bounding_box_ymin, st.bounding_box_xmax, st.bounding_box_ymax,
       cast(st.level_1_grid_desc as nvarchar(60)), cast(st.level_2_grid_desc as nvarchar(60)),
       cast(st.level_3_grid_desc as nvarchar(60)), cast(st.level_4_grid_desc as nvarchar(60)),
       cast(st.cells_per_object as int)
from sys.spatial_indexes si
join sys.spatial_index_tessellations st on st.object_id = si.object_id and st.index_id = si.index_id
join sys.index_columns ic on ic.object_id = si.object_id and ic.index_id = si.index_id
join sys.columns c on c.object_id = ic.object_id and c.column_id = ic.column_id
where si.object_id = @id and si.is_hypothetical = 0
order by si.name;";

/// A table's indexes, heaps and hypothetical ones left out, the primary key's
/// first. `@P1` is `[schema].[name]`.
const INDEXES: &str = "\
select i.name, i.is_unique, i.is_primary_key, i.type_desc, i.filter_definition, cast(i.type as int),
       c.name, ic.is_descending_key, ic.is_included_column, cast(ic.key_ordinal as int)
from sys.indexes i
left join sys.index_columns ic on ic.object_id = i.object_id and ic.index_id = i.index_id
left join sys.columns c on c.object_id = ic.object_id and c.column_id = ic.column_id
where i.object_id = object_id(@P1) and i.type <> 0 and i.is_hypothetical = 0
order by case when i.is_primary_key = 1 then 0 else 1 end, i.name,
         ic.is_included_column, ic.key_ordinal, ic.index_column_id";

/// Where a row of `sys.index_columns` shows in a table's structure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IndexColumnPlace {
    Key,
    Included,
}

/// A rowstore index lists its key and its INCLUDE columns; a columnstore
/// marks every column it keeps as included, and they are its columns; an XML
/// or a spatial index has one column, with no key ordinal. Anything else (a
/// partitioning column the index did not name) is left out.
pub(crate) fn index_column_place(index_type: i32, key_ordinal: i32, included: bool) -> Option<IndexColumnPlace> {
    match index_type {
        3 | 4 => Some(IndexColumnPlace::Key),
        5 | 6 => included.then_some(IndexColumnPlace::Key),
        _ if key_ordinal > 0 => Some(IndexColumnPlace::Key),
        _ if included => Some(IndexColumnPlace::Included),
        _ => None,
    }
}

/// `@P1` is `[schema].[name]`.
const SEQUENCE: &str = "\
select t.name, case when t.is_user_defined = 1 then schema_name(t.schema_id) end,
       cast(s.precision as int), cast(s.scale as int),
       cast(s.start_value as nvarchar(64)), cast(s.increment as nvarchar(64)),
       cast(s.minimum_value as nvarchar(64)), cast(s.maximum_value as nvarchar(64)),
       s.is_cycling, s.is_cached, s.cache_size
from sys.sequences s
join sys.types t on t.user_type_id = s.user_type_id
where s.object_id = object_id(@P1)";

/// `@P1` is `[schema].[name]`.
const COMMENT: &str = "\
select cast(value as nvarchar(max))
from sys.extended_properties
where class = 1 and major_id = object_id(@P1) and minor_id = 0 and name = N'MS_Description'";

fn flag(row: &Row, index: usize) -> Result<bool> {
    Ok(number::<bool>(row, index)?.unwrap_or(false))
}

fn int(row: &Row, index: usize) -> Result<i32> {
    Ok(number::<i32>(row, index)?.unwrap_or(0))
}

fn name(row: &Row, index: usize) -> Result<String> {
    Ok(text(row, index)?.unwrap_or_default())
}

/// The rows of one result set of a batch; a set the batch did not return reads as empty.
fn set(sets: &[Vec<Row>], index: usize) -> &[Row] {
    sets.get(index).map_or(&[], Vec::as_slice)
}

/// Rows grouped by their first column, which holds a constraint's or an
/// index's name; the queries sort by it, so a group's rows are together.
fn by_name(rows: &[Row]) -> Result<Vec<(String, Vec<&Row>)>> {
    let mut groups: Vec<(String, Vec<&Row>)> = Vec::new();
    for row in rows {
        let key = name(row, 0)?;
        match groups.last_mut() {
            Some((last, members)) if *last == key => members.push(row),
            _ => groups.push((key, vec![row])),
        }
    }
    Ok(groups)
}

impl MssqlConnection {
    /// A view's, a function's, a procedure's or a trigger's text, as it was written.
    async fn module_ddl(&self, qualified: &str, options: &CallOptions) -> Result<String> {
        let rows = self.catalog_rows(DEFINITION, &[qualified], options).await?;
        let Some(row) = rows.first() else {
            return Err(DriverError::not_found(format!("no object {qualified}")));
        };
        if !flag(row, 0)? {
            return Err(DriverError::not_found(format!("no object {qualified}")));
        }
        match text(row, 1)? {
            Some(definition) => Ok(definition),
            None if flag(row, 2)? => Err(DriverError::unsupported(format!("{qualified} is encrypted"))),
            // A CLR module, or one this login may not see the text of.
            None => Err(DriverError::unsupported(format!(
                "the server shows no text for {qualified}"
            ))),
        }
    }

    async fn table_ddl(&self, schema: &str, table: &str, options: &CallOptions) -> Result<String> {
        let qualified = ddl::qualified(schema, table);
        let sets = self.catalog_sets(TABLE, &[qualified.as_str()], options).await?;
        let columns = set(&sets, 1);
        if columns.is_empty() {
            return Err(DriverError::not_found(format!("no table {qualified}")));
        }
        let mut out = ddl::Table {
            schema: schema.to_owned(),
            name: table.to_owned(),
            database_collation: set(&sets, 0)
                .first()
                .map(|row| name(row, 0))
                .transpose()?
                .unwrap_or_default(),
            ..ddl::Table::default()
        };
        for row in columns {
            let identity = match (text(row, 8)?, text(row, 9)?) {
                (Some(seed), Some(increment)) => Some(ddl::Identity { seed, increment }),
                _ => None,
            };
            let computed = match text(row, 10)? {
                Some(definition) => Some(ddl::Computed {
                    definition,
                    persisted: flag(row, 11)?,
                }),
                None => None,
            };
            let default = match (text(row, 12)?, text(row, 13)?) {
                (Some(name), Some(definition)) => Some(ddl::DefaultConstraint { name, definition }),
                _ => None,
            };
            out.columns.push(ddl::TableColumn {
                name: name(row, 0)?,
                ty: ddl::SqlType {
                    name: name(row, 1)?,
                    user_schema: text(row, 2)?,
                    max_length: int(row, 3)?,
                    precision: int(row, 4)?,
                    scale: int(row, 5)?,
                },
                nullable: number::<bool>(row, 6)?.unwrap_or(true),
                collation: text(row, 7)?,
                identity,
                computed,
                default,
            });
        }
        for (key_name, rows) in by_name(set(&sets, 2))? {
            let Some(first) = rows.first() else { continue };
            out.keys.push(ddl::KeyConstraint {
                name: key_name,
                primary: flag(first, 1)?,
                clustered: flag(first, 2)?,
                columns: rows
                    .iter()
                    .map(|row| {
                        Ok(ddl::IndexColumn {
                            name: name(row, 3)?,
                            descending: flag(row, 4)?,
                        })
                    })
                    .collect::<Result<_>>()?,
            });
        }
        for row in set(&sets, 3) {
            out.checks.push(ddl::CheckConstraint {
                name: name(row, 0)?,
                definition: name(row, 1)?,
            });
        }
        for (key_name, rows) in by_name(set(&sets, 4))? {
            let Some(first) = rows.first() else { continue };
            let mut key = ddl::ForeignKey {
                name: key_name,
                referenced_schema: name(first, 1)?,
                referenced_table: name(first, 2)?,
                on_delete: name(first, 5)?,
                on_update: name(first, 6)?,
                ..ddl::ForeignKey::default()
            };
            for row in rows {
                key.columns.push(name(row, 3)?);
                key.referenced_columns.push(name(row, 4)?);
            }
            out.foreign_keys.push(key);
        }
        for (index_name, rows) in by_name(set(&sets, 5))? {
            let Some(first) = rows.first() else { continue };
            let mut index = ddl::Index {
                name: index_name,
                unique: flag(first, 1)?,
                clustered: flag(first, 2)?,
                filter: text(first, 3)?,
                ..ddl::Index::default()
            };
            for row in rows {
                if flag(row, 6)? {
                    index.included.push(name(row, 4)?);
                } else {
                    index.columns.push(ddl::IndexColumn {
                        name: name(row, 4)?,
                        descending: flag(row, 5)?,
                    });
                }
            }
            out.indexes.push(index);
        }
        for row in set(&sets, 6) {
            out.descriptions.push(ddl::Description {
                column: text(row, 0)?,
                text: name(row, 1)?,
            });
        }
        let orders = by_name(set(&sets, 8))?;
        for (index_name, rows) in by_name(set(&sets, 7))? {
            let Some(first) = rows.first() else { continue };
            let order = orders
                .iter()
                .find(|(ordered, _)| *ordered == index_name)
                .map(|(_, rows)| rows.iter().map(|row| name(row, 1)).collect::<Result<Vec<_>>>())
                .transpose()?
                .unwrap_or_default();
            out.columnstores.push(ddl::ColumnstoreIndex {
                clustered: flag(first, 1)?,
                filter: text(first, 2)?,
                columns: rows
                    .iter()
                    .map(|row| text(row, 3))
                    .filter_map(Result::transpose)
                    .collect::<Result<_>>()?,
                order,
                name: index_name,
            });
        }
        for row in set(&sets, 9) {
            let secondary = match (text(row, 2)?, text(row, 3)?) {
                (None, _) => None,
                (Some(code), Some(primary)) => match ddl::XmlPurpose::from_code(&code) {
                    Some(purpose) => Some(ddl::SecondaryXml { primary, purpose }),
                    // A kind this driver cannot write; leaving it out keeps the script runnable.
                    None => continue,
                },
                (Some(_), None) => continue,
            };
            out.xml_indexes.push(ddl::XmlIndex {
                name: name(row, 0)?,
                column: name(row, 1)?,
                secondary,
            });
        }
        for row in set(&sets, 10) {
            let bounding_box = match (
                number::<f64>(row, 3)?,
                number::<f64>(row, 4)?,
                number::<f64>(row, 5)?,
                number::<f64>(row, 6)?,
            ) {
                (Some(xmin), Some(ymin), Some(xmax), Some(ymax)) => Some([xmin, ymin, xmax, ymax]),
                _ => None,
            };
            out.spatial_indexes.push(ddl::SpatialIndex {
                name: name(row, 0)?,
                column: name(row, 1)?,
                scheme: name(row, 2)?,
                bounding_box,
                grids: (7..=10)
                    .map(|index| text(row, index))
                    .filter_map(Result::transpose)
                    .collect::<Result<_>>()?,
                cells_per_object: number::<i32>(row, 11)?,
            });
        }
        Ok(ddl::table_ddl(&out))
    }

    async fn sequence_ddl(&self, schema: &str, sequence: &str, options: &CallOptions) -> Result<String> {
        let qualified = ddl::qualified(schema, sequence);
        let rows = self.catalog_rows(SEQUENCE, &[qualified.as_str()], options).await?;
        let Some(row) = rows.first() else {
            return Err(DriverError::not_found(format!("no sequence {qualified}")));
        };
        Ok(ddl::sequence_ddl(&ddl::Sequence {
            schema: schema.to_owned(),
            name: sequence.to_owned(),
            ty: ddl::SqlType {
                name: name(row, 0)?,
                user_schema: text(row, 1)?,
                max_length: 0,
                precision: int(row, 2)?,
                scale: int(row, 3)?,
            },
            start: name(row, 4)?,
            increment: name(row, 5)?,
            minimum: name(row, 6)?,
            maximum: name(row, 7)?,
            cycling: flag(row, 8)?,
            cached: flag(row, 9)?,
            cache_size: number::<i32>(row, 10)?,
        }))
    }
}

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
                    generated: flag(row, 6)?,
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

    async fn list_indexes(&self, table: &TableRef, options: CallOptions) -> Result<Vec<IndexInfo>> {
        let qualified = ddl::qualified(table.schema.as_deref().unwrap_or("dbo"), &table.name);
        let rows = self.catalog_rows(INDEXES, &[qualified.as_str()], &options).await?;
        let mut indexes = Vec::new();
        for (index_name, rows) in by_name(&rows)? {
            let Some(first) = rows.first() else { continue };
            let index_type = int(first, 5)?;
            let mut index = IndexInfo {
                name: index_name,
                columns: Vec::new(),
                included: Vec::new(),
                unique: flag(first, 1)?,
                primary: flag(first, 2)?,
                method: name(first, 3)?,
                filter: text(first, 4)?,
            };
            for row in rows {
                let Some(column) = text(row, 6)? else { continue };
                match index_column_place(index_type, int(row, 9)?, flag(row, 8)?) {
                    Some(IndexColumnPlace::Key) if flag(row, 7)? => index.columns.push(format!("{column} DESC")),
                    Some(IndexColumnPlace::Key) => index.columns.push(column),
                    Some(IndexColumnPlace::Included) => index.included.push(column),
                    None => {}
                }
            }
            indexes.push(index);
        }
        Ok(indexes)
    }

    async fn object_ddl(&self, object: &ObjectInfo, options: CallOptions) -> Result<String> {
        let schema = object.schema.as_deref().unwrap_or("dbo");
        match object.kind {
            ObjectKind::Table => self.table_ddl(schema, &object.name, &options).await,
            ObjectKind::Sequence => self.sequence_ddl(schema, &object.name, &options).await,
            ObjectKind::View
            | ObjectKind::Function
            | ObjectKind::TableFunction
            | ObjectKind::Procedure
            | ObjectKind::Trigger
            | ObjectKind::Other => self.module_ddl(&ddl::qualified(schema, &object.name), &options).await,
            _ => Err(DriverError::unsupported("SQL Server scripts no object of this kind")),
        }
    }

    async fn object_comment(&self, object: &ObjectInfo, options: CallOptions) -> Result<Option<String>> {
        let qualified = ddl::qualified(object.schema.as_deref().unwrap_or("dbo"), &object.name);
        let rows = self.catalog_rows(COMMENT, &[qualified.as_str()], &options).await?;
        rows.first().map_or(Ok(None), |row| text(row, 0))
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
