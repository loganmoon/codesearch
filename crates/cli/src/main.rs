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
use codesearch_core::entities::EntityType;
use codesearch_embeddings::EmbeddingManager;
use codesearch_storage::{create_collection_manager, create_storage_client, SearchFilters};
use indexer::RepositoryIndexer;
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
    /// Initialize codesearch in the current repository
    Init,
    /// Start the MCP (Model Context Protocol) server for client integration
    Serve {
        /// Port to bind to
        #[arg(short, long, default_value = "8699")]
        port: u16,

        /// Host to bind to
        #[arg(long, default_value = "localhost")]
        host: String,
    },
    /// Index the repository
    Index {
        /// Force re-indexing of all files
        #[arg(long)]
        force: bool,

        /// Show indexing progress
        #[arg(long)]
        progress: bool,
    },
    /// Search the indexed code
    Search {
        /// Search query
        query: String,

        /// Number of results to return
        #[arg(short, long, default_value = "10")]
        limit: usize,

        /// Filter by entity type (function, class, struct, etc.)
        #[arg(long)]
        entity_type: Option<String>,

        /// Filter by programming language
        #[arg(long)]
        language: Option<String>,

        /// Filter by file path pattern
        #[arg(long)]
        file: Option<PathBuf>,
    },
    /// Manage containerized dependencies
    #[command(subcommand)]
    Deps(DepsCommands),
}

#[derive(Subcommand)]
enum DepsCommands {
    /// Start containerized dependencies
    Start {
        /// Docker compose file to use
        #[arg(short = 'f', long)]
        compose_file: Option<String>,
    },
    /// Stop containerized dependencies
    Stop {
        /// Docker compose file to use
        #[arg(short = 'f', long)]
        compose_file: Option<String>,
    },
    /// Check status of dependencies
    Status,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize logging
    init_logging(cli.verbose)?;

