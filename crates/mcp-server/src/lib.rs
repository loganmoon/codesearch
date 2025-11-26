//! MCP Server for Codesearch
//!
//! Provides a Model Context Protocol server with a single `agentic_code_search` tool
//! that leverages the multi-agent orchestration from `codesearch-agentic-search`.

#![deny(warnings)]
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]

mod error;
mod output_formatter;
mod repository_inference;
mod server;
mod tool;

pub use error::{McpError, Result};
pub use repository_inference::IndexedRepository;
pub use server::run_mcp_server;
