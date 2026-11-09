use std::sync::Mutex;
use std::time::Duration;

use paboable_store::{ResultStore, WindowReader, WindowRequest};

use super::*;

/// Collects what a statement returns: the last set's rows, and the commands.
#[derive(Default)]
struct Collect {
    store: Mutex<Option<Arc<ResultStore>>>,
    commands: Mutex<Vec<Option<u64>>>,
}

impl ResultSink for Collect {
    fn begin_set(&self, columns: Vec<ColumnMeta>) {
        *self.store.lock().unwrap() = Some(Arc::new(ResultStore::new(columns, Some(1000))));
    }

    fn push_rows(&self, values: &[Value]) -> bool {
        self.store
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|store| store.push_rows(values))
    }

    fn end_set(&self) {
        if let Some(store) = self.store.lock().unwrap().as_ref() {
            store.finish();
        }
    }

    fn command_done(&self, rows_affected: Option<u64>) {
        self.commands.lock().unwrap().push(rows_affected);
    }

    fn notice(&self, _text: &str) {}
}

async fn memory() -> Arc<dyn Connection> {
    let config = ConnectionConfig {
        engine: "sqlite".into(),
        target: Target::File {
            path: ":memory:".into(),
            read_only: false,
        },
        options: Default::default(),
    };
    SqliteDriver
        .connect(&config, CallOptions::default())
        .await
        .expect("an in-memory database opens")
}

async fn run(conn: &Arc<dyn Connection>, sql: &str) -> (Arc<Collect>, Result<ExecOutcome>) {
    let sink = Arc::new(Collect::default());
    let result = conn.execute(sql, sink.clone(), CallOptions::default()).await;
    (sink, result)
}

#[tokio::test(flavor = "multi_thread")]
async fn commands_report_rows_affected_and_queries_stream_rows() {
    let conn = memory().await;
    run(&conn, "create table t (id integer primary key, name text, data blob)")
        .await
        .1
        .unwrap();
    let (sink, result) = run(&conn, "insert into t (name, data) values ('a', x'00ff'), (null, null)").await;
    result.unwrap();
    assert_eq!(*sink.commands.lock().unwrap(), [Some(2)]);

    let (sink, result) = run(&conn, "select id, name, data from t order by id").await;
    result.unwrap();
    let store = sink.store.lock().unwrap().clone().expect("a result set");
    assert_eq!(store.columns()[0].kind, ValueKind::Int);
    assert_eq!(store.columns()[2].kind, ValueKind::Bytes);
    let state = store.state();
    assert_eq!((state.total_rows, state.finished), (2, true));

    let bytes = store.fetch_window(WindowRequest {
        first_row: 0,
        row_count: 10,
        first_column: 0,
        column_count: 3,
        max_chars: 50,
    });
    let window = WindowReader::new(&bytes).unwrap();
    assert_eq!(window.text(0, 1), "a");
    assert_eq!(window.text(0, 2), "0x00ff");
    assert!(window.is_null(1, 1));
}

#[tokio::test(flavor = "multi_thread")]
async fn the_row_limit_stops_reading() {
    let conn = memory().await;
    let sql = "with recursive c(x) as (select 1 union all select x + 1 from c limit 5000) select x from c";
    let (sink, result) = run(&conn, sql).await;
    assert!(result.unwrap().limited);
    let store = sink.store.lock().unwrap().clone().unwrap();
    assert_eq!(store.state().total_rows, 1000);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_cancel_interrupts_a_running_statement() {
    let conn = memory().await;
    let options = CallOptions::default();
    let cancel = options.cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        cancel.cancel();
    });
    let endless = "with recursive c(x) as (select 1 union all select x + 1 from c) select count(*) from c";
    let error = conn
        .execute(endless, Arc::new(Collect::default()), options)
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Cancelled);
    // The connection is still usable after an interrupt.
    run(&conn, "select 1").await.1.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_deadline_interrupts_and_reports_a_timeout() {
    let conn = memory().await;
    let options = CallOptions::default().with_timeout(Duration::from_millis(100));
    let endless = "with recursive c(x) as (select 1 union all select x + 1 from c) select count(*) from c";
    let error = conn
        .execute(endless, Arc::new(Collect::default()), options)
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Timeout);
}

