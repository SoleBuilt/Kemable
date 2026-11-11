use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use paboable_core::config::{ConnectionConfig, Target};
use paboable_core::connection::Connection;
use paboable_core::driver::Driver;
use paboable_core::engine::EngineInfo;
use paboable_core::registry::DriverRegistry;
use paboable_core::{CallOptions, DriverError, Result};
use paboable_store::ResultStore;
use tokio_util::sync::CancellationToken;

use crate::catalog::CatalogCache;

/// Neon wakes a suspended compute in a few seconds; leave room for it.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// What the app holds open. The UI names each thing by an id from one
/// counter, so an id is never reused within a launch.
pub struct AppState {
    pub registry: DriverRegistry,
    /// The query history, opened in the app's folder at startup.
    pub history: std::sync::OnceLock<Arc<paboable_storage::history::QueryHistory>>,
    ids: AtomicU64,
    servers: Mutex<HashMap<u64, Arc<OpenServer>>>,
    results: Mutex<HashMap<u64, ResultEntry>>,
    runs: Mutex<HashMap<u64, CancellationToken>>,
    /// What the editor's completion has read of each server's catalog.
    pub catalog: CatalogCache,
}

/// A result set and where it came from.
type ResultEntry = (Arc<ResultStore>, Option<ResultOrigin>);

/// Where a result set came from, so a sort can run its statement again
/// (`docs/decisions/0005-sorting-a-result.md`).
#[derive(Debug, Clone)]
pub struct ResultOrigin {
    pub database: Option<String>,
    /// `None` for a console's command, which only the core sorts.
    pub grammar: Option<paboable_core::sql::SqlGrammar>,
    pub statement: String,
    /// The run held this one statement and nothing else, so running it again
    /// alone means the same thing.
    pub alone: bool,
    pub row_limit: Option<u64>,
    /// The statement inside a SQL Server batch that made this set
    /// (`paboable_core::sql::batch_queries`), for "Open <table> to edit".
    pub part: Option<String>,
}

/// A connection the user opened: one server, and a session per database
/// in use on it. PostgreSQL binds a session to one database, so each
/// database the tree expands or a run uses gets its own, opened on first use.
/// An engine without databases (SQLite) has one session, under `home`.
pub struct OpenServer {
    /// What the UI calls it: the saved name, or host and port.
    pub title: String,
    pub driver: Arc<dyn Driver>,
    pub engine: &'static EngineInfo,
    config: ConnectionConfig,
    /// The database the first session opened on; empty for an engine without databases.
    pub home: String,
    sessions: tokio::sync::Mutex<HashMap<String, Arc<dyn Connection>>>,
}

impl OpenServer {
    pub fn new(driver: Arc<dyn Driver>, config: ConnectionConfig, first: Arc<dyn Connection>, title: String) -> Self {
        let engine = first.engine();
        let home = if engine.has_databases {
            first.current_database().unwrap_or_default()
        } else {
            String::new()
        };
        Self {
            title,
            driver,
            engine,
            config,
            sessions: tokio::sync::Mutex::new(HashMap::from([(home.clone(), first)])),
            home,
        }
    }

    /// The session on `database`, opened now if it is not open yet. `None`,
    /// an empty name, or an engine without databases means the home session.
    pub async fn session(&self, database: Option<&str>) -> Result<Arc<dyn Connection>> {
        let key = match database {
            Some(name) if self.engine.has_databases && !name.is_empty() => name.to_owned(),
            _ => self.home.clone(),
        };
        // One lock across the open, so two callers asking for the same
        // database at once share one session instead of racing to open two.
        let mut sessions = self.sessions.lock().await;
        if let Some(session) = sessions.get(&key) {
            return Ok(Arc::clone(session));
        }
        let config = with_database(&self.config, &key);
        let session = self
            .driver
            .connect(&config, CallOptions::default().with_timeout(CONNECT_TIMEOUT))
            .await?;
        sessions.insert(key, Arc::clone(&session));
        Ok(session)
    }

    pub async fn close(&self) {
        let sessions: Vec<_> = self.sessions.lock().await.drain().map(|(_, session)| session).collect();
        for session in sessions {
            let _ = session.close().await;
        }
    }

    /// Closes the app's own session on `database`, so a `DROP DATABASE` is
    /// not refused because Paboable itself is still connected to it. The home
    /// session stays: the tree and the database list go through it.
    pub async fn close_session(&self, database: &str) {
        if database == self.home {
            return;
        }
        let session = self.sessions.lock().await.remove(database);
        if let Some(session) = session {
            let _ = session.close().await;
        }
    }
}

/// The same server, pointed at another database.
fn with_database(config: &ConnectionConfig, database: &str) -> ConnectionConfig {
    let mut config = config.clone();
    match &mut config.target {
        Target::Network { database: target, .. } => *target = Some(database.to_owned()),
        // D1 names its database in the options; the target is the account.
        Target::Http { .. } => {
            config.options.insert("database".to_owned(), database.to_owned());
        }
        Target::File { .. } => {}
    }
    config
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    // Every map stays consistent between statements, so a panic elsewhere
    // never leaves one half-written.
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl AppState {
    pub fn new(registry: DriverRegistry) -> Self {
        Self {
            registry,
            history: std::sync::OnceLock::new(),
            ids: AtomicU64::new(1),
            servers: Mutex::default(),
            results: Mutex::default(),
            runs: Mutex::default(),
            catalog: CatalogCache::default(),
        }
    }

    fn next_id(&self) -> u64 {
        self.ids.fetch_add(1, Ordering::Relaxed)
    }

    pub fn add_server(&self, server: Arc<OpenServer>) -> u64 {
        let id = self.next_id();
        lock(&self.servers).insert(id, server);
        id
    }

    pub fn server(&self, id: u64) -> Result<Arc<OpenServer>> {
        lock(&self.servers)
            .get(&id)
            .cloned()
            .ok_or_else(|| DriverError::not_found("connection"))
    }

    pub fn remove_server(&self, id: u64) -> Option<Arc<OpenServer>> {
        lock(&self.servers).remove(&id)
    }

    pub fn add_result(&self, store: Arc<ResultStore>, origin: Option<ResultOrigin>) -> u64 {
        let id = self.next_id();
        lock(&self.results).insert(id, (store, origin));
        id
    }

    pub fn origin(&self, id: u64) -> Option<ResultOrigin> {
        lock(&self.results).get(&id).and_then(|(_, origin)| origin.clone())
    }

    /// The sets of a batch whose results did not match its reading: their parts are wrong.
    pub fn forget_parts(&self, ids: &[u64]) {
        let mut results = lock(&self.results);
        for id in ids {
            if let Some((_, Some(origin))) = results.get_mut(id) {
                origin.part = None;
            }
        }
    }

    pub fn result(&self, id: u64) -> Result<Arc<ResultStore>> {
        lock(&self.results)
            .get(&id)
            .map(|(store, _)| Arc::clone(store))
            .ok_or_else(|| DriverError::not_found("result"))
    }

    pub fn remove_result(&self, id: u64) {
        lock(&self.results).remove(&id);
    }

    pub fn add_run(&self, token: CancellationToken) -> u64 {
        let id = self.next_id();
        lock(&self.runs).insert(id, token);
        id
    }

    pub fn cancel_run(&self, id: u64) {
        if let Some(token) = lock(&self.runs).get(&id) {
            token.cancel();
        }
    }

    pub fn remove_run(&self, id: u64) {
        lock(&self.runs).remove(&id);
    }
}
