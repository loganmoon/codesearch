//! OpenAI-compatible API provider for embeddings (vLLM, OpenAI, etc.)

use crate::{config::EmbeddingConfig, error::EmbeddingError, provider::EmbeddingProvider};
use async_openai::types::{CreateEmbeddingRequest, EmbeddingInput};
use async_openai::{config::OpenAIConfig, Client};
use async_trait::async_trait;
use codesearch_core::error::Result;
use futures::stream::{self, StreamExt};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;
use tracing::{debug, info, warn};

/// OpenAI-compatible API provider
pub struct OpenAiApiProvider {
    client: Client<OpenAIConfig>,
    model: String,
    dimensions: usize,
    max_context: usize,
    batch_size: usize,
    max_concurrent: usize,
    concurrency_limiter: Arc<Semaphore>,
    retry_attempts: usize,
}

impl OpenAiApiProvider {
    /// Create a new API provider from configuration
    pub(crate) async fn new(config: EmbeddingConfig) -> Result<Self> {
        // Validate configuration
        config
            .validate()
            .map_err(|e| EmbeddingError::ModelLoadError(format!("Invalid configuration: {e}")))?;

        info!("Initializing OpenAI-compatible API embeddings");
        info!("  Model: {}", config.model);
        info!("  Dimensions: {}", config.embedding_dimension);
        info!("  Batch size: {}", config.texts_per_api_request);
        info!(
            "  Max concurrent requests: {}",
            config.max_concurrent_api_requests
        );
        info!("  Retry attempts: {}", config.retry_attempts);

        // Get base URL (required)
        let base_url = config
            .api_base_url
            .clone()
            .unwrap_or_else(|| "http://localhost:8000/v1".to_string());

        info!("  Base URL: {}", base_url);

        // Configure async-openai client with custom base URL
        let mut openai_config = OpenAIConfig::new();
        openai_config = openai_config.with_api_base(&base_url);

        // Set API key if provided
        if let Some(ref api_key) = config.api_key {
            openai_config = openai_config.with_api_key(api_key);
        }

        let client = Client::with_config(openai_config);

        // Perform health check (warn on failure, don't block)
        Self::check_health(&client).await;

        // Use a reasonable max_context default
        // Simple heuristic: ~4 chars per token with safety margin
        let max_context = (32768.0f64 * 4.0f64 * 0.8f64).floor() as usize;

        Ok(Self {
            client,
            model: config.model,
            dimensions: config.embedding_dimension,
            max_context,
            batch_size: config.texts_per_api_request,
            max_concurrent: config.max_concurrent_api_requests,
            concurrency_limiter: Arc::new(Semaphore::new(config.max_concurrent_api_requests)),
            retry_attempts: config.retry_attempts,
        })
    }

    /// Check if the API is healthy (non-blocking, warns on failure)
    async fn check_health(client: &Client<OpenAIConfig>) {
        debug!("Checking API health via /v1/models endpoint");

        match client.models().list().await {
            Ok(models_response) => {
                info!("API health check passed");
                debug!("  Available models: {}", models_response.data.len());
            }
            Err(e) => {
                warn!("API health check failed: {e}");
                warn!("  The vLLM service may not be running or still starting up.");
                warn!("  It can take 30-60 seconds for the service to become available.");
                warn!(
                    "  If you're using docker compose, check: docker compose logs vllm-embeddings"
                );
            }
        }
    }
}

#[async_trait]
impl EmbeddingProvider for OpenAiApiProvider {
    async fn embed(&self, texts: Vec<String>) -> Result<Vec<Option<Vec<f32>>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        let mut all_embeddings = Vec::with_capacity(texts.len());

        // Process in batches with concurrency control
        let chunks: Vec<_> = texts
            .chunks(self.batch_size)
            .map(|chunk| chunk.to_vec())
            .collect();

