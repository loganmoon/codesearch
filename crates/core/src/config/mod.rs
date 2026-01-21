//! Configuration module for the codesearch system
//!
//! This module provides configuration structures and loading mechanisms for the
//! codesearch system. Configuration can be loaded from TOML files and/or environment
//! variables.

mod defaults;
mod loading;
mod storage;

#[cfg(test)]
mod tests;

use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// Re-export default_max_entities_per_db_operation for external use
pub use defaults::default_max_entities_per_db_operation;

use defaults::*;

/// Returns the path to the global configuration file
///
/// The global config is stored at `~/.codesearch/config.toml` and contains
/// user preferences that apply across all repositories.
pub fn global_config_path() -> Result<PathBuf> {
    let home_dir = dirs::home_dir()
        .ok_or_else(|| Error::config("Unable to determine home directory".to_string()))?;
    Ok(home_dir.join(".codesearch").join("config.toml"))
}

/// Indexer configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexerConfig {
    /// Number of files discovered together in Stage 1 of the indexing pipeline
    #[serde(default = "default_files_per_discovery_batch")]
    pub files_per_discovery_batch: usize,

    /// Channel buffer capacity for inter-stage communication in the pipeline
    #[serde(default = "default_pipeline_channel_capacity")]
    pub pipeline_channel_capacity: usize,

    /// Maximum entities sent to the embedding API in a single batch
    #[serde(default = "default_entities_per_embedding_batch")]
    pub entities_per_embedding_batch: usize,

    /// Maximum concurrent file parsing operations in Stage 2
    #[serde(default = "default_max_concurrent_file_extractions")]
    pub max_concurrent_file_extractions: usize,

    /// Maximum concurrent database snapshot updates in Stage 5
    #[serde(default = "default_max_concurrent_snapshot_updates")]
    pub max_concurrent_snapshot_updates: usize,
}

/// Main configuration structure for the codesearch system
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Indexer configuration
    pub indexer: IndexerConfig,

    /// Embeddings configuration
    pub embeddings: EmbeddingsConfig,

    /// Sparse embeddings configuration
    #[serde(default)]
    pub sparse_embeddings: SparseEmbeddingsConfig,

    /// File watcher configuration
    pub watcher: WatcherConfig,

    /// Storage configuration
    pub storage: StorageConfig,

    /// Server configuration
    #[serde(default)]
    pub server: ServerConfig,

    /// Language configuration
    #[serde(default)]
    pub languages: LanguagesConfig,

    /// Reranking configuration
    #[serde(default)]
    pub reranking: RerankingConfig,

    /// Hybrid search configuration
    #[serde(default)]
    pub hybrid_search: HybridSearchConfig,

    /// Outbox processor configuration
    #[serde(default)]
    pub outbox: OutboxConfig,
}

/// Configuration for embeddings generation
///
/// # Providers
/// - `jina` (default): Jina API - no self-hosting required, uses JINA_API_KEY env var
/// - `localapi`: vLLM or OpenAI-compatible API for self-hosted models
/// - `mock`: Mock provider for testing
#[derive(Clone, Serialize, Deserialize)]
pub struct EmbeddingsConfig {
    /// Provider type: "jina" (default), "localapi", "mock"
    #[serde(default = "default_provider")]
    pub provider: String,

    /// Model name to use
    #[serde(default = "default_model")]
    pub model: String,

    /// Number of text chunks sent in a single embedding API request
    #[serde(default = "default_texts_per_api_request")]
    pub texts_per_api_request: usize,

    /// Device to use: "cuda" or "cpu" (localapi only)
    #[serde(default = "default_device")]
    pub device: String,

    /// API base URL for LocalApi provider
    #[serde(default = "default_api_base_url")]
    pub api_base_url: Option<String>,

    /// API key for authentication (or use JINA_API_KEY / EMBEDDING_API_KEY env vars)
    pub api_key: Option<String>,

    /// Embedding dimension size
    #[serde(default = "default_embedding_dimension")]
    pub embedding_dimension: usize,

    /// Maximum concurrent embedding API requests
    #[serde(default = "default_max_concurrent_api_requests")]
    pub max_concurrent_api_requests: usize,

