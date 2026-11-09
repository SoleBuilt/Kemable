use std::sync::Arc;

use crate::driver::Driver;
use crate::engine::EngineInfo;

/// The engines an app ships. Only the app names every driver crate; the rest
/// of the core reaches drivers through this.
#[derive(Default, Clone)]
pub struct DriverRegistry {
    drivers: Vec<Arc<dyn Driver>>,
}

impl DriverRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn with(mut self, driver: Arc<dyn Driver>) -> Self {
        self.drivers.push(driver);
        self
    }

    pub fn get(&self, id: &str) -> Option<&Arc<dyn Driver>> {
        self.drivers.iter().find(|driver| driver.info().id == id)
    }

    pub fn engines(&self) -> impl Iterator<Item = &'static EngineInfo> + '_ {
        self.drivers.iter().map(|driver| driver.info())
    }
}
