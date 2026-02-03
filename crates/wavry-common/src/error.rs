//! Common error types for Wavry.

use thiserror::Error;

/// Result type alias using Wavry's error type.
pub type Result<T> = std::result::Result<T, Error>;

/// Top-level error type for Wavry operations.
#[derive(Debug, Error)]
pub enum Error {
    /// I/O error (file, network, etc.)
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Serialization/deserialization error
    #[error("serialization error: {0}")]
    Serialization(String),

    /// Configuration error
    #[error("configuration error: {0}")]
    Config(String),

    /// Cryptographic operation failed
    #[error("crypto error: {0}")]
    Crypto(String),

    /// Protocol error
    #[error("protocol error: {0}")]
    Protocol(String),

    /// Authentication failed
    #[error("authentication error: {0}")]
    Auth(String),

    /// Resource not found
    #[error("not found: {0}")]
    NotFound(String),

    /// Operation timed out
    #[error("timeout: {0}")]
    Timeout(String),

    /// Rate limit exceeded
    #[error("rate limited: {0}")]
    RateLimited(String),

    /// Internal error
    #[error("internal error: {0}")]
    Internal(String),
}

impl Error {
    /// Create a serialization error from any displayable type.
    pub fn serialization(msg: impl std::fmt::Display) -> Self {
        Self::Serialization(msg.to_string())
    }

    /// Create a config error from any displayable type.
    pub fn config(msg: impl std::fmt::Display) -> Self {
        Self::Config(msg.to_string())
    }

    /// Create a crypto error from any displayable type.
    pub fn crypto(msg: impl std::fmt::Display) -> Self {
        Self::Crypto(msg.to_string())
    }

    /// Create a protocol error from any displayable type.
    pub fn protocol(msg: impl std::fmt::Display) -> Self {
        Self::Protocol(msg.to_string())
    }

    /// Create an auth error from any displayable type.
    pub fn auth(msg: impl std::fmt::Display) -> Self {
        Self::Auth(msg.to_string())
    }

    /// Create a not found error from any displayable type.
    pub fn not_found(msg: impl std::fmt::Display) -> Self {
        Self::NotFound(msg.to_string())
    }

    /// Create a timeout error from any displayable type.
    pub fn timeout(msg: impl std::fmt::Display) -> Self {
        Self::Timeout(msg.to_string())
    }

    /// Create an internal error from any displayable type.
    pub fn internal(msg: impl std::fmt::Display) -> Self {
        Self::Internal(msg.to_string())
    }
}
