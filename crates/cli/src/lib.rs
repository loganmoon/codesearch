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

// Internal function needed by init module
fn parse_provider_type(provider: &str) -> codesearch_embeddings::EmbeddingProviderType {
    match provider.to_lowercase().as_str() {
        "localapi" | "api" => codesearch_embeddings::EmbeddingProviderType::LocalApi,
        "mock" => codesearch_embeddings::EmbeddingProviderType::Mock,
        _ => codesearch_embeddings::EmbeddingProviderType::LocalApi,
    }
}

/// Helper function to create an embedding manager from configuration
pub async fn create_embedding_manager(
    config: &Config,
) -> Result<std::sync::Arc<codesearch_embeddings::EmbeddingManager>> {
    use anyhow::Context;
    use codesearch_embeddings::EmbeddingManager;

    let mut embeddings_config_builder = codesearch_embeddings::EmbeddingConfigBuilder::default()
        .provider(parse_provider_type(&config.embeddings.provider))
        .model(config.embeddings.model.clone())
        .batch_size(config.embeddings.batch_size)
        .embedding_dimension(config.embeddings.embedding_dimension);

    if let Some(ref api_base_url) = config.embeddings.api_base_url {
        embeddings_config_builder = embeddings_config_builder.api_base_url(api_base_url.clone());
    }

    let api_key = config
        .embeddings
        .api_key
        .clone()
        .or_else(|| std::env::var("VLLM_API_KEY").ok());
    if let Some(key) = api_key {
        embeddings_config_builder = embeddings_config_builder.api_key(key);
    }

    let embeddings_config = embeddings_config_builder.build();

    let embedding_manager = EmbeddingManager::from_config(embeddings_config)
        .await
        .context("Failed to create embedding manager")?;

    Ok(std::sync::Arc::new(embedding_manager))
}