    // Execute commands
    match cli.command {
        Some(Commands::Init) => init_repository(cli.config.as_deref()).await,
        Some(Commands::Serve { port, host }) => {
            // Find repository root
            let repo_root = find_repository_root()?;
            // Load configuration
            let config = load_config(&repo_root, cli.config.as_deref()).await?;
            serve(config, host, port).await
        }
        Some(Commands::Index { force, progress }) => {
            // Find repository root
            let repo_root = find_repository_root()?;
            // Load configuration
            let config = load_config(&repo_root, cli.config.as_deref()).await?;
            index_repository(config, force, progress).await
        }
        Some(Commands::Search {
            query,
            limit,
            entity_type,
            language,
            file,
        }) => {
            // Find repository root
            let repo_root = find_repository_root()?;
            // Load configuration
            let config = load_config(&repo_root, cli.config.as_deref()).await?;
            search_code(config, query, limit, entity_type, language, file).await
        }
        Some(Commands::Deps(deps_cmd)) => {
            handle_deps_command(deps_cmd, cli.config.as_deref()).await
        }
        None => {
            // Default behavior - show help
            println!(
                "Use 'codesearch init' to initialize a repository, or --help for more options"
            );
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

/// Initialize codesearch in a repository
async fn init_repository(config_path: Option<&Path>) -> Result<()> {
    let current_dir = env::current_dir()?;

    info!("Initializing codesearch in {:?}", current_dir);

    // Find the repository root
    let repo_root = find_repository_root()?;

    // Create default configuration if it doesn't exist
    let config_file = current_dir.join("codesearch.toml");
    if !config_file.exists() {
        // Generate collection name from repository path
        let collection_name = StorageConfig::generate_collection_name(&repo_root);
        info!("Generated collection name: {}", collection_name);

        let storage_config = StorageConfig {
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
        };

        let config = Config::builder().storage(storage_config).build();

        config
            .save(&config_file)
            .with_context(|| format!("Failed to save config to {config_file:?}"))?;
        info!("Created default configuration at {:?}", config_file);
    }

    // Load or use provided configuration
    let config_path = config_path.unwrap_or(&config_file);
    let config = Config::from_file(config_path)?;

    // Ensure collection name is set
    let config = if config.storage.collection_name.is_empty() {
        let collection_name = StorageConfig::generate_collection_name(&repo_root);
        info!("Updated collection name: {}", collection_name);
        let updated_config = Config::builder()
            .storage(StorageConfig {
                collection_name,
                ..config.storage
            })
            .embeddings(config.embeddings)
            .watcher(config.watcher)
            .languages(config.languages)
            .build();
        updated_config.save(config_path)?;
        updated_config
    } else {
        config
    };

    config.validate()?;

    // Ensure dependencies are running if auto-start is enabled
    if config.storage.auto_start_deps {
        let api_base_url = if parse_provider_type(&config.embeddings.provider)
            == codesearch_embeddings::EmbeddingProviderType::LocalApi
        {
            config.embeddings.api_base_url.as_deref()
        } else {
            None
        };
        docker::ensure_dependencies_running(&config.storage, api_base_url).await?;
    }

    // Create embedding manager to get dimensions
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

    info!("✓ Database migrations completed");

    info!("✓ Repository initialized successfully");
    info!("  Collection: {}", config.storage.collection_name);
    info!("  Dimensions: {}", dimensions);
    info!("  Config: {:?}", config_path);

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

/// Load configuration from file or defaults
async fn load_config(repo_root: &Path, config_path: Option<&Path>) -> Result<Config> {
    let config_file = if let Some(path) = config_path {
        path.to_path_buf()
    } else {
        repo_root.join("codesearch.toml")
    };

    let config = if config_file.exists() {
        let loaded = Config::from_file(&config_file)
            .with_context(|| format!("Failed to load configuration from {config_file:?}"))?;

        // Ensure collection name is set
        if loaded.storage.collection_name.is_empty() {
            let collection_name = StorageConfig::generate_collection_name(repo_root);
            info!("Generated collection name: {}", collection_name);
            Config::builder()
                .storage(StorageConfig {
                    collection_name,
                    ..loaded.storage
                })
                .embeddings(loaded.embeddings)
                .watcher(loaded.watcher)
                .languages(loaded.languages)
                .build()
        } else {
            loaded
        }
    } else {
        warn!("No configuration file found, using defaults");
        let collection_name = StorageConfig::generate_collection_name(repo_root);
        info!("Generated collection name: {}", collection_name);

        let storage_config = StorageConfig {
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
        };

        Config::builder().storage(storage_config).build()
    };

    Ok(config)
}

/// Start the MCP server
async fn serve(config: Config, _host: String, _port: u16) -> Result<()> {
    info!("Checking dependencies...");

    // Ensure dependencies are running
    let api_base_url = if parse_provider_type(&config.embeddings.provider)
        == codesearch_embeddings::EmbeddingProviderType::LocalApi
    {
        config.embeddings.api_base_url.as_deref()
    } else {
        None
    };
    docker::ensure_dependencies_running(&config.storage, api_base_url).await?;

    println!("🚀 Starting MCP server on stdio...");

    // TODO: Initialize storage connection
    // TODO: Start MCP server on stdio

    todo!("MCP server implementation")
}

/// Index the repository
async fn index_repository(config: Config, _force: bool, _progress: bool) -> Result<()> {
    info!("Starting repository indexing");

    // Step 1: Ensure dependencies are running
    if config.storage.auto_start_deps {
        let api_base_url = if parse_provider_type(&config.embeddings.provider)
            == codesearch_embeddings::EmbeddingProviderType::LocalApi
        {
            config.embeddings.api_base_url.as_deref()
        } else {
            None
        };
        docker::ensure_dependencies_running(&config.storage, api_base_url)
            .await
            .context("Failed to ensure dependencies are running")?;
    }

    // Step 2: Verify collection exists (fail if not initialized)
    let collection_manager = create_collection_manager(&config.storage)
        .await
        .context("Failed to create collection manager")?;

    if !collection_manager
        .collection_exists(&config.storage.collection_name)
        .await
        .context("Failed to check if collection exists")?
    {
        return Err(anyhow!(
            "Collection '{}' does not exist. Please run 'codesearch init' first.",
            config.storage.collection_name
        ));
    }

    // Step 3: Create embedding manager
    let mut embedding_config_builder = codesearch_embeddings::EmbeddingConfigBuilder::default()
        .provider(parse_provider_type(&config.embeddings.provider))
        .model(config.embeddings.model.clone())
        .batch_size(config.embeddings.batch_size)
        .embedding_dimension(config.embeddings.embedding_dimension)
        .device(match config.embeddings.device.as_str() {
            "cuda" => codesearch_embeddings::DeviceType::Cuda,
            _ => codesearch_embeddings::DeviceType::Cpu,
        });

    if let Some(ref api_base_url) = config.embeddings.api_base_url {
        embedding_config_builder = embedding_config_builder.api_base_url(api_base_url.clone());
    }

    let api_key = config
        .embeddings
        .api_key
        .clone()
        .or_else(|| std::env::var("VLLM_API_KEY").ok());
    if let Some(key) = api_key {
        embedding_config_builder = embedding_config_builder.api_key(key);
    }

    let embedding_config = embedding_config_builder.build();
    let embedding_manager = Arc::new(
        EmbeddingManager::from_config(embedding_config)
            .await
            .context("Failed to create embedding manager")?,
    );

    // Step 4: Create postgres client (required for Phase 4+)
    let postgres_client = codesearch_storage::create_postgres_client(&config.storage)
        .await
        .context("Failed to connect to Postgres (required for indexing)")?;
    info!("Successfully connected to Postgres metadata store");

    // Step 5: Get repository path
    let repo_path = find_repository_root()?;

    // Step 5.5: Create GitRepository if possible
    let git_repo = match codesearch_watcher::GitRepository::open(&repo_path) {
        Ok(repo) => {
            info!("Git repository detected");
            Some(repo)
        }
        Err(e) => {
            warn!("Not a Git repository or failed to open: {e}");
            None
        }
    };

    // Step 5.6: Get repository_id from database
    let repository_id = postgres_client
        .get_repository_id(&config.storage.collection_name)
        .await
        .context("Failed to query repository")?
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Repository not found for collection '{}'. Please run 'codesearch init' first.",
                config.storage.collection_name
            )
        })?;

    info!("Repository ID: {}", repository_id);

    // Step 6: Create and run indexer
    let mut indexer = RepositoryIndexer::new(
        repo_path.clone(),
        repository_id.to_string(),
        embedding_manager,
        postgres_client,
        git_repo,
    );

    // Step 7: Run indexing (it has built-in progress tracking)
    let result = indexer
        .index_repository()
        .await
        .context("Failed to index repository")?;

    // Step 8: Report statistics
    info!("✅ Indexing completed successfully!");
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

/// Search the indexed code
async fn search_code(
    config: Config,
    query: String,
    limit: usize,
    entity_type: Option<String>,
    language: Option<String>,
    file_path: Option<PathBuf>,
) -> Result<()> {
    info!("🔍 Searching for: {}", query);

    // Step 1: Ensure dependencies are running
    if config.storage.auto_start_deps {
        let api_base_url = if parse_provider_type(&config.embeddings.provider)
            == codesearch_embeddings::EmbeddingProviderType::LocalApi
        {
            config.embeddings.api_base_url.as_deref()
        } else {
            None
        };
        docker::ensure_dependencies_running(&config.storage, api_base_url)
            .await
            .context("Failed to ensure dependencies are running")?;
    }

    // Step 2: Create collection manager and verify collection exists
    let collection_manager = create_collection_manager(&config.storage)
        .await
        .context("Failed to create collection manager")?;

    if !collection_manager
        .collection_exists(&config.storage.collection_name)
        .await
        .context("Failed to check if collection exists")?
    {
        return Err(anyhow!(
            "Collection '{}' does not exist. Please run 'codesearch init' and 'codesearch index' first.",
            config.storage.collection_name
        ));
    }

    // Step 3: Get storage client from manager
    let storage_client = create_storage_client(&config.storage, &config.storage.collection_name)
        .await
        .context("Failed to create storage client")?;

    // Step 4: Create embedding manager
    let mut embedding_config_builder = codesearch_embeddings::EmbeddingConfigBuilder::default()
        .provider(parse_provider_type(&config.embeddings.provider))
        .model(config.embeddings.model.clone())
        .batch_size(config.embeddings.batch_size)
        .embedding_dimension(config.embeddings.embedding_dimension)
        .device(match config.embeddings.device.as_str() {
            "cuda" => codesearch_embeddings::DeviceType::Cuda,
            _ => codesearch_embeddings::DeviceType::Cpu,
        });

    if let Some(ref api_base_url) = config.embeddings.api_base_url {
        embedding_config_builder = embedding_config_builder.api_base_url(api_base_url.clone());
    }

    let api_key = config
        .embeddings
        .api_key
        .clone()
        .or_else(|| std::env::var("VLLM_API_KEY").ok());
    if let Some(key) = api_key {
        embedding_config_builder = embedding_config_builder.api_key(key);
    }

    let embedding_config = embedding_config_builder.build();
    let embedding_manager = EmbeddingManager::from_config(embedding_config)
        .await
        .context("Failed to create embedding manager")?;

    // Step 5: Generate query embedding
    let query_embeddings = embedding_manager
        .embed(vec![query.clone()])
        .await
        .context("Failed to generate query embedding")?;

    let query_embedding_option = query_embeddings
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("Failed to get query embedding"))?;

    let query_embedding =
        query_embedding_option.ok_or_else(|| anyhow!("Query text exceeds model context window"))?;

    // Step 6: Construct search filters if provided
    let filters = if entity_type.is_some() || language.is_some() || file_path.is_some() {
        let parsed_entity_type = entity_type
            .as_ref()
            .map(|t| parse_entity_type(t))
            .transpose()?;

        Some(SearchFilters {
            entity_type: parsed_entity_type,
            language,
            file_path,
        })
    } else {
        None
    };

    // Step 7: Create Postgres client for fetching full entities
    let postgres_client = codesearch_storage::create_postgres_client(&config.storage)
        .await
        .context("Failed to connect to Postgres")?;

    // Step 8: Search for similar entities (returns IDs only)
    let search_results = storage_client
        .search_similar(query_embedding, limit, filters)
        .await
        .context("Failed to search for similar entities")?;

    if search_results.is_empty() {
        println!("No results found for query: {query}");
        return Ok(());
    }

    // Step 9: Batch fetch full entities from Postgres
    let entity_refs: Vec<(codesearch_storage::Uuid, String)> = search_results
        .iter()
        .filter_map(|(entity_id, repo_id, _score)| {
            codesearch_storage::Uuid::parse_str(repo_id)
                .ok()
                .map(|uuid| (uuid, entity_id.clone()))
        })
        .collect();

    let full_entities = postgres_client
        .get_entities_by_ids(&entity_refs)
        .await?;

    // Create map for lookup
    let entity_map: std::collections::HashMap<String, codesearch_core::CodeEntity> = full_entities
        .into_iter()
        .map(|e| (e.entity_id.clone(), e))
        .collect();

    // Step 9: Display results with scores
    println!("\n📊 Found {} results:\n", search_results.len());
    println!("{}", "─".repeat(80));

    for (idx, (entity_id, _repo_id, score)) in search_results.iter().enumerate() {
        if let Some(entity) = entity_map.get(entity_id) {
            let similarity_percent = (score * 100.0) as u32;

            println!(
                "{}. {} ({}% similarity)",
                idx + 1,
                entity.name,
                similarity_percent
            );
            println!("   Type: {:?}", entity.entity_type);
            println!(
                "   File: {}:{}",
                entity.file_path.display(),
                entity.location.start_line
            );

            if let Some(ref content) = entity.content {
                // Show first 200 chars of content
                let preview = if content.len() > 200 {
                    format!("{}...", &content[..200])
                } else {
                    content.to_string()
                };
                println!("   Preview: {}", preview.replace('\n', "\n            "));
            }

            if idx < search_results.len() - 1 {
                println!("{}", "─".repeat(80));
            }
        }
    }
    println!("{}", "─".repeat(80));
    println!("\n✅ Search completed successfully");

    Ok(())
}

/// Handle dependency management commands
async fn handle_deps_command(cmd: DepsCommands, config_path: Option<&Path>) -> Result<()> {
    match cmd {
        DepsCommands::Start { compose_file } => {
            let compose_file = compose_file.or_else(|| {
                config_path
                    .and_then(|p| p.parent())
                    .map(|p| p.join("docker-compose.yml").to_string_lossy().into_owned())
            });

            docker::start_dependencies(compose_file.as_deref())?;
            println!("✅ Dependencies started successfully");
            Ok(())
        }
        DepsCommands::Stop { compose_file } => {
            let compose_file = compose_file.or_else(|| {
                config_path
                    .and_then(|p| p.parent())
                    .map(|p| p.join("docker-compose.yml").to_string_lossy().into_owned())
            });

            docker::stop_dependencies(compose_file.as_deref())?;
            println!("✅ Dependencies stopped successfully");
            Ok(())
        }
        DepsCommands::Status => {
            // Try to load config to get Qdrant settings, use defaults if not found
            let config = if let Ok(repo_root) = find_repository_root() {
                match load_config(&repo_root, config_path).await {
                    Ok(config) => config,
                    Err(_) => {
                        // Use default storage settings for status check
                        let storage_config = StorageConfig {
                            qdrant_host: "localhost".to_string(),
                            qdrant_port: 6334,
                            qdrant_rest_port: 6333,
                            collection_name: "codesearch".to_string(),
                            auto_start_deps: true,
                            docker_compose_file: None,
                            postgres_host: "localhost".to_string(),
                            postgres_port: 5432,
                            postgres_database: "codesearch".to_string(),
                            postgres_user: "codesearch".to_string(),
                            postgres_password: "codesearch".to_string(),
                        };
                        Config::builder().storage(storage_config).build()
                    }
                }
            } else {
                // Use default storage settings for status check
                let storage_config = StorageConfig {
                    qdrant_host: "localhost".to_string(),
                    qdrant_port: 6334,
                    qdrant_rest_port: 6333,
                    collection_name: "codesearch".to_string(),
                    auto_start_deps: true,
                    docker_compose_file: None,
                    postgres_host: "localhost".to_string(),
                    postgres_port: 5432,
                    postgres_database: "codesearch".to_string(),
                    postgres_user: "codesearch".to_string(),
                    postgres_password: "codesearch".to_string(),
                };
                Config::builder().storage(storage_config).build()
            };

            let api_base_url = if parse_provider_type(&config.embeddings.provider)
                == codesearch_embeddings::EmbeddingProviderType::LocalApi
            {
                config.embeddings.api_base_url.as_deref()
            } else {
                None
            };

            let status = docker::get_dependencies_status(&config.storage, api_base_url).await?;
            println!("{status}");
            Ok(())
        }
    }
}

/// Parse entity type string to EntityType enum
fn parse_entity_type(entity_type: &str) -> Result<EntityType> {
    match entity_type.to_lowercase().as_str() {
        "function" => Ok(EntityType::Function),
        "method" => Ok(EntityType::Method),
        "class" => Ok(EntityType::Class),
        "struct" => Ok(EntityType::Struct),
        "interface" => Ok(EntityType::Interface),
        "trait" => Ok(EntityType::Trait),
        "enum" => Ok(EntityType::Enum),
        "module" => Ok(EntityType::Module),
        "package" => Ok(EntityType::Package),
        "const" | "constant" => Ok(EntityType::Constant),
        "variable" | "var" => Ok(EntityType::Variable),
        "type" | "typealias" | "type_alias" => Ok(EntityType::TypeAlias),
        "macro" => Ok(EntityType::Macro),
        _ => Err(anyhow!(
            "Invalid entity type: {entity_type}. Valid types are: function, method, class, struct, interface, trait, enum, module, package, constant, variable, type, macro"
        )),
    }
}
