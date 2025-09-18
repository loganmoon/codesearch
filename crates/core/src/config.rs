use crate::error::{Error, Result};
use config::{Config as ExternalConfigBuilder, Environment, File};
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
    #[serde(default = "EmbeddingsConfig::default_provider")]
    pub provider: String,

    /// Model name to use
    #[serde(default = "EmbeddingsConfig::default_model")]
    pub model: String,

    /// Batch size for embedding generation
    #[serde(default = "EmbeddingsConfig::default_batch_size")]
    pub batch_size: usize,

    /// Device to use: "cuda" or "cpu"
    #[serde(default = "EmbeddingsConfig::default_device")]
    pub device: String,
}

impl EmbeddingsConfig {
    fn default_provider() -> String {
        "local".to_string()
    }

    fn default_model() -> String {
        "all-minilm-l6-v2".to_string()
    }

    fn default_batch_size() -> usize {
        32
    }

    fn default_device() -> String {
        "cpu".to_string()
    }
}

impl Default for EmbeddingsConfig {
    fn default() -> Self {
        Self {
            provider: Self::default_provider(),
            model: Self::default_model(),
            batch_size: Self::default_batch_size(),
            device: Self::default_device(),
        }
    }
}

/// Builder for EmbeddingsConfig
pub struct EmbeddingsConfigBuilder {
    provider: String,
    model: String,
    batch_size: usize,
    device: String,
}

impl Default for EmbeddingsConfigBuilder {
    fn default() -> Self {
        Self {
            provider: EmbeddingsConfig::default_provider(),
            model: EmbeddingsConfig::default_model(),
            batch_size: EmbeddingsConfig::default_batch_size(),
            device: EmbeddingsConfig::default_device(),
        }
    }
}

impl EmbeddingsConfigBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn provider(mut self, provider: impl Into<String>) -> Self {
        self.provider = provider.into();
        self
    }

    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.model = model.into();
        self
    }

    pub fn batch_size(mut self, batch_size: usize) -> Self {
        self.batch_size = batch_size;
        self
    }

    pub fn device(mut self, device: impl Into<String>) -> Self {
        self.device = device.into();
        self
    }

    pub fn build(self) -> EmbeddingsConfig {
        EmbeddingsConfig {
            provider: self.provider,
            model: self.model,
            batch_size: self.batch_size,
            device: self.device,
        }
    }
}

/// Configuration for file watching
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatcherConfig {
    /// Debounce time in milliseconds
    #[serde(default = "WatcherConfig::default_debounce_ms")]
    pub debounce_ms: u64,

    /// Patterns to ignore
    #[serde(default = "WatcherConfig::default_ignore_patterns")]
    pub ignore_patterns: Vec<String>,

    /// Branch strategy: "index_current", "index_all", "track_changes"
    #[serde(default = "WatcherConfig::default_branch_strategy")]
    pub branch_strategy: String,
}

impl WatcherConfig {
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

    fn default_branch_strategy() -> String {
        "index_current".to_string()
    }
}

impl Default for WatcherConfig {
    fn default() -> Self {
        Self {
            debounce_ms: Self::default_debounce_ms(),
            ignore_patterns: Self::default_ignore_patterns(),
            branch_strategy: Self::default_branch_strategy(),
        }
    }
}

/// Builder for WatcherConfig
pub struct WatcherConfigBuilder {
    debounce_ms: u64,
    ignore_patterns: Vec<String>,
    branch_strategy: String,
}

impl Default for WatcherConfigBuilder {
    fn default() -> Self {
        Self {
            debounce_ms: WatcherConfig::default_debounce_ms(),
            ignore_patterns: WatcherConfig::default_ignore_patterns(),
            branch_strategy: WatcherConfig::default_branch_strategy(),
        }
    }
}

