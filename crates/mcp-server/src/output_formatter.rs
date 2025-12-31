//! Adaptive output formatting for MCP tool responses
//!
//! Formats search results adaptively based on size:
//! - Small results: Include full content
//! - Large results: Summary with file:line references

use codesearch_agentic_search::{AgenticEntity, AgenticSearchMetadata, AgenticSearchResponse};
use serde::Serialize;

/// Threshold for switching to summary mode (character count of all content)
const FULL_CONTENT_THRESHOLD: usize = 8000;

/// Maximum entities to include with full content
const MAX_FULL_CONTENT_ENTITIES: usize = 5;

/// Formatted result for MCP output
#[derive(Debug, Serialize)]
pub struct FormattedResult {
    /// Entity identifier
    pub entity_id: String,
    /// Qualified name (e.g., `module::function`)
    pub qualified_name: String,
    /// Entity type (function, class, etc.)
    pub entity_type: String,
    /// File location with line number
    pub location: String,
    /// Language of the code
    pub language: String,
    /// Content (full or summary)
    pub content: ContentFormat,
    /// Relevance score
    pub score: f32,
    /// Reasoning for why this result is relevant
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
}

/// Content format variants
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum ContentFormat {
    /// Full content included
    Full(String),
    /// Summary with truncation indicator
    Summary { preview: String, truncated: bool },
}

/// Formatted search response for MCP
#[derive(Debug, Serialize)]
pub struct FormattedSearchResponse {
    pub results: Vec<FormattedResult>,
    pub metadata: FormattedMetadata,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Simplified metadata for MCP output
#[derive(Debug, Serialize)]
pub struct FormattedMetadata {
    pub total_results: usize,
    pub query_time_ms: u64,
    pub iterations: usize,
    pub graph_context_used: bool,
}

/// Format search response adaptively based on result size
pub fn format_response(response: AgenticSearchResponse, verbose: bool) -> FormattedSearchResponse {
    let total_content_size: usize = response
        .results
        .iter()
        .map(|r| r.entity.content.as_ref().map_or(0, |c| c.len()))
        .sum();

    let use_full_content = verbose
        || (total_content_size < FULL_CONTENT_THRESHOLD
            && response.results.len() <= MAX_FULL_CONTENT_ENTITIES);

    let results: Vec<FormattedResult> = response
        .results
        .into_iter()
        .map(|agentic_entity| format_entity(agentic_entity, use_full_content))
        .collect();

    let note = if !use_full_content && !verbose {
        Some(
            "Results summarized for context efficiency. Use verbose:true for full content."
                .to_string(),
        )
    } else {
        None
    };

    let result_count = results.len();
    FormattedSearchResponse {
        results,
        metadata: format_metadata(response.metadata, result_count),
        note,
    }
}

fn format_entity(agentic_entity: AgenticEntity, full_content: bool) -> FormattedResult {
    let entity = agentic_entity.entity;
    let location = format!("{}:{}", entity.file_path, entity.location.start_line);

    let content = if full_content {
        ContentFormat::Full(entity.content.unwrap_or_default())
    } else {
        let (preview, truncated) = entity.content.as_ref().map_or((String::new(), false), |c| {
            let lines: Vec<&str> = c.lines().take(6).collect();
            let truncated = lines.len() > 5;
            let preview = if truncated {
                format!("{}...", lines[..5].join("\n"))
            } else {
                lines.join("\n")
            };
            (preview, truncated)
        });
        ContentFormat::Summary { preview, truncated }
    };

    FormattedResult {
        entity_id: entity.entity_id,
        qualified_name: entity.qualified_name,
        entity_type: entity.entity_type.to_string(),
        location,
        language: entity.language.to_string(),
        content,
        score: entity.score,
        reasoning: entity.reasoning,
    }
}

fn format_metadata(metadata: AgenticSearchMetadata, result_count: usize) -> FormattedMetadata {
    FormattedMetadata {
        total_results: result_count,
        query_time_ms: metadata.query_time_ms,
        iterations: metadata.iterations,
        graph_context_used: metadata.graph_traversal_used,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codesearch_agentic_search::{RerankingMethod, RetrievalSource};
    use codesearch_core::entities::{EntityType, Language, SourceLocation, Visibility};
    use codesearch_core::search_models::EntityResult;
    use uuid::Uuid;

    fn make_agentic_entity(content_size: usize) -> AgenticEntity {
        let entity = EntityResult {
            entity_id: Uuid::new_v4().to_string(),
            repository_id: Uuid::new_v4(),
            name: "function".to_string(),
            qualified_name: "test::function".to_string(),
            entity_type: EntityType::Function,
            file_path: "src/test.rs".to_string(),
            location: SourceLocation {
                start_line: 10,
                end_line: 20,
                start_column: 0,
                end_column: 0,
            },
            language: Language::Rust,
            content: Some("x".repeat(content_size)),
            signature: None,
            documentation_summary: None,
            visibility: Some(Visibility::Public),
            score: 0.95,
            reranked: false,
            reasoning: Some("Test match".to_string()),
        };
        AgenticEntity {
            entity,
            source: RetrievalSource::Semantic,
            relevance_justification: "Semantic match".to_string(),
        }
    }

    fn make_metadata() -> AgenticSearchMetadata {
        AgenticSearchMetadata {
            query_time_ms: 100,
            iterations: 2,
            workers_spawned: 3,
            workers_succeeded: 3,
            partial_outage: false,
            total_direct_candidates: 10,
            graph_context_entities: 2,
            graph_entities_in_results: 1,
            reranking_method: RerankingMethod::CrossEncoder,
            graph_traversal_used: true,
            estimated_cost_usd: 0.05,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
        }
    }

    #[test]
    fn test_small_results_get_full_content() {
        let response = AgenticSearchResponse {
            results: vec![make_agentic_entity(100)],
            metadata: make_metadata(),
        };

        let formatted = format_response(response, false);
        assert!(formatted.note.is_none());
        assert!(matches!(
            formatted.results[0].content,
            ContentFormat::Full(_)
        ));
    }

    #[test]
    fn test_large_results_get_summary() {
        let response = AgenticSearchResponse {
            results: vec![make_agentic_entity(10000)],
            metadata: make_metadata(),
        };

        let formatted = format_response(response, false);
        assert!(formatted.note.is_some());
        assert!(matches!(
            formatted.results[0].content,
            ContentFormat::Summary { .. }
        ));
    }

    #[test]
    fn test_verbose_forces_full_content() {
        let response = AgenticSearchResponse {
            results: vec![make_agentic_entity(10000)],
            metadata: make_metadata(),
        };

        let formatted = format_response(response, true);
        assert!(formatted.note.is_none());
        assert!(matches!(
            formatted.results[0].content,
            ContentFormat::Full(_)
        ));
    }
}
