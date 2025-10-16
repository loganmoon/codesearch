//! Storage initialization module
//!
//! This module contains the storage initialization logic that sets up
//! configuration, Docker containers, database migrations, and repository registration.

use anyhow::{Context, Result};
use codesearch_core::config::{default_storage_max_entity_batch_size, Config, StorageConfig};
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
        max_entity_batch_size: default_storage_max_entity_batch_size(),
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
/// 2. Creates local config file if missing
/// 3. Generates collection name if empty
/// 4. Starts Docker dependencies if auto_start_deps enabled
/// 5. Creates embedding manager
/// 6. Initializes Qdrant collection
/// 7. Runs database migrations
/// 8. Registers repository in Postgres
pub async fn ensure_storage_initialized(
    repo_root: &Path,
    config_path: Option<&Path>,
) -> Result<Config> {
    let current_dir = env::current_dir()?;

    // Determine local config file path
    let config_file = if let Some(path) = config_path {
        path.to_path_buf()
    } else {
        current_dir.join("codesearch.toml")
    };

    // If local config doesn't exist, create it with minimal settings
    if !config_file.exists() {
        // Generate collection name from repository path
        let collection_name = StorageConfig::generate_collection_name(repo_root)?;
        info!("Generated collection name: {}", collection_name);

        let storage_config = create_default_storage_config(collection_name);
        let config = Config::builder(storage_config).build();

        config
            .save(&config_file)
            .with_context(|| format!("Failed to save config to {config_file:?}"))?;
        info!("Created local configuration at {:?}", config_file);
    }

    // Load configuration using layered approach
    let (mut config, sources) = Config::load_layered(Some(&config_file))?;

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

    // Ensure collection name is set
    let needs_save = config.storage.collection_name.is_empty();
    config = ensure_collection_name(config, repo_root)?;
    if needs_save {
        // Only save to local config, not global
        config.save(&config_file)?;
        info!("Updated local config with collection name");
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

/// Load configuration for the serve command
///
/// This function uses layered configuration loading (global + local + env vars) and supports
/// overriding the collection name via CLI flag.
///
/// **Note:** If `auto_start_deps` is enabled in the config, this function will start Docker
/// containers (Postgres, Qdrant, vLLM, outbox-processor) via `docker compose`. It does not
/// create or modify configuration files or databases, but it can create/start Docker containers.
///
/// It expects that `codesearch index` has already been run to set up the initial configuration.
pub async fn load_config_for_serve(
    config_path: Option<&Path>,
    collection_override: Option<&str>,
) -> Result<Config> {
    // Load configuration using layered approach
    let (mut config, sources) = Config::load_layered(config_path)?;

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

    // Apply collection override if provided
    if let Some(collection) = collection_override {
        info!("Using collection from --collection flag: {}", collection);
        config.storage.collection_name = collection.to_string();
    }

    // Ensure collection name is set
    if config.storage.collection_name.is_empty() {
        anyhow::bail!(
            "No collection name specified.\n\
            Either:\n\
            1. Run 'codesearch serve --collection <name>' to specify a collection\n\
            2. Run 'codesearch serve' from a repository directory with codesearch.toml\n\
            3. Create ~/.codesearch/config.toml with a collection_name\n\
            \n\
            Run 'codesearch index' from a git repository to create a collection."
        );
    }

    // Validate the config
    config.validate()?;

    // Ensure infrastructure is running if auto-start is enabled
    if config.storage.auto_start_deps {
        infrastructure::ensure_shared_infrastructure(&config.storage).await?;

        let api_base_url = get_api_base_url_if_local_api(&config);
        docker::ensure_dependencies_running(&config.storage, api_base_url).await?;
    }

    Ok(config)
}
