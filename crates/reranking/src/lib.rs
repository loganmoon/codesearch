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

/// Sort scored documents by relevance score descending, with NaN values sorted to the end.
pub(crate) fn sort_scores_descending(scored_docs: &mut [(String, f32)]) {
    scored_docs.sort_by(|a, b| {
        let a_is_nan = a.1.is_nan();
        let b_is_nan = b.1.is_nan();
        match (a_is_nan, b_is_nan) {
            (true, true) => std::cmp::Ordering::Equal,
            (true, false) => std::cmp::Ordering::Greater, // NaN sorts to end
            (false, true) => std::cmp::Ordering::Less,    // NaN sorts to end
            (false, false) => b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal),
        }
    });
}

/// Trait for reranker providers
///
/// This trait defines the interface for reranking providers that use cross-encoder
/// models to rescore candidate documents against a query.
#[async_trait]
pub trait RerankerProvider: Send + Sync {
    /// Rerank documents by relevance to the query
    ///
    /// Scores all provided documents and returns them sorted by relevance.
    /// The caller is responsible for truncating to desired number of results.
    ///
    /// # Arguments
    /// * `query` - The search query text
    /// * `documents` - List of (document_id, document_content) tuples to rerank
    ///
    /// # Returns
    /// A vector of (document_id, relevance_score) tuples for all documents, sorted by descending relevance
    async fn rerank(&self, query: &str, documents: &[(String, &str)])
        -> Result<Vec<(String, f32)>>;
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
        "vllm" => {
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
        other => Err(Error::config(format!(
            "Unknown reranking provider: '{other}'. Valid providers: jina, vllm"
        ))),
    }
}
