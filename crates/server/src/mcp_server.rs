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
    tool, tool_handler, tool_router, RoleServer, ServerHandler, ServiceExt,
};
use serde::Deserialize;
use std::{path::PathBuf, sync::Arc};
use tracing::info;

/// MCP server for codesearch semantic code search
#[derive(Clone)]
#[allow(dead_code)]
struct CodeSearchMcpServer {
    repository_id: uuid::Uuid,
    repository_root: PathBuf,
    collection_name: String,
    embedding_manager: Arc<EmbeddingManager>,
    storage_client: Arc<dyn StorageClient>,
    postgres_client: Arc<dyn PostgresClientTrait>,
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
    #[tool(
        description = "Search for code entities semantically using natural language queries. \
                          Returns similar functions, classes, and other code constructs with full \
                          details including content, documentation, and signature."
    )]
    async fn search_code(
        &self,
        Parameters(request): Parameters<SearchCodeRequest>,
    ) -> std::result::Result<CallToolResult, McpError> {
        // Validate limit
        let limit = request.limit.unwrap_or(10).clamp(1, 100);

        // Extract query to avoid clone
        let query_text = request.query;

        // Generate query embedding
        let embeddings = self
            .embedding_manager
            .embed(vec![query_text.clone()])
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

        // Search Qdrant
        let results = self
            .storage_client
            .search_similar(query_embedding, limit, Some(filters))
            .await
            .map_err(|e| {
                McpError::new(
                    ErrorCode::INTERNAL_ERROR,
                    format!("Search failed: {e}"),
                    None,
                )
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
            "query": query_text,
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
        storage_client: Arc<dyn StorageClient>,
        postgres_client: Arc<dyn PostgresClientTrait>,
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
                    uri: "codesearch://repo/info".to_string(),
                    name: "Repository Information".to_string(),
                    title: None,
                    description: Some("Current repository metadata and configuration".to_string()),
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
            "Collection '{collection_name}' does not exist. Please run 'codesearch init' first."
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
) -> std::result::Result<(ServerClients, uuid::Uuid), codesearch_core::Error> {
    let storage = create_storage_client(&config.storage, &config.storage.collection_name)
        .await
        .context("Failed to create storage client")?;

    let embedding_manager = crate::storage_init::create_embedding_manager(config).await?;

    let postgres = codesearch_storage::create_postgres_client(&config.storage)
        .await
        .context("Failed to connect to Postgres")?;

    let repository_id = postgres
        .get_repository_id(&config.storage.collection_name)
        .await
        .context("Failed to query repository")?
        .ok_or_else(|| {
            Error::config(format!(
                "Repository not found for collection '{}'. Run 'codesearch init' first.",
                config.storage.collection_name
            ))
        })?;

    info!("Repository ID: {repository_id}");

    Ok((
        ServerClients {
            storage,
            postgres,
            embedding_manager,
        },
        repository_id,
    ))
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
        .debounce_ms(500)
        .max_file_size(10 * 1024 * 1024) // 10MB
        .batch_size(100)
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

/// Run MCP server with shutdown handling
async fn run_mcp_server_with_shutdown(
    server: CodeSearchMcpServer,
    mut watcher: FileWatcher,
    watcher_task: tokio::task::JoinHandle<codesearch_core::Result<()>>,
) -> std::result::Result<(), codesearch_core::Error> {
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

    println!("🚀 Starting MCP server on stdio...");
    let transport = (tokio::io::stdin(), tokio::io::stdout());
    let server_handle = server
        .serve(transport)
        .await
        .map_err(|e| Error::config(format!("Failed to start MCP server: {e}")))?;

    info!("MCP server connected and running");

    tokio::select! {
        quit_reason = server_handle.waiting() => {
            match quit_reason {
                Ok(reason) => info!("MCP server stopped normally. Reason: {reason:?}"),
                Err(e) => tracing::error!("MCP server error: {e}"),
            }
        }
        _ = shutdown_rx.recv() => {
            info!("Shutdown signal received");
        }
    }

    info!("Stopping file watcher...");
    watcher.stop().await?;
    match watcher_task.await {
        Ok(Ok(())) => {}
        Ok(Err(e)) => tracing::error!("Watcher task error: {e}"),
        Err(e) => tracing::error!("Watcher task join error: {e}"),
    }

    info!("Codesearch MCP server shut down successfully");

    Ok(())
}

/// Run the MCP server implementation
pub(crate) async fn run_server_impl(
    config: Config,
) -> std::result::Result<(), codesearch_core::Error> {
    verify_collection_exists(&config.storage.collection_name, &config.storage).await?;

    let (clients, repository_id) = initialize_server_clients(&config).await?;

    let repo_root = crate::storage_init::find_repository_root()?;

    run_catchup_indexing(&repo_root, repository_id, &clients).await?;

    let (watcher, watcher_task) = setup_file_watcher(&repo_root, repository_id, &clients).await?;

    let mcp_server = CodeSearchMcpServer::new(
        repository_id,
        repo_root,
        config.storage.collection_name.clone(),
        clients.embedding_manager,
        clients.storage,
        clients.postgres,
    );

    run_mcp_server_with_shutdown(mcp_server, watcher, watcher_task).await?;

    Ok(())
}
