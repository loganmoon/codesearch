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
use codesearch_core::config::Config;
use codesearch_indexer::{Indexer, RepositoryIndexer};
use dialoguer::{Confirm, Select};
use std::env;
use std::path::{Path, PathBuf};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

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
    /// Manage embedding cache
    #[command(subcommand)]
    Cache(CacheCommands),
}

#[derive(Subcommand)]
enum CacheCommands {
    /// Show cache statistics
    Stats,
    /// Clear cache entries
    Clear {
        /// Only clear entries for a specific model version
        #[arg(long)]
        model: Option<String>,
    },
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
        Some(Commands::Drop) => drop_data(cli.config.as_deref()).await,
        Some(Commands::Cache(cache_cmd)) => {
            handle_cache_command(cache_cmd, cli.config.as_deref()).await
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

    // Run migrations ONCE before starting services
    info!("Running database migrations");
    postgres_client
        .run_migrations()
        .await
        .context("Failed to run database migrations")?;
    info!("Database migrations completed successfully");

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
                    "Skipping repository '{}' ({}) - path {} no longer exists (may have been moved or deleted)",
                    collection_name,
                    repo_id,
                    path.display()
                );
                false
            }
        })
        .collect();

    if valid_repos.is_empty() {
        anyhow::bail!(
            "No valid repositories found to serve.\n\
            All indexed repositories have non-existent paths.\n\
            Run 'codesearch index' from a valid repository directory to re-index."
        );
    }

    // Create Qdrant config for outbox processor
    let qdrant_config = codesearch_storage::QdrantConfig {
        host: config.storage.qdrant_host.clone(),
        port: config.storage.qdrant_port,
        rest_port: config.storage.qdrant_rest_port,
    };

    // Create outbox processor shutdown channel
    let (outbox_shutdown_tx, outbox_shutdown_rx) = tokio::sync::oneshot::channel();

    // Spawn outbox processor as background task
    let postgres_client_clone = postgres_client.clone();
    let storage_config = config.storage.clone();
    let outbox_config = config.outbox.clone();
    let outbox_handle = tokio::spawn(async move {
        if let Err(e) = codesearch_outbox_processor::start_outbox_processor(
            postgres_client_clone,
            &qdrant_config,
            storage_config,
            &outbox_config,
            outbox_shutdown_rx,
        )
        .await
        {
            error!("Outbox processor task failed: {e}");
        }
    });

    info!("Outbox processor started successfully");

    info!(
        "Starting multi-repository MCP server with {} valid repositories",
        valid_repos.len()
    );

    // Delegate to multi-repository server
    let server_result =
        codesearch_server::run_multi_repo_server(config, valid_repos, postgres_client)
            .await
            .map_err(|e| anyhow!("MCP server error: {e}"));

    // Always perform graceful shutdown of outbox processor, regardless of server result
    // This ensures proper cleanup even if the server failed
    info!("Shutting down outbox processor...");
    let _ = outbox_shutdown_tx.send(());

    // Wait for outbox task to complete (with timeout)
    // This wait happens before returning, ensuring cleanup completes
    match tokio::time::timeout(std::time::Duration::from_secs(5), outbox_handle).await {
        Ok(Ok(())) => info!("Outbox processor stopped successfully"),
        Ok(Err(e)) => warn!("Outbox processor task panicked: {e}"),
        Err(_) => warn!("Outbox processor shutdown timed out after 5 seconds"),
    }

    // Return server result after cleanup is complete
    server_result
}

/// Index the repository
async fn index_repository(repo_root: &Path, config_path: Option<&Path>, force: bool) -> Result<()> {
    info!("Starting repository indexing");

    // Ensure storage is initialized (creates config, collection, runs migrations if needed)
    let (config, collection_name) =
        ensure_storage_initialized(repo_root, config_path, force).await?;

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

    // Resolve CONTAINS relationships in Neo4j
    info!("Resolving graph relationships...");
    let neo4j_client = codesearch_storage::create_neo4j_client(&config.storage)
        .await
        .context("Failed to create Neo4j client")?;

    let postgres_client = codesearch_storage::create_postgres_client(&config.storage)
        .await
        .context("Failed to connect to Postgres")?;

    resolve_contains_relationships(&postgres_client, &neo4j_client, repository_id)
        .await
        .context("Failed to resolve CONTAINS relationships")?;

    resolve_trait_implementations(&postgres_client, &neo4j_client, repository_id)
        .await
        .context("Failed to resolve trait implementations")?;

    resolve_class_inheritance(&postgres_client, &neo4j_client, repository_id)
        .await
        .context("Failed to resolve class inheritance")?;

    // Mark graph as ready
    postgres_client
        .set_graph_ready(repository_id, true)
        .await
        .context("Failed to set graph_ready flag")?;

    info!("Graph relationships resolved successfully");

    Ok(())
}

