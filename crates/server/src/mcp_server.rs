use axum::Router;
use codesearch_core::error::{Error, ResultExt};
use codesearch_core::{config::Config, entities::EntityType};
use codesearch_embeddings::EmbeddingManager;
use codesearch_storage::{
    create_collection_manager, create_storage_client, PostgresClientTrait, SearchFilters,
    StorageClient,
};
use codesearch_watcher::{FileWatcher, WatcherConfig};
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
    tool, tool_handler, tool_router,
    transport::streamable_http_server::{
        session::local::LocalSessionManager,
        tower::{StreamableHttpServerConfig, StreamableHttpService},
    },
    RoleServer, ServerHandler,
};
use serde::Deserialize;
use std::{collections::HashMap, net::SocketAddr, path::PathBuf, sync::Arc};
use tokio::sync::RwLock;
use tracing::info;
use uuid::Uuid;

const DEFAULT_DEBOUNCE_MS: u64 = 500;
const MAX_FILE_SIZE_BYTES: u64 = 10 * 1024 * 1024; // 10MB

/// Information about a single indexed repository
struct RepositoryInfo {
    repository_id: Uuid,
    repository_root: PathBuf,
    collection_name: String,
    storage_client: Arc<dyn StorageClient>,
    last_indexed_commit: Option<String>,
}

/// MCP server for codesearch semantic code search
#[derive(Clone)]
struct CodeSearchMcpServer {
    repositories: Arc<RwLock<HashMap<Uuid, RepositoryInfo>>>,
    embedding_manager: Arc<EmbeddingManager>,
    postgres_client: Arc<dyn PostgresClientTrait>,
    #[allow(dead_code)]
    watchers: Arc<RwLock<HashMap<Uuid, FileWatcher>>>,
    tool_router: ToolRouter<Self>,
}

/// Request parameters for search_code tool
#[derive(Debug, Deserialize, JsonSchema)]
struct SearchCodeRequest {
    /// Semantic search query. Works best with code-like patterns (e.g., function signatures,
    /// type names, code snippets) rather than abstract descriptions.
    query: String,

    /// Maximum number of results (1-100)
    #[serde(default = "default_limit")]
    limit: Option<usize>,

    /// Filter by entity type (e.g., function, method, class, struct)
    entity_type: Option<String>,

    /// Filter by programming language (currently only "rust" is supported)
    language: Option<String>,

    /// Filter by file path pattern
    file_path: Option<String>,

    /// Repository path to search (optional)
    /// If provided, searches only the repository at this path.
    /// If omitted, searches all indexed repositories.
    repository_path: Option<String>,
}

fn default_limit() -> Option<usize> {
    Some(10)
}

