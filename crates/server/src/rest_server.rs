//! REST API server implementation using Axum
//!
//! This module provides the REST API server with OpenAPI documentation,
//! integrating the service layer from codesearch-api-service.

use crate::api::{
    generate_embeddings, get_entities_batch, list_repositories, query_graph, search_agentic,
    search_fulltext, search_semantic, search_unified, AgenticSearchApiRequest,
    AgenticSearchApiResponse, BackendClients, BatchEntityRequest, BatchEntityResponse,
    EmbeddingRequest, EmbeddingResponse, FulltextSearchRequest, FulltextSearchResponse,
    GraphQueryRequest, GraphQueryResponse, ListRepositoriesResponse, RepositoryInfo, SearchConfig,
    SemanticSearchRequest, SemanticSearchResponse, UnifiedSearchRequest, UnifiedSearchResponse,
};
use axum::{
    extract::State,
    http::{HeaderValue, Method, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use codesearch_core::config::ServerConfig;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;
use uuid::Uuid;

/// Shared application state
#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) clients: Arc<BackendClients>,
    pub(crate) config: Arc<SearchConfig>,
    pub(crate) repositories: Arc<RwLock<HashMap<Uuid, RepositoryInfo>>>,
    pub(crate) enable_agentic: bool,
}

/// Build the Axum router with all endpoints
pub(crate) fn build_router(state: AppState, server_config: &ServerConfig) -> Router {
    let router = Router::new()
        // Search endpoints
        .route("/api/v1/search/semantic", post(semantic_search_handler))
        .route("/api/v1/search/fulltext", post(fulltext_search_handler))
        .route("/api/v1/search/unified", post(unified_search_handler))
        .route("/api/v1/search/agentic", post(agentic_search_handler))
        .route("/api/v1/graph/query", post(graph_query_handler))
        // Entity operations
        .route("/api/v1/entities/batch", post(entities_batch_handler))
        // Embedding generation
        .route("/api/v1/embed", post(embed_handler))
        // Repository listing
        .route("/api/v1/repositories", get(repositories_handler))
        // Health check
        .route("/health", get(health_handler))
        // OpenAPI documentation
        .merge(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", ApiDoc::openapi()));

    // Configure CORS based on allowed_origins
    let cors_layer = if server_config.allowed_origins.is_empty() {
        // CORS disabled
        CorsLayer::new()
    } else if server_config.allowed_origins.contains(&"*".to_string()) {
        // Allow all origins
        CorsLayer::permissive()
    } else {
        // Allow specific origins
        let mut cors = CorsLayer::new()
            .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
            .allow_headers([
                axum::http::header::CONTENT_TYPE,
                axum::http::header::AUTHORIZATION,
            ]);

        for origin in &server_config.allowed_origins {
            if let Ok(header_value) = HeaderValue::from_str(origin) {
                cors = cors.allow_origin(header_value);
            }
        }
        cors
    };

    router
        .layer(cors_layer)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

/// POST /api/v1/search/semantic
#[utoipa::path(
    post,
    path = "/api/v1/search/semantic",
    request_body = SemanticSearchRequest,
    responses(
        (status = 200, description = "Semantic search results", body = SemanticSearchResponse),
        (status = 400, description = "Invalid request"),
        (status = 500, description = "Internal server error")
    ),
    tag = "search"
)]
async fn semantic_search_handler(
    State(state): State<AppState>,
    Json(request): Json<SemanticSearchRequest>,
) -> Result<Json<SemanticSearchResponse>, ApiError> {
    tracing::info!(
        "Semantic search request: {} repositories, limit={}",
        request.repository_ids.as_ref().map_or(0, |ids| ids.len()),
        request.limit
    );

    let response = search_semantic(request, &state.clients, &state.config).await?;
    Ok(Json(response))
}

/// POST /api/v1/search/fulltext
#[utoipa::path(
    post,
    path = "/api/v1/search/fulltext",
    request_body = FulltextSearchRequest,
    responses(
        (status = 200, description = "Full-text search results", body = FulltextSearchResponse),
        (status = 400, description = "Invalid request"),
        (status = 500, description = "Internal server error")
    ),
    tag = "search"
)]
async fn fulltext_search_handler(
    State(state): State<AppState>,
    Json(request): Json<FulltextSearchRequest>,
) -> Result<Json<FulltextSearchResponse>, ApiError> {
    tracing::info!(
        "Full-text search request: repository={}, query='{}', limit={}",
        request.repository_id,
        request.query,
        request.limit
    );

    let response = search_fulltext(request, &state.clients.postgres).await?;
    Ok(Json(response))
}

