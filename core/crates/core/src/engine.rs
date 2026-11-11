use serde::Serialize;

use crate::catalog::ObjectKind;
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

/// One folder of the object tree under a schema: the words the UI shows and
/// the kinds of object it holds. A folder with nothing in it is not shown;
/// the first folder starts open, the others closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TreeFolder {
    pub label: &'static str,
    pub kinds: &'static [ObjectKind],
}

/// The folders most SQL engines show: PostgreSQL, SQLite, D1.
pub const SQL_TREE_FOLDERS: &[TreeFolder] = &[
    TreeFolder {
        label: "Tables",
        kinds: &[ObjectKind::Table],
    },
    TreeFolder {
        label: "Views",
        kinds: &[ObjectKind::View, ObjectKind::MaterializedView],
    },
    TreeFolder {
        label: "Functions",
        kinds: &[ObjectKind::Function, ObjectKind::TableFunction, ObjectKind::Procedure],
    },
    TreeFolder {
        label: "Sequences",
        kinds: &[ObjectKind::Sequence],
    },
    TreeFolder {
        label: "Triggers",
        kinds: &[ObjectKind::Trigger],
    },
    TreeFolder {
        label: "Indexes",
        kinds: &[ObjectKind::Index],
    },
];

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
    /// Whether a server holds several databases a login can list and open.
    /// The UI then shows them at the top of the tree and runs in one of them.
    pub has_databases: bool,
    /// The folders a schema's objects fall in, as the engine's own tools
    /// show them (SSMS's for SQL Server); empty for an engine without a tree.
    pub tree_folders: &'static [TreeFolder],
}
