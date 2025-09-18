use crate::error::{Error, Result};
use config::{Config as ConfigBuilder, Environment, File};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Indexer configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IndexerConfig {}

/// Main configuration structure for the codesearch system
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    /// Indexer configuration
    pub indexer: IndexerConfig,

    /// Embeddings configuration
    pub embeddings: EmbeddingsConfig,

    /// File watcher configuration
    pub watcher: WatcherConfig,

    /// Storage configuration
    pub storage: StorageConfig,

    /// Language configuration
    #[serde(default)]
    pub languages: LanguagesConfig,
}

/// Configuration for embeddings generation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingsConfig {
    /// Provider type: "local", "openai", "gemini"
    #[serde(default = "default_provider")]
    pub provider: String,

    /// Model name to use
    #[serde(default = "default_model")]
    pub model: String,

    /// Batch size for embedding generation
    #[serde(default = "default_batch_size")]
    pub batch_size: usize,

    /// Device to use: "cuda" or "cpu"
    #[serde(default = "default_device")]
    pub device: String,
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

    /// Branch strategy: "index_current", "index_all", "track_changes"
    #[serde(default = "default_branch_strategy")]
    pub branch_strategy: String,
}

/// Configuration for storage backend
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    /// Provider type: "qdrant" or "mock"
    #[serde(default = "default_storage_provider")]
    pub provider: String,

    /// Host for the storage backend
    #[serde(default = "default_storage_host")]
    pub host: String,

    /// Port for the storage backend
    #[serde(default = "default_storage_port")]
    pub port: u16,

    /// Optional API key for cloud services
    #[serde(default)]
    pub api_key: Option<String>,

    /// Collection name for storing entities
    #[serde(default = "default_collection_name")]
    pub collection_name: String,

    /// Vector size for embeddings (must match embedding model)
    #[serde(default = "default_vector_size")]
    pub vector_size: usize,

    /// Distance metric: "cosine", "euclidean", or "dot"
    #[serde(default = "default_distance_metric")]
    pub distance_metric: String,

    /// Batch size for bulk operations
    #[serde(default = "default_storage_batch_size")]
    pub batch_size: usize,

    /// Timeout in milliseconds for storage operations
    #[serde(default = "default_storage_timeout_ms")]
    pub timeout_ms: u64,

    /// Use mock storage for testing
    #[serde(default = "default_use_mock")]
    pub use_mock: bool,
}

/// Configuration for language support
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanguagesConfig {
    /// List of enabled languages
    #[serde(default = "default_enabled_languages")]
    pub enabled: Vec<String>,

    /// Python-specific configuration
    #[serde(default)]
    pub python: PythonConfig,

    /// JavaScript-specific configuration
    #[serde(default)]
    pub javascript: JavaScriptConfig,
}

/// Python language configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PythonConfig {
    /// Whether to preserve docstrings with functions
    #[serde(default = "default_true")]
    pub preserve_docstrings: bool,

    /// Whether to include type hints
    #[serde(default = "default_true")]
    pub include_type_hints: bool,
}

/// JavaScript/TypeScript language configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct JavaScriptConfig {
    /// Whether to preserve JSX components intact
    #[serde(default = "default_true")]
    pub preserve_jsx: bool,

    /// Whether to treat TypeScript files separately
    #[serde(default = "default_true")]
    pub typescript_enabled: bool,
}

fn default_enabled_languages() -> Vec<String> {
    vec![
        "rust".to_string(),
        // "python".to_string(),
        // "javascript".to_string(),
        // "typescript".to_string(),
        // "go".to_string(),
    ]
}

fn default_batch_size() -> usize {
    32
}
fn default_device() -> String {
    "cpu".to_string()
}

fn default_provider() -> String {
    "local".to_string()
}

fn default_model() -> String {
    "all-minilm-l6-v2".to_string()
}

fn default_branch_strategy() -> String {
    "index_current".to_string()
}

fn default_true() -> bool {
    true
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

fn default_storage_provider() -> String {
    "qdrant".to_string()
}

fn default_storage_host() -> String {
    "localhost".to_string()
}

fn default_storage_port() -> u16 {
    6334
}

fn default_collection_name() -> String {
    "codesearch".to_string()
}

fn default_vector_size() -> usize {
    768 // Default for all-minilm-l6-v2
}

fn default_distance_metric() -> String {
    "cosine".to_string()
}

fn default_storage_batch_size() -> usize {
    100
}

fn default_storage_timeout_ms() -> u64 {
    30000
}

fn default_use_mock() -> bool {
    false
}

impl Default for EmbeddingsConfig {
    fn default() -> Self {
        Self {
            provider: default_provider(),
            model: default_model(),
            batch_size: default_batch_size(),
            device: default_device(),
        }
    }
}

impl Default for WatcherConfig {
    fn default() -> Self {
        Self {
            debounce_ms: default_debounce_ms(),
            ignore_patterns: default_ignore_patterns(),
            branch_strategy: default_branch_strategy(),
        }
    }
}

impl Default for LanguagesConfig {
    fn default() -> Self {
        Self {
            enabled: default_enabled_languages(),
            python: PythonConfig::default(),
            javascript: JavaScriptConfig::default(),
        }
    }
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            provider: default_storage_provider(),
            host: default_storage_host(),
            port: default_storage_port(),
            api_key: None,
            collection_name: default_collection_name(),
            vector_size: default_vector_size(),
            distance_metric: default_distance_metric(),
            batch_size: default_storage_batch_size(),
            timeout_ms: default_storage_timeout_ms(),
            use_mock: default_use_mock(),
        }
    }
}

impl Config {
    /// Loads configuration from a TOML file with environment variable overrides
    ///
    /// Environment variables are prefixed with `CODESEARCH_` and use double underscores
    /// for nested values. For example:
    /// - `CODESEARCH_EMBEDDINGS__PROVIDER=openai`
    pub fn from_file(path: &Path) -> Result<Self> {
        let mut builder = ConfigBuilder::builder();

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

    /// Validates the configuration
    pub fn validate(&self) -> Result<()> {
        // Validate embeddings provider
        let valid_providers = ["local", "openai", "gemini"];
        if !valid_providers.contains(&self.embeddings.provider.as_str()) {
            return Err(Error::config(format!(
                "Invalid embeddings provider '{}'. Must be one of: {:?}",
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

        // Validate storage provider
        let valid_storage_providers = ["qdrant", "mock"];
        if !valid_storage_providers.contains(&self.storage.provider.as_str()) {
            return Err(Error::config(format!(
                "Invalid storage provider '{}'. Must be one of: {:?}",
                self.storage.provider, valid_storage_providers
            )));
        }

        // Validate vector size
        if self.storage.vector_size == 0 || self.storage.vector_size > 4096 {
            return Err(Error::config(format!(
                "Invalid vector size {}. Must be between 1 and 4096",
                self.storage.vector_size
            )));
        }

        // Validate distance metric
        let valid_metrics = ["cosine", "euclidean", "dot"];
        if !valid_metrics.contains(&self.storage.distance_metric.as_str()) {
            return Err(Error::config(format!(
                "Invalid distance metric '{}'. Must be one of: {:?}",
                self.storage.distance_metric, valid_metrics
            )));
        }

        // Validate batch size
        if self.storage.batch_size == 0 || self.storage.batch_size > 1000 {
            return Err(Error::config(format!(
                "Invalid batch size {}. Must be between 1 and 1000",
                self.storage.batch_size
            )));
        }

        // Validate port
        if self.storage.port == 0 {
            return Err(Error::config(
                "Invalid port: must be greater than 0".to_string(),
            ));
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
}
