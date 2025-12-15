use crate::error::{Error, Result};
use config::{Config as ConfigLib, Environment, File};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

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

    /// Query preprocessing configuration
    #[serde(default)]
    pub query_preprocessing: QueryPreprocessingConfig,

    /// Specificity boost configuration
    #[serde(default)]
    pub specificity: SpecificityConfig,
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

    /// Task type prefix for Jina embeddings (e.g., "nl2code" -> "nl2code.query"/"nl2code.passage")
    #[serde(default = "default_task_type")]
    pub task_type: String,
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
            .field("task_type", &self.task_type)
            .finish()
    }
}

/// Configuration for file watching
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatcherConfig {
    /// Debounce time in milliseconds
    #[serde(default = "default_debounce_ms")]
    pub debounce_ms: u64,

    /// Patterns to ignore
    #[serde(default = "default_ignore_patterns")]
    pub ignore_patterns: Vec<String>,
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

    /// Request timeout in seconds for reranking API calls (default: 5)
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

/// Configuration for query preprocessing to improve search relevance
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryPreprocessingConfig {
    /// Whether query preprocessing is enabled (default: true)
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Extract code identifiers (snake_case, CamelCase, path::sep) from queries (default: true)
    #[serde(default = "default_true")]
    pub extract_identifiers: bool,

    /// Infer entity types from query text (default: false)
    /// Note: inference runs but results are not used in search filtering
    #[serde(default)]
    pub infer_entity_types: bool,

    /// Detect query intent to adjust search strategy (default: true)
    #[serde(default = "default_true")]
    pub detect_query_intent: bool,
}

impl Default for QueryPreprocessingConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            extract_identifiers: true,
            infer_entity_types: false,
            detect_query_intent: true,
        }
    }
}

/// Configuration for specificity-based score boosting
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpecificityConfig {
    /// Whether specificity boost is enabled (default: false)
    #[serde(default)]
    pub enabled: bool,

    /// Weight factor for specificity boost, 0.0-1.0 (default: 0.1)
    #[serde(default = "default_specificity_weight")]
    pub weight: f32,

    /// Maximum line count before no boost is applied (default: 500)
    #[serde(default = "default_specificity_max_lines")]
    pub max_lines: usize,
}

impl Default for SpecificityConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            weight: default_specificity_weight(),
            max_lines: default_specificity_max_lines(),
        }
    }
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

// Default constants
const DEFAULT_DEVICE: &str = "cpu";
const DEFAULT_PROVIDER: &str = "jina";
const DEFAULT_MODEL: &str = "jina-embeddings-v3";
const DEFAULT_API_BASE_URL: &str = "http://localhost:8000/v1";
const DEFAULT_BGE_INSTRUCTION: &str = "Represent this code search query for retrieving semantically similar code snippets, function implementations, type definitions, and code patterns";
const DEFAULT_TASK_TYPE: &str = "retrieval";
const DEFAULT_QDRANT_HOST: &str = "localhost";
const DEFAULT_POSTGRES_HOST: &str = "localhost";
const DEFAULT_POSTGRES_DATABASE: &str = "codesearch";
const DEFAULT_POSTGRES_USER: &str = "codesearch";
const DEFAULT_POSTGRES_PASSWORD: &str = "codesearch";

fn default_enabled_languages() -> Vec<String> {
    vec![
        "rust".to_string(),
        // "python".to_string(),
        // "javascript".to_string(),
        // "typescript".to_string(),
        // "go".to_string(),
    ]
}

fn default_texts_per_api_request() -> usize {
    64
}

fn default_device() -> String {
    DEFAULT_DEVICE.to_string()
}

fn default_provider() -> String {
    DEFAULT_PROVIDER.to_string()
}

fn default_model() -> String {
    DEFAULT_MODEL.to_string()
}

fn default_api_base_url() -> Option<String> {
    Some(DEFAULT_API_BASE_URL.to_string())
}

fn default_embedding_dimension() -> usize {
    1024
}

fn default_max_concurrent_api_requests() -> usize {
    4 // Reduced from 64 to prevent vLLM OOM
}

fn default_bge_instruction() -> String {
    DEFAULT_BGE_INSTRUCTION.to_string()
}

fn default_embedding_retry_attempts() -> usize {
    5
}

fn default_task_type() -> String {
    DEFAULT_TASK_TYPE.to_string()
}

fn default_debounce_ms() -> u64 {
    500
}

fn default_ignore_patterns() -> Vec<String> {
    vec![
        "*.log".to_string(),
        "node_modules".to_string(),
        "target".to_string(),
        ".git".to_string(),
        "*.pyc".to_string(),
        "__pycache__".to_string(),
    ]
}

fn default_qdrant_host() -> String {
    DEFAULT_QDRANT_HOST.to_string()
}

fn default_qdrant_port() -> u16 {
    6334
}

fn default_qdrant_rest_port() -> u16 {
    6333
}

fn default_auto_start_deps() -> bool {
    true
}

fn default_postgres_host() -> String {
    DEFAULT_POSTGRES_HOST.to_string()
}

fn default_postgres_port() -> u16 {
    5432
}

fn default_postgres_database() -> String {
    DEFAULT_POSTGRES_DATABASE.to_string()
}

fn default_postgres_user() -> String {
    DEFAULT_POSTGRES_USER.to_string()
}

fn default_postgres_password() -> String {
    DEFAULT_POSTGRES_PASSWORD.to_string()
}

fn default_postgres_pool_size() -> u32 {
    20 // Increased from SQLx default of 5 for better concurrency
}

fn default_neo4j_host() -> String {
    "localhost".to_string()
}

fn default_neo4j_http_port() -> u16 {
    7474
}

fn default_neo4j_bolt_port() -> u16 {
    7687
}

fn default_neo4j_user() -> String {
    "neo4j".to_string()
}

fn default_neo4j_password() -> String {
    "codesearch".to_string()
}

fn default_entities_per_embedding_batch() -> usize {
    500 // Reduced from 2000 to prevent vLLM OOM
}

pub fn default_max_entities_per_db_operation() -> usize {
    10000
}

fn default_server_port() -> u16 {
    3000
}

fn default_allowed_origins() -> Vec<String> {
    Vec::new() // Empty by default = CORS disabled
}

fn default_files_per_discovery_batch() -> usize {
    50
}

fn default_pipeline_channel_capacity() -> usize {
    20
}

fn default_max_concurrent_file_extractions() -> usize {
    32
}

fn default_max_concurrent_snapshot_updates() -> usize {
    16
}

