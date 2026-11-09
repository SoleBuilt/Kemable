//! The driver against a small HTTP server in the test that answers as
//! Cloudflare's API documents (`/d1/database` and `/raw`), and against the
//! real API when `$PABOABLE_D1_ACCOUNT`, `$PABOABLE_D1_TOKEN` and
//! `$PABOABLE_D1_DATABASE` are set.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use paboable_core::catalog::{ColumnMeta, ObjectKind, TableRef};
use paboable_core::config::{ConnectionConfig, Secret, Target};
use paboable_core::driver::Driver;
use paboable_core::sink::ResultSink;
use paboable_core::value::{Value, ValueKind};
use paboable_core::{CallOptions, ErrorCode};
use paboable_store::ResultStore;
use serde_json::{Value as Json, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

use crate::api::sql_offset;
use crate::{D1Driver, value};

const ACCOUNT: &str = "0123456789abcdef0123456789abcdef";
const SHOP: &str = "11111111-2222-3333-4444-555555555555";

/// What the server saw: each request's method, path and body.
type Seen = Arc<Mutex<Vec<(String, String, Json)>>>;

fn envelope(result: Json) -> Json {
    json!({ "success": true, "errors": [], "messages": [], "result": result })
}

fn failure(code: i64, message: &str) -> Json {
    json!({ "success": false, "errors": [{ "code": code, "message": message }], "messages": [], "result": null })
}

fn rows(columns: &[&str], rows: Json, meta: Json) -> Json {
    envelope(json!([{ "results": { "columns": columns, "rows": rows }, "meta": meta, "success": true }]))
}

/// The answer to one request; `None` never answers.
fn answer(method: &str, path: &str, authorized: bool, body: &Json) -> Option<(u16, Json)> {
    if !authorized {
        return Some((401, failure(10000, "Authentication error")));
    }
    let list = format!("/client/v4/accounts/{ACCOUNT}/d1/database");
    if method == "GET" && path.starts_with(&list) {
        let all = [
            json!({ "uuid": SHOP, "name": "shop" }),
            json!({ "uuid": "99999999-0000-0000-0000-000000000000", "name": "logs" }),
        ];
        let wanted = path
            .split("name=")
            .nth(1)
            .map(|name| name.split('&').next().unwrap_or_default().to_owned());
        let found: Vec<Json> = all
            .into_iter()
            .filter(|db| wanted.as_ref().is_none_or(|name| db["name"] == *name))
            .collect();
        let mut reply = envelope(Json::Array(found.clone()));
        reply["result_info"] = json!({ "page": 1, "per_page": 1000, "count": found.len(), "total_count": found.len() });
        return Some((200, reply));
    }
    if method != "POST" || !path.ends_with("/raw") {
        return Some((404, failure(7000, "No route for that URI")));
    }
    if !path.contains(SHOP) {
        return Some((404, failure(7404, "Database not found")));
    }
    let sql = body["sql"].as_str().unwrap_or_default();
    let meta = json!({ "changes": 0, "duration": 0.25, "rows_read": 2, "rows_written": 0 });
    Some(match sql {
        "select * from items" => (
            200,
            rows(
                &["id", "name", "price", "photo", "id"],
                json!([
                    [1, "xin chào", 1.5, [0, 255], 10],
                    [2, null, 2, null, 20],
                    [9_223_372_036_854_775_807i64, "max", -0.0, [], 30]
                ]),
                meta,
            ),
        ),
        "select * from empty" => (200, rows(&["a"], json!([]), meta)),
        "insert into items (name) values ('x')" => (
            200,
            rows(
                &[],
                json!([]),
                json!({ "changes": 1, "duration": 0.5, "rows_read": 0, "rows_written": 1, "changed_db": true }),
            ),
        ),
        "slow" => return None,
        _ if sql.starts_with("select type, name from sqlite_schema") => (
            200,
            rows(
                &["type", "name"],
                json!([["table", "items"], ["view", "cheap"], ["index", "items_name"]]),
                meta,
            ),
        ),
        _ if sql.starts_with("PRAGMA table_info(\"items\")") => (
            200,
            rows(
                &["cid", "name", "type", "notnull", "dflt_value", "pk"],
                json!([[0, "id", "INTEGER", 0, null, 1], [1, "name", "TEXT", 1, "'none'", 0]]),
                meta,
            ),
        ),
        _ if sql.starts_with("select 1 from big") => {
            let many: Vec<Json> = (0..1500).map(|n| json!([n])).collect();
            (200, rows(&["n"], Json::Array(many), meta))
        }
        _ => (
            400,
            failure(
                7500,
                &format!(
                    "D1_ERROR: near \"{}\": syntax error at offset 0: SQLITE_ERROR",
                    sql.split(' ').next().unwrap_or_default()
                ),
            ),
        ),
    })
}

/// A one-request-per-connection HTTP/1.1 server on a loopback port.
async fn serve() -> (String, Seen) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://127.0.0.1:{}/client/v4", listener.local_addr().unwrap().port());
    let seen: Seen = Arc::default();
    let log = Arc::clone(&seen);
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let log = Arc::clone(&log);
            tokio::spawn(async move {
                let mut buffer = Vec::new();
                let mut chunk = [0u8; 4096];
                let head_end = loop {
                    let Ok(read) = socket.read(&mut chunk).await else {
                        return;
                    };
                    if read == 0 {
                        return;
                    }
                    buffer.extend_from_slice(&chunk[..read]);
                    if let Some(at) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
                        break at + 4;
                    }
                };
                let head = String::from_utf8_lossy(&buffer[..head_end]).into_owned();
                let length: usize = head
                    .lines()
                    .find_map(|line| {
                        line.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .map(|value| value.trim().parse().unwrap_or(0))
                    })
                    .unwrap_or(0);
                while buffer.len() < head_end + length {
                    let Ok(read) = socket.read(&mut chunk).await else {
                        return;
                    };
                    if read == 0 {
                        break;
                    }
                    buffer.extend_from_slice(&chunk[..read]);
                }
                let mut words = head.split_whitespace();
                let method = words.next().unwrap_or_default().to_owned();
                let path = words.next().unwrap_or_default().to_owned();
                let authorized = head
                    .lines()
                    .any(|line| line.eq_ignore_ascii_case("authorization: Bearer good-token"));
                let body: Json = serde_json::from_slice(&buffer[head_end..]).unwrap_or(Json::Null);
                log.lock().unwrap().push((method.clone(), path.clone(), body.clone()));
                let Some((status, reply)) = answer(&method, &path, authorized, &body) else {
                    std::future::pending::<()>().await;
                    return;
                };
                let text = reply.to_string();
                let response = format!(
                    "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{text}",
                    text.len()
                );
                let _ = socket.write_all(response.as_bytes()).await;
            });
        }
    });
    (base, seen)
}

