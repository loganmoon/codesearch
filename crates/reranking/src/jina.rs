//! Jina AI reranker provider

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

const JINA_API_URL: &str = "https://api.jina.ai/v1/rerank";

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

        let doc_texts: Vec<String> = documents
            .iter()
            .map(|(_, content)| (*content).to_string())
            .collect();

        let request = JinaRerankRequest {
            model: self.model.clone(),
            query: query.to_string(),
            documents: doc_texts,
            top_n: documents.len(),
            return_documents: false,
        };

        debug!(
            "Sending Jina rerank request for {} documents",
            documents.len()
        );

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
                RerankingError::InferenceError(format!("Jina rerank API request failed: {e}"))
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unable to read error response".to_string());
            return Err(RerankingError::InferenceError(format!(
                "Jina rerank API returned error {status}: {error_text}"
            ))
            .into());
        }

        let rerank_response: JinaRerankResponse = response.json().await.map_err(|e| {
            RerankingError::InferenceError(format!("Failed to parse Jina rerank response: {e}"))
        })?;

        // Map indices back to document IDs with scores
        let mut scored_docs: Vec<(String, f32)> = rerank_response
            .results
            .into_iter()
            .filter_map(|result| match documents.get(result.index) {
                Some((id, _)) => Some((id.clone(), result.relevance_score)),
                None => {
                    warn!(
                        "Jina rerank API returned out-of-bounds index {}, dropping result",
                        result.index
                    );
                    None
                }
            })
            .collect();

        // Sort by relevance score descending with NaN handling
        sort_scores_descending(&mut scored_docs);

        debug!(
            "Jina reranking complete: returned {} results",
            scored_docs.len()
        );

        Ok(scored_docs)
    }
}
