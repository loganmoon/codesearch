//! Code Context CLI - Semantic Code Indexing System
//!
//! This binary provides the command-line interface for the codesearch system.

#![deny(warnings)]
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]

mod docker;
mod storage_init;

use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand};
use codesearch_core::config::{Config, StorageConfig};
use codesearch_embeddings::EmbeddingManager;
use codesearch_indexer::{Indexer, RepositoryIndexer};
use std::env;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::{info, warn};

/// Convert provider string to EmbeddingProviderType enum
fn parse_provider_type(provider: &str) -> codesearch_embeddings::EmbeddingProviderType {
    match provider.to_lowercase().as_str() {
        "localapi" | "api" => codesearch_embeddings::EmbeddingProviderType::LocalApi,
        "mock" => codesearch_embeddings::EmbeddingProviderType::Mock,
        _ => codesearch_embeddings::EmbeddingProviderType::LocalApi, // Default to LocalApi
    }
}

/// Create default storage configuration for a repository
fn create_default_storage_config(collection_name: String) -> StorageConfig {
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
    }
}

/// Ensure config has a collection name, generating one if needed
fn ensure_collection_name(mut config: Config, repo_root: &Path) -> Result<Config> {
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
fn get_api_base_url_if_local_api(config: &Config) -> Option<&str> {
    let provider_type = parse_provider_type(&config.embeddings.provider);
    if matches!(
        provider_type,
        codesearch_embeddings::EmbeddingProviderType::LocalApi
    ) {
        config.embeddings.api_base_url.as_deref()
    } else {
        None
    }
}

/// Helper function to create an embedding manager from configuration
async fn create_embedding_manager(config: &Config) -> Result<Arc<EmbeddingManager>> {
    let mut embeddings_config_builder = codesearch_embeddings::EmbeddingConfigBuilder::default()
        .provider(parse_provider_type(&config.embeddings.provider))
        .model(config.embeddings.model.clone())
        .batch_size(config.embeddings.batch_size)
        .embedding_dimension(config.embeddings.embedding_dimension)
        .device(match config.embeddings.device.as_str() {
            "cuda" => codesearch_embeddings::DeviceType::Cuda,
            _ => codesearch_embeddings::DeviceType::Cpu,
        });

    if let Some(ref api_base_url) = config.embeddings.api_base_url {
        embeddings_config_builder = embeddings_config_builder.api_base_url(api_base_url.clone());
    }

    let api_key = config
        .embeddings
        .api_key
        .clone()
        .or_else(|| std::env::var("VLLM_API_KEY").ok());
    if let Some(key) = api_key {
        embeddings_config_builder = embeddings_config_builder.api_key(key);
    }

    let embeddings_config = embeddings_config_builder.build();

    let embedding_manager = codesearch_embeddings::EmbeddingManager::from_config(embeddings_config)
        .await
        .context("Failed to create embedding manager")?;

    Ok(Arc::new(embedding_manager))
}

/// Ensure storage is initialized, creating config and collection if needed
async fn ensure_storage_initialized(
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
        let config = Config::builder().storage(storage_config).build()?;

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
        let api_base_url = get_api_base_url_if_local_api(&config);
        docker::ensure_dependencies_running(&config.storage, api_base_url).await?;
    }

    // Create embedding manager to get dimensions
    let embedding_manager = create_embedding_manager(&config).await?;

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

#[derive(Parser)]
#[command(name = "codesearch")]
#[command(about = "Semantic code indexing and RAG system")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Configuration file path
    #[arg(short, long, value_name = "FILE", global = true)]
    config: Option<PathBuf>,

    /// Verbose logging
    #[arg(short, long, global = true)]
    verbose: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Start MCP server with semantic code search
    Serve,
    /// Index the repository
    Index {
        /// Force re-indexing of all files
        #[arg(long)]
        force: bool,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize logging
    init_logging(cli.verbose)?;

    // Execute commands
    match cli.command {
        Some(Commands::Serve) => {
            // Find repository root
            let repo_root = find_repository_root()?;
            serve(&repo_root, cli.config.as_deref()).await
        }
        Some(Commands::Index { force }) => {
            // Find repository root
            let repo_root = find_repository_root()?;
            index_repository(&repo_root, cli.config.as_deref(), force).await
        }
        None => {
            // Default behavior - show help
            println!("Run 'codesearch serve' to start the MCP server, or --help for more options");
            Ok(())
        }
    }
}

/// Initialize logging system
fn init_logging(verbose: bool) -> Result<()> {
    let level = if verbose { "debug" } else { "info" };

    tracing_subscriber::fmt()
        .with_env_filter(format!(
            "codesearch={level},{}={level}",
            env!("CARGO_PKG_NAME")
        ))
        .init();

    Ok(())
}

/// Find the repository root directory
fn find_repository_root() -> Result<PathBuf> {
    let current_dir = env::current_dir().context("Failed to get current directory")?;

    // Walk up the directory tree looking for .git
    let mut dir = current_dir.as_path();
    loop {
        let git_dir = dir.join(".git");
        if git_dir.exists() {
            // Check if it's a regular git repo or a worktree/submodule
            if git_dir.is_dir() {
                // Regular git repository
                return Ok(dir.to_path_buf());
            } else if git_dir.is_file() {
                // Worktree or submodule - read the gitdir pointer
                let contents =
                    std::fs::read_to_string(&git_dir).context("Failed to read .git file")?;
                if let Some(_gitdir_line) = contents.lines().find(|l| l.starts_with("gitdir:")) {
                    // This is a worktree/submodule, but we still use this as the root
                    return Ok(dir.to_path_buf());
                }
            }
        }

        // Move up one directory
        dir = dir
            .parent()
            .ok_or_else(|| anyhow!("Not inside a git repository (reached filesystem root)"))?;
    }
}

/// Start the MCP server
async fn serve(repo_root: &Path, config_path: Option<&Path>) -> Result<()> {
    info!("Preparing to start MCP server...");

    // Ensure storage is initialized
    let config = ensure_storage_initialized(repo_root, config_path).await?;

    // Check if repository has been indexed
    let postgres_client = codesearch_storage::create_postgres_client(&config.storage)
        .await
        .context("Failed to connect to Postgres")?;

    let repository_id = postgres_client
        .get_repository_id(&config.storage.collection_name)
        .await
        .context("Failed to query repository")?
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Repository not found for collection '{}'. This is unexpected after initialization.",
                config.storage.collection_name
            )
        })?;

    // Check if repository has been indexed by checking for last indexed commit
    let last_indexed_commit = postgres_client
        .get_last_indexed_commit(repository_id)
        .await
        .context("Failed to check indexing status")?;

    if last_indexed_commit.is_none() {
        info!("Repository not yet indexed. Running initial indexing...");

        // Run indexing inline (we already have repo_root and config)
        let embedding_manager = create_embedding_manager(&config).await?;

        let git_repo = match codesearch_watcher::GitRepository::open(repo_root) {
            Ok(repo) => {
                info!("Git repository detected");
                Some(repo)
            }
            Err(e) => {
                warn!("Not a Git repository or failed to open: {e}");
                None
            }
        };

        let mut indexer = RepositoryIndexer::new(
            repo_root.to_path_buf(),
            repository_id.to_string(),
            embedding_manager,
            postgres_client.clone(),
            git_repo,
        );

        let result = indexer
            .index_repository()
            .await
            .context("Failed to index repository")?;

        info!("Initial indexing completed successfully");
        info!("  Files processed: {}", result.stats().total_files());
        info!(
            "  Entities extracted: {}",
            result.stats().entities_extracted()
        );
    } else {
        info!(
            "Repository already indexed (last commit: {})",
            last_indexed_commit.as_deref().unwrap_or("unknown")
        );
    }

    info!("Starting MCP server...");

    // Delegate to server crate
    codesearch_server::run_server(config)
        .await
        .map_err(|e| anyhow!("MCP server error: {e}"))
}

