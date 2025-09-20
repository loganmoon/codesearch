//! Collection management for storage backends
//!
//! This module provides traits for managing collection lifecycle operations,
//! separate from CRUD operations.

use async_trait::async_trait;
use codesearch_core::error::Result;

/// Trait for collection lifecycle management operations
#[async_trait]
pub trait CollectionManager: Send + Sync {
    /// Create or verify collection with specified dimensions
    async fn ensure_collection(
        &self,
        collection_name: &str,
        vector_dimensions: usize,
    ) -> Result<()>;

    /// Delete collection (for testing/reset)
    async fn delete_collection(&self, collection_name: &str) -> Result<()>;

    /// Check if collection exists
    async fn collection_exists(&self, collection_name: &str) -> Result<bool>;

    /// Health check for the storage backend
    async fn health_check(&self) -> Result<()>;
}
