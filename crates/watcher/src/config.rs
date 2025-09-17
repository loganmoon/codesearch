//! Configuration types for the file watcher
//!
//! This module provides immutable configuration structures for controlling
//! file watching behavior, debouncing, and Git integration.

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Immutable configuration for the file watcher
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatcherConfig {
    /// Debounce window in milliseconds (default: 500ms)
    pub debounce_ms: u64,
    /// Patterns to ignore (glob patterns)
    pub ignore_patterns: Vec<String>,
    /// Git branch handling strategy
    pub branch_strategy: BranchStrategy,
    /// Maximum file size to watch in bytes (default: 10MB)
    pub max_file_size: u64,
    /// Whether to follow symbolic links (default: false)
    pub follow_symlinks: bool,
    /// Maximum recursion depth for directory watching (default: 50)
    pub recursive_depth: u32,
    /// Maximum number of events in queue (default: 10000)
    pub max_queue_size: usize,
    /// Batch size for processing events (default: 100)
    pub batch_size: usize,
    /// Batch timeout in milliseconds (default: 1000ms)
    pub batch_timeout_ms: u64,
    /// Enable performance monitoring (default: false)
    pub enable_metrics: bool,
}

impl WatcherConfig {
    /// Create a new configuration with custom values
    pub fn new() -> Self {
        Self::default()
    }

    /// Create configuration from builder
    pub fn builder() -> WatcherConfigBuilder {
        WatcherConfigBuilder::default()
    }

    /// Get the debounce duration
    pub fn debounce_duration(&self) -> Duration {
        Duration::from_millis(self.debounce_ms)
    }

    /// Get the batch timeout duration
    pub fn batch_timeout_duration(&self) -> Duration {
        Duration::from_millis(self.batch_timeout_ms)
    }

    /// Check if a file size exceeds the limit
    pub fn exceeds_size_limit(&self, size: u64) -> bool {
        size > self.max_file_size
    }

    /// Get default ignore patterns for common files
    pub fn default_ignore_patterns() -> Vec<String> {
        vec![
            "*.log".to_string(),
            "*.tmp".to_string(),
            "*.swp".to_string(),
            "*.swo".to_string(),
            "*~".to_string(),
            ".DS_Store".to_string(),
            "Thumbs.db".to_string(),
            "node_modules/**".to_string(),
            "target/**".to_string(),
            ".git/**".to_string(),
            ".svn/**".to_string(),
            ".hg/**".to_string(),
            "__pycache__/**".to_string(),
            "*.pyc".to_string(),
            ".pytest_cache/**".to_string(),
            ".coverage".to_string(),
            "*.egg-info/**".to_string(),
            "dist/**".to_string(),
            "build/**".to_string(),
            ".idea/**".to_string(),
            ".vscode/**".to_string(),
            "*.iml".to_string(),
            ".classpath".to_string(),
            ".project".to_string(),
        ]
    }
}

impl Default for WatcherConfig {
    fn default() -> Self {
        Self {
            debounce_ms: 500,
            ignore_patterns: Self::default_ignore_patterns(),
            branch_strategy: BranchStrategy::IndexCurrent,
            max_file_size: 10 * 1024 * 1024, // 10MB
            follow_symlinks: false,
            recursive_depth: 50,
            max_queue_size: 10000,
            batch_size: 100,
            batch_timeout_ms: 1000,
            enable_metrics: false,
        }
    }
}

/// Builder for WatcherConfig
#[derive(Debug, Default)]
pub struct WatcherConfigBuilder {
    config: WatcherConfig,
}

impl WatcherConfigBuilder {
    /// Set debounce window in milliseconds
    pub fn debounce_ms(mut self, ms: u64) -> Self {
        self.config.debounce_ms = ms;
        self
    }

    /// Set ignore patterns
    pub fn ignore_patterns(mut self, patterns: Vec<String>) -> Self {
        self.config.ignore_patterns = patterns;
        self
    }

    /// Add an ignore pattern
    pub fn add_ignore_pattern(mut self, pattern: String) -> Self {
        self.config.ignore_patterns.push(pattern);
        self
    }

    /// Set branch strategy
    pub fn branch_strategy(mut self, strategy: BranchStrategy) -> Self {
        self.config.branch_strategy = strategy;
        self
    }

    /// Set maximum file size
    pub fn max_file_size(mut self, size: u64) -> Self {
        self.config.max_file_size = size;
        self
    }

    /// Set whether to follow symlinks
    pub fn follow_symlinks(mut self, follow: bool) -> Self {
        self.config.follow_symlinks = follow;
        self
    }

    /// Set recursive depth
    pub fn recursive_depth(mut self, depth: u32) -> Self {
        self.config.recursive_depth = depth;
        self
    }

