//! Granite sparse embedding provider using SPLADE architecture
//!
//! This module provides learned sparse embeddings using IBM's Granite 30M Sparse model.
//! The model uses SPLADE (Sparse Lexical and Expansion via Deep Embeddings) architecture
//! to generate interpretable sparse vectors.

mod config;
mod model;
mod tokenizer;

pub use config::GraniteSparseConfig;

use crate::sparse_provider::SparseEmbeddingProvider;
use async_trait::async_trait;
use candle_core::Device;
use codesearch_core::error::{Error, Result};
use futures::future::join_all;
use model::GraniteSparseModel;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokenizer::GraniteTokenizer;

/// Default HuggingFace model ID
const DEFAULT_MODEL_ID: &str = "ibm-granite/granite-embedding-30m-sparse";

/// Device selection for Granite sparse model
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SparseDevice {
    /// CPU inference
    #[default]
    Cpu,
    /// CUDA GPU (with device index)
    Cuda(usize),
    /// Apple Metal GPU
    Metal,
}

impl SparseDevice {
    /// Detect the best available device
    pub fn detect_best_available() -> Self {
        #[cfg(feature = "cuda")]
        {
            if candle_core::utils::cuda_is_available() {
                return Self::Cuda(0);
            }
        }

        #[cfg(feature = "metal")]
        {
            if candle_core::utils::metal_is_available() {
                return Self::Metal;
            }
        }

        Self::Cpu
    }

    /// Parse device from configuration string
    pub fn from_config(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "auto" => Self::detect_best_available(),
            "cpu" => Self::Cpu,
            "metal" => Self::Metal,
            "cuda" => Self::Cuda(0),
            s if s.starts_with("cuda:") => {
                let idx = s[5..].parse().unwrap_or(0);
                Self::Cuda(idx)
            }
            _ => Self::Cpu,
        }
    }

    /// Convert to Candle device
    pub fn to_candle_device(&self) -> Result<Device> {
        match self {
            Self::Cpu => Ok(Device::Cpu),
            #[cfg(feature = "cuda")]
            Self::Cuda(idx) => Device::new_cuda(*idx)
                .map_err(|e| Error::embedding(format!("Failed to create CUDA device: {e}"))),
            #[cfg(not(feature = "cuda"))]
            Self::Cuda(_) => Err(Error::embedding(
                "CUDA support not compiled. Rebuild with --features cuda".to_string(),
            )),
            #[cfg(feature = "metal")]
            Self::Metal => Device::new_metal(0)
                .map_err(|e| Error::embedding(format!("Failed to create Metal device: {e}"))),
            #[cfg(not(feature = "metal"))]
            Self::Metal => Err(Error::embedding(
                "Metal support not compiled. Rebuild with --features metal".to_string(),
            )),
        }
    }
}

/// Default batch size for Granite model inference
const DEFAULT_BATCH_SIZE: usize = 32;

/// Granite sparse embedding provider
pub struct GraniteSparseProvider {
    model: Arc<GraniteSparseModel>,
    tokenizer: GraniteTokenizer,
    device: Device,
    top_k: usize,
    max_batch_size: usize,
    /// Whether running on GPU (requires sequential chunk processing)
    is_gpu: bool,
}

impl GraniteSparseProvider {
    /// Create a new Granite sparse provider
    ///
    /// # Arguments
    /// * `device` - Device to run inference on
    /// * `cache_dir` - Directory to cache downloaded models
    /// * `top_k` - Maximum number of sparse dimensions to keep
    /// * `batch_size` - Maximum batch size for inference (default: 32)
    pub async fn new(
        device: SparseDevice,
        cache_dir: PathBuf,
        top_k: usize,
        batch_size: usize,
    ) -> Result<Self> {
        let candle_device = device.to_candle_device()?;

        // Download or use cached model
        tracing::debug!("Downloading/caching Granite sparse model...");
        let model_dir = download_model(DEFAULT_MODEL_ID, &cache_dir).await?;
        tracing::debug!(model_dir = %model_dir.display(), "Model files downloaded");

        // Load configuration
        let config_path = model_dir.join("config.json");
        tracing::debug!(config_path = %config_path.display(), "Loading model config");
        let config = GraniteSparseConfig::from_file(&config_path)?;
        tracing::debug!(
            hidden_size = config.hidden_size,
            num_layers = config.num_hidden_layers,
            vocab_size = config.vocab_size,
            "Config loaded"
        );

        // Load model
        let model_path = model_dir.join("model.safetensors");
        tracing::debug!(model_path = %model_path.display(), "Loading model weights");
        let model = GraniteSparseModel::load(config, &model_path, &candle_device).map_err(|e| {
            tracing::error!(error = %e, "Failed to load Granite model weights");
            Error::embedding(format!("Failed to load model: {e}"))
        })?;
        tracing::debug!("Model weights loaded successfully");

        // Load tokenizer
        let tokenizer_path = model_dir.join("tokenizer.json");
        tracing::debug!(tokenizer_path = %tokenizer_path.display(), "Loading tokenizer");
        let tokenizer = GraniteTokenizer::from_file(&tokenizer_path)?;
        tracing::debug!("Tokenizer loaded successfully");

        let max_batch_size = if batch_size == 0 {
            DEFAULT_BATCH_SIZE
        } else {
            batch_size
        };

        let is_gpu = matches!(device, SparseDevice::Cuda(_) | SparseDevice::Metal);

        tracing::info!(
            device = ?device,
            top_k = top_k,
            max_batch_size = max_batch_size,
            is_gpu = is_gpu,
            model_dir = %model_dir.display(),
            "Loaded Granite sparse model"
        );

        Ok(Self {
            model: Arc::new(model),
            tokenizer,
            device: candle_device,
            top_k,
            max_batch_size,
            is_gpu,
        })
    }