fn default_enable_reranking() -> bool {
    false
}

fn default_reranking_provider() -> String {
    "jina".to_string()
}

fn default_reranking_model() -> String {
    "jina-reranker-v3".to_string()
}

fn default_reranking_candidates() -> usize {
    100 // Reduced from 350 for Jina rate limits (vLLM can handle more)
}

fn default_reranking_top_k() -> usize {
    10
}

fn default_reranking_timeout_secs() -> u64 {
    5
}

fn default_reranking_max_concurrent_requests() -> usize {
    16
}

fn default_prefetch_multiplier() -> usize {
    5
}

fn default_true() -> bool {
    true
}

fn default_specificity_weight() -> f32 {
    0.1
}

fn default_specificity_max_lines() -> usize {
    500
}

fn default_outbox_poll_interval_ms() -> u64 {
    1000
}

fn default_outbox_entries_per_poll() -> i64 {
    500
}

fn default_outbox_max_retries() -> i32 {
    3
}

fn default_outbox_max_embedding_dim() -> usize {
    100_000
}

fn default_outbox_max_cached_collections() -> usize {
    200
}

fn default_outbox_drain_timeout_secs() -> u64 {
    600 // 10 minutes - sufficient for ~100k entries at 200 entries/sec
}

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
            task_type: default_task_type(),
        }
    }
}

