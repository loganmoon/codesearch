//! Code Context Indexer - Three-stage indexing pipeline
//!
//! This crate provides a three-stage indexing pipeline (Extract → Transform → Commit)
//! for processing source code repositories.

#![warn(warnings)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]

mod common;
mod git_provider;
mod repository_indexer;
mod types;

use async_trait::async_trait;

// Re-export main types
pub use repository_indexer::IndexProgress;
pub use types::{DiffContext, EntityChange, IndexResult, IndexStats};

// Re-export error types from core
pub use codesearch_core::error::{Error, Result};

/// Public trait for repository indexing
#[async_trait]
pub trait Indexer: Send + Sync {
    /// Index the entire repository
    async fn index_repository(&mut self) -> Result<IndexResult>;

    /// Get the repository path
    fn repository_path(&self) -> &std::path::Path;
}

/// Create a new repository indexer with the specified storage backend
///
/// # Arguments
///
/// * `storage_client` - The storage client to use
/// * `repository_path` - The path to the repository to index
pub fn create_indexer(
    storage_client: std::sync::Arc<dyn codesearch_storage::StorageClient>,
    repository_path: std::path::PathBuf,
) -> Box<dyn Indexer> {
    Box::new(repository_indexer::RepositoryIndexer::new(
        storage_client,
        repository_path,
    ))
}
