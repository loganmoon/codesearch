//! Configuration for indexer behavior
//!
//! Contains tunable parameters for indexing operations.

/// Configuration for indexer operations
#[derive(Debug, Clone)]
pub struct IndexerConfig {
    /// Batch size for file change watching
    pub watch_batch_size: usize,
    /// Timeout in milliseconds for watch batching
    pub watch_timeout_ms: u64,
    /// Batch size for full repository indexing
    pub index_batch_size: usize,
}

impl Default for IndexerConfig {
    fn default() -> Self {
        Self {
            watch_batch_size: 10,
            watch_timeout_ms: 1000,
            index_batch_size: 10,
        }
    }
}

impl IndexerConfig {
    /// Create a new configuration with default values
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a configuration with custom watch batch settings
    pub fn with_watch_batch(mut self, size: usize, timeout_ms: u64) -> Self {
        self.watch_batch_size = size;
        self.watch_timeout_ms = timeout_ms;
        self
    }

    /// Create a configuration with custom index batch size
    pub fn with_index_batch_size(mut self, size: usize) -> Self {
        self.index_batch_size = size;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = IndexerConfig::default();
        assert_eq!(config.watch_batch_size, 10);
        assert_eq!(config.watch_timeout_ms, 1000);
        assert_eq!(config.index_batch_size, 10);
    }

    #[test]
    fn test_custom_config() {
        let config = IndexerConfig::new()
            .with_watch_batch(20, 500)
            .with_index_batch_size(50);

        assert_eq!(config.watch_batch_size, 20);
        assert_eq!(config.watch_timeout_ms, 500);
        assert_eq!(config.index_batch_size, 50);
    }
}
