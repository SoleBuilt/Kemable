//! Domain types and the driver contract every engine implements.
//!
//! This crate depends on no other workspace crate and on no GUI toolkit
//! (`core/ARCHITECTURE.md`, Rules).

pub mod call;
pub mod catalog;
pub mod command;
pub mod config;
pub mod connection;
pub mod connection_url;
pub mod driver;
pub mod engine;
pub mod error;
pub mod format;
pub mod keys;
pub mod registry;
pub mod sink;
pub mod sql;
pub mod text_file;
pub mod value;

pub use call::CallOptions;
pub use error::{DriverError, ErrorCode};
pub type Result<T, E = DriverError> = std::result::Result<T, E>;
