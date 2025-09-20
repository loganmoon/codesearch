//! Qdrant collection manager implementation for lifecycle operations

use crate::CollectionManager;
use async_trait::async_trait;
use codesearch_core::error::{Error, Result};
use qdrant_client::{
    qdrant::{CreateCollection, Distance, VectorParams, VectorsConfig},
    Qdrant,
};
use std::sync::Arc;

/// Qdrant collection manager handling collection lifecycle
pub(crate) struct QdrantCollectionManager {
    qdrant_client: Arc<Qdrant>,
}

impl QdrantCollectionManager {
    /// Create a new Qdrant collection manager
    pub async fn new(connection: Arc<Qdrant>) -> Result<Self> {
        Ok(Self {
            qdrant_client: connection,
        })
    }
}

#[async_trait]
impl CollectionManager for QdrantCollectionManager {
    async fn ensure_collection(
        &self,
        collection_name: &str,
        vector_dimensions: usize,
    ) -> Result<()> {
        // Check if collection exists
        let exists = self.collection_exists(collection_name).await?;

        if exists {
            // Verify dimensions match
            let collection_info = self
                .qdrant_client
                .collection_info(collection_name)
                .await
                .map_err(|e| Error::storage(format!("Failed to get collection info: {}", e)))?;

            if let Some(result) = collection_info.result {
                if let Some(config) = result.config {
                    if let Some(params) = config.params {
                        if let Some(vectors_config) = params.vectors_config {
                            // Check dimensions
                            let current_dims = match vectors_config.config {
                                Some(qdrant_client::qdrant::vectors_config::Config::Params(p)) => {
                                    p.size as usize
                                }
                                _ => 0,
                            };

                            if current_dims != vector_dimensions {
                                return Err(Error::storage(format!(
                                    "Collection '{}' exists with {} dimensions, but {} dimensions requested",
                                    collection_name, current_dims, vector_dimensions
                                )));
                            }
                        }
                    }
                }
            }
        } else {
            // Create new collection
            let create_collection = CreateCollection::from(
                qdrant_client::qdrant::CreateCollectionBuilder::new(collection_name)
                    .vectors_config(VectorsConfig::from(VectorParams::from(
                        qdrant_client::qdrant::VectorParamsBuilder::new(
                            vector_dimensions as u64,
                            Distance::Cosine,
                        ),
                    ))),
            );

            self.qdrant_client
                .create_collection(create_collection)
                .await
                .map_err(|e| Error::storage(format!("Failed to create collection: {}", e)))?;
        }

        Ok(())
    }

    async fn delete_collection(&self, collection_name: &str) -> Result<()> {
        self.qdrant_client
            .delete_collection(collection_name)
            .await
            .map_err(|e| Error::storage(format!("Failed to delete collection: {}", e)))?;

        Ok(())
    }

    async fn collection_exists(&self, collection_name: &str) -> Result<bool> {
        let collections = self
            .qdrant_client
            .list_collections()
            .await
            .map_err(|e| Error::storage(format!("Failed to list collections: {}", e)))?;

        Ok(collections
            .collections
            .iter()
            .any(|c| c.name == collection_name))
    }

    async fn health_check(&self) -> Result<()> {
        self.qdrant_client
            .health_check()
            .await
            .map_err(|e| Error::storage(format!("Qdrant health check failed: {}", e)))?;

        Ok(())
    }
}