fn config(base: &str, token: &str, database: Option<&str>) -> ConnectionConfig {
    let mut options = BTreeMap::from([("api_base".to_owned(), base.to_owned())]);
    if let Some(database) = database {
        options.insert("database".into(), database.into());
    }
    ConnectionConfig {
        engine: "d1".into(),
        target: Target::Http {
            endpoint: ACCOUNT.into(),
            token: Secret::new(token),
        },
        options,
    }
}

fn options() -> CallOptions {
    CallOptions::default().with_timeout(Duration::from_secs(10))
}

#[derive(Default)]
struct Keep {
    cap: Option<u64>,
    sets: Mutex<Vec<(Vec<ColumnMeta>, ResultStore)>>,
    done: Mutex<Vec<Option<u64>>>,
    notices: Mutex<Vec<String>>,
}

impl ResultSink for Keep {
    fn begin_set(&self, columns: Vec<ColumnMeta>) {
        let store = ResultStore::new(columns.clone(), self.cap);
        self.sets.lock().unwrap().push((columns, store));
    }
    fn push_rows(&self, values: &[Value]) -> bool {
        self.sets.lock().unwrap().last().unwrap().1.push_rows(values)
    }
    fn end_set(&self) {}
    fn command_done(&self, rows_affected: Option<u64>) {
        self.done.lock().unwrap().push(rows_affected);
    }
    fn notice(&self, text: &str) {
        self.notices.lock().unwrap().push(text.to_owned());
    }
}

#[tokio::test]
async fn a_wrong_token_is_an_auth_error_and_a_database_is_found_by_name() {
    let (base, seen) = serve().await;
    let refused = D1Driver
        .connect(&config(&base, "bad", Some("shop")), options())
        .await
        .err();
    assert_eq!(refused.as_ref().map(|error| error.code), Some(ErrorCode::Auth));
    assert_eq!(refused.and_then(|error| error.engine_code).as_deref(), Some("10000"));

    let connection = D1Driver
        .connect(&config(&base, "good-token", Some("shop")), options())
        .await
        .unwrap();
    assert_eq!(connection.current_database().as_deref(), Some("shop"));
    let missing = D1Driver
        .connect(&config(&base, "good-token", Some("nope")), options())
        .await
        .err();
    assert_eq!(missing.map(|error| error.code), Some(ErrorCode::NotFound));

    let databases = connection.list_databases(options()).await.unwrap();
    let names: Vec<(&str, bool)> = databases.iter().map(|db| (db.name.as_str(), db.is_current)).collect();
    assert_eq!(names, [("logs", false), ("shop", true)]);
    assert!(
        seen.lock()
            .unwrap()
            .iter()
            .any(|(_, path, _)| path.contains("name=shop"))
    );
}