/// POST /api/v1/search/unified
#[utoipa::path(
    post,
    path = "/api/v1/search/unified",
    request_body = UnifiedSearchRequest,
    responses(
        (status = 200, description = "Unified search results (full-text + semantic merged via RRF)", body = UnifiedSearchResponse),
        (status = 400, description = "Invalid request"),
        (status = 500, description = "Internal server error")
    ),
    tag = "search"
)]
async fn unified_search_handler(
    State(state): State<AppState>,
    Json(request): Json<UnifiedSearchRequest>,
) -> Result<Json<UnifiedSearchResponse>, ApiError> {
    tracing::info!(
        "Unified search request: repository={}, fulltext={}, semantic={}, limit={}",
        request.repository_id,
        request.enable_fulltext,
        request.enable_semantic,
        request.limit
    );

    let response = search_unified(request, &state.clients, &state.config).await?;
    Ok(Json(response))
}

/// POST /api/v1/search/agentic
#[utoipa::path(
    post,
    path = "/api/v1/search/agentic",
    request_body = AgenticSearchApiRequest,
    responses(
        (status = 200, description = "Agentic search results with multi-agent orchestration", body = AgenticSearchApiResponse),
        (status = 400, description = "Invalid request"),
        (status = 501, description = "Agentic search not enabled"),
        (status = 500, description = "Internal server error")
    ),
    tag = "search"
)]
async fn agentic_search_handler(
    State(state): State<AppState>,
    Json(request): Json<AgenticSearchApiRequest>,
) -> Result<Json<AgenticSearchApiResponse>, ApiError> {
    // Check if agentic search is enabled
    if !state.enable_agentic {
        return Err(ApiError::NotImplemented(
            "Agentic search not enabled. Start server with --enable-agentic flag".into(),
        ));
    }

    tracing::info!(
        "Agentic search request: query='{}', {} repositories",
        request.query,
        if request.repository_ids.is_empty() {
            "all".to_string()
        } else {
            request.repository_ids.len().to_string()
        }
    );

    let response = search_agentic(request, &state.clients, &state.config).await?;
    Ok(Json(response))
}

/// POST /api/v1/graph/query
#[utoipa::path(
    post,
    path = "/api/v1/graph/query",
    request_body = GraphQueryRequest,
    responses(
        (status = 200, description = "Graph query results", body = GraphQueryResponse),
        (status = 400, description = "Invalid request"),
        (status = 503, description = "Neo4j service unavailable"),
        (status = 500, description = "Internal server error")
    ),
    tag = "graph"
)]
async fn graph_query_handler(
    State(state): State<AppState>,
    Json(request): Json<GraphQueryRequest>,
) -> Result<Json<GraphQueryResponse>, ApiError> {
    tracing::info!(
        "Graph query request: repository={}, query_type={:?}",
        request.repository_id,
        request.query_type
    );

    let neo4j_client = state
        .clients
        .neo4j
        .as_ref()
        .ok_or_else(|| ApiError::ServiceUnavailable("Neo4j not available".into()))?;

    let response = query_graph(
        request,
        neo4j_client,
        &state.clients.postgres,
        &state.clients.reranker,
    )
    .await?;
    Ok(Json(response))
}

/// POST /api/v1/entities/batch
#[utoipa::path(
    post,
    path = "/api/v1/entities/batch",
    request_body = BatchEntityRequest,
    responses(
        (status = 200, description = "Batch entity retrieval results", body = BatchEntityResponse),
        (status = 400, description = "Invalid request"),
        (status = 500, description = "Internal server error")
    ),
    tag = "entities"
)]
async fn entities_batch_handler(
    State(state): State<AppState>,
    Json(request): Json<BatchEntityRequest>,
) -> Result<Json<BatchEntityResponse>, ApiError> {
    tracing::info!(
        "Batch entity request: {} entities",
        request.entity_refs.len()
    );

    let response = get_entities_batch(
        request,
        &state.clients.postgres,
        state.config.max_batch_size,
    )
    .await?;
    Ok(Json(response))
}

/// POST /api/v1/embed
#[utoipa::path(
    post,
    path = "/api/v1/embed",
    request_body = EmbeddingRequest,
    responses(
        (status = 200, description = "Generated embeddings", body = EmbeddingResponse),
        (status = 400, description = "Invalid request"),
        (status = 500, description = "Internal server error")
    ),
    tag = "embeddings"
)]
async fn embed_handler(
    State(state): State<AppState>,
    Json(request): Json<EmbeddingRequest>,
) -> Result<Json<EmbeddingResponse>, ApiError> {
    tracing::info!(
        "Embedding generation request: {} texts",
        request.texts.len()
    );

    let response = generate_embeddings(request, &state.clients.embedding_manager).await?;
    Ok(Json(response))
}