impl Default for WatcherConfig {
    fn default() -> Self {
        Self {
            debounce_ms: default_debounce_ms(),
            ignore_patterns: default_ignore_patterns(),
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

impl StorageConfig {
    /// Generate a collection name from a repository path
    ///
    /// Creates a unique, deterministic Qdrant-compatible collection name using xxHash3_128.
    /// Format: `<sanitized_repo_name>_<xxhash3_128_hex>`
    ///
    /// The repo name is sanitized (alphanumeric, dash, underscore only) and truncated to
    /// 50 characters if needed. The full absolute path is hashed using xxHash3_128 to ensure
    /// uniqueness. The same path always generates the same collection name.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The current directory cannot be determined (for relative paths)
    /// - The path has no valid filename component
    /// - The filename cannot be converted to UTF-8
    pub fn generate_collection_name(repo_path: &Path) -> Result<String> {
        use twox_hash::XxHash3_128;

        // Get the absolute path without requiring it to exist
        let absolute_path = if repo_path.is_absolute() {
            repo_path.to_path_buf()
        } else {
            std::env::current_dir()
                .map_err(|e| Error::config(format!("Failed to get current dir: {e}")))?
                .join(repo_path)
        };

        // Canonicalize the path to resolve symlinks and normalize (e.g., remove .. and .)
        // This prevents the same repository from being registered multiple times with
        // different path representations (e.g., /home/user/repo vs /home/user/../user/repo)
        // If the path doesn't exist, fall back to the absolute path
        let normalized_path =
            std::fs::canonicalize(&absolute_path).unwrap_or_else(|_| absolute_path.clone());

        // Extract repository name (last component of path)
        let repo_name = normalized_path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| {
                Error::config(format!(
                    "Path {} has no valid filename component",
                    normalized_path.display()
                ))
            })?;

        // Truncate repo name to 50 chars and sanitize
        let sanitized_name: String = repo_name
            .chars()
            .take(50)
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();

        // Hash the full normalized path to ensure uniqueness
        let path_str = normalized_path.to_string_lossy();
        let hash = XxHash3_128::oneshot(path_str.as_bytes());

        // Format: <repo_name>_<hash>
        Ok(format!("{sanitized_name}_{hash:032x}"))
    }

    /// Generate a deterministic repository ID from repository path
    ///
    /// Creates a deterministic UUID v5 from a repository path, ensuring the same path
    /// always generates the same UUID. This makes entity IDs stable across re-indexing,
    /// even if the repository is dropped and re-indexed.
    ///
    /// # UUID v5 Generation
    ///
    /// Uses UUID v5 (name-based, SHA-1) as defined in RFC 4122. The UUID is generated
    /// from the normalized repository path using `NAMESPACE_DNS` as the namespace UUID.
    /// While DNS namespace is typically used for domain names, it's a standard, well-known
    /// namespace suitable for generating deterministic UUIDs from filesystem paths.
    ///
    /// # Path Normalization
    ///
    /// The function normalizes paths to ensure consistent UUID generation:
    ///
    /// 1. **Relative paths**: Converted to absolute paths using the current working directory
    /// 2. **Symlinks**: Resolved to their target path via `std::fs::canonicalize`
    /// 3. **Path components**: Normalized (`.` and `..` are resolved)
    ///
    /// If canonicalization fails (e.g., permission errors, I/O errors), the function falls
    /// back to the absolute (but non-canonical) path. A warning is logged for non-NotFound
    /// errors to help diagnose cases where different path representations might generate
    /// different UUIDs.
    ///
    /// # Idempotency and Thread Safety
    ///
    /// - **Idempotent**: Calling this function multiple times with the same path always
    ///   returns the same UUID
    /// - **Thread-safe**: This function has no mutable state and can be called safely
    ///   from multiple threads
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The current directory cannot be determined (for relative paths)
    ///
    /// # Examples
    ///
    /// ```
    /// use codesearch_core::config::StorageConfig;
    /// use std::path::Path;
    ///
    /// // Absolute path
    /// let id1 = StorageConfig::generate_repository_id(Path::new("/home/user/repo")).unwrap();
    ///
    /// // Same path should produce same UUID
    /// let id2 = StorageConfig::generate_repository_id(Path::new("/home/user/repo")).unwrap();
    /// assert_eq!(id1, id2);
    ///
    /// // Different paths produce different UUIDs
    /// let id3 = StorageConfig::generate_repository_id(Path::new("/home/user/other")).unwrap();
    /// assert_ne!(id1, id3);
    /// ```
    pub fn generate_repository_id(repo_path: &Path) -> Result<uuid::Uuid> {
        // Get the absolute path without requiring it to exist
        let absolute_path = if repo_path.is_absolute() {
            repo_path.to_path_buf()
        } else {
            std::env::current_dir()
                .map_err(|e| Error::config(format!("Failed to get current dir: {e}")))?
                .join(repo_path)
        };

        // Canonicalize the path to resolve symlinks and normalize
        // If canonicalization fails, fall back to the absolute path and log a warning
        let normalized_path = match std::fs::canonicalize(&absolute_path) {
            Ok(canonical) => canonical,
            Err(e) => {
                // Log warning for non-NotFound errors (permissions, I/O, etc.)
                if e.kind() != std::io::ErrorKind::NotFound {
                    tracing::warn!(
                        path = %absolute_path.display(),
                        error = %e,
                        "Failed to canonicalize repository path, using absolute path. \
                         Different path representations may generate different repository IDs."
                    );
                }
                absolute_path.clone()
            }
        };

        // Generate deterministic UUID v5 from the normalized path
        // Using DNS namespace as it's a standard namespace for name-based UUIDs
        let path_str = normalized_path.to_string_lossy();
        let repository_id = uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_DNS, path_str.as_bytes());

        Ok(repository_id)
    }
}

impl Config {
    /// Loads configuration from a TOML file with environment variable overrides
    ///
    /// Environment variables are prefixed with `CODESEARCH_` and use double underscores
    /// for nested values. For example:
    /// - `CODESEARCH_EMBEDDINGS__PROVIDER=openai`
    pub fn from_file(path: &Path) -> Result<Self> {
        let mut builder = ConfigLib::builder()
            // Set outbox defaults explicitly (config crate doesn't apply serde defaults for missing sections)
            .set_default(
                "outbox.poll_interval_ms",
                default_outbox_poll_interval_ms() as i64,
            )
            .map_err(|e| Error::config(format!("Failed to set outbox default: {e}")))?
            .set_default("outbox.entries_per_poll", default_outbox_entries_per_poll())
            .map_err(|e| Error::config(format!("Failed to set outbox default: {e}")))?
            .set_default("outbox.max_retries", default_outbox_max_retries() as i64)
            .map_err(|e| Error::config(format!("Failed to set outbox default: {e}")))?
            .set_default(
                "outbox.max_embedding_dim",
                default_outbox_max_embedding_dim() as i64,
            )
            .map_err(|e| Error::config(format!("Failed to set outbox default: {e}")))?
            .set_default(
                "outbox.max_cached_collections",
                default_outbox_max_cached_collections() as i64,
            )
            .map_err(|e| Error::config(format!("Failed to set outbox default: {e}")))?
            .set_default(
                "outbox.drain_timeout_secs",
                default_outbox_drain_timeout_secs() as i64,
            )
            .map_err(|e| Error::config(format!("Failed to set outbox default: {e}")))?
            // Reranking defaults
            .set_default("reranking.enabled", default_enable_reranking())
            .map_err(|e| Error::config(format!("Failed to set reranking default: {e}")))?
            .set_default("reranking.provider", default_reranking_provider())
            .map_err(|e| Error::config(format!("Failed to set reranking default: {e}")))?
            .set_default("reranking.model", default_reranking_model())
            .map_err(|e| Error::config(format!("Failed to set reranking default: {e}")))?
            .set_default(
                "reranking.candidates",
                default_reranking_candidates() as i64,
            )
            .map_err(|e| Error::config(format!("Failed to set reranking default: {e}")))?
            .set_default("reranking.top_k", default_reranking_top_k() as i64)
            .map_err(|e| Error::config(format!("Failed to set reranking default: {e}")))?
            .set_default(
                "reranking.timeout_secs",
                default_reranking_timeout_secs() as i64,
            )
            .map_err(|e| Error::config(format!("Failed to set reranking default: {e}")))?
            .set_default(
                "reranking.max_concurrent_requests",
                default_reranking_max_concurrent_requests() as i64,
            )
            .map_err(|e| Error::config(format!("Failed to set reranking default: {e}")))?;

        // Add the config file if it exists
        if path.exists() {
            builder = builder.add_source(File::from(path));
        }

        // Add environment variables with CODESEARCH_ prefix
        builder = builder.add_source(
            Environment::with_prefix("CODESEARCH")
                .separator("__")
                .try_parsing(true),
        );

        // Support backward-compatible environment variables for storage
        if let Ok(host) = std::env::var("QDRANT_HOST") {
            builder = builder
                .set_override("storage.qdrant_host", host)
                .map_err(|e| Error::config(format!("Failed to set QDRANT_HOST: {e}")))?;
        }
        if let Ok(port) = std::env::var("QDRANT_PORT") {
            if let Ok(port_num) = port.parse::<u16>() {
                builder = builder
                    .set_override("storage.qdrant_port", port_num)
                    .map_err(|e| Error::config(format!("Failed to set QDRANT_PORT: {e}")))?;
            }
        }
        if let Ok(port) = std::env::var("QDRANT_REST_PORT") {
            if let Ok(port_num) = port.parse::<u16>() {
                builder = builder
                    .set_override("storage.qdrant_rest_port", port_num)
                    .map_err(|e| Error::config(format!("Failed to set QDRANT_REST_PORT: {e}")))?;
            }
        }

        // Support Postgres environment variables
        if let Ok(host) = std::env::var("POSTGRES_HOST") {
            builder = builder
                .set_override("storage.postgres_host", host)
                .map_err(|e| Error::config(format!("Failed to set POSTGRES_HOST: {e}")))?;
        }
        if let Ok(port) = std::env::var("POSTGRES_PORT") {
            if let Ok(port_num) = port.parse::<u16>() {
                builder = builder
                    .set_override("storage.postgres_port", port_num)
                    .map_err(|e| Error::config(format!("Failed to set POSTGRES_PORT: {e}")))?;
            }
        }
        if let Ok(db) = std::env::var("POSTGRES_DATABASE") {
            builder = builder
                .set_override("storage.postgres_database", db)
                .map_err(|e| Error::config(format!("Failed to set POSTGRES_DATABASE: {e}")))?;
        }
        if let Ok(user) = std::env::var("POSTGRES_USER") {
            builder = builder
                .set_override("storage.postgres_user", user)
                .map_err(|e| Error::config(format!("Failed to set POSTGRES_USER: {e}")))?;
        }
        if let Ok(password) = std::env::var("POSTGRES_PASSWORD") {
            builder = builder
                .set_override("storage.postgres_password", password)
                .map_err(|e| Error::config(format!("Failed to set POSTGRES_PASSWORD: {e}")))?;
        }

        // Neo4j configuration
        if let Ok(host) = std::env::var("NEO4J_HOST") {
            builder = builder
                .set_override("storage.neo4j_host", host)
                .map_err(|e| Error::config(format!("Failed to set NEO4J_HOST: {e}")))?;
        }
        if let Ok(port) = std::env::var("NEO4J_BOLT_PORT") {
            if let Ok(port_num) = port.parse::<u16>() {
                builder = builder
                    .set_override("storage.neo4j_bolt_port", port_num)
                    .map_err(|e| Error::config(format!("Failed to set NEO4J_BOLT_PORT: {e}")))?;
            }
        }
        if let Ok(password) = std::env::var("NEO4J_PASSWORD") {
            builder = builder
                .set_override("storage.neo4j_password", password)
                .map_err(|e| Error::config(format!("Failed to set NEO4J_PASSWORD: {e}")))?;
        }

        // Support indexer environment variables
        if let Ok(batch_size) = std::env::var("CODESEARCH_INDEXER__FILES_PER_DISCOVERY_BATCH") {
            if let Ok(size) = batch_size.parse::<i64>() {
                builder = builder
                    .set_override("indexer.files_per_discovery_batch", size)
                    .map_err(|e| {
                        Error::config(format!("Failed to set files_per_discovery_batch: {e}"))
                    })?;
            }
        }

        if let Ok(buffer_size) = std::env::var("CODESEARCH_INDEXER__PIPELINE_CHANNEL_CAPACITY") {
            if let Ok(size) = buffer_size.parse::<i64>() {
                builder = builder
                    .set_override("indexer.pipeline_channel_capacity", size)
                    .map_err(|e| {
                        Error::config(format!("Failed to set pipeline_channel_capacity: {e}"))
                    })?;
            }
        }

        if let Ok(entity_batch) = std::env::var("CODESEARCH_INDEXER__ENTITIES_PER_EMBEDDING_BATCH")
        {
            if let Ok(size) = entity_batch.parse::<i64>() {
                builder = builder
                    .set_override("indexer.entities_per_embedding_batch", size)
                    .map_err(|e| {
                        Error::config(format!("Failed to set entities_per_embedding_batch: {e}"))
                    })?;
            }
        }

        if let Ok(concurrency) =
            std::env::var("CODESEARCH_INDEXER__MAX_CONCURRENT_FILE_EXTRACTIONS")
        {
            if let Ok(val) = concurrency.parse::<i64>() {
                builder = builder
                    .set_override("indexer.max_concurrent_file_extractions", val)
                    .map_err(|e| {
                        Error::config(format!(
                            "Failed to set max_concurrent_file_extractions: {e}"
                        ))
                    })?;
            }
        }

        if let Ok(concurrency) =
            std::env::var("CODESEARCH_INDEXER__MAX_CONCURRENT_SNAPSHOT_UPDATES")
        {
            if let Ok(val) = concurrency.parse::<i64>() {
                builder = builder
                    .set_override("indexer.max_concurrent_snapshot_updates", val)
                    .map_err(|e| {
                        Error::config(format!(
                            "Failed to set max_concurrent_snapshot_updates: {e}"
                        ))
                    })?;
            }
        }

        let config = builder
            .build()
            .map_err(|e| Error::config(format!("Failed to build config: {e}")))?;

        config
            .try_deserialize()
            .map_err(|e| Error::config(format!("Failed to deserialize config: {e}")))
    }

