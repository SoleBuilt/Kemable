use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use paboable_core::connection::Connection;
use paboable_core::registry::DriverRegistry;
use paboable_core::{DriverError, Result};
use paboable_store::ResultStore;
use tokio_util::sync::CancellationToken;

/// What the app holds open. The UI names each thing by an id from one
/// counter, so an id is never reused within a launch.
pub struct AppState {
    pub registry: DriverRegistry,
    ids: AtomicU64,
    connections: Mutex<HashMap<u64, Arc<dyn Connection>>>,
    results: Mutex<HashMap<u64, Arc<ResultStore>>>,
    runs: Mutex<HashMap<u64, CancellationToken>>,
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
            ids: AtomicU64::new(1),
            connections: Mutex::default(),
            results: Mutex::default(),
            runs: Mutex::default(),
        }
    }

    fn next_id(&self) -> u64 {
        self.ids.fetch_add(1, Ordering::Relaxed)
    }

    pub fn add_connection(&self, connection: Arc<dyn Connection>) -> u64 {
        let id = self.next_id();
        lock(&self.connections).insert(id, connection);
        id
    }

    pub fn connection(&self, id: u64) -> Result<Arc<dyn Connection>> {
        lock(&self.connections)
            .get(&id)
            .cloned()
            .ok_or_else(|| DriverError::not_found("connection"))
    }

    pub fn remove_connection(&self, id: u64) -> Option<Arc<dyn Connection>> {
        lock(&self.connections).remove(&id)
    }

    pub fn add_result(&self, store: Arc<ResultStore>) -> u64 {
        let id = self.next_id();
        lock(&self.results).insert(id, store);
        id
    }

    pub fn result(&self, id: u64) -> Result<Arc<ResultStore>> {
        lock(&self.results)
            .get(&id)
            .cloned()
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
