use crate::qdrant::QdrantStorage;
use codesearch_core::{config::StorageConfig, Error};
use qdrant_client::Qdrant;
use std::time::Duration;

/// Builder for QdrantStorage
pub struct QdrantStorageBuilder {
    config: StorageConfig,
}

#[allow(dead_code)]
impl QdrantStorageBuilder {
    /// Create a new builder with the given configuration
    pub fn from_config(config: StorageConfig) -> Self {
        Self { config }
    }

    /// Set the storage provider
    pub fn provider(mut self, provider: impl Into<String>) -> Self {
        self.config.provider = provider.into();
        self
    }

    /// Set the host
    pub fn host(mut self, host: impl Into<String>) -> Self {
        self.config.host = host.into();
        self
    }

    /// Set the port
    pub fn port(mut self, port: u16) -> Self {
        self.config.port = port;
        self
    }

    /// Set the API key
    pub fn api_key(mut self, key: impl Into<String>) -> Self {
        self.config.api_key = Some(key.into());
        self
    }

    /// Set the collection name
    pub fn collection_name(mut self, name: impl Into<String>) -> Self {
        self.config.collection_name = name.into();
        self
    }

    /// Set the vector size
    pub fn vector_size(mut self, size: usize) -> Self {
        self.config.vector_size = size;
        self
    }

    /// Set the distance metric
    pub fn distance_metric(mut self, metric: impl Into<String>) -> Self {
        self.config.distance_metric = metric.into();
        self
    }

    /// Set the batch size
    pub fn batch_size(mut self, size: usize) -> Self {
        self.config.batch_size = size;
        self
    }

    /// Set the timeout in milliseconds
    pub fn timeout_ms(mut self, ms: u64) -> Self {
        self.config.timeout_ms = ms;
        self
    }

    /// Build and connect to QdrantStorage
    pub async fn build(self) -> Result<QdrantStorage, Error> {
        // Create Qdrant client with retry configuration
        let url = format!("http://{}:{}", self.config.host, self.config.port);

        let mut client_config = qdrant_client::config::QdrantConfig::from_url(&url);
        client_config.timeout = Duration::from_millis(self.config.timeout_ms);

        // Add API key if provided (for Qdrant Cloud)
        if let Some(api_key) = &self.config.api_key {
            client_config.api_key = Some(api_key.clone());
        }

        let client = Qdrant::new(client_config)
            .map_err(|e| Error::storage(format!("Connection failed: {e}")))?;

        // Verify connection is alive
        client
            .health_check()
            .await
            .map_err(|e| Error::storage(format!("Health check failed: {e}")))?;

        Ok(QdrantStorage {
            client,
            config: self.config,
        })
    }
}