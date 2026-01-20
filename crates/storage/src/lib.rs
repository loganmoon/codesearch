//! Storage layer for indexed code entities
//!
//! This module provides the persistence layer for storing and retrieving
//! indexed code entities and their relationships.

#![deny(warnings)]
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]

mod collection_manager;
mod neo4j;
mod postgres;
mod qdrant;

use async_trait::async_trait;
use codesearch_core::{
    config::StorageConfig,
    entities::EntityType,
    error::{Error, Result},
    CodeEntity,
};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

// Re-export only the trait
pub use collection_manager::CollectionManager;
pub use postgres::{BM25Statistics, EmbeddingCacheEntry, OutboxOperation, PostgresClientTrait};

// Re-export types needed by outbox-processor
pub use postgres::{OutboxEntry, TargetStore};

// Re-export concrete implementation for testing
pub use postgres::PostgresClient;

// Re-export mock for testing
pub use postgres::mock::MockPostgresClient;

// Re-export Neo4j trait and mock - only trait-based API is public
pub use neo4j::{MockNeo4jClient, Neo4jClientTrait, ALLOWED_RELATIONSHIP_TYPES};

pub use uuid::Uuid;

/// Cache statistics for embedding cache
#[derive(Debug, Clone)]
pub struct CacheStats {
    pub total_entries: i64,
    pub total_size_bytes: i64,
    pub entries_by_model: HashMap<String, i64>,
    pub oldest_entry: Option<chrono::DateTime<chrono::Utc>>,
    pub newest_entry: Option<chrono::DateTime<chrono::Utc>>,
}

/// Validate a database name for PostgreSQL
///
/// Ensures the database name:
/// - Contains only alphanumeric characters, underscores, and hyphens
/// - Does not exceed PostgreSQL's 63-character limit
///
/// This prevents SQL injection in CREATE DATABASE statements.
fn validate_database_name(name: &str) -> Result<()> {
    if !name
        .chars()
        .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
    {
        return Err(Error::storage(format!(
            "Invalid database name '{name}': only alphanumeric, underscore, and hyphen allowed"
        )));
    }
    if name.len() > 63 {
        return Err(Error::storage(
            "Database name exceeds PostgreSQL's 63-character limit".to_string(),
        ));
    }
    if name.is_empty() {
        return Err(Error::storage("Database name cannot be empty".to_string()));
    }
    Ok(())
}

/// Search filters for querying entities
#[derive(Debug, Default, Clone)]
pub struct SearchFilters {
    /// Filter by one or more entity types (OR logic)
    pub entity_types: Option<Vec<EntityType>>,
    pub language: Option<String>,
    pub file_path: Option<PathBuf>,
}

/// Represents a code entity with its vector embeddings
#[derive(Debug, Clone)]
pub struct EmbeddedEntity {
    pub entity: CodeEntity,
    pub dense_embedding: Vec<f32>,
    /// Sparse embedding is optional - not all embedding models produce sparse vectors
    pub sparse_embedding: Option<Vec<(u32, f32)>>,
    pub bm25_token_count: usize,
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