    /// Default instruction for BGE embedding models (localapi only)
    #[serde(default = "default_bge_instruction")]
    pub default_bge_instruction: String,

    /// Number of retry attempts for failed embedding requests
    #[serde(default = "default_embedding_retry_attempts")]
    pub retry_attempts: usize,
}

impl std::fmt::Debug for EmbeddingsConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EmbeddingsConfig")
            .field("provider", &self.provider)
            .field("model", &self.model)
            .field("texts_per_api_request", &self.texts_per_api_request)
            .field("device", &self.device)
            .field("api_base_url", &self.api_base_url)
            .field("api_key", &self.api_key.as_ref().map(|_| "***REDACTED***"))
            .field("embedding_dimension", &self.embedding_dimension)
            .field(
                "max_concurrent_api_requests",
                &self.max_concurrent_api_requests,
            )
            .field("default_bge_instruction", &self.default_bge_instruction)
            .field("retry_attempts", &self.retry_attempts)
            .finish()
    }
}

/// Configuration for sparse embeddings generation
///
/// # Providers
/// - `granite` (default when feature enabled): IBM Granite 30M learned sparse embeddings
/// - `bm25`: BM25 statistical sparse embeddings (fallback, no model required)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SparseEmbeddingsConfig {
    /// Provider type: "granite" (default), "bm25"
    #[serde(default = "default_sparse_provider")]
    pub provider: String,

    /// Device for Granite model: "auto" (default), "cpu", "cuda", "cuda:0", "metal"
    #[serde(default = "default_sparse_device")]
    pub device: String,

    /// Model cache directory (default: ~/.codesearch/models/)
    #[serde(default)]
    pub model_cache_dir: Option<String>,

    /// Top-k sparse dimensions to keep (default: 256)
    #[serde(default = "default_sparse_top_k")]
    pub top_k: usize,

    /// Batch size for Granite model inference (default: 32)
    /// Larger batches improve GPU throughput; smaller batches reduce memory usage
    #[serde(default = "default_sparse_batch_size")]
    pub batch_size: usize,
}

impl Default for SparseEmbeddingsConfig {
    fn default() -> Self {
        Self {
            provider: default_sparse_provider(),
            device: default_sparse_device(),
            model_cache_dir: None,
            top_k: default_sparse_top_k(),
            batch_size: default_sparse_batch_size(),
        }
    }
}

/// Update strategy for keeping the index synchronized
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum UpdateStrategy {
    /// Keep the main branch indexed - poll git for changes (default)
    #[default]
    MainOnly,
    /// Watch for file changes in real-time (expensive, continuous CPU/IO)
    Live,
    /// No automatic updating - only explicit `codesearch index`
    Disabled,
}

/// Configuration for file watching
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatcherConfig {
    /// Update strategy for keeping the index synchronized
    #[serde(default)]
    pub update_strategy: UpdateStrategy,

    /// Debounce time in milliseconds
    #[serde(default = "default_debounce_ms")]
    pub debounce_ms: u64,

    /// Patterns to ignore
    #[serde(default = "default_ignore_patterns")]
    pub ignore_patterns: Vec<String>,

    /// Interval in seconds for polling git in MainOnly strategy
    #[serde(default = "default_main_branch_poll_interval_secs")]
    pub main_branch_poll_interval_secs: u64,
}

