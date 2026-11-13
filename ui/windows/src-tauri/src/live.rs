//! A Live tab's listening (`docs/decisions/0020-live-tab.md`): the driver
//! streams events into a result set the grid reads like any other, and the
//! UI is told how many there are, never each one.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use paboable_core::catalog::ColumnMeta;
use paboable_core::live::LiveTopic;
use paboable_core::sink::ResultSink;
use paboable_core::value::Value;
use paboable_core::{CallOptions, DriverError, ErrorCode};
use paboable_store::ResultStore;
use serde::Serialize;
use tauri::ipc::Channel;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

use crate::state::{AppState, OpenServer};

/// The most events a Live tab keeps. One past it, the listening stops and
/// says so: a tab left open on a busy channel must not fill memory.
pub const MAX_EVENTS: u64 = 100_000;

/// Event counts reach the UI at most this often, as a run's row counts do.
const EVENTS_INTERVAL: Duration = Duration::from_millis(50);

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum LiveEvent {
    /// The server took every topic: what arrives from now on is a row of `result`.
    Listening {
        result: u64,
        columns: Vec<ColumnMeta>,
    },
    /// How many events the result holds, and how many its filter shows.
    Events {
        total: u64,
        shown: u64,
    },
    Notice {
        text: String,
    },
    /// The listening ended: stopped by the user (no error), at the cap
    /// (`limited`), or because it failed. The result stays until it is released.
    Finished {
        total: u64,
        shown: u64,
        limited: bool,
        error: Option<DriverError>,
    },
}

pub struct Live {
    pub id: u64,
    pub server: Arc<OpenServer>,
    /// The database to listen in; `None` for the one the connection opened on.
    pub database: Option<String>,
    pub topics: Vec<LiveTopic>,
    pub max_events: u64,
    pub cancel: CancellationToken,
    pub channel: Channel<LiveEvent>,
}

/// The sink a driver's `listen` writes into.
struct LiveSink {
    state: Arc<AppState>,
    channel: Channel<LiveEvent>,
    max_events: u64,
    store: Mutex<Option<Arc<ResultStore>>>,
    /// Told of every event; the reporter wakes on it.
    changed: Notify,
}

impl LiveSink {
    fn send(&self, event: LiveEvent) {
        // A closed window drops the channel; the listening then ends when its server closes.
        let _ = self.channel.send(event);
    }

    fn store(&self) -> Option<Arc<ResultStore>> {
        self.store.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    /// The events held and the events shown.
    fn counts(&self) -> (u64, u64) {
        self.store().map_or((0, 0), |store| {
            let state = store.state();
            (state.total_rows, state.shown_rows)
        })
    }
}

impl ResultSink for LiveSink {
    fn begin_set(&self, columns: Vec<ColumnMeta>) {
        let store = Arc::new(ResultStore::new(columns.clone(), Some(self.max_events)));
        let result = self.state.add_result(Arc::clone(&store), None);
        *self.store.lock().unwrap_or_else(PoisonError::into_inner) = Some(store);
        self.send(LiveEvent::Listening { result, columns });
    }

    fn push_rows(&self, values: &[Value]) -> bool {
        let more = self.store().is_some_and(|store| store.push_rows(values));
        self.changed.notify_one();
        more
    }

    fn end_set(&self) {}

    fn command_done(&self, _rows_affected: Option<u64>) {}

    fn notice(&self, text: &str) {
        self.send(LiveEvent::Notice { text: text.to_owned() });
    }
}

/// Sends the counts after an event, then waits out the interval. An event
/// that arrives while it waits is counted when it wakes, so the last count
/// always goes out, however long the channel then stays quiet.
async fn report(sink: Arc<LiveSink>) {
    loop {
        sink.changed.notified().await;
        let (total, shown) = sink.counts();
        sink.send(LiveEvent::Events { total, shown });
        tokio::time::sleep(EVENTS_INTERVAL).await;
    }
}

/// Listens until the run is cancelled, its server closes, the cap is reached
/// or the link fails, and says which with the last event.
pub async fn listen(state: Arc<AppState>, live: Live) {
    let sink = Arc::new(LiveSink {
        state: Arc::clone(&state),
        channel: live.channel.clone(),
        max_events: live.max_events,
        store: Mutex::new(None),
        changed: Notify::new(),
    });
    let reporter = tokio::spawn(report(Arc::clone(&sink)));
    let config = live.server.config_for(live.database.as_deref());
    let shared: Arc<dyn ResultSink> = sink.clone();
    let outcome = live
        .server
        .driver
        .listen(&config, &live.topics, shared, CallOptions::new(live.cancel.clone()))
        .await;
    reporter.abort();
    state.remove_run(live.id);
    if let Some(store) = sink.store() {
        store.finish();
    }

    let (total, shown) = sink.counts();
    let (limited, error) = match outcome {
        Ok(outcome) => (outcome.limited, None),
        // A stop is how a listening normally ends.
        Err(error) if error.code == ErrorCode::Cancelled => (false, None),
        Err(error) => (false, Some(error)),
    };
    sink.send(LiveEvent::Finished {
        total,
        shown,
        limited,
        error,
    });
}
