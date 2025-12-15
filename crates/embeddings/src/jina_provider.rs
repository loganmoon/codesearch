//! Jina AI embedding provider
//!
//! This provider uses the Jina Embeddings API to generate embeddings.
//! It supports task-aware embedding via the `task` parameter in API requests.

use crate::{
    error::EmbeddingError,
    provider::{EmbeddingContext, EmbeddingProvider, EmbeddingTask},
};
use async_trait::async_trait;
use codesearch_core::error::Result;
use futures::stream::{self, StreamExt};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;
use tracing::{debug, info, warn};

/// Jina API endpoint for embeddings
const JINA_API_URL: &str = "https://api.jina.ai/v1/embeddings";

/// Maximum characters per batch request for Jina API.
/// Jina has 32K token limit; using ~2 chars/token for code gives 64K chars.
const JINA_MAX_BATCH_CHARS: usize = 65536;

/// Maximum number of texts per batch (Jina API limit)
const JINA_MAX_BATCH_SIZE: usize = 100;

/// Maximum characters per individual text (matches Jina's ~32K token context)
const JINA_MAX_TEXT_CHARS: usize = 65536;

/// Sanitize text for Jina API by removing problematic characters.
///
/// Allows the Basic Multilingual Plane (U+0000-U+FFFF) which covers all major
/// world languages, but blocks:
/// - Control characters (except tab, newline, CR)
/// - Emoji ranges within the BMP
/// - Everything above U+FFFF (supplementary planes where most emoji live)
fn sanitize_text(text: &str) -> String {
    text.chars()
        .filter(|&c| {
            // Allow standard whitespace
            if c == '\t' || c == '\n' || c == '\r' {
                return true;
            }
            // Block control characters
            if c.is_control() {
                return false;
            }
            let cp = c as u32;
            // Block everything outside BMP (U+10000+) - this is where most emoji live
            if cp > 0xFFFF {
                return false;
            }
            // Block emoji/symbol ranges within BMP
            if (0x2600..=0x27BF).contains(&cp)  // Misc symbols & dingbats
                || (0x2300..=0x23FF).contains(&cp)  // Misc technical (some emoji)
                || (0x2B00..=0x2BFF).contains(&cp)  // Misc symbols and arrows
                || (0xFE00..=0xFE0F).contains(&cp)  // Variation selectors
                || (0x200B..=0x200F).contains(&cp)  // Zero-width chars
                || (0x202A..=0x202E).contains(&cp)  // Directional formatting
                || (0x2060..=0x206F).contains(&cp)  // Invisible operators
                || (0xE000..=0xF8FF).contains(&cp)  // Private use area
                || (0xFFF0..=0xFFFF).contains(&cp)
            // Specials
            {
                return false;
            }
            true
        })
        .collect()
}

/// Request payload for Jina embeddings API
#[derive(Debug, Serialize)]
struct JinaEmbeddingRequest<'a> {
    model: &'a str,
    input: &'a [String],
    task: &'a str,
    /// Output embedding dimensions (Matryoshka truncation)
    dimensions: usize,
    /// Drop content exceeding max token length instead of erroring
    truncate: bool,
}

/// Response from Jina embeddings API
#[derive(Debug, Deserialize)]
struct JinaEmbeddingResponse {
    data: Vec<JinaEmbedding>,
}

/// Individual embedding from Jina
#[derive(Debug, Deserialize)]
struct JinaEmbedding {
    index: usize,
    embedding: Vec<f32>,
}

/// Jina AI embedding provider
pub struct JinaEmbeddingProvider {
    client: Client,
    api_key: String,
    model: String,
    dimensions: usize,
    batch_size: usize,
    max_concurrent: usize,
    concurrency_limiter: Arc<Semaphore>,
    retry_attempts: usize,
}

