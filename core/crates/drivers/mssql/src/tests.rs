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
