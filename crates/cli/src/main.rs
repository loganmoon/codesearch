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
use codesearch_storage::postgres::PostgresClient;
use codesearch_storage::{create_collection_manager, create_storage_client, SearchFilters};
use indexer::{Indexer, RepositoryIndexer};
use rmcp::schemars;
use rmcp::serde_json;
use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{
        CallToolResult, Content, ErrorCode, ErrorData as McpError, InitializeRequestParam,
        InitializeResult, ListResourcesResult, PaginatedRequestParam, ProtocolVersion,
        ReadResourceRequestParam, ReadResourceResult, ResourceContents, ResourcesCapability,
        ServerCapabilities,
    },
    schemars::JsonSchema,
    service::RequestContext,
    tool, tool_handler, tool_router, RoleServer, ServerHandler, ServiceExt,
};
use serde::Deserialize;
use sqlx::types::uuid;
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
    /// Start MCP server with semantic code search
    Serve,
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

/// MCP server for codesearch semantic code search
#[derive(Clone)]
#[allow(dead_code)]
struct CodeSearchMcpServer {
    repository_id: uuid::Uuid,
    repository_root: PathBuf,
    collection_name: String,
    embedding_manager: Arc<EmbeddingManager>,
    storage_client: Arc<dyn codesearch_storage::StorageClient>,
    postgres_client: Arc<PostgresClient>,
    tool_router: ToolRouter<Self>,
}

/// Request parameters for search_code tool
#[derive(Debug, Deserialize, JsonSchema)]
struct SearchCodeRequest {
    /// Semantic search query describing the code you're looking for
    query: String,

    /// Maximum number of results (1-100)
    #[serde(default = "default_limit")]
    limit: Option<usize>,

    /// Filter by entity type (e.g., function, method, class, struct)
    entity_type: Option<String>,

    /// Filter by programming language (e.g., rust, python, javascript)
    language: Option<String>,

    /// Filter by file path pattern
    file_path: Option<String>,
}

fn default_limit() -> Option<usize> {
    Some(10)
}

#[tool_router]
impl CodeSearchMcpServer {
    #[tool(description = "Search for code entities semantically using natural language queries. \
                          Returns similar functions, classes, and other code constructs with full \
                          details including content, documentation, and signature.")]
    async fn search_code(
        &self,
        Parameters(request): Parameters<SearchCodeRequest>,
    ) -> Result<CallToolResult, McpError> {
        // Validate limit
        let limit = request.limit.unwrap_or(10).clamp(1, 100);

        // Generate query embedding
        let embeddings = self
            .embedding_manager
            .embed(vec![request.query.clone()])
            .await
            .map_err(|e| {
                McpError::new(
                    ErrorCode::INTERNAL_ERROR,
                    format!("Failed to generate embedding: {e}"),
                    None,
                )
            })?;

        let query_embedding = embeddings
            .into_iter()
            .next()
            .flatten()
            .ok_or_else(|| {
                McpError::new(
                    ErrorCode::INTERNAL_ERROR,
                    "Failed to generate embedding".to_string(),
                    None,
                )
            })?;

        // Parse entity type filter
        let entity_type = request
            .entity_type
            .as_ref()
            .and_then(|s| EntityType::try_from(s.as_str()).ok());

        // Build filters
        let filters = SearchFilters {
            entity_type,
            language: request.language.clone(),
            file_path: request.file_path.as_ref().map(PathBuf::from),
        };

        // Search Qdrant
        let results = self
            .storage_client
            .search_similar(query_embedding, limit, Some(filters))
            .await
            .map_err(|e| {
                McpError::new(ErrorCode::INTERNAL_ERROR, format!("Search failed: {e}"), None)
            })?;

        // Batch fetch from Postgres
        let entity_refs: Vec<_> = results
            .iter()
            .map(|(eid, _rid, _)| (self.repository_id, eid.to_string()))
            .collect();

        let entities_vec = self
            .postgres_client
            .get_entities_by_ids(&entity_refs)
            .await
            .map_err(|e| {
                McpError::new(
                    ErrorCode::INTERNAL_ERROR,
                    format!("Failed to fetch entities: {e}"),
                    None,
                )
            })?;

        // Convert to HashMap for efficient lookup
        let entities_map: std::collections::HashMap<String, _> = entities_vec
            .into_iter()
            .map(|e| (e.entity_id.clone(), e))
            .collect();

        // Format results with full entity details
        let formatted_results: Vec<_> = results
            .into_iter()
            .filter_map(|(entity_id, _repo_id, score)| {
                entities_map.get(&entity_id).map(|entity| {
                    serde_json::json!({
                        "entity_id": entity_id,
                        "similarity_percent": (score * 100.0).round() as i32,
                        "name": entity.name,
                        "qualified_name": entity.qualified_name,
                        "entity_type": format!("{:?}", entity.entity_type),
                        "language": format!("{:?}", entity.language),
                        "file_path": entity.file_path.display().to_string(),
                        "line_range": {
                            "start": entity.location.start_line,
                            "end": entity.location.end_line,
                        },
                        "content": entity.content,
                        "documentation_summary": entity.documentation_summary,
                        "signature": entity.signature.as_ref().map(|s| format!("{s:?}")),
                        "visibility": format!("{:?}", entity.visibility),
                    })
                })
            })
            .collect();

        let response = serde_json::json!({
            "results": formatted_results,
            "total": formatted_results.len(),
            "query": request.query,
        });

        let response_str = serde_json::to_string_pretty(&response).map_err(|e| {
            McpError::new(
                ErrorCode::INTERNAL_ERROR,
                format!("Failed to serialize response: {e}"),
                None,
            )
        })?;

        Ok(CallToolResult::success(vec![Content::text(response_str)]))
    }