impl JinaEmbeddingProvider {
    /// Create a new Jina embedding provider
    ///
    /// # Arguments
    /// * `api_key` - Jina API key for authentication
    /// * `model` - Model name (e.g., "jina-embeddings-v3")
    /// * `dimensions` - Embedding dimensions (e.g., 1024)
    /// * `batch_size` - Maximum texts per API request
    /// * `max_concurrent` - Maximum concurrent API requests
    /// * `retry_attempts` - Number of retry attempts for failed requests
    pub fn new(
        api_key: String,
        model: String,
        dimensions: usize,
        batch_size: usize,
        max_concurrent: usize,
        retry_attempts: usize,
    ) -> Result<Self> {
        info!("Initializing Jina embedding provider");
        info!("  Model: {model}");
        info!("  Dimensions: {dimensions}");
        info!("  Batch size: {batch_size}");
        info!("  Max concurrent requests: {max_concurrent}");

        let client = Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .map_err(|e| {
                EmbeddingError::ModelLoadError(format!("Failed to create HTTP client: {e}"))
            })?;

        // Clamp batch_size to Jina's limit
        let batch_size = batch_size.min(JINA_MAX_BATCH_SIZE);

        Ok(Self {
            client,
            api_key,
            model,
            dimensions,
            batch_size,
            max_concurrent,
            concurrency_limiter: Arc::new(Semaphore::new(max_concurrent)),
            retry_attempts,
        })
    }

    /// Get the Jina task string for the given embedding task
    /// For jina-embeddings-v3, valid tasks are: retrieval.query, retrieval.passage, text-matching, classification
    fn task_string(&self, task: EmbeddingTask) -> &'static str {
        match task {
            EmbeddingTask::Query => "retrieval.query",
            EmbeddingTask::Passage => "retrieval.passage",
        }
    }

    /// Embed a batch of texts with the given task type
    async fn embed_batch(
        &self,
        texts: &[String],
        task: EmbeddingTask,
    ) -> std::result::Result<Vec<Vec<f32>>, EmbeddingError> {
        let task_str = self.task_string(task);

        let request = JinaEmbeddingRequest {
            model: &self.model,
            input: texts,
            task: task_str,
            dimensions: self.dimensions,
            truncate: true,
        };

        // Acquire semaphore permit for concurrency control
        let _permit = self.concurrency_limiter.acquire().await.map_err(|e| {
            EmbeddingError::InferenceError(format!("Failed to acquire concurrency permit: {e}"))
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
                EmbeddingError::InferenceError(format!("Jina embedding API request failed: {e}"))
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unable to read error response".to_string());
            return Err(EmbeddingError::InferenceError(format!(
                "Jina embedding API returned error {status}: {error_text}"
            )));
        }

        let embedding_response: JinaEmbeddingResponse = response.json().await.map_err(|e| {
            EmbeddingError::InferenceError(format!("Failed to parse Jina embedding response: {e}"))
        })?;

        // Sort by index and extract embeddings
        let mut indexed_embeddings: Vec<(usize, Vec<f32>)> = embedding_response
            .data
            .into_iter()
            .map(|e| (e.index, e.embedding))
            .collect();
        indexed_embeddings.sort_by_key(|(idx, _)| *idx);

        // Validate dimensions
        for (idx, embedding) in &indexed_embeddings {
            if embedding.len() != self.dimensions {
                return Err(EmbeddingError::InferenceError(format!(
                    "Dimension mismatch at index {idx}: expected {}, got {}",
                    self.dimensions,
                    embedding.len()
                )));
            }
        }

        Ok(indexed_embeddings.into_iter().map(|(_, e)| e).collect())
    }

    /// Internal implementation of embedding with task support
    async fn embed_internal(
        &self,
        texts: Vec<String>,
        _contexts: Option<Vec<EmbeddingContext>>,
        task: EmbeddingTask,
    ) -> Result<Vec<Option<Vec<f32>>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        // Initialize results array - None for texts that are skipped
        let mut all_embeddings = vec![None; texts.len()];

        // Step 1: Sanitize texts and filter by size
        let mut filtered_texts: Vec<(usize, String, usize)> = Vec::new();
        let mut skipped_count = 0;

        for (i, text) in texts.iter().enumerate() {
            // Sanitize text to remove control characters that Jina can't encode
            let sanitized = sanitize_text(text);
            let char_count = sanitized.chars().count();
            if char_count <= JINA_MAX_TEXT_CHARS {
                filtered_texts.push((i, sanitized, char_count));
            } else {
                skipped_count += 1;
                debug!(
                    "Text at index {i} exceeds max chars ({char_count} > {JINA_MAX_TEXT_CHARS}), skipping"
                );
            }
        }

        if skipped_count > 0 {
            warn!(
                "Skipped {skipped_count}/{} texts exceeding max length of {JINA_MAX_TEXT_CHARS} chars",
                texts.len()
            );
        }

        if filtered_texts.is_empty() {
            return Ok(all_embeddings);
        }

        // Step 2: Build dynamic batches based on character count
        let mut batches: Vec<Vec<(usize, String)>> = Vec::new();
        let mut current_batch: Vec<(usize, String)> = Vec::new();
        let mut current_batch_chars: usize = 0;

        for (orig_idx, text, char_count) in filtered_texts {
            // Start new batch if adding this text would exceed limits
            if (current_batch_chars + char_count > JINA_MAX_BATCH_CHARS
                || current_batch.len() >= self.batch_size)
                && !current_batch.is_empty()
            {
                batches.push(std::mem::take(&mut current_batch));
                current_batch_chars = 0;
            }

            current_batch.push((orig_idx, text));
            current_batch_chars += char_count;
        }

        // Don't forget the last batch
        if !current_batch.is_empty() {
            batches.push(current_batch);
        }

        debug!(
            "Created {} batches for {} texts (max_batch_chars={JINA_MAX_BATCH_CHARS}, batch_size={})",
            batches.len(),
            texts.len() - skipped_count,
            self.batch_size
        );

        // Step 3: Process batches concurrently with retries
        let retry_attempts = self.retry_attempts;
        let results = stream::iter(batches)
            .map(|batch| async move {
                let (indices, texts_to_embed): (Vec<usize>, Vec<String>) =
                    batch.into_iter().unzip();

                let mut last_error = None;
                for attempt in 0..=retry_attempts {
                    if attempt > 0 {
                        let delay = Duration::from_millis(100 * 2u64.pow(attempt as u32 - 1));
                        warn!(
                            "Retrying Jina embedding batch (attempt {}/{})",
                            attempt + 1,
                            retry_attempts + 1
                        );
                        tokio::time::sleep(delay).await;
                    }

                    match self.embed_batch(&texts_to_embed, task).await {
                        Ok(embeddings) => {
                            let results: Vec<(usize, Vec<f32>)> =
                                indices.iter().copied().zip(embeddings).collect();
                            return Ok::<_, EmbeddingError>(results);
                        }
                        Err(e) => {
                            last_error = Some(e);
                        }
                    }
                }

                Err(last_error
                    .unwrap_or_else(|| EmbeddingError::InferenceError("Unknown error".to_string())))
            })
            .buffer_unordered(self.max_concurrent)
            .collect::<Vec<_>>()
            .await;

        // Step 4: Place results back into original positions
        for result in results {
            let batch_results = result
                .map_err(|e: EmbeddingError| -> codesearch_core::error::Error { e.into() })?;
            for (orig_idx, embedding) in batch_results {
                all_embeddings[orig_idx] = Some(embedding);
            }
        }

        Ok(all_embeddings)
    }
}