impl WatcherConfigBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn debounce_ms(mut self, ms: u64) -> Self {
        self.debounce_ms = ms;
        self
    }

    pub fn ignore_patterns(mut self, patterns: Vec<String>) -> Self {
        self.ignore_patterns = patterns;
        self
    }

    pub fn add_ignore_pattern(mut self, pattern: impl Into<String>) -> Self {
        self.ignore_patterns.push(pattern.into());
        self
    }

    pub fn branch_strategy(mut self, strategy: impl Into<String>) -> Self {
        self.branch_strategy = strategy.into();
        self
    }

    pub fn build(self) -> WatcherConfig {
        WatcherConfig {
            debounce_ms: self.debounce_ms,
            ignore_patterns: self.ignore_patterns,
            branch_strategy: self.branch_strategy,
        }
    }
}

/// Configuration for storage backend
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    /// Provider type: "qdrant" or "mock"
    #[serde(default = "StorageConfig::default_provider")]
    pub provider: String,

    /// Host for the storage backend
    #[serde(default = "StorageConfig::default_host")]
    pub host: String,

    /// Port for the storage backend
    #[serde(default = "StorageConfig::default_port")]
    pub port: u16,

    /// Optional API key for cloud services
    #[serde(default)]
    pub api_key: Option<String>,

    /// Collection name for storing entities
    #[serde(default = "StorageConfig::default_collection_name")]
    pub collection_name: String,

    /// Vector size for embeddings (must match embedding model)
    #[serde(default = "StorageConfig::default_vector_size")]
    pub vector_size: usize,

    /// Distance metric: "cosine", "euclidean", or "dot"
    #[serde(default = "StorageConfig::default_distance_metric")]
    pub distance_metric: String,

    /// Batch size for bulk operations
    #[serde(default = "StorageConfig::default_batch_size")]
    pub batch_size: usize,

    /// Timeout in milliseconds for storage operations
    #[serde(default = "StorageConfig::default_timeout_ms")]
    pub timeout_ms: u64,
}

impl StorageConfig {
    fn default_provider() -> String {
        "qdrant".to_string()
    }

    fn default_host() -> String {
        "localhost".to_string()
    }

    fn default_port() -> u16 {
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

    fn default_batch_size() -> usize {
        100
    }

    fn default_timeout_ms() -> u64 {
        30000
    }
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            provider: Self::default_provider(),
            host: Self::default_host(),
            port: Self::default_port(),
            api_key: None,
            collection_name: Self::default_collection_name(),
            vector_size: Self::default_vector_size(),
            distance_metric: Self::default_distance_metric(),
            batch_size: Self::default_batch_size(),
            timeout_ms: Self::default_timeout_ms(),
        }
    }
}

/// Builder for StorageConfig
pub struct StorageConfigBuilder {
    provider: String,
    host: String,
    port: u16,
    api_key: Option<String>,
    collection_name: String,
    vector_size: usize,
    distance_metric: String,
    batch_size: usize,
    timeout_ms: u64,
}

impl Default for StorageConfigBuilder {
    fn default() -> Self {
        Self {
            provider: StorageConfig::default_provider(),
            host: StorageConfig::default_host(),
            port: StorageConfig::default_port(),
            api_key: None,
            collection_name: StorageConfig::default_collection_name(),
            vector_size: StorageConfig::default_vector_size(),
            distance_metric: StorageConfig::default_distance_metric(),
            batch_size: StorageConfig::default_batch_size(),
            timeout_ms: StorageConfig::default_timeout_ms(),
        }
    }
}

impl StorageConfigBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn provider(mut self, provider: impl Into<String>) -> Self {
        self.provider = provider.into();
        self
    }

    pub fn host(mut self, host: impl Into<String>) -> Self {
        self.host = host.into();
        self
    }

    pub fn port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    pub fn api_key(mut self, key: impl Into<String>) -> Self {
        self.api_key = Some(key.into());
        self
    }

    pub fn collection_name(mut self, name: impl Into<String>) -> Self {
        self.collection_name = name.into();
        self
    }

    pub fn vector_size(mut self, size: usize) -> Self {
        self.vector_size = size;
        self
    }

    pub fn distance_metric(mut self, metric: impl Into<String>) -> Self {
        self.distance_metric = metric.into();
        self
    }

    pub fn batch_size(mut self, size: usize) -> Self {
        self.batch_size = size;
        self
    }

    pub fn timeout_ms(mut self, ms: u64) -> Self {
        self.timeout_ms = ms;
        self
    }

    pub fn build(self) -> StorageConfig {
        StorageConfig {
            provider: self.provider,
            host: self.host,
            port: self.port,
            api_key: self.api_key,
            collection_name: self.collection_name,
            vector_size: self.vector_size,
            distance_metric: self.distance_metric,
            batch_size: self.batch_size,
            timeout_ms: self.timeout_ms,
        }
    }
}