/// Resolve CONTAINS relationships after indexing completes
async fn resolve_contains_relationships(
    postgres: &std::sync::Arc<dyn codesearch_storage::PostgresClientTrait>,
    neo4j: &codesearch_storage::Neo4jClient,
    repository_id: Uuid,
) -> Result<()> {
    // Ensure Neo4j database context
    let db_name = neo4j
        .ensure_repository_database(repository_id, postgres.as_ref())
        .await?;
    neo4j.use_database(&db_name).await?;

    // Find all nodes with unresolved parent
    info!("Searching for unresolved CONTAINS relationships...");

    let unresolved_nodes = neo4j
        .find_unresolved_contains_nodes()
        .await
        .context("Failed to find unresolved nodes")?;

    info!("Found {} unresolved nodes", unresolved_nodes.len());

    if unresolved_nodes.is_empty() {
        return Ok(());
    }

    // Resolve each node
    let mut resolved_count = 0;
    let mut failed_count = 0;

    for (child_id, parent_qname) in unresolved_nodes {
        match neo4j
            .resolve_contains_relationship(&child_id, &parent_qname)
            .await
        {
            Ok(true) => resolved_count += 1,
            Ok(false) => {
                warn!(
                    "Parent '{}' not found for child '{}'",
                    parent_qname, child_id
                );
                failed_count += 1;
            }
            Err(e) => {
                warn!(
                    "Failed to resolve parent '{}' for child '{}': {}",
                    parent_qname, child_id, e
                );
                failed_count += 1;
            }
        }
    }

    info!(
        "Resolved {} CONTAINS relationships ({} failed)",
        resolved_count, failed_count
    );

    Ok(())
}

/// Resolve trait implementations and interface extensions after indexing completes
async fn resolve_trait_implementations(
    postgres: &std::sync::Arc<dyn codesearch_storage::PostgresClientTrait>,
    neo4j: &codesearch_storage::Neo4jClient,
    repository_id: Uuid,
) -> Result<()> {
    use codesearch_core::entities::EntityType;
    use std::collections::HashMap;

    info!("Resolving trait implementations...");

    // Ensure Neo4j database context
    let db_name = neo4j
        .ensure_repository_database(repository_id, postgres.as_ref())
        .await?;
    neo4j.use_database(&db_name).await?;

    // Get all impl blocks
    let impls = postgres
        .get_entities_by_type(repository_id, EntityType::Impl)
        .await
        .context("Failed to get impl blocks")?;

    // Build trait name -> entity_id map
    let traits = postgres
        .get_entities_by_type(repository_id, EntityType::Trait)
        .await
        .context("Failed to get traits")?;
    let trait_map: HashMap<String, String> = traits
        .iter()
        .map(|t| (t.name.clone(), t.entity_id.clone()))
        .collect();

    // Build type name -> entity_id map (for ASSOCIATES)
    let structs = postgres
        .get_entities_by_type(repository_id, EntityType::Struct)
        .await
        .context("Failed to get structs")?;
    let enums = postgres
        .get_entities_by_type(repository_id, EntityType::Enum)
        .await
        .context("Failed to get enums")?;
    let mut type_map: HashMap<String, String> = HashMap::new();
    type_map.extend(
        structs
            .iter()
            .map(|s| (s.name.clone(), s.entity_id.clone())),
    );
    type_map.extend(enums.iter().map(|e| (e.name.clone(), e.entity_id.clone())));

    // Build interface name -> entity_id map (for TypeScript/JavaScript)
    let interfaces = postgres
        .get_entities_by_type(repository_id, EntityType::Interface)
        .await
        .context("Failed to get interfaces")?;
    let interface_map: HashMap<String, String> = interfaces
        .iter()
        .map(|i| (i.name.clone(), i.entity_id.clone()))
        .collect();

    let mut implements_count = 0;
    let mut associates_count = 0;
    let mut extends_count = 0;

    // Resolve IMPLEMENTS and ASSOCIATES relationships from impl blocks
    for impl_entity in impls {
        // Resolve IMPLEMENTS relationship
        if let Some(trait_name) = impl_entity.metadata.attributes.get("implements_trait") {
            if let Some(trait_id) = trait_map.get(trait_name) {
                neo4j
                    .create_relationship(
                        &impl_entity.entity_id,
                        trait_id,
                        "IMPLEMENTS",
                        &HashMap::new(),
                    )
                    .await
                    .context("Failed to create IMPLEMENTS relationship")?;
                implements_count += 1;
            } else {
                debug!("Trait '{trait_name}' not found in repository");
            }
        }

        // Resolve ASSOCIATES relationship
        if let Some(for_type) = impl_entity.metadata.attributes.get("for_type") {
            // Strip generic parameters: "Container<T>" -> "Container"
            let type_name = for_type.split('<').next().unwrap_or(for_type).trim();

            if let Some(type_id) = type_map.get(type_name) {
                neo4j
                    .create_relationship(
                        &impl_entity.entity_id,
                        type_id,
                        "ASSOCIATES",
                        &HashMap::new(),
                    )
                    .await
                    .context("Failed to create ASSOCIATES relationship")?;
                associates_count += 1;
            } else {
                debug!("Type '{type_name}' not found in repository");
            }
        }
    }

    // Resolve EXTENDS_INTERFACE relationships from interfaces
    for interface_entity in interfaces {
        if let Some(extends) = interface_entity.metadata.attributes.get("extends") {
            // Parse comma-separated interface names: "Base, ICloneable"
            for interface_name in extends.split(',').map(|s| s.trim()) {
                if let Some(parent_id) = interface_map.get(interface_name) {
                    neo4j
                        .create_relationship(
                            &interface_entity.entity_id,
                            parent_id,
                            "EXTENDS_INTERFACE",
                            &HashMap::new(),
                        )
                        .await
                        .context("Failed to create EXTENDS_INTERFACE relationship")?;
                    extends_count += 1;
                } else {
                    debug!("Interface '{interface_name}' not found in repository");
                }
            }
        }
    }

    info!(
        "Resolved {} IMPLEMENTS, {} ASSOCIATES, {} EXTENDS_INTERFACE relationships",
        implements_count, associates_count, extends_count
    );

    Ok(())
}

