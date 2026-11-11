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
