//! Code Context Indexer - Three-stage indexing pipeline
//!
//! This crate provides a three-stage indexing pipeline (Extract → Transform → Commit)
//! for processing source code repositories.

#![deny(warnings)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// Private implementation modules
mod common;
mod config;
mod event_batcher;
mod repository_indexer;

// Public modules for file change processing
pub mod catch_up_indexer;
pub mod entity_processor;
pub mod file_change_processor;

use event_batcher::EventBatcher;

// Re-export config for public use
pub use config::IndexerConfig;

// Re-export error types from core
pub use codesearch_core::error::{Error, Result};

// Re-export RepositoryIndexer for direct use
pub use repository_indexer::RepositoryIndexer;

// Re-export public functions and types
pub use catch_up_indexer::{catch_up_from_git, CatchUpStats};
pub use file_change_processor::{process_file_changes, ProcessingStats};

use codesearch_watcher::FileChange;
use std::sync::Arc;
use tokio::sync::mpsc::Receiver;
use tokio::task::JoinHandle;
use tracing::error;

/// Main trait for repository indexing
#[async_trait]
pub trait Indexer: Send + Sync {
    /// Index the entire repository
    async fn index_repository(&mut self) -> Result<IndexResult>;
}

/// A partial error that occurred during indexing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexError {
    /// The file path where the error occurred
    pub file_path: String,
    /// The error message
    pub message: String,
}

impl IndexError {
    /// Create a new IndexError
    pub fn new(file_path: String, message: String) -> Self {
        Self { file_path, message }
    }
}

/// Result of an indexing operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexResult {
    /// Statistics about the indexing operation
    stats: IndexStats,
    /// Any errors that occurred (non-fatal)
    errors: Vec<IndexError>,
}

impl IndexResult {
    /// Get the statistics from the indexing operation
    pub fn stats(&self) -> &IndexStats {
        &self.stats
    }

    /// Get any errors that occurred during indexing
    pub fn errors(&self) -> &[IndexError] {
        &self.errors
    }

    /// Create a new IndexResult (for internal use)
    pub(crate) fn new(stats: IndexStats, errors: Vec<IndexError>) -> Self {
        Self { stats, errors }
    }
}

/// Statistics for indexing operations
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IndexStats {
    /// Total number of files processed
    total_files: usize,
    /// Number of files that failed processing
    failed_files: usize,
    /// Number of entities extracted
    entities_extracted: usize,
    /// Number of entities skipped due to size limits
    entities_skipped_size: usize,
    /// Processing time in milliseconds
    processing_time_ms: u64,
}

impl IndexStats {
    /// Get the total number of files processed
    pub fn total_files(&self) -> usize {
        self.total_files
    }

    /// Get the number of files that failed processing
    pub fn failed_files(&self) -> usize {
        self.failed_files
    }

    /// Get the number of entities extracted
    pub fn entities_extracted(&self) -> usize {
        self.entities_extracted
    }

    /// Get the number of entities skipped due to size limits
    pub fn entities_skipped_size(&self) -> usize {
        self.entities_skipped_size
    }

    /// Get the processing time in milliseconds
    pub fn processing_time_ms(&self) -> u64 {
        self.processing_time_ms
    }

    /// Merge another stats instance into this one (for internal use)
    #[allow(dead_code)]
    pub(crate) fn merge(&mut self, other: IndexStats) {
        self.total_files += other.total_files;
        self.failed_files += other.failed_files;
        self.entities_extracted += other.entities_extracted;
        self.entities_skipped_size += other.entities_skipped_size;
        self.processing_time_ms += other.processing_time_ms;
    }

    /// Create stats with specific values (for internal use)
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Set fields (for internal use)
    pub(crate) fn set_total_files(&mut self, value: usize) {
        self.total_files = value;
    }

    pub(crate) fn set_processing_time_ms(&mut self, value: u64) {
        self.processing_time_ms = value;
    }

    pub(crate) fn set_entities_extracted(&mut self, value: usize) {
        self.entities_extracted = value;
    }

    pub(crate) fn increment_failed_files(&mut self) {
        self.failed_files += 1;
    }
}

/// Create a new repository indexer
///
/// # Errors
///
/// Returns an error if:
/// - The `repository_id` is not a valid UUID string
/// - The repository path is invalid or inaccessible
///
/// # Example
///
/// ```no_run
/// use codesearch_indexer::{create_indexer, IndexerConfig};
/// use std::sync::Arc;
/// use std::path::PathBuf;
///
/// # async fn example() -> codesearch_indexer::Result<()> {
/// # let embedding_manager = panic!("example code");
/// # let postgres_client: Arc<dyn codesearch_storage::PostgresClientTrait> = panic!("example code");
/// let indexer = create_indexer(
///     PathBuf::from("/path/to/repo"),
///     "550e8400-e29b-41d4-a716-446655440000".to_string(),
///     embedding_manager,
///     postgres_client,
///     None,
///     IndexerConfig::default(),
/// )?;
/// # Ok(())
/// # }
/// ```
pub fn create_indexer(
    repository_path: PathBuf,
    repository_id: String,
    embedding_manager: std::sync::Arc<codesearch_embeddings::EmbeddingManager>,
    postgres_client: std::sync::Arc<dyn codesearch_storage::PostgresClientTrait>,
    git_repo: Option<codesearch_watcher::GitRepository>,
    config: config::IndexerConfig,
) -> Result<Box<dyn Indexer>> {
    Ok(Box::new(repository_indexer::RepositoryIndexer::new(
        repository_path,
        repository_id,
        embedding_manager,
        postgres_client,
        git_repo,
        config,
    )?))
}

