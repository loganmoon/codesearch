//! Agentic search orchestrator with iterative loop

use crate::{
    config::AgenticSearchConfig,
    error::{AgenticSearchError, Result},
    prompts,
    types::{
        AgenticEntity, AgenticSearchMetadata, AgenticSearchRequest, AgenticSearchResponse,
        RerankingMethod, RetrievalSource,
    },
    worker::{execute_workers, WorkerQuery, WorkerType},
};
use codesearch_core::SearchApi;
use serde::Deserialize;
use std::sync::Arc;
use std::time::Instant;
use tracing::{debug, info, warn};

const MAX_ITERATIONS: usize = 5;

pub struct AgenticSearchOrchestrator {
    search_api: Arc<dyn SearchApi>,
    sonnet_client: Arc<claudius::Anthropic>,
    haiku_client: Arc<claudius::Anthropic>,
    sonnet_model: claudius::Model,
    haiku_model: claudius::Model,
    #[allow(dead_code)]
    config: AgenticSearchConfig,
}

impl std::fmt::Debug for AgenticSearchOrchestrator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgenticSearchOrchestrator")
            .field("sonnet_client", &"<Anthropic>")
            .field("haiku_client", &"<Anthropic>")
            .field("sonnet_model", &self.sonnet_model)
            .field("haiku_model", &self.haiku_model)
            .field("config", &self.config)
            .finish()
    }
}

impl AgenticSearchOrchestrator {
    pub fn new(search_api: Arc<dyn SearchApi>, config: AgenticSearchConfig) -> Result<Self> {
        config.validate().map_err(AgenticSearchError::Config)?;

        let api_key = config
            .resolve_api_key()
            .ok_or(AgenticSearchError::MissingApiKey)?;

        let sonnet_client = Arc::new(claudius::Anthropic::new(Some(api_key.clone())).map_err(
            |e| AgenticSearchError::Config(format!("Failed to create Sonnet client: {e}")),
        )?);
        let haiku_client = Arc::new(claudius::Anthropic::new(Some(api_key)).map_err(|e| {
            AgenticSearchError::Config(format!("Failed to create Haiku client: {e}"))
        })?);

        let sonnet_model = claudius::Model::Custom(config.orchestrator_model.clone());
        let haiku_model = claudius::Model::Custom(config.worker_model.clone());

        Ok(Self {
            search_api,
            sonnet_client,
            haiku_client,
            sonnet_model,
            haiku_model,
            config,
        })
    }

    /// Execute agentic search with iterative loop
    pub async fn search(&self, request: AgenticSearchRequest) -> Result<AgenticSearchResponse> {
        let start_time = Instant::now();
        let mut iteration = 0;
        let mut accumulated_entities: Vec<AgenticEntity> = Vec::new();
        let mut total_cost = 0.0;
        let mut workers_spawned = 0;
        let mut workers_succeeded = 0;
        let mut partial_outage = false;

        info!("Starting agentic search: {}", request.query);

        // Agentic loop
        loop {
            iteration += 1;
            info!("Iteration {}/{}", iteration, MAX_ITERATIONS);

            // Call orchestrator to evaluate and plan next operations
            let decision = self
                .orchestrator_loop_iteration(&request.query, &accumulated_entities, iteration)
                .await?;

            // Check stop conditions
            if decision.should_stop {
                info!("Orchestrator decided to stop: {}", decision.reason);
                break;
            }

            if iteration >= MAX_ITERATIONS {
                info!("Reached max iterations");
                break;
            }

            // Execute planned operations
            let (new_entities, worker_stats) = self
                .execute_operations(&decision.operations, &request)
                .await?;

            workers_spawned += worker_stats.spawned;
            workers_succeeded += worker_stats.succeeded;
            if worker_stats.failed > 0 {
                partial_outage = true;
            }

            // Merge with accumulated entities (deduplicate by entity_id)
            let initial_count = accumulated_entities.len();
            for entity in new_entities {
                if !accumulated_entities
                    .iter()
                    .any(|e| e.entity.entity_id == entity.entity.entity_id)
                {
                    accumulated_entities.push(entity);
                }
            }

            let new_count = accumulated_entities.len() - initial_count;
            debug!(
                "Added {} new entities (total: {})",
                new_count,
                accumulated_entities.len()
            );

            // Check if we found nothing new
            if accumulated_entities.is_empty() {
                info!("No entities found, stopping");
                break;
            }

            if new_count == 0 && iteration > 1 {
                info!("No new entities found, stopping");
                break;
            }

            total_cost += decision.iteration_cost;
        }

        // Final synthesis: select top 10
        let final_results = self
            .synthesize_final_results(accumulated_entities.clone(), &request.query)
            .await?;

        let query_time_ms = start_time.elapsed().as_millis() as u64;

        // Calculate metadata
        let direct_candidates = accumulated_entities
            .iter()
            .filter(|e| e.is_direct_match())
            .count();
        let graph_context = accumulated_entities
            .iter()
            .filter(|e| e.is_graph_context())
            .count();
        let graph_in_results = final_results
            .iter()
            .filter(|e| e.is_graph_context())
            .count();

        Ok(AgenticSearchResponse {
            results: final_results.into_iter().map(|e| e.entity).collect(),
            metadata: AgenticSearchMetadata {
                query_time_ms,
                iterations: iteration,
                workers_spawned,
                workers_succeeded,
                partial_outage,
                total_direct_candidates: direct_candidates,
                graph_context_entities: graph_context,
                graph_entities_in_results: graph_in_results,
                reranking_method: RerankingMethod::HaikuOnly,
                graph_traversal_used: graph_context > 0,
                estimated_cost_usd: total_cost,
            },
        })
    }