/// Configuration for storage backend
#[derive(Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    /// Qdrant host address
    #[serde(default = "default_qdrant_host")]
    pub qdrant_host: String,

    /// Qdrant gRPC port
    #[serde(default = "default_qdrant_port")]
    pub qdrant_port: u16,

    /// Qdrant REST API port
    #[serde(default = "default_qdrant_rest_port")]
    pub qdrant_rest_port: u16,

    /// Automatically start containerized dependencies
    #[serde(default = "default_auto_start_deps")]
    pub auto_start_deps: bool,

    /// Docker compose file path (optional)
    #[serde(default)]
    pub docker_compose_file: Option<String>,

    /// Postgres host address
    #[serde(default = "default_postgres_host")]
    pub postgres_host: String,

    /// Postgres port
    #[serde(default = "default_postgres_port")]
    pub postgres_port: u16,

    /// Postgres database name
    #[serde(default = "default_postgres_database")]
    pub postgres_database: String,

    /// Postgres username
    #[serde(default = "default_postgres_user")]
    pub postgres_user: String,

    /// Postgres password
    #[serde(default = "default_postgres_password")]
    pub postgres_password: String,

    /// Postgres connection pool size (max connections)
    #[serde(default = "default_postgres_pool_size")]
    pub postgres_pool_size: u32,

    /// Maximum entities allowed in a single Postgres batch operation (safety limit)
    #[serde(default = "default_max_entities_per_db_operation")]
    pub max_entities_per_db_operation: usize,

    /// Neo4j host address
    #[serde(default = "default_neo4j_host")]
    pub neo4j_host: String,

    /// Neo4j HTTP port (web interface)
    #[serde(default = "default_neo4j_http_port")]
    pub neo4j_http_port: u16,

    /// Neo4j Bolt port (driver connection)
    #[serde(default = "default_neo4j_bolt_port")]
    pub neo4j_bolt_port: u16,

    /// Neo4j username
    #[serde(default = "default_neo4j_user")]
    pub neo4j_user: String,

    /// Neo4j password
    #[serde(default = "default_neo4j_password")]
    pub neo4j_password: String,
}

impl std::fmt::Debug for StorageConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StorageConfig")
            .field("qdrant_host", &self.qdrant_host)
            .field("qdrant_port", &self.qdrant_port)
            .field("qdrant_rest_port", &self.qdrant_rest_port)
            .field("auto_start_deps", &self.auto_start_deps)
            .field("docker_compose_file", &self.docker_compose_file)
            .field("postgres_host", &self.postgres_host)
            .field("postgres_port", &self.postgres_port)
            .field("postgres_database", &self.postgres_database)
            .field("postgres_user", &self.postgres_user)
            .field("postgres_password", &"***REDACTED***")
            .field("postgres_pool_size", &self.postgres_pool_size)
            .field(
                "max_entities_per_db_operation",
                &self.max_entities_per_db_operation,
            )
            .field("neo4j_host", &self.neo4j_host)
            .field("neo4j_http_port", &self.neo4j_http_port)
            .field("neo4j_bolt_port", &self.neo4j_bolt_port)
            .field("neo4j_user", &self.neo4j_user)
            .field("neo4j_password", &"***REDACTED***")
            .finish()
    }
}

/// Configuration for REST API server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    /// Port to listen on (host is always 127.0.0.1 for localhost-only access)
    #[serde(default = "default_server_port")]
    pub port: u16,

    /// Allowed CORS origins (empty = disabled, ["*"] = all origins)
    #[serde(default = "default_allowed_origins")]
    pub allowed_origins: Vec<String>,
}

/// Configuration for language support
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanguagesConfig {
    /// List of enabled languages (rust, javascript, typescript, python)
    #[serde(default = "default_enabled_languages")]
    pub enabled: Vec<String>,
}

/// Configuration for reranking with cross-encoder models
#[derive(Clone, Serialize, Deserialize)]
pub struct RerankingConfig {
    /// Whether reranking is enabled (default: false)
    #[serde(default = "default_enable_reranking")]
    pub enabled: bool,

    /// Reranker provider type: "jina" or "vllm" (default: "jina")
    #[serde(default = "default_reranking_provider")]
    pub provider: String,

    /// Reranker model name
    #[serde(default = "default_reranking_model")]
    pub model: String,

    /// Number of candidates to retrieve from vector search before reranking
    #[serde(default = "default_reranking_candidates")]
    pub candidates: usize,

    /// Number of top results to return after reranking
    #[serde(default = "default_reranking_top_k")]
    pub top_k: usize,

    /// API base URL for reranker service (defaults to embeddings URL if not set)
    pub api_base_url: Option<String>,

    /// API key for reranker service (uses EMBEDDING_API_KEY env if not set)
    pub api_key: Option<String>,

    /// Request timeout in seconds for reranking API calls (default: 15)
    #[serde(default = "default_reranking_timeout_secs")]
    pub timeout_secs: u64,

    /// Maximum concurrent reranking API requests (default: 16)
    #[serde(default = "default_reranking_max_concurrent_requests")]
    pub max_concurrent_requests: usize,
}

