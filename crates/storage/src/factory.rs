use crate::{MockStorageClient, Storage};
use codesearch_core::{config::StorageConfig, Error};
use std::sync::Arc;

use crate::qdrant::QdrantStorage;

/// Creates a storage client based on configuration
/// Returns trait objects, hiding implementation details
pub async fn create_storage_client(config: StorageConfig) -> Result<Arc<dyn Storage>, Error> {
    match config.provider.as_str() {
        "qdrant" => {
            let qdrant_storage = QdrantStorage::new(config).await?;
            Ok(Arc::new(qdrant_storage) as Arc<dyn Storage>)
        }
        _ => {
            // Default to mock for unknown providers
            Ok(Arc::new(MockStorageClient::new()) as Arc<dyn Storage>)
        }
    }
}

/// Creates a storage client from config, with initialization
pub async fn create_and_initialize_storage(
    config: StorageConfig,
) -> Result<Arc<dyn Storage>, Error> {
    let client = create_storage_client(config).await?;
    client.initialize().await?;
    Ok(client)
}
