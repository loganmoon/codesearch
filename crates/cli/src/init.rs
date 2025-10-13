//! Storage initialization module
//!
//! This module contains the storage initialization logic that sets up
//! configuration, Docker containers, database migrations, and repository registration.

use anyhow::{Context, Result};
use codesearch_core::config::{Config, StorageConfig};
use std::env;
use std::path::Path;
use tracing::info;

use crate::{docker, infrastructure, storage_init};

/// Create default storage configuration for a repository
pub fn create_default_storage_config(collection_name: String) -> StorageConfig {
    StorageConfig {
        qdrant_host: "localhost".to_string(),
        qdrant_port: 6334,
        qdrant_rest_port: 6333,
        collection_name,
        auto_start_deps: true,
        docker_compose_file: None,
        postgres_host: "localhost".to_string(),
        postgres_port: 5432,
        postgres_database: "codesearch".to_string(),
        postgres_user: "codesearch".to_string(),
        postgres_password: "codesearch".to_string(),
        max_entity_batch_size: 1000,
    }
}

/// Ensure config has a collection name, generating one if needed
pub fn ensure_collection_name(mut config: Config, repo_root: &Path) -> Result<Config> {
    if config.storage.collection_name.is_empty() {
        config.storage.collection_name = StorageConfig::generate_collection_name(repo_root)?;
        info!(
            "Generated collection name: {}",
            config.storage.collection_name
        );
    }
    Ok(config)
}

/// Get API base URL if provider is LocalApi, None otherwise
pub fn get_api_base_url_if_local_api(config: &Config) -> Option<&str> {
    let provider_type = crate::parse_provider_type(&config.embeddings.provider);
    if matches!(
        provider_type,
        codesearch_embeddings::EmbeddingProviderType::LocalApi
    ) {
        config.embeddings.api_base_url.as_deref()
    } else {
        None
    }
}

/// Ensure storage is initialized, creating config and collection if needed
///
/// This function orchestrates the complete initialization flow:
/// 1. Creates config file if missing
/// 2. Generates collection name if empty
/// 3. Starts Docker dependencies if auto_start_deps enabled
/// 4. Creates embedding manager
/// 5. Initializes Qdrant collection
/// 6. Runs database migrations
/// 7. Registers repository in Postgres
pub async fn ensure_storage_initialized(
    repo_root: &Path,
    config_path: Option<&Path>,
) -> Result<Config> {
    let current_dir = env::current_dir()?;

    // Create default configuration if it doesn't exist
    let config_file = if let Some(path) = config_path {
        path.to_path_buf()
    } else {
        current_dir.join("codesearch.toml")
    };

    if !config_file.exists() {
        // Generate collection name from repository path
        let collection_name = StorageConfig::generate_collection_name(repo_root)?;
        info!("Generated collection name: {}", collection_name);

        let storage_config = create_default_storage_config(collection_name);
        let config = Config::builder(storage_config).build();

        config
            .save(&config_file)
            .with_context(|| format!("Failed to save config to {config_file:?}"))?;
        info!("Created default configuration at {:?}", config_file);
    }

    // Load configuration
    let config = Config::from_file(&config_file)?;

    // Ensure collection name is set
    let needs_save = config.storage.collection_name.is_empty();
    let config = ensure_collection_name(config, repo_root)?;
    if needs_save {
        config.save(&config_file)?;
    }

    config.validate()?;

    // Ensure dependencies are running if auto-start is enabled
    if config.storage.auto_start_deps {
        // First, ensure shared infrastructure is running (or start it if needed)
        infrastructure::ensure_shared_infrastructure(&config.storage).await?;

        // Then ensure all services are healthy
        let api_base_url = get_api_base_url_if_local_api(&config);
        docker::ensure_dependencies_running(&config.storage, api_base_url).await?;
    }

    // Create embedding manager to get dimensions
    let embedding_manager = crate::create_embedding_manager(&config).await?;

    // Get embedding dimensions from the provider
    let dimensions = embedding_manager.provider().embedding_dimension();
    info!("Embedding model dimensions: {}", dimensions);

    // Create collection manager with retry logic
    let collection_manager = storage_init::create_collection_manager_with_retry(&config.storage)
        .await
        .context("Failed to create collection manager")?;

    // Initialize collection with proper error handling
    storage_init::initialize_collection(
        collection_manager.as_ref(),
        &config.storage.collection_name,
        dimensions,
    )
    .await
    .context("Failed to initialize collection")?;

    // Perform health check with diagnostics
    storage_init::verify_storage_health(collection_manager.as_ref())
        .await
        .context("Storage backend verification failed")?;

    // Create PostgresClient and run migrations
    let postgres_client = codesearch_storage::create_postgres_client(&config.storage)
        .await
        .context("Failed to create PostgreSQL client")?;

    postgres_client
        .run_migrations()
        .await
        .context("Failed to run database migrations")?;

    info!("Database migrations completed");

    // Register repository in Postgres
    let repository_id = postgres_client
        .ensure_repository(repo_root, &config.storage.collection_name, None)
        .await
        .context("Failed to register repository")?;

    info!("Repository registered with ID: {}", repository_id);
    info!("Storage initialized successfully");
    info!("  Collection: {}", config.storage.collection_name);
    info!("  Dimensions: {}", dimensions);

    Ok(config)
}