impl std::fmt::Debug for RerankingConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RerankingConfig")
            .field("enabled", &self.enabled)
            .field("provider", &self.provider)
            .field("model", &self.model)
            .field("candidates", &self.candidates)
            .field("top_k", &self.top_k)
            .field("api_base_url", &self.api_base_url)
            .field("api_key", &self.api_key.as_ref().map(|_| "***REDACTED***"))
            .field("timeout_secs", &self.timeout_secs)
            .field("max_concurrent_requests", &self.max_concurrent_requests)
            .finish()
    }
}

/// Per-request reranking override configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RerankingRequestConfig {
    pub enabled: Option<bool>,
    pub candidates: Option<usize>,
    pub top_k: Option<usize>,
}

impl RerankingRequestConfig {
    /// Merge with base config, request overrides take precedence
    pub fn merge_with(&self, base: &RerankingConfig) -> RerankingConfig {
        RerankingConfig {
            enabled: self.enabled.unwrap_or(base.enabled),
            provider: base.provider.clone(),
            candidates: self.candidates.unwrap_or(base.candidates).min(1000),
            top_k: self.top_k.unwrap_or(base.top_k),
            model: base.model.clone(),
            api_base_url: base.api_base_url.clone(),
            api_key: base.api_key.clone(),
            timeout_secs: base.timeout_secs,
            max_concurrent_requests: base.max_concurrent_requests,
        }
    }
}

/// Hybrid search configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HybridSearchConfig {
    /// Prefetch multiplier: retrieve N * limit candidates per method (default: 5)
    #[serde(default = "default_prefetch_multiplier")]
    pub prefetch_multiplier: usize,
}

/// Configuration for outbox processor
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutboxConfig {
    /// Outbox polling interval in milliseconds
    #[serde(default = "default_outbox_poll_interval_ms")]
    pub poll_interval_ms: u64,

    /// Number of outbox entries to fetch per poll
    #[serde(default = "default_outbox_entries_per_poll")]
    pub entries_per_poll: i64,

    /// Maximum retry attempts for failed operations
    #[serde(default = "default_outbox_max_retries")]
    pub max_retries: i32,

    /// Maximum embedding dimension size (safety limit)
    #[serde(default = "default_outbox_max_embedding_dim")]
    pub max_embedding_dim: usize,

    /// Maximum number of Qdrant client connections to cache
    #[serde(default = "default_outbox_max_cached_collections")]
    pub max_cached_collections: usize,

    /// Drain timeout in seconds (how long to wait for outbox to drain after indexing)
    #[serde(default = "default_outbox_drain_timeout_secs")]
    pub drain_timeout_secs: u64,
}

// Default implementations

impl Default for EmbeddingsConfig {
    fn default() -> Self {
        Self {
            provider: default_provider(),
            model: default_model(),
            texts_per_api_request: default_texts_per_api_request(),
            device: default_device(),
            api_base_url: default_api_base_url(),
            api_key: None,
            embedding_dimension: default_embedding_dimension(),
            max_concurrent_api_requests: default_max_concurrent_api_requests(),
            default_bge_instruction: default_bge_instruction(),
            retry_attempts: default_embedding_retry_attempts(),
        }
    }
}

impl Default for WatcherConfig {
    fn default() -> Self {
        Self {
            update_strategy: UpdateStrategy::default(),
            debounce_ms: default_debounce_ms(),
            ignore_patterns: default_ignore_patterns(),
            main_branch_poll_interval_secs: default_main_branch_poll_interval_secs(),
        }
    }
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            port: default_server_port(),
            allowed_origins: default_allowed_origins(),
        }
    }
}

impl Default for LanguagesConfig {
    fn default() -> Self {
        Self {
            enabled: default_enabled_languages(),
        }
    }
}

impl Default for RerankingConfig {
    fn default() -> Self {
        Self {
            enabled: default_enable_reranking(),
            provider: default_reranking_provider(),
            model: default_reranking_model(),
            candidates: default_reranking_candidates(),
            top_k: default_reranking_top_k(),
            api_base_url: None,
            api_key: None,
            timeout_secs: default_reranking_timeout_secs(),
            max_concurrent_requests: default_reranking_max_concurrent_requests(),
        }
    }
}