    /// Get the model version string
    pub fn model_version() -> &'static str {
        "granite-embedding-30m-sparse-v1"
    }
}

#[async_trait]
impl SparseEmbeddingProvider for GraniteSparseProvider {
    async fn embed_sparse(&self, texts: Vec<&str>) -> Result<Vec<Option<Vec<(u32, f32)>>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        let max_length = self.model.config().max_position_embeddings;

        // Tokenize all texts (fast CPU-bound work, keep on async thread)
        let tokenized = self.tokenizer.encode_batch(&texts, max_length)?;

        // Handle empty texts - collect non-empty with their original indices
        let mut non_empty_indices = Vec::new();
        let mut non_empty_tokenized = Vec::new();

        for (i, (text, tok)) in texts.iter().zip(tokenized.iter()).enumerate() {
            if text.is_empty() || tok.input_ids.is_empty() {
                continue;
            }
            non_empty_indices.push(i);
            non_empty_tokenized.push(tok.clone());
        }

        // If all texts are empty, return all None
        if non_empty_tokenized.is_empty() {
            return Ok(vec![None; texts.len()]);
        }

        let seq_len = non_empty_tokenized[0].input_ids.len();
        let total_items = non_empty_tokenized.len();
        let num_chunks = total_items.div_ceil(self.max_batch_size);

        if num_chunks > 1 {
            tracing::debug!(
                total_items = total_items,
                max_batch_size = self.max_batch_size,
                num_chunks = num_chunks,
                is_gpu = self.is_gpu,
                "Processing sparse embeddings in chunks"
            );
        }

        // Process chunks - sequential on GPU (to avoid VRAM exhaustion), parallel on CPU
        let all_sparse_vectors = if self.is_gpu {
            // GPU: process chunks sequentially to avoid OOM
            let mut results = Vec::with_capacity(total_items);
            for (chunk_idx, chunk) in non_empty_tokenized.chunks(self.max_batch_size).enumerate() {
                let chunk_size = chunk.len();
                let input_ids_vec: Vec<u32> = chunk
                    .iter()
                    .flat_map(|t| t.input_ids.iter().copied())
                    .collect();
                let attention_mask_vec: Vec<u32> = chunk
                    .iter()
                    .flat_map(|t| t.attention_mask.iter().copied())
                    .collect();

                let model = Arc::clone(&self.model);
                let device = self.device.clone();
                let top_k = self.top_k;

                let chunk_vectors = tokio::task::spawn_blocking(move || {
                    use candle_core::Tensor;

                    let input_ids = Tensor::from_vec(input_ids_vec, (chunk_size, seq_len), &device)
                        .map_err(|e| {
                            Error::embedding(format!("Failed to create input tensor: {e}"))
                        })?;
                    let attention_mask =
                        Tensor::from_vec(attention_mask_vec, (chunk_size, seq_len), &device)
                            .map_err(|e| {
                                Error::embedding(format!(
                                    "Failed to create attention mask tensor: {e}"
                                ))
                            })?;

                    model
                        .embed_sparse(&input_ids, &attention_mask, top_k)
                        .map_err(|e| Error::embedding(format!("Model inference failed: {e}")))
                })
                .await
                .map_err(|e| Error::embedding(format!("Inference task panicked: {e}")))??;

                if num_chunks > 1 {
                    tracing::debug!(
                        chunk = chunk_idx + 1,
                        total_chunks = num_chunks,
                        items_in_chunk = chunk_vectors.len(),
                        "Processed chunk"
                    );
                }

                results.extend(chunk_vectors);
            }
            results
        } else {
            // CPU: process chunks in parallel for throughput
            let chunk_futures: Vec<_> = non_empty_tokenized
                .chunks(self.max_batch_size)
                .map(|chunk| {
                    let chunk_size = chunk.len();
                    let input_ids_vec: Vec<u32> = chunk
                        .iter()
                        .flat_map(|t| t.input_ids.iter().copied())
                        .collect();
                    let attention_mask_vec: Vec<u32> = chunk
                        .iter()
                        .flat_map(|t| t.attention_mask.iter().copied())
                        .collect();

                    let model = Arc::clone(&self.model);
                    let device = self.device.clone();
                    let top_k = self.top_k;

                    tokio::task::spawn_blocking(move || {
                        use candle_core::Tensor;

                        let input_ids =
                            Tensor::from_vec(input_ids_vec, (chunk_size, seq_len), &device)
                                .map_err(|e| {
                                    Error::embedding(format!("Failed to create input tensor: {e}"))
                                })?;
                        let attention_mask =
                            Tensor::from_vec(attention_mask_vec, (chunk_size, seq_len), &device)
                                .map_err(|e| {
                                    Error::embedding(format!(
                                        "Failed to create attention mask tensor: {e}"
                                    ))
                                })?;

                        model
                            .embed_sparse(&input_ids, &attention_mask, top_k)
                            .map_err(|e| Error::embedding(format!("Model inference failed: {e}")))
                    })
                })
                .collect();

            let chunk_results = join_all(chunk_futures).await;

            let mut results = Vec::with_capacity(total_items);
            for (chunk_idx, result) in chunk_results.into_iter().enumerate() {
                let chunk_vectors = result
                    .map_err(|e| Error::embedding(format!("Inference task panicked: {e}")))??;

                if num_chunks > 1 {
                    tracing::debug!(
                        chunk = chunk_idx + 1,
                        total_chunks = num_chunks,
                        items_in_chunk = chunk_vectors.len(),
                        "Processed chunk"
                    );
                }

                results.extend(chunk_vectors);
            }
            results
        };

