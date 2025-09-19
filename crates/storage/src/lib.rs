//! Storage layer for indexed code entities
//!
//! This module provides the persistence layer for storing and retrieving
//! indexed code entities and their relationships.

use async_trait::async_trait;
use codesearch_core::{error::Result, CodeEntity};

/// Trait for storage clients
#[async_trait]
pub trait StorageClient: Send + Sync {
    /// Bulk load entities into storage
    async fn bulk_load_entities(
        &self,
        entities: &[CodeEntity],
        functions: &[CodeEntity],
        types: &[CodeEntity],
        variables: &[CodeEntity],
        relationships: &[(String, String, String)],
    ) -> Result<()>;
}

/// Mock storage client for testing
pub struct MockStorageClient;

impl MockStorageClient {
    /// Create a new mock storage client
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl StorageClient for MockStorageClient {
    async fn bulk_load_entities(
        &self,
        _entities: &[CodeEntity],
        _functions: &[CodeEntity],
        _types: &[CodeEntity],
        _variables: &[CodeEntity],
        _relationships: &[(String, String, String)],
    ) -> Result<()> {
        // Mock implementation - just succeed
        Ok(())
    }
}
