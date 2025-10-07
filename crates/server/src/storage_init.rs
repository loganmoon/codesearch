//! Storage and embedding initialization helpers for the server
//!
//! This module provides helper functions for initializing embedding managers
//! and finding the repository root.

use anyhow::{Context, Result};
use codesearch_core::config::Config;
use codesearch_embeddings::EmbeddingManager;
use std::{path::PathBuf, sync::Arc};

/// Helper function to parse provider type from string
fn parse_provider_type(provider: &str) -> codesearch_embeddings::EmbeddingProviderType {
    match provider.to_lowercase().as_str() {
        "localapi" | "api" => codesearch_embeddings::EmbeddingProviderType::LocalApi,
        "mock" => codesearch_embeddings::EmbeddingProviderType::Mock,
        _ => codesearch_embeddings::EmbeddingProviderType::LocalApi,
    }
}

/// Create an embedding manager from configuration
pub(crate) async fn create_embedding_manager(config: &Config) -> Result<Arc<EmbeddingManager>> {
    let mut embeddings_config_builder = codesearch_embeddings::EmbeddingConfigBuilder::default()
        .provider(parse_provider_type(&config.embeddings.provider))
        .model(config.embeddings.model.clone())
        .batch_size(config.embeddings.batch_size)
        .embedding_dimension(config.embeddings.embedding_dimension)
        .device(match config.embeddings.device.as_str() {
            "cuda" => codesearch_embeddings::DeviceType::Cuda,
            _ => codesearch_embeddings::DeviceType::Cpu,
        });

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

    let embedding_manager = codesearch_embeddings::EmbeddingManager::from_config(embeddings_config)
        .await
        .context("Failed to create embedding manager")?;

    Ok(Arc::new(embedding_manager))
}

/// Find the git repository root by walking up the directory tree
pub(crate) fn find_repository_root() -> Result<PathBuf> {
    let current_dir = std::env::current_dir().context("Failed to get current directory")?;

    let mut path = current_dir.as_path();
    loop {
        if path.join(".git").is_dir() {
            return Ok(path.to_path_buf());
        }

        match path.parent() {
            Some(parent) => path = parent,
            None => {
                return Err(anyhow::anyhow!(
                    "Not a git repository (or any parent up to mount point)"
                ))
            }
        }
    }
}
