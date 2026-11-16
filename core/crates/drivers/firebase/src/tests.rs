//! Firebase against a small HTTP server inside the tests: Google's token
//! endpoint, Firestore's REST API and the Realtime Database's.
//! `test-only-key.pem` is a key made for these tests and nothing else.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use kemable_core::catalog::{ColumnMeta, ObjectKind};
use kemable_core::config::{ConnectionConfig, Secret, Target};
use kemable_core::driver::Driver;
use kemable_core::sink::ResultSink;
use kemable_core::value::{Value, ValueKind};
use kemable_core::{CallOptions, ErrorCode};
use ring::signature::{RSA_PKCS1_2048_8192_SHA256, RsaKeyPair, UnparsedPublicKey};
use rustls::pki_types::PrivateKeyDer;
use rustls::pki_types::pem::PemObject;
use serde_json::{Value as Json, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use super::{FIRESTORE, FirebaseDriver, REALTIME, console, values};

const KEY: &str = include_str!("test-only-key.pem");

type Seen = Arc<Mutex<Vec<(String, String, String)>>>;

/// Whether `assertion` is a JWT this key signed, for `audience`.
fn signed_by_the_key(assertion: &str, audience: &str) -> bool {
    let der = PrivateKeyDer::from_pem_slice(KEY.as_bytes()).unwrap();
    let PrivateKeyDer::Pkcs8(der) = der else { return false };
    let pair = RsaKeyPair::from_pkcs8(der.secret_pkcs8_der()).unwrap();
    let public = UnparsedPublicKey::new(&RSA_PKCS1_2048_8192_SHA256, pair.public().as_ref().to_vec());
    let Some((signed, signature)) = assertion.rsplit_once('.') else {
        return false;
    };
    let decode = |text: &str| values::base64_decode(&text.replace('-', "+").replace('_', "/")).unwrap_or_default();
    let claims: Json =
        serde_json::from_slice(&decode(signed.split('.').nth(1).unwrap_or_default())).unwrap_or(Json::Null);
    public.verify(signed.as_bytes(), &decode(signature)).is_ok()
        && claims["aud"] == audience
        && claims["iss"] == "kemable@demo.iam.gserviceaccount.com"
        && claims["scope"].as_str().unwrap_or_default().contains("datastore")
}

fn document(id: &str, fields: Json) -> Json {
    json!({
        "name": format!("projects/demo/databases/(default)/documents/orders/{id}"),
        "fields": fields,
        "createTime": "2026-11-01T00:00:00Z",
        "updateTime": "2026-11-14T01:02:03.456789Z",
    })
}

/// What the server answers: a status and a JSON body.
fn answer(method: &str, target: &str, authorized: bool, body: &str, base: &str) -> (u16, Json) {
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    if path == "/token" {
        let assertion = body
            .split('&')
            .find_map(|pair| pair.strip_prefix("assertion="))
            .unwrap_or_default();
        return if signed_by_the_key(assertion, &format!("{base}/token")) {
            (
                200,
                json!({ "access_token": "good-token", "expires_in": 3600, "token_type": "Bearer" }),
            )
        } else {
            (
                400,
                json!({ "error": "invalid_grant", "error_description": "Invalid JWT Signature." }),
            )
        };
    }
    if path.starts_with("/rtdb") {
        if !query.contains("access_token=good-token") {
            return (401, json!({ "error": "Unauthorized request." }));
        }
        return match path {
            "/rtdb/.json" if query.contains("shallow=true") => (200, json!({ "users": true, "chats": true })),
            "/rtdb/chats.json" => (
                200,
                json!({ "room1": { "title": "Chung", "count": 2 }, "room2": { "title": "Riêng" } }),
            ),
            "/rtdb/flags/beta.json" => (200, json!(true)),
            _ => (200, Json::Null),
        };
    }
    if !authorized {
        return (
            401,
            json!({ "error": { "code": 401, "message": "Request had invalid authentication credentials.", "status": "UNAUTHENTICATED" } }),
        );
    }
    let documents = "/v1/projects/demo/databases/(default)/documents";
    let Some(rest) = path.strip_prefix(documents) else {
        return (
            404,
            json!({ "error": { "code": 404, "message": "no such page", "status": "NOT_FOUND" } }),
        );
    };
    match (method, rest) {
        ("POST", ":listCollectionIds") => (200, json!({ "collectionIds": ["orders", "users"] })),
        ("GET", "/orders") if query.contains("pageToken=p2") => (
            200,
            json!({ "documents": [document("o3", json!({ "total": { "integerValue": "7" }, "late": { "booleanValue": true } }))] }),
        ),
        ("GET", "/orders") => (
            200,
            json!({
                "documents": [
                    document("o1", json!({
                        "total": { "integerValue": "9007199254740993" },
                        "note": { "stringValue": "xin chào" },
                        "at": { "timestampValue": "2026-11-14T08:00:00.123456+07:00" },
                        "items": { "arrayValue": { "values": [{ "mapValue": { "fields": { "sku": { "stringValue": "A" }, "qty": { "integerValue": "2" } } } }] } },
                        "raw": { "bytesValue": "AP8=" },
                    })),
                    document("o2", json!({ "total": { "doubleValue": 1.5 }, "owner": { "referenceValue": "projects/demo/databases/(default)/documents/users/u1" } })),
                ],
                "nextPageToken": "p2",
            }),
        ),
        ("GET", "/orders/o1") => (200, document("o1", json!({ "note": { "stringValue": "một" } }))),
        ("GET", "/orders/none") => (
            404,
            json!({ "error": { "code": 404, "message": "Document not found", "status": "NOT_FOUND" } }),
        ),
        ("POST", ":runQuery") => {
            let query: Json = serde_json::from_str(body).unwrap_or(Json::Null);
            if query.pointer("/structuredQuery/orderBy").is_some() {
                return (
                    400,
                    json!([{ "error": { "code": 400, "message": "The query requires an index. You can create it here: https://console.firebase.google.com/x", "status": "FAILED_PRECONDITION" } }]),
                );
            }
            (
                200,
                json!([{ "document": document("o3", json!({ "total": { "integerValue": "7" } })), "readTime": "2026-11-14T00:00:00Z" }, { "readTime": "2026-11-14T00:00:00Z" }]),
            )
        }
        _ => (
            404,
            json!({ "error": { "code": 404, "message": format!("no {method} {rest}"), "status": "NOT_FOUND" } }),
        ),
    }
}

/// A one-request-per-connection HTTP/1.1 server on a loopback port; its address and what it was asked.
async fn serve() -> (String, Seen) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    let seen: Seen = Arc::default();
    let log = Arc::clone(&seen);
    let address = base.clone();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let log = Arc::clone(&log);
            let base = address.clone();
            tokio::spawn(async move {
                let mut buffer = Vec::new();
                let mut chunk = [0u8; 8192];
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
                let target = words.next().unwrap_or_default().to_owned();
                let authorized = head
                    .lines()
                    .any(|line| line.eq_ignore_ascii_case("authorization: Bearer good-token"));
                let body = String::from_utf8_lossy(&buffer[head_end..]).into_owned();
                log.lock().unwrap().push((method.clone(), target.clone(), body.clone()));
                let (status, reply) = answer(&method, &target, authorized, &body, &base);
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

fn config(base: &str, key: &str) -> ConnectionConfig {
    let account = json!({
        "type": "service_account",
        "project_id": "demo",
        "client_email": "kemable@demo.iam.gserviceaccount.com",
        "private_key": key,
        "token_uri": format!("{base}/token"),
    });
    ConnectionConfig {
        engine: "firebase".into(),
        target: Target::Http {
            endpoint: String::new(),
            token: Secret::new(account.to_string()),
        },
        options: BTreeMap::from([
            ("firestore_url".to_owned(), format!("{base}/v1")),
            ("database_url".to_owned(), format!("{base}/rtdb")),
        ]),
        ssh: None,
        dial: None,
    }
}

fn options() -> CallOptions {
    CallOptions::default().with_timeout(std::time::Duration::from_secs(10))
}

#[derive(Default)]
struct Rows {
    columns: Mutex<Vec<ColumnMeta>>,
    cells: Mutex<Vec<Value>>,
    limit: usize,
    /// The sink's memory budget, `ResultSink::byte_limit`.
    bytes: Option<u64>,
}

impl ResultSink for Rows {
    fn begin_set(&self, columns: Vec<ColumnMeta>) {
        *self.columns.lock().unwrap() = columns;
    }

    fn push_rows(&self, values: &[Value]) -> bool {
        let mut cells = self.cells.lock().unwrap();
        cells.extend_from_slice(values);
        let width = self.columns.lock().unwrap().len().max(1);
        self.limit == 0 || cells.len() / width < self.limit
    }

    fn byte_limit(&self) -> Option<u64> {
        self.bytes
    }

    fn end_set(&self) {}

    fn command_done(&self, _rows_affected: Option<u64>) {}

    fn notice(&self, _text: &str) {}
}

impl Rows {
    fn names(&self) -> Vec<String> {
        self.columns
            .lock()
            .unwrap()
            .iter()
            .map(|column| column.name.clone())
            .collect()
    }

    fn row(&self, at: usize) -> Vec<Value> {
        let width = self.columns.lock().unwrap().len();
        self.cells.lock().unwrap()[at * width..(at + 1) * width].to_vec()
    }

    fn count(&self) -> usize {
        self.cells.lock().unwrap().len() / self.columns.lock().unwrap().len().max(1)
    }
}

#[tokio::test]
async fn a_service_account_signs_in_and_a_collection_reads_page_after_page_with_its_values_exact() {
    let (base, seen) = serve().await;
    let connection = FirebaseDriver.connect(&config(&base, KEY), options()).await.unwrap();
    assert_eq!(
        connection.server_version().as_deref(),
        Some("demo"),
        "the project, from the key file"
    );

    let rows = Arc::new(Rows::default());
    connection.execute("get orders", rows.clone(), options()).await.unwrap();
    // Fields by name, as Firestore's console shows them.
    assert_eq!(
        rows.names(),
        [
            "id",
            "at",
            "items",
            "note",
            "raw",
            "total",
            "owner",
            "other_fields",
            "update_time"
        ]
    );
    assert_eq!(rows.columns.lock().unwrap()[5].kind, ValueKind::Int);
    assert_eq!(rows.count(), 3, "both pages");
    let first = rows.row(0);
    assert_eq!(first[0], Value::Text("o1".into()));
    assert_eq!(first[5], Value::Int(9_007_199_254_740_993), "past a float, exactly");
    assert_eq!(first[3], Value::Text("xin chào".into()));
    assert_eq!(
        first[1],
        Value::TimestampTz {
            micros: 1_794_618_000_123_456,
            offset_seconds: 0
        },
        "08:00+07:00 is 01:00 UTC"
    );
    assert_eq!(first[2], Value::Json(r#"[{"qty":2,"sku":"A"}]"#.into()));
    assert_eq!(first[4], Value::Bytes(vec![0x00, 0xff]));
    assert_eq!(first[7], Value::Null);
    assert_eq!(
        first[8],
        Value::TimestampTz {
            micros: 1_794_618_123_456_789,
            offset_seconds: 0
        }
    );
    assert_eq!(rows.row(1)[5], Value::Float(1.5));
    assert_eq!(
        rows.row(1)[6],
        Value::Text("users/u1".into()),
        "a reference as its path"
    );
    // A field only the second page has goes into other_fields.
    assert_eq!(rows.row(2)[7], Value::Json(r#"{"late":true}"#.into()));

    // The token is asked for once and kept.
    let asked = seen
        .lock()
        .unwrap()
        .iter()
        .filter(|(_, target, _)| target == "/token")
        .count();
    assert_eq!(asked, 1);

    // A row cap stops between pages.
    let capped = Arc::new(Rows {
        limit: 2,
        ..Rows::default()
    });
    let outcome = connection
        .execute("get orders", capped.clone(), options())
        .await
        .unwrap();
    assert!(outcome.limited);
}

#[tokio::test]
async fn a_document_a_query_and_the_realtime_database_read_and_errors_keep_googles_words() {
    let (base, seen) = serve().await;
    let connection = FirebaseDriver.connect(&config(&base, KEY), options()).await.unwrap();

    let one = Arc::new(Rows::default());
    connection
        .execute("get orders/o1", one.clone(), options())
        .await
        .unwrap();
    assert_eq!(one.row(0)[..2], [Value::Text("o1".into()), Value::Text("một".into())]);

    let missing = connection
        .execute("get orders/none", Arc::new(Rows::default()), options())
        .await
        .unwrap_err();
    assert_eq!(missing.code, ErrorCode::Query);
    assert_eq!(missing.engine_code.as_deref(), Some("NOT_FOUND"));

    let queried = Arc::new(Rows::default());
    connection
        .execute(
            r#"get orders where total > 5 and note == "x" limit 10"#,
            queried.clone(),
            options(),
        )
        .await
        .unwrap();
    assert_eq!(queried.count(), 1);
    let sent = seen
        .lock()
        .unwrap()
        .iter()
        .find(|(_, target, _)| target.ends_with(":runQuery"))
        .map(|(_, _, body)| serde_json::from_str::<Json>(body).unwrap())
        .unwrap();
    assert_eq!(sent["structuredQuery"]["from"], json!([{ "collectionId": "orders" }]));
    assert_eq!(sent["structuredQuery"]["limit"], 10);
    assert_eq!(
        sent["structuredQuery"]["where"]["compositeFilter"]["filters"][0]["fieldFilter"]["value"],
        json!({ "integerValue": "5" })
    );

    let index = connection
        .execute("get orders order by total desc", Arc::new(Rows::default()), options())
        .await
        .unwrap_err();
    assert!(index.message.contains("requires an index"), "{}", index.message);

    let chats = Arc::new(Rows::default());
    connection
        .execute("rtdb /chats", chats.clone(), options())
        .await
        .unwrap();
    assert_eq!(chats.names(), ["key", "count", "title"]);
    assert_eq!(
        chats.row(0),
        [Value::Text("room1".into()), Value::Int(2), Value::Text("Chung".into())]
    );
    assert_eq!(chats.row(1)[1], Value::Null);
    let flag = Arc::new(Rows::default());
    connection
        .execute("rtdb flags/beta", flag.clone(), options())
        .await
        .unwrap();
    assert_eq!(flag.row(0), [Value::Bool(true)]);

    let schemas = connection.list_schemas(options()).await.unwrap();
    assert_eq!(
        schemas.iter().map(|schema| schema.name.as_str()).collect::<Vec<_>>(),
        [FIRESTORE, REALTIME]
    );
    let collections = connection.list_objects(Some(FIRESTORE), options()).await.unwrap();
    assert_eq!(
        collections
            .iter()
            .map(|object| object.name.as_str())
            .collect::<Vec<_>>(),
        ["orders", "users"]
    );
    assert!(collections.iter().all(|object| object.kind == ObjectKind::Collection));
    let children = connection.list_objects(Some(REALTIME), options()).await.unwrap();
    assert_eq!(
        children.iter().map(|object| object.name.as_str()).collect::<Vec<_>>(),
        ["chats", "users"]
    );
    // Every kind the tree gets sits in one of its folders.
    for object in collections.iter().chain(&children) {
        assert!(
            super::INFO
                .tree_folders
                .iter()
                .any(|folder| folder.kinds.contains(&object.kind))
        );
    }
}

#[tokio::test]
async fn a_realtime_read_with_no_limit_stops_past_the_memory_budget() {
    let (base, _) = serve().await;
    let connection = FirebaseDriver.connect(&config(&base, KEY), options()).await.unwrap();
    // /chats is about 80 bytes of JSON; a budget of 40 bytes keeps 10 of text.
    let small = Arc::new(Rows {
        bytes: Some(40),
        ..Rows::default()
    });
    let error = connection
        .execute("rtdb /chats", small.clone(), options())
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::TooBig);
    assert_eq!(small.count(), 0, "no row of a value read in part");

    let roomy = Arc::new(Rows {
        bytes: Some(1_000_000),
        ..Rows::default()
    });
    connection
        .execute("rtdb /chats", roomy.clone(), options())
        .await
        .unwrap();
    assert_eq!(roomy.count(), 2);
}

#[tokio::test]
async fn a_key_google_refuses_or_one_that_does_not_read_fails_at_connect() {
    let (base, _) = serve().await;
    // Another key: a signature the server cannot verify.
    let other =
        "-----BEGIN PRIVATE KEY-----\nMIIBVQIBADANBgkqhkiG9w0BAQEFAASCAT8wggE7AgEAAkEA\n-----END PRIVATE KEY-----\n";
    let error = FirebaseDriver
        .connect(&config(&base, other), options())
        .await
        .err()
        .unwrap();
    assert_eq!(error.code, ErrorCode::ConfigInvalid, "{}", error.message);

    let mut no_key = config(&base, KEY);
    no_key.target = Target::Http {
        endpoint: "demo".into(),
        token: Secret::default(),
    };
    assert_eq!(
        FirebaseDriver.connect(&no_key, options()).await.err().unwrap().code,
        ErrorCode::ConfigInvalid
    );

    let mut not_json = config(&base, KEY);
    not_json.target = Target::Http {
        endpoint: "demo".into(),
        token: Secret::new("not json"),
    };
    assert_eq!(
        FirebaseDriver.connect(&not_json, options()).await.err().unwrap().code,
        ErrorCode::ConfigInvalid
    );
}

#[test]
fn console_lines_read_as_commands_and_typed_values() {
    let words = |line: &str| kemable_core::command::parse_args(line).unwrap();
    assert_eq!(
        console::parse(&words("get users/u1/orders")).unwrap(),
        console::Command::Get {
            path: "users/u1/orders".into(),
            query: None
        }
    );
    let console::Command::Get { query: Some(query), .. } = console::parse(&words(
        r#"get orders where status in '["new","paid"]' order by total limit 5"#,
    ))
    .unwrap() else {
        panic!("a query")
    };
    assert_eq!(query["where"]["fieldFilter"]["op"], "IN");
    assert_eq!(query["orderBy"][0]["direction"], "ASCENDING");
    assert!(
        console::parse(&words("get orders/o1 limit 5")).is_err(),
        "a document takes no query"
    );
    assert!(console::parse(&words("get orders where x ~ 1")).is_err());
    assert_eq!(
        console::parse(&words("rtdb /a/b limit 3")).unwrap(),
        console::Command::Rtdb {
            path: "a/b".into(),
            limit: Some(3)
        }
    );
    assert!(console::parse(&words("delete orders")).is_err());

    assert_eq!(values::typed_literal("12"), json!({ "integerValue": "12" }));
    assert_eq!(values::typed_literal("new"), json!({ "stringValue": "new" }));
    assert_eq!(values::rfc3339_micros("1970-01-01T00:00:00.5Z"), Some(500_000));
    assert_eq!(values::rfc3339_micros("1969-12-31T23:59:59Z"), Some(-1_000_000));
    assert_eq!(values::rfc3339_micros("not a time"), None);
    assert_eq!(values::base64_decode("aGk="), Some(b"hi".to_vec()));
}

#[tokio::test]
async fn a_key_file_is_read_at_connect_and_a_missing_one_says_where() {
    let (base, _) = serve().await;
    let folder = std::env::temp_dir().join(format!("kemable-firebase-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    let path = folder.join("key.json");
    let Target::Http { token, .. } = config(&base, KEY).target else {
        panic!("an http target")
    };
    std::fs::write(&path, token.expose()).unwrap();
    let mut from_file = config(&base, KEY);
    from_file.target = Target::Http {
        endpoint: String::new(),
        token: Secret::default(),
    };
    from_file
        .options
        .insert("key_file".into(), path.to_string_lossy().into_owned());
    let connection = FirebaseDriver.connect(&from_file, options()).await.unwrap();
    assert_eq!(connection.server_version().as_deref(), Some("demo"));

    from_file.options.insert(
        "key_file".into(),
        folder.join("gone.json").to_string_lossy().into_owned(),
    );
    let error = FirebaseDriver.connect(&from_file, options()).await.err().unwrap();
    assert_eq!(error.code, ErrorCode::ConfigInvalid);
    assert!(error.message.contains("gone.json"));
    std::fs::remove_dir_all(&folder).unwrap();
}

/// The events one stream sends, by path: its first value, then changes, a
/// keep-alive between them. `/denied` is cut off by the rules.
fn events(path: &str) -> Option<String> {
    let event = |name: &str, data: &str| format!("event: {name}\ndata: {data}\n\n");
    Some(match path {
        "/rtdb/chats.json" => [
            event("put", r#"{"path":"/","data":{"room1":{"title":"Chung"}}}"#),
            event("keep-alive", "null"),
            event("put", r#"{"path":"/room2","data":{"title":"Mới"}}"#),
            // A data line split across two `data:` lines, and \r\n endings.
            "event: patch\r\ndata: {\"path\":\"/room1\",\r\ndata: \"data\":{\"count\":3}}\r\n\r\n".to_owned(),
            event("put", r#"{"path":"/room2/title","data":null}"#),
        ]
        .concat(),
        "/rtdb/flags.json" => [
            event("put", r#"{"path":"/","data":true}"#),
            event("put", r#"{"path":"/","data":false}"#),
        ]
        .concat(),
        "/rtdb/denied.json" => [event("put", r#"{"path":"/","data":1}"#), event("cancel", "null")].concat(),
        _ => return None,
    })
}

/// A server that answers an event-stream request with `events`, then keeps it open.
async fn stream_server() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut buffer = Vec::new();
                let mut chunk = [0u8; 4096];
                while !buffer.windows(4).any(|window| window == b"\r\n\r\n") {
                    let Ok(read) = socket.read(&mut chunk).await else {
                        return;
                    };
                    if read == 0 {
                        return;
                    }
                    buffer.extend_from_slice(&chunk[..read]);
                }
                let head = String::from_utf8_lossy(&buffer).into_owned();
                let target = head.split_whitespace().nth(1).unwrap_or_default().to_owned();
                let (path, query) = target.split_once('?').unwrap_or((&target, ""));
                let streaming = head
                    .lines()
                    .any(|line| line.eq_ignore_ascii_case("accept: text/event-stream"));
                let Some(body) = events(path).filter(|_| streaming && query.contains("access_token=good-token")) else {
                    let _ = socket
                        .write_all(b"HTTP/1.1 401 X\r\ncontent-type: application/json\r\ncontent-length: 33\r\nconnection: close\r\n\r\n{\"error\":\"Unauthorized request.\"}")
                        .await;
                    return;
                };
                let _ = socket
                    .write_all(b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\n\r\n")
                    .await;
                let _ = socket.write_all(body.as_bytes()).await;
                std::future::pending::<()>().await;
            });
        }
    });
    base
}

#[tokio::test]
async fn a_live_path_turns_each_change_after_the_first_value_into_a_row() {
    use kemable_core::live::{LiveTopicKind, topics_of};
    let (base, _) = serve().await;
    let streams = stream_server().await;
    let mut listening = config(&base, KEY);
    listening
        .options
        .insert("database_url".into(), format!("{streams}/rtdb"));
    let heard = Arc::new(Rows {
        limit: 4,
        ..Rows::default()
    });
    let topics = topics_of(&[(LiveTopicKind::Path, "/chats flags")]);
    let outcome = FirebaseDriver
        .listen(&listening, &topics, heard.clone(), options())
        .await
        .unwrap();
    assert!(outcome.limited);
    assert_eq!(heard.names(), ["received", "path", "event", "data"]);
    let mut rows: Vec<Vec<Value>> = (0..4).map(|at| heard.row(at)[1..].to_vec()).collect();
    // The two paths' events interleave; each path's keep their order.
    rows.sort_by_key(|row| format!("{:?}", row[0]));
    assert_eq!(
        rows,
        [
            vec![
                Value::Text("/chats/room1".into()),
                Value::Text("patch".into()),
                Value::Json(r#"{"count":3}"#.into())
            ],
            vec![
                Value::Text("/chats/room2".into()),
                Value::Text("put".into()),
                Value::Json(r#"{"title":"Mới"}"#.into())
            ],
            vec![
                Value::Text("/chats/room2/title".into()),
                Value::Text("put".into()),
                Value::Null
            ],
            vec![
                Value::Text("/flags".into()),
                Value::Text("put".into()),
                Value::Bool(false)
            ],
        ]
    );

    let denied = FirebaseDriver
        .listen(
            &listening,
            &topics_of(&[(LiveTopicKind::Path, "/denied")]),
            Arc::new(Rows::default()),
            options(),
        )
        .await
        .unwrap_err();
    assert_eq!(denied.code, ErrorCode::Query);
    assert!(denied.message.contains("denied"));

    let refused = FirebaseDriver
        .listen(
            &listening,
            &topics_of(&[(LiveTopicKind::Path, "/nowhere")]),
            Arc::new(Rows::default()),
            options(),
        )
        .await
        .unwrap_err();
    assert_eq!(refused.code, ErrorCode::Auth);
}

#[test]
fn server_sent_events_read_across_chunks_and_paths_join() {
    let mut reader = super::live::EventReader::default();
    assert!(reader.feed(b"event: put\nda").is_empty());
    assert_eq!(
        reader.feed(b"ta: 1\n\nevent: keep-alive\ndata: null\n\n"),
        [
            ("put".to_owned(), "1".to_owned()),
            ("keep-alive".to_owned(), "null".to_owned())
        ]
    );
    assert_eq!(super::live::joined("/chats/", "/room1"), "/chats/room1");
    assert_eq!(super::live::joined("", "/"), "/");
    assert_eq!(super::live::joined("a", "/"), "/a");
}