        // Build result vector with proper ordering
        let mut results = vec![None; texts.len()];
        for (sparse_idx, original_idx) in non_empty_indices.into_iter().enumerate() {
            let sparse = all_sparse_vectors
                .get(sparse_idx)
                .cloned()
                .unwrap_or_default();
            if !sparse.is_empty() {
                results[original_idx] = Some(sparse);
            }
        }

        Ok(results)
    }
}

/// Download model from HuggingFace Hub
async fn download_model(model_id: &str, cache_dir: &Path) -> Result<PathBuf> {
    let model_id = model_id.to_string();
    let cache_dir = cache_dir.to_path_buf();

    tokio::task::spawn_blocking(move || {
        use hf_hub::{api::sync::ApiBuilder, Repo, RepoType};

        // Create cache directory if it doesn't exist
        std::fs::create_dir_all(&cache_dir).map_err(|e| {
            Error::embedding(format!(
                "Failed to create cache directory {}: {e}",
                cache_dir.display()
            ))
        })?;

        // Create HuggingFace API client with custom cache directory
        let api = ApiBuilder::new()
            .with_cache_dir(cache_dir.clone())
            .build()
            .map_err(|e| Error::embedding(format!("Failed to create HuggingFace API: {e}")))?;

        let repo = api.repo(Repo::new(model_id.to_string(), RepoType::Model));

        // Download required files
        let files = ["config.json", "tokenizer.json", "model.safetensors"];
        let mut model_dir: Option<PathBuf> = None;

        for file in files {
            let path = repo.get(file).map_err(|e| {
                Error::embedding(format!("Failed to download {file} from {model_id}: {e}"))
            })?;

            if model_dir.is_none() {
                model_dir = path.parent().map(|p| p.to_path_buf());
            }
        }

        model_dir.ok_or_else(|| Error::embedding("Failed to determine model directory".to_string()))
    })
    .await
    .map_err(|e| Error::embedding(format!("Model download task panicked: {e}")))?
}

/// Get the default model cache directory
pub fn default_model_cache_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".codesearch")
        .join("models")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sparse_device_from_config() {
        assert_eq!(SparseDevice::from_config("cpu"), SparseDevice::Cpu);
        assert_eq!(SparseDevice::from_config("CPU"), SparseDevice::Cpu);
        assert_eq!(SparseDevice::from_config("cuda"), SparseDevice::Cuda(0));
        assert_eq!(SparseDevice::from_config("cuda:1"), SparseDevice::Cuda(1));
        assert_eq!(SparseDevice::from_config("metal"), SparseDevice::Metal);
        assert_eq!(SparseDevice::from_config("unknown"), SparseDevice::Cpu);
    }

    #[test]
    fn test_default_cache_dir() {
        let cache_dir = default_model_cache_dir();
        assert!(cache_dir.ends_with("models"));
    }
}
