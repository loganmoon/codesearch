//! REST API server for semantic code search
//!
//! This crate provides the REST API server for codesearch. It integrates filesystem
//! watching for real-time index updates.

#![deny(warnings)]
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]

use codesearch_core::config::Config;
use codesearch_core::error::ResultExt;
use codesearch_storage::PostgresClientTrait;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::api::{BackendClients, RepositoryInfo, SearchConfig};

// Public modules
pub mod api;
pub mod graph_queries;

// Private modules
mod rest_server;

// Re-export error types from core
pub use codesearch_core::error::{Error, Result};

/// Run the REST API server
///
/// This is the main entry point for starting the REST API server. It handles:
/// - Embedding manager initialization
/// - Reranker initialization (if enabled)
/// - Storage client creation for each repository
/// - Neo4j client initialization (if enabled)
/// - Server startup with graceful shutdown
///
/// # Arguments
/// * `config` - Application configuration
/// * `valid_repos` - List of (repository_id, collection_name, path) tuples
/// * `postgres_client` - PostgreSQL client for database operations
/// * `enable_agentic` - Whether agentic search endpoint is enabled
pub async fn run_rest_server(
    config: Config,
    valid_repos: Vec<(Uuid, String, PathBuf)>,
    postgres_client: Arc<dyn PostgresClientTrait>,
    enable_agentic: bool,
) -> Result<()> {
    info!(
        "Starting multi-repository REST API server with {} valid repositories",
        valid_repos.len()
    );

    // Initialize embedding manager
    let embedding_manager =
        codesearch_embeddings::create_embedding_manager_from_app_config(&config.embeddings)
            .await
            .context("Failed to create embedding manager")?;

    // Initialize reranker if enabled
    let reranker: Option<Arc<dyn codesearch_reranking::RerankerProvider>> =
        if config.reranking.enabled {
            match codesearch_reranking::create_reranker_provider(&config.reranking).await {
                Ok(provider) => {
                    info!("Reranker initialized successfully");
                    Some(provider)
                }
                Err(e) => {
                    warn!("Failed to initialize reranker: {e}");
                    warn!("Reranking will be disabled for this session");
                    None
                }
            }
        } else {
            None
        };

    // Load repository storage clients
    let mut repositories = HashMap::new();
    let mut first_storage_client = None;

    for (repository_id, collection_name, repo_path) in valid_repos {
        let storage_client =
            codesearch_storage::create_storage_client(&config.storage, &collection_name)
                .await
                .context("Failed to create storage client")?;

        // Store first storage client for BackendClients (temporary solution)
        if first_storage_client.is_none() {
            first_storage_client = Some(storage_client.clone());
        }

        let last_indexed_commit = postgres_client
            .get_last_indexed_commit(repository_id)
            .await
            .context("Failed to get last indexed commit")?;

        let repo_info = RepositoryInfo {
            repository_id,
            repository_name: collection_name.clone(),
            repository_path: repo_path.display().to_string(),
            collection_name,
            last_indexed_commit,
        };

        repositories.insert(repository_id, repo_info);
    }

    let qdrant_client = first_storage_client.ok_or_else(|| {
        Error::invalid_input("No valid repositories found to create storage client")
    })?;

    // Initialize Neo4j client if enabled
    let neo4j_client = match codesearch_storage::create_neo4j_client(&config.storage).await {
        Ok(client) => {
            info!("Neo4j client initialized successfully");
            Some(client)
        }
        Err(e) => {
            warn!("Failed to initialize Neo4j client: {e}");
            warn!("Graph queries will be disabled for this session");
            None
        }
    };

    // Build AppState
    let app_state = rest_server::AppState {
        clients: Arc::new(BackendClients {
            postgres: postgres_client,
            qdrant: qdrant_client,
            neo4j: neo4j_client,
            embedding_manager,
            reranker,
        }),
        config: Arc::new(SearchConfig {
            hybrid_search: config.hybrid_search.clone(),
            reranking: config.reranking.clone(),
            default_bge_instruction: config.embeddings.default_bge_instruction.clone(),
            max_batch_size: config.storage.max_entities_per_db_operation,
        }),
        repositories: Arc::new(RwLock::new(repositories)),
        enable_agentic,
    };

    // Build router
    let app = rest_server::build_router(app_state, &config.server);

    // Start server
    let addr = SocketAddr::from(([127, 0, 0, 1], config.server.port));
    info!("Starting REST API server on http://{}", addr);
    info!("API documentation available at http://{}/swagger-ui", addr);

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .context("Failed to bind to address")?;

    // Run server with graceful shutdown
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            tokio::signal::ctrl_c()
                .await
                .map_err(|e| error!("Failed to listen for shutdown signal: {e}"))
                .ok();
            info!("Shutdown signal received, stopping server...");
        })
        .await
        .context("REST server error")?;

    Ok(())
}
