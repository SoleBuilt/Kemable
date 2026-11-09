//! The driver against a small RESP2 server in the test (enough of Redis for
//! login, scans and each kind of key), and against a real Redis when
//! `$PABOABLE_REDIS_URL` is set.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use paboable_core::catalog::ColumnMeta;
use paboable_core::config::{ConnectionConfig, Secret, Target, TlsMode};
use paboable_core::driver::Driver;
use paboable_core::keys::{KeyKind, KeyScan};
use paboable_core::sink::ResultSink;
use paboable_core::value::Value;
use paboable_core::{CallOptions, ErrorCode};
use paboable_store::ResultStore;
use tokio::io::{AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

use crate::resp::{self, Reply};
use crate::{RedisDriver, next_stream_id};

#[derive(Clone)]
enum Entry {
    Text(Vec<u8>),
    Hash(Vec<(String, String)>),
    List(Vec<String>),
    Set(Vec<String>),
    Sorted(Vec<(String, String)>),
    Stream(Vec<(String, Vec<(String, String)>)>),
}

type Store = Arc<Mutex<BTreeMap<String, (Entry, i64)>>>;

fn bulk(text: &str) -> Reply {
    Reply::Bulk(text.as_bytes().to_vec())
}

fn array(items: impl IntoIterator<Item = Reply>) -> Reply {
    Reply::Array(items.into_iter().collect())
}

/// One reply as RESP2 bytes.
fn encode_reply(reply: &Reply, out: &mut Vec<u8>) {
    match reply {
        Reply::Status(text) => out.extend(format!("+{text}\r\n").into_bytes()),
        Reply::Error(text) => out.extend(format!("-{text}\r\n").into_bytes()),
        Reply::Integer(value) => out.extend(format!(":{value}\r\n").into_bytes()),
        Reply::Nil => out.extend(b"$-1\r\n"),
        Reply::Bulk(bytes) => {
            out.extend(format!("${}\r\n", bytes.len()).into_bytes());
            out.extend(bytes);
            out.extend(b"\r\n");
        }
        Reply::Array(items) => {
            out.extend(format!("*{}\r\n", items.len()).into_bytes());
            for item in items {
                encode_reply(item, out);
            }
        }
    }
}

fn matches(pattern: &str, key: &str) -> bool {
    match pattern.strip_suffix('*') {
        Some(prefix) => key.starts_with(prefix),
        None => pattern == key,
    }
}

fn type_of(entry: &Entry) -> &'static str {
    match entry {
        Entry::Text(_) => "string",
        Entry::Hash(_) => "hash",
        Entry::List(_) => "list",
        Entry::Set(_) => "set",
        Entry::Sorted(_) => "zset",
        Entry::Stream(_) => "stream",
    }
}

/// A page of `items` from the scan cursor `cursor` (an offset here).
fn page<T: Clone>(items: &[T], cursor: &str, count: usize) -> (String, Vec<T>) {
    let start: usize = cursor.parse().unwrap_or(0);
    let end = (start + count).min(items.len());
    let next = if end >= items.len() {
        "0".to_owned()
    } else {
        end.to_string()
    };
    (next, items[start.min(end)..end].to_vec())
}

