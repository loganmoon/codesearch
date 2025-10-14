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

/// Run the MCP server with the given configuration.
///
/// This is the only public function in this crate. It:
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
///
/// # Returns
///
/// Returns `Ok(())` on clean shutdown, or an error if startup fails.
pub async fn run_server(
    config: codesearch_core::config::Config,
    repo_root: std::path::PathBuf,
    repository_id: uuid::Uuid,
) -> Result<()> {
    mcp_server::run_server_impl(config, repo_root, repository_id).await
}
