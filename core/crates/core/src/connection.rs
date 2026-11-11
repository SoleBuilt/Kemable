use std::sync::Arc;

use async_trait::async_trait;

use crate::catalog::{ColumnInfo, DatabaseInfo, IndexInfo, ObjectInfo, SchemaInfo, TableRef};
use crate::engine::EngineInfo;
use crate::keys::{KeyDetail, KeyPage, KeyScan};
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

    /// A table's indexes, the primary key's first, for Show Structure.
    async fn list_indexes(&self, _table: &TableRef, _options: CallOptions) -> Result<Vec<IndexInfo>> {
        Err(DriverError::unsupported("this engine does not list indexes"))
    }

    /// The statement that makes `object` as the server holds it now, for Show
    /// DDL and Copy DDL (`docs/decisions/0017-tree-actions.md`): a table with
    /// its columns, keys, constraints and indexes; a view, function or
    /// procedure as it was written. Several statements end each with `;`.
    async fn object_ddl(&self, _object: &ObjectInfo, _options: CallOptions) -> Result<String> {
        Err(DriverError::unsupported("this engine does not script its objects"))
    }

    /// The comment on `object` (PostgreSQL's `COMMENT ON`, SQL Server's
    /// `MS_Description`), or `None` when it has none.
    async fn object_comment(&self, _object: &ObjectInfo, _options: CallOptions) -> Result<Option<String>> {
        Err(DriverError::unsupported("this engine keeps no comments on its objects"))
    }

    /// Runs one statement, already split from its script, and streams what
    /// it returns into `sink`. A cancel stops the statement on the server.
    async fn execute(&self, statement: &str, sink: Arc<dyn ResultSink>, options: CallOptions) -> Result<ExecOutcome>;

    /// Saves a table tab's edits (`docs/decisions/0010-editing-rows.md`):
    /// `statements` in one transaction, each of which must change exactly one
    /// row, since an edit names its row by its key. Any failure, or another
    /// count (`ErrorCode::RowGone`), rolls every statement back.
    async fn apply_edits(&self, _statements: &[String], _options: CallOptions) -> Result<()> {
        Err(DriverError::unsupported("this engine does not save edits"))
    }

    /// One step of a scan over a key-value engine's keys, with each key's
    /// kind and time to live. Never a command that reads every key at once.
    async fn scan_keys(&self, _scan: &KeyScan, _options: CallOptions) -> Result<KeyPage> {
        Err(DriverError::unsupported("this engine has no keys"))
    }

    /// Streams one key's value into `sink` as a result set (a hash as
    /// field and value, a list as index and value, ...), in steps, stopping
    /// when the sink takes no more.
    async fn read_key(&self, _key: &str, _sink: Arc<dyn ResultSink>, _options: CallOptions) -> Result<KeyDetail> {
        Err(DriverError::unsupported("this engine has no keys"))
    }

    async fn close(&self) -> Result<()>;
}
