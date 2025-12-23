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
    pub max_queued_events: usize,
    /// Number of file system events batched before processing (default: 100)
    pub events_per_batch: usize,
    /// Maximum time to wait before processing a partial batch (default: 1000ms)
    pub max_batch_wait_time_ms: u64,
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
        Duration::from_millis(self.max_batch_wait_time_ms)
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
            max_queued_events: 100000, // Increased to handle burst activity from editors
            events_per_batch: 100,
            max_batch_wait_time_ms: 1000,
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
    pub fn max_queued_events(mut self, size: usize) -> Self {
        self.config.max_queued_events = size;
        self
    }

    /// Set events per batch
    pub fn events_per_batch(mut self, size: usize) -> Self {
        self.config.events_per_batch = size;
        self
    }

    /// Set maximum batch wait time in milliseconds
    pub fn max_batch_wait_time_ms(mut self, ms: u64) -> Self {
        self.config.max_batch_wait_time_ms = ms;
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
    fn test_config_durations() {
        let config = WatcherConfig {
            debounce_ms: 500,
            max_batch_wait_time_ms: 1000,
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
    fn test_max_queued_events_increased() {
        let config = WatcherConfig::default();
        assert_eq!(
            config.max_queued_events, 100000,
            "Channel size should be increased to 100000 to handle burst activity"
        );
    }
}
