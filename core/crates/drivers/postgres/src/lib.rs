//! PostgreSQL, and the services that speak it: Neon, Supabase, and PGlite
//! through pglite-socket (`docs/decisions/0004-engines.md`).
//!
//! A connection holds two sessions: one runs the editor's statements, the
//! other reads the catalog, so the sidebar answers while a long query runs.
//! Values arrive in binary and are decoded exactly (`decode`). A stop sends
//! PostgreSQL's cancel request, so the statement ends on the server.

mod connection;
mod ddl;
mod decode;
mod errors;
mod tls;

use std::sync::Arc;

use async_trait::async_trait;
use paboable_core::config::ConnectionConfig;
use paboable_core::connection::Connection;
use paboable_core::driver::Driver;
use paboable_core::engine::{DataModel, EngineInfo, TargetKind};
use paboable_core::sql::SqlGrammar;
use paboable_core::{CallOptions, Result};

pub use connection::{PostgresConnection, StatementMode};

pub static INFO: EngineInfo = EngineInfo {
    id: "postgres",
    display_name: "PostgreSQL",
    data_model: DataModel::Sql,
    targets: &[TargetKind::Network],
    default_port: Some(5432),
    sql_grammar: Some(SqlGrammar::Postgres),
    live: true,
    has_databases: true,
    tree_folders: paboable_core::engine::SQL_TREE_FOLDERS,
};

pub struct PostgresDriver;

#[async_trait]
impl Driver for PostgresDriver {
    fn info(&self) -> &'static EngineInfo {
        &INFO
    }

    async fn connect(&self, config: &ConnectionConfig, options: CallOptions) -> Result<Arc<dyn Connection>> {
        Ok(Arc::new(PostgresConnection::connect(config, options).await?))
    }
}

#[cfg(test)]
mod tests;