fn answer(store: &Store, logged_in: &mut bool, words: &[String]) -> Option<Reply> {
    let upper: Vec<String> = words.iter().map(|word| word.to_ascii_uppercase()).collect();
    let arg = |index: usize| words.get(index).cloned().unwrap_or_default();
    let count = |default: usize| {
        upper
            .iter()
            .position(|word| word == "COUNT")
            .and_then(|at| words.get(at + 1))
            .and_then(|count| count.parse().ok())
            .unwrap_or(default)
    };
    if upper[0] == "AUTH" {
        return Some(if words.last().map(String::as_str) == Some("pw") {
            *logged_in = true;
            Reply::Status("OK".into())
        } else {
            Reply::Error("WRONGPASS invalid username-password pair or user is disabled.".into())
        });
    }
    if !*logged_in {
        return Some(Reply::Error("NOAUTH Authentication required.".into()));
    }
    let data = store.lock().unwrap();
    let entry = data.get(&arg(1)).map(|(entry, _)| entry.clone());
    Some(match upper[0].as_str() {
        "PING" => Reply::Status("PONG".into()),
        "SELECT" => Reply::Status("OK".into()),
        "INFO" => bulk("# Server\r\nredis_version:7.4.1\r\nos:test\r\n"),
        "CONFIG" => array([bulk("databases"), bulk("4")]),
        "BLPOP" => return None,
        "TYPE" => Reply::Status(entry.as_ref().map_or("none", type_of).into()),
        "PTTL" => Reply::Integer(data.get(&arg(1)).map_or(-2, |(_, ttl)| *ttl)),
        "GET" => match entry {
            Some(Entry::Text(bytes)) => Reply::Bulk(bytes),
            Some(_) => Reply::Error("WRONGTYPE Operation against a key holding the wrong kind of value".into()),
            None => Reply::Nil,
        },
        "STRLEN" | "HLEN" | "LLEN" | "SCARD" | "ZCARD" | "XLEN" => Reply::Integer(match entry {
            Some(Entry::Text(bytes)) => bytes.len(),
            Some(Entry::Hash(items) | Entry::Sorted(items)) => items.len(),
            Some(Entry::List(items) | Entry::Set(items)) => items.len(),
            Some(Entry::Stream(items)) => items.len(),
            None => 0,
        } as i64),
        "SCAN" => {
            let pattern = upper
                .iter()
                .position(|word| word == "MATCH")
                .map_or("*".to_owned(), |at| arg(at + 1));
            let kind = upper.iter().position(|word| word == "TYPE").map(|at| arg(at + 1));
            let keys: Vec<String> = data.keys().cloned().collect();
            let (next, found) = page(&keys, &arg(1), count(10));
            let found = found
                .into_iter()
                .filter(|key| matches(&pattern, key))
                .filter(|key| kind.as_deref().is_none_or(|kind| type_of(&data[key].0) == kind))
                .map(|key| bulk(&key));
            array([bulk(&next), array(found)])
        }
        "HSCAN" | "SSCAN" => {
            let (next, found): (String, Vec<Reply>) = match entry {
                Some(Entry::Hash(items)) => {
                    let (next, items) = page(&items, &arg(2), count(10));
                    (next, items.iter().flat_map(|(k, v)| [bulk(k), bulk(v)]).collect())
                }
                Some(Entry::Set(items)) => {
                    let (next, items) = page(&items, &arg(2), count(10));
                    (next, items.iter().map(|item| bulk(item)).collect())
                }
                _ => ("0".to_owned(), Vec::new()),
            };
            array([bulk(&next), array(found)])
        }
        "LRANGE" | "ZRANGE" => {
            let start: usize = arg(2).parse().unwrap_or(0);
            let stop: usize = arg(3).parse().unwrap_or(0);
            let within = |len: usize| start.min(len)..(stop + 1).min(len);
            match entry {
                Some(Entry::List(items)) => array(items[within(items.len())].iter().map(|item| bulk(item))),
                Some(Entry::Sorted(items)) => array(
                    items[within(items.len())]
                        .iter()
                        .flat_map(|(member, score)| [bulk(member), bulk(score)]),
                ),
                _ => array([]),
            }
        }
        "XRANGE" => match entry {
            Some(Entry::Stream(items)) => {
                let from = arg(2);
                let after = |id: &str| from == "-" || id_key(id) >= id_key(&from);
                array(
                    items
                        .iter()
                        .filter(|(id, _)| after(id))
                        .take(count(usize::MAX))
                        .map(|(id, fields)| {
                            array([bulk(id), array(fields.iter().flat_map(|(k, v)| [bulk(k), bulk(v)]))])
                        }),
                )
            }
            _ => array([]),
        },
        other => Reply::Error(format!("ERR unknown command '{other}'")),
    })
}

fn id_key(id: &str) -> (u64, u64) {
    let (ms, seq) = id.split_once('-').unwrap_or((id, "0"));
    (ms.parse().unwrap_or(0), seq.parse().unwrap_or(0))
}

/// Starts the server; its address and how many connections it accepted.
async fn serve(store: Store) -> (u16, Arc<Mutex<usize>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let accepted = Arc::new(Mutex::new(0));
    let counter = Arc::clone(&accepted);
    tokio::spawn(async move {
        loop {
            let Ok((socket, _)) = listener.accept().await else {
                return;
            };
            *counter.lock().unwrap() += 1;
            let store = Arc::clone(&store);
            tokio::spawn(async move {
                let (read, mut write) = socket.into_split();
                let mut read = BufReader::new(read);
                let mut logged_in = false;
                while let Ok(Reply::Array(words)) = resp::read(&mut read).await {
                    let words: Vec<String> = words.iter().map(|word| word.text().unwrap_or_default()).collect();
                    let Some(reply) = answer(&store, &mut logged_in, &words) else {
                        // A blocking command: never answer, as BLPOP on an empty list.
                        std::future::pending::<()>().await;
                        return;
                    };
                    let mut out = Vec::new();
                    encode_reply(&reply, &mut out);
                    if write.write_all(&out).await.is_err() {
                        return;
                    }
                }
            });
        }
    });
    (port, accepted)
}

