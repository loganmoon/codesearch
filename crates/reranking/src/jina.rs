//! Jina AI reranker provider

use crate::error::RerankingError;
use crate::sort_scores_descending;
use crate::RerankerProvider;
use async_trait::async_trait;
use codesearch_core::error::Result;
use futures::future::join_all;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Semaphore;
use tracing::{info, warn};

const JINA_API_URL: &str = "https://api.jina.ai/v1/rerank";

/// Maximum documents per batch to avoid timeouts with large payloads
const BATCH_SIZE: usize = 25;

/// Request payload for Jina rerank API
#[derive(Debug, Serialize)]
struct JinaRerankRequest {
    model: String,
    query: String,
    documents: Vec<String>,
    top_n: usize,
    return_documents: bool,
}

/// Response from Jina rerank API
#[derive(Debug, Deserialize)]
struct JinaRerankResponse {
    results: Vec<JinaRerankResult>,
}

/// Individual rerank result from Jina
#[derive(Debug, Deserialize)]
struct JinaRerankResult {
    index: usize,
    relevance_score: f32,
}

/// Jina AI reranker provider
pub struct JinaRerankerProvider {
    client: Client,
    api_key: String,
    model: String,
    concurrency_limiter: Arc<Semaphore>,
}

impl JinaRerankerProvider {
    /// Create a new Jina reranker provider
    ///
    /// # Arguments
    /// * `api_key` - Jina API key for authentication
    /// * `model` - Model name (e.g., "jina-reranker-v3")
    /// * `timeout_secs` - Request timeout in seconds
    /// * `max_concurrent_requests` - Maximum concurrent API requests
    pub fn new(
        api_key: String,
        model: String,
        timeout_secs: u64,
        max_concurrent_requests: usize,
    ) -> Result<Self> {
        info!("Initializing Jina reranker provider");
        info!("  Model: {model}");
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
            api_key,
            model,
            concurrency_limiter: Arc::new(Semaphore::new(max_concurrent_requests)),
        })
    }
}

#[async_trait]
impl RerankerProvider for JinaRerankerProvider {
    async fn rerank(
        &self,
        query: &str,
        documents: &[(String, &str)],
    ) -> Result<Vec<(String, f32)>> {
        if documents.is_empty() {
            return Ok(Vec::new());
        }

        // Calculate total content size for logging
        let total_content_bytes: usize = documents.iter().map(|(_, c)| c.len()).sum();
        let num_batches = documents.len().div_ceil(BATCH_SIZE);

        info!(
            "Jina rerank: {} documents ({} KB) in {} batches",
            documents.len(),
            total_content_bytes / 1024,
            num_batches
        );

        // Split into batches and process in parallel
        let batch_futures: Vec<_> = documents
            .chunks(BATCH_SIZE)
            .enumerate()
            .map(|(batch_idx, batch)| self.rerank_batch(query, batch, batch_idx))
            .collect();

        let batch_results = join_all(batch_futures).await;

        // Merge results from all batches
        let mut all_scored: Vec<(String, f32)> = Vec::with_capacity(documents.len());
        let mut failed_batches = 0;

        for (batch_idx, result) in batch_results.into_iter().enumerate() {
            match result {
                Ok(scored) => all_scored.extend(scored),
                Err(e) => {
                    warn!("Batch {batch_idx} failed: {e}");
                    failed_batches += 1;
                }
            }
        }

        if failed_batches > 0 {
            warn!(
                "Jina rerank: {failed_batches}/{num_batches} batches failed, returning partial results"
            );
        }

        // Sort merged results by relevance score descending
        sort_scores_descending(&mut all_scored);

        info!(
            "Jina reranking complete: {} results from {} batches",
            all_scored.len(),
            num_batches - failed_batches
        );

        Ok(all_scored)
    }
}

impl JinaRerankerProvider {
    /// Rerank a single batch of documents
    async fn rerank_batch(
        &self,
        query: &str,
        batch: &[(String, &str)],
        batch_idx: usize,
    ) -> Result<Vec<(String, f32)>> {
        let doc_texts: Vec<String> = batch
            .iter()
            .map(|(_, content)| (*content).to_string())
            .collect();
        let batch_bytes: usize = doc_texts.iter().map(|d| d.len()).sum();

        let request = JinaRerankRequest {
            model: self.model.clone(),
            query: query.to_string(),
            documents: doc_texts,
            top_n: batch.len(),
            return_documents: false,
        };

        // Acquire semaphore permit for concurrency control
        let _permit = self.concurrency_limiter.acquire().await.map_err(|e| {
            RerankingError::InferenceError(format!("Failed to acquire concurrency permit: {e}"))
        })?;

        let response = self
            .client
            .post(JINA_API_URL)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await
            .map_err(|e| {
                let error_kind = if e.is_timeout() {
                    "timeout"
                } else if e.is_connect() {
                    "connection"
                } else if e.is_request() {
                    "request build"
                } else if e.is_body() {
                    "body"
                } else {
                    "unknown"
                };
                warn!(
                    "Batch {batch_idx} failed ({}): {} - {} docs, {} KB",
                    error_kind,
                    e,
                    batch.len(),
                    batch_bytes / 1024
                );
                RerankingError::InferenceError(format!(
                    "Jina rerank batch {batch_idx} failed ({}): {}",
                    error_kind, e
                ))
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unable to read error response".to_string());
            warn!(
                "Batch {batch_idx} API error {} - {} docs, {} KB: {}",
                status,
                batch.len(),
                batch_bytes / 1024,
                error_text
            );
            return Err(RerankingError::InferenceError(format!(
                "Jina rerank batch {batch_idx} returned error {status}: {error_text}"
            ))
            .into());
        }

        let rerank_response: JinaRerankResponse = response.json().await.map_err(|e| {
            RerankingError::InferenceError(format!(
                "Failed to parse Jina rerank response for batch {batch_idx}: {e}"
            ))
        })?;

        // Map indices back to document IDs with scores
        let scored_docs: Vec<(String, f32)> = rerank_response
            .results
            .into_iter()
            .filter_map(|result| match batch.get(result.index) {
                Some((id, _)) => Some((id.clone(), result.relevance_score)),
                None => {
                    warn!(
                        "Batch {batch_idx}: out-of-bounds index {}, dropping",
                        result.index
                    );
                    None
                }
            })
            .collect();

        Ok(scored_docs)
    }
}
