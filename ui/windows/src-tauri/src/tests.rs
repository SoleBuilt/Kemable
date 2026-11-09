use std::sync::{Arc, Mutex};

use paboable_core::CallOptions;
use paboable_core::registry::DriverRegistry;
use paboable_core::sql::{SqlGrammar, split};
use paboable_driver_postgres::PostgresDriver;
use serde_json::Value as Json;
use tauri::ipc::{Channel, InvokeResponseBody};
use tokio_util::sync::CancellationToken;

use crate::run::{self, Run, Statement};
use crate::state::{AppState, OpenServer};

/// Runs `script` the way `run_query` does and returns its events as JSON.
async fn run_script(state: &Arc<AppState>, server: &Arc<OpenServer>, script: &str) -> Vec<Json> {
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
    let statements = split(script, SqlGrammar::Postgres)
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
        grammar: SqlGrammar::Postgres,
        statements,
        follow_created_database: true,
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
    let server = Arc::new(OpenServer::new(driver, config, first));
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
    let events = run_script(&state, &server, &script).await;

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