#[async_trait]
impl EmbeddingProvider for JinaEmbeddingProvider {
    async fn embed_with_context(
        &self,
        texts: Vec<String>,
        contexts: Option<Vec<EmbeddingContext>>,
    ) -> Result<Vec<Option<Vec<f32>>>> {
        // Default to Passage task for backward compatibility
        self.embed_internal(texts, contexts, EmbeddingTask::Passage)
            .await
    }

    async fn embed_for_task(
        &self,
        texts: Vec<String>,
        contexts: Option<Vec<EmbeddingContext>>,
        task: EmbeddingTask,
    ) -> Result<Vec<Option<Vec<f32>>>> {
        self.embed_internal(texts, contexts, task).await
    }

    fn embedding_dimension(&self) -> usize {
        self.dimensions
    }

    fn max_sequence_length(&self) -> usize {
        JINA_MAX_TEXT_CHARS
    }
}

/// Create a new Jina embedding provider
pub async fn create_jina_provider(
    api_key: String,
    model: String,
    dimensions: usize,
    batch_size: usize,
    max_concurrent: usize,
    retry_attempts: usize,
) -> Result<Box<dyn EmbeddingProvider>> {
    let provider = JinaEmbeddingProvider::new(
        api_key,
        model,
        dimensions,
        batch_size,
        max_concurrent,
        retry_attempts,
    )?;
    Ok(Box::new(provider))
}
