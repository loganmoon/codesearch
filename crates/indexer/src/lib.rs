//! Code Context Indexer - Three-stage indexing pipeline
//!
//! This crate provides a three-stage indexing pipeline (Extract → Transform → Commit)
//! for processing source code repositories.

#![warn(warnings)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// Private implementation modules
mod common;
mod git_provider;
mod repository_indexer;
mod types;

// Re-export error types from core
pub use codesearch_core::error::{Error, Result};

// Re-export RepositoryIndexer for direct use
pub use repository_indexer::RepositoryIndexer;

/// Main trait for repository indexing
#[async_trait]
pub trait Indexer: Send + Sync {
    /// Index the entire repository
    async fn index_repository(&mut self) -> Result<IndexResult>;
}

/// Result of an indexing operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexResult {
    /// Statistics about the indexing operation
    stats: IndexStats,
    /// Any errors that occurred (non-fatal)
    errors: Vec<String>,
}

impl IndexResult {
    /// Get the statistics from the indexing operation
    pub fn stats(&self) -> &IndexStats {
        &self.stats
    }

    /// Get any errors that occurred during indexing
    pub fn errors(&self) -> &[String] {
        &self.errors
    }

    /// Create a new IndexResult (for internal use)
    pub(crate) fn new(stats: IndexStats, errors: Vec<String>) -> Self {
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
    /// Number of relationships extracted
    relationships_extracted: usize,
    /// Number of functions indexed
    functions_indexed: usize,
    /// Number of types indexed
    types_indexed: usize,
    /// Number of variables indexed
    variables_indexed: usize,
    /// Number of entities skipped due to size limits
    entities_skipped_size: usize,
    /// Processing time in milliseconds
    processing_time_ms: u64,
    /// Memory usage in bytes (approximate)
    memory_usage_bytes: Option<u64>,
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

    /// Get the number of relationships extracted
    pub fn relationships_extracted(&self) -> usize {
        self.relationships_extracted
    }

    /// Get the number of functions indexed
    pub fn functions_indexed(&self) -> usize {
        self.functions_indexed
    }

    /// Get the number of types indexed
    pub fn types_indexed(&self) -> usize {
        self.types_indexed
    }

    /// Get the number of variables indexed
    pub fn variables_indexed(&self) -> usize {
        self.variables_indexed
    }

    /// Get the number of entities skipped due to size limits
    pub fn entities_skipped_size(&self) -> usize {
        self.entities_skipped_size
    }

    /// Get the processing time in milliseconds
    pub fn processing_time_ms(&self) -> u64 {
        self.processing_time_ms
    }

    /// Get the memory usage in bytes if available
    pub fn memory_usage_bytes(&self) -> Option<u64> {
        self.memory_usage_bytes
    }

    /// Merge another stats instance into this one (for internal use)
    pub(crate) fn merge(&mut self, other: IndexStats) {
        self.total_files += other.total_files;
        self.failed_files += other.failed_files;
        self.entities_extracted += other.entities_extracted;
        self.relationships_extracted += other.relationships_extracted;
        self.functions_indexed += other.functions_indexed;
        self.types_indexed += other.types_indexed;
        self.variables_indexed += other.variables_indexed;
        self.entities_skipped_size += other.entities_skipped_size;
        self.processing_time_ms += other.processing_time_ms;

        // For memory, take the max if both are present
        self.memory_usage_bytes = match (self.memory_usage_bytes, other.memory_usage_bytes) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        };
    }

    /// Create stats with specific values (for internal use)
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Set fields (for internal use)
    pub(crate) fn set_total_files(&mut self, value: usize) {
        self.total_files = value;
    }

    #[allow(dead_code)]
    pub(crate) fn set_failed_files(&mut self, value: usize) {
        self.failed_files = value;
    }

    pub(crate) fn set_entities_extracted(&mut self, value: usize) {
        self.entities_extracted = value;
    }

    #[allow(dead_code)]
    pub(crate) fn set_relationships_extracted(&mut self, value: usize) {
        self.relationships_extracted = value;
    }

    #[allow(dead_code)]
    pub(crate) fn set_functions_indexed(&mut self, value: usize) {
        self.functions_indexed = value;
    }

    #[allow(dead_code)]
    pub(crate) fn set_types_indexed(&mut self, value: usize) {
        self.types_indexed = value;
    }

    #[allow(dead_code)]
    pub(crate) fn set_variables_indexed(&mut self, value: usize) {
        self.variables_indexed = value;
    }

    pub(crate) fn set_processing_time_ms(&mut self, value: u64) {
        self.processing_time_ms = value;
    }

    #[allow(dead_code)]
    pub(crate) fn set_memory_usage_bytes(&mut self, value: Option<u64>) {
        self.memory_usage_bytes = value;
    }

    pub(crate) fn increment_failed_files(&mut self) {
        self.failed_files += 1;
    }
}

/// Create a new repository indexer
pub fn create_indexer(
    repository_path: PathBuf,
    storage_client: std::sync::Arc<dyn codesearch_storage::StorageClient>,
    embedding_manager: std::sync::Arc<codesearch_embeddings::EmbeddingManager>,
    postgres_client: Option<std::sync::Arc<codesearch_storage::postgres::PostgresClient>>,
    git_repo: Option<codesearch_watcher::GitRepository>,
) -> Box<dyn Indexer> {
    Box::new(repository_indexer::RepositoryIndexer::new(
        repository_path,
        storage_client,
        embedding_manager,
        postgres_client,
        git_repo,
    ))
}
