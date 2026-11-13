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
mod live;
mod tls;

use std::sync::Arc;

use async_trait::async_trait;
use kemable_core::config::ConnectionConfig;
use kemable_core::connection::Connection;
use kemable_core::driver::Driver;
use kemable_core::engine::{DataModel, EngineInfo, TargetKind};
use kemable_core::live::{LiveTopic, LiveTopicKind};
use kemable_core::sink::{ExecOutcome, ResultSink};
use kemable_core::sql::SqlGrammar;
use kemable_core::{CallOptions, Result};

pub use connection::{PostgresConnection, StatementMode};

pub static INFO: EngineInfo = EngineInfo {
    id: "postgres",
    display_name: "PostgreSQL",
    data_model: DataModel::Sql,
    targets: &[TargetKind::Network],
    default_port: Some(5432),
    sql_grammar: Some(SqlGrammar::Postgres),
    live_topics: &[LiveTopicKind::Channel],
    has_databases: true,
    tree_folders: kemable_core::engine::SQL_TREE_FOLDERS,
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

    async fn listen(
        &self,
        config: &ConnectionConfig,
        topics: &[LiveTopic],
        sink: Arc<dyn ResultSink>,
        options: CallOptions,
    ) -> Result<ExecOutcome> {
        live::listen(config, topics, sink.as_ref(), &options).await
    }
}

#[cfg(test)]
mod tests;
