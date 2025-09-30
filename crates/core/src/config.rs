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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_collection_name_generation_basic() {
        let path = Path::new("/home/user/projects/myrepo");
        let name = StorageConfig::generate_collection_name(path);

        // Should have format: <repo_name>_<hash>
        assert!(name.starts_with("myrepo_"));

        // Should be deterministic
        let name2 = StorageConfig::generate_collection_name(path);
        assert_eq!(name, name2);

        // Should only contain valid characters
        assert!(name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'));

        // Hash part should be 32 hex chars (128 bits / 4 bits per hex char)
        let parts: Vec<&str> = name.splitn(2, '_').collect();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[1].len(), 32);
        assert!(parts[1].chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_collection_name_special_characters() {
        let path = Path::new("/home/user-name/my project/repo@v1.0");
        let name = StorageConfig::generate_collection_name(path);

        // Should sanitize special chars in repo name
        assert!(name.starts_with("repo_v1_0_"));

        // Should only contain valid characters
        assert!(name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'));

        // Should be deterministic
        let name2 = StorageConfig::generate_collection_name(path);
        assert_eq!(name, name2);
    }

    #[test]
    fn test_collection_name_long_repo_name() {
        // Create a path with very long repo name
        let long_name = "a".repeat(100);
        let path_str = format!("/home/user/{long_name}");
        let path = Path::new(&path_str);
        let name = StorageConfig::generate_collection_name(path);

        // Repo name should be truncated to 50 chars
        let parts: Vec<&str> = name.splitn(2, '_').collect();
        assert_eq!(parts[0].len(), 50);
        assert!(parts[0].chars().all(|c| c == 'a'));

        // Should be deterministic
        let name2 = StorageConfig::generate_collection_name(path);
        assert_eq!(name, name2);
    }

    #[test]
    fn test_collection_name_windows_path() {
        let path = Path::new("C:\\Users\\Developer\\Projects\\MyRepo");
        let name = StorageConfig::generate_collection_name(path);

        // On non-Windows systems, this path won't canonicalize properly,
        // but should still extract "MyRepo" as the last component
        // On Windows, it should work correctly
        if cfg!(windows) {
            assert!(name.starts_with("MyRepo_"));
        } else {
            // On Linux/Mac, the whole path becomes the filename
            // Just verify format and determinism
            assert!(name.contains('_'));
        }

        // Should not contain path separators
        assert!(!name.contains('\\'));
        assert!(!name.contains('/'));
        assert!(!name.contains(':'));

        // Should only contain valid characters
        assert!(name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'));

        // Should be deterministic
        let name2 = StorageConfig::generate_collection_name(path);
        assert_eq!(name, name2);
    }

    #[test]
    fn test_collection_name_relative_path() {
        let path = Path::new("./myrepo");
        let name = StorageConfig::generate_collection_name(path);

        // Should work with relative paths (will be canonicalized)
        // Note: actual repo name depends on where test runs
        assert!(name.contains('_'));

        // Should be deterministic
        let name2 = StorageConfig::generate_collection_name(path);
        assert_eq!(name, name2);
    }

    #[test]
    fn test_collection_name_dashes_underscores() {
        let path = Path::new("/home/user/my-awesome_repo");
        let name = StorageConfig::generate_collection_name(path);

        // Should preserve dashes and underscores in repo name
        assert!(name.starts_with("my-awesome_repo_"));

        // Should be deterministic
        let name2 = StorageConfig::generate_collection_name(path);
        assert_eq!(name, name2);
    }

    #[test]
    fn test_config_builder_basic() {
        let storage = StorageConfig {
            qdrant_host: "localhost".to_string(),
            qdrant_port: 6334,
            qdrant_rest_port: 6333,
            collection_name: "test_collection".to_string(),
            auto_start_deps: true,
            docker_compose_file: None,
        };

        let config = Config::builder().storage(storage).build();

        assert_eq!(config.storage.collection_name, "test_collection");
        assert_eq!(config.embeddings.provider, "localapi");
        assert_eq!(config.embeddings.model, "BAAI/bge-code-v1");
    }

    #[test]
    fn test_config_builder_storage_settings() {
        let storage = StorageConfig {
            qdrant_host: "192.168.1.1".to_string(),
            qdrant_port: 6335,
            qdrant_rest_port: 6333,
            collection_name: "my_collection".to_string(),
            auto_start_deps: false,
            docker_compose_file: None,
        };

        let config = Config::builder().storage(storage).build();

        assert_eq!(config.storage.collection_name, "my_collection");
        assert_eq!(config.storage.qdrant_host, "192.168.1.1");
        assert_eq!(config.storage.qdrant_port, 6335);
        assert!(!config.storage.auto_start_deps);
    }

    #[test]
    fn test_config_builder_embeddings_settings() {
        let storage = StorageConfig {
            qdrant_host: "localhost".to_string(),
            qdrant_port: 6334,
            qdrant_rest_port: 6333,
            collection_name: "test".to_string(),
            auto_start_deps: true,
            docker_compose_file: None,
        };

        let embeddings = EmbeddingsConfig {
            provider: "openai".to_string(),
            model: "text-embedding-ada-002".to_string(),
            batch_size: 64,
            device: "cuda".to_string(),
            api_base_url: Some("http://localhost:8000".to_string()),
            api_key: None,
            embedding_dimension: 768,
        };

        let config = Config::builder()
            .storage(storage)
            .embeddings(embeddings)
            .build();

        assert_eq!(config.embeddings.provider, "openai");
        assert_eq!(config.embeddings.model, "text-embedding-ada-002");
        assert_eq!(config.embeddings.batch_size, 64);
        assert_eq!(config.embeddings.device, "cuda");
    }

    #[test]
    fn test_config_builder_watcher_settings() {
        let storage = StorageConfig {
            qdrant_host: "localhost".to_string(),
            qdrant_port: 6334,
            qdrant_rest_port: 6333,
            collection_name: "test".to_string(),
            auto_start_deps: true,
            docker_compose_file: None,
        };

        let watcher = WatcherConfig {
            debounce_ms: 1000,
            ignore_patterns: vec!["*.tmp".to_string(), "build/".to_string()],
            branch_strategy: "index_current".to_string(),
        };

        let config = Config::builder().storage(storage).watcher(watcher).build();

        assert_eq!(config.watcher.debounce_ms, 1000);
        assert_eq!(config.watcher.ignore_patterns, vec!["*.tmp", "build/"]);
    }

    #[test]
    fn test_config_builder_language_settings() {
        let storage = StorageConfig {
            qdrant_host: "localhost".to_string(),
            qdrant_port: 6334,
            qdrant_rest_port: 6333,
            collection_name: "test".to_string(),
            auto_start_deps: true,
            docker_compose_file: None,
        };

        let languages = LanguagesConfig {
            enabled: vec!["rust".to_string(), "python".to_string()],
            python: PythonConfig::default(),
            javascript: JavaScriptConfig::default(),
        };

        let config = Config::builder()
            .storage(storage)
            .languages(languages)
            .build();

        assert_eq!(config.languages.enabled, vec!["rust", "python"]);
    }

    #[test]
    fn test_config_builder_complete_config() {
        let storage = StorageConfig {
            qdrant_host: "custom-host".to_string(),
            qdrant_port: 7000,
            qdrant_rest_port: 7001,
            collection_name: "custom_collection".to_string(),
            auto_start_deps: false,
            docker_compose_file: Some("custom-compose.yml".to_string()),
        };

        let embeddings = EmbeddingsConfig {
            provider: "gemini".to_string(),
            model: "embedding-001".to_string(),
            batch_size: 128,
            device: "metal".to_string(),
            api_base_url: Some("http://localhost:8000".to_string()),
            api_key: None,
            embedding_dimension: 768,
        };

        let config = Config::builder()
            .storage(storage.clone())
            .embeddings(embeddings.clone())
            .build();

        assert_eq!(config.storage.qdrant_host, "custom-host");
        assert_eq!(config.storage.collection_name, "custom_collection");
        assert_eq!(config.embeddings.provider, "gemini");
        assert_eq!(config.embeddings.model, "embedding-001");
    }
}
