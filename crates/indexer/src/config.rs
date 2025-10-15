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
    /// Channel buffer size for inter-stage communication
    pub channel_buffer_size: usize,
    /// Maximum number of entities per EntityBatch sent to embedding generation
    pub max_entity_batch_size: usize,
    /// Number of concurrent file extractions in Stage 2
    pub file_extraction_concurrency: usize,
    /// Number of concurrent snapshot updates in Stage 5
    pub snapshot_update_concurrency: usize,
}

impl Default for IndexerConfig {
    fn default() -> Self {
        Self {
            watch_batch_size: 10,
            watch_timeout_ms: 1000,
            index_batch_size: 50,
            channel_buffer_size: 20,
            max_entity_batch_size: 256,
            file_extraction_concurrency: 16,
            snapshot_update_concurrency: 16,
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

    /// Set channel buffer size
    pub fn with_channel_buffer_size(mut self, size: usize) -> Self {
        self.channel_buffer_size = size;
        self
    }

    /// Set max entity batch size
    pub fn with_max_entity_batch_size(mut self, size: usize) -> Self {
        self.max_entity_batch_size = size;
        self
    }

    /// Set file extraction concurrency
    pub fn with_file_extraction_concurrency(mut self, concurrency: usize) -> Self {
        self.file_extraction_concurrency = concurrency;
        self
    }

    /// Set snapshot update concurrency
    pub fn with_snapshot_update_concurrency(mut self, concurrency: usize) -> Self {
        self.snapshot_update_concurrency = concurrency;
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
        assert_eq!(config.index_batch_size, 50);
        assert_eq!(config.channel_buffer_size, 20);
        assert_eq!(config.max_entity_batch_size, 256);
        assert_eq!(config.file_extraction_concurrency, 16);
        assert_eq!(config.snapshot_update_concurrency, 16);
    }

    #[test]
    fn test_custom_config() {
        let config = IndexerConfig::new()
            .with_watch_batch(20, 500)
            .with_index_batch_size(100)
            .with_channel_buffer_size(50)
            .with_max_entity_batch_size(512);

        assert_eq!(config.watch_batch_size, 20);
        assert_eq!(config.watch_timeout_ms, 500);
        assert_eq!(config.index_batch_size, 100);
        assert_eq!(config.channel_buffer_size, 50);
        assert_eq!(config.max_entity_batch_size, 512);
    }
}
