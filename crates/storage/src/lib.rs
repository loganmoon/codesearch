#![deny(warnings)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]

use async_trait::async_trait;
use codesearch_core::{CodeEntity, Error};
use serde::{Deserialize, Serialize};

// ==== Traits ====

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

impl From<CodeEntity> for StorageEntity {
    fn from(entity: CodeEntity) -> Self {
        StorageEntity {
            id: entity.entity_id.clone(),
            name: entity.name.clone(),
            kind: format!("{:?}", entity.entity_type),
            file_path: entity.file_path.to_string_lossy().into_owned(),
            start_line: entity.location.start_line,
            end_line: entity.location.end_line,
            content: entity.content.unwrap_or_default(),
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
