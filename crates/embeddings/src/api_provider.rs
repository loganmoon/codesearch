//! OpenAI-compatible API provider for embeddings (vLLM, OpenAI, etc.)

use crate::{config::EmbeddingConfig, error::EmbeddingError, provider::EmbeddingProvider};
use async_openai::types::{CreateEmbeddingRequest, EmbeddingInput};
use async_openai::{config::OpenAIConfig, Client};
use async_trait::async_trait;
use codesearch_core::error::Result;
use std::sync::Arc;
use tokio::sync::Semaphore;
use tracing::{debug, info, warn};

/// OpenAI-compatible API provider
pub struct OpenAiApiProvider {
    client: Client<OpenAIConfig>,
    model: String,
    dimensions: usize,
    max_context: usize,
    batch_size: usize,
    concurrency_limiter: Arc<Semaphore>,
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
            batch_size: config.batch_size,
            concurrency_limiter: Arc::new(Semaphore::new(config.max_workers)),
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

    /// Generate embeddings by calling the API
    async fn embed_batch(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>> {
        debug!("Sending embedding request for {} texts", texts.len());

        // Create embedding request using async-openai types
        let request = CreateEmbeddingRequest {
            model: self.model.clone(),
            input: EmbeddingInput::StringArray(texts),
            encoding_format: None,
            dimensions: None,
            user: None,
        };

        // Call API using async-openai client
        let response = self
            .client
            .embeddings()
            .create(request)
            .await
            .map_err(|e| EmbeddingError::InferenceError(format!("API request failed: {e}")))?;

        // Extract embeddings and sort by index
        let mut embeddings: Vec<(usize, Vec<f32>)> = response
            .data
            .into_iter()
            .map(|embedding_data| (embedding_data.index as usize, embedding_data.embedding))
            .collect();
        embeddings.sort_by_key(|(idx, _)| *idx);

        // Validate dimensions
        let results: Vec<Vec<f32>> = embeddings
            .into_iter()
            .map(|(_, embedding)| -> Result<Vec<f32>> {
                if embedding.len() != self.dimensions {
                    return Err(EmbeddingError::InferenceError(format!(
                        "Dimension mismatch: expected {}, got {}",
                        self.dimensions,
                        embedding.len()
                    ))
                    .into());
                }
                Ok(embedding)
            })
            .collect::<Result<Vec<_>>>()?;

        debug!("Received {} embeddings", results.len());
        Ok(results)
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

        for chunk in chunks {
            // Pre-filter texts by length (simple char-based heuristic)
            let mut texts_to_embed = Vec::new();
            let mut indices_to_embed = Vec::new();
            let mut chunk_results = vec![None; chunk.len()];

            for (i, text) in chunk.iter().enumerate() {
                if text.chars().count() <= self.max_context {
                    texts_to_embed.push(text.clone());
                    indices_to_embed.push(i);
                } else {
                    debug!(
                        "Text at index {} exceeds max_context ({} > {}), skipping",
                        i,
                        text.len(),
                        self.max_context
                    );
                }
                // Texts exceeding limit remain as None
            }

            if !texts_to_embed.is_empty() {
                // Acquire semaphore permit for concurrency control
                let permit = self
                    .concurrency_limiter
                    .clone()
                    .acquire_owned()
                    .await
                    .map_err(|e| {
                        EmbeddingError::InferenceError(format!(
                            "Failed to acquire concurrency permit: {e}"
                        ))
                    })?;

                // Generate embeddings
                let mut embeddings = {
                    let _permit = permit; // Keep permit alive
                    self.embed_batch(texts_to_embed).await?
                };

                // Place embeddings at their original indices (consuming vector)
                for orig_idx in indices_to_embed.into_iter() {
                    chunk_results[orig_idx] = Some(embeddings.swap_remove(0));
                }
            }

            all_embeddings.extend(chunk_results);
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
