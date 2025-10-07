//! MCP server for semantic code search
//!
//! This crate provides the MCP (Model Context Protocol) server implementation
//! for codesearch. It integrates filesystem watching for real-time index updates.

#![deny(warnings)]
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]

// All modules are private (will be created in later phases)
mod catch_up;
mod file_watcher;
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
///
/// # Returns
///
/// Returns `Ok(())` on clean shutdown, or an error if startup fails.
pub async fn run_server(config: codesearch_core::config::Config) -> Result<()> {
    // Stub implementation - will be filled in Phase 2
    mcp_server::run_server_impl(config).await
}