        let results = stream::iter(chunks)
            .map(|chunk| {
                let limiter = self.concurrency_limiter.clone();
                let client = self.client.clone();
                let model = self.model.clone();
                let max_context = self.max_context;
                let dimensions = self.dimensions;
                let retry_attempts = self.retry_attempts;

                async move {
                    // Pre-filter texts by length (simple char-based heuristic)
                    let mut texts_to_embed = Vec::new();
                    let mut indices_to_embed = Vec::new();
                    let mut chunk_results = vec![None; chunk.len()];

                    let mut skipped_count = 0;
                    for (i, text) in chunk.iter().enumerate() {
                        let char_count = text.chars().count();
                        if char_count <= max_context {
                            texts_to_embed.push(text.clone());
                            indices_to_embed.push(i);
                        } else {
                            skipped_count += 1;
                            debug!(
                                "Text at index {i} exceeds max_context ({char_count} > {max_context} chars), skipping"
                            );
                        }
                        // Texts exceeding limit remain as None
                    }
                    if skipped_count > 0 {
                        warn!(
                            "Skipped {skipped_count}/{} texts exceeding max length of {max_context} chars",
                            chunk.len()
                        );
                    }

                    if texts_to_embed.is_empty() {
                        return Ok::<_, EmbeddingError>(chunk_results);
                    }

                    // Acquire semaphore permit for concurrency control
                    let _permit = limiter.acquire_owned().await.map_err(|e| {
                        EmbeddingError::InferenceError(format!(
                            "Failed to acquire concurrency permit: {e}"
                        ))
                    })?;

                    // Retry loop with exponential backoff
                    let mut attempt = 0;

                    loop {
                        // Generate embeddings via API call
                        let request = CreateEmbeddingRequest {
                            model: model.clone(),
                            input: EmbeddingInput::StringArray(texts_to_embed.clone()),
                            encoding_format: None,
                            dimensions: None,
                            user: None,
                        };

                        match client.embeddings().create(request).await {
                            Ok(response) => {
                                // Extract embeddings and sort by index
                                let mut sorted_embeddings: Vec<(usize, Vec<f32>)> = response
                                    .data
                                    .into_iter()
                                    .map(|emb| (emb.index as usize, emb.embedding))
                                    .collect();
                                sorted_embeddings.sort_by_key(|(idx, _)| *idx);

                                // Validate dimensions
                                for (_, embedding) in &sorted_embeddings {
                                    if embedding.len() != dimensions {
                                        return Err(EmbeddingError::InferenceError(format!(
                                            "Dimension mismatch: expected {}, got {}",
                                            dimensions,
                                            embedding.len()
                                        )));
                                    }
                                }

                                // Place embeddings at their original indices
                                for (result_idx, orig_idx) in
                                    indices_to_embed.into_iter().enumerate()
                                {
                                    chunk_results[orig_idx] =
                                        Some(sorted_embeddings[result_idx].1.clone());
                                }

                                return Ok(chunk_results);
                            }
                            Err(e) if attempt < retry_attempts => {
                                attempt += 1;

                                // Exponential backoff: 10s, 20s, 40s, 60s (capped)
                                // vLLM can take 30-60s to restart after a crash
                                let backoff_secs = (10 * 2u64.pow(attempt as u32 - 1)).min(60);
                                let backoff = Duration::from_secs(backoff_secs);
                                warn!(
                                    "Embedding batch failed (attempt {}/{}), retrying in {:?}: {}",
                                    attempt, retry_attempts, backoff, e
                                );
                                tokio::time::sleep(backoff).await;
                            }
                            Err(e) => {
                                return Err(EmbeddingError::InferenceError(format!(
                                    "API request failed after {retry_attempts} attempts: {e}"
                                )));
                            }
                        }
                    }
                }
            })
            .buffer_unordered(self.max_concurrent)
            .collect::<Vec<_>>()
            .await;

        // Flatten results
        for result in results {
            all_embeddings.extend(
                result
                    .map_err(|e: EmbeddingError| -> codesearch_core::error::Error { e.into() })?,
            );
        }

        Ok(all_embeddings)
    }

    fn embedding_dimension(&self) -> usize {
        self.dimensions
    }

    fn max_sequence_length(&self) -> usize {
        // Return as tokens (char count / 4 as rough estimate)
        self.max_context / 4
    }
}

/// Create a new OpenAI-compatible API provider from configuration
pub async fn create_api_provider(config: EmbeddingConfig) -> Result<Box<dyn EmbeddingProvider>> {
    let provider = OpenAiApiProvider::new(config).await?;
    Ok(Box::new(provider))
}
