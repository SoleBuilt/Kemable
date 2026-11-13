//! Supabase Realtime for the Live tab (`docs/decisions/0025-supabase-realtime.md`):
//! a project's changes to its tables and its broadcast messages, heard over
//! the project's WebSocket (`/realtime/v1/websocket`, Phoenix channels, JSON).
//! Each change or message is one row, as a `NOTIFY` is for `LISTEN`.
//!
//! Realtime is a service beside the database, reached with the project's URL
//! and an API key, not through the PostgreSQL protocol: the app calls
//! [`listen`] for a Supabase connection, with the URL [`project_url`] reads
//! from its host.

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use futures_util::{SinkExt, StreamExt};
use kemable_core::catalog::ColumnMeta;
use kemable_core::config::{ConnectionConfig, Secret, Target};
use kemable_core::live::{LiveTopic, LiveTopicKind};
use kemable_core::sink::{ExecOutcome, ResultSink};
use kemable_core::value::{Value, ValueKind};
use kemable_core::{CallOptions, DriverError, ErrorCode, Result};
use rustls::ClientConfig;
use serde_json::{Value as Json, json};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{Connector, connect_async_tls_with_config};

/// How often the socket says it is there, as Supabase's own clients do; an
/// answer missing when the next one is due ends the listening as lost.
const HEARTBEAT: Duration = Duration::from_secs(25);

/// The channel every table change is heard on.
const CHANGES: &str = "realtime:kemable";

/// The project's URL for a Supabase connection: `https://<ref>.supabase.co`,
/// read from a direct host (`db.<ref>.supabase.co`) or a pooler's user
/// (`postgres.<ref>` at `*.pooler.supabase.com`). `None` for any other server.
pub fn project_url(config: &ConnectionConfig) -> Option<String> {
    let Target::Network { host, user, .. } = &config.target else {
        return None;
    };
    let host = host.trim().to_ascii_lowercase();
    let project = if let Some(rest) = host
        .strip_prefix("db.")
        .and_then(|rest| rest.strip_suffix(".supabase.co"))
    {
        rest.to_owned()
    } else if host.ends_with(".pooler.supabase.com") {
        user.as_deref()?.trim().strip_prefix("postgres.")?.to_ascii_lowercase()
    } else {
        return None;
    };
    let plain = !project.is_empty() && project.chars().all(|c| c.is_ascii_alphanumeric());
    plain.then(|| format!("https://{project}.supabase.co"))
}

/// What one join asks for: a table's changes (`schema.table`, `schema.*`
/// for a whole schema, `*` for every table), or a broadcast channel.
fn changes_filter(name: &str) -> Json {
    match name.split_once('.') {
        Some((schema, "*")) => json!({ "event": "*", "schema": schema }),
        Some((schema, table)) => json!({ "event": "*", "schema": schema, "table": table }),
        None if name == "*" => json!({ "event": "*", "schema": "*" }),
        None => json!({ "event": "*", "schema": "public", "table": name }),
    }
}

/// The socket's address: `wss` for `https`, with the key and the protocol version.
fn socket_url(url: &str, key: &Secret) -> Result<String> {
    let url = url.trim().trim_end_matches('/');
    let (scheme, rest) = if let Some(rest) = url.strip_prefix("https://") {
        ("wss", rest)
    } else if let Some(rest) = url.strip_prefix("http://") {
        ("ws", rest)
    } else {
        return Err(DriverError::config(format!("{url} is not an http or https URL")));
    };
    let key: String = key
        .expose()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        .collect();
    if key.is_empty() {
        return Err(DriverError::config("Realtime needs the project's API key"));
    }
    Ok(format!(
        "{scheme}://{rest}/realtime/v1/websocket?apikey={key}&vsn=1.0.0"
    ))
}

fn tls() -> Result<Arc<ClientConfig>> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let tls_error = |error: rustls::Error| DriverError::new(ErrorCode::Tls, error.to_string());
    let verifier = rustls_platform_verifier::Verifier::new(provider.clone()).map_err(tls_error)?;
    Ok(Arc::new(
        ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .map_err(tls_error)?
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(verifier))
            .with_no_client_auth(),
    ))
}

fn columns() -> Vec<ColumnMeta> {
    let column = |name: &str, type_name: &str, kind| ColumnMeta {
        name: name.to_owned(),
        type_name: type_name.to_owned(),
        kind,
    };
    vec![
        column("received", "timestamptz", ValueKind::TimestampTz),
        column("channel", "text", ValueKind::Text),
        column("event", "text", ValueKind::Text),
        column("committed", "text", ValueKind::Text),
        column("record", "json", ValueKind::Json),
        column("old_record", "json", ValueKind::Json),
    ]
}