    fn new(
        repository_id: uuid::Uuid,
        repository_root: PathBuf,
        collection_name: String,
        embedding_manager: Arc<EmbeddingManager>,
        storage_client: Arc<dyn codesearch_storage::StorageClient>,
        postgres_client: Arc<PostgresClient>,
    ) -> Self {
        Self {
            repository_id,
            repository_root,
            collection_name,
            embedding_manager,
            storage_client,
            postgres_client,
            tool_router: Self::tool_router(),
        }
    }
}

#[tool_handler]
impl ServerHandler for CodeSearchMcpServer {
    async fn initialize(
        &self,
        _request: InitializeRequestParam,
        _context: RequestContext<RoleServer>,
    ) -> Result<InitializeResult, rmcp::model::ErrorData> {
        Ok(InitializeResult {
            protocol_version: ProtocolVersion::default(),
            capabilities: ServerCapabilities {
                tools: Some(rmcp::model::ToolsCapability {
                    list_changed: None,
                }),
                resources: Some(ResourcesCapability {
                    subscribe: None,
                    list_changed: None,
                }),
                ..Default::default()
            },
            server_info: rmcp::model::Implementation {
                name: "codesearch".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                title: None,
                website_url: None,
                icons: None,
            },
            ..Default::default()
        })
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParam>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        Ok(ListResourcesResult {
            resources: vec![rmcp::model::Annotated::new(
                rmcp::model::RawResource {
                    uri: "codesearch://repo/info".to_string(),
                    name: "Repository Information".to_string(),
                    title: None,
                    description: Some(
                        "Current repository metadata and configuration".to_string(),
                    ),
                    mime_type: Some("application/json".to_string()),
                    size: None,
                    icons: None,
                },
                None,
            )],
            next_cursor: None,
        })
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParam,
        _context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResult, McpError> {
        let contents = match request.uri.as_str() {
            "codesearch://repo/info" => {
                let info = serde_json::json!({
                    "repository_root": self.repository_root.display().to_string(),
                    "collection_name": self.collection_name,
                    "repository_id": self.repository_id.to_string(),
                    "languages_supported": ["rust", "python", "javascript", "typescript", "go"],
                });

                let text = serde_json::to_string_pretty(&info).map_err(|e| {
                    McpError::new(
                        ErrorCode::INTERNAL_ERROR,
                        format!("Failed to serialize resource: {e}"),
                        None,
                    )
                })?;

                vec![ResourceContents::TextResourceContents {
                    uri: request.uri.clone(),
                    mime_type: Some("application/json".to_string()),
                    text,
                    meta: None,
                }]
            }

            _ => {
                return Err(McpError::new(
                    ErrorCode::INVALID_PARAMS,
                    format!("Unknown resource URI: {}", request.uri),
                    None,
                ))
            }
        };

        Ok(ReadResourceResult { contents })
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize logging
    init_logging(cli.verbose)?;

    // Execute commands
    match cli.command {
        Some(Commands::Init) => init_repository(cli.config.as_deref()).await,
        Some(Commands::Serve) => {
            // Find repository root
            let repo_root = find_repository_root()?;
            // Load configuration
            let config = load_config(&repo_root, cli.config.as_deref()).await?;
            serve(config).await
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

    // Register repository in Postgres
    let repository_id = postgres_client
        .ensure_repository(&repo_root, &config.storage.collection_name, None)
        .await
        .context("Failed to register repository")?;

    info!("✓ Repository registered with ID: {}", repository_id);

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

/// Catch up the index by processing changes between last indexed commit and current HEAD
#[allow(dead_code)]
async fn catch_up_index(
    repo_root: &Path,
    repository_id: uuid::Uuid,
    postgres_client: &codesearch_storage::postgres::PostgresClient,
    embedding_manager: &Arc<EmbeddingManager>,
    git_repo: &codesearch_watcher::GitRepository,
) -> Result<()> {
    // Get last indexed commit from database
    let last_indexed_commit = postgres_client
        .get_last_indexed_commit(repository_id)
        .await
        .context("Failed to get last indexed commit")?;

    // Get current HEAD commit
    let current_commit = git_repo
        .current_commit_hash()
        .context("Failed to get current commit")?;

    // Check if we need to catch up
    if let Some(ref last_commit) = last_indexed_commit {
        if last_commit == &current_commit {
            info!("Index is up-to-date at commit {}", &current_commit[..8]);
            return Ok(());
        }

        info!(
            "Catching up index from {}..{} ({})",
            &last_commit[..8],
            &current_commit[..8],
            if last_commit.len() >= 8 && current_commit.len() >= 8 {
                "git diff"
            } else {
                "full scan"
            }
        );
    } else {
        info!(
            "No previous index found, will update to commit {}",
            &current_commit[..8]
        );
    }

    // Get changed files using git diff
    let changed_files = git_repo
        .get_changed_files_between_commits(last_indexed_commit.as_deref(), &current_commit)
        .context("Failed to get changed files from git")?;

    if changed_files.is_empty() {
        info!("No file changes detected");
        postgres_client
            .set_last_indexed_commit(repository_id, &current_commit)
            .await
            .context("Failed to update last indexed commit")?;
        return Ok(());
    }

    info!("Found {} changed files to process", changed_files.len());

    // Process each changed file
    for file_diff in changed_files {
        match file_diff.change_type {
            codesearch_watcher::FileDiffChangeType::Added
            | codesearch_watcher::FileDiffChangeType::Modified => {
                // Re-index the file
                if let Err(e) = reindex_single_file(
                    repo_root,
                    repository_id,
                    &file_diff.path,
                    postgres_client,
                    embedding_manager,
                )
                .await
                {
                    warn!("Failed to reindex file {}: {}", file_diff.path.display(), e);
                }
            }
            codesearch_watcher::FileDiffChangeType::Deleted => {
                // Mark all entities in the file as deleted
                if let Err(e) =
                    handle_file_deletion(repository_id, &file_diff.path, postgres_client).await
                {
                    warn!(
                        "Failed to handle deletion of file {}: {}",
                        file_diff.path.display(),
                        e
                    );
                }
            }
        }
    }

    // Update last indexed commit
    postgres_client
        .set_last_indexed_commit(repository_id, &current_commit)
        .await
        .context("Failed to update last indexed commit")?;

    info!(
        "✅ Catch-up indexing completed at commit {}",
        &current_commit[..8]
    );
    Ok(())
}

/// Re-index a single file
#[allow(dead_code)]
async fn reindex_single_file(
    _repo_root: &Path,
    _repository_id: uuid::Uuid,
    file_path: &Path,
    _postgres_client: &codesearch_storage::postgres::PostgresClient,
    _embedding_manager: &Arc<EmbeddingManager>,
) -> Result<()> {
    // Check if file should be indexed (language support, etc.)
    if !should_index_file(file_path) {
        return Ok(());
    }

    info!("Re-indexing file: {}", file_path.display());

    // TODO: Implement full file re-indexing logic
    // This will require:
    // 1. Reading and parsing the file with language-specific extractors
    // 2. Generating embeddings for extracted entities
    // 3. Storing entities with outbox entries
    // 4. Updating file snapshots
    //
    // For now, this is a placeholder that logs the intent
    warn!(
        "File re-indexing not yet fully implemented, file will be indexed on next full index run"
    );

    Ok(())
}

/// Handle deletion of a file
#[allow(dead_code)]
async fn handle_file_deletion(
    repository_id: uuid::Uuid,
    file_path: &Path,
    postgres_client: &codesearch_storage::postgres::PostgresClient,
) -> Result<()> {
    info!("Handling deletion of file: {}", file_path.display());

    let file_path_str = file_path
        .to_str()
        .ok_or_else(|| anyhow!("Invalid file path"))?;

    // Get entities for this file
    let entity_ids = postgres_client
        .get_file_snapshot(repository_id, file_path_str)
        .await
        .context("Failed to get file snapshot")?
        .unwrap_or_default();

    if entity_ids.is_empty() {
        return Ok(());
    }

    // Mark entities as deleted
    postgres_client
        .mark_entities_deleted(repository_id, &entity_ids)
        .await
        .context("Failed to mark entities as deleted")?;

    // Write DELETE outbox entries
    for entity_id in &entity_ids {
        let payload = serde_json::json!({
            "entity_ids": [entity_id],
            "reason": "file_deleted"
        });

        postgres_client
            .write_outbox_entry(
                repository_id,
                entity_id,
                codesearch_storage::postgres::OutboxOperation::Delete,
                codesearch_storage::postgres::TargetStore::Qdrant,
                payload,
            )
            .await
            .context("Failed to write outbox entry")?;
    }

    info!("Marked {} entities as deleted", entity_ids.len());
    Ok(())
}

/// Check if a file should be indexed based on extension
#[allow(dead_code)]
fn should_index_file(file_path: &Path) -> bool {
    let Some(extension) = file_path.extension() else {
        return false;
    };

    let ext_str = extension.to_string_lossy();
    matches!(
        ext_str.as_ref(),
        "rs" | "py" | "js" | "ts" | "jsx" | "tsx" | "go"
    )
}

/// Handle a single file change event from the watcher
async fn handle_file_change_event(
    event: codesearch_watcher::FileChange,
    repo_root: &Path,
    repository_id: uuid::Uuid,
    embedding_manager: &Arc<EmbeddingManager>,
    postgres_client: &Arc<PostgresClient>,
) -> Result<()> {
    match event {
        codesearch_watcher::FileChange::Created(path, _)
        | codesearch_watcher::FileChange::Modified(path, _) => {
            info!("Re-indexing file: {}", path.display());

            // Re-index the single file (reuse from catch-up)
            reindex_single_file(
                repo_root,
                repository_id,
                &path,
                postgres_client,
                embedding_manager,
            )
            .await?;
        }

        codesearch_watcher::FileChange::Deleted(path) => {
            info!("File deleted: {}", path.display());

            handle_file_deletion(repository_id, &path, postgres_client).await?;
        }

        codesearch_watcher::FileChange::Renamed { from, to } => {
            info!("File renamed: {} -> {}", from.display(), to.display());

            // Treat as delete + create
            handle_file_deletion(repository_id, &from, postgres_client).await?;
            reindex_single_file(
                repo_root,
                repository_id,
                &to,
                postgres_client,
                embedding_manager,
            )
            .await?;
        }

        codesearch_watcher::FileChange::PermissionsChanged(_) => {
            // Ignore permission changes
        }
    }

    Ok(())
}

/// Start the MCP server
async fn serve(config: Config) -> Result<()> {
    info!("Checking dependencies...");

    // Step 1: Ensure dependencies are running
    let api_base_url = if parse_provider_type(&config.embeddings.provider)
        == codesearch_embeddings::EmbeddingProviderType::LocalApi
    {
        config.embeddings.api_base_url.as_deref()
    } else {
        None
    };
    docker::ensure_dependencies_running(&config.storage, api_base_url).await?;

    // Step 2: Verify collection exists
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

    // Step 3: Create storage client
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
    let embedding_manager = Arc::new(
        EmbeddingManager::from_config(embedding_config)
            .await
            .context("Failed to create embedding manager")?,
    );

    // Step 5: Create postgres client
    let postgres_client = codesearch_storage::create_postgres_client(&config.storage)
        .await
        .context("Failed to connect to Postgres")?;

    // Step 6: Get repository metadata
    let repo_root = find_repository_root()?;
    let repository_id = postgres_client
        .get_repository_id(&config.storage.collection_name)
        .await
        .context("Failed to query repository")?
        .ok_or_else(|| {
            anyhow!(
                "Repository not found for collection '{}'. Run 'codesearch init' first.",
                config.storage.collection_name
            )
        })?;

    info!("Repository ID: {repository_id}");

    // Step 7: Run catch-up indexing
    info!("Checking for offline changes...");
    let git_repo = codesearch_watcher::GitRepository::open(&repo_root)
        .context("Failed to open git repository")?;

    catch_up_index(
        &repo_root,
        repository_id,
        &postgres_client,
        &embedding_manager,
        &git_repo,
    )
    .await
    .context("Catch-up indexing failed")?;

    // Step 8: Initialize and start file watcher
    info!("Starting filesystem watcher...");
    let watcher_config = codesearch_watcher::WatcherConfig::builder()
        .debounce_ms(500)
        .max_file_size(10 * 1024 * 1024) // 10MB
        .batch_size(100)
        .build();

    let mut watcher = codesearch_watcher::FileWatcher::new(watcher_config)
        .context("Failed to create file watcher")?;

    let mut event_rx = watcher
        .watch(&repo_root)
        .await
        .context("Failed to start watching repository")?;

    // Clone dependencies for watcher task
    let watcher_repo_root = repo_root.clone();
    let watcher_repo_id = repository_id;
    let watcher_embedding_mgr = embedding_manager.clone();
    let watcher_postgres = Arc::new(postgres_client.clone());

    // Spawn background task to handle file changes
    let watcher_task = tokio::spawn(async move {
        info!("File watcher task started");

        while let Some(event) = event_rx.recv().await {
            if let Err(e) = handle_file_change_event(
                event,
                &watcher_repo_root,
                watcher_repo_id,
                &watcher_embedding_mgr,
                &watcher_postgres,
            )
            .await
            {
                // Log error but don't crash watcher
                tracing::error!("Error handling file change: {e}");
            }
        }

        tracing::warn!("File watcher task stopped");
    });

    // Step 9: Setup signal handler for graceful shutdown
    let (shutdown_tx, mut shutdown_rx) = tokio::sync::mpsc::channel::<()>(1);

    tokio::spawn(async move {
        match tokio::signal::ctrl_c().await {
            Ok(()) => {
                info!("Received Ctrl+C, initiating graceful shutdown");
                let _ = shutdown_tx.send(()).await;
            }
            Err(e) => {
                tracing::error!("Error setting up signal handler: {e}");
            }
        }
    });

    // Step 10: Create MCP server
    let mcp_server = CodeSearchMcpServer::new(
        repository_id,
        repo_root.clone(),
        config.storage.collection_name.clone(),
        embedding_manager.clone(),
        storage_client,
        postgres_client,
    );

    // Step 11: Start MCP server on stdio
    println!("🚀 Starting MCP server on stdio...");
    let transport = (tokio::io::stdin(), tokio::io::stdout());
    let server = mcp_server.serve(transport).await?;

    info!("MCP server connected and running");

    // Step 12: Run server with shutdown handling
    tokio::select! {
        quit_reason = server.waiting() => {
            match quit_reason {
                Ok(reason) => info!("MCP server stopped normally. Reason: {reason:?}"),
                Err(e) => tracing::error!("MCP server error: {e}"),
            }
        }
        _ = shutdown_rx.recv() => {
            info!("Shutdown signal received");
        }
    }

    // Step 13: Cleanup
    info!("Stopping file watcher...");
    watcher.stop().await?;
    if let Err(e) = watcher_task.await {
        tracing::error!("Watcher task error: {e}");
    }

    info!("Codesearch MCP server shut down successfully");

    Ok(())
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

    let full_entities = postgres_client.get_entities_by_ids(&entity_refs).await?;

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