    /// Set maximum queue size
    pub fn max_queue_size(mut self, size: usize) -> Self {
        self.config.max_queue_size = size;
        self
    }

    /// Set batch size
    pub fn batch_size(mut self, size: usize) -> Self {
        self.config.batch_size = size;
        self
    }

    /// Set batch timeout in milliseconds
    pub fn batch_timeout_ms(mut self, ms: u64) -> Self {
        self.config.batch_timeout_ms = ms;
        self
    }

    /// Enable metrics collection
    pub fn enable_metrics(mut self, enable: bool) -> Self {
        self.config.enable_metrics = enable;
        self
    }

    /// Build the configuration
    pub fn build(self) -> WatcherConfig {
        self.config
    }
}

/// Strategy for handling Git branches
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BranchStrategy {
    /// Index only files in the current branch
    IndexCurrent,
    /// Index all branches
    IndexAll,
    /// Index specific branches by pattern
    IndexPattern,
    /// Disable Git integration
    Disabled,
}

impl BranchStrategy {
    /// Check if Git integration is enabled
    pub fn is_enabled(&self) -> bool {
        !matches!(self, Self::Disabled)
    }

    /// Check if we should index the current branch
    pub fn should_index_current(&self) -> bool {
        matches!(self, Self::IndexCurrent | Self::IndexAll)
    }
}

/// Configuration for file filtering
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilterConfig {
    /// File extensions to include (empty means all)
    pub include_extensions: Vec<String>,
    /// File extensions to exclude
    pub exclude_extensions: Vec<String>,
    /// Minimum file size in bytes
    pub min_file_size: u64,
    /// Maximum file size in bytes
    pub max_file_size: u64,
    /// Include hidden files (starting with .)
    pub include_hidden: bool,
    /// Include binary files
    pub include_binary: bool,
}

impl FilterConfig {
    /// Check if a file extension should be included
    pub fn should_include_extension(&self, ext: &str) -> bool {
        // If include list is empty, include all except excluded
        if self.include_extensions.is_empty() {
            !self.exclude_extensions.iter().any(|e| e == ext)
        } else {
            // Include only if in include list and not in exclude list
            self.include_extensions.iter().any(|e| e == ext)
                && !self.exclude_extensions.iter().any(|e| e == ext)
        }
    }

    /// Check if a file size is within limits
    pub fn is_size_valid(&self, size: u64) -> bool {
        size >= self.min_file_size && size <= self.max_file_size
    }

    /// Check if a hidden file should be included
    pub fn should_include_hidden_file(&self, filename: &str) -> bool {
        !filename.starts_with('.') || self.include_hidden
    }
}

impl Default for FilterConfig {
    fn default() -> Self {
        Self {
            include_extensions: vec![
                "rs".to_string(),
                "py".to_string(),
                "js".to_string(),
                "jsx".to_string(),
                "ts".to_string(),
                "tsx".to_string(),
                "go".to_string(),
                "java".to_string(),
                "c".to_string(),
                "cpp".to_string(),
                "cc".to_string(),
                "h".to_string(),
                "hpp".to_string(),
                "cs".to_string(),
                "rb".to_string(),
                "php".to_string(),
                "swift".to_string(),
                "kt".to_string(),
                "scala".to_string(),
                "r".to_string(),
                "m".to_string(),
                "mm".to_string(),
                "lua".to_string(),
                "pl".to_string(),
                "sh".to_string(),
                "bash".to_string(),
                "zsh".to_string(),
                "fish".to_string(),
                "vim".to_string(),
                "el".to_string(),
                "clj".to_string(),
                "cljs".to_string(),
                "ex".to_string(),
                "exs".to_string(),
                "erl".to_string(),
                "hrl".to_string(),
                "ml".to_string(),
                "mli".to_string(),
                "fs".to_string(),
                "fsx".to_string(),
                "hs".to_string(),
                "lhs".to_string(),
                "jl".to_string(),
                "nim".to_string(),
                "nims".to_string(),
                "cr".to_string(),
                "dart".to_string(),
                "zig".to_string(),
                "v".to_string(),
                "sql".to_string(),
                "md".to_string(),
                "markdown".to_string(),
                "rst".to_string(),
                "adoc".to_string(),
                "tex".to_string(),
                "json".to_string(),
                "yaml".to_string(),
                "yml".to_string(),
                "toml".to_string(),
                "xml".to_string(),
                "html".to_string(),
                "htm".to_string(),
                "css".to_string(),
                "scss".to_string(),
                "sass".to_string(),
                "less".to_string(),
            ],
            exclude_extensions: vec![
                "exe".to_string(),
                "dll".to_string(),
                "so".to_string(),
                "dylib".to_string(),
                "a".to_string(),
                "o".to_string(),
                "obj".to_string(),
                "lib".to_string(),
                "pdb".to_string(),
                "pdf".to_string(),
                "zip".to_string(),
                "tar".to_string(),
                "gz".to_string(),
                "bz2".to_string(),
                "xz".to_string(),
                "7z".to_string(),
                "rar".to_string(),
                "jpg".to_string(),
                "jpeg".to_string(),
                "png".to_string(),
                "gif".to_string(),
                "bmp".to_string(),
                "ico".to_string(),
                "svg".to_string(),
                "mp3".to_string(),
                "mp4".to_string(),
                "avi".to_string(),
                "mov".to_string(),
                "wmv".to_string(),
                "flv".to_string(),
                "mkv".to_string(),
                "webm".to_string(),
                "wav".to_string(),
                "flac".to_string(),
                "ogg".to_string(),
                "doc".to_string(),
                "docx".to_string(),
                "xls".to_string(),
                "xlsx".to_string(),
                "ppt".to_string(),
                "pptx".to_string(),
            ],
            min_file_size: 0,
            max_file_size: 10 * 1024 * 1024, // 10MB
            include_hidden: false,
            include_binary: false,
        }
    }
}

