use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use paboable_core::catalog::{ColumnMeta, ObjectKind, TableRef};
use paboable_core::config::{ConnectionConfig, Secret, Target, TlsMode};
use paboable_core::connection::Connection;
use paboable_core::driver::Driver;
use paboable_core::sink::ResultSink;
use paboable_core::value::Value;
use paboable_core::{CallOptions, ErrorCode};
use paboable_store::{ResultStore, WindowReader, WindowRequest};
use tiberius::ColumnData;
use tiberius::time::{Date, DateTime2, Time};

use crate::MssqlDriver;
use crate::decode::{decode, numeric_text};
use crate::errors::line_offset;

// --- without a server ---------------------------------------------------------

#[test]
fn decimals_keep_every_digit_of_their_scale() {
    assert_eq!(numeric_text(12_345_678, 3), "12345.678");
    assert_eq!(numeric_text(-5, 2), "-0.05");
    assert_eq!(numeric_text(1250, 2), "12.50");
    assert_eq!(numeric_text(42, 0), "42");
}

#[test]
fn money_reads_with_four_places() {
    let value = decode(tiberius::ColumnType::Money, &ColumnData::F64(Some(12.5)));
    assert_eq!(value, Value::Decimal("12.5000".into()));
}

#[test]
fn a_seventh_fractional_digit_is_kept_in_full() {
    // 2023-11-14 is 738,837 days after 0001-01-01.
    let date = Date::new(738_837);
    let whole = DateTime2::new(date, Time::new(800_003_048_650, 7));
    assert_eq!(
        paboable_core::format::display(&decode(
            tiberius::ColumnType::Datetime2,
            &ColumnData::DateTime2(Some(whole))
        )),
        "2023-11-14 22:13:20.304865"
    );
    let finer = DateTime2::new(date, Time::new(800_003_048_651, 7));
    assert_eq!(
        decode(tiberius::ColumnType::Datetime2, &ColumnData::DateTime2(Some(finer))),
        Value::Other("2023-11-14 22:13:20.3048651".into())
    );
}

