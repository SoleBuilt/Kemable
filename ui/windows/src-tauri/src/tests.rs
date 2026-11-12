use std::sync::{Arc, Mutex};

use paboable_core::CallOptions;
use paboable_core::registry::DriverRegistry;
use paboable_core::sql::{SqlGrammar, split};
use paboable_driver_mssql::MssqlDriver;
use paboable_driver_postgres::PostgresDriver;
use serde_json::Value as Json;
use tauri::ipc::{Channel, InvokeResponseBody};
use tokio_util::sync::CancellationToken;

use crate::run::{self, Run, Statement};
use crate::state::{AppState, OpenServer};

/// Runs `script` the way `run_query` does and returns its events as JSON.
async fn run_script(state: &Arc<AppState>, server: &Arc<OpenServer>, script: &str, grammar: SqlGrammar) -> Vec<Json> {
    let events = Arc::new(Mutex::new(Vec::new()));
    let collected = Arc::clone(&events);
    let channel = Channel::new(move |body| {
        if let InvokeResponseBody::Json(json) = body {
            collected
                .lock()
                .unwrap()
                .push(serde_json::from_str::<Json>(&json).unwrap());
        }
        Ok(())
    });
    let statements = split(script, grammar)
        .iter()
        .map(|span| Statement {
            text: span.text(script).to_owned(),
            kind: span.kind,
        })
        .collect();
    let run = Run {
        id: 1,
        server: Arc::clone(server),
        database: None,
        grammar: Some(grammar),
        statements,
        follow_created_database: true,
        row_limit: Some(500_000),
        record: false,
        cancel: CancellationToken::new(),
        channel,
    };
    run::execute(Arc::clone(state), run).await;
    events.lock().unwrap().clone()
}