    /// Creates a config from a TOML string (useful for testing)
    pub fn from_toml_str(content: &str) -> Result<Self> {
        toml::from_str(content).map_err(|e| Error::config(format!("Failed to parse TOML: {e}")))
    }

    /// Load configuration from a single file
    ///
    /// Precedence (lowest to highest):
    /// 1. Hardcoded defaults
    /// 2. Config file (~/.codesearch/config.toml or custom --config path)
    /// 3. Environment variables (CODESEARCH_*)
    pub fn load(config_path: Option<&Path>) -> Result<Self> {
        let path = match config_path {
            Some(p) => p.to_path_buf(),
            None => global_config_path()?,
        };
        Self::from_file(&path)
    }

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

        // Validate specificity configuration
        if self.specificity.enabled {
            if self.specificity.weight < 0.0 || self.specificity.weight > 1.0 {
                return Err(Error::config(format!(
                    "specificity.weight must be between 0.0 and 1.0 (got {})",
                    self.specificity.weight
                )));
            }
            if self.specificity.max_lines == 0 {
                return Err(Error::config(
                    "specificity.max_lines must be greater than 0".to_string(),
                ));
            }
        }

        Ok(())
    }

    /// Saves the configuration to a TOML file
    pub fn save(&self, path: &Path) -> Result<()> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn create_temp_config_file(content: &str) -> Result<NamedTempFile> {
        let mut file = tempfile::Builder::new()
            .suffix(".toml")
            .tempfile()
            .map_err(|e| Error::config(format!("Failed to create temp file: {e}")))?;
        file.write_all(content.as_bytes())
            .map_err(|e| Error::config(format!("Failed to write temp file: {e}")))?;
        file.flush()
            .map_err(|e| Error::config(format!("Failed to flush temp file: {e}")))?;
        Ok(file)
    }

    fn with_env_var<F, T>(key: &str, value: &str, f: F) -> T
    where
        F: FnOnce() -> T,
    {
        std::env::set_var(key, value);
        let result = f();
        std::env::remove_var(key);
        result
    }

    #[test]
    fn test_from_toml_str_valid() {
        let toml = r#"
            [indexer]

            [embeddings]
            provider = "localapi"
            model = "nomic-embed-text-v1.5"
            device = "cpu"
            embedding_dimension = 768

            [watcher]

            [storage]
            qdrant_host = "localhost"
            qdrant_port = 6334
        "#;

        let config = Config::from_toml_str(toml).expect("Failed to parse valid TOML");
        assert_eq!(config.embeddings.provider, "localapi");
        assert_eq!(config.embeddings.embedding_dimension, 768);
    }

    #[test]
    fn test_from_toml_str_minimal() {
        let toml = r#"
            [indexer]

            [embeddings]

            [watcher]

            [storage]
        "#;

        let config = Config::from_toml_str(toml).expect("Failed to parse minimal TOML");
        // Check defaults are applied
        assert_eq!(config.embeddings.provider, "jina");
        assert_eq!(config.embeddings.device, "cpu");
    }

