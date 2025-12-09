//! Reranker providers for cross-encoder reranking
//!
//! This crate provides reranking capabilities using cross-encoder models
//! to rescore candidate documents against a query.

#![deny(warnings)]
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]

use async_trait::async_trait;
use codesearch_core::config::RerankingConfig;
use codesearch_core::error::{Error, Result};
use std::sync::Arc;
use tracing::info;

pub mod error;
mod jina;
mod vllm;

pub use error::RerankingError;
pub use jina::JinaRerankerProvider;

/// Trait for reranker providers
///
/// This trait defines the interface for reranking providers that use cross-encoder
/// models to rescore candidate documents against a query.
#[async_trait]
pub trait RerankerProvider: Send + Sync {
    /// Rerank documents by relevance to the query
    ///
    /// # Arguments
    /// * `query` - The search query text
    /// * `documents` - List of (document_id, document_content) tuples to rerank
    /// * `top_k` - Number of top results to return
    ///
    /// # Returns
    /// A vector of (document_id, relevance_score) tuples, sorted by descending relevance
    async fn rerank(
        &self,
        query: &str,
        documents: &[(String, &str)],
        top_k: usize,
    ) -> Result<Vec<(String, f32)>>;
}

/// Create a new reranker provider based on configuration
///
/// # Arguments
/// * `config` - Reranking configuration including provider type
pub async fn create_reranker_provider(
    config: &RerankingConfig,
) -> Result<Arc<dyn RerankerProvider>> {
    match config.provider.as_str() {
        "jina" => {
            let api_key = config
                .api_key
                .clone()
                .or_else(|| std::env::var("JINA_API_KEY").ok())
                .ok_or_else(|| {
                    Error::config(
                        "Jina API key required. Set reranking.api_key or JINA_API_KEY env var"
                            .to_string(),
                    )
                })?;

            info!("Creating Jina reranker provider");
            let provider = jina::JinaRerankerProvider::new(
                api_key,
                config.model.clone(),
                config.timeout_secs,
                config.max_concurrent_requests,
            )?;

            Ok(Arc::new(provider))
        }
        _ => {
            // Default to vLLM for backwards compatibility
            let api_base_url = config
                .api_base_url
                .clone()
                .unwrap_or_else(|| "http://localhost:8001/v1".to_string());

            info!("Creating vLLM reranker provider");
            let provider = vllm::VllmRerankerProvider::new(
                config.model.clone(),
                api_base_url,
                config.timeout_secs,
                config.max_concurrent_requests,
            )?;

            // Perform health check (non-blocking)
            provider.check_health().await;

            Ok(Arc::new(provider))
        }
    }
}
