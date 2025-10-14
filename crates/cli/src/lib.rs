//! Library interface for codesearch CLI
//!
//! This module exposes internal functions for integration testing while keeping
//! the main binary logic in main.rs.

pub mod docker;
pub mod infrastructure;
pub mod storage_init;

// Public module for storage initialization
pub mod init;

// Re-export commonly needed types for tests
pub use anyhow::Result;
pub use codesearch_core::config::Config;
pub use std::path::Path;

/// Helper function to create an embedding manager from configuration
///
/// This now delegates to the shared implementation in codesearch-embeddings crate.
pub async fn create_embedding_manager(
    config: &Config,
) -> Result<std::sync::Arc<codesearch_embeddings::EmbeddingManager>> {
    codesearch_embeddings::create_embedding_manager_from_app_config(&config.embeddings)
        .await
        .map_err(Into::into)
}