/// Resolve class inheritance after indexing completes
async fn resolve_class_inheritance(
    postgres: &std::sync::Arc<dyn codesearch_storage::PostgresClientTrait>,
    neo4j: &codesearch_storage::Neo4jClient,
    repository_id: Uuid,
) -> Result<()> {
    use codesearch_core::entities::EntityType;
    use std::collections::HashMap;

    info!("Resolving class inheritance...");

    // Ensure Neo4j database context
    let db_name = neo4j
        .ensure_repository_database(repository_id, postgres.as_ref())
        .await?;
    neo4j.use_database(&db_name).await?;

    // Get all classes
    let classes = postgres
        .get_entities_by_type(repository_id, EntityType::Class)
        .await
        .context("Failed to get classes")?;

    // Build class name -> entity_id map
    let class_map: HashMap<String, String> = classes
        .iter()
        .map(|c| (c.name.clone(), c.entity_id.clone()))
        .collect();

    let mut inherits_count = 0;

    for class in classes {
        if let Some(extends) = class.metadata.attributes.get("extends") {
            if let Some(parent_id) = class_map.get(extends) {
                neo4j
                    .create_relationship(
                        &class.entity_id,
                        parent_id,
                        "INHERITS_FROM",
                        &HashMap::new(),
                    )
                    .await
                    .context("Failed to create INHERITS_FROM relationship")?;
                inherits_count += 1;
            } else {
                debug!("Parent class '{extends}' not found in repository");
            }
        }
    }

    info!("Resolved {} INHERITS_FROM relationships", inherits_count);

    Ok(())
}

/// Repository selection result from TUI
#[derive(Debug)]
enum RepositorySelection {
    /// Single repository selected by index
    Single(usize),
    /// All repositories selected
    All,
}

/// Display interactive repository selector
///
/// Returns the user's selection or error if interaction fails
fn display_repository_selector(
    repos: &[(Uuid, String, std::path::PathBuf)],
) -> Result<RepositorySelection> {
    if repos.is_empty() {
        anyhow::bail!("No repositories available for selection");
    }

    // Build display items: repository name and path
    let mut items: Vec<String> = repos
        .iter()
        .map(|(_, name, path)| format!("{name} ({})", path.display()))
        .collect();

    // Add "All repositories" option at the end
    items.push("All repositories".to_string());

    // Display interactive selector
    let selection = Select::new()
        .with_prompt("Select repository to drop")
        .items(&items)
        .default(0)
        .interact()
        .map_err(|e| anyhow!("Failed to display selector: {e}"))?;

    // Check if user selected "All repositories"
    if selection == items.len() - 1 {
        Ok(RepositorySelection::All)
    } else {
        Ok(RepositorySelection::Single(selection))
    }
}