/// Start watching for file changes and processing them in the background
///
/// Spawns a background task that consumes file change events from the watcher
/// and processes them in batches using the default `IndexerConfig` settings.
///
/// # Behavior
///
/// - Events are batched by size (default: 10) and timeout (default: 1000ms)
/// - Processes batch when full OR when timeout expires with pending events
/// - Continues until the event_rx channel is closed
/// - Errors during processing are logged but do not stop the task
///
/// # Returns
///
/// A `JoinHandle` that completes with `Ok(())` when the channel closes normally.
/// The task will not return an error - all processing errors are logged internally.
///
/// # Example
///
/// ```no_run
/// use codesearch_indexer::start_watching;
/// use tokio::sync::mpsc;
/// use std::sync::Arc;
/// use std::path::PathBuf;
/// use uuid::Uuid;
///
/// # async fn example() -> codesearch_indexer::Result<()> {
/// # let repo_id = Uuid::new_v4();
/// # let repo_root = PathBuf::from("/path/to/repo");
/// # let embedding_manager = panic!("example code");
/// # let postgres_client: Arc<dyn codesearch_storage::PostgresClientTrait> = panic!("example code");
/// let (tx, rx) = mpsc::channel(100);
///
/// let task = start_watching(
///     rx,
///     repo_id,
///     repo_root,
///     embedding_manager,
///     postgres_client,
/// );
///
/// // Task runs until tx is dropped or channel is closed
/// // Join handle can be awaited for graceful shutdown
/// let _ = task.await;
/// # Ok(())
/// # }
/// ```
pub fn start_watching(
    mut event_rx: Receiver<FileChange>,
    repo_id: uuid::Uuid,
    repo_root: PathBuf,
    embedding_manager: Arc<codesearch_embeddings::EmbeddingManager>,
    postgres_client: Arc<dyn codesearch_storage::PostgresClientTrait>,
) -> JoinHandle<Result<()>> {
    tokio::spawn(async move {
        tracing::info!("File watcher indexer task started");

        let config = IndexerConfig::default();
        let mut batcher = EventBatcher::new(config.watch_batch_size, config.watch_timeout_ms);

        loop {
            let timeout = tokio::time::sleep(batcher.timeout());

            tokio::select! {
                // Receive event
                maybe_event = event_rx.recv() => {
                    match maybe_event {
                        Some(event) => {
                            // Add event to batch, process if full
                            if let Some(batch) = batcher.push(event) {
                                if let Err(e) = process_file_changes(
                                    batch,
                                    repo_id,
                                    &repo_root,
                                    &embedding_manager,
                                    &postgres_client,
                                )
                                .await
                                {
                                    error!("Error processing file changes: {e}");
                                }
                            }
                        }
                        None => {
                            tracing::info!("File watcher channel closed, stopping indexer task");
                            break;
                        }
                    }
                }

                // Timeout - process partial batch
                _ = timeout => {
                    if !batcher.is_empty() {
                        let batch = batcher.flush();
                        if let Err(e) = process_file_changes(
                            batch,
                            repo_id,
                            &repo_root,
                            &embedding_manager,
                            &postgres_client,
                        )
                        .await
                        {
                            error!("Error processing file changes: {e}");
                        }
                    }
                }
            }
        }

        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_index_stats_merge() {
        let mut stats1 = IndexStats {
            total_files: 10,
            failed_files: 2,
            entities_extracted: 50,
            entities_skipped_size: 3,
            processing_time_ms: 1000,
        };

        let stats2 = IndexStats {
            total_files: 5,
            failed_files: 1,
            entities_extracted: 20,
            entities_skipped_size: 1,
            processing_time_ms: 500,
        };

        stats1.merge(stats2);

        assert_eq!(stats1.total_files, 15);
        assert_eq!(stats1.failed_files, 3);
        assert_eq!(stats1.entities_extracted, 70);
        assert_eq!(stats1.entities_skipped_size, 4);
        assert_eq!(stats1.processing_time_ms, 1500);
    }

    #[test]
    fn test_index_stats_merge_with_empty() {
        let mut stats = IndexStats {
            total_files: 10,
            failed_files: 2,
            entities_extracted: 50,
            entities_skipped_size: 3,
            processing_time_ms: 1000,
        };

        stats.merge(IndexStats::default());

        assert_eq!(stats.total_files, 10);
        assert_eq!(stats.failed_files, 2);
        assert_eq!(stats.entities_extracted, 50);
        assert_eq!(stats.entities_skipped_size, 3);
        assert_eq!(stats.processing_time_ms, 1000);
    }
}
