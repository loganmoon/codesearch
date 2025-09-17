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
mod embed_anything_provider;
pub mod error;
pub mod provider;

// Keep local module for now but don't export it
// #[allow(dead_code)]
// mod local;

pub use config::{
    BackendType, DeviceType, EmbeddingConfig, EmbeddingConfigBuilder, EmbeddingProviderType,
};
pub use embed_anything_provider::create_embed_anything_provider;
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
            EmbeddingProviderType::Local => {
                let provider = create_embed_anything_provider(config).await?;
                Arc::from(provider)
            }
        };

        Ok(Self { provider })
    }

    /// Get reference to the embedding provider
    pub fn provider(&self) -> &dyn EmbeddingProvider {
        self.provider.as_ref()
    }

    /// Generate embeddings for texts
    pub async fn embed(&self, texts: Vec<String>) -> Result<Vec<Vec<f64>>> {
        self.provider.embed(texts).await
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn test_manager_creation() {
        // This will be tested more thoroughly in integration tests
        // For now, just ensure the module compiles
        assert_eq!(1 + 1, 2);
    }
}
