//! Qdrant collection manager implementation for lifecycle operations

use crate::CollectionManager;
use async_trait::async_trait;
use codesearch_core::error::{Error, Result};
use qdrant_client::{
    qdrant::{
        vectors_config::Config as VectorsConfigEnum, CreateCollection, Distance, Modifier,
        SparseIndexConfig, SparseVectorParams, VectorParams, VectorParamsMap, VectorsConfig,
    },
    Qdrant,
};
use std::{collections::HashMap, sync::Arc};

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
        dense_dimensions: usize,
    ) -> Result<()> {
        // Check if collection exists
        let exists = self.collection_exists(collection_name).await?;

        if exists {
            // Verify dimensions match for both dense and sparse vectors
            let collection_info = self
                .qdrant_client
                .collection_info(collection_name)
                .await
                .map_err(|e| Error::storage(format!("Failed to get collection info: {e}")))?;

            if let Some(result) = collection_info.result {
                if let Some(config) = result.config {
                    if let Some(params) = config.params {
                        if let Some(vectors_config) = params.vectors_config {
                            // Check for named vectors configuration
                            match vectors_config.config {
                                Some(VectorsConfigEnum::ParamsMap(ref map)) => {
                                    // Verify dense vector dimensions
                                    if let Some(dense_params) = map.map.get("dense") {
                                        let current_dense_dims = dense_params.size as usize;
                                        if current_dense_dims != dense_dimensions {
                                            return Err(Error::storage(format!(
                                                "Collection '{collection_name}' exists with dense dimensions {current_dense_dims}, but {dense_dimensions} requested"
                                            )));
                                        }
                                    } else {
                                        return Err(Error::storage(format!(
                                            "Collection '{collection_name}' exists but missing 'dense' vector configuration"
                                        )));
                                    }

                                    // Verify sparse vector exists
                                    if !map.map.contains_key("sparse") {
                                        return Err(Error::storage(format!(
                                            "Collection '{collection_name}' exists but missing 'sparse' vector configuration"
                                        )));
                                    }
                                }
                                Some(VectorsConfigEnum::Params(_)) => {
                                    return Err(Error::storage(format!(
                                        "Collection '{collection_name}' exists with single vector config, but named vectors required for hybrid search"
                                    )));
                                }
                                None => {
                                    return Err(Error::storage(format!(
                                        "Collection '{collection_name}' exists but has no vector configuration"
                                    )));
                                }
                            }
                        }
                    }
                }
            }
        } else {
            // Create new collection with named vectors (dense + sparse)
            let mut vector_params_map = HashMap::new();

            // Dense vector configuration
            vector_params_map.insert(
                "dense".to_string(),
                VectorParams {
                    size: dense_dimensions as u64,
                    // Note: distance field is i32 per qdrant-client API (enum discriminant)
                    distance: Distance::Cosine as i32,
                    ..Default::default()
                },
            );

            // Note: Sparse vectors are configured separately via sparse_vectors_config
            // in CreateCollectionBuilder, not in the vector_params_map

            let vectors_config = VectorsConfig {
                config: Some(VectorsConfigEnum::ParamsMap(VectorParamsMap {
                    map: vector_params_map,
                })),
            };

            // Create sparse vector params with IDF modifier
            let mut sparse_vectors_config = HashMap::new();
            let sparse_index_config = SparseIndexConfig {
                full_scan_threshold: Some(10000),
                on_disk: None,
                datatype: None,
            };

            sparse_vectors_config.insert(
                "sparse".to_string(),
                SparseVectorParams {
                    modifier: Some(Modifier::Idf as i32),
                    index: Some(sparse_index_config),
                },
            );

            let create_collection = CreateCollection::from(
                qdrant_client::qdrant::CreateCollectionBuilder::new(collection_name)
                    .vectors_config(vectors_config)
                    .sparse_vectors_config(sparse_vectors_config),
            );

            self.qdrant_client
                .create_collection(create_collection)
                .await
                .map_err(|e| Error::storage(format!("Failed to create collection: {e}")))?;
        }

        Ok(())
    }

    async fn delete_collection(&self, collection_name: &str) -> Result<()> {
        self.qdrant_client
            .delete_collection(collection_name)
            .await
            .map_err(|e| Error::storage(format!("Failed to delete collection: {e}")))?;

        Ok(())
    }

    async fn collection_exists(&self, collection_name: &str) -> Result<bool> {
        let collections = self
            .qdrant_client
            .list_collections()
            .await
            .map_err(|e| Error::storage(format!("Failed to list collections: {e}")))?;

        Ok(collections
            .collections
            .iter()
            .any(|c| c.name == collection_name))
    }

    async fn health_check(&self) -> Result<()> {
        self.qdrant_client
            .health_check()
            .await
            .map_err(|e| Error::storage(format!("Qdrant health check failed: {e}")))?;

        Ok(())
    }
}
