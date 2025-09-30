//! Embedding generation for code chunks
//!
//! This crate provides both local and remote embedding generation
//! capabilities for semantic code search.

#![deny(warnings)]
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]

use codesearch_core::error::Result;
use std::sync::Arc;

pub mod config;
pub mod error;
mod mock_provider;
pub mod provider;

pub use config::{
    BackendType, DeviceType, EmbeddingConfig, EmbeddingConfigBuilder, EmbeddingProviderType,
};
pub use error::EmbeddingError;
pub use provider::EmbeddingProvider;

/// Manager for handling embedding generation with immutable configuration
pub struct EmbeddingManager {
    provider: Arc<dyn EmbeddingProvider>,
}

impl EmbeddingManager {
    /// Creates a new embedding manager with the specified provider
    pub fn new(provider: Arc<dyn EmbeddingProvider>) -> Self {
        Self { provider }
    }

    /// Initialize manager from configuration
    pub async fn from_config(config: EmbeddingConfig) -> Result<Self> {
        let provider = match config.provider {
            EmbeddingProviderType::Mock => {
                let provider = mock_provider::MockEmbeddingProvider::new(384);
                Arc::new(provider) as Arc<dyn EmbeddingProvider>
            }
        };

        Ok(Self { provider })
    }

    /// Get reference to the embedding provider
    pub fn provider(&self) -> &dyn EmbeddingProvider {
        self.provider.as_ref()
    }

    /// Generate embeddings for texts
    pub async fn embed(&self, texts: Vec<String>) -> Result<Vec<Option<Vec<f32>>>> {
        self.provider.embed(texts).await
    }
}
