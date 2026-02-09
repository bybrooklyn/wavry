//! Shared utilities for Wavry: configuration, logging, error types.
//!
//! This crate provides common infrastructure used across all Wavry components.

#![forbid(unsafe_code)]

pub mod error;
pub mod helpers;
pub mod protocol;

pub use error::{Error, Result};
pub use protocol::*;

/// Initialize tracing with sensible defaults.
///
/// Log level is controlled by the `RUST_LOG` environment variable.
/// Defaults to `info` if not set.
pub fn init_tracing() {
    use tracing_subscriber::EnvFilter;

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .init();
}

/// Initialize tracing with a specific default level.
pub fn init_tracing_with_default(default_level: &str) {
    use tracing_subscriber::EnvFilter;

    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_level));

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .init();
}
