use std::sync::Arc;

use async_trait::async_trait;

use crate::config::ConnectionConfig;
use crate::connection::Connection;
use crate::engine::EngineInfo;
use crate::live::LiveTopic;
use crate::sink::{ExecOutcome, ResultSink};
use crate::{CallOptions, DriverError, Result};

/// One engine. A driver crate exports one value of this, and the app lists it
/// in its [`crate::registry::DriverRegistry`].
#[async_trait]
pub trait Driver: Send + Sync {
    fn info(&self) -> &'static EngineInfo;

    /// Opens a connection. The cancel token is checked between stages, and a
    /// stage that ends after a cancel closes what it opened.
    async fn connect(&self, config: &ConnectionConfig, options: CallOptions) -> Result<Arc<dyn Connection>>;

    /// Listens to `topics` on a link of its own and streams what arrives into
    /// `sink` as the rows of one set, one row an event, each pushed as it
    /// comes (`docs/decisions/0020-live-tab.md`). `begin_set` is called once
    /// the server has taken every topic, so rows after it miss nothing.
    ///
    /// It returns only when it stops: at the sink's row limit (`limited`),
    /// with `options.stop_reason()` after a cancel or the deadline, or with
    /// `driver.lost` when the link drops. The link closes as it returns, and
    /// with it what the server kept for the listener.
    async fn listen(
        &self,
        _config: &ConnectionConfig,
        _topics: &[LiveTopic],
        _sink: Arc<dyn ResultSink>,
        _options: CallOptions,
    ) -> Result<ExecOutcome> {
        Err(DriverError::unsupported("this engine pushes no changes to listen to"))
    }
}