#[tokio::test]
async fn a_bad_config_is_refused_before_any_request() {
    let (base, seen) = serve().await;
    let cases = [
        config("http://example.com/client/v4", "good-token", None),
        config(&base, "", None),
        ConnectionConfig {
            target: Target::Http {
                endpoint: "not an id!".into(),
                token: Secret::new("good-token"),
            },
            ..config(&base, "good-token", None)
        },
    ];
    for case in cases {
        let code = D1Driver.connect(&case, options()).await.err().map(|error| error.code);
        assert_eq!(code, Some(ErrorCode::ConfigInvalid));
    }
    assert!(seen.lock().unwrap().is_empty());
}

#[tokio::test]
async fn rows_keep_their_order_types_and_repeated_names() {
    let (base, seen) = serve().await;
    let connection = D1Driver
        .connect(&config(&base, "good-token", Some("shop")), options())
        .await
        .unwrap();
    let sink = Arc::new(Keep::default());
    connection
        .execute("select * from items", sink.clone(), options())
        .await
        .unwrap();
    let sets = sink.sets.lock().unwrap();
    let (columns, store) = &sets[0];
    let names: Vec<&str> = columns.iter().map(|column| column.name.as_str()).collect();
    assert_eq!(names, ["id", "name", "price", "photo", "id"]);
    let kinds: Vec<ValueKind> = columns.iter().map(|column| column.kind).collect();
    assert_eq!(
        kinds,
        [
            ValueKind::Int,
            ValueKind::Text,
            ValueKind::Float,
            ValueKind::Bytes,
            ValueKind::Int
        ]
    );
    assert_eq!(store.state().total_rows, 3);
    assert_eq!(
        store.row_texts(0).unwrap_or_default(),
        [
            Some("1".into()),
            Some("xin chào".into()),
            Some("1.5".into()),
            Some("0x00ff".into()),
            Some("10".into())
        ]
    );
    assert_eq!(store.row_texts(1).unwrap_or_default()[1], None);
    assert_eq!(
        store.row_texts(2).unwrap_or_default()[0].as_deref(),
        Some("9223372036854775807")
    );
    assert_eq!(sink.notices.lock().unwrap()[0], "D1: 2 rows read, 0 written, 0.2 ms");
    let raw = seen
        .lock()
        .unwrap()
        .iter()
        .find(|(method, _, _)| method == "POST")
        .cloned()
        .unwrap();
    assert!(raw.1.ends_with(&format!("/accounts/{ACCOUNT}/d1/database/{SHOP}/raw")));
    assert_eq!(raw.2, json!({ "sql": "select * from items" }));
}

#[tokio::test]
async fn a_write_is_done_with_its_changes_and_an_empty_select_still_has_columns() {
    let (base, _) = serve().await;
    let connection = D1Driver
        .connect(&config(&base, "good-token", Some(SHOP)), options())
        .await
        .unwrap();
    let sink = Arc::new(Keep::default());
    connection
        .execute("insert into items (name) values ('x')", sink.clone(), options())
        .await
        .unwrap();
    assert_eq!(*sink.done.lock().unwrap(), [Some(1)]);
    assert!(sink.sets.lock().unwrap().is_empty());

    let sink = Arc::new(Keep::default());
    connection
        .execute("select * from empty", sink.clone(), options())
        .await
        .unwrap();
    let sets = sink.sets.lock().unwrap();
    assert_eq!(sets[0].0[0].name, "a");
    assert_eq!(sets[0].1.state().total_rows, 0);
}

#[tokio::test]
async fn a_sql_error_carries_d1s_code_and_the_offset() {
    let (base, _) = serve().await;
    let connection = D1Driver
        .connect(&config(&base, "good-token", Some("shop")), options())
        .await
        .unwrap();
    let error = connection
        .execute("selec 1", Arc::new(Keep::default()), options())
        .await
        .err()
        .unwrap();
    assert_eq!(error.code, ErrorCode::Query);
    assert_eq!(error.engine_code.as_deref(), Some("7500"));
    assert_eq!(error.position.map(|position| position.byte_offset), Some(0));
    assert_eq!(
        sql_offset("near \"x\": syntax error at offset 17: SQLITE_ERROR"),
        Some(17)
    );
    assert_eq!(sql_offset("no offset"), None);
}

#[tokio::test]
async fn the_row_cap_stops_a_result_and_says_so() {
    let (base, _) = serve().await;
    let connection = D1Driver
        .connect(&config(&base, "good-token", Some("shop")), options())
        .await
        .unwrap();
    let sink = Arc::new(Keep {
        cap: Some(700),
        ..Keep::default()
    });
    let outcome = connection
        .execute("select 1 from big", sink.clone(), options())
        .await
        .unwrap();
    assert!(outcome.limited);
    assert_eq!(sink.sets.lock().unwrap()[0].1.state().total_rows, 700);
}

