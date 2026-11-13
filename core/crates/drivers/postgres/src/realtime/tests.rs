//! Realtime against a small Phoenix server inside the tests.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use futures_util::{SinkExt, StreamExt};
use kemable_core::catalog::ColumnMeta;
use kemable_core::config::{ConnectionConfig, Secret, Target, TlsMode};
use kemable_core::live::{LiveTopicKind, topics_of};
use kemable_core::sink::ResultSink;
use kemable_core::value::Value;
use kemable_core::{CallOptions, ErrorCode};
use serde_json::{Value as Json, json};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;

use super::{listen, project_url, socket_url};

#[derive(Default)]
struct Heard {
    columns: Mutex<Vec<String>>,
    rows: Mutex<Vec<Vec<Value>>>,
    notices: Mutex<Vec<String>>,
    limit: usize,
}

impl ResultSink for Heard {
    fn begin_set(&self, columns: Vec<ColumnMeta>) {
        *self.columns.lock().unwrap() = columns.into_iter().map(|column| column.name).collect();
    }

    fn push_rows(&self, values: &[Value]) -> bool {
        let mut rows = self.rows.lock().unwrap();
        rows.push(values[1..].to_vec());
        rows.len() < self.limit
    }

    fn end_set(&self) {}

    fn command_done(&self, _rows_affected: Option<u64>) {}

    fn notice(&self, text: &str) {
        self.notices.lock().unwrap().push(text.to_owned());
    }
}

/// What the server does once a client joins: send these messages, or refuse.
enum Script {
    Events(Vec<Json>),
    RefuseJoin(&'static str),
    RefuseChanges(&'static str),
}

/// A Phoenix server on a free port that answers every join, says the table
/// changes are subscribed, then plays `script`. Hands back the joins it got.
async fn server(script: Script) -> (u16, Arc<Mutex<Vec<Json>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let joins = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&joins);
    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
        let mut answered = false;
        while let Some(Ok(frame)) = socket.next().await {
            let Message::Text(text) = frame else { continue };
            let message: Json = serde_json::from_str(&text).unwrap();
            let topic = message["topic"].clone();
            let reference = message["ref"].clone();
            match message["event"].as_str().unwrap() {
                "phx_join" => {
                    seen.lock().unwrap().push(message.clone());
                    let status = if matches!(script, Script::RefuseJoin(_)) {
                        "error"
                    } else {
                        "ok"
                    };
                    let reason = if let Script::RefuseJoin(reason) = script {
                        reason
                    } else {
                        ""
                    };
                    let reply = json!({ "topic": topic, "event": "phx_reply", "ref": reference,
                        "payload": { "status": status, "response": { "reason": reason } } });
                    socket.send(Message::text(reply.to_string())).await.unwrap();
                    if topic == "realtime:kemable" {
                        let (status, words) = match &script {
                            Script::RefuseChanges(reason) => ("error", *reason),
                            _ => ("ok", "Subscribed to PostgreSQL"),
                        };
                        let system = json!({ "topic": topic, "event": "system", "ref": null,
                            "payload": { "channel": "kemable", "extension": "postgres_changes", "status": status, "message": words } });
                        socket.send(Message::text(system.to_string())).await.unwrap();
                    }
                }
                "heartbeat" => {
                    let reply = json!({ "topic": "phoenix", "event": "phx_reply", "ref": reference, "payload": { "status": "ok", "response": {} } });
                    socket.send(Message::text(reply.to_string())).await.unwrap();
                }
                _ => {}
            }
            let joined = seen.lock().unwrap().len();
            if !answered && joined >= 2 {
                answered = true;
                if let Script::Events(events) = &script {
                    for event in events {
                        socket.send(Message::text(event.to_string())).await.unwrap();
                    }
                }
            }
        }
    });
    (port, joins)
}

fn change(kind: &str, record: Json, old: Json) -> Json {
    json!({ "topic": "realtime:kemable", "event": "postgres_changes", "ref": null, "payload": { "ids": [1], "data": {
        "schema": "public", "table": "orders", "type": kind, "commit_timestamp": "2026-11-14T01:02:03.456Z",
        "record": record, "old_record": old, "columns": [], "errors": null } } })
}

fn broadcast(event: &str, payload: Json) -> Json {
    json!({ "topic": "realtime:room1", "event": "broadcast", "ref": null, "payload": { "type": "broadcast", "event": event, "payload": payload } })
}

fn options() -> CallOptions {
    CallOptions::default().with_timeout(std::time::Duration::from_secs(10))
}