    #[test]
    fn test_from_toml_str_invalid_syntax() {
        let toml = r#"
            [embeddings
            provider = "localapi"
        "#;

        let result = Config::from_toml_str(toml);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Failed to parse TOML"));
    }

    #[test]
    fn test_validate_valid_config() {
        let toml = r#"
            [indexer]

            [embeddings]
            provider = "localapi"
            device = "cpu"
            embedding_dimension = 1536

            [watcher]

            [storage]
        "#;

        let config = Config::from_toml_str(toml).expect("Failed to parse TOML");
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_invalid_provider() {
        let toml = r#"
            [indexer]

            [embeddings]
            provider = "invalid_provider"

            [watcher]

            [storage]
        "#;

        let config = Config::from_toml_str(toml).expect("Failed to parse TOML");
        let result = config.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Invalid provider"));
    }

    #[test]
    fn test_validate_invalid_device() {
        let toml = r#"
            [indexer]

            [embeddings]
            provider = "localapi"
            device = "gpu"

            [watcher]

            [storage]
        "#;

        let config = Config::from_toml_str(toml).expect("Failed to parse TOML");
        let result = config.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Invalid device"));
    }

    #[test]
    fn test_validate_zero_embedding_dimension() {
        let toml = r#"
            [indexer]

            [embeddings]
            provider = "localapi"
            device = "cpu"
            embedding_dimension = 0

            [watcher]

            [storage]
        "#;

        let config = Config::from_toml_str(toml).expect("Failed to parse TOML");
        let result = config.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("embedding_dimension must be greater than 0"));
    }

    #[test]
    fn test_save_and_load_roundtrip() -> Result<()> {
        let original_toml = r#"
            [indexer]

            [embeddings]
            provider = "mock"
            model = "test-model"
            device = "cpu"
            embedding_dimension = 384

            [watcher]

            [storage]
            qdrant_host = "testhost"
            qdrant_port = 7777
        "#;

        let config = Config::from_toml_str(original_toml)?;

        // Save to temp file
        let temp_file = NamedTempFile::new()
            .map_err(|e| Error::config(format!("Failed to create temp file: {e}")))?;
        config.save(temp_file.path())?;

        // Load from temp file
        let loaded_content = std::fs::read_to_string(temp_file.path())
            .map_err(|e| Error::config(format!("Failed to read temp file: {e}")))?;
        let loaded_config = Config::from_toml_str(&loaded_content)?;

        // Verify roundtrip
        assert_eq!(
            config.embeddings.provider,
            loaded_config.embeddings.provider
        );
        assert_eq!(config.embeddings.model, loaded_config.embeddings.model);
        assert_eq!(
            config.embeddings.embedding_dimension,
            loaded_config.embeddings.embedding_dimension
        );

        Ok(())
    }

    #[test]
    fn test_from_file_loads_successfully() {
        let toml = r#"
            [indexer]

            [embeddings]
            provider = "mock"

            [watcher]

            [storage]
        "#;

        let temp_file = create_temp_config_file(toml).expect("Failed to create temp file");

        let config = Config::from_file(temp_file.path()).expect("Failed to load config from file");
        assert_eq!(config.embeddings.provider, "mock");
    }

    #[test]
    fn test_from_file_backward_compat_qdrant() {
        let toml = r#"
            [indexer]

            [embeddings]

            [watcher]

            [storage]
        "#;

        let temp_file = create_temp_config_file(toml).expect("Failed to create temp file");

        with_env_var("QDRANT_HOST", "remote.example.com", || {
            with_env_var("QDRANT_PORT", "7334", || {
                let config =
                    Config::from_file(temp_file.path()).expect("Failed to load config from file");
                assert_eq!(config.storage.qdrant_host, "remote.example.com");
                assert_eq!(config.storage.qdrant_port, 7334);
            });
        });
    }

    #[test]
    fn test_from_file_backward_compat_postgres() {
        let toml = r#"
            [indexer]

            [embeddings]

            [watcher]

            [storage]
        "#;

        let temp_file = create_temp_config_file(toml).expect("Failed to create temp file");

        with_env_var("POSTGRES_HOST", "db.example.com", || {
            with_env_var("POSTGRES_DATABASE", "testdb", || {
                let config =
                    Config::from_file(temp_file.path()).expect("Failed to load config from file");
                assert_eq!(config.storage.postgres_host, "db.example.com");
                assert_eq!(config.storage.postgres_database, "testdb");
            });
        });
    }

    #[test]
    fn test_save_creates_valid_toml() {
        let toml = r#"
            [indexer]

            [embeddings]
            provider = "mock"
            model = "test-model"

            [watcher]

            [storage]
        "#;

        let config = Config::from_toml_str(toml).expect("Failed to parse TOML");

        // Save to temp file
        let temp_file = NamedTempFile::new()
            .map_err(|e| Error::config(format!("Failed to create temp file: {e}")))
            .expect("Failed to create temp file");
        config
            .save(temp_file.path())
            .expect("Failed to save config");

        // Verify file was created and is valid TOML
        assert!(temp_file.path().exists());
        let saved_content =
            std::fs::read_to_string(temp_file.path()).expect("Failed to read saved config");
        assert!(saved_content.contains("[embeddings]"));
        assert!(saved_content.contains("[storage]"));
    }

    #[test]
    fn test_generate_collection_name() {
        let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
        let collection_name = StorageConfig::generate_collection_name(temp_dir.path())
            .expect("Failed to generate collection name");

        // Verify format: name_hash
        assert!(collection_name.contains('_'));

        // Verify length is reasonable (50 + 1 + 32 = 83 max)
        assert!(collection_name.len() <= 83);

        // Verify only contains valid characters
        assert!(collection_name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
    }

    #[test]
    fn test_generate_collection_name_special_chars() {
        let temp_base = tempfile::tempdir().expect("Failed to create temp dir");
        let special_path = temp_base.path().join("my repo (v2.0)!");

        // Create the directory
        std::fs::create_dir(&special_path).expect("Failed to create dir");

        let collection_name = StorageConfig::generate_collection_name(&special_path)
            .expect("Failed to generate collection name");

        // Special characters should be replaced with underscores
        assert!(!collection_name.contains('('));
        assert!(!collection_name.contains(')'));
        assert!(!collection_name.contains('!'));
        assert!(!collection_name.contains(' '));
    }

    #[test]
    fn test_generate_collection_name_deterministic() {
        let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");

        let name1 = StorageConfig::generate_collection_name(temp_dir.path())
            .expect("Failed to generate collection name");
        let name2 = StorageConfig::generate_collection_name(temp_dir.path())
            .expect("Failed to generate collection name");

        // Same path should generate same name
        assert_eq!(name1, name2);
    }

    #[test]
    fn test_generate_collection_name_nonexistent_path() {
        // Non-existent paths should now work (no canonicalization required)
        let nonexistent = std::path::PathBuf::from("/tmp/this_path_does_not_exist_test_12345");

        let result = StorageConfig::generate_collection_name(&nonexistent);
        assert!(result.is_ok());

        let collection_name = result.expect("test setup failed");
        assert!(collection_name.contains("this_path_does_not_exist_test_12345"));
        assert!(collection_name.contains('_')); // Should have hash separator
    }

    #[test]
    fn test_generate_collection_name_relative_path() {
        // Relative paths should work and be converted to absolute
        let relative = std::path::PathBuf::from("relative/test/path");

        let result = StorageConfig::generate_collection_name(&relative);
        assert!(result.is_ok());

        let collection_name = result.expect("test setup failed");
        // Should use the last component as the name
        assert!(collection_name.starts_with("path_"));
    }

    #[test]
    fn test_generate_collection_name_root_path() {
        // Root path should fail - no filename component
        let root = std::path::PathBuf::from("/");

        let result = StorageConfig::generate_collection_name(&root);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("no valid filename component"));
    }

    #[test]
    fn test_generate_repository_id_deterministic() {
        let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");

        let id1 = StorageConfig::generate_repository_id(temp_dir.path())
            .expect("Failed to generate repository ID");
        let id2 = StorageConfig::generate_repository_id(temp_dir.path())
            .expect("Failed to generate repository ID");

        // Same path should generate same UUID
        assert_eq!(id1, id2);
    }

    #[test]
    fn test_generate_repository_id_different_paths() {
        let temp_dir1 = tempfile::tempdir().expect("Failed to create temp dir 1");
        let temp_dir2 = tempfile::tempdir().expect("Failed to create temp dir 2");

        let id1 = StorageConfig::generate_repository_id(temp_dir1.path())
            .expect("Failed to generate repository ID 1");
        let id2 = StorageConfig::generate_repository_id(temp_dir2.path())
            .expect("Failed to generate repository ID 2");

        // Different paths should generate different UUIDs
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_generate_repository_id_relative_vs_absolute() {
        let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");

        // Get absolute path
        let absolute_id = StorageConfig::generate_repository_id(temp_dir.path())
            .expect("Failed to generate ID from absolute path");

        // Change to parent directory and use relative path
        let original_dir = std::env::current_dir().expect("Failed to get current dir");
        let parent = temp_dir.path().parent().expect("No parent directory");
        let dir_name = temp_dir.path().file_name().expect("No file name");

        std::env::set_current_dir(parent).expect("Failed to change directory");
        let relative_id =
            StorageConfig::generate_repository_id(&std::path::PathBuf::from(dir_name))
                .expect("Failed to generate ID from relative path");
        std::env::set_current_dir(original_dir).expect("Failed to restore directory");

        // Relative and absolute paths should generate same UUID
        assert_eq!(absolute_id, relative_id);
    }

    #[test]
    fn test_generate_repository_id_symlink_resolution() {
        use std::os::unix::fs::symlink;

        let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
        let real_path = temp_dir.path().join("real_repo");
        let symlink_path = temp_dir.path().join("symlink_repo");

        std::fs::create_dir(&real_path).expect("Failed to create real directory");
        symlink(&real_path, &symlink_path).expect("Failed to create symlink");

        let real_id = StorageConfig::generate_repository_id(&real_path)
            .expect("Failed to generate ID from real path");
        let symlink_id = StorageConfig::generate_repository_id(&symlink_path)
            .expect("Failed to generate ID from symlink");

        // Symlink should resolve to same UUID as real path
        assert_eq!(real_id, symlink_id);
    }

    #[test]
    fn test_generate_repository_id_path_normalization() {
        let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");

        // Create subdirectory and sibling
        let subdir = temp_dir.path().join("subdir");
        let other = temp_dir.path().join("other");
        std::fs::create_dir(&subdir).expect("Failed to create subdirectory");
        std::fs::create_dir(&other).expect("Failed to create other directory");

        // Generate ID from clean path
        let clean_id = StorageConfig::generate_repository_id(&subdir)
            .expect("Failed to generate ID from clean path");

        // Generate ID from path with .. (e.g., /tmp/foo/other/../subdir)
        // This path exists and will canonicalize to /tmp/foo/subdir
        let with_parent = other.join("..").join("subdir");
        let normalized_id = StorageConfig::generate_repository_id(&with_parent)
            .expect("Failed to generate ID from path with ..");

        // Both should generate same UUID (after canonicalization)
        assert_eq!(clean_id, normalized_id);
    }

    #[test]
    fn test_generate_repository_id_uuid_v5_format() {
        let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");

        let id = StorageConfig::generate_repository_id(temp_dir.path())
            .expect("Failed to generate repository ID");

        // UUID v5 has version bits set to 0101 (5) in the time_hi_and_version field
        // The variant should be 10xx (RFC 4122)
        let bytes = id.as_bytes();

        // Check version (bits 4-7 of byte 6 should be 0101 = 5)
        assert_eq!((bytes[6] >> 4) & 0x0F, 5, "UUID should be version 5");

        // Check variant (bits 6-7 of byte 8 should be 10)
        assert_eq!(
            (bytes[8] >> 6) & 0x03,
            2,
            "UUID should have RFC 4122 variant"
        );
    }

    #[test]
    fn test_generate_repository_id_nonexistent_path() {
        // Non-existent paths should work (no canonicalization required for generation)
        let nonexistent = std::path::PathBuf::from("/tmp/this_path_does_not_exist_test_12345");

        let result = StorageConfig::generate_repository_id(&nonexistent);
        assert!(result.is_ok(), "Should handle non-existent paths");

        // Should be deterministic even for non-existent paths
        let id1 = result.expect("Failed to generate ID");
        let id2 = StorageConfig::generate_repository_id(&nonexistent)
            .expect("Failed to generate ID second time");
        assert_eq!(id1, id2, "Non-existent paths should still be deterministic");
    }

    #[test]
    fn test_reranking_config_custom_values() {
        let toml = r#"
            [indexer]

            [embeddings]

            [watcher]

            [storage]

            [reranking]
            enabled = true
            model = "custom-model"
            candidates = 100
            top_k = 20
            api_base_url = "http://localhost:8001"
        "#;

        let config =
            Config::from_toml_str(toml).expect("Failed to parse TOML with custom reranking");

        assert!(config.reranking.enabled);
        assert_eq!(config.reranking.model, "custom-model");
        assert_eq!(config.reranking.candidates, 100);
        assert_eq!(config.reranking.top_k, 20);
        assert_eq!(
            config.reranking.api_base_url,
            Some("http://localhost:8001".to_string())
        );
    }

    #[test]
    fn test_reranking_validation_candidates_too_large() {
        let toml = r#"
            [indexer]

            [embeddings]

            [watcher]

            [storage]

            [reranking]
            enabled = true
            candidates = 2000
        "#;

        let config = Config::from_toml_str(toml).expect("Failed to parse TOML");
        let result = config.validate();

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("too large"));
    }

    #[test]
    fn test_reranking_validation_top_k_exceeds_candidates() {
        let toml = r#"
            [indexer]

            [embeddings]

            [watcher]

            [storage]

            [reranking]
            enabled = true
            candidates = 50
            top_k = 100
        "#;

        let config = Config::from_toml_str(toml).expect("Failed to parse TOML");
        let result = config.validate();

        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("top_k"));
        assert!(error_msg.contains("cannot exceed"));
    }

