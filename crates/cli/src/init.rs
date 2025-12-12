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
/// 8. Initializes Qdrant collection (or drops and recreates if force=true)
///
/// Returns (Config, collection_name) tuple
pub async fn ensure_storage_initialized(
    repo_root: &Path,
    config_path: Option<&Path>,
    force: bool,
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
            neo4j_host: "localhost".to_string(),
            neo4j_http_port: 7474,
            neo4j_bolt_port: 7687,
            neo4j_user: "neo4j".to_string(),
            neo4j_password: "codesearch".to_string(),
            max_entities_per_db_operation: default_max_entities_per_db_operation(),
            postgres_pool_size: 20,
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
        let vllm_reqs = infrastructure::VllmRequirements::from_config(&config);
        infrastructure::ensure_shared_infrastructure(&config.storage, vllm_reqs).await?;
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

    // Generate deterministic repository ID from repository path
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
            // Register new repository in database with deterministic UUID
            info!("Registering new repository in database...");
            let repo_id = postgres_client
                .ensure_repository(repo_root, &collection_name, None)
                .await
                .context("Failed to register repository")?;
            info!("Generated deterministic repository ID: {repo_id}");
            repo_id
        }
    };

    // If force mode, delete all repository data to ensure clean rebuild
    if force {
        info!("Force mode enabled: deleting existing repository data...");

        // Delete file snapshots
        let deleted_snapshots =
            sqlx::query("DELETE FROM file_entity_snapshots WHERE repository_id = $1")
                .bind(repository_id)
                .execute(postgres_client.get_pool())
                .await
                .context("Failed to delete file snapshots")?
                .rows_affected();

        info!("Deleted {} file snapshots", deleted_snapshots);

        // Delete entities and related data
        let deleted_entities = sqlx::query("DELETE FROM entity_metadata WHERE repository_id = $1")
            .bind(repository_id)
            .execute(postgres_client.get_pool())
            .await
            .context("Failed to delete entities")?
            .rows_affected();

        info!("Deleted {} entities", deleted_entities);

        // Delete outbox entries for this repository
        let deleted_outbox = sqlx::query("DELETE FROM entity_outbox WHERE collection_name = $1")
            .bind(&collection_name)
            .execute(postgres_client.get_pool())
            .await
            .context("Failed to delete outbox entries")?
            .rows_affected();

        info!("Deleted {} outbox entries", deleted_outbox);

        // Reset BM25 statistics to defaults
        sqlx::query(
            "UPDATE repositories
             SET bm25_avgdl = 50.0, bm25_total_tokens = 0, bm25_entity_count = 0, last_indexed_commit = NULL
             WHERE repository_id = $1"
        )
        .bind(repository_id)
        .execute(postgres_client.get_pool())
        .await
        .context("Failed to reset BM25 statistics")?;

        info!("Reset BM25 statistics to defaults");
    }

    // Create embedding manager to get dimensions
    let embedding_manager = crate::create_embedding_manager(&config).await?;
    let dimensions = embedding_manager.provider().embedding_dimension();
    info!("Embedding model dimensions: {}", dimensions);

    // Create collection manager and initialize Qdrant collection
    let collection_manager = storage_init::create_collection_manager_with_retry(&config.storage)
        .await
        .context("Failed to create collection manager")?;

    storage_init::initialize_collection(
        collection_manager.as_ref(),
        &collection_name,
        dimensions,
        force,
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
