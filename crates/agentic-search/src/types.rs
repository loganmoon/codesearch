//! Public API types for agentic search

use codesearch_core::search_models::EntityResult;
use serde::{Deserialize, Serialize};

/// Request for agentic search
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgenticSearchRequest {
    pub query: String,
    #[serde(default)]
    pub force_sonnet: bool,
    #[serde(default)]
    pub repository_ids: Vec<String>,
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
    pub workers_spawned: usize,
    pub workers_succeeded: usize,
    pub partial_outage: bool,
    pub total_direct_candidates: usize,
    pub graph_context_entities: usize,
    pub graph_entities_in_results: usize,
    pub reranking_method: RerankingMethod,
    pub graph_traversal_used: bool,
    pub estimated_cost_usd: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RerankingMethod {
    HaikuOnly,
    HaikuWithSonnet,
}