impl Default for HybridSearchConfig {
    fn default() -> Self {
        Self {
            prefetch_multiplier: default_prefetch_multiplier(),
        }
    }
}

impl Default for OutboxConfig {
    fn default() -> Self {
        Self {
            poll_interval_ms: default_outbox_poll_interval_ms(),
            entries_per_poll: default_outbox_entries_per_poll(),
            max_retries: default_outbox_max_retries(),
            max_embedding_dim: default_outbox_max_embedding_dim(),
            max_cached_collections: default_outbox_max_cached_collections(),
            drain_timeout_secs: default_outbox_drain_timeout_secs(),
        }
    }
}

impl Default for IndexerConfig {
    fn default() -> Self {
        Self {
            files_per_discovery_batch: default_files_per_discovery_batch(),
            pipeline_channel_capacity: default_pipeline_channel_capacity(),
            entities_per_embedding_batch: default_entities_per_embedding_batch(),
            max_concurrent_file_extractions: default_max_concurrent_file_extractions(),
            max_concurrent_snapshot_updates: default_max_concurrent_snapshot_updates(),
        }
    }
}

impl Config {
    /// Validates the configuration
    pub fn validate(&self) -> Result<()> {
        // Validate provider
        let valid_providers = ["jina", "localapi", "api", "mock"];
        if !valid_providers.contains(&self.embeddings.provider.as_str()) {
            return Err(Error::config(format!(
                "Invalid provider '{}'. Must be one of: {:?}",
                self.embeddings.provider, valid_providers
            )));
        }

        // Validate device
        let valid_devices = ["cpu", "cuda", "metal"];
        if !valid_devices.contains(&self.embeddings.device.as_str()) {
            return Err(Error::config(format!(
                "Invalid device '{}'. Must be one of: {:?}",
                self.embeddings.device, valid_devices
            )));
        }

        // Validate embedding_dimension
        if self.embeddings.embedding_dimension == 0 {
            return Err(Error::config(
                "embedding_dimension must be greater than 0".to_string(),
            ));
        }

        // Validate max_concurrent_api_requests
        if self.embeddings.max_concurrent_api_requests == 0 {
            return Err(Error::config(
                "embeddings.max_concurrent_api_requests must be greater than 0".to_string(),
            ));
        }
        if self.embeddings.max_concurrent_api_requests > 256 {
            return Err(Error::config(format!(
                "embeddings.max_concurrent_api_requests too large (max 256, got {})",
                self.embeddings.max_concurrent_api_requests
            )));
        }

        // Validate sparse embeddings configuration
        let valid_sparse_providers = ["granite", "bm25"];
        if !valid_sparse_providers.contains(&self.sparse_embeddings.provider.as_str()) {
            return Err(Error::config(format!(
                "Invalid sparse embeddings provider '{}'. Must be one of: {:?}",
                self.sparse_embeddings.provider, valid_sparse_providers
            )));
        }

        // Validate sparse device (supports "auto", "cpu", "cuda", "cuda:N", "metal")
        let device = &self.sparse_embeddings.device;
        let valid_sparse_device = device == "auto"
            || device == "cpu"
            || device == "cuda"
            || device == "metal"
            || device.starts_with("cuda:");
        if !valid_sparse_device {
            return Err(Error::config(format!(
                "Invalid sparse embeddings device '{}'. Must be one of: auto, cpu, cuda, cuda:N, metal",
                device
            )));
        }

        if self.sparse_embeddings.top_k == 0 {
            return Err(Error::config(
                "sparse_embeddings.top_k must be greater than 0".to_string(),
            ));
        }
        if self.sparse_embeddings.top_k > 10000 {
            return Err(Error::config(format!(
                "sparse_embeddings.top_k too large (max 10000, got {})",
                self.sparse_embeddings.top_k
            )));
        }

        // Validate indexer configuration
        if self.indexer.files_per_discovery_batch == 0 {
            return Err(Error::config(
                "indexer.files_per_discovery_batch must be greater than 0".to_string(),
            ));
        }
        if self.indexer.files_per_discovery_batch > 1000 {
            return Err(Error::config(format!(
                "indexer.files_per_discovery_batch too large (max 1000, got {})",
                self.indexer.files_per_discovery_batch
            )));
        }

        if self.indexer.pipeline_channel_capacity == 0 {
            return Err(Error::config(
                "indexer.pipeline_channel_capacity must be greater than 0".to_string(),
            ));
        }
        if self.indexer.pipeline_channel_capacity > 100 {
            return Err(Error::config(format!(
                "indexer.pipeline_channel_capacity too large (max 100, got {})",
                self.indexer.pipeline_channel_capacity
            )));
        }

        if self.indexer.entities_per_embedding_batch == 0 {
            return Err(Error::config(
                "indexer.entities_per_embedding_batch must be greater than 0".to_string(),
            ));
        }
        if self.indexer.entities_per_embedding_batch > 2000 {
            return Err(Error::config(format!(
                "indexer.entities_per_embedding_batch too large (max 2000, got {})",
                self.indexer.entities_per_embedding_batch
            )));
        }

        if self.indexer.max_concurrent_file_extractions == 0 {
            return Err(Error::config(
                "indexer.max_concurrent_file_extractions must be greater than 0".to_string(),
            ));
        }
        if self.indexer.max_concurrent_file_extractions > 128 {
            return Err(Error::config(format!(
                "indexer.max_concurrent_file_extractions too large (max 128, got {})",
                self.indexer.max_concurrent_file_extractions
            )));
        }

        if self.indexer.max_concurrent_snapshot_updates == 0 {
            return Err(Error::config(
                "indexer.max_concurrent_snapshot_updates must be greater than 0".to_string(),
            ));
        }
        if self.indexer.max_concurrent_snapshot_updates > 128 {
            return Err(Error::config(format!(
                "indexer.max_concurrent_snapshot_updates too large (max 128, got {})",
                self.indexer.max_concurrent_snapshot_updates
            )));
        }

        // Validate reranking configuration
        let valid_reranking_providers = ["jina", "vllm"];
        if !valid_reranking_providers.contains(&self.reranking.provider.as_str()) {
            return Err(Error::config(format!(
                "Invalid reranking provider '{}'. Must be one of: {:?}",
                self.reranking.provider, valid_reranking_providers
            )));
        }

        if self.reranking.enabled {
            if self.reranking.candidates == 0 {
                return Err(Error::config(
                    "reranking.candidates must be greater than 0".to_string(),
                ));
            }
            if self.reranking.candidates > 1000 {
                return Err(Error::config(format!(
                    "reranking.candidates too large (max 1000, got {})",
                    self.reranking.candidates
                )));
            }
            if self.reranking.top_k == 0 {
                return Err(Error::config(
                    "reranking.top_k must be greater than 0".to_string(),
                ));
            }
            if self.reranking.top_k > self.reranking.candidates {
                return Err(Error::config(format!(
                    "reranking.top_k ({}) cannot exceed candidates ({})",
                    self.reranking.top_k, self.reranking.candidates
                )));
            }
        }

        // Validate hybrid search configuration
        if self.hybrid_search.prefetch_multiplier == 0 {
            return Err(Error::config(
                "hybrid_search.prefetch_multiplier must be greater than 0".to_string(),
            ));
        }
        if self.hybrid_search.prefetch_multiplier > 100 {
            return Err(Error::config(format!(
                "hybrid_search.prefetch_multiplier too large (max 100, got {})",
                self.hybrid_search.prefetch_multiplier
            )));
        }

        // Validate outbox configuration
        if self.outbox.poll_interval_ms == 0 {
            return Err(Error::config(
                "outbox.poll_interval_ms must be greater than 0".to_string(),
            ));
        }
        if self.outbox.poll_interval_ms > 60_000 {
            return Err(Error::config(format!(
                "outbox.poll_interval_ms too large (max 60000ms, got {})",
                self.outbox.poll_interval_ms
            )));
        }
        if self.outbox.entries_per_poll <= 0 {
            return Err(Error::config(
                "outbox.entries_per_poll must be greater than 0".to_string(),
            ));
        }
        if self.outbox.entries_per_poll > 1000 {
            return Err(Error::config(format!(
                "outbox.entries_per_poll too large (max 1000, got {})",
                self.outbox.entries_per_poll
            )));
        }
        if self.outbox.max_retries < 0 {
            return Err(Error::config(
                "outbox.max_retries must be non-negative".to_string(),
            ));
        }
        if self.outbox.max_embedding_dim == 0 {
            return Err(Error::config(
                "outbox.max_embedding_dim must be greater than 0".to_string(),
            ));
        }
        if self.outbox.max_cached_collections == 0 {
            return Err(Error::config(
                "outbox.max_cached_collections must be greater than 0".to_string(),
            ));
        }
        if self.outbox.max_cached_collections > 1000 {
            return Err(Error::config(format!(
                "outbox.max_cached_collections too large (max 1000, got {})",
                self.outbox.max_cached_collections
            )));
        }

        Ok(())
    }

