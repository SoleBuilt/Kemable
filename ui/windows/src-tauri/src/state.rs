use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use kemable_core::config::{ConnectionConfig, Target};
use kemable_core::connection::Connection;
use kemable_core::driver::Driver;
use kemable_core::engine::EngineInfo;
use kemable_core::registry::DriverRegistry;
use kemable_core::{CallOptions, DriverError, ErrorCode, Result};
use kemable_store::ResultStore;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

use crate::catalog::CatalogCache;

/// Neon wakes a suspended compute in a few seconds; leave room for it.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// A database's session unused this long is closed (`docs/roadmap.md`): each
/// PostgreSQL database touched holds two server connections until then.
pub const IDLE_AFTER: Duration = Duration::from_secs(10 * 60);

/// How often idle sessions are looked for.
const IDLE_CHECK: Duration = Duration::from_secs(60);

/// What the app holds open. The UI names each thing by an id from one
/// counter, so an id is never reused within a launch.
pub struct AppState {
    pub registry: DriverRegistry,
    /// The query history, opened in the app's folder at startup.
    pub history: std::sync::OnceLock<Arc<kemable_storage::history::QueryHistory>>,
    ids: AtomicU64,
    servers: Mutex<HashMap<u64, Arc<OpenServer>>>,
    results: Mutex<HashMap<u64, ResultEntry>>,
    runs: Mutex<HashMap<u64, CancellationToken>>,
    /// What the editor's completion has read of each server's catalog.
    pub catalog: CatalogCache,
    /// The SSH servers trusted, in the data folder's `known_hosts`, set at startup.
    pub host_keys: std::sync::OnceLock<Arc<kemable_tunnel::HostKeys>>,
    /// The most bytes one result set holds (`crate::memory`), as the row
    /// limit is the most rows; `None` keeps any size.
    pub result_bytes: Option<u64>,
}

/// A result set and where it came from.
type ResultEntry = (Arc<ResultStore>, Option<ResultOrigin>);

/// Where a result set came from, so a sort can run its statement again
/// (`docs/decisions/0005-sorting-a-result.md`).
#[derive(Debug, Clone)]
pub struct ResultOrigin {
    pub database: Option<String>,
    /// `None` for a console's command, which only the core sorts.
    pub grammar: Option<kemable_core::sql::SqlGrammar>,
    pub statement: String,
    /// The run held this one statement and nothing else, so running it again
    /// alone means the same thing.
    pub alone: bool,
    pub row_limit: Option<u64>,
    /// The statement inside a SQL Server batch that made this set
    /// (`kemable_core::sql::batch_queries`), for "Open <table> to edit".
    pub part: Option<String>,
}

/// A session being opened, as its callers wait for it.
type Opened = Result<Arc<dyn Connection>>;

/// One database's session in the map.
enum Slot {
    Open {
        session: Arc<dyn Connection>,
        /// When it was last handed out.
        used: Instant,
    },
    /// Opened by a task of its own: a slow connect blocks no other database,
    /// and each caller waits within its own deadline and Stop.
    Opening(watch::Receiver<Option<Opened>>),
}

