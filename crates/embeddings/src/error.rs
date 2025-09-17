//! Error types for the embeddings module

use std::fmt;

/// Errors that can occur during embedding operations
#[derive(Debug)]
pub enum EmbeddingError {
    /// Model loading failed
    ModelLoadError(String),

    /// Tokenization failed
    TokenizationError(String),

    /// Inference failed
    InferenceError(String),

    /// Batch size exceeded
    BatchSizeExceeded { requested: usize, max: usize },

    /// Sequence too long
    SequenceTooLong { length: usize, max: usize },

    /// Out of memory
    OutOfMemory(String),

    /// Unsupported provider
    UnsupportedProvider(String),

    /// Configuration error
    ConfigError(String),

    /// IO error
    IoError(std::io::Error),

    /// Other error
    Other(String),
}

impl fmt::Display for EmbeddingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ModelLoadError(msg) => write!(f, "Failed to load model: {msg}"),
            Self::TokenizationError(msg) => write!(f, "Tokenization failed: {msg}"),
            Self::InferenceError(msg) => write!(f, "Inference failed: {msg}"),
            Self::BatchSizeExceeded { requested, max } => {
                write!(f, "Batch size {requested} exceeds maximum {max}")
            }
            Self::SequenceTooLong { length, max } => {
                write!(f, "Sequence length {length} exceeds maximum {max}")
            }
            Self::OutOfMemory(msg) => write!(f, "Out of memory: {msg}"),
            Self::UnsupportedProvider(provider) => {
                write!(f, "Unsupported embedding provider: {provider}")
            }
            Self::ConfigError(msg) => write!(f, "Configuration error: {msg}"),
            Self::IoError(err) => write!(f, "IO error: {err}"),
            Self::Other(msg) => write!(f, "Embedding error: {msg}"),
        }
    }
}

impl std::error::Error for EmbeddingError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::IoError(err) => Some(err),
            _ => None,
        }
    }
}

impl From<std::io::Error> for EmbeddingError {
    fn from(err: std::io::Error) -> Self {
        Self::IoError(err)
    }
}

impl From<EmbeddingError> for codesearch_core::error::Error {
    fn from(err: EmbeddingError) -> Self {
        codesearch_core::error::Error::Embedding(err.to_string())
    }
}
