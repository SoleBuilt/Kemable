use std::sync::Arc;

use async_trait::async_trait;

use crate::catalog::{ColumnInfo, DatabaseInfo, ObjectInfo, SchemaInfo, TableRef};
use crate::engine::EngineInfo;
use crate::sink::{ExecOutcome, ResultSink};
use crate::{CallOptions, DriverError, Result};

/// A live connection to one engine.
///
/// Every call takes [`CallOptions`] and returns within its deadline. Methods
/// a data model does not use default to `driver.unsupported`, so a key-value
/// or queue engine implements only what it has.
#[async_trait]
pub trait Connection: Send + Sync {
    fn engine(&self) -> &'static EngineInfo;

    fn server_version(&self) -> Option<String>;

    /// A cheap round trip that proves the link is up.
    /// The database the session is on, for an engine with databases.
    fn current_database(&self) -> Option<String> {
        None
    }

    async fn ping(&self, options: CallOptions) -> Result<()>;

    /// The databases this login may open, for an engine with databases
    /// (`EngineInfo::has_databases`). The server's permissions decide the list.
    async fn list_databases(&self, _options: CallOptions) -> Result<Vec<DatabaseInfo>> {
        Err(DriverError::unsupported("this engine has no databases to list"))
    }

    async fn list_schemas(&self, options: CallOptions) -> Result<Vec<SchemaInfo>>;

    async fn list_objects(&self, schema: Option<&str>, options: CallOptions) -> Result<Vec<ObjectInfo>>;

    async fn fetch_columns(&self, table: &TableRef, options: CallOptions) -> Result<Vec<ColumnInfo>>;

    /// Runs one statement, already split from its script, and streams what
    /// it returns into `sink`. A cancel stops the statement on the server.
    async fn execute(&self, statement: &str, sink: Arc<dyn ResultSink>, options: CallOptions) -> Result<ExecOutcome>;

    async fn close(&self) -> Result<()>;
}