    #[test]
    fn test_reranking_validation_disabled_no_check() {
        let toml = r#"
            [indexer]

            [embeddings]

            [watcher]

            [storage]

            [reranking]
            enabled = false
            candidates = 2000
            top_k = 100
        "#;

        let config = Config::from_toml_str(toml).expect("Failed to parse TOML");
        let result = config.validate();

        // Should pass validation because reranking is disabled
        assert!(result.is_ok());
    }

    #[test]
    fn test_reranking_config_jina_provider() {
        let toml = r#"
            [indexer]
            [embeddings]
            [watcher]
            [storage]

            [reranking]
            enabled = true
            provider = "jina"
            model = "jina-reranker-v3"
            api_key = "test_key"
            candidates = 100
        "#;

        let config = Config::from_toml_str(toml).expect("Failed to parse TOML with Jina provider");

        assert!(config.reranking.enabled);
        assert_eq!(config.reranking.provider, "jina");
        assert_eq!(config.reranking.model, "jina-reranker-v3");
        assert_eq!(config.reranking.api_key, Some("test_key".to_string()));
        assert_eq!(config.reranking.candidates, 100);
    }

    #[test]
    fn test_reranking_config_vllm_provider() {
        let toml = r#"
            [indexer]
            [embeddings]
            [watcher]
            [storage]

            [reranking]
            enabled = true
            provider = "vllm"
            model = "BAAI/bge-reranker-v2-m3"
            api_base_url = "http://localhost:8001/v1"
            candidates = 350
        "#;

        let config = Config::from_toml_str(toml).expect("Failed to parse TOML with vLLM provider");

        assert!(config.reranking.enabled);
        assert_eq!(config.reranking.provider, "vllm");
        assert_eq!(config.reranking.model, "BAAI/bge-reranker-v2-m3");
        assert_eq!(config.reranking.candidates, 350);
    }

