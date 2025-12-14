//! Library interface for codesearch CLI
//!
//! This module exposes internal functions for integration testing while keeping
//! the main binary logic in main.rs.

pub mod docker;
pub mod infrastructure;
pub mod storage_init;

// Public module for storage initialization
pub mod init;

// Re-export commonly needed types for tests
pub use anyhow::Result;
pub use codesearch_core::config::Config;
pub use std::path::Path;

use anyhow::Context;
use codesearch_storage::PostgresClientTrait;
use init::get_api_base_url_if_local_api;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{info, warn};
use uuid::Uuid;

/// Result of shared initialization for multi-repository commands
pub struct InitializedBackends {
    /// Loaded and validated configuration
    pub config: Config,
    /// PostgreSQL client
    pub postgres_client: Arc<dyn PostgresClientTrait>,
    /// Valid repositories (id, collection_name, path)
    pub valid_repos: Vec<(Uuid, String, PathBuf)>,
}

/// Shared initialization for commands that need repository access
///
/// Performs:
/// - Configuration loading and validation
/// - Infrastructure startup (if auto_start_deps enabled)
/// - PostgreSQL connection
/// - Repository loading and path validation
pub async fn initialize_backends(config_path: Option<&Path>) -> Result<InitializedBackends> {
    // Load configuration
    let config = Config::load(config_path)?;
    config.validate()?;

    // Ensure infrastructure is running
    if config.storage.auto_start_deps {
        let vllm_reqs = infrastructure::VllmRequirements::from_config(&config);
        infrastructure::ensure_shared_infrastructure(&config.storage, vllm_reqs).await?;
        let api_base_url = get_api_base_url_if_local_api(&config);
        docker::ensure_dependencies_running(&config.storage, api_base_url).await?;
    }

    // Connect to PostgreSQL
    let postgres_client = codesearch_storage::create_postgres_client(&config.storage)
        .await
        .context("Failed to connect to Postgres")?;

    // Load ALL indexed repositories from database
    let all_repos = postgres_client
        .list_all_repositories()
        .await
        .context("Failed to list repositories")?;

    if all_repos.is_empty() {
        anyhow::bail!(
            "No indexed repositories found.\n\
            Run 'codesearch index' from a git repository to create an index."
        );
    }

    info!("Found {} indexed repositories", all_repos.len());

    // Filter out repositories with non-existent paths
    let valid_repos: Vec<_> = all_repos
        .into_iter()
        .filter(|(repo_id, collection_name, path)| {
            if path.exists() {
                info!(
                    "  - {} ({}) at {}",
                    collection_name,
                    repo_id,
                    path.display()
                );
                true
            } else {
                warn!(
                    "Skipping repository '{}' - path {} no longer exists",
                    collection_name,
                    path.display()
                );
                false
            }
        })
        .collect();

    if valid_repos.is_empty() {
        anyhow::bail!(
            "No valid repositories found to serve.\n\
            All indexed repositories have non-existent paths."
        );
    }

    Ok(InitializedBackends {
        config,
        postgres_client,
        valid_repos,
    })
}

/// Helper function to create an embedding manager from configuration
///
/// This now delegates to the shared implementation in codesearch-embeddings crate.
pub async fn create_embedding_manager(
    config: &Config,
) -> Result<std::sync::Arc<codesearch_embeddings::EmbeddingManager>> {
    codesearch_embeddings::create_embedding_manager_from_app_config(&config.embeddings)
        .await
        .map_err(Into::into)
}
