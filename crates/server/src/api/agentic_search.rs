//! Agentic search API endpoint
//!
//! Provides AI-powered multi-agent search combining semantic, fulltext, and graph traversal.

use crate::api::{BackendClients, SearchApiImpl, SearchConfig};
use codesearch_agentic_search::{
    AgenticSearchConfig, AgenticSearchMetadata, AgenticSearchOrchestrator, AgenticSearchRequest,
};
use codesearch_core::error::Result;
use codesearch_core::search_models::EntityResult;
use codesearch_core::SearchApi;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use utoipa::ToSchema;

/// Request for agentic search endpoint
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AgenticSearchApiRequest {
    /// Natural language search query
    pub query: String,

    /// Repository IDs to search (empty means all repositories)
    #[serde(default)]
    pub repository_ids: Vec<String>,

    /// Force Sonnet model for quality gate (increases cost but may improve quality)
    #[serde(default)]
    pub force_sonnet: bool,
}

/// Response from agentic search endpoint
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct AgenticSearchApiResponse {
    /// Search results
    pub results: Vec<EntityResult>,

    /// Search execution metadata
    pub metadata: AgenticSearchApiMetadata,
}

/// Metadata about the agentic search execution
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct AgenticSearchApiMetadata {
    /// Total query time in milliseconds
    pub query_time_ms: u64,

    /// Number of search iterations performed
    pub iterations: usize,

    /// Number of worker agents spawned
    pub workers_spawned: usize,

    /// Number of workers that succeeded
    pub workers_succeeded: usize,

    /// Whether there was a partial outage during search
    pub partial_outage: bool,

    /// Total direct candidate results before filtering
    pub total_direct_candidates: usize,

    /// Number of entities discovered via graph traversal
    pub graph_context_entities: usize,

    /// Number of graph-discovered entities in final results
    pub graph_entities_in_results: usize,

    /// Whether graph traversal was used
    pub graph_traversal_used: bool,

    /// Estimated cost in USD
    pub estimated_cost_usd: f32,
}

impl From<AgenticSearchMetadata> for AgenticSearchApiMetadata {
    fn from(m: AgenticSearchMetadata) -> Self {
        Self {
            query_time_ms: m.query_time_ms,
            iterations: m.iterations,
            workers_spawned: m.workers_spawned,
            workers_succeeded: m.workers_succeeded,
            partial_outage: m.partial_outage,
            total_direct_candidates: m.total_direct_candidates,
            graph_context_entities: m.graph_context_entities,
            graph_entities_in_results: m.graph_entities_in_results,
            graph_traversal_used: m.graph_traversal_used,
            estimated_cost_usd: m.estimated_cost_usd,
        }
    }
}

/// Execute agentic search
pub async fn search_agentic(
    request: AgenticSearchApiRequest,
    clients: &Arc<BackendClients>,
    config: &Arc<SearchConfig>,
) -> Result<AgenticSearchApiResponse> {
    // Create SearchApiImpl for the orchestrator
    let search_api: Arc<dyn SearchApi> =
        Arc::new(SearchApiImpl::new(clients.clone(), config.clone()));

    // Create agentic search config (using defaults for now)
    let agentic_config = AgenticSearchConfig::default();

    // Create orchestrator
    let orchestrator = AgenticSearchOrchestrator::new(search_api, agentic_config).map_err(|e| {
        codesearch_core::error::Error::from(anyhow::anyhow!("Failed to create orchestrator: {e}"))
    })?;

    // Build search request
    let search_request = AgenticSearchRequest {
        query: request.query,
        force_sonnet: request.force_sonnet,
        repository_ids: request.repository_ids,
    };

    // Execute search
    let response = orchestrator.search(search_request).await.map_err(|e| {
        codesearch_core::error::Error::from(anyhow::anyhow!("Agentic search failed: {e}"))
    })?;

    Ok(AgenticSearchApiResponse {
        results: response.results,
        metadata: response.metadata.into(),
    })
}