    #[test]
    fn test_reranking_config_invalid_provider() {
        let toml = r#"
            [indexer]
            [embeddings]
            [watcher]
            [storage]

            [reranking]
            provider = "invalid_provider"
        "#;

        let config = Config::from_toml_str(toml).expect("Failed to parse TOML");
        let result = config.validate();

        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("Invalid reranking provider"));
        assert!(error_msg.contains("invalid_provider"));
    }

    #[test]
    fn test_reranking_defaults_to_jina() {
        let config = RerankingConfig::default();
        assert_eq!(config.provider, "jina");
        assert_eq!(config.model, "jina-reranker-v3");
        assert_eq!(config.candidates, 100);
    }

    #[test]
    fn test_outbox_validation_poll_interval_zero() {
        let toml = r#"
            [indexer]
            [embeddings]
            [watcher]
            [storage]
            [outbox]
            poll_interval_ms = 0
        "#;
        let config = Config::from_toml_str(toml).expect("Failed to parse TOML");
        let result = config.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("poll_interval_ms must be greater than 0"));
    }

    #[test]
    fn test_outbox_validation_poll_interval_too_large() {
        let toml = r#"
            [indexer]
            [embeddings]
            [watcher]
            [storage]
            [outbox]
            poll_interval_ms = 60001
        "#;
        let config = Config::from_toml_str(toml).expect("Failed to parse TOML");
        let result = config.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("poll_interval_ms too large"));
    }

    #[test]
    fn test_outbox_validation_entries_per_poll_zero() {
        let toml = r#"
            [indexer]
            [embeddings]
            [watcher]
            [storage]
            [outbox]
            entries_per_poll = 0
        "#;
        let config = Config::from_toml_str(toml).expect("Failed to parse TOML");
        let result = config.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("entries_per_poll must be greater than 0"));
    }

    #[test]
    fn test_outbox_validation_entries_per_poll_negative() {
        let toml = r#"
            [indexer]
            [embeddings]
            [watcher]
            [storage]
            [outbox]
            entries_per_poll = -1
        "#;
        let config = Config::from_toml_str(toml).expect("Failed to parse TOML");
        let result = config.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("entries_per_poll must be greater than 0"));
    }

    #[test]
    fn test_outbox_validation_entries_per_poll_too_large() {
        let toml = r#"
            [indexer]
            [embeddings]
            [watcher]
            [storage]
            [outbox]
            entries_per_poll = 1001
        "#;
        let config = Config::from_toml_str(toml).expect("Failed to parse TOML");
        let result = config.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("entries_per_poll too large"));
    }

    #[test]
    fn test_outbox_validation_max_retries_negative() {
        let toml = r#"
            [indexer]
            [embeddings]
            [watcher]
            [storage]
            [outbox]
            max_retries = -1
        "#;
        let config = Config::from_toml_str(toml).expect("Failed to parse TOML");
        let result = config.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("max_retries must be non-negative"));
    }

    #[test]
    fn test_outbox_validation_max_embedding_dim_zero() {
        let toml = r#"
            [indexer]
            [embeddings]
            [watcher]
            [storage]
            [outbox]
            max_embedding_dim = 0
        "#;
        let config = Config::from_toml_str(toml).expect("Failed to parse TOML");
        let result = config.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("max_embedding_dim must be greater than 0"));
    }

    #[test]
    fn test_outbox_validation_max_cached_collections_zero() {
        let toml = r#"
            [indexer]
            [embeddings]
            [watcher]
            [storage]
            [outbox]
            max_cached_collections = 0
        "#;
        let config = Config::from_toml_str(toml).expect("Failed to parse TOML");
        let result = config.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("max_cached_collections must be greater than 0"));
    }

    #[test]
    fn test_outbox_validation_max_cached_collections_too_large() {
        let toml = r#"
            [indexer]
            [embeddings]
            [watcher]
            [storage]
            [outbox]
            max_cached_collections = 1001
        "#;
        let config = Config::from_toml_str(toml).expect("Failed to parse TOML");
        let result = config.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("max_cached_collections too large"));
    }

    #[test]
    fn test_outbox_validation_valid_config() {
        let toml = r#"
            [indexer]
            [embeddings]
            [watcher]
            [storage]
            [outbox]
            poll_interval_ms = 1000
            entries_per_poll = 100
            max_retries = 3
            max_embedding_dim = 100000
            max_cached_collections = 200
        "#;
        let config = Config::from_toml_str(toml).expect("Failed to parse TOML");
        let result = config.validate();
        assert!(result.is_ok());
    }

    #[test]
    fn test_outbox_validation_defaults() {
        let toml = r#"
            [indexer]
            [embeddings]
            [watcher]
            [storage]
        "#;
        let config = Config::from_toml_str(toml).expect("Failed to parse TOML");
        let result = config.validate();
        assert!(result.is_ok());
        assert_eq!(config.outbox.poll_interval_ms, 1000);
        assert_eq!(config.outbox.entries_per_poll, 500);
        assert_eq!(config.outbox.max_retries, 3);
        assert_eq!(config.outbox.max_embedding_dim, 100_000);
        assert_eq!(config.outbox.max_cached_collections, 200);
    }

    #[test]
    fn test_reranking_request_config_merge_override_all() {
        let base = RerankingConfig {
            enabled: false,
            provider: "jina".to_string(),
            model: "base-model".to_string(),
            candidates: 100,
            top_k: 10,
            api_base_url: Some("http://base.com".to_string()),
            api_key: Some("base-key".to_string()),
            timeout_secs: 30,
            max_concurrent_requests: 16,
        };

        let request = RerankingRequestConfig {
            enabled: Some(true),
            candidates: Some(350),
            top_k: Some(20),
        };

        let merged = request.merge_with(&base);

        assert!(merged.enabled);
        assert_eq!(merged.candidates, 350);
        assert_eq!(merged.top_k, 20);
        assert_eq!(merged.model, "base-model");
        assert_eq!(merged.api_base_url, Some("http://base.com".to_string()));
        assert_eq!(merged.api_key, Some("base-key".to_string()));
        assert_eq!(merged.timeout_secs, 30);
    }

    #[test]
    fn test_reranking_request_config_merge_partial_override() {
        let base = RerankingConfig {
            enabled: true,
            provider: "jina".to_string(),
            model: "base-model".to_string(),
            candidates: 100,
            top_k: 10,
            api_base_url: None,
            api_key: None,
            timeout_secs: 30,
            max_concurrent_requests: 16,
        };

        let request = RerankingRequestConfig {
            enabled: None,
            candidates: Some(200),
            top_k: None,
        };

        let merged = request.merge_with(&base);

        assert!(merged.enabled);
        assert_eq!(merged.candidates, 200);
        assert_eq!(merged.top_k, 10);
        assert_eq!(merged.model, "base-model");
    }

    #[test]
    fn test_reranking_request_config_merge_no_override() {
        let base = RerankingConfig {
            enabled: true,
            provider: "jina".to_string(),
            model: "base-model".to_string(),
            candidates: 100,
            top_k: 10,
            api_base_url: None,
            api_key: None,
            timeout_secs: 30,
            max_concurrent_requests: 16,
        };

        let request = RerankingRequestConfig {
            enabled: None,
            candidates: None,
            top_k: None,
        };

        let merged = request.merge_with(&base);

        assert!(merged.enabled);
        assert_eq!(merged.candidates, 100);
        assert_eq!(merged.top_k, 10);
        assert_eq!(merged.model, "base-model");
    }

    #[test]
    fn test_reranking_request_config_merge_enforces_1000_limit() {
        let base = RerankingConfig {
            enabled: true,
            provider: "jina".to_string(),
            model: "base-model".to_string(),
            candidates: 100,
            top_k: 10,
            api_base_url: None,
            api_key: None,
            timeout_secs: 30,
            max_concurrent_requests: 16,
        };

        let request = RerankingRequestConfig {
            enabled: None,
            candidates: Some(5000),
            top_k: None,
        };

        let merged = request.merge_with(&base);

        assert_eq!(merged.candidates, 1000);
    }

    #[test]
    fn test_reranking_request_config_merge_allows_1000() {
        let base = RerankingConfig {
            enabled: true,
            provider: "jina".to_string(),
            model: "base-model".to_string(),
            candidates: 100,
            top_k: 10,
            api_base_url: None,
            api_key: None,
            timeout_secs: 30,
            max_concurrent_requests: 16,
        };

        let request = RerankingRequestConfig {
            enabled: None,
            candidates: Some(1000),
            top_k: None,
        };

        let merged = request.merge_with(&base);

        assert_eq!(merged.candidates, 1000);
    }
}

