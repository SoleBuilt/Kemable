use std::sync::Arc;

use async_trait::async_trait;

use crate::catalog::{ColumnInfo, ObjectInfo, SchemaInfo, TableRef};
use crate::engine::EngineInfo;
use crate::sink::{ExecOutcome, ResultSink};
use crate::{CallOptions, Result};

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
    async fn ping(&self, options: CallOptions) -> Result<()>;

    async fn list_schemas(&self, options: CallOptions) -> Result<Vec<SchemaInfo>>;

    async fn list_objects(&self, schema: Option<&str>, options: CallOptions) -> Result<Vec<ObjectInfo>>;

    async fn fetch_columns(&self, table: &TableRef, options: CallOptions) -> Result<Vec<ColumnInfo>>;

    /// Runs one statement, already split from its script, and streams what
    /// it returns into `sink`. A cancel stops the statement on the server.
    async fn execute(&self, statement: &str, sink: Arc<dyn ResultSink>, options: CallOptions) -> Result<ExecOutcome>;

    async fn close(&self) -> Result<()>;
}
