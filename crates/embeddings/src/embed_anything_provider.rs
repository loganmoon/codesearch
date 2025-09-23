//! Flexible embeddings using embed_anything with support for any HuggingFace model

use crate::{config::EmbeddingConfig, error::EmbeddingError, provider::EmbeddingProvider};
use async_trait::async_trait;
use codesearch_core::error::Result;
use embed_anything::embeddings::local::jina::{JinaEmbed, JinaEmbedder};
use embed_anything::embeddings::local::modernbert::ModernBertEmbedder;
use embed_anything::{
    embed_query,
    embeddings::embed::{Embedder, TextEmbedder},
};
use hf_hub::api::tokio::ApiBuilder;
use serde::Deserialize;
use std::sync::Arc;
use tokenizers::Tokenizer;
use tokio::sync::Semaphore;
use tracing::{debug, info};

/// Supported model architectures
#[derive(Debug, Clone)]
enum ModelArchitecture {
    ModernBert,
    Jina,
    Unknown(String),
}

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
    model_type: Option<String>,
}

/// Detect model architecture from HuggingFace config
fn detect_architecture(config: &HuggingFaceConfig) -> ModelArchitecture {
    match config.model_type.as_deref() {
        Some("modernbert") => ModelArchitecture::ModernBert,
        Some("jina_bert") | Some("jina") => ModelArchitecture::Jina,
        Some(other) => {
            // Try to detect based on model patterns
            if other.contains("bert") && !other.contains("jina") {
                ModelArchitecture::ModernBert
            } else if other.contains("jina") {
                ModelArchitecture::Jina
            } else {
                ModelArchitecture::Unknown(other.to_string())
            }
        }
        None => ModelArchitecture::Unknown("unspecified".to_string()),
    }
}

/// Get model metadata from HuggingFace
async fn get_model_metadata(model_id: &str) -> Result<(usize, usize, ModelArchitecture)> {
    // Fetch metadata dynamically from HuggingFace for all models
    let api = ApiBuilder::new()
        .with_progress(false)
        .build()
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

    let hidden_size = config.hidden_size;
    let max_context = config.max_position_embeddings;
    let architecture = detect_architecture(&config);

    info!(
        "Model {model_id} metadata: hidden_size={}, max_position_embeddings={}, architecture={:?}",
        hidden_size, max_context, architecture
    );
    Ok((hidden_size, max_context, architecture))
}

/// Flexible embeddings implementation using embed_anything
pub struct EmbedAnythingProvider {
    embedder: Arc<Embedder>,
    tokenizer: Arc<Tokenizer>,
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

        // Use model ID directly
        let model_id = &config.model;
        debug!("Using model ID: {model_id}");

        // Get model metadata from HuggingFace config
        let (dimensions, max_context, architecture) = get_model_metadata(model_id).await?;

        // Create appropriate embedder based on detected architecture
        let (embedder, tokenizer) = match architecture {
            ModelArchitecture::ModernBert => {
                let model_id_owned = model_id.to_string();
                let modernbert_embedder = tokio::task::spawn_blocking(move || {
                    ModernBertEmbedder::new(
                        model_id_owned,
                        None, // No specific revision
                        None, // No auth token
                        None, // No specific dtype
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
                .map_err(|e| {
                    EmbeddingError::ModelLoadError(format!(
                        "Failed to load ModernBert model: {e:?}"
                    ))
                })?;

                // Clone tokenizer before moving embedder
                let tokenizer = Arc::new(modernbert_embedder.tokenizer.clone());
                let embedder = Arc::new(Embedder::Text(TextEmbedder::ModernBert(Box::new(
                    modernbert_embedder,
                ))));
                (embedder, tokenizer)
            }
            ModelArchitecture::Jina => {
                let model_id_owned = model_id.to_string();
                let jina_embedder = tokio::task::spawn_blocking(move || {
                    JinaEmbedder::new(&model_id_owned, None, None)
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
                .map_err(|e| {
                    EmbeddingError::ModelLoadError(format!("Failed to load Jina model: {e:?}"))
                })?;

                // Clone tokenizer before moving embedder
                let tokenizer = Arc::new(jina_embedder.tokenizer.clone());
                let embedder = Arc::new(Embedder::Text(TextEmbedder::Jina(
                    Box::new(jina_embedder) as Box<dyn JinaEmbed + Send + Sync>,
                )));
                (embedder, tokenizer)
            }
            ModelArchitecture::Unknown(model_type) => {
                return Err(EmbeddingError::ModelLoadError(format!(
                    "Unsupported model architecture: {model_type}. Only ModernBert and Jina models are currently supported."
                )))?;
            }
        };

        info!(
            "Embeddings initialized with {:?} model '{}', {} dimensions, max context: {}",
            architecture, model_id, dimensions, max_context
        );

        Ok(Self {
            embedder,
            tokenizer,
            dimensions,
            max_context,
            batch_size: config.batch_size,
            concurrency_limiter: Arc::new(Semaphore::new(config.max_workers)),
        })
    }
}

#[async_trait]
impl EmbeddingProvider for EmbedAnythingProvider {
    async fn embed(&self, texts: Vec<String>) -> Result<Vec<Option<Vec<f32>>>> {
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
            // Check each text's length against context limit
            let mut texts_to_embed = Vec::new();
            let mut indices_to_embed = Vec::new();
            let mut chunk_results = vec![None; chunk.len()];

            for (i, text) in chunk.iter().enumerate() {
                // Quick pre-filter for obviously too long texts
                // Assuming worst case of ~5 characters per token
                if text.chars().count() > self.max_context * 5 {
                    debug!(
                        "Text too long (chars: {}, max tokens: {}), skipping",
                        text.chars().count(),
                        self.max_context
                    );
                    continue; // Text is way too long, skip it
                }

                // Accurate token counting for borderline cases
                match self.tokenizer.encode(text.as_str(), false) {
                    Ok(encoding) => {
                        let token_count = encoding.get_ids().len();
                        if token_count <= self.max_context {
                            texts_to_embed.push(text.clone());
                            indices_to_embed.push(i);
                        } else {
                            debug!(
                                "Text exceeds token limit ({} > {}), skipping",
                                token_count, self.max_context
                            );
                        }
                    }
                    Err(e) => {
                        debug!("Failed to tokenize text: {e}, skipping");
                        continue; // Tokenization failed, skip this text
                    }
                }
            }

            // Only process texts that fit within context window
            if !texts_to_embed.is_empty() {
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
                let embeddings = tokio::spawn(async move {
                    // Keep the permit alive for the duration of the operation
                    let _permit = permit;

                    // Use embed_query for embedding generation
                    let embeddings = embed_query(
                        &texts_to_embed
                            .iter()
                            .map(String::as_str)
                            .collect::<Vec<_>>(),
                        &embedder,
                        None,
                    )
                    .await
                    .map_err(|e| {
                        EmbeddingError::InferenceError(format!(
                            "Embedding generation failed: {e:?}"
                        ))
                    })?;

                    // Extract and convert embeddings
                    let mut results = Vec::with_capacity(embeddings.len());
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

                        results.push(dense_vec);
                    }

                    Ok::<Vec<Vec<f32>>, EmbeddingError>(results)
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

                // Place embeddings at their original indices
                for (embed_idx, orig_idx) in indices_to_embed.iter().enumerate() {
                    chunk_results[*orig_idx] = Some(embeddings[embed_idx].clone());
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
