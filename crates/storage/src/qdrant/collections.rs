use crate::error::StorageError;
use crate::{StorageClient, StorageManager};
use async_trait::async_trait;
use codesearch_core::Error;
use qdrant_client::qdrant::{CreateCollectionBuilder, DeleteCollectionBuilder};

use super::client::QdrantStorage;

#[async_trait]
impl StorageManager for QdrantStorage {
    async fn create_collection(&self, name: &str) -> Result<(), Error> {
        // Check if collection already exists
        if self.collection_exists(name).await? {
            return Ok(());
        }

        // Create collection with proper vector configuration
        let create_collection =
            CreateCollectionBuilder::new(name).vectors_config(self.get_vectors_config());

        self.client
            .create_collection(create_collection.build())
            .await
            .map_err(|e| {
                StorageError::BackendError(format!("Failed to create collection {name}: {e}"))
            })?;

        Ok(())
    }

    async fn delete_collection(&self, name: &str) -> Result<(), Error> {
        let delete_collection = DeleteCollectionBuilder::new(name);

        self.client
            .delete_collection(delete_collection.build())
            .await
            .map_err(|e| {
                StorageError::BackendError(format!("Failed to delete collection {name}: {e}"))
            })?;

        Ok(())
    }

    async fn collection_exists(&self, name: &str) -> Result<bool, Error> {
        let collections =
            self.client.list_collections().await.map_err(|e| {
                StorageError::BackendError(format!("Failed to list collections: {e}"))
            })?;

        Ok(collections.collections.iter().any(|c| c.name == name))
    }
}

#[async_trait]
impl StorageClient for QdrantStorage {
    async fn initialize(&self) -> Result<(), Error> {
        // Create default collection if it doesn't exist
        self.create_collection(&self.config.collection_name).await
    }

    async fn clear(&self) -> Result<(), Error> {
        // Delete and recreate the collection to clear all data
        let collection_name = &self.config.collection_name;
        if self.collection_exists(collection_name).await? {
            self.delete_collection(collection_name).await?;
        }
        self.create_collection(collection_name).await
    }

    // Other StorageClient trait methods will be implemented in operations.rs and search.rs
    async fn bulk_load_entities(
        &self,
        entities: &[codesearch_core::CodeEntity],
        functions: &[codesearch_core::CodeEntity],
        types: &[codesearch_core::CodeEntity],
        variables: &[codesearch_core::CodeEntity],
        relationships: &[(String, String, String)],
    ) -> Result<(), Error> {
        // Implementation in operations.rs
        super::operations::bulk_load_entities(
            self,
            entities,
            functions,
            types,
            variables,
            relationships,
        )
        .await
    }

    async fn search_similar(
        &self,
        query_vector: Vec<f32>,
        limit: usize,
        score_threshold: Option<f32>,
    ) -> Result<Vec<crate::ScoredEntity>, Error> {
        // Implementation in search.rs
        super::search::search_similar(self, query_vector, limit, score_threshold).await
    }

    async fn get_entity_by_id(&self, id: &str) -> Result<Option<crate::StorageEntity>, Error> {
        // Implementation in search.rs
        super::search::get_entity_by_id(self, id).await
    }

    async fn get_entities_by_ids(
        &self,
        ids: &[String],
    ) -> Result<Vec<crate::StorageEntity>, Error> {
        // Implementation in search.rs
        super::search::get_entities_by_ids(self, ids).await
    }
}
