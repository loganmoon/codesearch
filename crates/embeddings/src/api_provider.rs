//! OpenAI-compatible API provider for embeddings (vLLM, OpenAI, etc.)

use crate::{
    config::EmbeddingConfig,
    error::EmbeddingError,
    provider::{EmbeddingContext, EmbeddingProvider, EmbeddingTask},
};
use async_openai::types::{CreateEmbeddingRequest, EmbeddingInput};
use async_openai::{config::OpenAIConfig, Client};
use async_trait::async_trait;
use codesearch_core::error::Result;
use futures::stream::{self, StreamExt};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;
use tracing::{debug, error, info, warn};

/// Maximum characters per API batch request.
/// Treating 1 char = 1 token to be safe against context overflow.
/// BGE models have 32768 token context limit.
const MAX_BATCH_CHARS: usize = 32768;

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
    /// Instruction prefix for query embeddings (BGE format: `<instruct>{instruction}\n<query>{text}`)
    query_instruction: Option<String>,
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

        // Max characters per individual text (texts exceeding this are skipped).
        let max_context = MAX_BATCH_CHARS;

        Ok(Self {
            client,
            model: config.model,
            dimensions: config.embedding_dimension,
            max_context,
            batch_size: config.texts_per_api_request,
            max_concurrent: config.max_concurrent_api_requests,
            concurrency_limiter: Arc::new(Semaphore::new(config.max_concurrent_api_requests)),
            retry_attempts: config.retry_attempts,
            query_instruction: config.query_instruction,
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
    async fn embed_with_context(
        &self,
        texts: Vec<String>,
        contexts: Option<Vec<EmbeddingContext>>,
    ) -> Result<Vec<Option<Vec<f32>>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        // Initialize results array - None for texts that are skipped
        let mut all_embeddings = vec![None; texts.len()];

        // Step 1: Filter texts by individual size and build batches
        // Each batch entry is (original_index, text, char_count)
        let mut filtered_texts: Vec<(usize, String, usize)> = Vec::new();
        let mut skipped_count = 0;

        for (i, text) in texts.iter().enumerate() {
            let char_count = text.chars().count();
            if char_count <= self.max_context {
                filtered_texts.push((i, text.clone(), char_count));
            } else {
                skipped_count += 1;
                debug!(
                    "Text at index {i} exceeds max_context ({char_count} > {} chars), skipping",
                    self.max_context
                );
            }
        }

        if skipped_count > 0 {
            warn!(
                "Skipped {skipped_count}/{} texts exceeding max length of {} chars",
                texts.len(),
                self.max_context
            );
        }

        if filtered_texts.is_empty() {
            return Ok(all_embeddings);
        }

        // Step 2: Build dynamic batches based on character count
        // Each batch is Vec<(original_index, text)>
        let mut batches: Vec<Vec<(usize, String)>> = Vec::new();
        let mut current_batch: Vec<(usize, String)> = Vec::new();
        let mut current_batch_chars: usize = 0;

        for (orig_idx, text, char_count) in filtered_texts {
            // If adding this text would exceed the batch char limit, start a new batch
            // (unless current batch is empty - a single text that's too large will be sent alone)
            if current_batch_chars + char_count > MAX_BATCH_CHARS && !current_batch.is_empty() {
                batches.push(std::mem::take(&mut current_batch));
                current_batch_chars = 0;
            }

            current_batch.push((orig_idx, text));
            current_batch_chars += char_count;

            // Also respect the configured batch_size as an upper bound
            if current_batch.len() >= self.batch_size {
                batches.push(std::mem::take(&mut current_batch));
                current_batch_chars = 0;
            }
        }

        // Don't forget the last batch
        if !current_batch.is_empty() {
            batches.push(current_batch);
        }

        debug!(
            "Created {} batches for {} texts (max_batch_chars={}, batch_size={})",
            batches.len(),
            texts.len() - skipped_count,
            MAX_BATCH_CHARS,
            self.batch_size
        );

        // Step 3: Process batches concurrently
        let contexts = contexts.map(Arc::new);
        let results = stream::iter(batches)
            .map(|batch| {
                let limiter = self.concurrency_limiter.clone();
                let client = self.client.clone();
                let model = self.model.clone();
                let dimensions = self.dimensions;
                let retry_attempts = self.retry_attempts;
                let contexts = contexts.clone();

                async move {
                    // Extract indices and texts
                    let (indices, texts_to_embed): (Vec<usize>, Vec<String>) =
                        batch.into_iter().unzip();

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
                                // Extract embeddings and sort by index (API response index)
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
                                            "Dimension mismatch: expected {dimensions}, got {}",
                                            embedding.len()
                                        )));
                                    }
                                }

                                // Return pairs of (original_index, embedding)
                                let results: Vec<(usize, Vec<f32>)> = indices
                                    .into_iter()
                                    .zip(sorted_embeddings.into_iter().map(|(_, emb)| emb))
                                    .collect();

                                return Ok::<_, EmbeddingError>(results);
                            }
                            Err(e) if attempt < retry_attempts => {
                                attempt += 1;

                                // Log error with entity context if available
                                error!("Embedding generation failed: {e}");
                                for (batch_idx, orig_idx) in indices.iter().enumerate() {
                                    let char_count = texts_to_embed
                                        .get(batch_idx)
                                        .map(|t| t.chars().count())
                                        .unwrap_or(0);
                                    if let Some(ref ctxs) = contexts {
                                        if let Some(ctx) = ctxs.get(*orig_idx) {
                                            error!("  {} | Chars: {}", ctx, char_count);
                                        } else {
                                            error!("  Text {}: {} chars", orig_idx, char_count);
                                        }
                                    } else {
                                        error!("  Text {}: {} chars", orig_idx, char_count);
                                    }
                                }

                                // Exponential backoff: 10s, 20s, 40s, 60s (capped)
                                // vLLM can take 30-60s to restart after a crash
                                let backoff_secs = (10 * 2u64.pow(attempt as u32 - 1)).min(60);
                                let backoff = Duration::from_secs(backoff_secs);
                                warn!(
                                    "Retrying in {backoff:?} (attempt {attempt}/{retry_attempts})"
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

        // Step 4: Place results back into the original positions
        for result in results {
            let batch_results = result
                .map_err(|e: EmbeddingError| -> codesearch_core::error::Error { e.into() })?;
            for (orig_idx, embedding) in batch_results {
                all_embeddings[orig_idx] = Some(embedding);
            }
        }

        Ok(all_embeddings)
    }

    fn embedding_dimension(&self) -> usize {
        self.dimensions
    }

    fn max_sequence_length(&self) -> usize {
        self.max_context
    }

    async fn embed_for_task(
        &self,
        texts: Vec<String>,
        contexts: Option<Vec<EmbeddingContext>>,
        task: EmbeddingTask,
    ) -> Result<Vec<Option<Vec<f32>>>> {
        match task {
            EmbeddingTask::Query => {
                // Apply BGE instruction prefix for queries
                let formatted_texts = if let Some(ref instruction) = self.query_instruction {
                    texts
                        .into_iter()
                        .map(|text| format!("<instruct>{instruction}\n<query>{text}"))
                        .collect()
                } else {
                    texts
                };
                self.embed_with_context(formatted_texts, contexts).await
            }
            EmbeddingTask::Passage => {
                // Passages are embedded as-is (no instruction prefix)
                self.embed_with_context(texts, contexts).await
            }
        }
    }
}

/// Create a new OpenAI-compatible API provider from configuration
pub async fn create_api_provider(config: EmbeddingConfig) -> Result<Box<dyn EmbeddingProvider>> {
    let provider = OpenAiApiProvider::new(config).await?;
    Ok(Box::new(provider))
}