fn sample() -> Store {
    let mut data = BTreeMap::new();
    let n = 2500;
    data.insert("greeting".to_owned(), (Entry::Text("xin chào".as_bytes().to_vec()), -1));
    data.insert("session:1".to_owned(), (Entry::Text(b"a".to_vec()), 90_000));
    data.insert("session:2".to_owned(), (Entry::Text(b"b".to_vec()), -1));
    data.insert(
        "h".to_owned(),
        (
            Entry::Hash((0..n).map(|i| (format!("f{i}"), format!("v{i}"))).collect()),
            -1,
        ),
    );
    data.insert(
        "l".to_owned(),
        (Entry::List((0..n).map(|i| format!("item {i}")).collect()), -1),
    );
    data.insert(
        "s".to_owned(),
        (Entry::Set((0..n).map(|i| format!("m{i}")).collect()), -1),
    );
    data.insert(
        "z".to_owned(),
        (
            Entry::Sorted((0..n).map(|i| (format!("p{i}"), format!("{i}.5"))).collect()),
            -1,
        ),
    );
    data.insert(
        "x".to_owned(),
        (
            Entry::Stream(
                (0..n)
                    .map(|i| {
                        (
                            format!("{}-{}", 1000 + i / 3, i % 3),
                            vec![("n".to_owned(), i.to_string())],
                        )
                    })
                    .collect(),
            ),
            -1,
        ),
    );
    Arc::new(Mutex::new(data))
}

fn config(port: u16, password: &str, database: Option<&str>) -> ConnectionConfig {
    ConnectionConfig {
        engine: "redis".into(),
        target: Target::Network {
            host: "127.0.0.1".into(),
            port: Some(port),
            user: None,
            password: Secret::new(password),
            database: database.map(str::to_owned),
            tls: TlsMode::Disable,
        },
        options: BTreeMap::new(),
    }
}

fn options() -> CallOptions {
    CallOptions::default().with_timeout(Duration::from_secs(10))
}

/// A sink that keeps one set in a store with `cap` rows.
struct Keep {
    cap: Option<u64>,
    store: Mutex<Option<ResultStore>>,
    columns: Mutex<Vec<String>>,
}

impl Keep {
    fn new(cap: Option<u64>) -> Arc<Self> {
        Arc::new(Self {
            cap,
            store: Mutex::new(None),
            columns: Mutex::new(Vec::new()),
        })
    }

    fn total(&self) -> u64 {
        self.store
            .lock()
            .unwrap()
            .as_ref()
            .map_or(0, |store| store.state().total_rows)
    }

    fn row(&self, row: u64) -> Vec<Option<String>> {
        self.store
            .lock()
            .unwrap()
            .as_ref()
            .and_then(|store| store.row_texts(row))
            .unwrap_or_default()
    }
}

impl ResultSink for Keep {
    fn begin_set(&self, columns: Vec<ColumnMeta>) {
        *self.columns.lock().unwrap() = columns.iter().map(|column| column.name.clone()).collect();
        *self.store.lock().unwrap() = Some(ResultStore::new(columns, self.cap));
    }
    fn push_rows(&self, values: &[Value]) -> bool {
        self.store
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|store| store.push_rows(values))
    }
    fn end_set(&self) {}
    fn command_done(&self, _rows_affected: Option<u64>) {}
    fn notice(&self, _text: &str) {}
}

#[tokio::test]
async fn a_wrong_password_is_an_auth_error_and_the_right_one_logs_in() {
    let (port, _) = serve(sample()).await;
    let refused = RedisDriver.connect(&config(port, "nope", None), options()).await.err();
    assert_eq!(refused.as_ref().map(|error| error.code), Some(ErrorCode::Auth));
    assert_eq!(
        refused.and_then(|error| error.engine_code).as_deref(),
        Some("WRONGPASS")
    );

    let connection = RedisDriver
        .connect(&config(port, "pw", Some("2")), options())
        .await
        .unwrap();
    assert_eq!(connection.server_version().as_deref(), Some("7.4.1"));
    assert_eq!(connection.current_database().as_deref(), Some("2"));
    let databases = connection.list_databases(options()).await.unwrap();
    assert_eq!(databases.len(), 4);
    assert!(databases[2].is_current);
}

