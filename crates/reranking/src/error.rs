//! Error types for the reranking module

use std::fmt;

/// Errors that can occur during reranking operations
#[derive(Debug)]
pub enum RerankingError {
    /// Inference failed
    InferenceError(String),

    /// Configuration error
    ConfigError(String),

    /// Other error
    Other(String),
}

impl fmt::Display for RerankingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InferenceError(msg) => write!(f, "Inference failed: {msg}"),
            Self::ConfigError(msg) => write!(f, "Configuration error: {msg}"),
            Self::Other(msg) => write!(f, "Reranking error: {msg}"),
        }
    }
}

impl std::error::Error for RerankingError {}

impl From<RerankingError> for codesearch_core::error::Error {
    fn from(err: RerankingError) -> Self {
        codesearch_core::error::Error::Other(anyhow::anyhow!(err.to_string()))
    }
}