/// Index the repository
async fn index_repository(
    repo_root: &Path,
    config_path: Option<&Path>,
    _force: bool,
) -> Result<()> {
    info!("Starting repository indexing");

    // Ensure storage is initialized (creates config, collection, runs migrations if needed)
    let config = ensure_storage_initialized(repo_root, config_path).await?;

    // Create embedding manager
    let embedding_manager = create_embedding_manager(&config).await?;

    // Create postgres client
    let postgres_client = codesearch_storage::create_postgres_client(&config.storage)
        .await
        .context("Failed to connect to Postgres")?;

    // Get repository_id from database
    let repository_id = postgres_client
        .get_repository_id(&config.storage.collection_name)
        .await
        .context("Failed to query repository")?
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Repository not found for collection '{}'. This is unexpected after initialization.",
                config.storage.collection_name
            )
        })?;

    info!("Repository ID: {}", repository_id);

    // Create GitRepository if possible
    let git_repo = match codesearch_watcher::GitRepository::open(repo_root) {
        Ok(repo) => {
            info!("Git repository detected");
            Some(repo)
        }
        Err(e) => {
            warn!("Not a Git repository or failed to open: {e}");
            None
        }
    };

    // Create and run indexer
    let mut indexer = RepositoryIndexer::new(
        repo_root.to_path_buf(),
        repository_id.to_string(),
        embedding_manager,
        postgres_client,
        git_repo,
    );

    // Run indexing
    let result = indexer
        .index_repository()
        .await
        .context("Failed to index repository")?;

    // Report statistics
    info!("Indexing completed successfully");
    info!("  Files processed: {}", result.stats().total_files());
    info!(
        "  Entities extracted: {}",
        result.stats().entities_extracted()
    );
    info!("  Failed files: {}", result.stats().failed_files());
    info!(
        "  Duration: {:.2}s",
        result.stats().processing_time_ms() as f64 / 1000.0
    );

    if result.stats().failed_files() > 0 && !result.errors().is_empty() {
        warn!("Errors encountered during indexing:");
        for err in result.errors().iter().take(5) {
            warn!("  - {}", err);
        }
        if result.errors().len() > 5 {
            warn!("  ... and {} more errors", result.errors().len() - 5);
        }
    }

    Ok(())
}
