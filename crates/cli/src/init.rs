//! Storage initialization module
//!
//! This module contains the storage initialization logic that sets up
//! configuration, Docker containers, database migrations, and repository registration.

use anyhow::{Context, Result};
use codesearch_core::config::{default_max_entities_per_db_operation, Config, StorageConfig};
use std::env;
use std::path::Path;
use tracing::info;

use crate::{docker, infrastructure, storage_init};

/// Get API base URL if provider is LocalApi, None otherwise
pub fn get_api_base_url_if_local_api(config: &Config) -> Option<&str> {
    // Check if provider is LocalApi (matches "localapi" or "api")
    let provider_lower = config.embeddings.provider.to_lowercase();
    if provider_lower == "localapi" || provider_lower == "api" {
        config.embeddings.api_base_url.as_deref()
    } else {
        None
    }
}

/// Ensure storage is initialized, creating config and collection if needed
///
/// This function orchestrates the complete initialization flow:
/// 1. Loads config using layered approach (global + local)
/// 2. Creates local config file if missing (without collection_name)
/// 3. Starts Docker dependencies if auto_start_deps enabled
/// 4. Connects to PostgreSQL and runs migrations
/// 5. Generates collection name from repository path
/// 6. Registers repository in Postgres or verifies existing registration
/// 7. Creates embedding manager
/// 8. Initializes Qdrant collection
///
/// Returns (Config, collection_name) tuple
pub async fn ensure_storage_initialized(
    repo_root: &Path,
    config_path: Option<&Path>,
) -> Result<(Config, String)> {
    let current_dir = env::current_dir()?;

    // Determine local config file path
    let config_file = if let Some(path) = config_path {
        path.to_path_buf()
    } else {
        current_dir.join("codesearch.toml")
    };

    // If local config doesn't exist, create it with minimal settings (NO collection_name)
    if !config_file.exists() {
        let storage_config = StorageConfig {
            qdrant_host: "localhost".to_string(),
            qdrant_port: 6334,
            qdrant_rest_port: 6333,
            auto_start_deps: true,
            docker_compose_file: None,
            postgres_host: "localhost".to_string(),
            postgres_port: 5432,
            postgres_database: "codesearch".to_string(),
            postgres_user: "codesearch".to_string(),
            postgres_password: "codesearch".to_string(),
            max_entities_per_db_operation: default_max_entities_per_db_operation(),
        };

        let config = Config::builder(storage_config).build();
        config
            .save(&config_file)
            .with_context(|| format!("Failed to save config to {config_file:?}"))?;
        info!("Created local configuration at {:?}", config_file);
    }

    // Load configuration using layered approach
    let (config, sources) = Config::load_layered(Some(&config_file))?;

    // Log which configs were loaded
    if sources.global_loaded {
        if let Some(ref path) = sources.global_path {
            info!("Loaded global config: {}", path.display());
        }
    }
    if sources.local_loaded {
        if let Some(ref path) = sources.local_path {
            info!("Loaded local config: {}", path.display());
        }
    }

    config.validate()?;

    // Ensure dependencies are running if auto-start is enabled
    if config.storage.auto_start_deps {
        infrastructure::ensure_shared_infrastructure(&config.storage).await?;
        let api_base_url = get_api_base_url_if_local_api(&config);
        docker::ensure_dependencies_running(&config.storage, api_base_url).await?;
    }

    // Connect to PostgreSQL and run migrations
    let postgres_client = codesearch_storage::create_postgres_client(&config.storage)
        .await
        .context("Failed to create PostgreSQL client")?;

    postgres_client
        .run_migrations()
        .await
        .context("Failed to run database migrations")?;

    info!("Database migrations completed");

    // Generate collection name deterministically from repository path
    let collection_name = StorageConfig::generate_collection_name(repo_root)?;
    info!("Generated collection name: {}", collection_name);

    // Check if repository exists in database
    let repository_info = postgres_client
        .get_repository_by_path(repo_root)
        .await
        .context("Failed to query repository by path")?;

    let repository_id = match repository_info {
        Some((repo_id, db_collection_name)) => {
            // Repository exists - verify collection name matches
            if db_collection_name != collection_name {
                return Err(anyhow::anyhow!(
                    "Collection name mismatch for repository at {}:\n\
                    Expected: {}\n\
                    Found in database: {}\n\n\
                    This usually means the repository path has changed.\n\
                    Please run 'codesearch drop' to remove old data and re-index.",
                    repo_root.display(),
                    collection_name,
                    db_collection_name
                ));
            }
            info!("Repository already registered with ID: {repo_id}");
            repo_id
        }
        None => {
            // Register new repository in database
            info!("Registering new repository in database...");
            postgres_client
                .ensure_repository(repo_root, &collection_name, None)
                .await
                .context("Failed to register repository")?
        }
    };

    // Create embedding manager to get dimensions
    let embedding_manager = crate::create_embedding_manager(&config).await?;
    let dimensions = embedding_manager.provider().embedding_dimension();
    info!("Embedding model dimensions: {}", dimensions);

    // Create collection manager and initialize Qdrant collection
    let collection_manager = storage_init::create_collection_manager_with_retry(&config.storage)
        .await
        .context("Failed to create collection manager")?;

    // Default sparse vocab size for BM25
    const SPARSE_VOCAB_SIZE: u32 = 100_000;

    storage_init::initialize_collection(
        collection_manager.as_ref(),
        &collection_name,
        dimensions,
        SPARSE_VOCAB_SIZE,
    )
    .await
    .context("Failed to initialize collection")?;

    // Perform health check
    storage_init::verify_storage_health(collection_manager.as_ref())
        .await
        .context("Storage backend verification failed")?;

    info!("Storage initialized successfully");
    info!("  Collection: {collection_name}");
    info!("  Repository ID: {repository_id}");

    // Return config AND collection_name separately
    Ok((config, collection_name))
}