/// Display warning message for repositories about to be deleted
///
/// Shows repository path, Qdrant collection name, and PostgreSQL data
/// that will be permanently removed.
fn display_deletion_warning(repos_to_delete: &[(Uuid, String, std::path::PathBuf)]) {
    println!("\nWARNING: This will permanently delete the following:");
    println!();

    for (_, collection_name, repo_path) in repos_to_delete {
        println!("  Repository: {}", repo_path.display());
        println!("  Qdrant collection: {collection_name}");
        println!("  PostgreSQL data: All entities, snapshots, and embeddings");
        println!();
    }

    println!("This action cannot be undone.");
}

/// Confirm deletion with user
///
/// Returns true if user confirms, false if cancelled
fn confirm_deletion() -> Result<bool> {
    Confirm::new()
        .with_prompt("Type 'yes' to confirm deletion")
        .default(false)
        .interact()
        .map_err(|e| anyhow!("Failed to read confirmation: {e}"))
}

/// Drop indexed data with repository selection
///
/// Displays an interactive selector to choose which repository to drop (or all).
/// Confirms deletion with user before proceeding.
///
/// Deletion is performed in this order:
/// 1. Qdrant collection (if it exists - warns but continues if missing)
/// 2. PostgreSQL repository data (cascades to all child tables)
async fn drop_data(config_path: Option<&Path>) -> Result<()> {
    info!("Preparing to drop indexed data");

    // Load configuration
    let (config, _sources) = Config::load_layered(config_path)?;
    config.validate()?;

    // Ensure dependencies are running
    if config.storage.auto_start_deps {
        infrastructure::ensure_shared_infrastructure(&config.storage).await?;
        let api_base_url = get_api_base_url_if_local_api(&config);
        docker::ensure_dependencies_running(&config.storage, api_base_url).await?;
    }

    // Connect to storage backends
    let postgres_client = codesearch_storage::create_postgres_client(&config.storage)
        .await
        .context("Failed to create PostgreSQL client")?;
    let collection_manager = codesearch_storage::create_collection_manager(&config.storage)
        .await
        .context("Failed to create collection manager")?;

    // List all repositories
    let all_repos = postgres_client
        .list_all_repositories()
        .await
        .context("Failed to list repositories")?;

    if all_repos.is_empty() {
        println!("No indexed repositories found.");
        println!("Run 'codesearch index' from a git repository to create an index.");
        return Ok(());
    }

    // Display repository selector
    let selection = display_repository_selector(&all_repos)?;

    // Determine which repositories to delete based on selection
    let repos_to_delete: Vec<_> = match selection {
        RepositorySelection::All => all_repos,
        RepositorySelection::Single(index) => vec![all_repos[index].clone()],
    };

    // Display warning with specifics
    display_deletion_warning(&repos_to_delete);

    // Confirm deletion
    if !confirm_deletion()? {
        println!("Operation cancelled.");
        return Ok(());
    }

    info!("User confirmed drop operation, proceeding...");

    // Delete selected repositories
    for (repo_id, collection_name, repo_path) in &repos_to_delete {
        info!(
            "Deleting repository: {} (collection: {collection_name})",
            repo_path.display()
        );

        // Delete from Qdrant first
        if collection_manager
            .collection_exists(collection_name)
            .await?
        {
            collection_manager
                .delete_collection(collection_name)
                .await
                .context(format!(
                    "Failed to delete Qdrant collection {collection_name}"
                ))?;
            info!("Deleted Qdrant collection: {collection_name}");
        } else {
            // Warn but continue - Qdrant might be temporarily down or collection already deleted
            warn!(
                "Qdrant collection '{}' does not exist, skipping Qdrant deletion",
                collection_name
            );
            println!(
                "  Warning: Qdrant collection '{collection_name}' not found (may already be deleted)"
            );
        }

        // Delete from Postgres (cascades to all child tables)
        postgres_client
            .drop_repository(*repo_id)
            .await
            .context(format!(
                "Failed to delete repository data from PostgreSQL: {}",
                repo_path.display()
            ))?;
        info!("Deleted repository {repo_id} from PostgreSQL");

        println!("  Deleted: {}", repo_path.display());
    }

    println!(
        "\nSuccessfully deleted {} repository(ies)",
        repos_to_delete.len()
    );
    println!("You can re-index any repository using 'codesearch index'.");

    Ok(())
}

