//! Error types for the MCP server

use thiserror::Error;

/// Result type alias for MCP operations
pub type Result<T> = std::result::Result<T, McpError>;

/// Errors that can occur in the MCP server
#[derive(Debug, Error)]
pub enum McpError {
    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Repository inference failed: {0}")]
    RepositoryInference(String),

    #[error("Search failed: {0}")]
    Search(String),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("MCP transport error: {0}")]
    Transport(String),
}

impl McpError {
    /// Convert to MCP tool error format (isError: true response)
    pub fn to_tool_error_message(&self) -> String {
        match self {
            McpError::Config(msg) => {
                format!("Configuration error: {msg}\n\nPlease check your codesearch configuration.")
            }
            McpError::RepositoryInference(msg) => {
                format!(
                    "Repository not found: {msg}\n\n\
                    Hint: Run 'codesearch index' from a git repository first, \
                    or specify repositories explicitly in the request."
                )
            }
            McpError::Search(msg) => {
                format!("Search failed: {msg}")
            }
            McpError::Serialization(e) => {
                format!("Failed to format results: {e}")
            }
            McpError::Transport(msg) => {
                format!("Transport error: {msg}")
            }
        }
    }
}