/// A connection the user opened: one server, and a session per database
/// in use on it. PostgreSQL binds a session to one database, so each
/// database the tree expands or a run uses gets its own, opened on first use.
/// An engine without databases (SQLite) has one session, under `home`.
pub struct OpenServer {
    /// What the UI calls it: the saved name, or host and port.
    pub title: String,
    /// What names it across launches, for its history entries: `saved:<id>`,
    /// `file:<path>` or `<engine>:<title>` (`connection_key` in commands.rs).
    pub key: String,
    pub driver: Arc<dyn Driver>,
    pub engine: &'static EngineInfo,
    /// Its `dial` moves when an SSH tunnel is opened again.
    config: Mutex<ConnectionConfig>,
    /// The database the first session opened on; empty for an engine without databases.
    pub home: String,
    /// Never held across an await: a connect runs in a task of its own.
    sessions: Arc<Mutex<HashMap<String, Slot>>>,
    /// Fires when the connection closes. What listens on a link of its own
    /// (a Live tab) takes a child of it, so it stops with its server.
    pub closing: CancellationToken,
    /// The SSH tunnel every session and listener of this server goes through;
    /// it closes with the server, and opens again when its SSH session ended
    /// (`docs/decisions/0024-ssh-tunnels.md`).
    tunnel: tokio::sync::Mutex<Option<kemable_tunnel::Tunnel>>,
    /// The databases Live tabs listen in, and how many each: their sessions
    /// are not closed for being idle.
    listening: Mutex<HashMap<String, usize>>,
    /// The databases whose table tabs hold unsaved edits, as the UI says.
    edited: Mutex<HashSet<String>>,
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
            key: String::new(),
            driver,
            engine,
            config: Mutex::new(config),
            sessions: Arc::new(Mutex::new(HashMap::from([(
                home.clone(),
                Slot::Open {
                    session: first,
                    used: Instant::now(),
                },
            )]))),
            home,
            closing: CancellationToken::new(),
            tunnel: tokio::sync::Mutex::new(None),
            listening: Mutex::default(),
            edited: Mutex::default(),
        }
    }

    /// A Live tab starts listening in `database`; `stop_listening` with the
    /// key returned ends it.
    pub fn start_listening(&self, database: Option<&str>) -> String {
        let key = self.key(database);
        *lock(&self.listening).entry(key.clone()).or_default() += 1;
        key
    }

    pub fn stop_listening(&self, key: &str) {
        let mut listening = lock(&self.listening);
        if let Some(count) = listening.get_mut(key) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                listening.remove(key);
            }
        }
    }

    /// The databases whose table tabs hold unsaved edits now.
    pub fn keep_for_edits(&self, databases: Vec<String>) {
        *lock(&self.edited) = databases.into_iter().collect();
    }

    /// Closes the sessions unused for `idle`: never the home session (the
    /// tree and the database list go through it), nor one that a run, an
    /// export or a job holds, one in a database a Live tab listens in or
    /// whose table tab holds unsaved edits, nor one with an open transaction.
    /// Says how many it closed.
    pub async fn close_idle(&self, idle: Duration) -> usize {
        let candidates: Vec<(String, Arc<dyn Connection>, Instant)> = {
            let sessions = lock(&self.sessions);
            let listening = lock(&self.listening);
            let edited = lock(&self.edited);
            sessions
                .iter()
                .filter_map(|(key, slot)| match slot {
                    Slot::Open { session, used }
                        if *key != self.home
                            && used.elapsed() >= idle
                            // Only the map holds it: no call is using it.
                            && Arc::strong_count(session) == 1
                            && !listening.contains_key(key)
                            && !edited.contains(key) =>
                    {
                        Some((key.clone(), Arc::clone(session), *used))
                    }
                    _ => None,
                })
                .collect()
        };
        let mut closed = 0;
        for (key, session, seen) in candidates {
            let options = CallOptions::new(self.closing.child_token()).with_timeout(IDLE_PROBE_TIMEOUT);
            // A session that cannot answer is not worth keeping either.
            if session.in_transaction(options).await.unwrap_or(false) {
                continue;
            }
            let dropped = {
                let mut sessions = lock(&self.sessions);
                match sessions.get(&key) {
                    // Untouched since it was picked: this loop's clone and the map's.
                    Some(Slot::Open { session: held, used })
                        if Arc::ptr_eq(held, &session) && *used == seen && Arc::strong_count(held) == 2 =>
                    {
                        sessions.remove(&key)
                    }
                    _ => None,
                }
            };
            if dropped.is_some() {
                close_later(session);
                closed += 1;
            }
        }
        closed
    }

    /// A Supabase project's URL, when this server is one.
    pub fn realtime_url(&self) -> Option<String> {
        let config = lock(&self.config);
        (config.engine == "postgres")
            .then(|| kemable_driver_postgres::realtime::project_url(&config))
            .flatten()
    }

    /// Names it across launches (`Self::key`).
    #[must_use]
    pub fn with_key(mut self, key: String) -> Self {
        self.key = key;
        self
    }

    /// Keeps `tunnel` open as long as this server is.
    #[must_use]
    pub fn with_tunnel(mut self, tunnel: Option<kemable_tunnel::Tunnel>) -> Self {
        self.tunnel = tokio::sync::Mutex::new(tunnel);
        self
    }

    /// The config that reaches `database` on this server, for a link opened
    /// beside the sessions. `None`, an empty name, or an engine without
    /// databases means where the connection opened.
    pub fn config_for(&self, database: Option<&str>) -> ConnectionConfig {
        let config = lock(&self.config);
        match database {
            Some(name) if self.engine.has_databases && !name.is_empty() => with_database(&config, name),
            _ => config.clone(),
        }
    }

    /// The key a session is kept under: the database, or `home`.
    fn key(&self, database: Option<&str>) -> String {
        match database {
            Some(name) if self.engine.has_databases && !name.is_empty() => name.to_owned(),
            _ => self.home.clone(),
        }
    }

    /// Runs `call`, a read, on the session for `database`. When the session
    /// answers that its link is gone, it is dropped and the call is tried
    /// once on a new one. Never for a statement that may have run: a run, an
    /// edit or a job only drops the session (`recover`), and the user runs it again.
    /// `options` bound the wait for a session to open.
    pub async fn read<T, F, Fut>(&self, database: Option<&str>, options: &CallOptions, call: F) -> Result<T>
    where
        F: Fn(Arc<dyn Connection>) -> Fut,
        Fut: Future<Output = Result<T>>,
    {
        let session = self.session(database, options).await?;
        match call(Arc::clone(&session)).await {
            Err(error) if self.recover(database, &session, &error).await => {
                call(self.session(database, options).await?).await
            }
            other => other,
        }
    }

    /// After `session` failed with `error`: when its link is gone
    /// (`driver.lost`, or the SSH tunnel under it ended) it is dropped, so the
    /// next call opens another, and an ended tunnel is opened again first.
    /// Says whether it dropped it.
    pub async fn recover(&self, database: Option<&str>, session: &Arc<dyn Connection>, error: &DriverError) -> bool {
        let tunnel_ended = self
            .tunnel
            .lock()
            .await
            .as_ref()
            .is_some_and(kemable_tunnel::Tunnel::is_closed);
        if error.code != ErrorCode::Lost && !tunnel_ended {
            return false;
        }
        let key = self.key(database);
        let dropped = {
            let mut sessions = lock(&self.sessions);
            match sessions.get(&key) {
                Some(Slot::Open { session: held, .. }) if Arc::ptr_eq(held, session) => sessions.remove(&key),
                _ => None,
            }
        };
        if let Some(Slot::Open { session, .. }) = dropped {
            close_later(session);
        }
        if tunnel_ended {
            // A tunnel that cannot open again now is tried again on the next call.
            let _ = self.reopen_tunnel().await;
        }
        true
    }

    /// Opens the SSH tunnel again on a new local port, and drops every
    /// session: they dialed the old one.
    async fn reopen_tunnel(&self) -> Result<()> {
        let mut tunnel = self.tunnel.lock().await;
        let Some(ended) = tunnel.as_ref().filter(|tunnel| tunnel.is_closed()) else {
            // Another call opened it again meanwhile.
            return Ok(());
        };
        let opened = ended
            .reopen(&CallOptions::new(self.closing.child_token()).with_timeout(CONNECT_TIMEOUT))
            .await?;
        lock(&self.config).dial = Some(opened.local());
        *tunnel = Some(opened);
        drop(tunnel);
        let stale: Vec<_> = lock(&self.sessions).drain().map(|(_, slot)| slot).collect();
        for slot in stale {
            // One still opening finds its slot gone and closes itself.
            if let Slot::Open { session, .. } = slot {
                close_later(session);
            }
        }
        Ok(())
    }

    /// The session on `database`, opened now if it is not open yet. `None`,
    /// an empty name, or an engine without databases means the home session.
    /// The open runs in a task of its own, shared by every caller asking for
    /// the same database, within `CONNECT_TIMEOUT`; `options` bound only this
    /// caller's wait, so Stop or a short deadline ends it and the open goes on
    /// for the next call.
    pub async fn session(&self, database: Option<&str>, options: &CallOptions) -> Result<Arc<dyn Connection>> {
        options.check()?;
        let key = self.key(database);
        let mut opening = {
            let mut sessions = lock(&self.sessions);
            match sessions.get_mut(&key) {
                Some(Slot::Open { session, used }) => {
                    *used = Instant::now();
                    return Ok(Arc::clone(session));
                }
                Some(Slot::Opening(opening)) => opening.clone(),
                None => {
                    let opening = self.open(key.clone());
                    sessions.insert(key, Slot::Opening(opening.clone()));
                    opening
                }
            }
        };
        options
            .run(async move {
                let opened = opening
                    .wait_for(Option::is_some)
                    .await
                    .map_err(|_| DriverError::internal("the session's open ended without a word"))?;
                opened
                    .clone()
                    .unwrap_or_else(|| Err(DriverError::internal("the session's open ended without a word")))
            })
            .await
    }

    /// Opens the session for `key` in a task, which puts it in the map.
    fn open(&self, key: String) -> watch::Receiver<Option<Opened>> {
        let (sender, receiver) = watch::channel(None);
        let mine = receiver.clone();
        let driver = Arc::clone(&self.driver);
        let config = with_database(&lock(&self.config), &key);
        let sessions = Arc::clone(&self.sessions);
        let closing = self.closing.clone();
        tokio::spawn(async move {
            let options = CallOptions::new(closing.child_token()).with_timeout(CONNECT_TIMEOUT);
            let opened = driver.connect(&config, options).await;
            let opened = {
                let mut map = lock(&sessions);
                let ours = matches!(map.get(&key), Some(Slot::Opening(slot)) if slot.same_channel(&mine));
                match opened {
                    Ok(session) if ours && !closing.is_cancelled() => {
                        map.insert(
                            key,
                            Slot::Open {
                                session: Arc::clone(&session),
                                used: Instant::now(),
                            },
                        );
                        Ok(session)
                    }
                    Ok(session) => {
                        // The server closed, or its tunnel opened again, while this opened.
                        if ours {
                            map.remove(&key);
                        }
                        close_later(session);
                        Err(DriverError::new(
                            ErrorCode::Lost,
                            "the connection closed as the session opened",
                        ))
                    }
                    Err(error) => {
                        if ours {
                            map.remove(&key);
                        }
                        Err(error)
                    }
                }
            };
            let _ = sender.send(Some(opened));
        });
        receiver
    }

    pub async fn close(&self) {
        self.closing.cancel();
        let sessions: Vec<_> = lock(&self.sessions).drain().map(|(_, slot)| slot).collect();
        for slot in sessions {
            if let Slot::Open { session, .. } = slot {
                let _ = session.close().await;
            }
        }
        // Last: the sessions said goodbye through it.
        self.tunnel.lock().await.take();
    }

    /// Closes the app's own session on `database`, so a `DROP DATABASE` is
    /// not refused because Kemable itself is still connected to it. The home
    /// session stays: the tree and the database list go through it.
    pub async fn close_session(&self, database: &str) {
        if database == self.home {
            return;
        }
        let slot = lock(&self.sessions).remove(database);
        if let Some(Slot::Open { session, .. }) = slot {
            let _ = session.close().await;
        }
    }
}

