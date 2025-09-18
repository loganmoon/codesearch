use codesearch_core::Error as CoreError;
use thiserror::Error;

/// Storage-specific error types
#[derive(Error, Debug)]
pub enum StorageError {
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),

    #[error("Collection not found: {0}")]
    CollectionNotFound(String),

    #[error("Batch size exceeded: requested {requested}, max {max}")]
    BatchSizeExceeded { requested: usize, max: usize },

    #[error("Invalid vector dimensions: expected {expected}, got {actual}")]
    InvalidDimensions { expected: usize, actual: usize },

    #[error("Operation timeout after {0}ms")]
    Timeout(u64),

    #[error("Storage backend error: {0}")]
    BackendError(String),

    #[error("Serialization error: {0}")]
    SerializationError(String),

    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),
}

impl From<StorageError> for CoreError {
    fn from(err: StorageError) -> Self {
        CoreError::storage(err.to_string())
    }
}
