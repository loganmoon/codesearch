//! MCP tool definitions for codesearch
//!
//! Defines the `agentic_code_search` tool schema.

use schemars::JsonSchema;
use serde::Deserialize;

/// Request schema for the agentic_code_search MCP tool
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct AgenticCodeSearchInput {
    /// Natural language search query describing what you're looking for.
    /// Examples: "authentication middleware", "error handling in API routes",
    /// "functions that call the database"
    #[schemars(description = "Natural language search query")]
    pub query: String,

    /// Override repository selection. If omitted, infers from current working directory.
    /// - Single repo: `["repo-name"]` or `["uuid"]`
    /// - Multiple repos: `["repo-a", "repo-b"]`
    /// - All indexed: `["all"]`
    #[schemars(
        description = "Repository names, UUIDs, or paths to search. Use [\"all\"] for all indexed repos. Omit to infer from CWD."
    )]
    pub repositories: Option<Vec<String>>,

    /// Force full content in results instead of adaptive summaries
    #[schemars(description = "Force full content output instead of summaries")]
    pub verbose: Option<bool>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_input_deserialization() {
        let json = r#"{
            "query": "authentication handling",
            "repositories": ["repo-a"],
            "verbose": true
        }"#;

        let input: AgenticCodeSearchInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.query, "authentication handling");
        assert_eq!(input.repositories, Some(vec!["repo-a".to_string()]));
        assert_eq!(input.verbose, Some(true));
    }

    #[test]
    fn test_minimal_input() {
        let json = r#"{"query": "search term"}"#;
        let input: AgenticCodeSearchInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.query, "search term");
        assert!(input.repositories.is_none());
        assert!(input.verbose.is_none());
    }
}
