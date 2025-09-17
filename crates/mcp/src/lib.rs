//! MCP (Model Context Protocol) server implementation
//!
//! This crate provides an MCP server that integrates with the codesearch
//! storage and embedding systems to provide semantic code search and analysis
//! capabilities through the Model Context Protocol.

#![deny(warnings)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]

mod handlers;
mod types;

use codesearch_core::error::Result;
use codesearch_embeddings::provider::EmbeddingProvider;
use codesearch_storage::{StorageClient, StorageManager};
use rmcp::ServerHandler;
use std::sync::Arc;

/// Create a new MCP server with the given storage and embedding dependencies
///
/// # Arguments
/// * `storage_config` - Configuration for the storage manager
/// * `storage_client` - Client for storage operations
/// * `embedding_provider` - Provider for generating embeddings
///
/// # Returns
/// A configured MCP server handler ready to process requests
///
/// # Example
/// ```ignore
/// let storage_manager = create_storage(storage_config);
/// let storage_client = create_storage_client("localhost", 8080).await?;
/// let embedding_provider = create_embedding_provider(embedding_config).await?;
///
/// let server = create_mcp_server(
///     Arc::new(storage_manager),
///     Arc::new(storage_client),
///     Arc::new(embedding_provider),
/// ).await?;
/// ```
pub async fn create_mcp_server(
    storage_manager: Arc<dyn StorageManager>,
    storage_client: Arc<dyn StorageClient>,
    embedding_provider: Arc<dyn EmbeddingProvider>,
) -> Result<impl ServerHandler> {
    Ok(handlers::McpServer::new(
        storage_manager,
        storage_client,
        embedding_provider,
    ))
}
