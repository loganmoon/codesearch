//! Configuration for embedding generation

use serde::{Deserialize, Serialize};

/// Embedding provider type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum EmbeddingProviderType {
    /// Jina API (default, no self-hosting required)
    #[default]
    Jina,
    /// OpenAI-compatible API (vLLM or remote)
    LocalApi,
    /// Mock provider for testing
    Mock,
}

/// Default instruction for BGE models
const DEFAULT_BGE_INSTRUCTION: &str = "Represent this sentence for searching relevant passages:";

/// Configuration for embedding generation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingConfig {
    /// Provider type
    pub(crate) provider: EmbeddingProviderType,

    /// Model name or path
    pub(crate) model: String,

    /// Number of text chunks sent in a single embedding API request
    pub(crate) texts_per_api_request: usize,

    /// API base URL for LocalApi provider
    pub(crate) api_base_url: Option<String>,

    /// API key for authentication
    pub(crate) api_key: Option<String>,

    /// Embedding dimension size
    pub(crate) embedding_dimension: usize,

    /// Maximum concurrent embedding API requests
    pub(crate) max_concurrent_api_requests: usize,

    /// Number of retry attempts for failed embedding requests
    pub(crate) retry_attempts: usize,

    /// Instruction prefix for query embeddings (BGE models only)
    pub(crate) query_instruction: Option<String>,

    /// Task type prefix for Jina embeddings (e.g., "nl2code" -> "nl2code.query", "nl2code.passage")
    pub(crate) task_prefix: String,
}

impl EmbeddingConfig {
    /// Validate the configuration
    pub fn validate(&self) -> Result<(), String> {
        if self.texts_per_api_request == 0 {
            return Err("texts_per_api_request must be greater than 0".to_string());
        }
        if self.texts_per_api_request > 2000 {
            return Err("texts_per_api_request too large (max 2000)".to_string());
        }
        if self.max_concurrent_api_requests == 0 {
            return Err("max_concurrent_api_requests must be greater than 0".to_string());
        }
        if self.max_concurrent_api_requests > 256 {
            return Err("max_concurrent_api_requests too large (max 256)".to_string());
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
            model: "jina-code-embeddings-1.5b".to_string(),
            texts_per_api_request: 64,
            api_base_url: Some("http://localhost:8000/v1".to_string()),
            api_key: None,
            embedding_dimension: 1536,
            max_concurrent_api_requests: 4,
            retry_attempts: 5,
            query_instruction: Some(DEFAULT_BGE_INSTRUCTION.to_string()),
            task_prefix: "retrieval".to_string(),
        }
    }
}

/// Builder for EmbeddingConfig
pub struct EmbeddingConfigBuilder {
    provider: Option<EmbeddingProviderType>,
    model: Option<String>,
    texts_per_api_request: Option<usize>,
    api_base_url: Option<Option<String>>,
    api_key: Option<Option<String>>,
    embedding_dimension: Option<usize>,
    max_concurrent_api_requests: Option<usize>,
    retry_attempts: Option<usize>,
    query_instruction: Option<Option<String>>,
    task_prefix: Option<String>,
}

impl EmbeddingConfigBuilder {
    /// Create a new builder with no defaults set
    pub fn new() -> Self {
        Self {
            provider: None,
            model: None,
            texts_per_api_request: None,
            api_base_url: None,
            api_key: None,
            embedding_dimension: None,
            max_concurrent_api_requests: None,
            retry_attempts: None,
            query_instruction: None,
            task_prefix: None,
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

    /// Set the number of texts per API request
    pub fn texts_per_api_request(mut self, texts_per_api_request: usize) -> Self {
        self.texts_per_api_request = Some(texts_per_api_request);
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

    /// Set the maximum number of concurrent API requests
    pub fn max_concurrent_api_requests(mut self, max_concurrent_api_requests: usize) -> Self {
        self.max_concurrent_api_requests = Some(max_concurrent_api_requests);
        self
    }

    /// Set the number of retry attempts
    pub fn retry_attempts(mut self, retry_attempts: usize) -> Self {
        self.retry_attempts = Some(retry_attempts);
        self
    }

    /// Set the query instruction prefix (for BGE models)
    pub fn query_instruction(mut self, instruction: impl Into<String>) -> Self {
        self.query_instruction = Some(Some(instruction.into()));
        self
    }

    /// Set the task prefix for Jina embeddings (e.g., "nl2code")
    pub fn task_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.task_prefix = Some(prefix.into());
        self
    }

    /// Build the configuration, using defaults for unset fields
    pub fn build(self) -> EmbeddingConfig {
        let defaults = EmbeddingConfig::default();

        EmbeddingConfig {
            provider: self.provider.unwrap_or(defaults.provider),
            model: self.model.unwrap_or(defaults.model),
            texts_per_api_request: self
                .texts_per_api_request
                .unwrap_or(defaults.texts_per_api_request),
            api_base_url: self.api_base_url.unwrap_or(defaults.api_base_url),
            api_key: self.api_key.unwrap_or(defaults.api_key),
            embedding_dimension: self
                .embedding_dimension
                .unwrap_or(defaults.embedding_dimension),
            max_concurrent_api_requests: self
                .max_concurrent_api_requests
                .unwrap_or(defaults.max_concurrent_api_requests),
            retry_attempts: self.retry_attempts.unwrap_or(defaults.retry_attempts),
            query_instruction: self.query_instruction.unwrap_or(defaults.query_instruction),
            task_prefix: self.task_prefix.unwrap_or(defaults.task_prefix),
        }
    }
}

impl Default for EmbeddingConfigBuilder {
    fn default() -> Self {
        Self::new()
    }
}