fn of_type<'a>(events: &'a [Json], kind: &str) -> Vec<&'a Json> {
    events.iter().filter(|event| event["type"] == kind).collect()
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs PostgreSQL at $PABOABLE_POSTGRES_URL"]
async fn a_script_runs_on_in_the_database_it_creates() {
    let url = std::env::var("PABOABLE_POSTGRES_URL").expect("PABOABLE_POSTGRES_URL is set");
    let config = paboable_core::connection_url::parse(&url).expect("a valid URL");
    let state = Arc::new(AppState::new(DriverRegistry::new().with(Arc::new(PostgresDriver))));
    let driver = state
        .registry
        .get("postgres")
        .cloned()
        .expect("the driver is registered");
    let first = driver
        .connect(&config, CallOptions::default())
        .await
        .expect("the server accepts the connection");
    let server = Arc::new(OpenServer::new(driver, config, first, "test".into()));
    let home = server.home.clone();
    let created = format!("paboable_follow_{}", std::process::id());

    let script = format!(
        "CREATE DATABASE {upper};\n\
         create table followed (x int);\n\
         select to_regclass('public.followed') is not null as here;\n\
         \\c {home}\n\
         select to_regclass('public.followed') is null as not_here;\n\
         drop database {created};",
        upper = created.to_uppercase(),
    );
    let events = run_script(&state, &server, &script, SqlGrammar::Postgres).await;

    assert!(of_type(&events, "statementFailed").is_empty(), "{events:#?}");
    let switches: Vec<_> = of_type(&events, "databaseChanged")
        .iter()
        .map(|event| {
            (
                event["statement"].as_u64().unwrap(),
                event["database"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    // The unquoted name folds to lower case, as PostgreSQL does.
    assert_eq!(switches, [(0, created.clone()), (3, home.clone())]);

    // Each check read one boolean: the table is in the new database, not the home one.
    let answers: Vec<_> = of_type(&events, "setStarted")
        .iter()
        .map(|event| {
            let store = state.result(event["result"].as_u64().unwrap()).unwrap();
            store.cell_text(0, 0).unwrap()
        })
        .collect();
    assert_eq!(answers, [Some("true".to_owned()), Some("true".to_owned())]);
    server.close().await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs SQL Server at $PABOABLE_MSSQL_URL"]
async fn a_sql_server_script_follows_create_database_and_use() {
    let url = std::env::var("PABOABLE_MSSQL_URL").expect("PABOABLE_MSSQL_URL is set");
    let config = paboable_core::connection_url::parse(&url).expect("a valid URL");
    let state = Arc::new(AppState::new(DriverRegistry::new().with(Arc::new(MssqlDriver))));
    let driver = state.registry.get("mssql").cloned().expect("the driver is registered");
    let first = driver
        .connect(&config, CallOptions::default())
        .await
        .expect("the server accepts the connection");
    let server = Arc::new(OpenServer::new(driver, config, first, "test".into()));
    let created = format!("paboable_follow_{}", std::process::id());

    // GO splits the batches; the table lands in the new database, then
    // USE master moves back and the drop is not refused by our own session.
    let script = format!(
        "CREATE DATABASE [{created}]\nGO\n\
         create table followed (x int)\nGO\n\
         select db_name()\nGO\n\
         use master\nGO\n\
         select case when db_id('{created}') is not null then 1 else 0 end\nGO\n\
         drop database [{created}]"
    );
    let events = run_script(&state, &server, &script, SqlGrammar::SqlServer).await;

    assert!(of_type(&events, "statementFailed").is_empty(), "{events:#?}");
    let switches: Vec<_> = of_type(&events, "databaseChanged")
        .iter()
        .map(|event| event["database"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(switches, [created.clone(), "master".to_owned()]);
    let answers: Vec<_> = of_type(&events, "setStarted")
        .iter()
        .map(|event| {
            state
                .result(event["result"].as_u64().unwrap())
                .unwrap()
                .cell_text(0, 0)
                .unwrap()
        })
        .collect();
    assert_eq!(answers, [Some(created.clone()), Some("1".to_owned())]);
    server.close().await;
}

/// The first row's cells of each result set the events started.
fn first_rows(state: &AppState, events: &[Json]) -> Vec<Vec<Option<String>>> {
    of_type(events, "setStarted")
        .iter()
        .map(|event| {
            state
                .result(event["result"].as_u64().unwrap())
                .unwrap()
                .row_texts(0)
                .unwrap_or_default()
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs PostgreSQL at $PABOABLE_POSTGRES_URL"]
async fn a_sorted_statement_runs_on_postgres_and_orders_before_the_limit() {
    use paboable_core::sql::{SortColumn, sorted_statement};
    let url = std::env::var("PABOABLE_POSTGRES_URL").expect("PABOABLE_POSTGRES_URL is set");
    let config = paboable_core::connection_url::parse(&url).expect("a valid URL");
    let state = Arc::new(AppState::new(DriverRegistry::new().with(Arc::new(PostgresDriver))));
    let driver = state.registry.get("postgres").cloned().unwrap();
    let first = driver.connect(&config, CallOptions::default()).await.unwrap();
    let server = Arc::new(OpenServer::new(driver, config, first, "test".into()));

    // m descending, then n: the LIMIT now takes the five rows that order first.
    let original = "select g as n, g % 3 as m from generate_series(1, 9) g limit 5 -- five rows";
    let keys = [
        SortColumn {
            column: 1,
            descending: true,
        },
        SortColumn {
            column: 0,
            descending: false,
        },
    ];
    let sql =
        sorted_statement(original, SqlGrammar::Postgres, &["n".into(), "m".into()], &keys).expect("it can run again");
    let events = run_script(&state, &server, &sql, SqlGrammar::Postgres).await;
    assert!(of_type(&events, "statementFailed").is_empty(), "{sql}\n{events:#?}");
    assert_eq!(
        first_rows(&state, &events),
        [vec![Some("2".to_owned()), Some("2".to_owned())]]
    );
    server.close().await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs SQL Server at $PABOABLE_MSSQL_URL"]
async fn a_sorted_statement_runs_on_sql_server_with_top_and_option() {
    use paboable_core::sql::{SortColumn, sorted_statement};
    let url = std::env::var("PABOABLE_MSSQL_URL").expect("PABOABLE_MSSQL_URL is set");
    let config = paboable_core::connection_url::parse(&url).expect("a valid URL");
    let state = Arc::new(AppState::new(DriverRegistry::new().with(Arc::new(MssqlDriver))));
    let driver = state.registry.get("mssql").cloned().unwrap();
    let first = driver.connect(&config, CallOptions::default()).await.unwrap();
    let server = Arc::new(OpenServer::new(driver, config, first, "test".into()));

    let original =
        "SELECT TOP 3 n, n % 2 AS parity FROM (VALUES (1), (2), (3), (4)) AS v(n) ORDER BY n OPTION (RECOMPILE)";
    let keys = [SortColumn {
        column: 0,
        descending: true,
    }];
    let sql = sorted_statement(original, SqlGrammar::SqlServer, &["n".into(), "parity".into()], &keys)
        .expect("it can run again");
    let events = run_script(&state, &server, &sql, SqlGrammar::SqlServer).await;
    assert!(of_type(&events, "statementFailed").is_empty(), "{sql}\n{events:#?}");
    assert_eq!(
        first_rows(&state, &events),
        [vec![Some("4".to_owned()), Some("0".to_owned())]]
    );
    server.close().await;
}

/// The same table on each engine, read through the statements a table tab
/// writes (docs/decisions/0006-table-browsing.md). The expected rows are
/// worked out here, so a wrong page or count on any engine fails.
async fn browse_checks(state: &Arc<AppState>, server: &Arc<OpenServer>, grammar: SqlGrammar) {
    use paboable_core::catalog::TableRef;
    use paboable_core::sql::{BrowseQuery, BrowseSort, Filter, FilterOp, count_statement, page_statement};

    let name = format!("paboable_browse_{}", std::process::id());
    let code = |id: u32| {
        if id == 7 {
            "50%_off".to_owned()
        } else {
            format!("C-{id:02}")
        }
    };
    let total = |id: u32| (id * 37) % 100;
    let rows: Vec<String> = (1..=25)
        .map(|id| {
            let note = if id % 5 == 0 {
                "NULL".to_owned()
            } else {
                format!("'n{id}'")
            };
            format!("({id}, '{}', {}, {note})", code(id), total(id))
        })
        .collect();
    let text_type = if grammar == SqlGrammar::SqlServer {
        "nvarchar(50)"
    } else {
        "varchar(50)"
    };
    let setup = format!(
        "create table {name} (id int primary key, code {text_type} not null, total decimal(10, 2) not null, note {text_type});\n\
         insert into {name} values {};",
        rows.join(", ")
    );
    let events = run_script(state, server, &setup, grammar).await;
    assert!(of_type(&events, "statementFailed").is_empty(), "{events:#?}");

    let query = |filters: Vec<Filter>| BrowseQuery {
        table: TableRef {
            schema: None,
            name: name.clone(),
        },
        filters,
        match_any: false,
        sort: Vec::new(),
        key: vec!["id".into()],
        offset: 0,
        limit: 100,
    };
    let filter = |column: &str, op: FilterOp, value: &str, second: &str| Filter {
        column: column.into(),
        op,
        value: value.into(),
        second: second.into(),
    };
    let count = |browse: BrowseQuery| {
        let sql = count_statement(grammar, &browse).expect("a count statement");
        async move {
            let events = run_script(state, server, &sql, grammar).await;
            assert!(of_type(&events, "statementFailed").is_empty(), "{sql}\n{events:#?}");
            first_rows(state, &events)[0][0].clone().unwrap_or_default()
        }
    };

    let cases: Vec<(Vec<Filter>, usize)> = vec![
        (vec![filter("code", FilterOp::Contains, "%_", "")], 1),
        (vec![filter("code", FilterOp::Contains, "c-1", "")], 10),
        (vec![filter("note", FilterOp::IsNull, "", "")], 5),
        (vec![filter("id", FilterOp::Equals, "7", "")], 1),
        (vec![filter("id", FilterOp::In, "3, 4, 99", "")], 2),
        (vec![filter("code", FilterOp::Equals, "O'x", "")], 0),
        (vec![filter("", FilterOp::Raw, "id % 2 = 0 -- even", "")], 12),
        (
            vec![
                filter("total", FilterOp::Between, "20", "40"),
                filter("code", FilterOp::StartsWith, "C-1", ""),
            ],
            (10..=19).filter(|id| (20..=40).contains(&total(*id))).count(),
        ),
    ];
    for (filters, expected) in cases {
        let shown = format!("{filters:?}");
        assert_eq!(count(query(filters)).await, expected.to_string(), "{grammar:?} {shown}");
    }

    // The second page of five, by total descending, ties broken by id.
    let mut order: Vec<u32> = (1..=25).collect();
    order.sort_by_key(|id| (std::cmp::Reverse(total(*id)), *id));
    let mut page = query(Vec::new());
    page.sort = vec![BrowseSort {
        column: "total".into(),
        descending: true,
    }];
    page.offset = 5;
    page.limit = 5;
    let sql = page_statement(grammar, &page).expect("a page statement");
    let events = run_script(state, server, &sql, grammar).await;
    assert!(of_type(&events, "statementFailed").is_empty(), "{sql}\n{events:#?}");
    let finished = of_type(&events, "setFinished");
    assert_eq!(finished[0]["total"], 5, "{grammar:?}");
    let first = first_rows(state, &events)[0][0].clone();
    assert_eq!(first, Some(order[5].to_string()), "{grammar:?}\n{sql}");

    run_script(state, server, &format!("drop table {name};"), grammar).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_table_tab_pages_and_filters_on_sqlite() {
    use paboable_core::config::{ConnectionConfig, Target};
    use paboable_driver_sqlite::SqliteDriver;
    let state = Arc::new(AppState::new(DriverRegistry::new().with(Arc::new(SqliteDriver))));
    let driver = state.registry.get("sqlite").cloned().unwrap();
    let config = ConnectionConfig {
        engine: "sqlite".into(),
        target: Target::File {
            path: ":memory:".into(),
            read_only: false,
        },
        options: Default::default(),
    };
    let first = driver.connect(&config, CallOptions::default()).await.unwrap();
    let server = Arc::new(OpenServer::new(driver, config, first, "test".into()));
    browse_checks(&state, &server, SqlGrammar::Sqlite).await;
    server.close().await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs PostgreSQL at $PABOABLE_POSTGRES_URL"]
async fn a_table_tab_pages_and_filters_on_postgres() {
    let url = std::env::var("PABOABLE_POSTGRES_URL").expect("PABOABLE_POSTGRES_URL is set");
    let config = paboable_core::connection_url::parse(&url).expect("a valid URL");
    let state = Arc::new(AppState::new(DriverRegistry::new().with(Arc::new(PostgresDriver))));
    let driver = state.registry.get("postgres").cloned().unwrap();
    let first = driver.connect(&config, CallOptions::default()).await.unwrap();
    let server = Arc::new(OpenServer::new(driver, config, first, "test".into()));
    browse_checks(&state, &server, SqlGrammar::Postgres).await;
    server.close().await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs SQL Server at $PABOABLE_MSSQL_URL"]
async fn a_table_tab_pages_and_filters_on_sql_server() {
    let url = std::env::var("PABOABLE_MSSQL_URL").expect("PABOABLE_MSSQL_URL is set");
    let config = paboable_core::connection_url::parse(&url).expect("a valid URL");
    let state = Arc::new(AppState::new(DriverRegistry::new().with(Arc::new(MssqlDriver))));
    let driver = state.registry.get("mssql").cloned().unwrap();
    let first = driver.connect(&config, CallOptions::default()).await.unwrap();
    let server = Arc::new(OpenServer::new(driver, config, first, "test".into()));
    browse_checks(&state, &server, SqlGrammar::SqlServer).await;
    server.close().await;
}

/// Completion read through the app's catalog cache from a real catalog
/// (docs/decisions/0007-autocomplete.md).
async fn completion_checks(state: &Arc<AppState>, server: &Arc<OpenServer>, grammar: SqlGrammar, schema: &str) {
    use paboable_core::sql::{completion_context, completions};

    let suffix = std::process::id();
    let orders = format!("pc_orders_{suffix}");
    let customers = format!("pc_customers_{suffix}");
    let (spaced, text_type) = match grammar {
        SqlGrammar::SqlServer => ("[Total Amount]", "nvarchar(50)"),
        SqlGrammar::Postgres | SqlGrammar::Sqlite => ("\"Total Amount\"", "varchar(50)"),
    };
    let setup = format!(
        "create table {customers} (id int primary key, full_name {text_type});\n\
         create table {orders} (id int primary key, customer_id int, {spaced} decimal(10, 2));"
    );
    let events = run_script(state, server, &setup, grammar).await;
    assert!(of_type(&events, "statementFailed").is_empty(), "{events:#?}");

    let labels = |text: String| {
        let session = Arc::clone(server);
        async move {
            let cursor = text.find('|').unwrap_or(text.len());
            let text = text.replacen('|', "", 1);
            let context = completion_context(&text, cursor, grammar).expect("a context");
            let session = session.session(None).await.expect("a session");
            let view = state.catalog.view(7, None, session, &context).await;
            completions(&context, &view, grammar)
                .into_iter()
                .map(|item| (item.label, item.apply))
                .collect::<Vec<_>>()
        }
    };

    let tables = labels("select * from |".to_owned()).await;
    for name in [&orders, &customers] {
        assert!(tables.iter().any(|(label, _)| label == name), "{grammar:?} {tables:?}");
    }
    let qualified = labels(format!("select * from {schema}.|")).await;
    assert!(
        qualified.iter().any(|(label, _)| *label == orders),
        "{grammar:?} {qualified:?}"
    );

    let aliased = labels(format!("select o.| from {orders} o")).await;
    let names: Vec<&str> = aliased.iter().map(|(label, _)| label.as_str()).collect();
    assert_eq!(names, ["id", "customer_id", "Total Amount"], "{grammar:?}");
    assert_eq!(aliased[2].1, spaced, "{grammar:?}");

    let joined = labels(format!(
        "select | from {orders} o join {customers} c on c.id = o.customer_id"
    ))
    .await;
    let names: Vec<&str> = joined.iter().map(|(label, _)| label.as_str()).collect();
    assert_eq!(
        names,
        ["id", "customer_id", "Total Amount", "full_name", "o", "c"],
        "{grammar:?}"
    );

    // A table made after the first lookup shows once the catalog is forgotten, as after a run.
    let later = format!("pc_later_{suffix}");
    run_script(state, server, &format!("create table {later} (x int);"), grammar).await;
    let before = labels("select * from |".to_owned()).await;
    assert!(
        !before.iter().any(|(label, _)| *label == later),
        "{grammar:?}: the cache keeps the first read"
    );
    state.catalog.forget(7);
    let after = labels("select * from |".to_owned()).await;
    assert!(after.iter().any(|(label, _)| *label == later), "{grammar:?} {after:?}");

    run_script(
        state,
        server,
        &format!("drop table {orders};\ndrop table {customers};\ndrop table {later};"),
        grammar,
    )
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn completion_reads_the_sqlite_catalog() {
    use paboable_core::config::{ConnectionConfig, Target};
    use paboable_driver_sqlite::SqliteDriver;
    let state = Arc::new(AppState::new(DriverRegistry::new().with(Arc::new(SqliteDriver))));
    let driver = state.registry.get("sqlite").cloned().unwrap();
    let config = ConnectionConfig {
        engine: "sqlite".into(),
        target: Target::File {
            path: ":memory:".into(),
            read_only: false,
        },
        options: Default::default(),
    };
    let first = driver.connect(&config, CallOptions::default()).await.unwrap();
    let server = Arc::new(OpenServer::new(driver, config, first, "test".into()));
    completion_checks(&state, &server, SqlGrammar::Sqlite, "main").await;
    server.close().await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs PostgreSQL at $PABOABLE_POSTGRES_URL"]
async fn completion_reads_the_postgres_catalog() {
    let url = std::env::var("PABOABLE_POSTGRES_URL").expect("PABOABLE_POSTGRES_URL is set");
    let config = paboable_core::connection_url::parse(&url).expect("a valid URL");
    let state = Arc::new(AppState::new(DriverRegistry::new().with(Arc::new(PostgresDriver))));
    let driver = state.registry.get("postgres").cloned().unwrap();
    let first = driver.connect(&config, CallOptions::default()).await.unwrap();
    let server = Arc::new(OpenServer::new(driver, config, first, "test".into()));
    completion_checks(&state, &server, SqlGrammar::Postgres, "public").await;
    server.close().await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs SQL Server at $PABOABLE_MSSQL_URL"]
async fn completion_reads_the_sql_server_catalog() {
    let url = std::env::var("PABOABLE_MSSQL_URL").expect("PABOABLE_MSSQL_URL is set");
    let config = paboable_core::connection_url::parse(&url).expect("a valid URL");
    let state = Arc::new(AppState::new(DriverRegistry::new().with(Arc::new(MssqlDriver))));
    let driver = state.registry.get("mssql").cloned().unwrap();
    let first = driver.connect(&config, CallOptions::default()).await.unwrap();
    let server = Arc::new(OpenServer::new(driver, config, first, "test".into()));
    completion_checks(&state, &server, SqlGrammar::SqlServer, "dbo").await;
    server.close().await;
}

/// A table tab's edits on each engine: from the page's rows to the statements
/// to the saved table, and a save that names a gone row changing nothing
/// (docs/decisions/0010-editing-rows.md).
async fn edit_checks(state: &Arc<AppState>, server: &Arc<OpenServer>, grammar: SqlGrammar) {
    use crate::commands::{CellEdit, EditRequest, RowUpdate, edit_statements};
    use paboable_core::ErrorCode;
    use paboable_core::catalog::TableRef;
    use paboable_core::sql::NewValue;

    let name = format!("paboable_edit_{}", std::process::id());
    let text_type = if grammar == SqlGrammar::SqlServer {
        "nvarchar(50)"
    } else {
        "varchar(50)"
    };
    let setup = format!(
        "create table {name} (id int primary key, name {text_type}, qty int);\n\
         insert into {name} values (1, 'a', 1), (2, 'b', 2), (3, 'c', 3);"
    );
    let events = run_script(state, server, &setup, grammar).await;
    assert!(of_type(&events, "statementFailed").is_empty(), "{events:#?}");
    let page = run_script(state, server, &format!("select * from {name} order by id"), grammar).await;
    let result = of_type(&page, "setStarted")[0]["result"].as_u64().unwrap();

    let text = |value: &str| NewValue::Text(value.to_owned());
    let request = |updates: Vec<RowUpdate>, deletes: Vec<u64>, inserts: Vec<Vec<CellEdit>>| EditRequest {
        result,
        table: TableRef {
            schema: None,
            name: name.clone(),
        },
        key: vec!["id".into()],
        updates,
        deletes,
        inserts,
    };
    let session = server.session(None).await.unwrap();
    let rows = |sql: String| async move {
        let events = run_script(state, server, &sql, grammar).await;
        let result = of_type(&events, "setStarted")[0]["result"].as_u64().unwrap();
        let store = state.result(result).unwrap();
        (0..store.state().total_rows)
            .map(|row| store.row_texts(row).unwrap_or_default())
            .collect::<Vec<_>>()
    };
    let cells = |values: &[Option<&str>]| values.iter().map(|value| value.map(str::to_owned)).collect::<Vec<_>>();

    let saved = request(
        vec![RowUpdate {
            row: 0,
            cells: vec![
                CellEdit {
                    column: 1,
                    value: text("A'1 chào"),
                },
                CellEdit {
                    column: 2,
                    value: NewValue::Null,
                },
            ],
        }],
        vec![1],
        vec![vec![
            CellEdit {
                column: 0,
                value: text("4"),
            },
            CellEdit {
                column: 1,
                value: text("d"),
            },
            CellEdit {
                column: 2,
                value: NewValue::Default,
            },
        ]],
    );
    let statements = edit_statements(state, grammar, &saved).unwrap();
    assert_eq!(statements.len(), 3, "{statements:#?}");
    session
        .apply_edits(&statements, CallOptions::default())
        .await
        .unwrap_or_else(|error| panic!("{grammar:?}: {error:?}\n{statements:#?}"));
    assert_eq!(
        rows(format!("select id, name, qty from {name} order by id")).await,
        [
            cells(&[Some("1"), Some("A'1 chào"), None]),
            cells(&[Some("3"), Some("c"), Some("3")]),
            cells(&[Some("4"), Some("d"), None]),
        ],
        "{grammar:?}"
    );

    // Row 1 of the page (id 2) is gone now: the update before it is rolled back too.
    let gone = request(
        vec![
            RowUpdate {
                row: 2,
                cells: vec![CellEdit {
                    column: 1,
                    value: text("changed"),
                }],
            },
            RowUpdate {
                row: 1,
                cells: vec![CellEdit {
                    column: 1,
                    value: text("x"),
                }],
            },
        ],
        Vec::new(),
        Vec::new(),
    );
    let statements = edit_statements(state, grammar, &gone).unwrap();
    let error = session.apply_edits(&statements, CallOptions::default()).await.err();
    assert_eq!(error.map(|error| error.code), Some(ErrorCode::RowGone), "{grammar:?}");
    assert_eq!(
        rows(format!("select name from {name} where id = 3")).await,
        [cells(&[Some("c")])],
        "{grammar:?}: nothing was saved"
    );
    // The session is out of the transaction: the next run works.
    let after = run_script(state, server, &format!("drop table {name};"), grammar).await;
    assert!(of_type(&after, "statementFailed").is_empty(), "{after:#?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn edits_save_on_sqlite() {
    use paboable_core::config::{ConnectionConfig, Target};
    use paboable_driver_sqlite::SqliteDriver;
    let state = Arc::new(AppState::new(DriverRegistry::new().with(Arc::new(SqliteDriver))));
    let driver = state.registry.get("sqlite").cloned().unwrap();
    let config = ConnectionConfig {
        engine: "sqlite".into(),
        target: Target::File {
            path: ":memory:".into(),
            read_only: false,
        },
        options: Default::default(),
    };
    let first = driver.connect(&config, CallOptions::default()).await.unwrap();
    let server = Arc::new(OpenServer::new(driver, config, first, "test".into()));
    edit_checks(&state, &server, SqlGrammar::Sqlite).await;
    server.close().await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs PostgreSQL at $PABOABLE_POSTGRES_URL"]
async fn edits_save_on_postgres() {
    let url = std::env::var("PABOABLE_POSTGRES_URL").expect("PABOABLE_POSTGRES_URL is set");
    let config = paboable_core::connection_url::parse(&url).expect("a valid URL");
    let state = Arc::new(AppState::new(DriverRegistry::new().with(Arc::new(PostgresDriver))));
    let driver = state.registry.get("postgres").cloned().unwrap();
    let first = driver.connect(&config, CallOptions::default()).await.unwrap();
    let server = Arc::new(OpenServer::new(driver, config, first, "test".into()));
    edit_checks(&state, &server, SqlGrammar::Postgres).await;
    server.close().await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs SQL Server at $PABOABLE_MSSQL_URL"]
async fn edits_save_on_sql_server() {
    let url = std::env::var("PABOABLE_MSSQL_URL").expect("PABOABLE_MSSQL_URL is set");
    let config = paboable_core::connection_url::parse(&url).expect("a valid URL");
    let state = Arc::new(AppState::new(DriverRegistry::new().with(Arc::new(MssqlDriver))));
    let driver = state.registry.get("mssql").cloned().unwrap();
    let first = driver.connect(&config, CallOptions::default()).await.unwrap();
    let server = Arc::new(OpenServer::new(driver, config, first, "test".into()));
    edit_checks(&state, &server, SqlGrammar::SqlServer).await;
    server.close().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_test_connection_reports_the_server_and_keeps_nothing_open() {
    use paboable_core::config::{ConnectionConfig, Target};
    use paboable_driver_sqlite::SqliteDriver;
    let state = AppState::new(DriverRegistry::new().with(Arc::new(SqliteDriver)));
    let config = ConnectionConfig {
        engine: "sqlite".into(),
        target: Target::File {
            path: ":memory:".into(),
            read_only: false,
        },
        options: Default::default(),
    };
    let tested = crate::commands::try_connection(&state.registry, &config).await.unwrap();
    let json = serde_json::to_value(&tested).unwrap();
    assert!(
        json["serverVersion"]
            .as_str()
            .is_some_and(|version| !version.is_empty())
    );
    // SQLite has no databases to count.
    assert!(json["databases"].is_null());
    assert!(state.server(1).is_err(), "a test registers no server");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_test_connection_fails_with_the_drivers_code() {
    use paboable_core::ErrorCode;
    use paboable_core::config::{ConnectionConfig, Secret, Target, TlsMode};
    let state = AppState::new(DriverRegistry::new().with(Arc::new(PostgresDriver)));
    let mut config = ConnectionConfig {
        engine: "postgres".into(),
        // Port 1 on this machine refuses at once.
        target: Target::Network {
            host: "127.0.0.1".into(),
            port: Some(1),
            user: Some("nobody".into()),
            password: Secret::new("x"),
            database: None,
            tls: TlsMode::Disable,
        },
        options: Default::default(),
    };
    let refused = crate::commands::try_connection(&state.registry, &config)
        .await
        .unwrap_err();
    assert_eq!(refused.code, ErrorCode::Connect);

    config.engine = "nosuch".into();
    let unknown = crate::commands::try_connection(&state.registry, &config)
        .await
        .unwrap_err();
    assert_eq!(unknown.code, ErrorCode::Unsupported);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_forms_database_list_says_why_it_has_none() {
    use paboable_core::ErrorCode;
    use paboable_core::config::{ConnectionConfig, Secret, Target, TlsMode};
    use paboable_driver_sqlite::SqliteDriver;
    let state = AppState::new(
        DriverRegistry::new()
            .with(Arc::new(PostgresDriver))
            .with(Arc::new(SqliteDriver)),
    );
    // A server that refuses: the picker shows the driver's code, not an empty list.
    let refused = ConnectionConfig {
        engine: "postgres".into(),
        target: Target::Network {
            host: "127.0.0.1".into(),
            port: Some(1),
            user: Some("nobody".into()),
            password: Secret::new("x"),
            database: None,
            tls: TlsMode::Disable,
        },
        options: Default::default(),
    };
    let error = crate::commands::form_databases(&state.registry, &refused)
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Connect);

    // An engine without databases connects, says so, and keeps nothing open.
    let file = ConnectionConfig {
        engine: "sqlite".into(),
        target: Target::File {
            path: ":memory:".into(),
            read_only: false,
        },
        options: Default::default(),
    };
    let error = crate::commands::form_databases(&state.registry, &file)
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Unsupported);
    assert!(state.server(1).is_err(), "listing registers no server");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_script_file_saves_back_in_the_encoding_it_was_read_in() {
    use paboable_core::text_file::{LineEnding, TextEncoding};
    let folder = std::env::temp_dir().join(format!("paboable-script-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    let path = folder.join("tạo bảng.sql");
    let path_text = path.to_string_lossy().into_owned();
    // SSMS's UTF-16 with CRLF.
    let original: Vec<u8> = [0xFF, 0xFE]
        .into_iter()
        .chain("select N'ký'\r\nGO\r\n".encode_utf16().flat_map(u16::to_le_bytes))
        .collect();
    std::fs::write(&path, &original).unwrap();

    let read = crate::commands::read_text_file(path_text.clone()).await.unwrap();
    assert_eq!(read.text, "select N'ký'\nGO\n");
    assert_eq!(
        (read.encoding, read.line_ending),
        (TextEncoding::Utf16Le, LineEnding::Crlf)
    );

    crate::commands::write_text_file(
        path_text.clone(),
        format!("{}select 2\n", read.text),
        read.encoding,
        read.line_ending,
    )
    .await
    .unwrap();
    let written = std::fs::read(&path).unwrap();
    let expected: Vec<u8> = [0xFF, 0xFE]
        .into_iter()
        .chain(
            "select N'ký'\r\nGO\r\nselect 2\r\n"
                .encode_utf16()
                .flat_map(u16::to_le_bytes),
        )
        .collect();
    assert_eq!(written, expected);
    assert!(
        !folder.join("tạo bảng.paboable-save.tmp").exists(),
        "no temporary file is left"
    );

    let missing = crate::commands::read_text_file(folder.join("gone.sql").to_string_lossy().into_owned())
        .await
        .unwrap_err();
    assert_eq!(missing.code, paboable_core::ErrorCode::Io);
    let _ = std::fs::remove_dir_all(&folder);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_result_names_its_table_and_brings_its_where_as_filters() {
    use paboable_core::config::{ConnectionConfig, Target};
    use paboable_driver_sqlite::SqliteDriver;
    let state = Arc::new(AppState::new(DriverRegistry::new().with(Arc::new(SqliteDriver))));
    let driver = state.registry.get("sqlite").cloned().unwrap();
    let config = ConnectionConfig {
        engine: "sqlite".into(),
        target: Target::File {
            path: ":memory:".into(),
            read_only: false,
        },
        options: Default::default(),
    };
    let first = driver.connect(&config, CallOptions::default()).await.unwrap();
    let server = Arc::new(OpenServer::new(driver, config, first, "test".into()));
    let events = run_script(
        &state,
        &server,
        "create table customer (id integer primary key, city text);\n\
         insert into customer (city) values ('Miami'), ('Hà Nội');\n\
         select * from customer c where c.city = 'Hà Nội';\n\
         select city, count(*) from customer group by city;",
        SqlGrammar::Sqlite,
    )
    .await;
    let sets: Vec<u64> = of_type(&events, "setStarted")
        .iter()
        .map(|event| event["result"].as_u64().unwrap())
        .collect();
    assert_eq!(sets.len(), 2);

    let source = crate::commands::source_of(&state, sets[0]).unwrap();
    let json = serde_json::to_value(&source).unwrap();
    assert_eq!(json["table"]["name"], "customer");
    assert_eq!(json["filters"][0]["column"], "city");
    assert_eq!(json["filters"][0]["op"], "equals");
    assert_eq!(json["filters"][0]["value"], "Hà Nội");
    assert_eq!(json["whereLeft"], false);
    // A grouped result is no table's rows.
    assert!(crate::commands::source_of(&state, sets[1]).is_none());
    server.close().await;
}

/// A SQLite file in the temp folder, removed at the end, as one open server.
async fn sqlite_server(state: &Arc<AppState>, name: &str) -> (Arc<OpenServer>, std::path::PathBuf) {
    use paboable_core::config::{ConnectionConfig, Target};
    let folder = std::env::temp_dir().join(format!("paboable-jobs-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).unwrap();
    let driver = state.registry.get("sqlite").cloned().unwrap();
    let config = ConnectionConfig {
        engine: "sqlite".into(),
        target: Target::File {
            path: folder.join("data.db"),
            read_only: false,
        },
        options: Default::default(),
    };
    let first = driver.connect(&config, CallOptions::default()).await.unwrap();
    (Arc::new(OpenServer::new(driver, config, first, "test".into())), folder)
}

/// The events of a job, collected until it ends.
fn job_channel() -> (Channel<crate::jobs::JobEvent>, Arc<Mutex<Vec<Json>>>) {
    let events = Arc::new(Mutex::new(Vec::new()));
    let collected = Arc::clone(&events);
    let channel = Channel::new(move |body| {
        if let InvokeResponseBody::Json(json) = body {
            collected
                .lock()
                .unwrap()
                .push(serde_json::from_str::<Json>(&json).unwrap());
        }
        Ok(())
    });
    (channel, events)
}

/// The rows of a SQLite table numbered from 1 with no gaps: one past the last rowid, less one.
async fn count(server: &Arc<OpenServer>, table: &str) -> i64 {
    let session = server.session(None).await.unwrap();
    let table = paboable_core::catalog::TableRef {
        schema: None,
        name: table.into(),
    };
    crate::jobs::key_start(&session, SqlGrammar::Sqlite, &table, "rowid")
        .await
        .unwrap()
        - 1
}

#[tokio::test(flavor = "multi_thread")]
async fn import_generate_and_export_move_rows_and_a_cancel_leaves_nothing() {
    use paboable_core::catalog::TableRef;
    use paboable_core::export::{ExportFormat, ExportOptions};
    use paboable_core::import::NewColumn;
    use paboable_driver_sqlite::SqliteDriver;
    let state = Arc::new(AppState::new(DriverRegistry::new().with(Arc::new(SqliteDriver))));
    let (server, folder) = sqlite_server(&state, "all").await;
    run_script(
        &state,
        &server,
        "create table khach (id integer primary key, ma text not null, ten text, so_du real)",
        SqlGrammar::Sqlite,
    )
    .await;
    let session = server.session(None).await.unwrap();
    let khach = TableRef {
        schema: None,
        name: "khach".into(),
    };

    // Import into the table: fields by name, a field it lacks left out, codes keep their zeros.
    let csv = "Ma;TEN;ghi_chu;so_du\r\n01121212;\"Nguyễn \"\"An\"\"\";x;12.5\r\n02;Bình;;\r\n";
    let columns = session.fetch_columns(&khach, CallOptions::default()).await.unwrap();
    let (channel, events) = job_channel();
    let cancel = CancellationToken::new();
    let id = state.add_run(cancel.clone());
    crate::jobs::run_import(
        Arc::clone(&state),
        crate::jobs::ImportJob {
            id,
            session: Arc::clone(&session),
            grammar: SqlGrammar::Sqlite,
            transactional: true,
            text: csv.into(),
            options: crate::jobs::CsvOptions {
                delimiter: ';',
                header: true,
                empty_is_null: true,
            },
            target: crate::jobs::ImportTarget::Existing { table: khach.clone() },
            columns,
            cancel,
            channel,
        },
    )
    .await;
    let last = events.lock().unwrap().last().cloned().unwrap();
    assert_eq!(last["type"], "finished", "{last}");
    assert_eq!(last["rows"], 2);
    assert_eq!(count(&server, "khach").await, 2);

    // Into a new table: it is made first, in the same transaction.
    let (channel, events) = job_channel();
    let cancel = CancellationToken::new();
    let id = state.add_run(cancel.clone());
    crate::jobs::run_import(
        Arc::clone(&state),
        crate::jobs::ImportJob {
            id,
            session: Arc::clone(&session),
            grammar: SqlGrammar::Sqlite,
            transactional: true,
            text: "a,b\n1,x\n2,y\n3,z\n".into(),
            options: crate::jobs::CsvOptions {
                delimiter: ',',
                header: true,
                empty_is_null: true,
            },
            target: crate::jobs::ImportTarget::New {
                table: TableRef {
                    schema: None,
                    name: "moi".into(),
                },
                columns: vec![
                    NewColumn {
                        name: "a".into(),
                        type_name: "INTEGER".into(),
                    },
                    NewColumn {
                        name: "b".into(),
                        type_name: "TEXT".into(),
                    },
                ],
            },
            columns: Vec::new(),
            cancel,
            channel,
        },
    )
    .await;
    assert_eq!(events.lock().unwrap().last().unwrap()["rows"], 3);
    assert_eq!(count(&server, "moi").await, 3);

    // A cancelled import rolls back: the new table is not there.
    let (channel, events) = job_channel();
    let cancel = CancellationToken::new();
    cancel.cancel();
    let id = state.add_run(cancel.clone());
    crate::jobs::run_import(
        Arc::clone(&state),
        crate::jobs::ImportJob {
            id,
            session: Arc::clone(&session),
            grammar: SqlGrammar::Sqlite,
            transactional: true,
            text: "a\n1\n".into(),
            options: crate::jobs::CsvOptions {
                delimiter: ',',
                header: true,
                empty_is_null: true,
            },
            target: crate::jobs::ImportTarget::New {
                table: TableRef {
                    schema: None,
                    name: "bo_di".into(),
                },
                columns: vec![NewColumn {
                    name: "a".into(),
                    type_name: "INTEGER".into(),
                }],
            },
            columns: Vec::new(),
            cancel,
            channel,
        },
    )
    .await;
    assert_eq!(events.lock().unwrap().last().unwrap()["type"], "cancelled");
    let objects = session.list_objects(None, CallOptions::default()).await.unwrap();
    assert!(!objects.iter().any(|object| object.name == "bo_di"));

    // Generate Rows: 1,234 rows in batches, the key counted from the next free number.
    let columns = session.fetch_columns(&khach, CallOptions::default()).await.unwrap();
    let (channel, events) = job_channel();
    let cancel = CancellationToken::new();
    let id = state.add_run(cancel.clone());
    crate::jobs::run_generate(
        Arc::clone(&state),
        crate::jobs::GenerateJob {
            id,
            session: Arc::clone(&session),
            grammar: SqlGrammar::Sqlite,
            transactional: true,
            table: khach.clone(),
            columns,
            count: 1234,
            seed: 42,
            key_start: 3,
            cancel,
            channel,
        },
    )
    .await;
    assert_eq!(events.lock().unwrap().last().unwrap()["rows"], 1234);
    assert_eq!(count(&server, "khach").await, 1236);

    // Export every row to CSV; the file takes its name only when done.
    let target = folder.join("khach.csv");
    let (channel, events) = job_channel();
    let cancel = CancellationToken::new();
    let id = state.add_run(cancel.clone());
    crate::export::run(
        Arc::clone(&state),
        crate::export::Export {
            id,
            session: Arc::clone(&session),
            statement: "SELECT * FROM \"khach\" ORDER BY \"id\"".into(),
            grammar: SqlGrammar::Sqlite,
            table: khach.clone(),
            options: ExportOptions {
                format: ExportFormat::Csv,
                delimiter: ',',
                header: true,
                bom: true,
            },
            path: target.clone(),
            cancel,
            channel,
        },
    )
    .await;
    assert_eq!(events.lock().unwrap().last().unwrap()["rows"], 1236);
    let written = std::fs::read_to_string(&target).unwrap();
    assert!(written.starts_with("\u{feff}id,ma,ten,so_du\r\n"), "{}", &written[..60]);
    assert!(
        written.contains("01121212,\"Nguyễn \"\"An\"\"\",12.5"),
        "the import's row, quoted again"
    );
    assert_eq!(written.lines().count(), 1237);
    assert!(!folder.join("khach.csv.part").exists());

    // A cancelled export leaves no file at all.
    let stopped = folder.join("stopped.json");
    let (channel, events) = job_channel();
    let cancel = CancellationToken::new();
    cancel.cancel();
    let id = state.add_run(cancel.clone());
    crate::export::run(
        Arc::clone(&state),
        crate::export::Export {
            id,
            session,
            statement: "SELECT * FROM \"khach\"".into(),
            grammar: SqlGrammar::Sqlite,
            table: khach,
            options: ExportOptions {
                format: ExportFormat::Json,
                delimiter: ',',
                header: true,
                bom: false,
            },
            path: stopped.clone(),
            cancel,
            channel,
        },
    )
    .await;
    assert_eq!(events.lock().unwrap().last().unwrap()["type"], "cancelled");
    assert!(!stopped.exists() && !folder.join("stopped.json.part").exists());

    server.close().await;
    let _ = std::fs::remove_dir_all(&folder);
}

// --- The data folder (docs/decisions/0019-data-folder.md) -------------------

fn data_folder(name: &str) -> crate::data::DataFolder {
    let path = std::env::temp_dir().join(format!("paboable-data-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).unwrap();
    crate::data::DataFolder { path, beside_app: true }
}

#[test]
fn pages_and_prefs_come_back_as_written_and_only_for_their_key() {
    let folder = data_folder("pages");
    assert_eq!(folder.prefs(), None);
    folder
        .write_prefs(&serde_json::json!({ "settings": { "theme": "dark" } }))
        .unwrap();
    assert_eq!(folder.prefs().unwrap()["settings"]["theme"], "dark");

    let kept = serde_json::json!({ "pages": [{ "number": 1, "text": "select 1", "cursor": 3 }], "active": 0, "at": 1 });
    folder.write_pages("saved:abc", kept.clone()).unwrap();
    assert_eq!(folder.pages("saved:abc"), Some(kept));
    assert_eq!(folder.pages("saved:other"), None);
    // A long file path is a key like any other.
    let long = format!("file:{}", r"D:\folder\".repeat(60));
    folder.write_pages(&long, serde_json::json!({ "pages": [] })).unwrap();
    assert!(folder.pages(&long).is_some());
    // Nothing is left half written.
    let leftovers = std::fs::read_dir(folder.path.join("pages"))
        .unwrap()
        .filter(|entry| entry.as_ref().unwrap().path().extension().unwrap() == "tmp")
        .count();
    assert_eq!(leftovers, 0);
    let _ = std::fs::remove_dir_all(&folder.path);
}

#[test]
fn only_the_forty_connections_written_last_keep_their_pages() {
    let folder = data_folder("prune");
    for index in 0..45 {
        folder
            .write_pages(&format!("saved:{index}"), serde_json::json!({ "pages": [] }))
            .unwrap();
        // File times need to differ for the order to show.
        std::thread::sleep(std::time::Duration::from_millis(15));
    }
    assert_eq!(std::fs::read_dir(folder.path.join("pages")).unwrap().count(), 40);
    assert!(folder.pages("saved:44").is_some());
    assert!(folder.pages("saved:0").is_none());
    let _ = std::fs::remove_dir_all(&folder.path);
}

#[test]
fn a_new_data_folder_takes_what_the_old_one_kept_once_and_leaves_it_there() {
    let old = data_folder("old");
    std::fs::write(old.path.join("connections.json"), b"[1]").unwrap();
    std::fs::write(old.path.join("history.db"), b"db").unwrap();
    let folder = data_folder("new");
    folder.carry_over(&old.path);
    assert_eq!(std::fs::read(folder.path.join("connections.json")).unwrap(), b"[1]");
    assert_eq!(std::fs::read(folder.path.join("history.db")).unwrap(), b"db");
    assert!(old.path.join("connections.json").exists());
    // Once the new folder has its own, the old one is not read again.
    std::fs::write(old.path.join("connections.json"), b"[2]").unwrap();
    folder.carry_over(&old.path);
    assert_eq!(std::fs::read(folder.path.join("connections.json")).unwrap(), b"[1]");
    // The folder in the profile is the old one itself: nothing to carry.
    let profile = crate::data::DataFolder {
        path: old.path.clone(),
        beside_app: false,
    };
    profile.carry_over(&folder.path);
    assert_eq!(std::fs::read(old.path.join("connections.json")).unwrap(), b"[2]");
    let _ = std::fs::remove_dir_all(&old.path);
    let _ = std::fs::remove_dir_all(&folder.path);
}

#[test]
fn the_old_webview_storage_is_read_once_then_the_new_folder_is_used() {
    let folder = data_folder("webview");
    let old = folder.path.join("old-EBWebView");
    // No older version: the new folder from the start.
    assert_eq!(
        crate::data::webview_for_launch(&folder, &old),
        (Some(folder.webview()), false)
    );
    // An older version's storage, not moved yet: this launch reads it.
    std::fs::create_dir_all(&old).unwrap();
    assert_eq!(crate::data::webview_for_launch(&folder, &old), (None, true));
    // Moved: the preferences file says so.
    folder.write_prefs(&serde_json::json!({})).unwrap();
    assert_eq!(
        crate::data::webview_for_launch(&folder, &old),
        (Some(folder.webview()), false)
    );
    let script = folder.startup_script(false);
    assert!(script.starts_with("window.__PABOABLE__ = {"), "{script}");
    assert!(script.contains("\"besideApp\":true"), "{script}");
    let _ = std::fs::remove_dir_all(&folder.path);
}

#[test]
fn a_window_is_on_screen_when_any_corner_is() {
    use tauri::{PhysicalPosition, PhysicalSize};
    let screen = PhysicalPosition::new(0, 0);
    let size = PhysicalSize::new(1920u32, 1080u32);
    let window = PhysicalSize::new(1000u32, 700u32);
    assert!(crate::window::on_screen(&screen, &size, 150, 120, window));
    assert!(crate::window::on_screen(&screen, &size, -900, -600, window));
    assert!(!crate::window::on_screen(&screen, &size, -30_000, -30_000, window));
    assert!(!crate::window::on_screen(&screen, &size, 1920, 0, window));
    let maximized = crate::window::Place {
        x: -8,
        y: -8,
        width: 1000,
        height: 700,
        maximized: true,
        prev_x: 150,
        prev_y: 120,
    };
    assert_eq!(maximized.restore_at(), (150, 120));
}

// --- Connect and save from the connect form ----------------------------------

#[tokio::test]
async fn the_connect_form_keeps_a_connection_only_once_it_connects() {
    use paboable_core::config::{ConnectionConfig, Target};
    use paboable_storage::SavedConnection;
    let state = Arc::new(AppState::new(
        DriverRegistry::new().with(Arc::new(paboable_driver_sqlite::SqliteDriver)),
    ));
    let folder = std::env::temp_dir().join(format!("paboable-keep-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).unwrap();
    let config = |path: std::path::PathBuf| ConnectionConfig {
        engine: "sqlite".into(),
        target: Target::File { path, read_only: true },
        options: Default::default(),
    };
    let saved = |name: &str| SavedConnection {
        id: "kept-1".into(),
        name: name.into(),
        config: config(folder.join("a.db")),
        save_password: false,
        last_used: None,
        profile_id: None,
        color: None,
    };

    // A server that does not answer (here, a read-only file that is not there): nothing is kept.
    let kept = std::sync::atomic::AtomicUsize::new(0);
    let missing = crate::commands::open_then_keep(&state, config(folder.join("not-there.db")), "x".into(), || {
        kept.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(saved("never"))
    })
    .await;
    assert!(missing.is_err());
    assert_eq!(kept.load(std::sync::atomic::Ordering::SeqCst), 0);

    // It connects: then it is kept, and the page names it as saved.
    rusqlite_file(&folder.join("a.db"));
    let opened = crate::commands::open_then_keep(&state, config(folder.join("a.db")), "x".into(), || Ok(saved("Shop")))
        .await
        .unwrap();
    assert_eq!(opened.saved_id.as_deref(), Some("kept-1"));
    assert_eq!(opened.title, "Shop");
    assert!(state.server(opened.id).is_ok());

    // Keeping fails: the connection does not stay open behind the error.
    let failed = crate::commands::open_then_keep(&state, config(folder.join("a.db")), "x".into(), || {
        Err(paboable_core::DriverError::config("a saved connection needs a name"))
    })
    .await;
    assert!(failed.is_err());
    // Ids count up: the one it opened came next, and is gone; the kept one stays.
    assert!(state.server(opened.id + 1).is_err());
    assert!(state.server(opened.id).is_ok());
    let _ = std::fs::remove_dir_all(&folder);
}

/// An empty SQLite database at `path`.
fn rusqlite_file(path: &std::path::Path) {
    std::fs::write(path, b"").unwrap();
}