#[tokio::test(flavor = "multi_thread")]
async fn syntax_errors_carry_the_query_code() {
    let conn = memory().await;
    let error = run(&conn, "selec 1").await.1.unwrap_err();
    assert_eq!(error.code, ErrorCode::Query);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_catalog_lists_tables_and_columns() {
    let conn = memory().await;
    run(
        &conn,
        "create table \"we\"\"ird\" (id integer primary key, name text not null default 'x')",
    )
    .await
    .1
    .unwrap();
    run(&conn, "create view v as select 1").await.1.unwrap();

    let schemas = conn.list_schemas(CallOptions::default()).await.unwrap();
    assert!(schemas.iter().any(|s| s.name == "main" && s.is_default));

    let objects = conn.list_objects(None, CallOptions::default()).await.unwrap();
    let names: Vec<_> = objects.iter().map(|o| (o.name.as_str(), o.kind)).collect();
    assert_eq!(names, [("we\"ird", ObjectKind::Table), ("v", ObjectKind::View)]);

    let table = TableRef {
        schema: None,
        name: "we\"ird".into(),
    };
    let columns = conn.fetch_columns(&table, CallOptions::default()).await.unwrap();
    assert_eq!(columns.len(), 2);
    assert_eq!(columns[0].primary_key, Some(1));
    assert!(!columns[1].nullable);
    assert_eq!(columns[1].default.as_deref(), Some("'x'"));
}

#[tokio::test(flavor = "multi_thread")]
async fn opening_a_file_that_is_not_a_database_fails_to_connect() {
    let dir = std::env::temp_dir().join(format!("paboable-sqlite-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("not-a-db.sqlite");
    std::fs::write(
        &path,
        b"this is plain text, long enough to hold a header of a hundred bytes or so, surely more than that",
    )
    .unwrap();
    let config = ConnectionConfig {
        engine: "sqlite".into(),
        target: Target::File {
            path: path.clone(),
            read_only: true,
        },
        options: Default::default(),
    };
    let error = SqliteDriver
        .connect(&config, CallOptions::default())
        .await
        .err()
        .expect("a refusal");
    assert_eq!(error.code, ErrorCode::Connect);
    std::fs::remove_dir_all(&dir).ok();
}

/// The rows of `sql`, each as its cells' text.
async fn texts(conn: &Arc<dyn Connection>, sql: &str) -> Vec<Vec<Option<String>>> {
    let (sink, result) = run(conn, sql).await;
    result.expect("the query runs");
    let store = sink.store.lock().unwrap().clone().expect("a result set");
    (0..store.state().total_rows)
        .map(|row| store.row_texts(row).unwrap_or_default())
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn edits_are_saved_together_or_not_at_all() {
    let conn = memory().await;
    run(&conn, "create table t (id integer primary key, name text)")
        .await
        .1
        .unwrap();
    run(&conn, "insert into t values (1, 'a'), (2, 'b')").await.1.unwrap();

    let saved = [
        "UPDATE \"t\" SET \"name\" = 'A' WHERE \"id\" = 1".to_owned(),
        "DELETE FROM \"t\" WHERE \"id\" = 2".to_owned(),
        "INSERT INTO \"t\" (\"name\") VALUES ('c')".to_owned(),
    ];
    conn.apply_edits(&saved, CallOptions::default()).await.unwrap();
    assert_eq!(
        texts(&conn, "select id, name from t order by id").await,
        [
            vec![Some("1".to_owned()), Some("A".to_owned())],
            vec![Some("2".to_owned()), Some("c".to_owned())]
        ]
    );

    // The second names a row that is gone: the first is rolled back with it.
    let refused = [
        "UPDATE \"t\" SET \"name\" = 'changed' WHERE \"id\" = 1".to_owned(),
        "UPDATE \"t\" SET \"name\" = 'x' WHERE \"id\" = 5".to_owned(),
    ];
    let error = conn.apply_edits(&refused, CallOptions::default()).await.err().unwrap();
    assert_eq!(error.code, ErrorCode::RowGone);
    assert_eq!(error.message, "statement 2 changed 0 rows");
    assert_eq!(
        texts(&conn, "select name from t where id = 1").await,
        [vec![Some("A".to_owned())]]
    );

    // A failing statement rolls back too, and the session takes the next run.
    let failing = [
        "DELETE FROM \"t\" WHERE \"id\" = 1".to_owned(),
        "INSERT INTO \"t\" (\"id\", \"name\") VALUES (2, 'duplicate')".to_owned(),
    ];
    let error = conn.apply_edits(&failing, CallOptions::default()).await.err().unwrap();
    assert_eq!(error.code, ErrorCode::Query);
    assert_eq!(
        texts(&conn, "select count(*) from t").await,
        [vec![Some("2".to_owned())]]
    );
}
