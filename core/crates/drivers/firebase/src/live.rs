//! The Realtime Database's changes for the Live tab
//! (`docs/decisions/0027-firebase.md`, Live): each path is a stream of
//! server-sent events (`Accept: text/event-stream`). The first `put` of a
//! stream is the path's value when it opened, not a change, and is not a
//! row; every `put` and `patch` after it is one.
//!
//! The access token in the stream's address ends after an hour: the server
//! then sends `auth_revoked`, and the stream opens again with a new token,
//! its first `put` again skipped.

use futures_util::StreamExt;
use kemable_core::catalog::ColumnMeta;
use kemable_core::live::{LiveTopic, LiveTopicKind};
use kemable_core::sink::{ExecOutcome, ResultSink};
use kemable_core::value::{Value, ValueKind};
use kemable_core::{CallOptions, DriverError, ErrorCode, Result};
use serde_json::Value as Json;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};

use crate::api::Firebase;
use crate::values::rtdb_value;

/// What a path's stream hands the listener.
enum Arrival {
    /// The stream is open and its first value read: nothing before this is missed.
    Ready,
    /// A change: the path it happened at, `put` or `patch`, and the data.
    Change(String, String, Json),
    Failed(DriverError),
}

fn columns() -> Vec<ColumnMeta> {
    let column = |name: &str, type_name: &str, kind| ColumnMeta {
        name: name.to_owned(),
        type_name: type_name.to_owned(),
        kind,
    };
    vec![
        column("received", "timestamptz", ValueKind::TimestampTz),
        column("path", "text", ValueKind::Text),
        column("event", "text", ValueKind::Text),
        column("data", "json", ValueKind::Json),
    ]
}

/// `base` and an event's `path` (relative to it) as one path.
pub(crate) fn joined(base: &str, under: &str) -> String {
    let base = base.trim_matches('/');
    let under = under.trim_matches('/');
    match (base.is_empty(), under.is_empty()) {
        (true, true) => "/".to_owned(),
        (true, false) => format!("/{under}"),
        (false, true) => format!("/{base}"),
        (false, false) => format!("/{base}/{under}"),
    }
}

/// One server-sent event's `event:` and `data:` lines, once a blank line ends it.
#[derive(Default)]
pub(crate) struct EventReader {
    pending: String,
    event: String,
    data: String,
}

impl EventReader {
    /// Feeds bytes; returns each event they complete, as its name and data.
    pub(crate) fn feed(&mut self, bytes: &[u8]) -> Vec<(String, String)> {
        self.pending.push_str(&String::from_utf8_lossy(bytes));
        let mut done = Vec::new();
        while let Some(at) = self.pending.find('\n') {
            let line: String = self.pending.drain(..=at).collect();
            let line = line.trim_end_matches(['\n', '\r']);
            if line.is_empty() {
                if !self.event.is_empty() || !self.data.is_empty() {
                    done.push((std::mem::take(&mut self.event), std::mem::take(&mut self.data)));
                }
            } else if let Some(name) = line.strip_prefix("event:") {
                self.event = name.trim().to_owned();
            } else if let Some(data) = line.strip_prefix("data:") {
                if !self.data.is_empty() {
                    self.data.push('\n');
                }
                self.data.push_str(data.trim_start());
            }
        }
        done
    }
}

