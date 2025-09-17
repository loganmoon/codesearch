//! Code Context Indexer - Three-stage indexing pipeline
//!
//! This crate provides a three-stage indexing pipeline (Extract → Transform → Commit)
//! for processing source code repositories.

#![warn(warnings)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]

pub mod common;
pub mod git_provider;
pub mod repository_indexer;
pub mod types;

// Re-export main types
pub use repository_indexer::{IndexProgress, RepositoryIndexer};
pub use types::{DiffContext, EntityChange, IndexResult, IndexStats};

// Re-export error types from core
pub use codesearch_core::error::{Error, Result};

/// Create a new repository indexer
pub fn create_indexer(
    storage_host: String,
    storage_port: u16,
    repository_path: std::path::PathBuf,
) -> RepositoryIndexer {
    RepositoryIndexer::new(storage_host, storage_port, repository_path)
}
