//! Embedding generation for code chunks
//!
//! This crate provides both local and remote embedding generation
//! capabilities for semantic code search.

#![deny(warnings)]
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]

use codesearch_core::error::{Result, ResultExt};
use std::sync::Arc;

mod api_provider;
mod bm25_provider;
mod code_tokenizer;
pub mod config;
pub mod error;
mod mock_provider;
pub mod provider;
mod sparse_provider;

pub use api_provider::create_api_provider;
pub use bm25_provider::Bm25SparseProvider;
pub use code_tokenizer::CodeTokenizer;
pub use config::{EmbeddingConfig, EmbeddingConfigBuilder, EmbeddingProviderType};
pub use error::EmbeddingError;
pub use mock_provider::MockEmbeddingProvider;
pub use provider::{EmbeddingContext, EmbeddingProvider};
pub use sparse_provider::SparseEmbeddingProvider;

// Re-export Tokenizer trait for use in indexer
pub use bm25::Tokenizer;

/// Helper function to parse provider type from string
fn parse_provider_type(provider: &str) -> EmbeddingProviderType {
    match provider.to_lowercase().as_str() {
        "localapi" | "api" => EmbeddingProviderType::LocalApi,
        "mock" => EmbeddingProviderType::Mock,
        _ => EmbeddingProviderType::LocalApi,
    }
}

/// Create an embedding manager from codesearch Config
///
/// This is a convenience function that converts from the main Config's EmbeddingsConfig
/// to the embeddings crate's EmbeddingConfig and creates an EmbeddingManager.
///
/// It also handles reading the API key from the EMBEDDING_API_KEY environment variable
/// if not specified in the config.
pub async fn create_embedding_manager_from_app_config(
    embeddings_config: &codesearch_core::config::EmbeddingsConfig,
) -> Result<Arc<EmbeddingManager>> {
    let mut config_builder = EmbeddingConfigBuilder::default()
        .provider(parse_provider_type(&embeddings_config.provider))
        .model(embeddings_config.model.clone())
        .texts_per_api_request(embeddings_config.texts_per_api_request)
        .embedding_dimension(embeddings_config.embedding_dimension)
        .max_concurrent_api_requests(embeddings_config.max_concurrent_api_requests)
        .retry_attempts(embeddings_config.retry_attempts);

    if let Some(ref api_base_url) = embeddings_config.api_base_url {
        config_builder = config_builder.api_base_url(api_base_url.clone());
    }

    let api_key = embeddings_config
        .api_key
        .clone()
        .or_else(|| std::env::var("EMBEDDING_API_KEY").ok());
    if let Some(key) = api_key {
        config_builder = config_builder.api_key(key);
    }

    let embedding_config = config_builder.build();

    let embedding_manager = EmbeddingManager::from_config(embedding_config)
        .await
        .context("Failed to create embedding manager")?;

    Ok(Arc::new(embedding_manager))
}

/// Manager for handling embedding generation with immutable configuration
pub struct EmbeddingManager {
    provider: Arc<dyn EmbeddingProvider>,
    model_version: String,
}

impl EmbeddingManager {
    /// Creates a new embedding manager with the specified provider and model version
    pub fn new(provider: Arc<dyn EmbeddingProvider>, model_version: String) -> Self {
        Self {
            provider,
            model_version,
        }
    }

    /// Initialize manager from configuration
    pub async fn from_config(config: EmbeddingConfig) -> Result<Self> {
        let model_version = config.model.clone();

        let provider = match config.provider {
            EmbeddingProviderType::LocalApi => {
                let provider = create_api_provider(config).await?;
                Arc::from(provider)
            }
            EmbeddingProviderType::Mock => {
                let provider =
                    mock_provider::MockEmbeddingProvider::new(config.embedding_dimension);
                Arc::new(provider) as Arc<dyn EmbeddingProvider>
            }
        };

        Ok(Self {
            provider,
            model_version,
        })
    }

    /// Get reference to the embedding provider
    pub fn provider(&self) -> &dyn EmbeddingProvider {
        self.provider.as_ref()
    }

    /// Get the model version string for cache keying
    pub fn model_version(&self) -> &str {
        &self.model_version
    }

    /// Generate embeddings for texts
    pub async fn embed(&self, texts: Vec<String>) -> Result<Vec<Option<Vec<f32>>>> {
        self.provider.embed(texts).await
    }

