//! Storage initialization helpers for the CLI
//!
//! This module provides helper functions for initializing storage backends
//! with proper error handling and retry logic.

use anyhow::{Context, Result};
use codesearch_core::config::StorageConfig;
use codesearch_storage::CollectionManager;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{info, warn};

/// Maximum number of retries for storage operations
const MAX_RETRIES: u32 = 3;

/// Delay between retries in seconds
const RETRY_DELAY_SECS: u64 = 2;

/// Create collection manager with retry logic
pub async fn create_collection_manager_with_retry(
    config: &StorageConfig,
) -> Result<Arc<dyn CollectionManager>> {
    let mut attempt = 0;

    loop {
        attempt += 1;

        match codesearch_storage::create_collection_manager(config).await {
            Ok(manager) => {
                info!("Successfully connected to storage backend");
                return Ok(manager);
            }
            Err(e) if attempt < MAX_RETRIES => {
                warn!(
                    "Failed to connect to storage backend (attempt {}/{}): {}",
                    attempt, MAX_RETRIES, e
                );
                info!("Retrying in {} seconds...", RETRY_DELAY_SECS);
                sleep(Duration::from_secs(RETRY_DELAY_SECS)).await;
            }
            Err(e) => {
                return Err(e).context(format!(
                    "Failed to connect to storage backend after {MAX_RETRIES} attempts"
                ))
            }
        }
    }
}

/// Initialize collection with proper error handling
pub async fn initialize_collection(
    manager: &dyn CollectionManager,
    collection_name: &str,
    dense_dimensions: usize,
) -> Result<()> {
    // Check if collection already exists
    let exists = manager
        .collection_exists(collection_name)
        .await
        .context("Failed to check if collection exists")?;

    if exists {
        info!(
            "Collection '{}' already exists, verifying configuration...",
            collection_name
        );
    } else {
        info!(
            "Creating new collection '{}' with {} dense dimensions...",
            collection_name, dense_dimensions
        );
    }

    // Ensure collection with proper dimensions
    manager
        .ensure_collection(collection_name, dense_dimensions)
        .await
        .context("Failed to ensure collection")?;

    info!("Collection '{}' is ready", collection_name);
    Ok(())
}

/// Perform health check with detailed diagnostics
pub async fn verify_storage_health(manager: &dyn CollectionManager) -> Result<()> {
    info!("Performing storage backend health check...");

    manager
        .health_check()
        .await
        .context("Storage backend health check failed")?;

    info!("Storage backend is healthy");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constants() {
        assert_eq!(MAX_RETRIES, 3);
        assert_eq!(RETRY_DELAY_SECS, 2);
    }
}
