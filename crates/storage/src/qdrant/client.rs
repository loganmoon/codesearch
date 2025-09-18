use codesearch_core::{config::StorageConfig, Error};
use qdrant_client::{
    qdrant::{Distance, VectorParams, VectorsConfig},
    Qdrant,
};
use std::time::Duration;

pub(crate) struct QdrantStorage {
    pub(crate) client: Qdrant,
    pub(crate) config: StorageConfig,
}

impl QdrantStorage {
    /// Create a new Qdrant storage client
    pub(crate) async fn new(config: StorageConfig) -> Result<Self, Error> {
        // Create Qdrant client with retry configuration
        let url = format!("http://{}:{}", config.host, config.port);

        let mut client_config = qdrant_client::config::QdrantConfig::from_url(&url);
        client_config.timeout = Duration::from_millis(config.timeout_ms);

        // Add API key if provided (for Qdrant Cloud)
        if let Some(api_key) = &config.api_key {
            client_config.api_key = Some(api_key.clone());
        }

        let client = Qdrant::new(client_config)
            .map_err(|e| Error::storage(format!("Connection failed: {e}")))?;

        // Verify connection is alive
        client
            .health_check()
            .await
            .map_err(|e| Error::storage(format!("Health check failed: {e}")))?;

        Ok(Self { client, config })
    }

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