    /// Saves the configuration to a TOML file
    pub fn save(&self, path: &std::path::Path) -> Result<()> {
        let toml_string = toml::to_string_pretty(self)
            .map_err(|e| Error::config(format!("Failed to serialize config: {e}")))?;

        std::fs::write(path, toml_string)
            .map_err(|e| Error::config(format!("Failed to write config file: {e}")))?;

        Ok(())
    }

    /// Create a new ConfigBuilder with required storage config
    pub fn builder(storage: StorageConfig) -> ConfigBuilder {
        ConfigBuilder::new(storage)
    }
}

/// Builder for Config with fluent API
#[derive(Debug, Clone)]
pub struct ConfigBuilder {
    indexer: IndexerConfig,
    embeddings: EmbeddingsConfig,
    sparse_embeddings: SparseEmbeddingsConfig,
    watcher: WatcherConfig,
    storage: StorageConfig,
    server: ServerConfig,
    languages: LanguagesConfig,
    reranking: RerankingConfig,
    hybrid_search: HybridSearchConfig,
    outbox: OutboxConfig,
}

impl ConfigBuilder {
    /// Create a new ConfigBuilder with required storage config and defaults for other fields
    pub fn new(storage: StorageConfig) -> Self {
        Self {
            indexer: IndexerConfig::default(),
            embeddings: EmbeddingsConfig::default(),
            sparse_embeddings: SparseEmbeddingsConfig::default(),
            watcher: WatcherConfig::default(),
            storage,
            server: ServerConfig::default(),
            languages: LanguagesConfig::default(),
            reranking: RerankingConfig::default(),
            hybrid_search: HybridSearchConfig::default(),
            outbox: OutboxConfig::default(),
        }
    }