#[test]
fn error_lines_become_byte_offsets() {
    let batch = "select 1\nselect 'đ'\nselec 2";
    assert_eq!(line_offset(batch, 1), Some(0));
    assert_eq!(line_offset(batch, 3), Some(21));
    assert_eq!(line_offset(batch, 9), None);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_file_target_is_refused_before_connecting() {
    let config = ConnectionConfig {
        engine: "mssql".into(),
        target: Target::File {
            path: "x.db".into(),
            read_only: false,
        },
        options: BTreeMap::new(),
    };
    let error = MssqlDriver
        .connect(&config, CallOptions::default())
        .await
        .err()
        .expect("a refusal");
    assert_eq!(error.code, ErrorCode::ConfigInvalid);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_closed_port_fails_to_connect() {
    let config = ConnectionConfig {
        engine: "mssql".into(),
        target: Target::Network {
            host: "127.0.0.1".into(),
            port: Some(1),
            user: Some("sa".into()),
            password: Secret::new("x"),
            database: None,
            tls: TlsMode::Disable,
        },
        options: BTreeMap::new(),
    };
    let error = MssqlDriver
        .connect(&config, CallOptions::default())
        .await
        .err()
        .expect("a refusal");
    assert_eq!(error.code, ErrorCode::Connect);
}

// --- with a server: PABOABLE_MSSQL_URL=sqlserver://localhost (Windows login) ----

fn live_config() -> ConnectionConfig {
    let url = std::env::var("PABOABLE_MSSQL_URL").expect("PABOABLE_MSSQL_URL is set");
    paboable_core::connection_url::parse(&url).expect("a valid URL")
}

fn in_database(mut config: ConnectionConfig, name: &str) -> ConnectionConfig {
    if let Target::Network { database, .. } = &mut config.target {
        *database = Some(name.into());
    }
    config
}

async fn live_in(database: &str) -> Arc<dyn Connection> {
    MssqlDriver
        .connect(&in_database(live_config(), database), CallOptions::default())
        .await
        .expect("the server accepts the connection")
}

#[derive(Default)]
struct Collect {
    stores: Mutex<Vec<Arc<ResultStore>>>,
    commands: Mutex<usize>,
}

impl Collect {
    fn last(&self) -> Arc<ResultStore> {
        self.stores.lock().unwrap().last().cloned().expect("a result set")
    }
}

impl ResultSink for Collect {
    fn begin_set(&self, columns: Vec<ColumnMeta>) {
        self.stores
            .lock()
            .unwrap()
            .push(Arc::new(ResultStore::new(columns, Some(1000))));
    }

    fn push_rows(&self, values: &[Value]) -> bool {
        self.stores
            .lock()
            .unwrap()
            .last()
            .is_some_and(|store| store.push_rows(values))
    }

    fn end_set(&self) {
        if let Some(store) = self.stores.lock().unwrap().last() {
            store.finish();
        }
    }

    fn command_done(&self, _rows_affected: Option<u64>) {
        *self.commands.lock().unwrap() += 1;
    }

    fn notice(&self, _text: &str) {}
}

async fn run(conn: &Arc<dyn Connection>, sql: &str) -> Arc<Collect> {
    let sink = Arc::new(Collect::default());
    conn.execute(sql, sink.clone(), CallOptions::default())
        .await
        .unwrap_or_else(|error| panic!("{sql}: {error}"));
    sink
}

fn first_row(store: &ResultStore) -> Vec<String> {
    let width = store.columns().len();
    let bytes = store.fetch_window(WindowRequest {
        first_row: 0,
        row_count: 1,
        first_column: 0,
        column_count: u32::try_from(width).unwrap(),
        max_chars: 200,
    });
    let window = WindowReader::new(&bytes).unwrap();
    (0..width)
        .map(|column| {
            if window.is_null(0, column) {
                "NULL".into()
            } else {
                window.text(0, column).to_owned()
            }
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs SQL Server at $PABOABLE_MSSQL_URL"]
async fn every_mapped_type_reads_as_sql_server_shows_it() {
    let conn = live_in("tempdb").await;
    let sink = run(
        &conn,
        "select cast(1 as bit), cast(255 as tinyint), cast(-2 as smallint), 3, cast(4 as bigint), \
         cast(0.1 as real), cast(2.5 as float), cast(12345.678 as decimal(10,3)), cast(12.5 as money), \
         N'Việt Nam', cast(0x00ff as varbinary(2)), cast('a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11' as uniqueidentifier), \
         cast('2024-02-29' as date), cast('2023-11-14 22:13:20.123' as datetime), \
         cast('2023-11-14 22:13:20' as smalldatetime), cast('2023-11-14 22:13:20.3048651' as datetime2), \
         cast('2023-11-14 22:13:20.304865' as datetime2(6)), cast('04:05:06.5' as time), \
         cast('2023-11-14 22:13:20 +07:00' as datetimeoffset), cast(null as int), cast('<a>1</a>' as xml)",
    )
    .await;
    assert_eq!(
        first_row(&sink.last()),
        [
            "true",
            "255",
            "-2",
            "3",
            "4",
            "0.1",
            "2.5",
            "12345.678",
            "12.5000",
            "Việt Nam",
            "0x00ff",
            "a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11",
            "2024-02-29",
            "2023-11-14 22:13:20.123",
            "2023-11-14 22:13:00",
            "2023-11-14 22:13:20.3048651",
            "2023-11-14 22:13:20.304865",
            "04:05:06.5",
            "2023-11-14 22:13:20+07:00",
            "NULL",
            "<a>1</a>",
        ]
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs SQL Server at $PABOABLE_MSSQL_URL"]
async fn a_batch_returns_each_of_its_result_sets_and_commands_report_done() {
    let conn = live_in("tempdb").await;
    let sink = run(&conn, "select 1 as a; select 'two' as b, 2 as c").await;
    let stores = sink.stores.lock().unwrap().clone();
    assert_eq!(stores.len(), 2);
    assert_eq!(first_row(&stores[1]), ["two", "2"]);

    let sink = run(
        &conn,
        "create table #paboable (id int); insert into #paboable values (1), (2)",
    )
    .await;
    assert_eq!(*sink.commands.lock().unwrap(), 1);
    let sink = run(&conn, "select * from #paboable where 1 = 0").await;
    assert_eq!(sink.last().columns().len(), 1);
    assert_eq!(sink.last().state().total_rows, 0);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs SQL Server at $PABOABLE_MSSQL_URL"]
async fn the_row_limit_stops_the_server_and_the_session_stays_usable() {
    let conn = live_in("tempdb").await;
    let sink = Arc::new(Collect::default());
    let outcome = conn
        .execute(
            "select top 200000 a.object_id from sys.all_objects a cross join sys.all_objects b",
            sink.clone(),
            CallOptions::default(),
        )
        .await
        .unwrap();
    assert!(outcome.limited);
    assert_eq!(sink.last().state().total_rows, 1000);
    assert_eq!(first_row(&run(&conn, "select 42").await.last()), ["42"]);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs SQL Server at $PABOABLE_MSSQL_URL"]
async fn a_cancel_sends_an_attention_and_the_session_stays_usable() {
    let conn = live_in("tempdb").await;
    let options = CallOptions::default();
    let cancel = options.cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(300)).await;
        cancel.cancel();
    });
    let started = std::time::Instant::now();
    let error = conn
        .execute("waitfor delay '00:00:30'", Arc::new(Collect::default()), options)
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Cancelled);
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "the server stopped the wait"
    );
    assert_eq!(first_row(&run(&conn, "select 1").await.last()), ["1"]);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs SQL Server at $PABOABLE_MSSQL_URL"]
async fn a_deadline_reports_a_timeout() {
    let conn = live_in("tempdb").await;
    let options = CallOptions::default().with_timeout(Duration::from_millis(300));
    let error = conn
        .execute("waitfor delay '00:00:30'", Arc::new(Collect::default()), options)
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Timeout);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs SQL Server at $PABOABLE_MSSQL_URL"]
async fn an_error_carries_its_number_and_the_line_it_is_on() {
    let conn = live_in("tempdb").await;
    let batch = "select 1\nselect 'đ'\nselect * from paboable_missing_table";
    let error = conn
        .execute(batch, Arc::new(Collect::default()), CallOptions::default())
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Query);
    assert_eq!(error.engine_code.as_deref(), Some("208"));
    assert_eq!(error.position.map(|p| p.byte_offset), Some(21));
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs SQL Server at $PABOABLE_MSSQL_URL"]
async fn the_catalog_lists_databases_schemas_tables_and_columns() {
    let conn = live_in("tempdb").await;
    assert_eq!(conn.current_database().as_deref(), Some("tempdb"));
    let databases = conn.list_databases(CallOptions::default()).await.unwrap();
    assert!(databases.iter().any(|db| db.name == "master"));
    assert_eq!(
        databases
            .iter()
            .filter(|db| db.is_current)
            .map(|db| db.name.as_str())
            .collect::<Vec<_>>(),
        ["tempdb"]
    );

    let schema = format!("paboable_{}", std::process::id());
    run(&conn, &format!("create schema {schema}")).await;
    run(
        &conn,
        &format!("create table {schema}.items (id int primary key, name nvarchar(50) not null default N'x')"),
    )
    .await;
    run(&conn, &format!("create view {schema}.v as select 1 as one")).await;

    let schemas = conn.list_schemas(CallOptions::default()).await.unwrap();
    assert!(schemas.iter().any(|s| s.name == schema));
    let objects = conn.list_objects(Some(&schema), CallOptions::default()).await.unwrap();
    let names: Vec<_> = objects.iter().map(|o| (o.name.as_str(), o.kind)).collect();
    assert_eq!(names, [("items", ObjectKind::Table), ("v", ObjectKind::View)]);
    let columns = conn
        .fetch_columns(
            &TableRef {
                schema: Some(schema.clone()),
                name: "items".into(),
            },
            CallOptions::default(),
        )
        .await
        .unwrap();
    assert_eq!(columns.len(), 2);
    assert_eq!(columns[0].primary_key, Some(1));
    assert_eq!(columns[1].type_name, "nvarchar(50)");
    assert!(!columns[1].nullable);
    assert_eq!(columns[1].default.as_deref(), Some("(N'x')"));

    run(
        &conn,
        &format!("drop view {schema}.v; drop table {schema}.items; drop schema {schema}"),
    )
    .await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs SQL Server at $PABOABLE_MSSQL_URL"]
async fn a_wrong_password_is_an_auth_error() {
    let mut config = live_config();
    if let Target::Network { user, password, .. } = &mut config.target {
        *user = Some("paboable_no_such_login".into());
        *password = Secret::new("certainly-not-the-password");
    }
    let error = MssqlDriver
        .connect(&config, CallOptions::default())
        .await
        .err()
        .expect("a refusal");
    assert_eq!(error.code, ErrorCode::Auth);
}

#[test]
fn objects_fall_in_ssms_folders_procedures_table_valued_and_scalar_functions() {
    use crate::connection::object_kind;
    use paboable_core::catalog::ObjectKind;
    for (code, kind) in [
        ("U", ObjectKind::Table),
        ("V", ObjectKind::View),
        ("SO", ObjectKind::Sequence),
        ("P", ObjectKind::Procedure),
        ("PC", ObjectKind::Procedure),
        ("IF", ObjectKind::TableFunction),
        ("TF", ObjectKind::TableFunction),
        ("FT", ObjectKind::TableFunction),
        ("FN", ObjectKind::Function),
        ("FS", ObjectKind::Function),
        ("TR", ObjectKind::Other),
    ] {
        assert_eq!(object_kind(code), kind, "{code}");
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs SQL Server at $PABOABLE_MSSQL_URL"]
async fn object_ddl_scripts_what_the_server_holds_and_the_table_script_runs_again() {
    use paboable_core::catalog::ObjectInfo;
    use paboable_core::sql::{SqlGrammar, split};

    let conn = live_in("tempdb").await;
    let schema = format!("paboable_ddl_{}", std::process::id());
    let copy = format!("paboable_ddl_copy_{}", std::process::id());
    let object = |name: &str, kind| ObjectInfo {
        schema: Some(schema.clone()),
        name: name.into(),
        kind,
    };
    run(&conn, &format!("create schema {schema}")).await;
    run(&conn, &format!("create schema {copy}")).await;
    run(
        &conn,
        &format!(
            "create table {schema}.customers (id int not null constraint pk_customers primary key);
             create table {schema}.[Khách hàng] (
                 id int identity(10, 5) not null,
                 code varchar(20) collate Latin1_General_BIN not null,
                 qty int not null constraint df_qty default (1),
                 price decimal(10, 2) not null,
                 total as (qty * price) persisted,
                 note nvarchar(max) null,
                 customer_id int null,
                 constraint pk_kh primary key clustered (id),
                 constraint uq_kh_code unique nonclustered (code desc),
                 constraint ck_kh_qty check (qty > 0),
                 constraint fk_kh_customer foreign key (customer_id)
                     references {schema}.customers (id) on delete cascade
             );
             create nonclustered index ix_kh_customer on {schema}.[Khách hàng] (customer_id desc)
                 include (note) where customer_id is not null;
             exec sys.sp_addextendedproperty @name = N'MS_Description', @value = N'Đơn hàng',
                 @level0type = N'SCHEMA', @level0name = N'{schema}',
                 @level1type = N'TABLE', @level1name = N'Khách hàng';
             exec sys.sp_addextendedproperty @name = N'MS_Description', @value = N'Số lượng ''đặt''',
                 @level0type = N'SCHEMA', @level0name = N'{schema}',
                 @level1type = N'TABLE', @level1name = N'Khách hàng',
                 @level2type = N'COLUMN', @level2name = N'qty';"
        ),
    )
    .await;
    let view = format!("create view {schema}.v_kh as select id, qty from {schema}.[Khách hàng]");
    let scalar = format!("create function {schema}.f_double (@x int) returns int as begin return @x * 2 end");
    let rows = format!(
        "create function {schema}.f_rows (@min int) returns table as return select id from {schema}.[Khách hàng] where qty >= @min"
    );
    let procedure = format!("create procedure {schema}.p_count as select count(*) from {schema}.[Khách hàng]");
    for module in [&view, &scalar, &rows, &procedure] {
        run(&conn, module).await;
    }
    run(
        &conn,
        &format!(
            "create sequence {schema}.seq as bigint start with 100 increment by 10 minvalue 1 maxvalue 100000 cycle cache 20"
        ),
    )
    .await;

    let options = CallOptions::default;
    let table = conn
        .object_ddl(&object("Khách hàng", ObjectKind::Table), options())
        .await
        .unwrap();
    let name = format!("[{schema}].[Khách hàng]");
    for piece in [
        format!("CREATE TABLE {name} (\n"),
        "[id] int IDENTITY(10, 5) NOT NULL,".into(),
        "[code] varchar(20) COLLATE Latin1_General_BIN NOT NULL,".into(),
        "[qty] int NOT NULL CONSTRAINT [df_qty] DEFAULT ((1)),".into(),
        "[price] decimal(10,2) NOT NULL,".into(),
        "[total] AS ([qty]*[price]) PERSISTED".into(),
        "[note] nvarchar(max) NULL,".into(),
        "CONSTRAINT [pk_kh] PRIMARY KEY CLUSTERED ([id] ASC),".into(),
        "CONSTRAINT [uq_kh_code] UNIQUE NONCLUSTERED ([code] DESC),".into(),
        "CONSTRAINT [ck_kh_qty] CHECK ([qty]>(0))\n);".into(),
        format!(
            "ALTER TABLE {name} ADD CONSTRAINT [fk_kh_customer] FOREIGN KEY ([customer_id]) \
             REFERENCES [{schema}].[customers] ([id]) ON DELETE CASCADE;"
        ),
        format!(
            "CREATE NONCLUSTERED INDEX [ix_kh_customer] ON {name} ([customer_id] DESC) INCLUDE ([note]) \
             WHERE ([customer_id] IS NOT NULL);"
        ),
        "@value = N'Đơn hàng'".into(),
        "@value = N'Số lượng ''đặt''',".into(),
        "@level2type = N'COLUMN', @level2name = N'qty';".into(),
    ] {
        assert!(table.contains(&piece), "{piece}\nnot in\n{table}");
    }
    assert_eq!(split(&table, SqlGrammar::SqlServer).len(), 5);

    assert_eq!(
        conn.object_comment(&object("Khách hàng", ObjectKind::Table), options())
            .await
            .unwrap()
            .as_deref(),
        Some("Đơn hàng")
    );
    assert_eq!(
        conn.object_comment(&object("v_kh", ObjectKind::View), options())
            .await
            .unwrap(),
        None
    );

    for (name, kind, text) in [
        ("v_kh", ObjectKind::View, &view),
        ("f_double", ObjectKind::Function, &scalar),
        ("f_rows", ObjectKind::TableFunction, &rows),
        ("p_count", ObjectKind::Procedure, &procedure),
    ] {
        let ddl = conn.object_ddl(&object(name, kind), options()).await.unwrap();
        assert_eq!(ddl.trim(), text.trim(), "{name}");
    }
    assert_eq!(
        conn.object_ddl(&object("seq", ObjectKind::Sequence), options())
            .await
            .unwrap(),
        format!(
            "CREATE SEQUENCE [{schema}].[seq] AS bigint START WITH 100 INCREMENT BY 10 \
             MINVALUE 1 MAXVALUE 100000 CYCLE CACHE 20;"
        )
    );
    let missing = conn
        .object_ddl(&object("paboable_missing", ObjectKind::View), options())
        .await
        .unwrap_err();
    assert_eq!(missing.code, ErrorCode::NotFound);

    // The script made again under another name, in another schema, scripts back the same.
    let copied = table
        .replace(&name, &format!("[{copy}].[Khách hàng copy]"))
        .replace(
            &format!("@level0name = N'{schema}'"),
            &format!("@level0name = N'{copy}'"),
        )
        .replace("@level1name = N'Khách hàng'", "@level1name = N'Khách hàng copy'");
    for span in split(&copied, SqlGrammar::SqlServer) {
        run(&conn, span.text(&copied)).await;
    }
    let again = conn
        .object_ddl(
            &ObjectInfo {
                schema: Some(copy.clone()),
                name: "Khách hàng copy".into(),
                kind: ObjectKind::Table,
            },
            options(),
        )
        .await
        .unwrap();
    assert_eq!(again, copied);

    run(
        &conn,
        &format!(
            "drop table [{copy}].[Khách hàng copy]; drop view {schema}.v_kh; drop function {schema}.f_double; \
             drop function {schema}.f_rows; drop procedure {schema}.p_count; drop sequence {schema}.seq; \
             drop table {schema}.[Khách hàng]; drop table {schema}.customers; drop schema {copy}; drop schema {schema}"
        ),
    )
    .await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs SQL Server at $PABOABLE_MSSQL_URL"]
async fn columnstore_xml_and_spatial_indexes_are_listed_scripted_and_run_again() {
    use paboable_core::catalog::{IndexInfo, ObjectInfo};
    use paboable_core::sql::{SqlGrammar, split};

    let conn = live_in("tempdb").await;
    let schema = format!("paboable_ix_{}", std::process::id());
    let copy = format!("paboable_ix_copy_{}", std::process::id());
    let table = |name: &str| TableRef {
        schema: Some(schema.clone()),
        name: name.into(),
    };
    let object = |schema: &str, name: &str| ObjectInfo {
        schema: Some(schema.into()),
        name: name.into(),
        kind: ObjectKind::Table,
    };
    let options = CallOptions::default;
    run(&conn, &format!("create schema {schema}")).await;
    run(&conn, &format!("create schema {copy}")).await;
    run(
        &conn,
        &format!(
            "create table {schema}.facts (
                 id int not null, qty int not null, note nvarchar(50) null,
                 constraint pk_facts primary key nonclustered (id)
             );
             create unique nonclustered index ix_facts_qty on {schema}.facts (qty desc) include (note);
             create nonclustered columnstore index ncci_facts on {schema}.facts (qty, id) where qty > 0;
             create table {schema}.events (id int not null, at datetime2(3) not null, kind tinyint not null);
             create clustered columnstore index cci_events on {schema}.events;
             create table {schema}.docs (
                 id int not null constraint pk_docs primary key clustered,
                 body xml null,
                 shape geometry null
             );
             create primary xml index pxml_docs on {schema}.docs (body);
             create xml index a_path on {schema}.docs (body) using xml index pxml_docs for path;
             create xml index b_value on {schema}.docs (body) using xml index pxml_docs for value;
             create spatial index sp_docs_shape on {schema}.docs (shape) using geometry_grid
                 with (bounding_box = (0, 0, 100, 100),
                       grids = (level_1 = low, level_2 = medium, level_3 = high, level_4 = medium),
                       cells_per_object = 8);
             create table {schema}.gen (
                 id int identity(1, 1) not null, plain int null, twice as (plain * 2), rv rowversion
             );"
        ),
    )
    .await;

    let index = |name: &str, method: &str, columns: &[&str]| IndexInfo {
        name: name.into(),
        columns: columns.iter().map(|&column| column.into()).collect(),
        included: Vec::new(),
        unique: false,
        primary: false,
        method: method.into(),
        filter: None,
    };
    assert_eq!(
        conn.list_indexes(&table("facts"), options()).await.unwrap(),
        [
            IndexInfo {
                unique: true,
                primary: true,
                ..index("pk_facts", "NONCLUSTERED", &["id"])
            },
            IndexInfo {
                unique: true,
                included: vec!["note".into()],
                ..index("ix_facts_qty", "NONCLUSTERED", &["qty DESC"])
            },
            IndexInfo {
                filter: Some("([qty]>(0))".into()),
                ..index("ncci_facts", "NONCLUSTERED COLUMNSTORE", &["qty", "id"])
            },
        ]
    );
    let events = conn.list_indexes(&table("events"), options()).await.unwrap();
    assert_eq!(
        events
            .iter()
            .map(|index| (index.name.as_str(), index.method.as_str()))
            .collect::<Vec<_>>(),
        [("cci_events", "CLUSTERED COLUMNSTORE")]
    );
    assert_eq!(
        conn.list_indexes(&table("docs"), options()).await.unwrap(),
        [
            IndexInfo {
                unique: true,
                primary: true,
                ..index("pk_docs", "CLUSTERED", &["id"])
            },
            index("a_path", "XML", &["body"]),
            index("b_value", "XML", &["body"]),
            index("pxml_docs", "XML", &["body"]),
            index("sp_docs_shape", "SPATIAL", &["shape"]),
        ]
    );
    assert_eq!(conn.list_indexes(&table("gen"), options()).await.unwrap(), []);

    let generated: Vec<(String, bool)> = conn
        .fetch_columns(&table("gen"), options())
        .await
        .unwrap()
        .into_iter()
        .map(|column| (column.name, column.generated))
        .collect();
    assert_eq!(
        generated,
        [
            ("id".into(), true),
            ("plain".into(), false),
            ("twice".into(), true),
            ("rv".into(), true)
        ]
    );

    let mut scripts = Vec::new();
    for (name, pieces) in [
        (
            "facts",
            vec![format!(
                "CREATE NONCLUSTERED COLUMNSTORE INDEX [ncci_facts] ON [{schema}].[facts] ([qty], [id]) \
                 WHERE ([qty]>(0));"
            )],
        ),
        (
            "events",
            vec![format!(
                "CREATE CLUSTERED COLUMNSTORE INDEX [cci_events] ON [{schema}].[events];"
            )],
        ),
        (
            "docs",
            vec![
                format!("CREATE PRIMARY XML INDEX [pxml_docs] ON [{schema}].[docs] ([body]);"),
                format!(
                    "CREATE XML INDEX [a_path] ON [{schema}].[docs] ([body]) USING XML INDEX [pxml_docs] FOR PATH;"
                ),
                format!(
                    "CREATE XML INDEX [b_value] ON [{schema}].[docs] ([body]) USING XML INDEX [pxml_docs] FOR VALUE;"
                ),
                format!(
                    "CREATE SPATIAL INDEX [sp_docs_shape] ON [{schema}].[docs] ([shape]) USING GEOMETRY_GRID \
                     WITH (BOUNDING_BOX = (0, 0, 100, 100), \
                     GRIDS = (LEVEL_1 = LOW, LEVEL_2 = MEDIUM, LEVEL_3 = HIGH, LEVEL_4 = MEDIUM), \
                     CELLS_PER_OBJECT = 8);"
                ),
            ],
        ),
    ] {
        let script = conn.object_ddl(&object(&schema, name), options()).await.unwrap();
        for piece in &pieces {
            assert!(script.contains(piece.as_str()), "{piece}\nnot in\n{script}");
        }
        scripts.push((name, script));
    }
    let docs = &scripts[2].1;
    let primary_at = docs.find("CREATE PRIMARY XML INDEX").unwrap();
    assert!(docs.find("CREATE XML INDEX").is_some_and(|at| at > primary_at));

    // Each script made again in another schema scripts back the same.
    for (name, script) in &scripts {
        let copied = script.replace(&format!("[{schema}]."), &format!("[{copy}]."));
        for span in split(&copied, SqlGrammar::SqlServer) {
            run(&conn, span.text(&copied)).await;
        }
        let again = conn.object_ddl(&object(&copy, name), options()).await.unwrap();
        assert_eq!(again, copied, "{name}");
    }

    run(
        &conn,
        &format!(
            "drop table {copy}.facts; drop table {copy}.events; drop table {copy}.docs;
             drop table {schema}.facts; drop table {schema}.events; drop table {schema}.docs; drop table {schema}.gen;
             drop schema {copy}; drop schema {schema}"
        ),
    )
    .await;
}

#[test]
fn index_columns_fall_in_key_or_include_by_the_kind_of_index() {
    use crate::connection::{IndexColumnPlace, index_column_place};
    // Rowstore: clustered, nonclustered, hash.
    for kind in [1, 2, 7] {
        assert_eq!(index_column_place(kind, 1, false), Some(IndexColumnPlace::Key));
        assert_eq!(index_column_place(kind, 0, true), Some(IndexColumnPlace::Included));
        // A partitioning column the index did not name.
        assert_eq!(index_column_place(kind, 0, false), None);
    }
    // Columnstore: every column it keeps is marked included.
    for kind in [5, 6] {
        assert_eq!(index_column_place(kind, 0, true), Some(IndexColumnPlace::Key));
        assert_eq!(index_column_place(kind, 0, false), None);
    }
    // XML and spatial: one column, no key ordinal.
    for kind in [3, 4] {
        assert_eq!(index_column_place(kind, 0, false), Some(IndexColumnPlace::Key));
    }
}

// --- object scripts, without a server -------------------------------------------

mod ddl_text {
    use paboable_core::sql::{SqlGrammar, split};

    use crate::ddl::*;

    const COLLATION: &str = "SQL_Latin1_General_CP1_CI_AS";

    fn system(name: &str, max_length: i32, precision: i32, scale: i32) -> SqlType {
        SqlType {
            name: name.into(),
            user_schema: None,
            max_length,
            precision,
            scale,
        }
    }

    fn column(name: &str, ty: SqlType, nullable: bool) -> TableColumn {
        TableColumn {
            name: name.into(),
            ty,
            nullable,
            ..TableColumn::default()
        }
    }

    fn key_column(name: &str, descending: bool) -> IndexColumn {
        IndexColumn {
            name: name.into(),
            descending,
        }
    }

    #[test]
    fn names_go_in_brackets_with_a_closing_bracket_doubled() {
        assert_eq!(bracket("orders"), "[orders]");
        assert_eq!(bracket("a]b"), "[a]]b]");
        assert_eq!(bracket("[x]"), "[[x]]]");
        assert_eq!(qualified("dbo", "Khách hàng"), "[dbo].[Khách hàng]");
        assert_eq!(qualified("s]", "t"), "[s]]].[t]");
    }

    #[test]
    fn text_becomes_an_n_literal_with_its_quotes_doubled() {
        assert_eq!(text_literal("plain"), "N'plain'");
        assert_eq!(text_literal("it's"), "N'it''s'");
        assert_eq!(text_literal("Mô tả 'đơn'"), "N'Mô tả ''đơn'''");
        assert_eq!(text_literal(""), "N''");
    }

    #[test]
    fn types_carry_their_size_as_the_column_list_writes_it() {
        for (ty, text) in [
            (system("varchar", 20, 0, 0), "varchar(20)"),
            (system("varchar", -1, 0, 0), "varchar(max)"),
            (system("char", 1, 0, 0), "char(1)"),
            (system("varbinary", -1, 0, 0), "varbinary(max)"),
            (system("binary", 16, 0, 0), "binary(16)"),
            (system("nvarchar", 100, 0, 0), "nvarchar(50)"),
            (system("nvarchar", -1, 0, 0), "nvarchar(max)"),
            (system("nchar", 2, 0, 0), "nchar(1)"),
            (system("decimal", 9, 10, 3), "decimal(10,3)"),
            (system("numeric", 9, 18, 0), "numeric(18,0)"),
            (system("datetime2", 8, 27, 7), "datetime2(7)"),
            (system("time", 4, 10, 0), "time(0)"),
            (system("datetimeoffset", 10, 34, 3), "datetimeoffset(3)"),
            (system("int", 4, 10, 0), "int"),
            (system("datetime", 8, 23, 3), "datetime"),
            (system("sysname", 256, 0, 0), "sysname"),
        ] {
            assert_eq!(type_text(&ty), text, "{}", ty.name);
        }
    }

    #[test]
    fn a_user_type_is_written_by_its_schema_and_name_alone() {
        let ty = SqlType {
            name: "Số điện thoại".into(),
            user_schema: Some("dbo".into()),
            max_length: 40,
            precision: 0,
            scale: 0,
        };
        assert_eq!(type_text(&ty), "[dbo].[Số điện thoại]");
    }

    #[test]
    fn a_column_line_has_its_identity_collation_nullability_and_default() {
        let mut id = column("id", system("int", 4, 10, 0), false);
        id.identity = Some(Identity {
            seed: "10".into(),
            increment: "5".into(),
        });
        assert_eq!(column_line(&id, COLLATION), "[id] int IDENTITY(10, 5) NOT NULL");

        let mut code = column("code", system("varchar", 20, 0, 0), true);
        code.collation = Some("Latin1_General_BIN".into());
        assert_eq!(
            column_line(&code, COLLATION),
            "[code] varchar(20) COLLATE Latin1_General_BIN NULL"
        );
        code.collation = Some(COLLATION.into());
        assert_eq!(column_line(&code, COLLATION), "[code] varchar(20) NULL");

        let mut qty = column("Số lượng", system("int", 4, 10, 0), false);
        qty.default = Some(DefaultConstraint {
            name: "df_qty]".into(),
            definition: "((1))".into(),
        });
        assert_eq!(
            column_line(&qty, COLLATION),
            "[Số lượng] int NOT NULL CONSTRAINT [df_qty]]] DEFAULT ((1))"
        );
    }

    #[test]
    fn a_computed_column_is_written_with_its_expression_and_no_type() {
        let mut total = column("total", system("decimal", 9, 21, 2), false);
        total.computed = Some(Computed {
            definition: "([qty]*[price])".into(),
            persisted: true,
        });
        assert_eq!(
            column_line(&total, COLLATION),
            "[total] AS ([qty]*[price]) PERSISTED NOT NULL"
        );
        total.computed = Some(Computed {
            definition: "([qty]*[price])".into(),
            persisted: false,
        });
        // The server refuses NOT NULL on a computed column that is not persisted.
        assert_eq!(column_line(&total, COLLATION), "[total] AS ([qty]*[price])");
    }

    #[test]
    fn keys_and_checks_are_written_as_table_constraints() {
        let key = KeyConstraint {
            name: "pk_orders".into(),
            primary: true,
            clustered: true,
            columns: vec![key_column("id", false), key_column("line", true)],
        };
        assert_eq!(
            key_constraint_line(&key),
            "CONSTRAINT [pk_orders] PRIMARY KEY CLUSTERED ([id] ASC, [line] DESC)"
        );
        let unique = KeyConstraint {
            name: "uq_code".into(),
            primary: false,
            clustered: false,
            columns: vec![key_column("code", false)],
        };
        assert_eq!(
            key_constraint_line(&unique),
            "CONSTRAINT [uq_code] UNIQUE NONCLUSTERED ([code] ASC)"
        );
        let check = CheckConstraint {
            name: "ck_qty".into(),
            definition: "([qty]>(0))".into(),
        };
        assert_eq!(check_line(&check), "CONSTRAINT [ck_qty] CHECK ([qty]>(0))");
    }

    #[test]
    fn a_foreign_key_writes_only_the_actions_that_are_not_no_action() {
        assert_eq!(referential_action("NO_ACTION"), None);
        assert_eq!(referential_action("CASCADE"), Some("CASCADE"));
        assert_eq!(referential_action("SET_NULL"), Some("SET NULL"));
        assert_eq!(referential_action("SET_DEFAULT"), Some("SET DEFAULT"));
        let mut key = ForeignKey {
            name: "fk_customer".into(),
            columns: vec!["customer_id".into(), "region".into()],
            referenced_schema: "sales".into(),
            referenced_table: "Khách hàng".into(),
            referenced_columns: vec!["id".into(), "region".into()],
            on_delete: "CASCADE".into(),
            on_update: "NO_ACTION".into(),
        };
        assert_eq!(
            foreign_key_statement("[dbo].[orders]", &key),
            "ALTER TABLE [dbo].[orders] ADD CONSTRAINT [fk_customer] FOREIGN KEY ([customer_id], [region]) \
             REFERENCES [sales].[Khách hàng] ([id], [region]) ON DELETE CASCADE;"
        );
        key.on_delete = "NO_ACTION".into();
        key.on_update = "SET_NULL".into();
        assert!(foreign_key_statement("[dbo].[orders]", &key).ends_with("([id], [region]) ON UPDATE SET NULL;"));
    }

    #[test]
    fn an_index_writes_its_keys_included_columns_and_filter() {
        let mut index = Index {
            name: "ix_customer".into(),
            unique: false,
            clustered: false,
            columns: vec![key_column("customer_id", true)],
            included: vec!["note".into(), "qty".into()],
            filter: Some("([customer_id] IS NOT NULL)".into()),
        };
        assert_eq!(
            index_statement("[dbo].[orders]", &index),
            "CREATE NONCLUSTERED INDEX [ix_customer] ON [dbo].[orders] ([customer_id] DESC) \
             INCLUDE ([note], [qty]) WHERE ([customer_id] IS NOT NULL);"
        );
        index.unique = true;
        index.clustered = true;
        index.included.clear();
        index.filter = None;
        assert_eq!(
            index_statement("[dbo].[orders]", &index),
            "CREATE UNIQUE CLUSTERED INDEX [ix_customer] ON [dbo].[orders] ([customer_id] DESC);"
        );
    }

    #[test]
    fn a_description_names_its_schema_table_and_column_as_literals() {
        let table = Description {
            column: None,
            text: "Đơn hàng của 'khách'".into(),
        };
        assert_eq!(
            description_statement("dbo", "Khách hàng", &table),
            "EXEC sys.sp_addextendedproperty @name = N'MS_Description', @value = N'Đơn hàng của ''khách''', \
             @level0type = N'SCHEMA', @level0name = N'dbo', @level1type = N'TABLE', @level1name = N'Khách hàng';"
        );
        let column = Description {
            column: Some("O'Brien".into()),
            text: "x".into(),
        };
        assert!(
            description_statement("dbo", "t", &column)
                .ends_with("@level1name = N't', @level2type = N'COLUMN', @level2name = N'O''Brien';")
        );
    }

    #[test]
    fn a_table_script_is_one_create_then_a_batch_per_foreign_key_index_and_description() {
        let mut id = column("id", system("int", 4, 10, 0), false);
        id.identity = Some(Identity {
            seed: "1".into(),
            increment: "1".into(),
        });
        let table = Table {
            schema: "dbo".into(),
            name: "Khách hàng".into(),
            database_collation: COLLATION.into(),
            columns: vec![id, column("tên", system("nvarchar", 200, 0, 0), true)],
            keys: vec![KeyConstraint {
                name: "pk".into(),
                primary: true,
                clustered: true,
                columns: vec![key_column("id", false)],
            }],
            checks: vec![CheckConstraint {
                name: "ck".into(),
                definition: "([id]>(0))".into(),
            }],
            foreign_keys: vec![ForeignKey {
                name: "fk".into(),
                columns: vec!["id".into()],
                referenced_schema: "dbo".into(),
                referenced_table: "other".into(),
                referenced_columns: vec!["id".into()],
                on_delete: "NO_ACTION".into(),
                on_update: "NO_ACTION".into(),
            }],
            indexes: vec![Index {
                name: "ix".into(),
                columns: vec![key_column("tên", false)],
                ..Index::default()
            }],
            descriptions: vec![Description {
                column: Some("tên".into()),
                text: "Tên".into(),
            }],
            ..Table::default()
        };
        let script = table_ddl(&table);
        assert_eq!(
            script,
            "CREATE TABLE [dbo].[Khách hàng] (\n    \
             [id] int IDENTITY(1, 1) NOT NULL,\n    \
             [tên] nvarchar(100) NULL,\n    \
             CONSTRAINT [pk] PRIMARY KEY CLUSTERED ([id] ASC),\n    \
             CONSTRAINT [ck] CHECK ([id]>(0))\n\
             );\nGO\n\
             ALTER TABLE [dbo].[Khách hàng] ADD CONSTRAINT [fk] FOREIGN KEY ([id]) REFERENCES [dbo].[other] ([id]);\nGO\n\
             CREATE NONCLUSTERED INDEX [ix] ON [dbo].[Khách hàng] ([tên] ASC);\nGO\n\
             EXEC sys.sp_addextendedproperty @name = N'MS_Description', @value = N'Tên', \
             @level0type = N'SCHEMA', @level0name = N'dbo', @level1type = N'TABLE', @level1name = N'Khách hàng', \
             @level2type = N'COLUMN', @level2name = N'tên';"
        );
        assert_eq!(split(&script, SqlGrammar::SqlServer).len(), 4);
    }

    #[test]
    fn a_columnstore_index_names_its_columns_only_when_nonclustered() {
        let mut index = ColumnstoreIndex {
            name: "cci".into(),
            clustered: true,
            ..ColumnstoreIndex::default()
        };
        assert_eq!(
            columnstore_statement("[dbo].[events]", &index),
            "CREATE CLUSTERED COLUMNSTORE INDEX [cci] ON [dbo].[events];"
        );
        index.order = vec!["at".into(), "Mã".into()];
        assert_eq!(
            columnstore_statement("[dbo].[events]", &index),
            "CREATE CLUSTERED COLUMNSTORE INDEX [cci] ON [dbo].[events] ORDER ([at], [Mã]);"
        );
        let index = ColumnstoreIndex {
            name: "ncci]".into(),
            clustered: false,
            columns: vec!["qty".into(), "id".into()],
            order: Vec::new(),
            filter: Some("([qty]>(0))".into()),
        };
        assert_eq!(
            columnstore_statement("[dbo].[facts]", &index),
            "CREATE NONCLUSTERED COLUMNSTORE INDEX [ncci]]] ON [dbo].[facts] ([qty], [id]) WHERE ([qty]>(0));"
        );
    }

    #[test]
    fn a_secondary_xml_index_names_its_primary_and_what_it_is_for() {
        assert_eq!(XmlPurpose::from_code("V"), Some(XmlPurpose::Value));
        assert_eq!(XmlPurpose::from_code("P"), Some(XmlPurpose::Path));
        assert_eq!(XmlPurpose::from_code("R"), Some(XmlPurpose::Property));
        assert_eq!(XmlPurpose::from_code("X"), None);
        let primary = XmlIndex {
            name: "pxml".into(),
            column: "body".into(),
            secondary: None,
        };
        assert_eq!(
            xml_index_statement("[dbo].[docs]", &primary),
            "CREATE PRIMARY XML INDEX [pxml] ON [dbo].[docs] ([body]);"
        );
        for (purpose, keyword) in [
            (XmlPurpose::Value, "VALUE"),
            (XmlPurpose::Path, "PATH"),
            (XmlPurpose::Property, "PROPERTY"),
        ] {
            let secondary = XmlIndex {
                name: "sxml".into(),
                column: "body".into(),
                secondary: Some(SecondaryXml {
                    primary: "pxml".into(),
                    purpose,
                }),
            };
            assert_eq!(
                xml_index_statement("[dbo].[docs]", &secondary),
                format!("CREATE XML INDEX [sxml] ON [dbo].[docs] ([body]) USING XML INDEX [pxml] FOR {keyword};")
            );
        }
    }

    fn spatial(scheme: &str) -> SpatialIndex {
        SpatialIndex {
            name: "sp".into(),
            column: "shape".into(),
            scheme: scheme.into(),
            bounding_box: Some([0.0, -10.5, 100.0, 1e6]),
            grids: vec!["LOW".into(), "MEDIUM".into(), "HIGH".into(), "MEDIUM".into()],
            cells_per_object: Some(8),
        }
    }

    #[test]
    fn a_spatial_index_writes_a_box_only_for_geometry_and_grids_only_when_not_auto() {
        assert_eq!(
            spatial_index_statement("[dbo].[docs]", &spatial("GEOMETRY_GRID")),
            "CREATE SPATIAL INDEX [sp] ON [dbo].[docs] ([shape]) USING GEOMETRY_GRID WITH (\
             BOUNDING_BOX = (0, -10.5, 100, 1000000), \
             GRIDS = (LEVEL_1 = LOW, LEVEL_2 = MEDIUM, LEVEL_3 = HIGH, LEVEL_4 = MEDIUM), \
             CELLS_PER_OBJECT = 8);"
        );
        assert_eq!(
            spatial_index_statement("[dbo].[docs]", &spatial("GEOMETRY_AUTO_GRID")),
            "CREATE SPATIAL INDEX [sp] ON [dbo].[docs] ([shape]) USING GEOMETRY_AUTO_GRID WITH (\
             BOUNDING_BOX = (0, -10.5, 100, 1000000), CELLS_PER_OBJECT = 8);"
        );
        assert_eq!(
            spatial_index_statement("[dbo].[docs]", &spatial("GEOGRAPHY_GRID")),
            "CREATE SPATIAL INDEX [sp] ON [dbo].[docs] ([shape]) USING GEOGRAPHY_GRID WITH (\
             GRIDS = (LEVEL_1 = LOW, LEVEL_2 = MEDIUM, LEVEL_3 = HIGH, LEVEL_4 = MEDIUM), CELLS_PER_OBJECT = 8);"
        );
        assert_eq!(
            spatial_index_statement("[dbo].[docs]", &spatial("GEOGRAPHY_AUTO_GRID")),
            "CREATE SPATIAL INDEX [sp] ON [dbo].[docs] ([shape]) USING GEOGRAPHY_AUTO_GRID WITH (\
             CELLS_PER_OBJECT = 8);"
        );
        let bare = SpatialIndex {
            bounding_box: None,
            grids: Vec::new(),
            cells_per_object: None,
            ..spatial("GEOMETRY_GRID")
        };
        assert_eq!(
            spatial_index_statement("[dbo].[docs]", &bare),
            "CREATE SPATIAL INDEX [sp] ON [dbo].[docs] ([shape]) USING GEOMETRY_GRID;"
        );
    }

    #[test]
    fn other_index_kinds_follow_the_rowstore_ones_and_xml_primaries_come_before_secondaries() {
        let secondary = |name: &str, purpose| XmlIndex {
            name: name.into(),
            column: "body".into(),
            secondary: Some(SecondaryXml {
                primary: "z_primary".into(),
                purpose,
            }),
        };
        let table = Table {
            schema: "dbo".into(),
            name: "docs".into(),
            database_collation: COLLATION.into(),
            columns: vec![column("id", system("int", 4, 10, 0), false)],
            indexes: vec![Index {
                name: "ix".into(),
                columns: vec![key_column("id", false)],
                ..Index::default()
            }],
            columnstores: vec![ColumnstoreIndex {
                name: "ncci".into(),
                columns: vec!["id".into()],
                ..ColumnstoreIndex::default()
            }],
            xml_indexes: vec![
                secondary("a_path", XmlPurpose::Path),
                XmlIndex {
                    name: "z_primary".into(),
                    column: "body".into(),
                    secondary: None,
                },
                secondary("b_value", XmlPurpose::Value),
            ],
            spatial_indexes: vec![spatial("GEOMETRY_AUTO_GRID")],
            descriptions: vec![Description {
                column: None,
                text: "x".into(),
            }],
            ..Table::default()
        };
        let script = table_ddl(&table);
        let statements: Vec<&str> = split(&script, SqlGrammar::SqlServer)
            .into_iter()
            .map(|span| span.text(&script).trim())
            .collect();
        let starts: Vec<&str> = statements
            .iter()
            .map(|statement| statement.split(" ON ").next().unwrap_or_default())
            .collect();
        assert_eq!(
            starts,
            [
                "CREATE TABLE [dbo].[docs] (\n    [id] int NOT NULL\n);",
                "CREATE NONCLUSTERED INDEX [ix]",
                "CREATE NONCLUSTERED COLUMNSTORE INDEX [ncci]",
                "CREATE PRIMARY XML INDEX [z_primary]",
                "CREATE XML INDEX [a_path]",
                "CREATE XML INDEX [b_value]",
                "CREATE SPATIAL INDEX [sp]",
                "EXEC sys.sp_addextendedproperty @name = N'MS_Description', @value = N'x', \
                 @level0type = N'SCHEMA', @level0name = N'dbo', @level1type = N'TABLE', @level1name = N'docs';",
            ]
        );
    }

    #[test]
    fn a_table_with_only_columns_is_one_statement() {
        let table = Table {
            schema: "dbo".into(),
            name: "t".into(),
            database_collation: COLLATION.into(),
            columns: vec![column("a", system("bit", 1, 1, 0), true)],
            ..Table::default()
        };
        assert_eq!(table_ddl(&table), "CREATE TABLE [dbo].[t] (\n    [a] bit NULL\n);");
    }

    #[test]
    fn a_sequence_writes_every_option_and_its_cache() {
        let mut sequence = Sequence {
            schema: "dbo".into(),
            name: "Số thứ tự".into(),
            ty: system("bigint", 8, 19, 0),
            start: "100".into(),
            increment: "10".into(),
            minimum: "1".into(),
            maximum: "100000".into(),
            cycling: true,
            cached: true,
            cache_size: Some(20),
        };
        assert_eq!(
            sequence_ddl(&sequence),
            "CREATE SEQUENCE [dbo].[Số thứ tự] AS bigint START WITH 100 INCREMENT BY 10 \
             MINVALUE 1 MAXVALUE 100000 CYCLE CACHE 20;"
        );
        sequence.cache_size = None;
        assert!(sequence_ddl(&sequence).ends_with(" CYCLE CACHE;"));
        sequence.cached = false;
        sequence.cycling = false;
        sequence.ty = system("decimal", 9, 12, 0);
        let text = sequence_ddl(&sequence);
        assert!(text.contains(" AS decimal(12,0) "));
        assert!(text.ends_with(" NO CYCLE NO CACHE;"));
    }
}

#[test]
fn every_kind_the_catalog_lists_has_a_folder_in_the_tree() {
    use crate::connection::object_kind;
    use paboable_core::catalog::ObjectKind;
    // The types list_objects reads (its OBJECTS query).
    for code in ["U", "V", "SO", "P", "PC", "FN", "FS", "IF", "TF", "FT"] {
        let kind = object_kind(code);
        assert_ne!(kind, ObjectKind::Other, "{code}");
        assert!(
            crate::INFO
                .tree_folders
                .iter()
                .any(|folder| folder.kinds.contains(&kind)),
            "{code} ({kind:?}) would show in no folder"
        );
    }
}