/// Handle cache subcommands
async fn handle_cache_command(command: CacheCommands, config_path: Option<&Path>) -> Result<()> {
    // Load configuration
    let (config, _sources) = Config::load_layered(config_path)?;
    config.validate()?;

    // Ensure dependencies are running
    if config.storage.auto_start_deps {
        infrastructure::ensure_shared_infrastructure(&config.storage).await?;
        let api_base_url = get_api_base_url_if_local_api(&config);
        docker::ensure_dependencies_running(&config.storage, api_base_url).await?;
    }

    let postgres_client = codesearch_storage::create_postgres_client(&config.storage)
        .await
        .context("Failed to connect to Postgres")?;

    match command {
        CacheCommands::Stats => {
            show_cache_stats(&postgres_client).await?;
        }
        CacheCommands::Clear { model } => {
            clear_cache(&postgres_client, model.as_deref()).await?;
        }
    }

    Ok(())
}

/// Display cache statistics in human-readable format
async fn show_cache_stats(
    postgres_client: &std::sync::Arc<dyn codesearch_storage::PostgresClientTrait>,
) -> Result<()> {
    let stats = postgres_client.get_cache_stats().await?;

    println!("\nEmbedding Cache Statistics");
    println!("==========================");
    println!("Total entries:     {}", stats.total_entries);
    println!(
        "Total size:        {:.2} MB",
        stats.total_size_bytes as f64 / 1_048_576.0
    );

    if let Some(oldest) = stats.oldest_entry {
        println!("Oldest entry:      {}", oldest.format("%Y-%m-%d %H:%M:%S"));
    }

    if let Some(newest) = stats.newest_entry {
        println!("Newest entry:      {}", newest.format("%Y-%m-%d %H:%M:%S"));
    }

    if !stats.entries_by_model.is_empty() {
        println!("\nEntries by model:");
        for (model, count) in &stats.entries_by_model {
            println!("  {model}: {count}");
        }
    }

    // Calculate estimated API call savings
    let avg_api_latency_ms = 200.0; // Typical API call latency
    let saved_time_seconds = (stats.total_entries as f64 * avg_api_latency_ms) / 1000.0;
    println!(
        "\nEstimated API time saved: {:.1} seconds ({:.1} minutes)",
        saved_time_seconds,
        saved_time_seconds / 60.0
    );

    Ok(())
}

/// Clear cache entries
async fn clear_cache(
    postgres_client: &std::sync::Arc<dyn codesearch_storage::PostgresClientTrait>,
    model_version: Option<&str>,
) -> Result<()> {
    if let Some(model) = model_version {
        print!("Clearing cache entries for model '{model}'... ");
    } else {
        print!("Clearing all cache entries... ");
    }

    let rows_deleted = postgres_client.clear_cache(model_version).await?;

    println!("Done! Removed {rows_deleted} entries.");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_repository_selection_enum() {
        // Test that RepositorySelection enum variants work correctly
        match RepositorySelection::Single(0) {
            RepositorySelection::Single(idx) => assert_eq!(idx, 0),
            RepositorySelection::All => panic!("Expected Single variant"),
        }

        match RepositorySelection::All {
            RepositorySelection::All => {} // Expected
            RepositorySelection::Single(_) => panic!("Expected All variant"),
        }
    }

    #[test]
    fn test_display_deletion_warning_output() {
        // Test that display_deletion_warning doesn't panic
        // We can't easily capture stdout in unit tests, but we can verify it runs
        use std::path::PathBuf;

        let repos = vec![(
            Uuid::new_v4(),
            "test-collection".to_string(),
            PathBuf::from("/tmp/test"),
        )];

        // This should not panic
        display_deletion_warning(&repos);
    }

    #[test]
    fn test_display_deletion_warning_multiple_repos() {
        use std::path::PathBuf;

        let repos = vec![
            (
                Uuid::new_v4(),
                "collection1".to_string(),
                PathBuf::from("/tmp/repo1"),
            ),
            (
                Uuid::new_v4(),
                "collection2".to_string(),
                PathBuf::from("/tmp/repo2"),
            ),
            (
                Uuid::new_v4(),
                "collection3".to_string(),
                PathBuf::from("/tmp/repo3"),
            ),
        ];

        // This should not panic with multiple repositories
        display_deletion_warning(&repos);
    }

    #[test]
    fn test_display_repository_selector_empty_list() {
        // Test that empty list returns error
        let repos: Vec<(Uuid, String, std::path::PathBuf)> = vec![];

        let result = display_repository_selector(&repos);
        assert!(
            result.is_err(),
            "Empty repository list should return an error"
        );
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("No repositories available"));
    }
}
