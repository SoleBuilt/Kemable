//! SQL Server through tiberius.
//!
//! A connection holds two sessions, editor and catalog, like the PostgreSQL
//! driver. A run sends one `GO` batch at a time and streams every result set
//! it returns. A stop drops the request and sends a TDS attention on the same
//! connection, so the batch ends on the server; a session that cannot be
//! brought back to a clean state is closed and opened again on the next call.
//!
//! Logins: SQL Server authentication with a user and password, or, with no
//! user, Windows integrated authentication as the signed-in Windows user.
//! `host\INSTANCE` reaches a named instance through the SQL Server Browser.

mod connection;
mod ddl;
mod decode;
mod errors;

use std::sync::Arc;

use async_trait::async_trait;
use paboable_core::catalog::ObjectKind;
use paboable_core::config::ConnectionConfig;
use paboable_core::connection::Connection;
use paboable_core::driver::Driver;
use paboable_core::engine::{DataModel, EngineInfo, TargetKind, TreeFolder};
use paboable_core::sql::SqlGrammar;
use paboable_core::{CallOptions, Result};

pub use connection::MssqlConnection;

/// SSMS's folders under a schema.
const TREE_FOLDERS: &[TreeFolder] = &[
    TreeFolder {
        label: "Tables",
        kinds: &[ObjectKind::Table],
    },
    TreeFolder {
        label: "Views",
        kinds: &[ObjectKind::View],
    },
    TreeFolder {
        label: "Stored Procedures",
        kinds: &[ObjectKind::Procedure],
    },
    TreeFolder {
        label: "Table-valued Functions",
        kinds: &[ObjectKind::TableFunction],
    },
    TreeFolder {
        label: "Scalar-valued Functions",
        kinds: &[ObjectKind::Function],
    },
    TreeFolder {
        label: "Sequences",
        kinds: &[ObjectKind::Sequence],
    },
];

pub static INFO: EngineInfo = EngineInfo {
    id: "mssql",
    display_name: "SQL Server",
    data_model: DataModel::Sql,
    targets: &[TargetKind::Network],
    default_port: Some(1433),
    sql_grammar: Some(SqlGrammar::SqlServer),
    live_topics: &[],
    has_databases: true,
    tree_folders: TREE_FOLDERS,
};

pub struct MssqlDriver;

#[async_trait]
impl Driver for MssqlDriver {
    fn info(&self) -> &'static EngineInfo {
        &INFO
    }

    async fn connect(&self, config: &ConnectionConfig, options: CallOptions) -> Result<Arc<dyn Connection>> {
        Ok(Arc::new(MssqlConnection::connect(config, options).await?))
    }
}

#[cfg(test)]
mod tests;