    /// Set the embeddings configuration
    pub fn embeddings(mut self, embeddings: EmbeddingsConfig) -> Self {
        self.embeddings = embeddings;
        self
    }

    /// Set the watcher configuration
    pub fn watcher(mut self, watcher: WatcherConfig) -> Self {
        self.watcher = watcher;
        self
    }

    /// Set the languages configuration
    pub fn languages(mut self, languages: LanguagesConfig) -> Self {
        self.languages = languages;
        self
    }

    /// Set the reranking configuration
    pub fn reranking(mut self, reranking: RerankingConfig) -> Self {
        self.reranking = reranking;
        self
    }

    /// Set the hybrid search configuration
    pub fn hybrid_search(mut self, hybrid_search: HybridSearchConfig) -> Self {
        self.hybrid_search = hybrid_search;
        self
    }

    /// Set the outbox processor configuration
    pub fn outbox(mut self, outbox: OutboxConfig) -> Self {
        self.outbox = outbox;
        self
    }

    /// Set the sparse embeddings configuration
    pub fn sparse_embeddings(mut self, sparse_embeddings: SparseEmbeddingsConfig) -> Self {
        self.sparse_embeddings = sparse_embeddings;
        self
    }

    /// Build the Config
    pub fn build(self) -> Config {
        Config {
            indexer: self.indexer,
            embeddings: self.embeddings,
            sparse_embeddings: self.sparse_embeddings,
            watcher: self.watcher,
            storage: self.storage,
            server: self.server,
            languages: self.languages,
            reranking: self.reranking,
            hybrid_search: self.hybrid_search,
            outbox: self.outbox,
        }
    }
}