#[tokio::test]
async fn table_changes_and_broadcasts_are_rows_once_every_topic_is_taken() {
    let (port, joins) = server(Script::Events(vec![
        json!({ "topic": "realtime:kemable", "event": "presence_state", "ref": null, "payload": {} }),
        change("INSERT", json!({ "id": 1, "note": "xin chào" }), json!({})),
        broadcast("cursor", json!({ "x": 3 })),
        change("UPDATE", json!({ "id": 1, "note": "đã sửa" }), json!({ "id": 1 })),
    ]))
    .await;
    let heard = Heard {
        limit: 3,
        ..Heard::default()
    };
    let topics = topics_of(&[
        (LiveTopicKind::Table, "public.orders, audit.*"),
        (LiveTopicKind::Broadcast, "room1"),
    ]);
    let outcome = listen(
        &format!("http://127.0.0.1:{port}"),
        &Secret::new("anon-key"),
        &topics,
        &heard,
        &options(),
    )
    .await
    .unwrap();
    assert!(outcome.limited, "it stops at the sink's limit");
    assert_eq!(
        *heard.columns.lock().unwrap(),
        ["received", "channel", "event", "committed", "record", "old_record"]
    );
    let rows = heard.rows.lock().unwrap().clone();
    assert_eq!(
        rows[0],
        [
            Value::Text("public.orders".into()),
            Value::Text("INSERT".into()),
            Value::Text("2026-11-14T01:02:03.456Z".into()),
            Value::Json(r#"{"id":1,"note":"xin chào"}"#.into()),
            Value::Null,
        ]
    );
    assert_eq!(
        rows[1],
        [
            Value::Text("room1".into()),
            Value::Text("cursor".into()),
            Value::Null,
            Value::Json(r#"{"x":3}"#.into()),
            Value::Null
        ]
    );
    assert_eq!(rows[2][4], Value::Json(r#"{"id":1}"#.into()));

    let joins = joins.lock().unwrap().clone();
    assert_eq!(joins[0]["topic"], "realtime:kemable");
    assert_eq!(
        joins[0]["payload"]["config"]["postgres_changes"],
        json!([{ "event": "*", "schema": "public", "table": "orders" }, { "event": "*", "schema": "audit" }])
    );
    assert_eq!(joins[0]["payload"]["access_token"], "anon-key");
    assert_eq!(joins[1]["topic"], "realtime:room1");
}

#[tokio::test]
async fn a_refused_join_or_subscription_fails_with_the_servers_reason() {
    let topics = topics_of(&[(LiveTopicKind::Table, "orders"), (LiveTopicKind::Broadcast, "room1")]);
    let (port, _) = server(Script::RefuseJoin("Unauthorized: You do not have permissions")).await;
    let error = listen(
        &format!("http://127.0.0.1:{port}"),
        &Secret::new("k"),
        &topics,
        &Heard::default(),
        &options(),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::Query);
    assert!(error.message.contains("Unauthorized"));

    let (port, _) = server(Script::RefuseChanges(
        "Unable to subscribe to changes: table not in publication",
    ))
    .await;
    let error = listen(
        &format!("http://127.0.0.1:{port}"),
        &Secret::new("k"),
        &topics,
        &Heard::default(),
        &options(),
    )
    .await
    .unwrap_err();
    assert!(error.message.contains("publication"));
}

#[tokio::test]
async fn a_stop_ends_the_listening_and_a_closed_socket_is_lost() {
    let (port, _) = server(Script::Events(Vec::new())).await;
    let topics = topics_of(&[(LiveTopicKind::Broadcast, "room1"), (LiveTopicKind::Broadcast, "room2")]);
    let call = options();
    let stopper = call.cancel.clone();
    let heard = Heard {
        limit: 10,
        ..Heard::default()
    };
    let url = format!("http://127.0.0.1:{port}");
    let key = Secret::new("k");
    let listening = listen(&url, &key, &topics, &heard, &call);
    let (outcome, ()) = tokio::join!(listening, async {
        // Once the set has begun, the user presses Stop.
        while heard.columns.lock().unwrap().is_empty() {
            tokio::task::yield_now().await;
        }
        stopper.cancel();
    });
    assert_eq!(outcome.unwrap_err().code, ErrorCode::Cancelled);

    // Nobody there: refused before any set begins.
    let closed = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = closed.local_addr().unwrap().port();
    drop(closed);
    let error = listen(
        &format!("http://127.0.0.1:{port}"),
        &Secret::new("k"),
        &topics,
        &Heard::default(),
        &options(),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::Connect);
}

#[test]
fn a_supabase_host_or_pooler_user_names_the_project_and_nothing_else_does() {
    let config = |host: &str, user: &str| ConnectionConfig {
        engine: "postgres".into(),
        target: Target::Network {
            host: host.into(),
            port: None,
            user: Some(user.into()),
            password: Secret::default(),
            database: None,
            tls: TlsMode::Require,
        },
        options: BTreeMap::new(),
        ssh: None,
        dial: None,
    };
    assert_eq!(
        project_url(&config("db.abcdefghij.supabase.co", "postgres")).as_deref(),
        Some("https://abcdefghij.supabase.co")
    );
    assert_eq!(
        project_url(&config(
            "aws-0-ap-southeast-1.pooler.supabase.com",
            "postgres.abcdefghij"
        ))
        .as_deref(),
        Some("https://abcdefghij.supabase.co")
    );
    assert_eq!(project_url(&config("aws-0-x.pooler.supabase.com", "postgres")), None);
    assert_eq!(project_url(&config("localhost", "postgres")), None);
    assert_eq!(project_url(&config("db.evil/x.supabase.co", "postgres")), None);

    assert_eq!(
        socket_url("https://abc.supabase.co/", &Secret::new("ey.J-x_y")).unwrap(),
        "wss://abc.supabase.co/realtime/v1/websocket?apikey=ey.J-x_y&vsn=1.0.0"
    );
    assert_eq!(
        socket_url("abc.supabase.co", &Secret::new("k")).unwrap_err().code,
        ErrorCode::ConfigInvalid
    );
    assert_eq!(
        socket_url("https://abc.supabase.co", &Secret::new(""))
            .unwrap_err()
            .code,
        ErrorCode::ConfigInvalid
    );
}