#[tokio::test]
async fn a_bad_target_is_a_config_error() {
    let mut bad = config(1, "pw", Some("two"));
    let error = RedisDriver.connect(&bad, options()).await.err().map(|error| error.code);
    assert_eq!(error, Some(ErrorCode::ConfigInvalid));
    bad.target = Target::File {
        path: "x".into(),
        read_only: false,
    };
    let error = RedisDriver.connect(&bad, options()).await.err().map(|error| error.code);
    assert_eq!(error, Some(ErrorCode::ConfigInvalid));
}

#[tokio::test]
async fn a_scan_goes_step_by_step_with_kinds_and_ttls() {
    let (port, _) = serve(sample()).await;
    let connection = RedisDriver.connect(&config(port, "pw", None), options()).await.unwrap();
    let mut scan = KeyScan {
        pattern: "*".into(),
        cursor: None,
        count: 3,
        kind: None,
    };
    let mut seen = Vec::new();
    loop {
        let page = connection.scan_keys(&scan, options()).await.unwrap();
        seen.extend(page.keys);
        match page.cursor {
            Some(cursor) => scan.cursor = Some(cursor),
            None => break,
        }
    }
    let names: Vec<&str> = seen.iter().map(|key| key.name.as_str()).collect();
    assert_eq!(names, ["greeting", "h", "l", "s", "session:1", "session:2", "x", "z"]);
    let session = seen.iter().find(|key| key.name == "session:1").unwrap();
    assert_eq!((session.kind, session.ttl_ms), (KeyKind::String, Some(90_000)));
    assert_eq!(
        seen.iter().find(|key| key.name == "z").map(|key| key.kind),
        Some(KeyKind::SortedSet)
    );

    let only = KeyScan {
        pattern: "session:*".into(),
        cursor: None,
        count: 100,
        kind: Some(KeyKind::String),
    };
    let page = connection.scan_keys(&only, options()).await.unwrap();
    assert_eq!(page.keys.len(), 2);
    assert_eq!(page.cursor, None);
}

#[tokio::test]
async fn each_kind_of_key_reads_in_steps_to_its_end() {
    let (port, _) = serve(sample()).await;
    let connection = RedisDriver.connect(&config(port, "pw", None), options()).await.unwrap();
    let cases = [
        (
            "h",
            KeyKind::Hash,
            vec!["field", "value"],
            vec![Some("f2499"), Some("v2499")],
        ),
        (
            "l",
            KeyKind::List,
            vec!["index", "value"],
            vec![Some("2499"), Some("item 2499")],
        ),
        ("s", KeyKind::Set, vec!["member"], vec![Some("m2499")]),
        (
            "z",
            KeyKind::SortedSet,
            vec!["member", "score"],
            vec![Some("p2499"), Some("2499.5")],
        ),
        (
            "x",
            KeyKind::Stream,
            vec!["id", "fields"],
            vec![Some("1833-0"), Some("{\"n\":\"2499\"}")],
        ),
    ];
    for (key, kind, columns, last) in cases {
        let sink = Keep::new(None);
        let detail = connection.read_key(key, sink.clone(), options()).await.unwrap();
        assert_eq!((detail.kind, detail.length), (kind, 2500), "{key}");
        assert_eq!(*sink.columns.lock().unwrap(), columns, "{key}");
        assert_eq!(sink.total(), 2500, "{key}");
        let last: Vec<Option<String>> = last.into_iter().map(|text| text.map(str::to_owned)).collect();
        assert_eq!(sink.row(2499), last, "{key}");
    }

    let sink = Keep::new(None);
    let detail = connection.read_key("greeting", sink.clone(), options()).await.unwrap();
    assert_eq!((detail.kind, detail.length, detail.ttl_ms), (KeyKind::String, 9, None));
    assert_eq!(sink.row(0), [Some("xin chào".to_owned())]);

    let missing = connection.read_key("nope", Keep::new(None), options()).await.err();
    assert_eq!(missing.map(|error| error.code), Some(ErrorCode::NotFound));
}

#[tokio::test]
async fn reading_a_key_stops_at_the_row_cap() {
    let (port, _) = serve(sample()).await;
    let connection = RedisDriver.connect(&config(port, "pw", None), options()).await.unwrap();
    let sink = Keep::new(Some(1200));
    connection.read_key("l", sink.clone(), options()).await.unwrap();
    assert_eq!(sink.total(), 1200);
}

