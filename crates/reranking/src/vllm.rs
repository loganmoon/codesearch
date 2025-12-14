//! vLLM-compatible reranker provider

use crate::error::RerankingError;
use crate::sort_scores_descending;
use crate::RerankerProvider;
use async_trait::async_trait;
use codesearch_core::error::Result;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Semaphore;
use tracing::{debug, info, warn};

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
    concurrency_limiter: Arc<Semaphore>,
}

impl VllmRerankerProvider {
    /// Create a new vLLM reranker provider
    ///
    /// # Arguments
    /// * `model` - Model name (e.g., "BAAI/bge-reranker-v2-m3")
    /// * `api_base_url` - Base URL for the vLLM API (e.g., "http://localhost:8001")
    /// * `timeout_secs` - Request timeout in seconds
    /// * `max_concurrent_requests` - Maximum concurrent API requests
    pub fn new(
        model: String,
        api_base_url: String,
        timeout_secs: u64,
        max_concurrent_requests: usize,
    ) -> Result<Self> {
        info!("Initializing vLLM reranker provider");
        info!("  Model: {model}");
        info!("  API Base URL: {api_base_url}");
        info!("  Timeout: {timeout_secs}s");
        info!("  Max concurrent requests: {max_concurrent_requests}");

        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(timeout_secs))
            .build()
            .map_err(|e| {
                RerankingError::ConfigError(format!("Failed to create HTTP client: {e}"))
            })?;

        Ok(Self {
            client,
            model,
            api_base_url,
            concurrency_limiter: Arc::new(Semaphore::new(max_concurrent_requests)),
        })
    }

    /// Check if the reranker API is healthy (non-blocking, warns on failure)
    pub async fn check_health(&self) {
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

/// Truncate text to approximately fit within token limit
///
/// Uses a conservative estimate of ~4 characters per token.
/// For an 8192 token model with 50 documents, we target ~1200 tokens per document
/// (4,800 chars) to leave room for the query and safety margin.
fn truncate_for_reranking(text: &str, max_chars: usize) -> String {
    if text.len() <= max_chars {
        text.to_string()
    } else {
        // Truncate and add ellipsis
        let truncated = &text[..max_chars.saturating_sub(3)];
        format!("{truncated}...")
    }
}

const MAX_DOCUMENT_CHARS: usize = 4_800; // ~1200 tokens per document

#[async_trait]
impl RerankerProvider for VllmRerankerProvider {
    async fn rerank(
        &self,
        query: &str,
        documents: &[(String, &str)],
    ) -> Result<Vec<(String, f32)>> {
        if documents.is_empty() {
            return Ok(Vec::new());
        }

        // Build request payload with truncation to fit within token limits
        let doc_texts: Vec<String> = documents
            .iter()
            .map(|(_, content)| {
                let truncated = truncate_for_reranking(content, MAX_DOCUMENT_CHARS);
                if truncated.len() < content.len() {
                    debug!(
                        "Truncated document from {} to {} chars for reranking",
                        content.len(),
                        truncated.len()
                    );
                }
                truncated
            })
            .collect();

        let request = RerankRequest {
            model: self.model.clone(),
            query: query.to_string(),
            documents: doc_texts,
        };

        // Send request to vLLM rerank endpoint
        let rerank_url = format!("{}/rerank", self.api_base_url);

        debug!("Sending rerank request for {} documents", documents.len());

        // Acquire semaphore permit for concurrency control
        let _permit = self.concurrency_limiter.acquire().await.map_err(|e| {
            RerankingError::InferenceError(format!("Failed to acquire concurrency permit: {e}"))
        })?;

        let response = self
            .client
            .post(&rerank_url)
            .json(&request)
            .send()
            .await
            .map_err(|e| {
                RerankingError::InferenceError(format!("Rerank API request failed: {e}"))
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unable to read error response".to_string());
            return Err(RerankingError::InferenceError(format!(
                "Rerank API returned error {status}: {error_text}"
            ))
            .into());
        }

        let rerank_response: RerankResponse = response.json().await.map_err(|e| {
            RerankingError::InferenceError(format!("Failed to parse rerank response: {e}"))
        })?;

        // Map indices back to document IDs with scores
        let mut scored_docs: Vec<(String, f32)> = rerank_response
            .results
            .into_iter()
            .filter_map(|result| match documents.get(result.index) {
                Some((id, _)) => Some((id.clone(), result.relevance_score)),
                None => {
                    warn!(
                        "Rerank API returned out-of-bounds index {}, dropping result",
                        result.index
                    );
                    None
                }
            })
            .collect();

        // Sort by relevance score descending with NaN handling
        sort_scores_descending(&mut scored_docs);

        debug!("Reranking complete: returned {} results", scored_docs.len());

        Ok(scored_docs)
    }
}
