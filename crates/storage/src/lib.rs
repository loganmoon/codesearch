//! Storage layer for indexed code entities
//!
//! This module provides the persistence layer for storing and retrieving
//! indexed code entities and their relationships.

#![deny(warnings)]
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]

mod collection_manager;
mod postgres;
mod qdrant;

use async_trait::async_trait;
use codesearch_core::{config::StorageConfig, entities::EntityType, error::Result, CodeEntity};
use sqlx::postgres::PgConnectOptions;
use std::path::PathBuf;
use std::sync::Arc;

// Re-export only the trait
pub use collection_manager::CollectionManager;
pub use postgres::{OutboxOperation, PostgresClientTrait};

// Re-export types needed by outbox-processor
pub use postgres::{OutboxEntry, TargetStore};

// Re-export mock for testing
pub use postgres::mock::MockPostgresClient;

pub use uuid::Uuid;

/// Search filters for querying entities
#[derive(Debug, Default, Clone)]
pub struct SearchFilters {
    pub entity_type: Option<EntityType>,
    pub language: Option<String>,
    pub file_path: Option<PathBuf>,
}

/// Represents a code entity with its vector embedding
#[derive(Debug, Clone)]
pub struct EmbeddedEntity {
    pub entity: CodeEntity,
    pub embedding: Vec<f32>,
    pub qdrant_point_id: Uuid,
}

/// Trait for storage clients (CRUD operations only)
#[async_trait]
pub trait StorageClient: Send + Sync {
    /// Bulk load entities with their embeddings
    /// Returns a vector of (entity_id, point_id) pairs
    async fn bulk_load_entities(
        &self,
        embedded_entities: Vec<EmbeddedEntity>,
    ) -> Result<Vec<(String, uuid::Uuid)>>;

    /// Search returns (entity_id, repository_id, score) tuples
    /// Caller must fetch full entities from Postgres
    async fn search_similar(
        &self,
        query_embedding: Vec<f32>,
        limit: usize,
        filters: Option<SearchFilters>,
    ) -> Result<Vec<(String, String, f32)>>;

    /// Get entity by ID
    async fn get_entity(&self, entity_id: &str) -> Result<Option<CodeEntity>>;

    /// Delete entities from vector store
    async fn delete_entities(&self, entity_ids: &[String]) -> Result<()>;
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
        _embedded_entities: Vec<EmbeddedEntity>,
    ) -> Result<Vec<(String, uuid::Uuid)>> {
        // Mock implementation - return empty vec
        Ok(vec![])
    }

    async fn search_similar(
        &self,
        _query_embedding: Vec<f32>,
        _limit: usize,
        _filters: Option<SearchFilters>,
    ) -> Result<Vec<(String, String, f32)>> {
        // Mock implementation - return empty results
        Ok(vec![])
    }

    async fn get_entity(&self, _entity_id: &str) -> Result<Option<CodeEntity>> {
        // Mock implementation - return None
        Ok(None)
    }

    async fn delete_entities(&self, _entity_ids: &[String]) -> Result<()> {
        // Mock implementation - do nothing
        Ok(())
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

/// Factory function to create a Postgres metadata client
pub async fn create_postgres_client(
    config: &StorageConfig,
) -> Result<Arc<dyn postgres::PostgresClientTrait>> {
    let connect_options = PgConnectOptions::new()
        .host(&config.postgres_host)
        .port(config.postgres_port)
        .username(&config.postgres_user)
        .password(&config.postgres_password)
        .database(&config.postgres_database);

    let pool = sqlx::PgPool::connect_with(connect_options)
        .await
        .map_err(|e| {
            codesearch_core::error::Error::storage(format!("Failed to connect to Postgres: {e}"))
        })?;

    Ok(Arc::new(postgres::PostgresClient::new(pool)) as Arc<dyn postgres::PostgresClientTrait>)
}
