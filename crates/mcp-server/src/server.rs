//! MCP Server implementation for codesearch
//!
//! Implements the MCP server with a single `agentic_code_search` tool
//! using rmcp SDK with stdio transport.

use crate::error::McpError;
use crate::output_formatter::format_response;
use crate::repository_inference::{resolve_repositories, IndexedRepository};
use crate::tool::AgenticCodeSearchInput;
use codesearch_agentic_search::{
    AgenticSearchConfig, AgenticSearchOrchestrator, AgenticSearchRequest,
};
use codesearch_core::SearchApi;
use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{
        CallToolResult, Content, ErrorCode, ErrorData, Implementation, ProtocolVersion,
        ServerCapabilities, ServerInfo,
    },
    tool, tool_handler, tool_router, ServerHandler, ServiceExt,
};
use std::path::PathBuf;
use std::sync::Arc;
use tracing::info;

/// MCP Server for codesearch
#[derive(Clone)]
pub struct CodesearchMcpServer {
    tool_router: ToolRouter<Self>,
    search_api: Arc<dyn SearchApi>,
    agentic_config: AgenticSearchConfig,
    indexed_repositories: Vec<IndexedRepository>,
    cwd: PathBuf,
}

impl std::fmt::Debug for CodesearchMcpServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CodesearchMcpServer")
            .field("search_api", &"<SearchApi>")
            .field("agentic_config", &self.agentic_config)
            .field("indexed_repositories", &self.indexed_repositories)
            .field("cwd", &self.cwd)
            .finish()
    }
}

impl CodesearchMcpServer {
    /// Create a new MCP server instance
    pub fn new(
        search_api: Arc<dyn SearchApi>,
        agentic_config: AgenticSearchConfig,
        indexed_repositories: Vec<IndexedRepository>,
        cwd: PathBuf,
    ) -> Self {
        Self {
            tool_router: Self::tool_router(),
            search_api,
            agentic_config,
            indexed_repositories,
            cwd,
        }
    }
}

#[tool_router]
impl CodesearchMcpServer {
    /// Search code semantically using multi-agent orchestration.
    ///
    /// Uses Claude models to iteratively search through indexed repositories,
    /// combining semantic search, full-text search, and code graph traversal
    /// to find the most relevant code entities.
    #[tool(
        name = "agentic_code_search",
        description = "Search code using AI-powered multi-agent orchestration. Combines semantic search, full-text search, and code graph traversal to find relevant functions, classes, and modules. Returns code snippets with file locations."
    )]
    async fn agentic_code_search(
        &self,
        Parameters(input): Parameters<AgenticCodeSearchInput>,
    ) -> Result<CallToolResult, ErrorData> {
        info!("Executing agentic_code_search: query={}", input.query);

        // Resolve repositories
        let repository_ids =
            resolve_repositories(&input.repositories, &self.cwd, &self.indexed_repositories)
                .map_err(|e| to_mcp_error(&e))?;

        // Create orchestrator
        let orchestrator =
            AgenticSearchOrchestrator::new(self.search_api.clone(), self.agentic_config.clone())
                .await
                .map_err(|e| to_mcp_error_str(&format!("Failed to create orchestrator: {e}")))?;

        // Build search request
        let request = AgenticSearchRequest {
            query: input.query.clone(),
            force_sonnet: false,
            repository_ids,
        };

        // Execute search
        let response = orchestrator
            .search(request)
            .await
            .map_err(|e| to_mcp_error_str(&format!("Search failed: {e}")))?;

        // Format response adaptively
        let verbose = input.verbose.unwrap_or(false);
        let formatted = format_response(response, verbose);

        // Serialize to JSON
        let json_output = serde_json::to_string_pretty(&formatted)
            .map_err(|e| to_mcp_error_str(&format!("Failed to serialize results: {e}")))?;

        info!(
            "agentic_code_search completed: {} results",
            formatted.results.len()
        );

        Ok(CallToolResult::success(vec![Content::text(json_output)]))
    }
}

#[tool_handler]
impl ServerHandler for CodesearchMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2024_11_05,
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation {
                name: "codesearch-mcp".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                ..Default::default()
            },
            instructions: Some(
                "Semantic code search with multi-agent orchestration. \
                Use the agentic_code_search tool to find functions, classes, and modules \
                in indexed repositories using natural language queries."
                    .to_string(),
            ),
        }
    }
}

/// Convert McpError to rmcp ErrorData
fn to_mcp_error(err: &McpError) -> ErrorData {
    ErrorData {
        code: ErrorCode::INTERNAL_ERROR,
        message: err.to_tool_error_message().into(),
        data: None,
    }
}

/// Convert string error to rmcp ErrorData
fn to_mcp_error_str(msg: &str) -> ErrorData {
    ErrorData {
        code: ErrorCode::INTERNAL_ERROR,
        message: msg.to_string().into(),
        data: None,
    }
}

/// Run the MCP server with stdio transport
///
/// This is the main entry point for the `codesearch mcp` command.
/// It sets up the MCP server and runs it until the client disconnects.
pub async fn run_mcp_server(
    search_api: Arc<dyn SearchApi>,
    agentic_config: AgenticSearchConfig,
    indexed_repositories: Vec<IndexedRepository>,
    cwd: PathBuf,
) -> crate::Result<()> {
    info!(
        "Starting MCP server with {} indexed repositories",
        indexed_repositories.len()
    );

    let server = CodesearchMcpServer::new(search_api, agentic_config, indexed_repositories, cwd);

    // Start server with stdio transport
    let service = server
        .serve(rmcp::transport::stdio())
        .await
        .map_err(|e| McpError::Transport(e.to_string()))?;

    info!("MCP server started, waiting for client requests");

    // Wait for the server to complete (client disconnect or error)
    service
        .waiting()
        .await
        .map_err(|e| McpError::Transport(e.to_string()))?;

    info!("MCP server shutting down");
    Ok(())
}
