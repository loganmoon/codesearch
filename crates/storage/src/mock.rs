// Private mock implementation for testing
use crate::{EntityBatch, ScoredEntity, StorageClient, StorageEntity, StorageManager};
use async_trait::async_trait;
use codesearch_core::Error;

#[derive(Default)]
pub(crate) struct MockStorageClient;

/// Builder for MockStorageClient
pub(crate) struct MockStorageClientBuilder;

impl MockStorageClientBuilder {
    pub(crate) fn new() -> Self {
        MockStorageClientBuilder
    }

    pub(crate) fn build(self) -> MockStorageClient {
        MockStorageClient
    }
}

impl MockStorageClient {
    /// Create a new mock client (kept for compatibility)
    pub(crate) fn new() -> Self {
        MockStorageClientBuilder::new().build()
    }
}

#[async_trait]
impl StorageClient for MockStorageClient {
    async fn bulk_load_entities(&self, batch: &EntityBatch<'_>) -> Result<(), Error> {
        eprintln!(
            "MockStorageClient: bulk_load_entities called with {} total entities",
            batch.total_count()
        );
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
    async fn initialize(&self) -> Result<(), Error> {
        eprintln!("MockStorageClient: initialize called");
        Ok(())
    }

    async fn clear(&self) -> Result<(), Error> {
        eprintln!("MockStorageClient: clear called");
        Ok(())
    }

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
