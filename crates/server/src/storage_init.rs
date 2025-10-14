//! Storage and embedding initialization helpers for the server
//!
//! This module provides helper functions for initializing embedding managers.

use codesearch_core::config::Config;
use codesearch_core::error::Result;
use codesearch_embeddings::EmbeddingManager;
use std::sync::Arc;

/// Create an embedding manager from configuration
///
/// This now delegates to the shared implementation in codesearch-embeddings crate.
pub(crate) async fn create_embedding_manager(config: &Config) -> Result<Arc<EmbeddingManager>> {
    codesearch_embeddings::create_embedding_manager_from_app_config(&config.embeddings).await
}
