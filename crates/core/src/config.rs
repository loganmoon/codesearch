use crate::error::{Error, Result};
use config::{Config as ConfigBuilder, Environment, File};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Indexer configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IndexerConfig {}

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

    /// Language configuration
    #[serde(default)]
    pub languages: LanguagesConfig,
}

/// Configuration for embeddings generation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingsConfig {
    /// Provider type: "localapi", "api", "mock"
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

    /// API base URL for LocalApi provider
    #[serde(default = "default_api_base_url")]
    pub api_base_url: Option<String>,

    /// API key for authentication
    pub api_key: Option<String>,

    /// Embedding dimension size
    #[serde(default = "default_embedding_dimension")]
    pub embedding_dimension: usize,
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
    /// Qdrant host address
    #[serde(default = "default_qdrant_host")]
    pub qdrant_host: String,

    /// Qdrant gRPC port
    #[serde(default = "default_qdrant_port")]
    pub qdrant_port: u16,

    /// Qdrant REST API port
    #[serde(default = "default_qdrant_rest_port")]
    pub qdrant_rest_port: u16,

    /// Collection name for storing entities
    pub collection_name: String,

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
    "localapi".to_string()
}

fn default_model() -> String {
    "BAAI/bge-code-v1".to_string()
}

fn default_api_base_url() -> Option<String> {
    Some("http://localhost:8000/v1".to_string())
}

fn default_embedding_dimension() -> usize {
    1536
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

fn default_qdrant_host() -> String {
    "localhost".to_string()
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
    "localhost".to_string()
}

fn default_postgres_port() -> u16 {
    5432
}

fn default_postgres_database() -> String {
    "codesearch".to_string()
}

fn default_postgres_user() -> String {
    "codesearch".to_string()
}

fn default_postgres_password() -> String {
    "codesearch".to_string()
}

impl Default for EmbeddingsConfig {
    fn default() -> Self {
        Self {
            provider: default_provider(),
            model: default_model(),
            batch_size: default_batch_size(),
            device: default_device(),
            api_base_url: default_api_base_url(),
            api_key: None,
            embedding_dimension: default_embedding_dimension(),
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

impl StorageConfig {
    /// Generate a collection name from a repository path
    ///
    /// Creates a unique, Qdrant-compatible collection name using the format:
    /// `<repo_name>_<xxhash3_128_of_full_path>`
    ///
    /// The repo name is truncated to 50 characters if needed.
    /// The name is deterministic - the same path always generates the same name.
    pub fn generate_collection_name(repo_path: &Path) -> String {
        use twox_hash::XxHash3_128;

        // Get the absolute path
        let absolute_path = repo_path
            .canonicalize()
            .unwrap_or_else(|_| repo_path.to_path_buf());

        // Extract repository name (last component of path)
        let repo_name = absolute_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("repo");

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

        // Hash the full absolute path
        let path_str = absolute_path.to_string_lossy();
        let hash = XxHash3_128::oneshot(path_str.as_bytes());

        // Format: <repo_name>_<hash>
        format!("{sanitized_name}_{hash:032x}")
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
        if let Ok(collection) = std::env::var("QDRANT_COLLECTION") {
            builder = builder
                .set_override("storage.collection_name", collection)
                .map_err(|e| Error::config(format!("Failed to set QDRANT_COLLECTION: {e}")))?;
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

    /// Create a new CodesearchConfigBuilder
    pub fn builder() -> CodesearchConfigBuilder {
        CodesearchConfigBuilder::new()
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
            collection_name = "test_collection"
            qdrant_host = "localhost"
            qdrant_port = 6334
        "#;

        let config = Config::from_toml_str(toml).expect("Failed to parse valid TOML");
        assert_eq!(config.embeddings.provider, "localapi");
        assert_eq!(config.embeddings.embedding_dimension, 768);
        assert_eq!(config.storage.collection_name, "test_collection");
    }

    #[test]
    fn test_from_toml_str_minimal() {
        let toml = r#"
            [indexer]

            [embeddings]

            [watcher]

            [storage]
            collection_name = "minimal_test"
        "#;

        let config = Config::from_toml_str(toml).expect("Failed to parse minimal TOML");
        // Check defaults are applied
        assert_eq!(config.embeddings.provider, "localapi");
        assert_eq!(config.embeddings.device, "cpu");
        assert_eq!(config.storage.collection_name, "minimal_test");
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
            collection_name = "test"
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
            collection_name = "test"
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
            collection_name = "test"
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
            collection_name = "test"
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
            collection_name = "roundtrip_test"
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
        assert_eq!(
            config.storage.collection_name,
            loaded_config.storage.collection_name
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
            collection_name = "test"
        "#;

        let temp_file = create_temp_config_file(toml).expect("Failed to create temp file");

        let config = Config::from_file(temp_file.path()).expect("Failed to load config from file");
        assert_eq!(config.embeddings.provider, "mock");
        assert_eq!(config.storage.collection_name, "test");
    }

    #[test]
    fn test_from_file_backward_compat_qdrant() {
        let toml = r#"
            [indexer]

            [embeddings]

            [watcher]

            [storage]
            collection_name = "test"
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
            collection_name = "test"
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
            collection_name = "save_test"
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
        let collection_name = StorageConfig::generate_collection_name(temp_dir.path());

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

        let collection_name = StorageConfig::generate_collection_name(&special_path);

        // Special characters should be replaced with underscores
        assert!(!collection_name.contains('('));
        assert!(!collection_name.contains(')'));
        assert!(!collection_name.contains('!'));
        assert!(!collection_name.contains(' '));
    }

    #[test]
    fn test_generate_collection_name_deterministic() {
        let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");

        let name1 = StorageConfig::generate_collection_name(temp_dir.path());
        let name2 = StorageConfig::generate_collection_name(temp_dir.path());

        // Same path should generate same name
        assert_eq!(name1, name2);
    }
}

/// Builder for Config with fluent API
#[derive(Debug, Clone)]
pub struct CodesearchConfigBuilder {
    indexer: IndexerConfig,
    embeddings: EmbeddingsConfig,
    watcher: WatcherConfig,
    storage: Option<StorageConfig>,
    languages: LanguagesConfig,
}

impl CodesearchConfigBuilder {
    /// Create a new CodesearchConfigBuilder with defaults
    pub fn new() -> Self {
        Self {
            indexer: IndexerConfig::default(),
            embeddings: EmbeddingsConfig::default(),
            watcher: WatcherConfig::default(),
            storage: None,
            languages: LanguagesConfig::default(),
        }
    }

    /// Set the storage configuration
    pub fn storage(mut self, storage: StorageConfig) -> Self {
        self.storage = Some(storage);
        self
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

    /// Build the Config
    pub fn build(self) -> Config {
        Config {
            indexer: self.indexer,
            embeddings: self.embeddings,
            watcher: self.watcher,
            storage: self.storage.expect("Storage config is required"),
            languages: self.languages,
        }
    }
}

impl Default for CodesearchConfigBuilder {
    fn default() -> Self {
        Self::new()
    }
}
