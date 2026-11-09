use serde::Serialize;

use crate::sql::SqlGrammar;

/// How an engine's data is shaped, which decides the tabs a UI opens for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DataModel {
    /// Schemas, tables and SQL: PostgreSQL, SQL Server, SQLite, D1, PGlite.
    Sql,
    /// Keys and typed values: Redis.
    KeyValue,
    /// Collections of documents: Firestore.
    Document,
    /// Queues, exchanges and messages: RabbitMQ.
    Queue,
}

/// How a connection reaches its engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TargetKind {
    File,
    Network,
    Http,
}

/// What a driver is, known before any connection exists.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineInfo {
    /// Stable id saved in connection files, such as `sqlite` or `postgres`.
    pub id: &'static str,
    pub display_name: &'static str,
    pub data_model: DataModel,
    pub targets: &'static [TargetKind],
    pub default_port: Option<u16>,
    /// The grammar scripts are split with; `None` for an engine without SQL.
    pub sql_grammar: Option<SqlGrammar>,
    /// Whether the engine can push changes as they happen (the Live tab).
    pub live: bool,
}
