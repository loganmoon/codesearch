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
    pub results: Vec<EntityResult>,
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
    HaikuOnly,
    HaikuWithSonnet,
}

/// Tracks how an entity was retrieved
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetrievalSource {
    Semantic,
    Fulltext,
    Unified,
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
            RetrievalSource::Semantic => format!("Semantic similarity: {:.2}", entity.score),
            RetrievalSource::Fulltext => format!("Full-text match: {:.2}", entity.score),
            RetrievalSource::Unified => format!("Hybrid match: {:.2}", entity.score),
            RetrievalSource::Graph { .. } => "Graph context".to_string(),
        };

        Self {
            entity,
            source,
            relevance_justification: justification,
        }
    }

    pub fn is_direct_match(&self) -> bool {
        matches!(
            self.source,
            RetrievalSource::Semantic | RetrievalSource::Fulltext | RetrievalSource::Unified
        )
    }

    pub fn is_graph_context(&self) -> bool {
        matches!(self.source, RetrievalSource::Graph { .. })
    }
}

// ============================================================================
// LLM Response Types (for parsing prompt outputs)
// These are internal types used only for deserializing LLM responses.
// ============================================================================

/// Individual result from quality gate composition
/// Currently only used in tests, kept for potential future use
#[cfg(test)]
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct QualityGateResult {
    pub entity_id: String,
    pub relevance_justification: String,
}
