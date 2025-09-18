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

// Re-export main types
pub use repository_indexer::{IndexProgress, RepositoryIndexer};
pub use types::{DiffContext, EntityChange, IndexResult, IndexStats};

// Re-export error types from core
pub use codesearch_core::error::{Error, Result};

/// Create a new repository indexer with the specified storage backend
///
/// # Arguments
///
/// * `storage_config` - The storage configuration
/// * `repository_path` - The path to the repository to index
pub fn create_indexer(
    storage_config: codesearch_core::config::StorageConfig,
    repository_path: std::path::PathBuf,
) -> RepositoryIndexer {
    RepositoryIndexer::new(storage_config, repository_path)
}

/// Create a new repository indexer with default storage configuration
///
/// # Arguments
///
/// * `repository_path` - The path to the repository to index
pub fn create_indexer_with_defaults(repository_path: std::path::PathBuf) -> RepositoryIndexer {
    RepositoryIndexer::with_defaults(repository_path)
}
