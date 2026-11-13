//! The catalog the editor's completion reads (`docs/decisions/0007-autocomplete.md`),
//! kept per open server and database so a word typed twice asks the server once.
//!
//! A lookup reads only what the context needs: the schemas, the tables of one
//! schema, the columns of the tables in scope. A failed or slow lookup gives
//! nothing and is not kept, so the next word tries again. The UI forgets a
//! server's catalog after a run that may have changed it.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use kemable_core::CallOptions;
use kemable_core::catalog::{ColumnInfo, ObjectInfo, SchemaInfo, TableRef};
use kemable_core::connection::Connection;
use kemable_core::sql::{CatalogView, CompletionContext};

/// Completion waits for the server no longer than this: a list that comes
/// late is a list the user typed past.
const LOOKUP_TIMEOUT: Duration = Duration::from_millis(1500);

/// An open server and one of its databases (`None`: the one it opened on).
type CatalogKey = (u64, Option<String>);

#[derive(Default)]
pub struct CatalogCache {
    entries: Mutex<HashMap<CatalogKey, Arc<Cached>>>,
}

#[derive(Default)]
struct Cached {
    schemas: Mutex<Option<Arc<Vec<SchemaInfo>>>>,
    objects: Mutex<HashMap<String, Arc<Vec<ObjectInfo>>>>,
    columns: Mutex<HashMap<TableRef, Arc<Vec<ColumnInfo>>>>,
}

fn options() -> CallOptions {
    CallOptions::default().with_timeout(LOOKUP_TIMEOUT)
}

impl CatalogCache {
    /// Drops what is kept for a server, after a change to its schema or when it closes.
    pub fn forget(&self, conn: u64) {
        self.entries
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retain(|(server, _), _| *server != conn);
    }

    fn entry(&self, conn: u64, database: Option<String>) -> Arc<Cached> {
        let mut entries = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
        Arc::clone(entries.entry((conn, database)).or_default())
    }

    /// What `context` needs, from the cache or the server.
    pub async fn view(
        &self,
        conn: u64,
        database: Option<String>,
        session: Arc<dyn Connection>,
        context: &CompletionContext,
    ) -> CatalogView {
        let cached = self.entry(conn, database);
        let schemas = cached.schemas(&*session).await;
        let lookups = context.lookups(&schemas);
        let mut view = CatalogView {
            schemas: schemas.to_vec(),
            ..CatalogView::default()
        };
        for schema in &lookups.objects_of {
            view.objects
                .extend(cached.objects(&*session, schema).await.iter().cloned());
        }
        for table in lookups.columns_of {
            let columns = cached.columns(&*session, &table).await;
            if !columns.is_empty() {
                view.columns.push((table, columns.to_vec()));
            }
        }
        view
    }
}

impl Cached {
    async fn schemas(&self, session: &dyn Connection) -> Arc<Vec<SchemaInfo>> {
        if let Some(schemas) = self.schemas.lock().unwrap_or_else(PoisonError::into_inner).clone() {
            return schemas;
        }
        match session.list_schemas(options()).await {
            Ok(schemas) => {
                let schemas = Arc::new(schemas);
                *self.schemas.lock().unwrap_or_else(PoisonError::into_inner) = Some(Arc::clone(&schemas));
                schemas
            }
            Err(_) => Arc::default(),
        }
    }

    async fn objects(&self, session: &dyn Connection, schema: &str) -> Arc<Vec<ObjectInfo>> {
        if let Some(objects) = self.objects.lock().unwrap_or_else(PoisonError::into_inner).get(schema) {
            return Arc::clone(objects);
        }
        match session.list_objects(Some(schema), options()).await {
            Ok(objects) => {
                let objects = Arc::new(objects);
                self.objects
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .insert(schema.to_owned(), Arc::clone(&objects));
                objects
            }
            Err(_) => Arc::default(),
        }
    }

    async fn columns(&self, session: &dyn Connection, table: &TableRef) -> Arc<Vec<ColumnInfo>> {
        if let Some(columns) = self.columns.lock().unwrap_or_else(PoisonError::into_inner).get(table) {
            return Arc::clone(columns);
        }
        match session.fetch_columns(table, options()).await {
            // An empty list is kept too: a name that is not a table stays one.
            Ok(columns) => {
                let columns = Arc::new(columns);
                self.columns
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .insert(table.clone(), Arc::clone(&columns));
                columns
            }
            Err(_) => Arc::default(),
        }
    }
}
