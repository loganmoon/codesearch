#![deny(warnings)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]

pub mod error;
mod factory;

// Keep qdrant module private
#[cfg(not(target_arch = "wasm32"))]
mod qdrant;

// Export factory functions
pub use factory::{create_and_initialize_storage, create_storage_client};

use async_trait::async_trait;
use codesearch_core::{CodeEntity, Error};
use serde::{Deserialize, Serialize};

// ==== Traits ====

/// Combined storage trait for both client and management operations
pub trait Storage: StorageClient + StorageManager {}

// Implement Storage for any type that implements both StorageClient and StorageManager
impl<T: StorageClient + StorageManager> Storage for T {}

#[async_trait]
pub trait StorageClient: Send + Sync {
    async fn bulk_load_entities(
        &self,
        entities: &[CodeEntity],
        functions: &[CodeEntity],
        types: &[CodeEntity],
        variables: &[CodeEntity],
        relationships: &[(String, String, String)],
    ) -> Result<(), Error>;

    async fn initialize(&self) -> Result<(), Error>;
    async fn clear(&self) -> Result<(), Error>;

    /// Search for entities similar to the given query vector
    async fn search_similar(
        &self,
        query_vector: Vec<f32>,
        limit: usize,
        score_threshold: Option<f32>,
    ) -> Result<Vec<ScoredEntity>, Error>;

    /// Get a single entity by its ID
    async fn get_entity_by_id(&self, id: &str) -> Result<Option<StorageEntity>, Error>;

    /// Get multiple entities by their IDs
    async fn get_entities_by_ids(&self, ids: &[String]) -> Result<Vec<StorageEntity>, Error>;
}

#[async_trait]
pub trait StorageManager: Send + Sync {
    async fn create_collection(&self, name: &str) -> Result<(), Error>;
    async fn delete_collection(&self, name: &str) -> Result<(), Error>;
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

/// Entity with a similarity score from search results
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

// ==== Mock Implementation ====

#[derive(Default)]
pub struct MockStorageClient;

impl MockStorageClient {
    pub fn new() -> Self {
        MockStorageClient
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
    ) -> Result<(), Error> {
        eprintln!("MockStorageClient: bulk_load_entities called");
        Ok(())
    }

    async fn initialize(&self) -> Result<(), Error> {
        eprintln!("MockStorageClient: initialize called");
        Ok(())
    }

    async fn clear(&self) -> Result<(), Error> {
        eprintln!("MockStorageClient: clear called");
        Ok(())
    }

    async fn search_similar(
        &self,
        _query_vector: Vec<f32>,
        limit: usize,
        _score_threshold: Option<f32>,
    ) -> Result<Vec<ScoredEntity>, Error> {
        eprintln!("MockStorageClient: search_similar called with limit {limit}");
        Ok(Vec::new())
    }

    async fn get_entity_by_id(&self, id: &str) -> Result<Option<StorageEntity>, Error> {
        eprintln!("MockStorageClient: get_entity_by_id called for {id}");
        Ok(None)
    }

    async fn get_entities_by_ids(&self, ids: &[String]) -> Result<Vec<StorageEntity>, Error> {
        eprintln!(
            "MockStorageClient: get_entities_by_ids called for {} ids",
            ids.len()
        );
        Ok(Vec::new())
    }
}

#[async_trait]
impl StorageManager for MockStorageClient {
    async fn create_collection(&self, name: &str) -> Result<(), Error> {
        eprintln!("MockStorageClient: create_collection called for {name}");
        Ok(())
    }

    async fn delete_collection(&self, name: &str) -> Result<(), Error> {
        eprintln!("MockStorageClient: delete_collection called for {name}");
        Ok(())
    }

    async fn collection_exists(&self, name: &str) -> Result<bool, Error> {
        eprintln!("MockStorageClient: collection_exists called for {name}");
        Ok(false)
    }
}
