//! Error types for agentic search operations

use thiserror::Error;

#[derive(Error, Debug)]
pub enum AgenticSearchError {
    #[error("Configuration error: {0}")]
    Config(String),

    #[error("API key not configured")]
    MissingApiKey,

    #[error("Orchestrator error: {0}")]
    Orchestrator(String),

    #[error("Worker error: {0}")]
    Worker(String),

    #[error("Reranking error: {0}")]
    Reranking(String),

    #[error("All workers failed")]
    AllWorkersFailed,

    #[error("Partial worker failure: {successful}/{total} succeeded")]
    PartialWorkerFailure { successful: usize, total: usize },

    #[error("Search API error: {0}")]
    SearchApi(String),

    #[error("Claudius SDK error: {0}")]
    Claudius(String),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Internal error: {0}")]
    Internal(String),
}

pub type Result<T> = std::result::Result<T, AgenticSearchError>;