/// Configuration for language support
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanguagesConfig {
    /// List of enabled languages
    #[serde(default = "LanguagesConfig::default_enabled")]
    pub enabled: Vec<String>,

    /// Python-specific configuration
    #[serde(default)]
    pub python: PythonConfig,

    /// JavaScript-specific configuration
    #[serde(default)]
    pub javascript: JavaScriptConfig,
}

impl LanguagesConfig {
    fn default_enabled() -> Vec<String> {
        vec!["rust".to_string()]
    }
}

impl Default for LanguagesConfig {
    fn default() -> Self {
        Self {
            enabled: Self::default_enabled(),
            python: PythonConfig::default(),
            javascript: JavaScriptConfig::default(),
        }
    }
}

/// Python language configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PythonConfig {
    /// Whether to preserve docstrings with functions
    #[serde(default = "PythonConfig::default_preserve_docstrings")]
    pub preserve_docstrings: bool,

    /// Whether to include type hints
    #[serde(default = "PythonConfig::default_include_type_hints")]
    pub include_type_hints: bool,
}

impl PythonConfig {
    fn default_preserve_docstrings() -> bool {
        true
    }

    fn default_include_type_hints() -> bool {
        true
    }
}

impl Default for PythonConfig {
    fn default() -> Self {
        Self {
            preserve_docstrings: Self::default_preserve_docstrings(),
            include_type_hints: Self::default_include_type_hints(),
        }
    }
}

/// JavaScript/TypeScript language configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JavaScriptConfig {
    /// Whether to preserve JSX components intact
    #[serde(default = "JavaScriptConfig::default_preserve_jsx")]
    pub preserve_jsx: bool,

    /// Whether to treat TypeScript files separately
    #[serde(default = "JavaScriptConfig::default_typescript_enabled")]
    pub typescript_enabled: bool,
}

impl JavaScriptConfig {
    fn default_preserve_jsx() -> bool {
        true
    }

    fn default_typescript_enabled() -> bool {
        true
    }
}

impl Default for JavaScriptConfig {
    fn default() -> Self {
        Self {
            preserve_jsx: Self::default_preserve_jsx(),
            typescript_enabled: Self::default_typescript_enabled(),
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
        let mut builder = ExternalConfigBuilder::builder();

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

/// Builder for Config
pub struct ConfigBuilder {
    indexer: IndexerConfig,
    embeddings: EmbeddingsConfig,
    watcher: WatcherConfig,
    storage: StorageConfig,
    languages: LanguagesConfig,
}

impl Default for ConfigBuilder {
    fn default() -> Self {
        Self {
            indexer: IndexerConfig::default(),
            embeddings: EmbeddingsConfig::default(),
            watcher: WatcherConfig::default(),
            storage: StorageConfig::default(),
            languages: LanguagesConfig::default(),
        }
    }
}

impl ConfigBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn indexer(mut self, indexer: IndexerConfig) -> Self {
        self.indexer = indexer;
        self
    }

    pub fn embeddings(mut self, embeddings: EmbeddingsConfig) -> Self {
        self.embeddings = embeddings;
        self
    }

    pub fn watcher(mut self, watcher: WatcherConfig) -> Self {
        self.watcher = watcher;
        self
    }

    pub fn storage(mut self, storage: StorageConfig) -> Self {
        self.storage = storage;
        self
    }

    pub fn languages(mut self, languages: LanguagesConfig) -> Self {
        self.languages = languages;
        self
    }

    pub fn build(self) -> Config {
        Config {
            indexer: self.indexer,
            embeddings: self.embeddings,
            watcher: self.watcher,
            storage: self.storage,
            languages: self.languages,
        }
    }
}
