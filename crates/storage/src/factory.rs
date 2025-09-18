use crate::{mock::MockStorageClient, Storage};
use codesearch_core::{config::StorageConfig, Error};
use std::sync::Arc;

use crate::qdrant::QdrantStorageBuilder;

/// Creates a storage client based on configuration.
///
/// This is the primary factory function for creating storage clients.
/// It returns a trait object that hides implementation details, allowing
/// for different storage backends (Qdrant, mock, etc.) to be used interchangeably.
///
/// # Arguments
/// * `config` - Storage configuration specifying provider type and connection details
///
/// # Returns
/// An `Arc<dyn Storage>` trait object that implements both `StorageClient` and `StorageManager`
///
/// # Errors
/// Returns an error if the storage backend cannot be created or connected
///
/// # Example
/// ```ignore
/// let config = StorageConfigBuilder::new()
///     .provider("qdrant")
///     .host("localhost")
///     .port(6334)
///     .build();
///
/// let storage = create_storage_client(config).await?;
/// ```
pub async fn create_storage_client(config: StorageConfig) -> Result<Arc<dyn Storage>, Error> {
    match config.provider.as_str() {
        "qdrant" => {
            let qdrant_storage = QdrantStorageBuilder::from_config(config).build().await?;
            Ok(Arc::new(qdrant_storage) as Arc<dyn Storage>)
        }
        _ => {
            // Default to mock for unknown providers
            Ok(Arc::new(MockStorageClient::new()) as Arc<dyn Storage>)
        }
    }
}

/// Creates and initializes a storage client from configuration.
///
/// This convenience function combines client creation and initialization
/// in a single call. It's equivalent to calling `create_storage_client()`
/// followed by `initialize()`.
///
/// # Arguments
/// * `config` - Storage configuration specifying provider type and connection details
///
/// # Returns
/// An initialized `Arc<dyn Storage>` trait object ready for use
///
/// # Errors
/// Returns an error if the storage backend cannot be created, connected, or initialized
///
/// # Example
/// ```ignore
/// let config = StorageConfig::default();
/// let storage = create_and_initialize_storage(config).await?;
/// // Storage is now ready to use
/// storage.bulk_load_entities(&batch).await?;
/// ```
pub async fn create_and_initialize_storage(
    config: StorageConfig,
) -> Result<Arc<dyn Storage>, Error> {
    let client = create_storage_client(config).await?;
    client.initialize().await?;
    Ok(client)
}
