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

/// Tracks which configuration sources were loaded
#[derive(Debug, Clone, Default)]
pub struct ConfigSources {
    /// Whether global config was loaded
    pub global_loaded: bool,
    /// Whether repo-local config was loaded
    pub local_loaded: bool,
    /// Path to global config (if loaded)
    pub global_path: Option<PathBuf>,
    /// Path to local config (if loaded)
    pub local_path: Option<PathBuf>,
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
}

/// Configuration for embeddings generation
#[derive(Clone, Serialize, Deserialize)]
pub struct EmbeddingsConfig {
    /// Provider type: "localapi", "api", "mock"
    #[serde(default = "default_provider")]
    pub provider: String,

    /// Model name to use
    #[serde(default = "default_model")]
    pub model: String,

    /// Number of text chunks sent in a single embedding API request
    #[serde(default = "default_texts_per_api_request")]
    pub texts_per_api_request: usize,

    /// Device to use: "cuda" or "cpu"
    #[serde(default = "default_device")]
    pub device: String,

    /// API base URL for LocalApi provider
    #[serde(default = "default_api_base_url")]
    pub api_base_url: Option<String>,

    /// API key for authentication
    pub api_key: Option<String>,

    /// Embedding dimension size
    #[serde(default = "default_embedding_dimension")]
    pub embedding_dimension: usize,

    /// Maximum concurrent embedding API requests
    #[serde(default = "default_max_concurrent_api_requests")]
    pub max_concurrent_api_requests: usize,

    /// Default instruction for BGE embedding models
    #[serde(default = "default_bge_instruction")]
    pub default_bge_instruction: String,
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

    /// Maximum entities allowed in a single Postgres batch operation (safety limit)
    #[serde(default = "default_max_entities_per_db_operation")]
    pub max_entities_per_db_operation: usize,
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
            .field(
                "max_entities_per_db_operation",
                &self.max_entities_per_db_operation,
            )
            .finish()
    }
}

/// Configuration for MCP server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    /// Port to listen on (host is always 127.0.0.1 for localhost-only access)
    #[serde(default = "default_server_port")]
    pub port: u16,
}

/// Configuration for language support
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanguagesConfig {
    /// List of enabled languages (currently only "rust" is supported)
    #[serde(default = "default_enabled_languages")]
    pub enabled: Vec<String>,
}

/// Configuration for reranking with cross-encoder models
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RerankingConfig {
    /// Whether reranking is enabled (default: false)
    #[serde(default = "default_enable_reranking")]
    pub enabled: bool,

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

    /// API key for reranker service (uses VLLM_API_KEY env if not set)
    pub api_key: Option<String>,

    /// Request timeout in seconds for reranking API calls (default: 30)
    #[serde(default = "default_reranking_timeout_secs")]
    pub timeout_secs: u64,
}

/// Hybrid search configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HybridSearchConfig {
    /// Whether hybrid search is enabled (default: true)
    #[serde(default = "default_enable_hybrid_search")]
    pub enabled: bool,

    /// Prefetch multiplier: retrieve N * limit candidates per method (default: 5)
    #[serde(default = "default_prefetch_multiplier")]
    pub prefetch_multiplier: usize,
}

// Default constants
const DEFAULT_DEVICE: &str = "cpu";
const DEFAULT_PROVIDER: &str = "localapi";
const DEFAULT_MODEL: &str = "BAAI/bge-code-v1";
const DEFAULT_API_BASE_URL: &str = "http://localhost:8000/v1";
const DEFAULT_BGE_INSTRUCTION: &str = "Represent this code search query for retrieving semantically similar code snippets, function implementations, type definitions, and code patterns";
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
    128
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
    1536
}

fn default_max_concurrent_api_requests() -> usize {
    64
}

fn default_bge_instruction() -> String {
    DEFAULT_BGE_INSTRUCTION.to_string()
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

fn default_entities_per_embedding_batch() -> usize {
    2000
}

pub fn default_max_entities_per_db_operation() -> usize {
    10000
}

fn default_server_port() -> u16 {
    3000
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

fn default_reranking_model() -> String {
    "BAAI/bge-reranker-v2-m3".to_string()
}

fn default_reranking_candidates() -> usize {
    100
}

fn default_reranking_top_k() -> usize {
    10
}

fn default_reranking_timeout_secs() -> u64 {
    5
}

fn default_enable_hybrid_search() -> bool {
    true
}

fn default_prefetch_multiplier() -> usize {
    5
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
            model: default_reranking_model(),
            candidates: default_reranking_candidates(),
            top_k: default_reranking_top_k(),
            api_base_url: None,
            api_key: None,
            timeout_secs: default_reranking_timeout_secs(),
        }
    }
}

