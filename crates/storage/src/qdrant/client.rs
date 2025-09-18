use codesearch_core::config::StorageConfig;
use qdrant_client::{
    qdrant::{Distance, VectorParams, VectorsConfig},
    Qdrant,
};

pub(crate) struct QdrantStorage {
    pub(crate) client: Qdrant,
    pub(crate) config: StorageConfig,
}

impl QdrantStorage {
    /// Convert distance metric from config to Qdrant Distance enum
    pub(crate) fn get_distance_metric(&self) -> Distance {
        match self.config.distance_metric.as_str() {
            "euclidean" => Distance::Euclid,
            "dot" => Distance::Dot,
            _ => Distance::Cosine, // Default to cosine
        }
    }

    /// Get vector configuration for collection creation
    pub(crate) fn get_vectors_config(&self) -> VectorsConfig {
        VectorsConfig::from(VectorParams {
            size: self.config.vector_size as u64,
            distance: self.get_distance_metric().into(),
            ..Default::default()
        })
    }
}