/// Streams one path until the listener goes, opening it again when the token ends.
async fn stream(firebase: &Firebase, path: String, arrived: UnboundedSender<Arrival>) {
    let mut ready_sent = false;
    loop {
        let query = match firebase.rtdb_query().await {
            Ok(query) => query,
            Err(error) => {
                let _ = arrived.send(Arrival::Failed(error));
                return;
            }
        };
        let response = firebase
            .http()
            .get(firebase.rtdb_url(&path))
            .query(&query)
            .header("accept", "text/event-stream")
            .send()
            .await;
        let response = match response {
            Ok(response) if response.status().is_success() => response,
            Ok(response) => {
                let status = response.status();
                let text = response.text().await.unwrap_or_default();
                let words = serde_json::from_str::<Json>(&text)
                    .ok()
                    .and_then(|body| body.get("error").and_then(Json::as_str).map(str::to_owned))
                    .unwrap_or_else(|| format!("HTTP {status}"));
                let code = if matches!(status.as_u16(), 401 | 403) {
                    ErrorCode::Auth
                } else {
                    ErrorCode::Query
                };
                let _ = arrived.send(Arrival::Failed(DriverError::new(code, words)));
                return;
            }
            Err(error) => {
                let _ = arrived.send(Arrival::Failed(DriverError::new(ErrorCode::Connect, error.to_string())));
                return;
            }
        };
        let mut bytes = response.bytes_stream();
        let mut reader = EventReader::default();
        let mut first = true;
        let mut renew = false;
        while let Some(chunk) = bytes.next().await {
            let chunk = match chunk {
                Ok(chunk) => chunk,
                Err(error) => {
                    let _ = arrived.send(Arrival::Failed(DriverError::new(ErrorCode::Lost, error.to_string())));
                    return;
                }
            };
            for (event, data) in reader.feed(&chunk) {
                match event.as_str() {
                    "put" | "patch" => {
                        let body: Json = serde_json::from_str(&data).unwrap_or(Json::Null);
                        if first && event == "put" {
                            first = false;
                            if !ready_sent {
                                ready_sent = true;
                                if arrived.send(Arrival::Ready).is_err() {
                                    return;
                                }
                            }
                            continue;
                        }
                        let at = joined(&path, body.get("path").and_then(Json::as_str).unwrap_or("/"));
                        let value = body.get("data").cloned().unwrap_or(Json::Null);
                        if arrived.send(Arrival::Change(at, event, value)).is_err() {
                            return;
                        }
                    }
                    "auth_revoked" => renew = true,
                    "cancel" => {
                        let _ = arrived.send(Arrival::Failed(DriverError::new(
                            ErrorCode::Query,
                            format!("{path}: the rules no longer let this account read it ({data})"),
                        )));
                        return;
                    }
                    // keep-alive, and anything new.
                    _ => {}
                }
            }
            if renew {
                break;
            }
        }
        if !renew {
            let _ = arrived.send(Arrival::Failed(DriverError::new(
                ErrorCode::Lost,
                format!("{path}: the server closed the stream"),
            )));
            return;
        }
    }
}

/// A change as a row, received now.
fn row(path: String, event: String, data: &Json) -> [Value; 4] {
    let micros = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| i64::try_from(elapsed.as_micros()).unwrap_or(i64::MAX));
    [
        Value::TimestampTz {
            micros,
            offset_seconds: 0,
        },
        Value::Text(path),
        Value::Text(event),
        rtdb_value(data),
    ]
}

pub(crate) async fn listen(
    firebase: &Firebase,
    topics: &[LiveTopic],
    sink: &dyn ResultSink,
    options: &CallOptions,
) -> Result<ExecOutcome> {
    if topics.iter().any(|topic| topic.kind != LiveTopicKind::Path) {
        return Err(DriverError::unsupported("Firebase listens to Realtime Database paths"));
    }
    let paths: Vec<String> = topics
        .iter()
        .map(|topic| topic.name.trim_matches('/').to_owned())
        .collect();
    if paths.is_empty() {
        return Err(DriverError::config("no path to listen to"));
    }
    let (arrived, mut arrivals) = unbounded_channel();
    let streams =
        futures_util::future::join_all(paths.iter().map(|path| stream(firebase, path.clone(), arrived.clone())));
    drop(arrived);
    let listening = async {
        // Until every stream has read its first value. A change a stream that
        // is open already hears meanwhile is kept for when the set begins.
        let mut waiting = paths.len();
        let mut early = Vec::new();
        while waiting > 0 {
            match arrivals.recv().await {
                Some(Arrival::Ready) => waiting -= 1,
                Some(Arrival::Failed(error)) => return Err(error),
                Some(Arrival::Change(path, event, data)) => early.push(row(path, event, &data)),
                None => {
                    return Err(DriverError::new(
                        ErrorCode::Lost,
                        "the streams ended before they opened",
                    ));
                }
            }
        }
        sink.notice("A path's value when Listen started is not a row: only what changes after it is.");
        sink.begin_set(columns());
        for values in early {
            if !sink.push_rows(&values) {
                return Ok(ExecOutcome { limited: true });
            }
        }
        loop {
            match arrivals.recv().await {
                Some(Arrival::Change(path, event, data)) => {
                    if !sink.push_rows(&row(path, event, &data)) {
                        return Ok(ExecOutcome { limited: true });
                    }
                }
                Some(Arrival::Ready) => {}
                Some(Arrival::Failed(error)) => return Err(error),
                None => return Err(DriverError::new(ErrorCode::Lost, "the streams ended")),
            }
        }
    };
    let outcome = tokio::select! {
        biased;
        () = options.stopped() => Err(options.stop_reason()),
        outcome = listening => outcome,
        // A stream that ends has said why first; `listening` reads it, and then
        // sees the channel close once every stream is done.
        () = async {
            streams.await;
            std::future::pending::<()>().await;
        } => Err(DriverError::new(ErrorCode::Lost, "the streams ended")),
    };
    sink.end_set();
    outcome
}
