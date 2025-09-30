//! Configuration for embedding generation

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Embedding provider type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum EmbeddingProviderType {
    /// OpenAI-compatible API (vLLM or remote)
    #[default]
    LocalApi,
    /// Mock provider for testing
    Mock,
}

/// Device type for computation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum DeviceType {
    /// CPU computation
    #[default]
    Cpu,
    /// CUDA GPU computation
    Cuda,
    /// Metal GPU computation (Apple Silicon)
    Metal,
}

/// Backend type for embeddings
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum BackendType {
    /// Candle backend - more flexible, supports any HuggingFace model
    #[default]
    Candle,
    /// ONNX backend - potentially faster for supported models. Currently not implemented.
    Onnx,
}

/// Configuration for embedding generation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingConfig {
    /// Provider type
    pub(crate) provider: EmbeddingProviderType,

    /// Model name or path
    pub(crate) model: String,

    /// Batch size for processing
    pub(crate) batch_size: usize,

    /// API base URL for LocalApi provider
    pub(crate) api_base_url: Option<String>,

    /// API key for authentication
    pub(crate) api_key: Option<String>,

    /// Embedding dimension size
    pub(crate) embedding_dimension: usize,

    /// Device to use for computation
    pub(crate) device: DeviceType,

    /// Backend to use for inference
    #[serde(default)]
    pub(crate) backend: BackendType,

    /// Maximum number of concurrent workers
    pub(crate) max_workers: usize,

    /// Model cache directory
    pub(crate) model_cache_dir: PathBuf,
}

impl EmbeddingConfig {
    /// Get the model cache directory as a Path
    pub fn model_cache_path(&self) -> &Path {
        &self.model_cache_dir
    }

    /// Validate the configuration
    pub fn validate(&self) -> Result<(), String> {
        if self.batch_size == 0 {
            return Err("Batch size must be greater than 0".to_string());
        }
        if self.batch_size > 1000 {
            return Err("Batch size too large (max 1000)".to_string());
        }
        if self.max_workers == 0 {
            return Err("Max workers must be greater than 0".to_string());
        }
        if self.max_workers > 32 {
            return Err("Max workers too large (max 32)".to_string());
        }
        if self.model.is_empty() {
            return Err("Model name cannot be empty".to_string());
        }
        if self.embedding_dimension == 0 {
            return Err("embedding_dimension must be greater than 0".to_string());
        }
        Ok(())
    }
}

impl Default for EmbeddingConfig {
    fn default() -> Self {
        Self {
            provider: EmbeddingProviderType::default(),
            model: "BAAI/bge-code-v1".to_string(),
            batch_size: 32,
            api_base_url: Some("http://localhost:8000".to_string()),
            api_key: None,
            embedding_dimension: 768,
            device: DeviceType::default(),
            backend: BackendType::default(),
            max_workers: 4,
            model_cache_dir: PathBuf::from("./models"),
        }
    }
}

/// Builder for EmbeddingConfig
pub struct EmbeddingConfigBuilder {
    provider: Option<EmbeddingProviderType>,
    model: Option<String>,
    batch_size: Option<usize>,
    api_base_url: Option<Option<String>>,
    api_key: Option<Option<String>>,
    embedding_dimension: Option<usize>,
    device: Option<DeviceType>,
    backend: Option<BackendType>,
    max_workers: Option<usize>,
    model_cache_dir: Option<PathBuf>,
}

impl EmbeddingConfigBuilder {
    /// Create a new builder with no defaults set
    pub fn new() -> Self {
        Self {
            provider: None,
            model: None,
            batch_size: None,
            api_base_url: None,
            api_key: None,
            embedding_dimension: None,
            device: None,
            backend: None,
            max_workers: None,
            model_cache_dir: None,
        }
    }

    /// Set the provider type
    pub fn provider(mut self, provider: EmbeddingProviderType) -> Self {
        self.provider = Some(provider);
        self
    }

    /// Set the model name or path
    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    /// Set the batch size
    pub fn batch_size(mut self, batch_size: usize) -> Self {
        self.batch_size = Some(batch_size);
        self
    }

    /// Set the API base URL
    pub fn api_base_url(mut self, url: impl Into<String>) -> Self {
        self.api_base_url = Some(Some(url.into()));
        self
    }

    /// Set the API key
    pub fn api_key(mut self, key: impl Into<String>) -> Self {
        self.api_key = Some(Some(key.into()));
        self
    }

    /// Set the embedding dimension
    pub fn embedding_dimension(mut self, dimension: usize) -> Self {
        self.embedding_dimension = Some(dimension);
        self
    }

    /// Set the device type
    pub fn device(mut self, device: DeviceType) -> Self {
        self.device = Some(device);
        self
    }

    /// Set the backend type
    pub fn backend(mut self, backend: BackendType) -> Self {
        self.backend = Some(backend);
        self
    }

    /// Set the maximum number of workers
    pub fn max_workers(mut self, max_workers: usize) -> Self {
        self.max_workers = Some(max_workers);
        self
    }

    /// Set the model cache directory
    pub fn model_cache_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.model_cache_dir = Some(dir.into());
        self
    }

    /// Build the configuration, using defaults for unset fields
    pub fn build(self) -> EmbeddingConfig {
        let defaults = EmbeddingConfig::default();

        EmbeddingConfig {
            provider: self.provider.unwrap_or(defaults.provider),
            model: self.model.unwrap_or(defaults.model),
            batch_size: self.batch_size.unwrap_or(defaults.batch_size),
            api_base_url: self.api_base_url.unwrap_or(defaults.api_base_url),
            api_key: self.api_key.unwrap_or(defaults.api_key),
            embedding_dimension: self
                .embedding_dimension
                .unwrap_or(defaults.embedding_dimension),
            device: self.device.unwrap_or(defaults.device),
            backend: self.backend.unwrap_or(defaults.backend),
            max_workers: self.max_workers.unwrap_or(defaults.max_workers),
            model_cache_dir: self.model_cache_dir.unwrap_or(defaults.model_cache_dir),
        }
    }
}

impl Default for EmbeddingConfigBuilder {
    fn default() -> Self {
        Self::new()
    }
}
