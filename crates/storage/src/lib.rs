#![deny(warnings)]
#![allow(dead_code)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]

pub mod error;
mod factory;
mod mock;

// Keep qdrant module private
#[cfg(not(target_arch = "wasm32"))]
mod qdrant;

// Export factory functions
pub use factory::{create_and_initialize_storage, create_storage_client};

use async_trait::async_trait;
use codesearch_core::{CodeEntity, Error};
use serde::{Deserialize, Serialize};

// ==== Models ====

/// Batch of entities to be loaded into storage.
///
/// This struct groups different types of code entities and their relationships
/// for efficient bulk loading into the storage backend.
///
/// # Example
/// ```ignore
/// let batch = EntityBatch::new()
///     .with_entities(&entities)
///     .with_functions(&functions)
///     .with_types(&types)
///     .with_variables(&variables)
///     .with_relationships(&relationships);
///
/// storage.bulk_load_entities(&batch).await?;
/// ```
#[derive(Debug, Default)]
pub struct EntityBatch<'a> {
    pub entities: &'a [CodeEntity],
    pub functions: &'a [CodeEntity],
    pub types: &'a [CodeEntity],
    pub variables: &'a [CodeEntity],
    pub relationships: &'a [(String, String, String)],
}

impl<'a> EntityBatch<'a> {
    /// Create a new empty batch
    pub fn new() -> Self {
        Self::default()
    }

    /// Builder methods for fluent API
    pub fn with_entities(mut self, entities: &'a [CodeEntity]) -> Self {
        self.entities = entities;
        self
    }

    pub fn with_functions(mut self, functions: &'a [CodeEntity]) -> Self {
        self.functions = functions;
        self
    }

    pub fn with_types(mut self, types: &'a [CodeEntity]) -> Self {
        self.types = types;
        self
    }

    pub fn with_variables(mut self, variables: &'a [CodeEntity]) -> Self {
        self.variables = variables;
        self
    }

    /// Get total count of all entities
    pub fn total_count(&self) -> usize {
        self.entities.len() + self.functions.len() + self.types.len() + self.variables.len()
    }
}

// ==== Traits ====

/// Combined storage trait for both client and management operations.
///
/// This trait combines `StorageClient` for data operations and `StorageManager`
/// for administrative operations. Implementations must support both interfaces.
///
/// # Example
/// ```ignore
/// let storage: Arc<dyn Storage> = create_storage_client(config).await?;
/// storage.initialize().await?;
/// storage.bulk_load_entities(&batch).await?;
/// ```
pub trait Storage: StorageClient + StorageManager {}

// Implement Storage for any type that implements both StorageClient and StorageManager
impl<T: StorageClient + StorageManager> Storage for T {}

/// Client interface for data operations on the storage backend.
///
/// Provides methods for loading, searching, and retrieving code entities.
/// All operations are async and return `Result` types for error handling.
#[async_trait]
pub trait StorageClient: Send + Sync {
    /// Load a batch of entities into storage.
    ///
    /// # Arguments
    /// * `batch` - The batch of entities to load
    ///
    /// # Errors
    /// Returns an error if the storage operation fails.
    async fn bulk_load_entities(&self, batch: &EntityBatch<'_>) -> Result<(), Error>;

    /// Search for entities similar to the given query vector.
    ///
    /// Performs a vector similarity search to find code entities that are
    /// semantically similar to the query.
    ///
    /// # Arguments
    /// * `query_vector` - The embedding vector to search for
    /// * `limit` - Maximum number of results to return
    /// * `score_threshold` - Optional minimum similarity score filter
    ///
    /// # Returns
    /// A vector of `ScoredEntity` objects sorted by similarity score (highest first)
    ///
    /// # Errors
    /// Returns an error if the search operation fails.
    async fn search_similar(
        &self,
        query_vector: Vec<f32>,
        limit: usize,
        score_threshold: Option<f32>,
    ) -> Result<Vec<ScoredEntity>, Error>;

    /// Get a single entity by its ID.
    ///
    /// # Arguments
    /// * `id` - The unique identifier of the entity
    ///
    /// # Returns
    /// * `Some(StorageEntity)` if found
    /// * `None` if not found
    ///
    /// # Errors
    /// Returns an error if the retrieval operation fails.
    async fn get_entity_by_id(&self, id: &str) -> Result<Option<StorageEntity>, Error>;

    /// Get multiple entities by their IDs.
    ///
    /// # Arguments
    /// * `ids` - A slice of entity identifiers
    ///
    /// # Returns
    /// A vector of found entities (may be shorter than input if some IDs don't exist)
    ///
    /// # Errors
    /// Returns an error if the retrieval operation fails.
    async fn get_entities_by_ids(&self, ids: &[String]) -> Result<Vec<StorageEntity>, Error>;
}

/// Manager interface for administrative operations on the storage backend.
///
/// Provides methods for initializing, clearing, and managing collections.
/// All operations are async and return `Result` types for error handling.
#[async_trait]
pub trait StorageManager: Send + Sync {
    /// Initialize the storage backend.
    ///
    /// Creates necessary collections and prepares the storage for use.
    ///
    /// # Errors
    /// Returns an error if initialization fails.
    async fn initialize(&self) -> Result<(), Error>;

    /// Clear all data from the storage.
    ///
    /// Removes all entities from the default collection but keeps the collection structure.
    ///
    /// # Errors
    /// Returns an error if the clear operation fails.
    async fn clear(&self) -> Result<(), Error>;

    /// Create a new collection.
    ///
    /// # Arguments
    /// * `name` - The name of the collection to create
    ///
    /// # Errors
    /// Returns an error if collection creation fails.
    /// May succeed silently if the collection already exists.
    async fn create_collection(&self, name: &str) -> Result<(), Error>;

    /// Delete a collection.
    ///
    /// # Arguments
    /// * `name` - The name of the collection to delete
    ///
    /// # Errors
    /// Returns an error if collection deletion fails.
    async fn delete_collection(&self, name: &str) -> Result<(), Error>;

    /// Check if a collection exists.
    ///
    /// # Arguments
    /// * `name` - The name of the collection to check
    ///
    /// # Returns
    /// `true` if the collection exists, `false` otherwise
    ///
    /// # Errors
    /// Returns an error if the check operation fails.
    async fn collection_exists(&self, name: &str) -> Result<bool, Error>;
}

// ==== Models ====

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageEntity {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub file_path: String,
    pub start_line: usize,
    pub end_line: usize,
    pub content: String,
    pub embedding: Option<Vec<f32>>,
}

/// Entity with a similarity score from search results.
///
/// Represents a code entity retrieved from a similarity search,
/// along with its relevance score.
///
/// # Fields
/// * `entity` - The storage entity
/// * `score` - Similarity score (higher is more similar)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoredEntity {
    pub entity: StorageEntity,
    pub score: f32,
}

impl From<&CodeEntity> for StorageEntity {
    fn from(entity: &CodeEntity) -> Self {
        StorageEntity {
            id: entity.entity_id.clone(),
            name: entity.name.clone(),
            kind: format!("{:?}", entity.entity_type),
            file_path: entity.file_path.to_string_lossy().into_owned(),
            start_line: entity.location.start_line,
            end_line: entity.location.end_line,
            content: entity.content.clone().unwrap_or_default(),
            embedding: None,
        }
    }
}

// Mock implementation is now in private mock module for test/development use

// Export mock storage for testing (available to integration tests in other crates)
pub use mock::MockStorageClient;