fn now() -> Value {
    let micros = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| i64::try_from(elapsed.as_micros()).unwrap_or(i64::MAX));
    Value::TimestampTz {
        micros,
        offset_seconds: 0,
    }
}

fn json_or_null(value: Option<&Json>) -> Value {
    match value {
        None | Some(Json::Null) => Value::Null,
        // An empty old record is what a table without REPLICA IDENTITY FULL sends: nothing known.
        Some(Json::Object(map)) if map.is_empty() => Value::Null,
        Some(other) => Value::Json(other.to_string()),
    }
}

fn text_or_null(value: Option<&Json>) -> Value {
    value
        .and_then(Json::as_str)
        .map_or(Value::Null, |text| Value::Text(text.to_owned()))
}

/// One message as a row: a table's change, or a broadcast. Anything else
/// (presence, replies) is not an event.
fn row(message: &Json) -> Option<[Value; 6]> {
    let payload = message.get("payload")?;
    match message.get("event")?.as_str()? {
        "postgres_changes" => {
            let data = payload.get("data")?;
            let schema = data.get("schema").and_then(Json::as_str).unwrap_or("");
            let table = data.get("table").and_then(Json::as_str).unwrap_or("");
            let kind = data.get("type").or_else(|| data.get("eventType"));
            Some([
                now(),
                Value::Text(format!("{schema}.{table}")),
                text_or_null(kind),
                text_or_null(data.get("commit_timestamp")),
                json_or_null(data.get("record")),
                json_or_null(data.get("old_record")),
            ])
        }
        "broadcast" => {
            let topic = message.get("topic").and_then(Json::as_str).unwrap_or("");
            Some([
                now(),
                Value::Text(topic.strip_prefix("realtime:").unwrap_or(topic).to_owned()),
                text_or_null(payload.get("event")),
                Value::Null,
                json_or_null(payload.get("payload")),
                Value::Null,
            ])
        }
        _ => None,
    }
}

fn lost(error: impl std::fmt::Display) -> DriverError {
    DriverError::new(ErrorCode::Lost, error.to_string())
}

/// Why the server refused a join or a subscription, as it said it.
fn refusal(payload: &Json) -> String {
    payload
        .pointer("/response/reason")
        .or_else(|| payload.get("message"))
        .or_else(|| payload.get("reason"))
        .map_or_else(
            || payload.to_string(),
            |reason| reason.as_str().map_or_else(|| reason.to_string(), str::to_owned),
        )
}

