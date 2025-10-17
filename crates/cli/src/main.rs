//! Code Context CLI - Semantic Code Indexing System
//!
//! This binary provides the command-line interface for the codesearch system.

#![deny(warnings)]
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]

// Use the library modules
use codesearch::init::{ensure_storage_initialized, get_api_base_url_if_local_api};
use codesearch::{docker, infrastructure};

use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand};
use codesearch_core::config::{Config, StorageConfig};
use codesearch_indexer::{Indexer, RepositoryIndexer};
use std::env;
use std::path::{Path, PathBuf};
use tracing::{info, warn};

// Re-use create_embedding_manager from lib
use codesearch::create_embedding_manager;

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
    /// Drop all indexed data from storage (requires confirmation)
    Drop,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize logging
    init_logging(cli.verbose)?;

    // Execute commands
    match cli.command {
        Some(Commands::Serve) => serve(cli.config.as_deref()).await,
        Some(Commands::Index { force }) => {
            // Find repository root
            let repo_root = find_repository_root()?;
            index_repository(&repo_root, cli.config.as_deref(), force).await
        }
        Some(Commands::Drop) => {
            // Find repository root
            let repo_root = find_repository_root()?;
            drop_data(&repo_root, cli.config.as_deref()).await
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

/// Start the MCP server (multi-repository mode)
async fn serve(config_path: Option<&Path>) -> Result<()> {
    info!("Preparing to start multi-repository MCP server...");

    // Load configuration (no collection_name needed)
    let (config, _sources) = Config::load_layered(config_path)?;
    config.validate()?;

    // Ensure infrastructure is running
    if config.storage.auto_start_deps {
        infrastructure::ensure_shared_infrastructure(&config.storage).await?;
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

    info!("Found {} indexed repositories:", all_repos.len());
    for (repo_id, collection_name, path) in &all_repos {
        info!(
            "  - {} ({}) at {}",
            collection_name,
            repo_id,
            path.display()
        );

        // Verify repository path still exists
        if !path.exists() {
            warn!(
                "Repository path {} no longer exists (may have been moved or deleted)",
                path.display()
            );
        }
    }

    info!("Starting multi-repository MCP server...");

    // Delegate to multi-repository server
    codesearch_server::run_multi_repo_server(config, all_repos, postgres_client)
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
    let (config, collection_name) = ensure_storage_initialized(repo_root, config_path).await?;

    // Create embedding manager
    let embedding_manager = create_embedding_manager(&config).await?;

    // Create postgres client
    let postgres_client = codesearch_storage::create_postgres_client(&config.storage)
        .await
        .context("Failed to connect to Postgres")?;

    // Get repository_id from database
    let repository_id = postgres_client
        .get_repository_id(&collection_name)
        .await
        .context("Failed to query repository")?
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Repository not found for collection '{collection_name}'. This is unexpected after initialization."
            )
        })?;

    info!(
        repository_id = %repository_id,
        collection_name = %collection_name,
        "Repository ID retrieved for indexing"
    );

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

    // Convert core config to indexer config
    let indexer_config = codesearch_indexer::IndexerConfig::new()
        .with_index_batch_size(config.indexer.files_per_discovery_batch)
        .with_channel_buffer_size(config.indexer.pipeline_channel_capacity)
        .with_max_entity_batch_size(config.indexer.entities_per_embedding_batch)
        .with_file_extraction_concurrency(config.indexer.max_concurrent_file_extractions)
        .with_snapshot_update_concurrency(config.indexer.max_concurrent_snapshot_updates);

    // Create and run indexer
    tracing::debug!(
        repository_id_string = %repository_id.to_string(),
        "Creating RepositoryIndexer with repository_id"
    );
    let mut indexer = RepositoryIndexer::new(
        repo_root.to_path_buf(),
        repository_id.to_string(),
        embedding_manager,
        postgres_client,
        git_repo,
        indexer_config,
    )?;

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
            warn!("  - {:?}", err);
        }
        if result.errors().len() > 5 {
            warn!("  ... and {} more errors", result.errors().len() - 5);
        }
    }

    Ok(())
}

/// Drop all indexed data from storage
async fn drop_data(repo_root: &Path, config_path: Option<&Path>) -> Result<()> {
    info!("Preparing to drop all indexed data");

    // Load configuration (doesn't need collection_name anymore)
    let (config, _sources) = Config::load_layered(config_path)?;
    config.validate()?;

    // Generate collection name from repository path
    let collection_name = StorageConfig::generate_collection_name(repo_root)?;

    // Display warning
    println!("\nWARNING: This will permanently delete all indexed data from:");
    println!("  - Qdrant collection: {collection_name}");
    let postgres_db = &config.storage.postgres_database;
    println!("  - PostgreSQL database: {postgres_db}");
    println!("\nThis action cannot be undone.");
    print!("\nType 'yes' to confirm: ");

    // Flush stdout and read confirmation
    use std::io::{self, Write};
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;

    if input.trim() != "yes" {
        println!("Operation cancelled.");
        return Ok(());
    }

    info!("User confirmed drop operation, proceeding...");

    // Ensure dependencies are running
    if config.storage.auto_start_deps {
        infrastructure::ensure_shared_infrastructure(&config.storage).await?;
        let api_base_url = get_api_base_url_if_local_api(&config);
        docker::ensure_dependencies_running(&config.storage, api_base_url).await?;
    }

    // Create collection manager and delete collection if it exists
    let collection_manager = codesearch_storage::create_collection_manager(&config.storage)
        .await
        .context("Failed to create collection manager")?;

    if collection_manager
        .collection_exists(&collection_name)
        .await?
    {
        info!("Deleting Qdrant collection...");
        collection_manager
            .delete_collection(&collection_name)
            .await
            .context("Failed to delete Qdrant collection")?;
        info!("Qdrant collection deleted successfully");
    } else {
        info!(
            "Qdrant collection '{}' does not exist, skipping",
            collection_name
        );
    }

    // Drop PostgreSQL data
    let postgres_client = codesearch_storage::create_postgres_client(&config.storage)
        .await
        .context("Failed to create PostgreSQL client")?;

    info!("Dropping PostgreSQL data...");
    postgres_client
        .drop_all_data()
        .await
        .context("Failed to drop PostgreSQL data")?;
    info!("PostgreSQL data dropped successfully");

    println!("\nAll indexed data has been successfully removed.");
    println!("You can re-index the repository using 'codesearch index'.");

    Ok(())
}
