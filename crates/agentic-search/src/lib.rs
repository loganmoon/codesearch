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
//! - [`RetrievalSource`] - How an entity was retrieved (semantic or graph)
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
// Internal Utilities (test-only since we use structured output)
// ============================================================================

/// Strip markdown code fences from LLM response.
#[cfg(test)]
fn strip_markdown_fences(response: &str) -> &str {
    let trimmed = response.trim();

    if trimmed.starts_with("```") {
        // Find the end of the opening fence (```json or ```)
        let after_fence = if let Some(newline_pos) = trimmed.find('\n') {
            &trimmed[newline_pos + 1..]
        } else {
            trimmed
                .strip_prefix("```json")
                .or_else(|| trimmed.strip_prefix("```"))
                .unwrap_or(trimmed)
        };

        // Find closing fence
        if let Some(close_pos) = after_fence.rfind("```") {
            after_fence[..close_pos].trim()
        } else {
            after_fence.trim()
        }
    } else {
        trimmed
    }
}

/// Extract balanced JSON structure starting at a given position.
/// Returns the balanced structure if found, None otherwise.
#[cfg(test)]
fn extract_balanced_at(content: &str, start_pos: usize) -> Option<&str> {
    let start_char = content.chars().nth(start_pos)?;
    let end_char = match start_char {
        '{' => '}',
        '[' => ']',
        _ => return None,
    };

    let json_content = &content[start_pos..];
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

    None
}

/// Check if a string is valid JSON.
#[cfg(test)]
fn is_valid_json(s: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(s).is_ok()
}

/// Extract JSON from LLM response, stripping markdown and extraneous text.
///
/// Handles:
/// - Markdown code blocks: ```json\n{...}\n```
/// - Chatty prefixes: "Here's the result:\n{...}"
/// - Trailing explanations: "{...}\n\nLet me know if you need..."
/// - Nested JSON (finds outermost balanced structure)
/// - False positives like `[entity-abc123]` (validates with serde_json)
#[cfg(test)]
fn extract_json(response: &str) -> Option<&str> {
    let content = strip_markdown_fences(response);

    // Collect all candidate start positions and sort by position
    // This ensures we find the outermost structure first (e.g., array before inner objects)
    let mut candidates: Vec<usize> = content.match_indices(['{', '[']).map(|(i, _)| i).collect();
    candidates.sort_unstable();

    // Try each candidate in positional order, validate with serde_json
    for pos in candidates {
        if let Some(json) = extract_balanced_at(content, pos) {
            if is_valid_json(json) {
                return Some(json);
            }
        }
    }

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

    #[test]
    fn test_extract_json_rejects_entity_id_brackets() {
        // Should find the actual JSON, not the entity ID reference
        let input = r#"Looking at [entity-2b53cad79507ddba4f9b6031c1a82f05]:
{"should_stop": true, "reason": "done", "operations": []}"#;
        assert_eq!(
            extract_json(input),
            Some(r#"{"should_stop": true, "reason": "done", "operations": []}"#)
        );
    }

    #[test]
    fn test_extract_json_handles_only_prose() {
        // When LLM completely ignores JSON instruction
        let input = "I found [entity-abc123] and {various helpers} in the code.";
        assert_eq!(extract_json(input), None);
    }

    #[test]
    fn test_extract_json_multiple_invalid_before_valid() {
        let input = r#"Found [entity-a1b2c3d4] and [entity-e5f6g7h8] in results:
{"should_stop": false, "reason": "continue", "operations": []}"#;
        assert_eq!(
            extract_json(input),
            Some(r#"{"should_stop": false, "reason": "continue", "operations": []}"#)
        );
    }

    #[test]
    fn test_extract_json_prefers_brace_over_bracket() {
        // When both exist, prefer { since objects are more common
        let input = r#"[not json] but {"valid": "json"}"#;
        assert_eq!(extract_json(input), Some(r#"{"valid": "json"}"#));
    }

    #[test]
    fn test_extract_json_valid_array_after_invalid_bracket() {
        // Should skip the entity ID and find the valid JSON array
        let input = r#"Looking at [entity-abc123]: [{"id": 1}, {"id": 2}]"#;
        assert_eq!(extract_json(input), Some(r#"[{"id": 1}, {"id": 2}]"#));
    }
}
