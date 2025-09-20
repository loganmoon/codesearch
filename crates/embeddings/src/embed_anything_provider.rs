//! Flexible embeddings using embed_anything with support for any HuggingFace model

use crate::{config::EmbeddingConfig, error::EmbeddingError, provider::EmbeddingProvider};
use async_trait::async_trait;
use codesearch_core::error::Result;
use embed_anything::embeddings::local::bert::BertEmbedder;
use embed_anything::{
    embed_query,
    embeddings::embed::{Embedder, TextEmbedder},
};
use hf_hub::api::tokio::Api;
use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::Semaphore;
use tracing::{debug, info};

/// HuggingFace model configuration structure
#[derive(Deserialize, Debug)]
struct HuggingFaceConfig {
    hidden_size: usize,
    max_position_embeddings: usize,
    #[allow(dead_code)]
    num_attention_heads: Option<usize>,
    #[allow(dead_code)]
    num_hidden_layers: Option<usize>,
    #[allow(dead_code)]
    vocab_size: Option<usize>,
    #[allow(dead_code)]
    model_type: Option<String>,
}

/// Model ID mappings for common aliases
fn resolve_model_id(model: &str) -> &str {
    match model {
        "sfr-small" | "small" => "Salesforce/SFR-Embedding-Code-400M_R",
        "sfr-large" | "large" => "Salesforce/SFR-Embedding-Code-2B_R",
        custom => custom,
    }
}

/// Get model metadata from HuggingFace
async fn get_model_metadata(model_id: &str) -> Result<(usize, usize)> {
    let api = Api::new()
        .map_err(|e| EmbeddingError::ModelLoadError(format!("Failed to initialize HF API: {e}")))?;

    let repo = api.model(model_id.to_string());
    let config_path = repo.get("config.json").await.map_err(|e| {
        EmbeddingError::ModelLoadError(format!(
            "Failed to download config.json for {model_id}: {e}"
        ))
    })?;

    let config_content = tokio::fs::read_to_string(&config_path)
        .await
        .map_err(|e| EmbeddingError::ModelLoadError(format!("Failed to read config.json: {e}")))?;

    let config: HuggingFaceConfig = serde_json::from_str(&config_content)
        .map_err(|e| EmbeddingError::ModelLoadError(format!("Failed to parse config.json: {e}")))?;

    info!(
        "Model {model_id} metadata: hidden_size={}, max_position_embeddings={}",
        config.hidden_size, config.max_position_embeddings
    );
    Ok((config.hidden_size, config.max_position_embeddings))
}

/// Flexible embeddings implementation using embed_anything
pub struct EmbedAnythingProvider {
    embedder: Arc<Embedder>,
    dimensions: usize,
    max_context: usize,
    batch_size: usize,
    /// Semaphore for controlling concurrent embedding operations
    concurrency_limiter: Arc<Semaphore>,
}

impl EmbedAnythingProvider {
    /// Create a new provider from configuration (crate-private)
    pub(crate) async fn new(config: EmbeddingConfig) -> Result<Self> {
        // Validate configuration
        config
            .validate()
            .map_err(|e| EmbeddingError::ModelLoadError(format!("Invalid configuration: {e}")))?;

        info!("Initializing embeddings with model: {}", config.model);

        // Resolve model ID from aliases
        let model_id = resolve_model_id(&config.model);
        debug!("Using model ID: {model_id}");

        // Get model metadata from HuggingFace config
        let (dimensions, max_context) = get_model_metadata(model_id).await?;

        // Create BertEmbedder with the specified model
        let model_id_owned = model_id.to_string();
        let bert_embedder = tokio::task::spawn_blocking(move || {
            BertEmbedder::new(
                model_id_owned,
                None, // No specific revision
                None, // No auth token
            )
        })
        .await
        .map_err(|e| {
            if e.is_panic() {
                EmbeddingError::ModelLoadError("Model loading panicked".to_string())
            } else {
                EmbeddingError::ModelLoadError(
                    "Runtime shutting down during model load".to_string(),
                )
            }
        })?
        .map_err(|e| EmbeddingError::ModelLoadError(format!("Failed to load model: {e:?}")))?;

        // Create embedder
        let embedder = Arc::new(Embedder::Text(TextEmbedder::Bert(Box::new(bert_embedder))));

        info!(
            "Embeddings initialized with model '{}', {} dimensions, max context: {}",
            model_id, dimensions, max_context
        );

        Ok(Self {
            embedder,
            dimensions,
            max_context,
            batch_size: config.batch_size,
            concurrency_limiter: Arc::new(Semaphore::new(config.max_workers)),
        })
    }
}

#[async_trait]
impl EmbeddingProvider for EmbedAnythingProvider {
    async fn embed(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        // Pre-calculate total capacity
        let mut all_embeddings = Vec::with_capacity(texts.len());

        // Process chunks with semaphore-based concurrency control
        let chunks: Vec<_> = texts
            .chunks(self.batch_size)
            .map(|chunk| chunk.to_vec())
            .collect();

        for chunk in chunks {
            // Acquire semaphore permit for this embedding operation
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

            // Clone necessary data for the async operation
            let embedder = Arc::clone(&self.embedder);
            let expected_dim = self.dimensions;

            // Perform embedding generation with controlled concurrency
            let chunk_embeddings = tokio::spawn(async move {
                // Keep the permit alive for the duration of the operation
                let _permit = permit;

                // Use embed_query for embedding generation
                let embeddings = embed_query(
                    &chunk.iter().map(String::as_str).collect::<Vec<_>>(),
                    &embedder,
                    None,
                )
                .await
                .map_err(|e| {
                    EmbeddingError::InferenceError(format!("Embedding generation failed: {e:?}"))
                })?;

                // Extract and convert embeddings
                let mut chunk_results = Vec::with_capacity(embeddings.len());
                for embed_data in embeddings {
                    let dense_vec = embed_data.embedding.to_dense().map_err(|e| {
                        EmbeddingError::InferenceError(format!(
                            "Failed to extract dense vector: {e:?}"
                        ))
                    })?;

                    // Validate dimensions
                    if dense_vec.len() != expected_dim {
                        return Err(EmbeddingError::InferenceError(format!(
                            "Dimension mismatch: expected {expected_dim}, got {}",
                            dense_vec.len()
                        )));
                    }

                    chunk_results.push(dense_vec);
                }

                Ok::<Vec<Vec<f32>>, EmbeddingError>(chunk_results)
            })
            .await
            .map_err(|e| {
                if e.is_panic() {
                    EmbeddingError::InferenceError("Embedding task panicked".to_string())
                } else {
                    EmbeddingError::InferenceError(
                        "Runtime shutting down during embedding".to_string(),
                    )
                }
            })??;

            all_embeddings.extend(chunk_embeddings);
        }

        Ok(all_embeddings)
    }

    fn embedding_dimension(&self) -> usize {
        self.dimensions
    }

    fn max_sequence_length(&self) -> usize {
        self.max_context
    }
}

/// Create a new embed_anything provider from configuration
pub async fn create_embed_anything_provider(
    config: EmbeddingConfig,
) -> Result<Box<dyn EmbeddingProvider>> {
    let provider = EmbedAnythingProvider::new(config).await?;
    Ok(Box::new(provider))
}
