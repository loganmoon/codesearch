//! Reranker provider for cross-encoder reranking

use crate::error::EmbeddingError;
use async_trait::async_trait;
use codesearch_core::error::Result;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{debug, info, warn};

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

/// Request payload for vLLM rerank API
#[derive(Debug, Serialize)]
struct RerankRequest {
    model: String,
    query: String,
    documents: Vec<String>,
}

/// Response from vLLM rerank API
#[derive(Debug, Deserialize)]
struct RerankResponse {
    results: Vec<RerankResult>,
}

/// Individual rerank result
#[derive(Debug, Deserialize)]
struct RerankResult {
    index: usize,
    relevance_score: f32,
}

/// vLLM-compatible reranker provider
pub struct VllmRerankerProvider {
    client: Client,
    model: String,
    api_base_url: String,
}

impl VllmRerankerProvider {
    /// Create a new vLLM reranker provider
    ///
    /// # Arguments
    /// * `model` - Model name (e.g., "BAAI/bge-reranker-v2-m3")
    /// * `api_base_url` - Base URL for the vLLM API (e.g., "http://localhost:8001/v1")
    /// * `timeout_secs` - Request timeout in seconds
    pub fn new(model: String, api_base_url: String, timeout_secs: u64) -> Result<Self> {
        info!("Initializing vLLM reranker provider");
        info!("  Model: {model}");
        info!("  API Base URL: {api_base_url}");
        info!("  Timeout: {timeout_secs}s");

        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(timeout_secs))
            .build()
            .map_err(|e| {
                EmbeddingError::ConfigError(format!("Failed to create HTTP client: {e}"))
            })?;

        Ok(Self {
            client,
            model,
            api_base_url,
        })
    }

    /// Check if the reranker API is healthy (non-blocking, warns on failure)
    async fn check_health(&self) {
        debug!("Checking reranker API health");

        let models_url = format!("{}/models", self.api_base_url);
        match self.client.get(&models_url).send().await {
            Ok(response) => {
                if response.status().is_success() {
                    info!("Reranker API health check passed");
                } else {
                    warn!(
                        "Reranker API health check failed with status: {}",
                        response.status()
                    );
                    warn!("  The vLLM reranker service may not be running or still starting up.");
                }
            }
            Err(e) => {
                warn!("Reranker API health check failed: {e}");
                warn!("  The vLLM reranker service may not be running or still starting up.");
                warn!("  It can take 30-60 seconds for the service to become available.");
                warn!("  If you're using docker compose, check: docker compose logs vllm-reranker");
            }
        }
    }
}

#[async_trait]
impl RerankerProvider for VllmRerankerProvider {
    async fn rerank(
        &self,
        query: &str,
        documents: &[(String, &str)],
        top_k: usize,
    ) -> Result<Vec<(String, f32)>> {
        if documents.is_empty() {
            return Ok(Vec::new());
        }

        // Build request payload
        let doc_texts: Vec<String> = documents
            .iter()
            .map(|(_, content)| content.to_string())
            .collect();

        let request = RerankRequest {
            model: self.model.clone(),
            query: query.to_string(),
            documents: doc_texts,
        };

        // Send request to vLLM rerank endpoint
        let rerank_url = format!("{}/rerank", self.api_base_url);

        debug!("Sending rerank request for {} documents", documents.len());

        let response = self
            .client
            .post(&rerank_url)
            .json(&request)
            .send()
            .await
            .map_err(|e| {
                EmbeddingError::InferenceError(format!("Rerank API request failed: {e}"))
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unable to read error response".to_string());
            return Err(EmbeddingError::InferenceError(format!(
                "Rerank API returned error {status}: {error_text}"
            ))
            .into());
        }

        let rerank_response: RerankResponse = response.json().await.map_err(|e| {
            EmbeddingError::InferenceError(format!("Failed to parse rerank response: {e}"))
        })?;

        // Map indices back to document IDs with scores
        let mut scored_docs: Vec<(String, f32)> = rerank_response
            .results
            .into_iter()
            .filter_map(|result| {
                documents
                    .get(result.index)
                    .map(|(id, _)| (id.clone(), result.relevance_score))
            })
            .collect();

        // Sort by relevance score descending
        scored_docs.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        // Truncate to top_k
        scored_docs.truncate(top_k);

        debug!("Reranking complete: returned {} results", scored_docs.len());

        Ok(scored_docs)
    }
}

/// Create a new reranker provider
///
/// # Arguments
/// * `model` - Model name (e.g., "BAAI/bge-reranker-v2-m3")
/// * `api_base_url` - Base URL for the vLLM API (e.g., "http://localhost:8001/v1")
/// * `timeout_secs` - Request timeout in seconds
pub async fn create_reranker_provider(
    model: String,
    api_base_url: String,
    timeout_secs: u64,
) -> Result<Arc<dyn RerankerProvider>> {
    let provider = VllmRerankerProvider::new(model, api_base_url, timeout_secs)?;

    // Perform health check (non-blocking)
    provider.check_health().await;

    Ok(Arc::new(provider))
}