    /// Hybrid search combining dense and sparse vectors with RRF fusion
    /// Returns (entity_id, repository_id, score) tuples
    async fn search_similar_hybrid(
        &self,
        dense_query_embedding: Vec<f32>,
        sparse_query_embedding: Vec<(u32, f32)>,
        limit: usize,
        filters: Option<SearchFilters>,
        prefetch_multiplier: usize,
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

    async fn search_similar_hybrid(
        &self,
        _dense_query_embedding: Vec<f32>,
        _sparse_query_embedding: Vec<(u32, f32)>,
        _limit: usize,
        _filters: Option<SearchFilters>,
        _prefetch_multiplier: usize,
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

/// Configuration for creating Qdrant clients
#[derive(Debug, Clone)]
pub struct QdrantConfig {
    pub host: String,
    pub port: u16,
    pub rest_port: u16,
}

/// Configuration for creating Postgres clients
#[derive(Debug, Clone)]
pub struct PostgresConfig {
    pub host: String,
    pub port: u16,
    pub database: String,
    pub user: String,
    pub password: String,
    pub pool_size: u32,
    pub max_entities_per_db_operation: usize,
}

/// Create a StorageClient for a specific collection using provided config
pub async fn create_storage_client_from_config(
    config: &QdrantConfig,
    collection_name: &str,
) -> Result<Arc<dyn StorageClient>> {
    let url = format!("http://{}:{}", config.host, config.port);
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
    // First, connect to the default 'postgres' database to check if target database exists
    let default_connect_options = PgConnectOptions::new()
        .host(&config.postgres_host)
        .port(config.postgres_port)
        .username(&config.postgres_user)
        .password(&config.postgres_password)
        .database("postgres");

    let default_pool = sqlx::PgPool::connect_with(default_connect_options)
        .await
        .map_err(|e| {
            codesearch_core::error::Error::storage(format!(
                "Failed to connect to default Postgres database: {e}"
            ))
        })?;

    // Check if target database exists
    let db_exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_database WHERE datname = $1)")
            .bind(&config.postgres_database)
            .fetch_one(&default_pool)
            .await
            .map_err(|e| {
                codesearch_core::error::Error::storage(format!(
                    "Failed to check database existence: {e}"
                ))
            })?;

    // Create database if it doesn't exist
    if !db_exists {
        // Validate database name before using it in CREATE DATABASE
        validate_database_name(&config.postgres_database)?;

        let create_db_query = format!("CREATE DATABASE \"{}\"", &config.postgres_database);
        sqlx::query(&create_db_query)
            .execute(&default_pool)
            .await
            .map_err(|e| {
                codesearch_core::error::Error::storage(format!(
                    "Failed to create database '{}': {e}",
                    config.postgres_database
                ))
            })?;
    }

    // Close connection to default database
    default_pool.close().await;

    // Now connect to the target database with configured pool size
    let connect_options = PgConnectOptions::new()
        .host(&config.postgres_host)
        .port(config.postgres_port)
        .username(&config.postgres_user)
        .password(&config.postgres_password)
        .database(&config.postgres_database);

    let pool = PgPoolOptions::new()
        .max_connections(config.postgres_pool_size)
        .connect_with(connect_options)
        .await
        .map_err(|e| {
            codesearch_core::error::Error::storage(format!("Failed to connect to Postgres: {e}"))
        })?;

    Ok(Arc::new(postgres::PostgresClient::new(
        pool,
        config.max_entities_per_db_operation,
    )) as Arc<dyn postgres::PostgresClientTrait>)
}

/// Create a Postgres client using provided config
pub async fn create_postgres_client_from_config(
    config: &PostgresConfig,
) -> Result<Arc<dyn postgres::PostgresClientTrait>> {
    // First, connect to the default 'postgres' database to check if target database exists
    let default_connect_options = PgConnectOptions::new()
        .host(&config.host)
        .port(config.port)
        .username(&config.user)
        .password(&config.password)
        .database("postgres");

    let default_pool = sqlx::PgPool::connect_with(default_connect_options)
        .await
        .map_err(|e| {
            codesearch_core::error::Error::storage(format!(
                "Failed to connect to default Postgres database: {e}"
            ))
        })?;

    // Check if target database exists
    let db_exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_database WHERE datname = $1)")
            .bind(&config.database)
            .fetch_one(&default_pool)
            .await
            .map_err(|e| {
                codesearch_core::error::Error::storage(format!(
                    "Failed to check database existence: {e}"
                ))
            })?;

    // Create database if it doesn't exist
    if !db_exists {
        // Validate database name before using it in CREATE DATABASE
        validate_database_name(&config.database)?;

        let create_db_query = format!("CREATE DATABASE \"{}\"", &config.database);
        sqlx::query(&create_db_query)
            .execute(&default_pool)
            .await
            .map_err(|e| {
                codesearch_core::error::Error::storage(format!(
                    "Failed to create database '{}': {e}",
                    config.database
                ))
            })?;
    }

    // Close connection to default database
    default_pool.close().await;

    // Now connect to the target database
    let connect_options = PgConnectOptions::new()
        .host(&config.host)
        .port(config.port)
        .username(&config.user)
        .password(&config.password)
        .database(&config.database);

    let pool = PgPoolOptions::new()
        .max_connections(config.pool_size)
        .connect_with(connect_options)
        .await
        .map_err(|e| {
            codesearch_core::error::Error::storage(format!("Failed to connect to Postgres: {e}"))
        })?;

    Ok(Arc::new(postgres::PostgresClient::new(
        pool,
        config.max_entities_per_db_operation,
    )) as Arc<dyn postgres::PostgresClientTrait>)
}

/// Create Neo4j client from configuration
pub async fn create_neo4j_client(config: &StorageConfig) -> Result<Arc<dyn Neo4jClientTrait>> {
    let client = neo4j::Neo4jClient::new(config).await?;
    Ok(Arc::new(client) as Arc<dyn Neo4jClientTrait>)
}