/// Closes a session dropped from the map off the caller's path: a lost
/// link's goodbye can take its time, or never come.
fn close_later(session: Arc<dyn Connection>) {
    tokio::spawn(async move {
        let _ = tokio::time::timeout(CONNECT_TIMEOUT, session.close()).await;
    });
}

/// How long an idle session has to say whether it holds a transaction.
const IDLE_PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// Looks for idle sessions of `server` every `IDLE_CHECK` until it closes.
fn close_when_idle(server: &Arc<OpenServer>) {
    let weak = Arc::downgrade(server);
    let closing = server.closing.clone();
    tokio::spawn(async move {
        let mut ticks = tokio::time::interval(IDLE_CHECK);
        loop {
            tokio::select! {
                () = closing.cancelled() => break,
                _ = ticks.tick() => {
                    let Some(server) = weak.upgrade() else { break };
                    server.close_idle(IDLE_AFTER).await;
                }
            }
        }
    });
}

/// The same server, pointed at another database.
pub(crate) fn with_database(config: &ConnectionConfig, database: &str) -> ConnectionConfig {
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
            host_keys: std::sync::OnceLock::new(),
            result_bytes: crate::memory::result_budget(),
        }
    }

    /// A result set of `columns` that stops at `row_limit` rows or at the memory budget.
    pub fn new_store(&self, columns: Vec<kemable_core::catalog::ColumnMeta>, row_limit: Option<u64>) -> ResultStore {
        ResultStore::new(columns, row_limit).with_byte_limit(self.result_bytes)
    }

    /// The known SSH hosts; before startup sets them (tests), a file in the temp folder.
    pub fn host_keys(&self) -> Arc<kemable_tunnel::HostKeys> {
        Arc::clone(self.host_keys.get_or_init(|| {
            Arc::new(kemable_tunnel::HostKeys::new(
                std::env::temp_dir().join(format!("kemable-known-hosts-{}", std::process::id())),
            ))
        }))
    }

    fn next_id(&self) -> u64 {
        self.ids.fetch_add(1, Ordering::Relaxed)
    }

    pub fn add_server(&self, server: Arc<OpenServer>) -> u64 {
        let id = self.next_id();
        close_when_idle(&server);
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
