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
    /// Maximum file size to watch in bytes (default: 10MB)
    pub max_file_size: u64,
    /// Whether to follow symbolic links (default: false)
    pub follow_symlinks: bool,
    /// Maximum recursion depth for directory watching (default: 50)
    pub recursive_depth: u32,
    /// Maximum number of events in queue (default: 100000)
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
    ///
    /// These patterns prevent processing of temporary files created by editors,
    /// build systems, and version control, which can saturate event channels
    /// during heavy activity and cause unnecessary processing errors.
    pub fn default_ignore_patterns() -> Vec<String> {
        vec![
            // Editor temporary files (prevent channel saturation and canonicalization errors)
            "*.tmp".to_string(),   // Files ending in .tmp
            "*.tmp.*".to_string(), // VS Code temp files: file.rs.tmp.12345.67890
            ".*.sw?".to_string(),  // Vim swap files: .file.swp, .file.swo, .file.swn
            ".*.swx".to_string(),  // Extended Vim swap files
            "*.swp".to_string(),   // Vim swap files (non-hidden)
            "*.swo".to_string(),   // Vim swap files (non-hidden)
            "*~".to_string(),      // Backup files (Vim, Emacs, etc.)
            "*.bak".to_string(),   // Backup files
            "*.orig".to_string(),  // Merge conflict originals
            "*.rej".to_string(),   // Patch rejects
            "#*#".to_string(),     // Emacs auto-save files
            ".#*".to_string(),     // Emacs lock files
            "4913".to_string(),    // Vim backup files (specific pattern)
            // Log files
            "*.log".to_string(),
            // OS-specific files
            ".DS_Store".to_string(),
            "Thumbs.db".to_string(),
            // Build artifacts and dependencies
            "node_modules/**".to_string(),
            "target/**".to_string(),
            "dist/**".to_string(),
            "build/**".to_string(),
            // Version control
            ".git/**".to_string(),
            ".svn/**".to_string(),
            ".hg/**".to_string(),
            // Python artifacts
            "__pycache__/**".to_string(),
            "*.pyc".to_string(),
            ".pytest_cache/**".to_string(),
            ".coverage".to_string(),
            "*.egg-info/**".to_string(),
            // IDE files
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
            max_file_size: 10 * 1024 * 1024, // 10MB
            follow_symlinks: false,
            recursive_depth: 50,
            max_queue_size: 100000, // Increased to handle burst activity from editors
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
    #[allow(dead_code)]
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
    #[allow(dead_code)]
    pub fn is_size_valid(&self, size: u64) -> bool {
        size >= self.min_file_size && size <= self.max_file_size
    }

    /// Check if a hidden file should be included
    #[allow(dead_code)]
    pub fn should_include_hidden_file(&self, filename: &str) -> bool {
        !filename.starts_with('.') || self.include_hidden
    }
}

impl Default for FilterConfig {
    fn default() -> Self {
        Self {
            // Only include languages with at least partial infrastructure
            // Note: Only Rust is fully implemented with AST parsing
            include_extensions: vec![
                "rs".to_string(),  // Rust (fully implemented)
                "py".to_string(),  // Python (partial infrastructure)
                "js".to_string(),  // JavaScript (partial infrastructure)
                "jsx".to_string(), // React JavaScript (partial infrastructure)
                "ts".to_string(),  // TypeScript (partial infrastructure)
                "tsx".to_string(), // React TypeScript (partial infrastructure)
                "go".to_string(),  // Go (partial infrastructure)
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
            .add_ignore_pattern("*.test".to_string())
            .build();

        assert_eq!(config.debounce_ms, 1000);
        assert_eq!(config.max_file_size, 5 * 1024 * 1024);
        assert!(config.follow_symlinks);
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
    fn test_config_durations() {
        let config = WatcherConfig {
            debounce_ms: 500,
            batch_timeout_ms: 1000,
            ..Default::default()
        };

        assert_eq!(config.debounce_duration(), Duration::from_millis(500));
        assert_eq!(config.batch_timeout_duration(), Duration::from_millis(1000));
    }

    #[test]
    fn test_default_ignore_patterns_includes_editor_temp_files() {
        let patterns = WatcherConfig::default_ignore_patterns();

        // Verify VS Code temp file pattern
        assert!(
            patterns.contains(&"*.tmp.*".to_string()),
            "Should include VS Code temp file pattern *.tmp.*"
        );

        // Verify Vim swap file patterns
        assert!(
            patterns.contains(&".*.sw?".to_string()),
            "Should include Vim swap file pattern .*.sw?"
        );
        assert!(
            patterns.contains(&"*.swp".to_string()),
            "Should include Vim swap file pattern *.swp"
        );

        // Verify Emacs patterns
        assert!(
            patterns.contains(&"#*#".to_string()),
            "Should include Emacs auto-save pattern #*#"
        );
        assert!(
            patterns.contains(&".#*".to_string()),
            "Should include Emacs lock file pattern .#*"
        );

        // Verify backup patterns
        assert!(
            patterns.contains(&"*.bak".to_string()),
            "Should include backup file pattern *.bak"
        );
        assert!(
            patterns.contains(&"*.orig".to_string()),
            "Should include merge conflict pattern *.orig"
        );
    }

    #[test]
    fn test_max_queue_size_increased() {
        let config = WatcherConfig::default();
        assert_eq!(
            config.max_queue_size, 100000,
            "Channel size should be increased to 100000 to handle burst activity"
        );
    }
}
