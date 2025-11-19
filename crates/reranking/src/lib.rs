//! Reranker providers for cross-encoder reranking
//!
//! This crate provides reranking capabilities using cross-encoder models
//! to rescore candidate documents against a query.

#![deny(warnings)]
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]

use async_trait::async_trait;
use codesearch_core::error::Result;
use std::sync::Arc;

pub mod error;
mod vllm;

pub use error::RerankingError;

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

/// Create a new reranker provider
///
/// # Arguments
/// * `model` - Model name (e.g., "BAAI/bge-reranker-v2-m3")
/// * `api_base_url` - Base URL for the vLLM API (e.g., "http://localhost:8001")
/// * `timeout_secs` - Request timeout in seconds
/// * `max_concurrent_requests` - Maximum concurrent API requests
pub async fn create_reranker_provider(
    model: String,
    api_base_url: String,
    timeout_secs: u64,
    max_concurrent_requests: usize,
) -> Result<Arc<dyn RerankerProvider>> {
    let provider = vllm::VllmRerankerProvider::new(
        model,
        api_base_url,
        timeout_secs,
        max_concurrent_requests,
    )?;

    // Perform health check (non-blocking)
    provider.check_health().await;

    Ok(Arc::new(provider))
}