/// GET /api/v1/repositories
#[utoipa::path(
    get,
    path = "/api/v1/repositories",
    responses(
        (status = 200, description = "List of indexed repositories", body = ListRepositoriesResponse),
        (status = 500, description = "Internal server error")
    ),
    tag = "repositories"
)]
async fn repositories_handler(
    State(state): State<AppState>,
) -> Result<Json<ListRepositoriesResponse>, ApiError> {
    tracing::info!("Repository listing request");

    let response = list_repositories(&state.clients.postgres).await?;
    Ok(Json(response))
}

/// GET /health
#[utoipa::path(
    get,
    path = "/health",
    responses(
        (status = 200, description = "Service is healthy")
    ),
    tag = "health"
)]
async fn health_handler(State(state): State<AppState>) -> impl IntoResponse {
    use serde_json::json;

    let repo_count = state.repositories.read().await.len();
    let neo4j_status = if state.clients.neo4j.is_some() {
        "initialized"
    } else {
        "disabled"
    };
    let reranker_status = if state.clients.reranker.is_some() {
        "initialized"
    } else {
        "disabled"
    };
    let agentic_status = if state.enable_agentic {
        "enabled"
    } else {
        "disabled"
    };

    let health_status = json!({
        "status": "healthy",
        "version": env!("CARGO_PKG_VERSION"),
        "dependencies": {
            "postgres": {"status": "initialized"},
            "qdrant": {"status": "initialized"},
            "neo4j": {"status": neo4j_status},
            "embedding_manager": {"status": "initialized"},
            "reranker": {"status": reranker_status},
            "agentic_search": {"status": agentic_status},
            "repositories": {"count": repo_count}
        }
    });

    (StatusCode::OK, Json(health_status))
}

/// Error handling for API endpoints
#[derive(Debug)]
#[allow(dead_code)]
pub enum ApiError {
    InvalidRequest(String),
    ServiceUnavailable(String),
    NotImplemented(String),
    Internal(anyhow::Error),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match self {
            ApiError::InvalidRequest(msg) => (StatusCode::BAD_REQUEST, msg),
            ApiError::ServiceUnavailable(msg) => (StatusCode::SERVICE_UNAVAILABLE, msg),
            ApiError::NotImplemented(msg) => (StatusCode::NOT_IMPLEMENTED, msg),
            ApiError::Internal(err) => {
                // Log the full error details for debugging
                tracing::error!("Internal server error: {err:?}");
                // Return a generic message to the client to avoid information disclosure
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "An internal server error occurred".to_string(),
                )
            }
        };

        (status, Json(serde_json::json!({ "error": message }))).into_response()
    }
}

impl From<codesearch_core::error::Error> for ApiError {
    fn from(err: codesearch_core::error::Error) -> Self {
        ApiError::Internal(err.into())
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(err: anyhow::Error) -> Self {
        ApiError::Internal(err)
    }
}

/// OpenAPI documentation
#[derive(OpenApi)]
#[openapi(
    paths(
        semantic_search_handler,
        fulltext_search_handler,
        unified_search_handler,
        agentic_search_handler,
        graph_query_handler,
        entities_batch_handler,
        embed_handler,
        repositories_handler,
        health_handler
    ),
    components(schemas(
        SemanticSearchRequest,
        SemanticSearchResponse,
        FulltextSearchRequest,
        FulltextSearchResponse,
        UnifiedSearchRequest,
        UnifiedSearchResponse,
        AgenticSearchApiRequest,
        AgenticSearchApiResponse,
        crate::api::AgenticSearchApiMetadata,
        GraphQueryRequest,
        GraphQueryResponse,
        BatchEntityRequest,
        BatchEntityResponse,
        EmbeddingRequest,
        EmbeddingResponse,
        ListRepositoriesResponse
    )),
    tags(
        (name = "search", description = "Semantic, full-text, and agentic search endpoints"),
        (name = "graph", description = "Code graph query endpoints"),
        (name = "entities", description = "Entity retrieval endpoints"),
        (name = "embeddings", description = "Embedding generation endpoints"),
        (name = "repositories", description = "Repository management endpoints"),
        (name = "health", description = "Health check endpoints")
    )
)]
struct ApiDoc;
