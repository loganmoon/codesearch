//! Event batching for efficient processing
//!
//! Provides batching logic for events with size and timeout constraints.

use std::time::Duration;

/// Batches events for efficient processing with size and timeout limits
pub struct EventBatcher<T> {
    batch: Vec<T>,
    batch_size: usize,
    timeout: Duration,
}

impl<T> EventBatcher<T> {
    /// Create a new event batcher
    pub fn new(batch_size: usize, timeout_ms: u64) -> Self {
        Self {
            batch: Vec::with_capacity(batch_size),
            batch_size,
            timeout: Duration::from_millis(timeout_ms),
        }
    }

    /// Add an event to the batch
    ///
    /// Returns Some(batch) if the batch is full and ready to be processed
    pub fn push(&mut self, item: T) -> Option<Vec<T>> {
        self.batch.push(item);
        if self.batch.len() >= self.batch_size {
            Some(std::mem::take(&mut self.batch))
        } else {
            None
        }
    }

    /// Flush the current batch, returning all accumulated events
    pub fn flush(&mut self) -> Vec<T> {
        std::mem::take(&mut self.batch)
    }

    /// Check if the batch is empty
    pub fn is_empty(&self) -> bool {
        self.batch.is_empty()
    }

    /// Get the timeout duration
    pub fn timeout(&self) -> Duration {
        self.timeout
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    use super::*;

    #[test]
    fn test_batcher_returns_batch_when_full() {
        let mut batcher = EventBatcher::new(3, 1000);

        assert!(batcher.push("a").is_none());
        assert!(batcher.push("b").is_none());

        let batch = batcher.push("c");
        assert!(batch.is_some());
        assert_eq!(batch.expect("Should have batch"), vec!["a", "b", "c"]);
        assert!(batcher.is_empty());
    }

    #[test]
    fn test_batcher_flush() {
        let mut batcher = EventBatcher::new(10, 1000);

        batcher.push("a");
        batcher.push("b");

        let batch = batcher.flush();
        assert_eq!(batch, vec!["a", "b"]);
        assert!(batcher.is_empty());
    }

    #[test]
    fn test_batcher_timeout_value() {
        let batcher = EventBatcher::<i32>::new(10, 500);
        assert_eq!(batcher.timeout(), Duration::from_millis(500));
    }
}