/// Listens to `topics` (tables and broadcast channels) on the project at
/// `url` with `key`, until the sink is full, a stop, or the socket drops.
/// The set begins once the server has taken every join and, for tables,
/// said the subscription to the database is in place.
pub async fn listen(
    url: &str,
    key: &Secret,
    topics: &[LiveTopic],
    sink: &dyn ResultSink,
    options: &CallOptions,
) -> Result<ExecOutcome> {
    let tables: Vec<Json> = topics
        .iter()
        .filter(|topic| topic.kind == LiveTopicKind::Table)
        .map(|topic| changes_filter(&topic.name))
        .collect();
    let broadcasts: Vec<&str> = topics
        .iter()
        .filter(|topic| topic.kind == LiveTopicKind::Broadcast)
        .map(|topic| topic.name.as_str())
        .collect();
    if tables.is_empty() && broadcasts.is_empty() {
        return Err(DriverError::config("no table or broadcast channel to listen to"));
    }
    let address = socket_url(url, key)?;
    let connector = if address.starts_with("wss://") {
        Connector::Rustls(tls()?)
    } else {
        Connector::Plain
    };
    let (mut socket, _) = options
        .run(async {
            connect_async_tls_with_config(address.as_str(), None, true, Some(connector))
                .await
                .map_err(|error| match error {
                    tokio_tungstenite::tungstenite::Error::Http(response)
                        if response.status().as_u16() == 401 || response.status().as_u16() == 403 =>
                    {
                        DriverError::new(
                            ErrorCode::Auth,
                            format!("the API key was refused ({})", response.status()),
                        )
                    }
                    other => DriverError::new(ErrorCode::Connect, other.to_string()),
                })
        })
        .await?;

    // The joins: one channel for every table's changes, one per broadcast channel.
    let mut joins: Vec<(String, u64)> = Vec::new();
    let mut next_ref = 0u64;
    let mut send_join = |topic: String, config: Json| {
        next_ref += 1;
        let message = json!({
            "topic": topic,
            "event": "phx_join",
            "payload": { "config": config, "access_token": key.expose() },
            "ref": next_ref.to_string(),
            "join_ref": next_ref.to_string(),
        });
        joins.push((topic, next_ref));
        message
    };
    let mut outgoing = Vec::new();
    if !tables.is_empty() {
        outgoing.push(send_join(
            CHANGES.to_owned(),
            json!({ "broadcast": { "self": false, "ack": false }, "presence": { "key": "" }, "postgres_changes": tables, "private": false }),
        ));
    }
    for name in &broadcasts {
        outgoing.push(send_join(
            format!("realtime:{name}"),
            json!({ "broadcast": { "self": false, "ack": false }, "presence": { "key": "" }, "postgres_changes": [], "private": false }),
        ));
    }
    for message in outgoing {
        socket.send(Message::text(message.to_string())).await.map_err(lost)?;
    }

    // Until every join is answered, and the table changes are subscribed.
    let mut waiting_joins = joins.len();
    let mut waiting_changes = !tables.is_empty();
    options
        .run(async {
            while waiting_joins > 0 || waiting_changes {
                let Some(frame) = socket.next().await else {
                    return Err(lost("the server closed the socket before it took the topics"));
                };
                let Message::Text(text) = frame.map_err(lost)? else {
                    continue;
                };
                let Ok(message) = serde_json::from_str::<Json>(&text) else {
                    continue;
                };
                let event = message.get("event").and_then(Json::as_str).unwrap_or("");
                let payload = message.get("payload").cloned().unwrap_or(Json::Null);
                match event {
                    "phx_reply" => {
                        let reference = message
                            .get("ref")
                            .and_then(Json::as_str)
                            .and_then(|text| text.parse::<u64>().ok());
                        if joins.iter().any(|(_, join)| Some(*join) == reference) {
                            if payload.get("status").and_then(Json::as_str) != Some("ok") {
                                return Err(DriverError::new(ErrorCode::Query, refusal(&payload)));
                            }
                            waiting_joins -= 1;
                        }
                    }
                    "system" if message.get("topic").and_then(Json::as_str) == Some(CHANGES) => {
                        if payload.get("status").and_then(Json::as_str) == Some("ok") {
                            waiting_changes = false;
                        } else {
                            return Err(DriverError::new(ErrorCode::Query, refusal(&payload)));
                        }
                    }
                    "phx_error" | "phx_close" => return Err(DriverError::new(ErrorCode::Query, refusal(&payload))),
                    _ => {}
                }
            }
            Ok(())
        })
        .await?;

    sink.begin_set(columns());
    let mut heartbeat = tokio::time::interval_at(tokio::time::Instant::now() + HEARTBEAT, HEARTBEAT);
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut unanswered: Option<String> = None;
    let outcome = loop {
        tokio::select! {
            biased;
            () = options.stopped() => break Err(options.stop_reason()),
            _ = heartbeat.tick() => {
                if unanswered.is_some() {
                    break Err(lost("the server did not answer the heartbeat"));
                }
                next_ref += 1;
                let reference = next_ref.to_string();
                let beat = json!({ "topic": "phoenix", "event": "heartbeat", "payload": {}, "ref": reference });
                if let Err(error) = socket.send(Message::text(beat.to_string())).await {
                    break Err(lost(error));
                }
                unanswered = Some(reference);
            }
            frame = socket.next() => {
                let message = match frame {
                    None => break Err(lost("the server closed the socket")),
                    Some(Err(error)) => break Err(lost(error)),
                    Some(Ok(Message::Close(frame))) => {
                        break Err(lost(frame.map_or_else(|| "the server closed the socket".to_owned(), |frame| frame.reason.to_string())));
                    }
                    Some(Ok(Message::Text(text))) => match serde_json::from_str::<Json>(&text) {
                        Ok(message) => message,
                        Err(_) => continue,
                    },
                    Some(Ok(_)) => continue,
                };
                let event = message.get("event").and_then(Json::as_str).unwrap_or("");
                if event == "phx_reply" && message.get("topic").and_then(Json::as_str) == Some("phoenix") {
                    if message.get("ref").and_then(Json::as_str) == unanswered.as_deref() {
                        unanswered = None;
                    }
                    continue;
                }
                if matches!(event, "phx_error" | "phx_close") {
                    let payload = message.get("payload").cloned().unwrap_or(Json::Null);
                    break Err(lost(refusal(&payload)));
                }
                if event == "system" {
                    let payload = message.get("payload").cloned().unwrap_or(Json::Null);
                    if payload.get("status").and_then(Json::as_str) != Some("ok") {
                        sink.notice(&refusal(&payload));
                    }
                    continue;
                }
                if let Some(values) = row(&message)
                    && !sink.push_rows(&values)
                {
                    break Ok(ExecOutcome { limited: true });
                }
            }
        }
    };
    sink.end_set();
    let _ = socket.close(None).await;
    outcome
}

#[cfg(test)]
mod tests;
