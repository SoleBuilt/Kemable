use std::sync::Arc;

use async_trait::async_trait;

use crate::config::ConnectionConfig;
use crate::connection::Connection;
use crate::engine::EngineInfo;
use crate::{CallOptions, Result};

/// One engine. A driver crate exports one value of this, and the app lists it
/// in its [`crate::registry::DriverRegistry`].
#[async_trait]
pub trait Driver: Send + Sync {
    fn info(&self) -> &'static EngineInfo;

    /// Opens a connection. The cancel token is checked between stages, and a
    /// stage that ends after a cancel closes what it opened.
    async fn connect(&self, config: &ConnectionConfig, options: CallOptions) -> Result<Arc<dyn Connection>>;
}