#[tokio::test]
async fn the_catalog_lists_objects_and_columns_with_the_key() {
    let (base, _) = serve().await;
    let connection = D1Driver
        .connect(&config(&base, "good-token", Some("shop")), options())
        .await
        .unwrap();
    let objects = connection.list_objects(Some("main"), options()).await.unwrap();
    let shown: Vec<(&str, ObjectKind)> = objects
        .iter()
        .map(|object| (object.name.as_str(), object.kind))
        .collect();
    assert_eq!(
        shown,
        [
            ("items", ObjectKind::Table),
            ("cheap", ObjectKind::View),
            ("items_name", ObjectKind::Index)
        ]
    );
    let table = TableRef {
        schema: Some("main".into()),
        name: "items".into(),
    };
    let columns = connection.fetch_columns(&table, options()).await.unwrap();
    assert_eq!(columns[0].primary_key, Some(1));
    assert_eq!(
        (columns[1].nullable, columns[1].default.as_deref()),
        (false, Some("'none'"))
    );
}

#[tokio::test]
async fn the_accounts_own_session_runs_nothing_until_a_database_is_picked() {
    let (base, _) = serve().await;
    let connection = D1Driver
        .connect(&config(&base, "good-token", None), options())
        .await
        .unwrap();
    assert_eq!(connection.current_database(), None);
    let code = connection
        .execute("select 1", Arc::new(Keep::default()), options())
        .await
        .err()
        .map(|error| error.code);
    assert_eq!(code, Some(ErrorCode::ConfigInvalid));
}

#[tokio::test]
async fn stop_closes_the_request() {
    let (base, _) = serve().await;
    let connection = D1Driver
        .connect(&config(&base, "good-token", Some("shop")), options())
        .await
        .unwrap();
    let cancel = CancellationToken::new();
    let stopper = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        stopper.cancel();
    });
    let code = connection
        .execute("slow", Arc::new(Keep::default()), CallOptions::new(cancel))
        .await
        .err()
        .map(|error| error.code);
    assert_eq!(code, Some(ErrorCode::Cancelled));
}

#[test]
fn json_values_map_to_sqlites_kinds() {
    assert_eq!(value(json!(null)), Value::Null);
    assert_eq!(value(json!(-5)), Value::Int(-5));
    assert_eq!(value(json!(18_446_744_073_709_551_615u64)), Value::UInt(u64::MAX));
    assert_eq!(value(json!(0.5)), Value::Float(0.5));
    assert_eq!(value(json!([1, 2])), Value::Bytes(vec![1, 2]));
    assert_eq!(value(json!([1, 300])), Value::Json("[1,300]".into()));
    assert_eq!(value(json!({"a": 1})), Value::Json("{\"a\":1}".into()));
}

#[tokio::test]
#[ignore = "needs D1 at $PABOABLE_D1_ACCOUNT, $PABOABLE_D1_TOKEN and $PABOABLE_D1_DATABASE"]
async fn the_real_api_runs_a_round_trip() {
    let account = std::env::var("PABOABLE_D1_ACCOUNT").expect("PABOABLE_D1_ACCOUNT is set");
    let token = std::env::var("PABOABLE_D1_TOKEN").expect("PABOABLE_D1_TOKEN is set");
    let database = std::env::var("PABOABLE_D1_DATABASE").expect("PABOABLE_D1_DATABASE is set");
    let config = ConnectionConfig {
        engine: "d1".into(),
        target: Target::Http {
            endpoint: account,
            token: Secret::new(token),
        },
        options: BTreeMap::from([("database".to_owned(), database)]),
    };
    let connection = D1Driver
        .connect(&config, options())
        .await
        .expect("the token is accepted");
    let table = format!("paboable_test_{}", std::process::id());
    for statement in [
        format!("create table {table} (id integer primary key, name text, photo blob, price real)"),
        format!("insert into {table} (name, photo, price) values ('xin chào', x'00ff', 1.5)"),
    ] {
        connection
            .execute(&statement, Arc::new(Keep::default()), options())
            .await
            .expect(&statement);
    }
    let sink = Arc::new(Keep::default());
    connection
        .execute(&format!("select * from {table}"), sink.clone(), options())
        .await
        .unwrap();
    {
        let sets = sink.sets.lock().unwrap();
        assert_eq!(
            sets[0].1.row_texts(0).unwrap_or_default(),
            [
                Some("1".into()),
                Some("xin chào".into()),
                Some("0x00ff".into()),
                Some("1.5".into())
            ]
        );
    }
    let error = connection
        .execute("selec 1", Arc::new(Keep::default()), options())
        .await
        .err()
        .unwrap();
    assert_eq!(error.code, ErrorCode::Query);
    connection
        .execute(&format!("drop table {table}"), Arc::new(Keep::default()), options())
        .await
        .unwrap();
}
