//! MCP server for semantic code search
//!
//! This crate provides the MCP (Model Context Protocol) server implementation
//! for codesearch. It integrates filesystem watching for real-time index updates.

#![deny(warnings)]
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]

// All modules are private
mod mcp_server;
mod storage_init;

// Re-export error types from core
pub use codesearch_core::error::{Error, Result};

/// Run the MCP server with the given configuration (single repository mode).
///
/// This function:
/// 1. Creates storage, embedding, and Postgres clients
/// 2. Verifies collection exists and gets repository metadata
/// 3. Runs catch-up indexing for offline changes
/// 4. Starts filesystem watcher for real-time updates
/// 5. Starts MCP server on stdio
/// 6. Handles graceful shutdown on Ctrl+C
///
/// # Arguments
///
/// * `config` - Application configuration with storage, embeddings, and repository settings
/// * `repo_root` - Path to the repository root directory
/// * `repository_id` - UUID of the repository in the database
/// * `collection_name` - Collection name for this repository
///
/// # Returns
///
/// Returns `Ok(())` on clean shutdown, or an error if startup fails.
pub async fn run_server(
    config: codesearch_core::config::Config,
    repo_root: std::path::PathBuf,
    repository_id: uuid::Uuid,
    collection_name: String,
) -> Result<()> {
    mcp_server::run_server_impl(config, repo_root, repository_id, collection_name).await
}

/// Run the MCP server in multi-repository mode.
///
/// This function:
/// 1. Loads all indexed repositories from the database
/// 2. Creates storage clients for each repository
/// 3. Starts file watchers for all repositories
/// 4. Runs catch-up indexing for each repository
/// 5. Starts the multi-repository MCP server
/// 6. Handles graceful shutdown on Ctrl+C
///
/// # Arguments
///
/// * `config` - Application configuration with storage, embeddings, and repository settings
/// * `all_repos` - List of (repository_id, collection_name, repository_path) tuples
/// * `postgres_client` - PostgreSQL client for database operations
///
/// # Returns
///
/// Returns `Ok(())` on clean shutdown, or an error if startup fails.
pub async fn run_multi_repo_server(
    config: codesearch_core::config::Config,
    all_repos: Vec<(uuid::Uuid, String, std::path::PathBuf)>,
    postgres_client: std::sync::Arc<dyn codesearch_storage::PostgresClientTrait>,
) -> Result<()> {
    mcp_server::run_multi_repo_server(config, all_repos, postgres_client).await
}