#[tokio::test]
async fn the_console_runs_a_line_and_refuses_what_would_break_the_link() {
    let (port, _) = serve(sample()).await;
    let connection = RedisDriver.connect(&config(port, "pw", None), options()).await.unwrap();
    let sink = Keep::new(None);
    connection
        .execute("GET greeting", sink.clone(), options())
        .await
        .unwrap();
    assert_eq!(sink.row(0), [Some("xin chào".to_owned())]);

    let error = connection
        .execute("GET h", Keep::new(None), options())
        .await
        .err()
        .unwrap();
    assert_eq!(
        (error.code, error.engine_code.as_deref()),
        (ErrorCode::Query, Some("WRONGTYPE"))
    );
    for refused in ["SUBSCRIBE news", "select 1", "MONITOR"] {
        let code = connection
            .execute(refused, Keep::new(None), options())
            .await
            .err()
            .map(|error| error.code);
        assert_eq!(code, Some(ErrorCode::Unsupported), "{refused}");
    }
    let code = connection
        .execute("GET \"open", Keep::new(None), options())
        .await
        .err()
        .map(|error| error.code);
    assert_eq!(code, Some(ErrorCode::ConfigInvalid));
}

#[tokio::test]
async fn a_cancelled_blocking_command_drops_the_link_and_the_next_call_opens_one() {
    let (port, accepted) = serve(sample()).await;
    let connection = RedisDriver.connect(&config(port, "pw", None), options()).await.unwrap();
    let cancel = CancellationToken::new();
    let stopper = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        stopper.cancel();
    });
    let error = connection
        .execute("BLPOP empty 0", Keep::new(None), CallOptions::new(cancel))
        .await
        .err()
        .map(|error| error.code);
    assert_eq!(error, Some(ErrorCode::Cancelled));
    connection.ping(options()).await.unwrap();
    assert_eq!(*accepted.lock().unwrap(), 2, "a new link replaced the one left waiting");

    let error = connection
        .execute(
            "BLPOP empty 0",
            Keep::new(None),
            CallOptions::default().with_timeout(Duration::from_millis(100)),
        )
        .await
        .err()
        .map(|error| error.code);
    assert_eq!(error, Some(ErrorCode::Timeout));
}

#[test]
fn a_stream_reads_on_from_the_id_after_the_last() {
    assert_eq!(next_stream_id("1700000000000-5").as_deref(), Some("1700000000000-6"));
    assert_eq!(next_stream_id(&format!("7-{}", u64::MAX)).as_deref(), Some("8-0"));
    assert_eq!(next_stream_id("bad"), None);
}

#[tokio::test]
#[ignore = "needs Redis at $PABOABLE_REDIS_URL"]
async fn a_real_server_scans_and_reads_every_kind() {
    let url = std::env::var("PABOABLE_REDIS_URL").expect("PABOABLE_REDIS_URL is set");
    let config = paboable_core::connection_url::parse(&url).expect("a valid URL");
    let connection = RedisDriver
        .connect(&config, options())
        .await
        .expect("the server accepts the login");
    let prefix = format!("paboable:test:{}:", std::process::id());
    let script = [
        format!("SET {prefix}s \"xin chào\" PX 60000"),
        format!("HSET {prefix}h a 1 b 2"),
        format!("RPUSH {prefix}l x y z"),
        format!("SADD {prefix}set m n"),
        format!("ZADD {prefix}z 1.5 p 2 q"),
        format!("XADD {prefix}x * n 1"),
    ];
    for line in &script {
        connection.execute(line, Keep::new(None), options()).await.expect(line);
    }
    let scan = KeyScan {
        pattern: format!("{prefix}*"),
        cursor: None,
        count: 1000,
        kind: None,
    };
    let mut found = Vec::new();
    let mut scan = scan;
    loop {
        let page = connection.scan_keys(&scan, options()).await.unwrap();
        found.extend(page.keys);
        match page.cursor {
            Some(cursor) => scan.cursor = Some(cursor),
            None => break,
        }
    }
    assert_eq!(found.len(), 6);
    for (suffix, length) in [("s", 9), ("h", 2), ("l", 3), ("set", 2), ("z", 2), ("x", 1)] {
        let sink = Keep::new(None);
        let detail = connection
            .read_key(&format!("{prefix}{suffix}"), sink.clone(), options())
            .await
            .unwrap();
        assert_eq!(detail.length, length, "{suffix}");
        assert_eq!(sink.total(), if suffix == "s" { 1 } else { length }, "{suffix}");
    }
    let keys: Vec<String> = found.iter().map(|key| key.name.clone()).collect();
    connection
        .execute(&format!("DEL {}", keys.join(" ")), Keep::new(None), options())
        .await
        .unwrap();
}