impl Default for HybridSearchConfig {
    fn default() -> Self {
        Self {
            enabled: default_enable_hybrid_search(),
            prefetch_multiplier: default_prefetch_multiplier(),
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
}

impl Config {
    /// Loads configuration from a TOML file with environment variable overrides
    ///
    /// Environment variables are prefixed with `CODESEARCH_` and use double underscores
    /// for nested values. For example:
    /// - `CODESEARCH_EMBEDDINGS__PROVIDER=openai`
    pub fn from_file(path: &Path) -> Result<Self> {
        let mut builder = ConfigLib::builder();

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

    /// Merge another config into this one, preferring values from the other config
    ///
    /// This is used for layered configuration where repo-local settings override global settings.
    /// Only non-default values from `other` will override values in `self`.
    pub fn merge_from(&mut self, other: Self) {
        // Always take other storage settings if they differ from defaults
        self.storage.qdrant_host = other.storage.qdrant_host;
        self.storage.qdrant_port = other.storage.qdrant_port;
        self.storage.qdrant_rest_port = other.storage.qdrant_rest_port;
        self.storage.auto_start_deps = other.storage.auto_start_deps;
        self.storage.docker_compose_file = other.storage.docker_compose_file;
        self.storage.postgres_host = other.storage.postgres_host;
        self.storage.postgres_port = other.storage.postgres_port;
        self.storage.postgres_database = other.storage.postgres_database;
        self.storage.postgres_user = other.storage.postgres_user;
        self.storage.postgres_password = other.storage.postgres_password;
        self.storage.max_entities_per_db_operation = other.storage.max_entities_per_db_operation;

        // Merge embeddings config
        self.embeddings.provider = other.embeddings.provider;
        self.embeddings.model = other.embeddings.model;
        self.embeddings.texts_per_api_request = other.embeddings.texts_per_api_request;
        self.embeddings.device = other.embeddings.device;
        self.embeddings.api_base_url = other.embeddings.api_base_url;
        self.embeddings.api_key = other.embeddings.api_key.or(self.embeddings.api_key.clone());
        self.embeddings.embedding_dimension = other.embeddings.embedding_dimension;
        self.embeddings.max_concurrent_api_requests = other.embeddings.max_concurrent_api_requests;
        self.embeddings.default_bge_instruction = other.embeddings.default_bge_instruction;

        // Merge watcher config
        self.watcher.debounce_ms = other.watcher.debounce_ms;
        self.watcher.ignore_patterns = other.watcher.ignore_patterns;

        // Merge server config
        self.server.port = other.server.port;

        // Merge languages config
        self.languages = other.languages;

        // Merge reranking config
        self.reranking.enabled = other.reranking.enabled;
        self.reranking.model = other.reranking.model;
        self.reranking.candidates = other.reranking.candidates;
        self.reranking.top_k = other.reranking.top_k;
        self.reranking.api_base_url = other
            .reranking
            .api_base_url
            .or(self.reranking.api_base_url.clone());
        self.reranking.api_key = other.reranking.api_key.or(self.reranking.api_key.clone());
    }

    /// Load configuration with layered precedence (git-style)
    ///
    /// Precedence order (lowest to highest):
    /// 1. Hardcoded defaults
    /// 2. Global config (~/.codesearch/config.toml) - if exists
    /// 3. Repo-local config (./codesearch.toml or specified path) - if exists
    /// 4. Environment variables (CODESEARCH_*)
    ///
    /// Returns the merged config and metadata about which sources were loaded.
    pub fn load_layered(repo_local_path: Option<&Path>) -> Result<(Self, ConfigSources)> {
        let mut sources = ConfigSources::default();

        // Start with defaults
        let mut config = Config {
            indexer: IndexerConfig::default(),
            embeddings: EmbeddingsConfig::default(),
            watcher: WatcherConfig::default(),
            storage: StorageConfig {
                qdrant_host: default_qdrant_host(),
                qdrant_port: default_qdrant_port(),
                qdrant_rest_port: default_qdrant_rest_port(),
                auto_start_deps: default_auto_start_deps(),
                docker_compose_file: None,
                postgres_host: default_postgres_host(),
                postgres_port: default_postgres_port(),
                postgres_database: default_postgres_database(),
                postgres_user: default_postgres_user(),
                postgres_password: default_postgres_password(),
                max_entities_per_db_operation: default_max_entities_per_db_operation(),
            },
            server: ServerConfig::default(),
            languages: LanguagesConfig::default(),
            reranking: RerankingConfig::default(),
            hybrid_search: HybridSearchConfig::default(),
        };

        // Try to load global config
        if let Ok(global_path) = global_config_path() {
            if global_path.exists() {
                if let Ok(global_config) = Self::from_file(&global_path) {
                    config.merge_from(global_config);
                    sources.global_loaded = true;
                    sources.global_path = Some(global_path);
                }
            }
        }

        // Try to load repo-local config
        let local_path = repo_local_path.map(|p| p.to_path_buf()).unwrap_or_else(|| {
            std::env::current_dir()
                .unwrap_or_default()
                .join("codesearch.toml")
        });

        if local_path.exists() {
            if let Ok(local_config) = Self::from_file(&local_path) {
                config.merge_from(local_config);
                sources.local_loaded = true;
                sources.local_path = Some(local_path);
            }
        }

        Ok((config, sources))
    }

    /// Validates the configuration
    pub fn validate(&self) -> Result<()> {
        // Validate provider
        let valid_providers = ["localapi", "api", "mock"];
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
        assert_eq!(config.embeddings.provider, "localapi");
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
        }
    }
}
