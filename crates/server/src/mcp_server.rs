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
use std::{net::SocketAddr, path::PathBuf, sync::Arc};
use tracing::info;

const DEFAULT_DEBOUNCE_MS: u64 = 500;
const MAX_FILE_SIZE_BYTES: u64 = 10 * 1024 * 1024; // 10MB

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
}

fn default_limit() -> Option<usize> {
    Some(10)
}

#[tool_router]
impl CodeSearchMcpServer {
    #[tool(
        description = "Semantic code search using embeddings. Works best with code-like queries \
                          (function signatures, type names, code patterns) rather than abstract descriptions. \
                          Examples: 'async fn process(data: Vec<T>)', 'impl StorageClient', 'QueryMatch source'. \
                          Returns matching functions, structs, impls, and other code entities with full details."
    )]
    async fn search_code(
        &self,
        Parameters(request): Parameters<SearchCodeRequest>,
    ) -> std::result::Result<CallToolResult, McpError> {
        // Validate limit
        let limit = request.limit.unwrap_or(10).clamp(1, 100);

        // Extract query to avoid clone
        let query_text = request.query;

        // Format query with BGE instruction for proper semantic matching
        // BGE-code-v1 requires this format to distinguish queries from documents
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
        let mut entities_map: std::collections::HashMap<String, _> =
            std::collections::HashMap::with_capacity(entities_vec.len());
        for entity in entities_vec {
            entities_map.insert(entity.entity_id.clone(), entity);
        }

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
) -> std::result::Result<ServerClients, codesearch_core::Error> {
    let storage = create_storage_client(&config.storage, &config.storage.collection_name)
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

/// Run MCP server with shutdown handling
async fn run_mcp_server_with_shutdown(
    server: CodeSearchMcpServer,
    mut watcher: FileWatcher,
    watcher_task: tokio::task::JoinHandle<codesearch_core::Result<()>>,
    port: u16,
) -> std::result::Result<(), codesearch_core::Error> {
    // Create HTTP service configuration
    let config = StreamableHttpServerConfig {
        sse_keep_alive: Some(std::time::Duration::from_secs(30)),
        stateful_mode: false, // Stateless for localhost-only deployment
    };

    // Create session manager
    let session_manager = Arc::new(LocalSessionManager::default());

    // Create StreamableHttpService
    let server_clone = server.clone();
    let http_service =
        StreamableHttpService::new(move || Ok(server_clone.clone()), session_manager, config);

    // Create axum router
    let app = Router::new().nest_service("/mcp", http_service);

    // Bind to localhost only (127.0.0.1)
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| Error::config(format!("Failed to bind to {addr}: {e}")))?;

    println!("🚀 Starting MCP server on http://{addr}/mcp");
    info!("MCP server listening on http://{addr}/mcp");

    // Spawn server task
    let server_task = tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .map_err(|e| Error::config(format!("Server error: {e}")))
    });

    // Wait for Ctrl+C
    match tokio::signal::ctrl_c().await {
        Ok(()) => {
            info!("Received Ctrl+C, initiating graceful shutdown");
        }
        Err(e) => {
            tracing::error!("Error setting up signal handler: {e}");
        }
    }

    // Abort server task
    server_task.abort();
    let _ = server_task.await;

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
    repo_root: PathBuf,
    repository_id: uuid::Uuid,
) -> std::result::Result<(), codesearch_core::Error> {
    verify_collection_exists(&config.storage.collection_name, &config.storage).await?;

    let clients = initialize_server_clients(&config, repository_id).await?;

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

    run_mcp_server_with_shutdown(mcp_server, watcher, watcher_task, config.server.port).await?;

    Ok(())
}
