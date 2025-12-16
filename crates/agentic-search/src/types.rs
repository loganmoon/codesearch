//! Public API types for agentic search

use crate::error::AgenticSearchError;
use codesearch_core::search_models::EntityResult;
use serde::{Deserialize, Serialize};

/// Maximum query length to prevent excessive token consumption
const MAX_QUERY_LENGTH: usize = 10000;

/// Request for agentic search
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgenticSearchRequest {
    pub query: String,
    #[serde(default)]
    pub force_sonnet: bool,
    #[serde(default)]
    pub repository_ids: Vec<String>,
}

impl AgenticSearchRequest {
    /// Validate the request, checking query constraints
    pub fn validate(&self) -> Result<(), AgenticSearchError> {
        if self.query.is_empty() {
            return Err(AgenticSearchError::Config(
                "Query cannot be empty".to_string(),
            ));
        }
        if self.query.len() > MAX_QUERY_LENGTH {
            return Err(AgenticSearchError::Config(format!(
                "Query exceeds maximum length of {MAX_QUERY_LENGTH} characters"
            )));
        }
        Ok(())
    }
}

/// Response from agentic search
#[derive(Debug, Clone, Serialize)]
pub struct AgenticSearchResponse {
    /// Results with retrieval source information
    pub results: Vec<AgenticEntity>,
    pub metadata: AgenticSearchMetadata,
}

/// Metadata about agentic search execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgenticSearchMetadata {
    pub query_time_ms: u64,
    pub iterations: usize,
    pub workers_spawned: usize,
    pub workers_succeeded: usize,
    pub partial_outage: bool,
    pub total_direct_candidates: usize,
    pub graph_context_entities: usize,
    pub graph_entities_in_results: usize,
    pub reranking_method: RerankingMethod,
    pub graph_traversal_used: bool,
    pub estimated_cost_usd: f32,
    /// Tokens read from Claude API prompt cache (90% cost reduction)
    #[serde(default)]
    pub cache_read_tokens: u64,
    /// Tokens written to Claude API prompt cache
    #[serde(default)]
    pub cache_creation_tokens: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RerankingMethod {
    /// No reranking applied
    None,
    /// Jina cross-encoder reranking
    Jina,
}

/// Tracks how an entity was retrieved
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetrievalSource {
    /// Semantic search (combines dense embeddings + BM25 sparse retrieval)
    Semantic,
    /// Graph traversal from a source entity
    Graph {
        source_entity_id: String,
        relationship: String,
    },
}

/// Enriched entity with retrieval metadata
#[derive(Debug, Clone, Serialize)]
pub struct AgenticEntity {
    #[serde(flatten)]
    pub entity: EntityResult,
    pub source: RetrievalSource,
    pub relevance_justification: String,
}

impl AgenticEntity {
    pub fn from_search_result(entity: EntityResult, source: RetrievalSource) -> Self {
        let justification = match &source {
            RetrievalSource::Semantic => format!("Semantic match: {:.2}", entity.score),
            RetrievalSource::Graph { .. } => "Graph context".to_string(),
        };

        Self {
            entity,
            source,
            relevance_justification: justification,
        }
    }

    pub fn is_direct_match(&self) -> bool {
        matches!(self.source, RetrievalSource::Semantic)
    }

    pub fn is_graph_context(&self) -> bool {
        matches!(self.source, RetrievalSource::Graph { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_empty_query_rejected() {
        let request = AgenticSearchRequest {
            query: "".to_string(),
            force_sonnet: false,
            repository_ids: vec![],
        };
        let result = request.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Query cannot be empty"));
    }

    #[test]
    fn test_validate_whitespace_only_query_accepted() {
        // Whitespace-only is not empty string, so it passes the empty check
        // (semantically invalid but syntactically passes current validation)
        let request = AgenticSearchRequest {
            query: "   ".to_string(),
            force_sonnet: false,
            repository_ids: vec![],
        };
        let result = request.validate();
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_query_exceeds_max_length() {
        let long_query = "a".repeat(MAX_QUERY_LENGTH + 1);
        let request = AgenticSearchRequest {
            query: long_query,
            force_sonnet: false,
            repository_ids: vec![],
        };
        let result = request.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("exceeds maximum length"));
    }

    #[test]
    fn test_validate_query_at_max_length() {
        let max_query = "a".repeat(MAX_QUERY_LENGTH);
        let request = AgenticSearchRequest {
            query: max_query,
            force_sonnet: false,
            repository_ids: vec![],
        };
        let result = request.validate();
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_normal_query() {
        let request = AgenticSearchRequest {
            query: "What functions handle authentication?".to_string(),
            force_sonnet: false,
            repository_ids: vec![],
        };
        let result = request.validate();
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_query_with_repository_ids() {
        let request = AgenticSearchRequest {
            query: "Find the main function".to_string(),
            force_sonnet: true,
            repository_ids: vec!["repo-1".to_string(), "repo-2".to_string()],
        };
        let result = request.validate();
        assert!(result.is_ok());
    }
}