/// Metrics configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsConfig {
    /// Enable event processing metrics
    pub track_events: bool,
    /// Enable memory usage tracking
    pub track_memory: bool,
    /// Enable Git operation metrics
    pub track_git_ops: bool,
    /// Metrics reporting interval in seconds
    pub report_interval_secs: u64,
}

impl Default for MetricsConfig {
    fn default() -> Self {
        Self {
            track_events: true,
            track_memory: true,
            track_git_ops: true,
            report_interval_secs: 60,
        }
    }
}

/// Recovery configuration for error handling
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryConfig {
    /// Maximum retry attempts for watcher initialization
    pub max_init_retries: u32,
    /// Delay between retry attempts in milliseconds
    pub retry_delay_ms: u64,
    /// Enable fallback to polling mode
    pub enable_polling_fallback: bool,
    /// Polling interval in milliseconds
    pub polling_interval_ms: u64,
    /// Enable automatic watcher restart on failure
    pub auto_restart: bool,
    /// Maximum consecutive failures before giving up
    pub max_consecutive_failures: u32,
}

impl Default for RecoveryConfig {
    fn default() -> Self {
        Self {
            max_init_retries: 3,
            retry_delay_ms: 1000,
            enable_polling_fallback: true,
            polling_interval_ms: 5000,
            auto_restart: true,
            max_consecutive_failures: 10,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_watcher_config_builder() {
        let config = WatcherConfig::builder()
            .debounce_ms(1000)
            .max_file_size(5 * 1024 * 1024)
            .follow_symlinks(true)
            .branch_strategy(BranchStrategy::IndexAll)
            .add_ignore_pattern("*.test".to_string())
            .build();

        assert_eq!(config.debounce_ms, 1000);
        assert_eq!(config.max_file_size, 5 * 1024 * 1024);
        assert!(config.follow_symlinks);
        assert_eq!(config.branch_strategy, BranchStrategy::IndexAll);
        assert!(config.ignore_patterns.contains(&"*.test".to_string()));
    }

    #[test]
    fn test_filter_config_extensions() {
        let config = FilterConfig::default();

        assert!(config.should_include_extension("rs"));
        assert!(config.should_include_extension("py"));
        assert!(!config.should_include_extension("exe"));
        assert!(!config.should_include_extension("jpg"));
    }

    #[test]
    fn test_filter_config_custom() {
        let config = FilterConfig {
            include_extensions: vec!["txt".to_string(), "md".to_string()],
            exclude_extensions: vec!["tmp".to_string()],
            min_file_size: 100,
            max_file_size: 1000,
            include_hidden: true,
            include_binary: false,
        };

        assert!(config.should_include_extension("txt"));
        assert!(!config.should_include_extension("rs"));
        assert!(config.is_size_valid(500));
        assert!(!config.is_size_valid(50));
        assert!(!config.is_size_valid(2000));
    }

    #[test]
    fn test_branch_strategy() {
        assert!(BranchStrategy::IndexCurrent.is_enabled());
        assert!(BranchStrategy::IndexCurrent.should_index_current());
        assert!(!BranchStrategy::Disabled.is_enabled());
        assert!(!BranchStrategy::Disabled.should_index_current());
    }

    #[test]
    fn test_config_durations() {
        let config = WatcherConfig {
            debounce_ms: 500,
            batch_timeout_ms: 1000,
            ..Default::default()
        };

        assert_eq!(config.debounce_duration(), Duration::from_millis(500));
        assert_eq!(config.batch_timeout_duration(), Duration::from_millis(1000));
    }
}