/// Builder for Config with fluent API
#[derive(Debug, Clone)]
pub struct ConfigBuilder {
    indexer: IndexerConfig,
    embeddings: EmbeddingsConfig,
    watcher: WatcherConfig,
    storage: StorageConfig,
    server: ServerConfig,
    languages: LanguagesConfig,
    reranking: RerankingConfig,
    hybrid_search: HybridSearchConfig,
    outbox: OutboxConfig,
    query_preprocessing: QueryPreprocessingConfig,
    specificity: SpecificityConfig,
}

impl ConfigBuilder {
    /// Create a new ConfigBuilder with required storage config and defaults for other fields
    pub fn new(storage: StorageConfig) -> Self {
        Self {
            indexer: IndexerConfig::default(),
            embeddings: EmbeddingsConfig::default(),
            watcher: WatcherConfig::default(),
            storage,
            server: ServerConfig::default(),
            languages: LanguagesConfig::default(),
            reranking: RerankingConfig::default(),
            hybrid_search: HybridSearchConfig::default(),
            outbox: OutboxConfig::default(),
            query_preprocessing: QueryPreprocessingConfig::default(),
            specificity: SpecificityConfig::default(),
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

    /// Set the query preprocessing configuration
    pub fn query_preprocessing(mut self, query_preprocessing: QueryPreprocessingConfig) -> Self {
        self.query_preprocessing = query_preprocessing;
        self
    }

    /// Set the specificity boost configuration
    pub fn specificity(mut self, specificity: SpecificityConfig) -> Self {
        self.specificity = specificity;
        self
    }

    /// Build the Config
    pub fn build(self) -> Config {
        Config {
            indexer: self.indexer,
            embeddings: self.embeddings,
            watcher: self.watcher,
            storage: self.storage,
            server: self.server,
            languages: self.languages,
            reranking: self.reranking,
            hybrid_search: self.hybrid_search,
            outbox: self.outbox,
            query_preprocessing: self.query_preprocessing,
            specificity: self.specificity,
        }
    }
}
