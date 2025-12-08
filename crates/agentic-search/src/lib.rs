//! Agentic search with multi-agent orchestration and dual-track pipeline
//!
//! This crate provides intelligent code search using multiple AI agents for
//! orchestration, parallel search execution, and multi-stage reranking.
//!
//! # Public API
//!
//! This crate exports a minimal public API following the principle of limiting
//! public exports to traits, models, errors, and factory functions:
//!
//! ## Main Entry Point
//! - [`AgenticSearchOrchestrator`] - Main orchestrator that executes agentic search
//!
//! ## Request/Response Models
//! - [`AgenticSearchRequest`] - Input request model
//! - [`AgenticSearchResponse`] - Output response model with results and metadata
//! - [`AgenticSearchMetadata`] - Execution metadata (iterations, cost, etc.)
//! - [`AgenticEntity`] - Entity result enriched with retrieval source
//! - [`RetrievalSource`] - How an entity was retrieved (semantic, fulltext, graph)
//! - [`RerankingMethod`] - Which reranking method was used
//!
//! ## Configuration
//! - [`AgenticSearchConfig`] - Main configuration
//! - [`QualityGateConfig`] - Quality gate thresholds
//!
//! ## Error Handling
//! - [`AgenticSearchError`] - Error types
//! - [`Result`] - Result type alias
//!
//! All implementation details (content selection, prompts, internal types) are
//! private and not exported.

#![deny(warnings)]
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]

// Private modules - implementation details
mod config;
mod content_selection;
mod error;
mod orchestrator;
mod prompts;
mod types;
mod worker;

// Public re-exports - narrow API surface
pub use config::{AgenticSearchConfig, QualityGateConfig};
pub use error::{AgenticSearchError, Result};
pub use orchestrator::AgenticSearchOrchestrator;
pub use types::{
    AgenticEntity, AgenticSearchMetadata, AgenticSearchRequest, AgenticSearchResponse,
    RerankingMethod, RetrievalSource,
};

// ============================================================================
// Internal Utilities
// ============================================================================

/// Extract JSON from LLM response, stripping markdown and extraneous text.
///
/// Handles:
/// - Markdown code blocks: ```json\n{...}\n```
/// - Chatty prefixes: "Here's the result:\n{...}"
/// - Trailing explanations: "{...}\n\nLet me know if you need..."
/// - Nested JSON (finds outermost balanced structure)
pub(crate) fn extract_json(response: &str) -> Option<&str> {
    let trimmed = response.trim();

    // First, try to strip markdown code blocks
    let content = if trimmed.starts_with("```") {
        // Find the end of the opening fence (```json or ```)
        let after_fence = if let Some(newline_pos) = trimmed.find('\n') {
            &trimmed[newline_pos + 1..]
        } else {
            trimmed
                .strip_prefix("```json")
                .or_else(|| trimmed.strip_prefix("```"))?
        };

        // Find closing fence
        if let Some(close_pos) = after_fence.rfind("```") {
            after_fence[..close_pos].trim()
        } else {
            after_fence.trim()
        }
    } else {
        trimmed
    };

    // Find the start of JSON (first '{' or '[')
    let json_start = content.find(['{', '['])?;
    let start_char = content.chars().nth(json_start)?;
    let end_char = if start_char == '{' { '}' } else { ']' };

    // Find matching end bracket (handling nesting)
    let json_content = &content[json_start..];
    let mut depth = 0;
    let mut in_string = false;
    let mut escape_next = false;

    for (i, c) in json_content.char_indices() {
        if escape_next {
            escape_next = false;
            continue;
        }

        match c {
            '\\' if in_string => escape_next = true,
            '"' => in_string = !in_string,
            _ if in_string => {}
            c if c == start_char => depth += 1,
            c if c == end_char => {
                depth -= 1;
                if depth == 0 {
                    return Some(&json_content[..=i]);
                }
            }
            _ => {}
        }
    }

    // No balanced match found
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_json_markdown_fence() {
        let input = "```json\n{\"key\": \"value\"}\n```";
        assert_eq!(extract_json(input), Some("{\"key\": \"value\"}"));
    }

    #[test]
    fn test_extract_json_plain_fence() {
        let input = "```\n[1, 2, 3]\n```";
        assert_eq!(extract_json(input), Some("[1, 2, 3]"));
    }

    #[test]
    fn test_extract_json_no_fence() {
        let input = "{\"key\": \"value\"}";
        assert_eq!(extract_json(input), Some("{\"key\": \"value\"}"));
    }

    #[test]
    fn test_extract_json_chatty_prefix() {
        let input = "Here's the result:\n{\"key\": \"value\"}";
        assert_eq!(extract_json(input), Some("{\"key\": \"value\"}"));
    }

    #[test]
    fn test_extract_json_chatty_suffix() {
        let input = "{\"key\": \"value\"}\n\nLet me know if you need more!";
        assert_eq!(extract_json(input), Some("{\"key\": \"value\"}"));
    }

    #[test]
    fn test_extract_json_array() {
        let input = "Here's the array:\n[{\"id\": 1}, {\"id\": 2}]\nThat's all!";
        assert_eq!(extract_json(input), Some("[{\"id\": 1}, {\"id\": 2}]"));
    }

    #[test]
    fn test_extract_json_nested_brackets_in_string() {
        let input = r#"{"content": "Hello [world] {test}"}"#;
        assert_eq!(
            extract_json(input),
            Some(r#"{"content": "Hello [world] {test}"}"#)
        );
    }

    #[test]
    fn test_extract_json_escaped_quotes() {
        let input = r#"{"content": "He said \"hello\""}"#;
        assert_eq!(
            extract_json(input),
            Some(r#"{"content": "He said \"hello\""}"#)
        );
    }

    #[test]
    fn test_extract_json_no_json() {
        let input = "No JSON here, just text";
        assert_eq!(extract_json(input), None);
    }

    #[test]
    fn test_extract_json_markdown_with_chatty_prefix() {
        let input = "Sure! Here's the JSON:\n```json\n{\"key\": \"value\"}\n```\nHope this helps!";
        // After stripping markdown, we get just the content; the outer chatty text is part of
        // the original response before the fence
        assert_eq!(extract_json(input), Some("{\"key\": \"value\"}"));
    }

    #[test]
    fn test_extract_json_nested_objects() {
        let input = r#"{"outer": {"inner": {"deep": "value"}}}"#;
        assert_eq!(
            extract_json(input),
            Some(r#"{"outer": {"inner": {"deep": "value"}}}"#)
        );
    }

    #[test]
    fn test_extract_json_array_of_objects() {
        let input = r#"[{"a": 1}, {"b": 2}, {"c": 3}]"#;
        assert_eq!(
            extract_json(input),
            Some(r#"[{"a": 1}, {"b": 2}, {"c": 3}]"#)
        );
    }
}