    /// Generate embeddings for texts with optional context for error logging
    pub async fn embed_with_context(
        &self,
        texts: Vec<String>,
        contexts: Option<Vec<EmbeddingContext>>,
    ) -> Result<Vec<Option<Vec<f32>>>> {
        self.provider.embed_with_context(texts, contexts).await
    }
}

/// Manager for handling sparse embedding generation with immutable configuration
pub struct SparseEmbeddingManager {
    provider: Arc<dyn crate::sparse_provider::SparseEmbeddingProvider>,
    model_version: String,
}

impl SparseEmbeddingManager {
    /// Creates a new sparse embedding manager with the specified provider and model version
    pub fn new(
        provider: Arc<dyn crate::sparse_provider::SparseEmbeddingProvider>,
        model_version: String,
    ) -> Self {
        Self {
            provider,
            model_version,
        }
    }

    /// Get the model version string for cache keying
    pub fn model_version(&self) -> &str {
        &self.model_version
    }

    /// Generate sparse embeddings for texts
    pub async fn embed_sparse(&self, texts: Vec<&str>) -> Result<Vec<Option<Vec<(u32, f32)>>>> {
        self.provider.embed_sparse(texts).await
    }
}

/// Create a sparse embedding manager with the specified average document length
///
/// # Arguments
/// * `avgdl` - Average document length in tokens (calculated per-repository)
///
/// # Returns
/// A configured sparse embedding manager using BM25
pub fn create_sparse_manager(avgdl: f32) -> Result<Arc<SparseEmbeddingManager>> {
    let provider = crate::bm25_provider::Bm25SparseProvider::new(avgdl);
    Ok(Arc::new(SparseEmbeddingManager::new(
        Arc::new(provider),
        "bm25-v2.3".to_string(),
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_embedding_api_key_from_env() {
        let embeddings_config = codesearch_core::config::EmbeddingsConfig {
            provider: "mock".to_string(),
            model: "test-model".to_string(),
            embedding_dimension: 384,
            texts_per_api_request: 10,
            max_concurrent_api_requests: 4,
            device: "cpu".to_string(),
            api_base_url: Some("http://localhost:8000".to_string()),
            api_key: None,
            default_bge_instruction: "Represent this sentence for searching relevant passages:"
                .to_string(),
            retry_attempts: 3,
        };

        std::env::set_var("EMBEDDING_API_KEY", "test-api-key-from-env");

        let result = create_embedding_manager_from_app_config(&embeddings_config).await;
        assert!(result.is_ok());

        std::env::remove_var("EMBEDDING_API_KEY");
    }

    #[tokio::test]
    async fn test_embedding_api_key_from_config_takes_precedence() {
        let embeddings_config = codesearch_core::config::EmbeddingsConfig {
            provider: "mock".to_string(),
            model: "test-model".to_string(),
            embedding_dimension: 384,
            texts_per_api_request: 10,
            max_concurrent_api_requests: 4,
            device: "cpu".to_string(),
            api_base_url: Some("http://localhost:8000".to_string()),
            api_key: Some("config-api-key".to_string()),
            default_bge_instruction: "Represent this sentence for searching relevant passages:"
                .to_string(),
            retry_attempts: 3,
        };

        std::env::set_var("EMBEDDING_API_KEY", "env-api-key");

        let result = create_embedding_manager_from_app_config(&embeddings_config).await;
        assert!(result.is_ok());

        std::env::remove_var("EMBEDDING_API_KEY");
    }

    #[tokio::test]
    async fn test_embedding_no_api_key() {
        let embeddings_config = codesearch_core::config::EmbeddingsConfig {
            provider: "mock".to_string(),
            model: "test-model".to_string(),
            embedding_dimension: 384,
            texts_per_api_request: 10,
            max_concurrent_api_requests: 4,
            device: "cpu".to_string(),
            api_base_url: Some("http://localhost:8000".to_string()),
            api_key: None,
            default_bge_instruction: "Represent this sentence for searching relevant passages:"
                .to_string(),
            retry_attempts: 3,
        };

        std::env::remove_var("EMBEDDING_API_KEY");

        let result = create_embedding_manager_from_app_config(&embeddings_config).await;
        assert!(result.is_ok());
    }
}
