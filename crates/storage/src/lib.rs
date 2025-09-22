//! Storage layer for indexed code entities
//!
//! This module provides the persistence layer for storing and retrieving
//! indexed code entities and their relationships.

mod collection_manager;
mod qdrant;

use async_trait::async_trait;
use codesearch_core::{config::StorageConfig, entities::EntityType, error::Result, CodeEntity};
use std::path::PathBuf;
use std::sync::Arc;

// Re-export only the trait
pub use collection_manager::CollectionManager;

/// Search filters for querying entities
#[derive(Debug, Default, Clone)]
pub struct SearchFilters {
    pub entity_type: Option<EntityType>,
    pub language: Option<String>,
    pub file_path: Option<PathBuf>,
}

/// Trait for storage clients (CRUD operations only)
#[async_trait]
pub trait StorageClient: Send + Sync {
    /// Bulk load entities with their embeddings
    async fn bulk_load_entities(
        &self,
        entities: Vec<CodeEntity>,
        embeddings: Vec<Vec<f32>>,
    ) -> Result<()>;

    /// Search for similar entities
    async fn search_similar(
        &self,
        query_embedding: Vec<f32>,
        limit: usize,
        filters: Option<SearchFilters>,
    ) -> Result<Vec<(CodeEntity, f32)>>;

    /// Get entity by ID
    async fn get_entity(&self, entity_id: &str) -> Result<Option<CodeEntity>>;
}

/// Mock storage client for testing
pub struct MockStorageClient;

impl Default for MockStorageClient {
    fn default() -> Self {
        Self::new()
    }
}

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
        _entities: Vec<CodeEntity>,
        _embeddings: Vec<Vec<f32>>,
    ) -> Result<()> {
        // Mock implementation - just succeed
        Ok(())
    }

    async fn search_similar(
        &self,
        _query_embedding: Vec<f32>,
        _limit: usize,
        _filters: Option<SearchFilters>,
    ) -> Result<Vec<(CodeEntity, f32)>> {
        // Mock implementation - return empty results
        Ok(vec![])
    }

    async fn get_entity(&self, _entity_id: &str) -> Result<Option<CodeEntity>> {
        // Mock implementation - return None
        Ok(None)
    }
}

/// Factory function to create a storage client for CRUD operations
pub async fn create_storage_client(
    config: &StorageConfig,
    collection_name: &str,
) -> Result<Arc<dyn StorageClient>> {
    let url = format!("http://{}:{}", config.qdrant_host, config.qdrant_port);
    let qdrant_client = qdrant_client::Qdrant::from_url(&url).build().map_err(|e| {
        codesearch_core::error::Error::storage(format!("Failed to connect to Qdrant: {e}"))
    })?;

    let client = qdrant::client::QdrantStorageClient::new(
        Arc::new(qdrant_client),
        collection_name.to_string(),
    )
    .await?;

    Ok(Arc::new(client))
}

/// Factory function to create a collection manager for lifecycle operations
pub async fn create_collection_manager(
    config: &StorageConfig,
) -> Result<Arc<dyn CollectionManager>> {
    let url = format!("http://{}:{}", config.qdrant_host, config.qdrant_port);
    let qdrant_client = qdrant_client::Qdrant::from_url(&url).build().map_err(|e| {
        codesearch_core::error::Error::storage(format!("Failed to connect to Qdrant: {e}"))
    })?;

    let manager =
        qdrant::collection_manager::QdrantCollectionManager::new(Arc::new(qdrant_client)).await?;

    Ok(Arc::new(manager))
}