    /// Single iteration of orchestrator loop
    async fn orchestrator_loop_iteration(
        &self,
        query: &str,
        accumulated_entities: &[AgenticEntity],
        iteration: usize,
    ) -> Result<OrchestratorDecision> {
        debug!(
            "Orchestrator evaluating (iteration {}, {} entities accumulated)",
            iteration,
            accumulated_entities.len()
        );

        // Format accumulated context
        let context = if accumulated_entities.is_empty() {
            "No entities found yet.".to_string()
        } else {
            accumulated_entities
                .iter()
                .take(20)
                .map(|e| {
                    format!(
                        "- {} ({}): {}",
                        e.entity.qualified_name,
                        match &e.source {
                            RetrievalSource::Semantic => "semantic",
                            RetrievalSource::Fulltext => "fulltext",
                            RetrievalSource::Unified => "unified",
                            RetrievalSource::Graph { .. } => "graph",
                        },
                        e.relevance_justification
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        };

        let prompt = prompts::format_prompt(
            prompts::ORCHESTRATOR_PLAN,
            &[
                ("query", query),
                ("context", &context),
                ("iteration", &iteration.to_string()),
                ("max_iterations", &MAX_ITERATIONS.to_string()),
            ],
        );

        // Call Sonnet
        let mut params = claudius::MessageCreateParams::simple(
            claudius::MessageParam::user(prompt),
            self.sonnet_model.clone(),
        );
        params.max_tokens = 4096;
        params.temperature = Some(0.0);

        let response = self.sonnet_client.send(params).await.map_err(|e| {
            AgenticSearchError::Orchestrator(format!("Sonnet API call failed: {e}"))
        })?;

        // Extract text content
        let response_text = response
            .content
            .iter()
            .filter_map(|block| match block {
                claudius::ContentBlock::Text(text_block) => Some(text_block.text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");

        // Parse JSON decision
        let decision: OrchestratorDecisionResponse =
            serde_json::from_str(&response_text).map_err(|e| {
                AgenticSearchError::Orchestrator(format!(
                    "Failed to parse Sonnet decision: {e}. Response: {response_text}"
                ))
            })?;

        debug!(
            "Orchestrator decision: should_stop={}, operations={}",
            decision.should_stop,
            decision.operations.len()
        );

        Ok(OrchestratorDecision {
            should_stop: decision.should_stop,
            reason: decision.reason,
            operations: decision
                .operations
                .into_iter()
                .map(|op| match op.operation_type.as_str() {
                    "search" => PlannedOperation::Search {
                        query: op.query.unwrap_or_default(),
                        search_types: op
                            .search_types
                            .unwrap_or_else(|| vec!["unified".to_string()])
                            .into_iter()
                            .filter_map(|s| match s.as_str() {
                                "semantic" => Some(WorkerType::Semantic),
                                "fulltext" => Some(WorkerType::Fulltext),
                                "unified" => Some(WorkerType::Unified),
                                _ => None,
                            })
                            .collect(),
                    },
                    "graph_traversal" => PlannedOperation::GraphTraversal {
                        entity_id: op.entity_id.unwrap_or_default(),
                        relationship: op.relationship.unwrap_or_default(),
                    },
                    _ => PlannedOperation::Search {
                        query: query.to_string(),
                        search_types: vec![WorkerType::Unified],
                    },
                })
                .collect(),
            iteration_cost: 0.01,
        })
    }

    /// Execute operations planned by orchestrator
    async fn execute_operations(
        &self,
        operations: &[PlannedOperation],
        request: &AgenticSearchRequest,
    ) -> Result<(Vec<AgenticEntity>, WorkerStats)> {
        let mut all_entities = Vec::new();
        let mut stats = WorkerStats {
            spawned: 0,
            succeeded: 0,
            failed: 0,
        };

        for op in operations {
            match op {
                PlannedOperation::Search {
                    query,
                    search_types,
                } => {
                    let worker_queries: Vec<WorkerQuery> = search_types
                        .iter()
                        .map(|worker_type| WorkerQuery {
                            worker_type: *worker_type,
                            query: query.clone(),
                            repository_ids: request.repository_ids.clone(),
                        })
                        .collect();

                    stats.spawned += worker_queries.len();

                    match execute_workers(
                        worker_queries,
                        self.search_api.clone(),
                        self.haiku_client.clone(),
                        self.haiku_model.clone(),
                    )
                    .await
                    {
                        Ok(results) => {
                            stats.succeeded += results.len();
                            for result in results {
                                all_entities.extend(result.entities);
                            }
                        }
                        Err(AgenticSearchError::AllWorkersFailed) => {
                            warn!("All workers failed for query: {}", query);
                            stats.failed += search_types.len();
                        }
                        Err(e) => {
                            warn!("Workers partially failed: {}", e);
                            stats.failed += 1;
                        }
                    }
                }
                PlannedOperation::GraphTraversal {
                    entity_id: _,
                    relationship: _,
                } => {
                    // Placeholder for Phase 3: graph traversal
                    debug!("Graph traversal not yet implemented");
                }
            }
        }

        Ok((all_entities, stats))
    }

    /// Synthesize final top 10 results
    async fn synthesize_final_results(
        &self,
        entities: Vec<AgenticEntity>,
        _query: &str,
    ) -> Result<Vec<AgenticEntity>> {
        // For Phase 2: simple top 10 by score
        // Phase 3 will add Quality Gate with dual-track composition
        let mut sorted = entities;
        sorted.sort_by(|a, b| {
            b.entity
                .score
                .partial_cmp(&a.entity.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        Ok(sorted.into_iter().take(10).collect())
    }
}

/// Decision from orchestrator loop iteration
#[derive(Debug)]
struct OrchestratorDecision {
    should_stop: bool,
    reason: String,
    operations: Vec<PlannedOperation>,
    iteration_cost: f32,
}

/// Response from Sonnet for orchestrator decision
#[derive(Debug, Deserialize)]
struct OrchestratorDecisionResponse {
    should_stop: bool,
    reason: String,
    operations: Vec<PlannedOperationResponse>,
}

#[derive(Debug, Deserialize)]
struct PlannedOperationResponse {
    operation_type: String,
    query: Option<String>,
    search_types: Option<Vec<String>>,
    entity_id: Option<String>,
    relationship: Option<String>,
}

/// Operation planned by orchestrator
#[derive(Debug, Clone)]
enum PlannedOperation {
    Search {
        query: String,
        search_types: Vec<WorkerType>,
    },
    GraphTraversal {
        #[allow(dead_code)]
        entity_id: String,
        #[allow(dead_code)]
        relationship: String,
    },
}

#[derive(Debug, Default)]
struct WorkerStats {
    spawned: usize,
    succeeded: usize,
    failed: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_orchestrator_decision_parsing() {
        let json = r#"{
            "should_stop": false,
            "reason": "Need to search for JWT validation",
            "operations": [
                {
                    "operation_type": "search",
                    "query": "JWT validation implementation",
                    "search_types": ["semantic", "fulltext"]
                }
            ]
        }"#;

        let decision: OrchestratorDecisionResponse = serde_json::from_str(json).unwrap();
        assert!(!decision.should_stop);
        assert_eq!(decision.operations.len(), 1);
        assert_eq!(decision.operations[0].operation_type, "search");
    }

    #[test]
    fn test_orchestrator_stop_decision() {
        let json = r#"{
            "should_stop": true,
            "reason": "Query satisfactorily answered",
            "operations": []
        }"#;

        let decision: OrchestratorDecisionResponse = serde_json::from_str(json).unwrap();
        assert!(decision.should_stop);
        assert_eq!(decision.operations.len(), 0);
    }
}