#[tool_router]
impl CodeSearchMcpServer {
    #[tool(
        description = "Semantic code search using embeddings. Searches the repository at the specified path, or all indexed repositories if no path is provided."
    )]
    async fn search_code(
        &self,
        Parameters(request): Parameters<SearchCodeRequest>,
    ) -> std::result::Result<CallToolResult, McpError> {
        // Validate limit
        let limit = request.limit.unwrap_or(10).clamp(1, 100);

        // Determine which repositories to search
        let repos = self.repositories.read().await;

        let target_repos: Vec<(Uuid, &RepositoryInfo)> = if let Some(ref repo_path) =
            request.repository_path
        {
            // Search specific repository by path
            let path_buf = PathBuf::from(repo_path);

            // Look up repository by path
            let repo_lookup = self
                .postgres_client
                .get_repository_by_path(&path_buf)
                .await
                .map_err(|e| {
                    McpError::new(
                        ErrorCode::INTERNAL_ERROR,
                        format!("Failed to look up repository: {e}"),
                        None,
                    )
                })?;

            match repo_lookup {
                Some((repo_id, _)) => {
                    if let Some(repo_info) = repos.get(&repo_id) {
                        vec![(repo_id, repo_info)]
                    } else {
                        return Err(McpError::new(
                            ErrorCode::INVALID_PARAMS,
                            format!("Repository at '{repo_path}' is not currently being served"),
                            None,
                        ));
                    }
                }
                None => {
                    return Err(McpError::new(
                        ErrorCode::INVALID_PARAMS,
                        format!("No indexed repository found at path '{repo_path}'"),
                        None,
                    ));
                }
            }
        } else {
            // Search all repositories
            repos.iter().map(|(id, info)| (*id, info)).collect()
        };

        if target_repos.is_empty() {
            return Err(McpError::new(
                ErrorCode::INTERNAL_ERROR,
                "No repositories available to search".to_string(),
                None,
            ));
        }

        // Extract query
        let query_text = request.query;

        // Format query with BGE instruction
        let bge_instruction = "Represent this code search query for retrieving semantically \
                               similar code snippets, function implementations, type definitions, \
                               and code patterns";
        let formatted_query = format!("<instruct>{bge_instruction}\n<query>{query_text}");

        // Generate query embedding
        let embeddings = self
            .embedding_manager
            .embed(vec![formatted_query])
            .await
            .map_err(|e| {
                McpError::new(
                    ErrorCode::INTERNAL_ERROR,
                    format!("Failed to generate embedding: {e}"),
                    None,
                )
            })?;

        let query_embedding = embeddings.into_iter().next().flatten().ok_or_else(|| {
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

        // Search each target repository
        let mut all_results = Vec::new();
        for (repo_id, repo_info) in &target_repos {
            let results = repo_info
                .storage_client
                .search_similar(query_embedding.clone(), limit, Some(filters.clone()))
                .await
                .map_err(|e| {
                    McpError::new(
                        ErrorCode::INTERNAL_ERROR,
                        format!(
                            "Search failed in repository {}: {e}",
                            repo_info.collection_name
                        ),
                        None,
                    )
                })?;

            // Add repository context to results
            for (entity_id, _repo_id_from_qdrant, score) in results {
                all_results.push((*repo_id, entity_id, score));
            }
        }

        // Sort by score and limit
        all_results.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
        all_results.truncate(limit);

        // Batch fetch entities from Postgres
        let entity_refs: Vec<_> = all_results
            .iter()
            .map(|(repo_id, eid, _)| (*repo_id, eid.to_string()))
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
        let mut entities_map: HashMap<String, _> = HashMap::with_capacity(entities_vec.len());
        for entity in entities_vec {
            entities_map.insert(entity.entity_id.clone(), entity);
        }

        // Format results with repository information
        let formatted_results: Vec<_> = all_results
            .into_iter()
            .filter_map(|(repo_id, entity_id, score)| {
                entities_map.get(&entity_id).and_then(|entity| {
                    repos.get(&repo_id).map(|repo| {
                        serde_json::json!({
                            "repository_id": repo_id.to_string(),
                            "repository_path": repo.repository_root.display().to_string(),
                            "collection_name": repo.collection_name,
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
            })
            .collect();

        let response = serde_json::json!({
            "results": formatted_results,
            "total": formatted_results.len(),
            "query": query_text,
            "repositories_searched": target_repos.len(),
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

    #[tool(description = "List all indexed repositories available for search")]
    async fn list_repositories(&self) -> std::result::Result<CallToolResult, McpError> {
        let repos = self.repositories.read().await;

        let repo_list: Vec<_> = repos
            .values()
            .map(|repo| {
                serde_json::json!({
                    "repository_id": repo.repository_id.to_string(),
                    "repository_name": repo.repository_root
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("unknown"),
                    "repository_path": repo.repository_root.display().to_string(),
                    "collection_name": repo.collection_name,
                    "last_indexed_commit": repo.last_indexed_commit,
                })
            })
            .collect();

        let response = serde_json::json!({
            "repositories": repo_list,
            "total": repo_list.len(),
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
        repositories: Arc<RwLock<HashMap<Uuid, RepositoryInfo>>>,
        embedding_manager: Arc<EmbeddingManager>,
        postgres_client: Arc<dyn PostgresClientTrait>,
        watchers: Arc<RwLock<HashMap<Uuid, FileWatcher>>>,
    ) -> Self {
        Self {
            repositories,
            embedding_manager,
            postgres_client,
            watchers,
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
    ) -> std::result::Result<InitializeResult, rmcp::model::ErrorData> {
        Ok(InitializeResult {
            protocol_version: ProtocolVersion::default(),
            capabilities: ServerCapabilities {
                tools: Some(rmcp::model::ToolsCapability { list_changed: None }),
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
    ) -> std::result::Result<ListResourcesResult, McpError> {
        Ok(ListResourcesResult {
            resources: vec![rmcp::model::Annotated::new(
                rmcp::model::RawResource {
                    uri: "codesearch://repositories/info".to_string(),
                    name: "Repositories Information".to_string(),
                    title: None,
                    description: Some("All indexed repositories and their metadata".to_string()),
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
    ) -> std::result::Result<ReadResourceResult, McpError> {
        let contents = match request.uri.as_str() {
            "codesearch://repositories/info" => {
                let repos = self.repositories.read().await;

                let repo_list: Vec<_> = repos
                    .values()
                    .map(|repo| {
                        serde_json::json!({
                            "repository_id": repo.repository_id.to_string(),
                            "repository_root": repo.repository_root.display().to_string(),
                            "collection_name": repo.collection_name,
                            "last_indexed_commit": repo.last_indexed_commit,
                        })
                    })
                    .collect();

                let info = serde_json::json!({
                    "repositories": repo_list,
                    "total": repo_list.len(),
                    "languages_supported": ["rust"],
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

/// Verify that the collection exists
async fn verify_collection_exists(
    collection_name: &str,
    storage_config: &codesearch_core::config::StorageConfig,
) -> std::result::Result<(), codesearch_core::Error> {
    let collection_manager = create_collection_manager(storage_config)
        .await
        .context("Failed to create collection manager")?;

    if !collection_manager
        .collection_exists(collection_name)
        .await
        .context("Failed to check if collection exists")?
    {
        return Err(Error::config(format!(
            "Collection '{collection_name}' does not exist. Please run 'codesearch serve' or 'codesearch index' to initialize."
        )));
    }

    Ok(())
}

/// Client handles for server operations
struct ServerClients {
    storage: Arc<dyn StorageClient>,
    postgres: Arc<dyn PostgresClientTrait>,
    embedding_manager: Arc<EmbeddingManager>,
}

/// Initialize all server clients
async fn initialize_server_clients(
    config: &Config,
    repository_id: uuid::Uuid,
    collection_name: &str,
) -> std::result::Result<ServerClients, codesearch_core::Error> {
    let storage = create_storage_client(&config.storage, collection_name)
        .await
        .context("Failed to create storage client")?;

    let embedding_manager = crate::storage_init::create_embedding_manager(config).await?;

    let postgres = codesearch_storage::create_postgres_client(&config.storage)
        .await
        .context("Failed to connect to Postgres")?;

    info!("Using repository ID: {repository_id}");

    Ok(ServerClients {
        storage,
        postgres,
        embedding_manager,
    })
}

/// Run catch-up indexing for offline changes
async fn run_catchup_indexing(
    repo_root: &PathBuf,
    repository_id: uuid::Uuid,
    clients: &ServerClients,
) -> std::result::Result<(), codesearch_core::Error> {
    info!("Checking for offline changes...");
    let git_repo = codesearch_watcher::GitRepository::open(repo_root)
        .context("Failed to open git repository")?;

    codesearch_indexer::catch_up_from_git(
        repo_root,
        repository_id,
        &clients.postgres,
        &clients.embedding_manager,
        &git_repo,
    )
    .await
    .context("Catch-up indexing failed")?;

    Ok(())
}

/// Setup and start the file watcher
async fn setup_file_watcher(
    repo_root: &PathBuf,
    repository_id: uuid::Uuid,
    clients: &ServerClients,
) -> std::result::Result<
    (
        FileWatcher,
        tokio::task::JoinHandle<codesearch_core::Result<()>>,
    ),
    codesearch_core::Error,
> {
    info!("Starting filesystem watcher...");
    let watcher_config = WatcherConfig::builder()
        .debounce_ms(DEFAULT_DEBOUNCE_MS)
        .max_file_size(MAX_FILE_SIZE_BYTES)
        .events_per_batch(100)
        .build();

    let mut watcher = FileWatcher::new(watcher_config).context("Failed to create file watcher")?;

    let event_rx = watcher
        .watch(repo_root)
        .await
        .context("Failed to start watching repository")?;

    let watcher_task = codesearch_indexer::start_watching(
        event_rx,
        repository_id,
        repo_root.clone(),
        clients.embedding_manager.clone(),
        clients.postgres.clone(),
    );

    Ok((watcher, watcher_task))
}

/// Run the MCP server implementation (single-repository wrapper for multi-repo infrastructure)
pub(crate) async fn run_server_impl(
    config: Config,
    repo_root: PathBuf,
    repository_id: Uuid,
    collection_name: String,
) -> std::result::Result<(), codesearch_core::Error> {
    verify_collection_exists(&collection_name, &config.storage).await?;

    let clients = initialize_server_clients(&config, repository_id, &collection_name).await?;

    run_catchup_indexing(&repo_root, repository_id, &clients).await?;

    let (watcher, _watcher_task) = setup_file_watcher(&repo_root, repository_id, &clients).await?;
    // Note: watcher_task is spawned and will be cleaned up when watcher is stopped

    // Wrap single repository in multi-repo structure
    let mut repositories = HashMap::new();
    let last_indexed_commit = clients
        .postgres
        .get_last_indexed_commit(repository_id)
        .await
        .context("Failed to get last indexed commit")?;

    repositories.insert(
        repository_id,
        RepositoryInfo {
            repository_id,
            repository_root: repo_root,
            collection_name,
            storage_client: clients.storage,
            last_indexed_commit,
        },
    );

    let repositories = Arc::new(RwLock::new(repositories));

    let mut watchers = HashMap::new();
    watchers.insert(repository_id, watcher);
    let watchers = Arc::new(RwLock::new(watchers));

    let mcp_server = CodeSearchMcpServer::new(
        repositories,
        clients.embedding_manager,
        clients.postgres,
        watchers.clone(),
    );

    run_mcp_server_with_shutdown_multi(mcp_server, watchers, config.server.port).await?;

    Ok(())
}

/// Run multi-repository MCP server
pub(crate) async fn run_multi_repo_server(
    config: Config,
    all_repos: Vec<(Uuid, String, PathBuf)>,
    postgres_client: Arc<dyn PostgresClientTrait>,
) -> std::result::Result<(), codesearch_core::Error> {
    info!("Initializing multi-repository MCP server...");

    let embedding_manager = crate::storage_init::create_embedding_manager(&config).await?;

    let mut repositories = HashMap::new();
    let collection_manager = create_collection_manager(&config.storage).await?;

    for (repository_id, collection_name, repo_path) in all_repos {
        if !collection_manager
            .collection_exists(&collection_name)
            .await?
        {
            tracing::warn!(
                "Collection '{}' for repository {} does not exist in Qdrant, skipping",
                collection_name,
                repo_path.display()
            );
            continue;
        }

        let storage_client = create_storage_client(&config.storage, &collection_name)
            .await
            .context("Failed to create storage client")?;

        let last_indexed_commit = postgres_client
            .get_last_indexed_commit(repository_id)
            .await
            .context("Failed to get last indexed commit")?;

        repositories.insert(
            repository_id,
            RepositoryInfo {
                repository_id,
                repository_root: repo_path.clone(),
                collection_name: collection_name.clone(),
                storage_client,
                last_indexed_commit,
            },
        );

        info!(
            "Loaded repository: {} ({}) at {}",
            collection_name,
            repository_id,
            repo_path.display()
        );
    }

    if repositories.is_empty() {
        return Err(Error::config(
            "No valid repositories found to serve.\n\
            Run 'codesearch index' from a git repository to create an index."
                .to_string(),
        ));
    }

    let repositories = Arc::new(RwLock::new(repositories));

    info!("Starting file watchers for all repositories...");
    let watchers = start_all_watchers(
        &repositories,
        embedding_manager.clone(),
        postgres_client.clone(),
    )
    .await?;

    info!("All watchers started successfully");

    let mcp_server = CodeSearchMcpServer::new(
        repositories,
        embedding_manager,
        postgres_client,
        watchers.clone(),
    );

    run_mcp_server_with_shutdown_multi(mcp_server, watchers, config.server.port).await
}

/// Start file watchers for all repositories
async fn start_all_watchers(
    repositories: &Arc<RwLock<HashMap<Uuid, RepositoryInfo>>>,
    embedding_manager: Arc<EmbeddingManager>,
    postgres_client: Arc<dyn PostgresClientTrait>,
) -> std::result::Result<Arc<RwLock<HashMap<Uuid, FileWatcher>>>, codesearch_core::Error> {
    let repos = repositories.read().await;
    let mut watchers = HashMap::new();

    for (repo_id, repo_info) in repos.iter() {
        info!(
            "Setting up watcher for {}",
            repo_info.repository_root.display()
        );

        if let Ok(git_repo) = codesearch_watcher::GitRepository::open(&repo_info.repository_root) {
            info!(
                "Running catch-up indexing for {}",
                repo_info.repository_root.display()
            );
            codesearch_indexer::catch_up_from_git(
                &repo_info.repository_root,
                *repo_id,
                &postgres_client,
                &embedding_manager,
                &git_repo,
            )
            .await
            .context(format!(
                "Catch-up indexing failed for {}",
                repo_info.repository_root.display()
            ))?;
        }

        let watcher_config = WatcherConfig::builder()
            .debounce_ms(DEFAULT_DEBOUNCE_MS)
            .max_file_size(MAX_FILE_SIZE_BYTES)
            .events_per_batch(100)
            .build();

        let mut watcher =
            FileWatcher::new(watcher_config).context("Failed to create file watcher")?;

        let event_rx = watcher
            .watch(&repo_info.repository_root)
            .await
            .context(format!(
                "Failed to watch repository at {}",
                repo_info.repository_root.display()
            ))?;

        let repo_id_clone = *repo_id;
        let repo_root_clone = repo_info.repository_root.clone();
        let embedding_manager_clone = embedding_manager.clone();
        let postgres_client_clone = postgres_client.clone();

        tokio::spawn(async move {
            let result = codesearch_indexer::start_watching(
                event_rx,
                repo_id_clone,
                repo_root_clone.clone(),
                embedding_manager_clone,
                postgres_client_clone,
            )
            .await;

            if let Err(e) = result {
                tracing::error!(
                    "Watcher task failed for repository at {}: {e}",
                    repo_root_clone.display()
                );
            }
        });

        watchers.insert(*repo_id, watcher);
        info!(
            "Watcher started for {}",
            repo_info.repository_root.display()
        );
    }

    Ok(Arc::new(RwLock::new(watchers)))
}

/// Run MCP server with shutdown handling for multi-repo mode
async fn run_mcp_server_with_shutdown_multi(
    server: CodeSearchMcpServer,
    watchers: Arc<RwLock<HashMap<Uuid, FileWatcher>>>,
    port: u16,
) -> std::result::Result<(), codesearch_core::Error> {
    let config = StreamableHttpServerConfig {
        sse_keep_alive: Some(std::time::Duration::from_secs(30)),
        stateful_mode: false,
    };

    let session_manager = Arc::new(LocalSessionManager::default());

    let server_clone = server.clone();
    let http_service =
        StreamableHttpService::new(move || Ok(server_clone.clone()), session_manager, config);

    let app = Router::new().nest_service("/mcp", http_service);

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| Error::config(format!("Failed to bind to {addr}: {e}")))?;

    println!("🚀 Starting multi-repository MCP server on http://{addr}/mcp");
    info!("MCP server listening on http://{addr}/mcp");

    let server_task = tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .map_err(|e| Error::config(format!("Server error: {e}")))
    });

    match tokio::signal::ctrl_c().await {
        Ok(()) => {
            info!("Received Ctrl+C, initiating graceful shutdown");
        }
        Err(e) => {
            tracing::error!("Error setting up signal handler: {e}");
        }
    }

    server_task.abort();
    let _ = server_task.await;

    info!("Stopping all file watchers...");
    let mut watchers_guard = watchers.write().await;
    for (repo_id, watcher) in watchers_guard.iter_mut() {
        info!("Stopping watcher for repository {}", repo_id);
        if let Err(e) = watcher.stop().await {
            tracing::error!("Error stopping watcher for {}: {e}", repo_id);
        }
    }

    info!("Codesearch MCP server shut down successfully");

    Ok(())
}
