//! Agentic search orchestrator with iterative loop

use crate::{
    config::AgenticSearchConfig,
    error::{truncate_for_error, AgenticSearchError, Result},
    prompts,
    types::{
        AgenticEntity, AgenticSearchMetadata, AgenticSearchRequest, AgenticSearchResponse,
        QualityGateResult, RerankingMethod, RetrievalSource,
    },
    worker::{execute_workers, WorkerQuery, WorkerType},
};
use codesearch_core::search_models::{GraphQueryParameters, GraphQueryRequest, GraphQueryType};
use codesearch_core::SearchApi;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;
use tracing::{debug, info, warn};

const MAX_ITERATIONS: usize = 5;
const MAX_LLM_QUERY_LENGTH: usize = 1000;
const VALID_RELATIONSHIPS: &[&str] = &[
    "callers",
    "called_by",
    "who_calls",
    "callees",
    "calls",
    "what_calls",
    "implementations",
    "implements",
    "implementors",
    "hierarchy",
    "extends",
    "inherits",
    "contains",
    "module_contents",
    "in_module",
    "dependencies",
    "imports",
    "uses",
];

// ============================================================================
// Helper Functions
// ============================================================================

/// Extract JSON content from between <result_list> XML tags
/// Handles chatty LLM responses that may have text before/after the tags
fn extract_result_list(response: &str) -> Result<String> {
    let start_tag = "<result_list>";
    let end_tag = "</result_list>";

    let start = response.find(start_tag).ok_or_else(|| {
        AgenticSearchError::QualityGate(format!(
            "Missing <result_list> tag in response: {}",
            &response[..response.len().min(200)]
        ))
    })?;

    let end = response.find(end_tag).ok_or_else(|| {
        AgenticSearchError::QualityGate(format!(
            "Missing </result_list> tag in response: {}",
            &response[..response.len().min(200)]
        ))
    })?;

    if end <= start {
        return Err(AgenticSearchError::QualityGate(
            "Invalid XML tag order".to_string(),
        ));
    }

    Ok(response[start + start_tag.len()..end].trim().to_string())
}

/// Validate entity_id has reasonable format (not empty, reasonable length)
fn is_valid_entity_id(entity_id: &str) -> bool {
    !entity_id.is_empty() && entity_id.len() <= 100
}

/// Check if relationship is in the allowed whitelist
fn is_valid_relationship(relationship: &str) -> bool {
    VALID_RELATIONSHIPS.contains(&relationship.to_lowercase().as_str())
}

/// Map relationship strings from orchestrator to GraphQueryType
fn relationship_to_query_type(relationship: &str) -> Option<GraphQueryType> {
    match relationship.to_lowercase().as_str() {
        "callers" | "called_by" | "who_calls" => Some(GraphQueryType::FindFunctionCallers),
        "callees" | "calls" | "what_calls" => Some(GraphQueryType::FindFunctionCallees),
        "implementations" | "implements" | "implementors" => {
            Some(GraphQueryType::FindTraitImplementations)
        }
        "hierarchy" | "extends" | "inherits" => Some(GraphQueryType::FindClassHierarchy),
        "contains" | "module_contents" | "in_module" => Some(GraphQueryType::FindModuleContents),
        "dependencies" | "imports" | "uses" => Some(GraphQueryType::FindModuleDependencies),
        _ => None,
    }
}

/// Format entities for inclusion in prompts
fn format_entities_for_prompt(entities: &[AgenticEntity], limit: usize) -> String {
    entities
        .iter()
        .take(limit)
        .map(|e| {
            let source_info = match &e.source {
                RetrievalSource::Graph {
                    source_entity_id,
                    relationship,
                } => format!(" [via {relationship} from {source_entity_id}]"),
                _ => String::new(),
            };
            format!(
                "[{}] {}{}\nScore: {:.2}\nJustification: {}\nContent: {}",
                e.entity.entity_id,
                e.entity.qualified_name,
                source_info,
                e.entity.score,
                e.relevance_justification,
                e.entity.content.as_ref().map_or("N/A".to_string(), |c| {
                    if c.len() > 200 {
                        format!("{}...", &c[..200])
                    } else {
                        c.clone()
                    }
                })
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n---\n\n")
}

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
        // Validate request before processing
        request.validate()?;

        let start_time = Instant::now();
        let mut iteration = 0;
        let mut accumulated_entities: Vec<AgenticEntity> = Vec::new();
        let mut seen_entity_ids: HashSet<String> = HashSet::new();
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

            // Merge with accumulated entities using HashSet for O(1) deduplication
            let initial_count = accumulated_entities.len();
            for entity in new_entities {
                if seen_entity_ids.insert(entity.entity.entity_id.clone()) {
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

        // Calculate metadata before synthesis (need counts from accumulated_entities)
        let direct_candidates = accumulated_entities
            .iter()
            .filter(|e| e.is_direct_match())
            .count();
        let graph_context = accumulated_entities
            .iter()
            .filter(|e| e.is_graph_context())
            .count();

        // Final synthesis: select top 10 (pass ownership, no clone needed)
        let final_results = self
            .synthesize_final_results(accumulated_entities, &request.query)
            .await?;

        let query_time_ms = start_time.elapsed().as_millis() as u64;

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
                    "Failed to parse Sonnet decision: {e}. Response: {}",
                    truncate_for_error(&response_text)
                ))
            })?;

        debug!(
            "Orchestrator decision: should_stop={}, operations={}",
            decision.should_stop,
            decision.operations.len()
        );

        // Parse and validate operations, filtering out invalid ones
        let operations: Vec<PlannedOperation> = decision
            .operations
            .into_iter()
            .filter_map(|op| match op.operation_type.as_str() {
                "search" => {
                    let search_query = op.query.unwrap_or_default();
                    // Validate query length
                    if search_query.len() > MAX_LLM_QUERY_LENGTH {
                        warn!(
                            "LLM returned query exceeding max length ({} > {}), truncating",
                            search_query.len(),
                            MAX_LLM_QUERY_LENGTH
                        );
                    }
                    let truncated_query: String =
                        search_query.chars().take(MAX_LLM_QUERY_LENGTH).collect();

                    Some(PlannedOperation::Search {
                        query: truncated_query,
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
                    })
                }
                "graph_traversal" => {
                    let entity_id = op.entity_id.unwrap_or_default();
                    let relationship = op.relationship.unwrap_or_default();

                    // Validate entity_id
                    if !is_valid_entity_id(&entity_id) {
                        warn!(
                            "LLM returned invalid entity_id '{}', skipping graph traversal",
                            truncate_for_error(&entity_id)
                        );
                        return None;
                    }

                    // Validate relationship against whitelist
                    if !is_valid_relationship(&relationship) {
                        warn!(
                            "LLM returned unknown relationship '{}', skipping graph traversal",
                            relationship
                        );
                        return None;
                    }

                    Some(PlannedOperation::GraphTraversal {
                        entity_id,
                        relationship,
                    })
                }
                _ => {
                    warn!(
                        "LLM returned unknown operation type '{}', using default search",
                        op.operation_type
                    );
                    Some(PlannedOperation::Search {
                        query: query.to_string(),
                        search_types: vec![WorkerType::Unified],
                    })
                }
            })
            .collect();

        Ok(OrchestratorDecision {
            should_stop: decision.should_stop,
            reason: decision.reason,
            operations,
            iteration_cost: 0.01,
        })
    }

    /// Execute operations planned by orchestrator
    ///
    /// Executes search and graph operations with parallelization:
    /// - All search operations run concurrently
    /// - Graph traversals run after searches (need entities to reference)
    async fn execute_operations(
        &self,
        operations: &[PlannedOperation],
        request: &AgenticSearchRequest,
    ) -> Result<(Vec<AgenticEntity>, WorkerStats)> {
        use futures::future::join_all;

        let mut all_entities = Vec::new();
        let mut stats = WorkerStats {
            spawned: 0,
            succeeded: 0,
            failed: 0,
        };

        // Separate search and graph operations
        let search_ops: Vec<_> = operations
            .iter()
            .filter_map(|op| match op {
                PlannedOperation::Search {
                    query,
                    search_types,
                } => Some((query.clone(), search_types.clone())),
                _ => None,
            })
            .collect();

        let graph_ops: Vec<_> = operations
            .iter()
            .filter_map(|op| match op {
                PlannedOperation::GraphTraversal {
                    entity_id,
                    relationship,
                } => Some((entity_id.clone(), relationship.clone())),
                _ => None,
            })
            .collect();

        // Execute all search operations concurrently
        if !search_ops.is_empty() {
            let search_futures: Vec<_> = search_ops
                .iter()
                .map(|(query, search_types)| {
                    let worker_queries: Vec<WorkerQuery> = search_types
                        .iter()
                        .map(|worker_type| WorkerQuery {
                            worker_type: *worker_type,
                            query: query.clone(),
                            repository_ids: request.repository_ids.clone(),
                        })
                        .collect();
                    let count = worker_queries.len();
                    let search_api = self.search_api.clone();
                    let haiku_client = self.haiku_client.clone();
                    let haiku_model = self.haiku_model.clone();
                    async move {
                        let result =
                            execute_workers(worker_queries, search_api, haiku_client, haiku_model)
                                .await;
                        (count, result)
                    }
                })
                .collect();

            let search_results = join_all(search_futures).await;

            for (worker_count, result) in search_results {
                stats.spawned += worker_count;
                match result {
                    Ok(results) => {
                        stats.succeeded += results.len();
                        for r in results {
                            all_entities.extend(r.entities);
                        }
                    }
                    Err(AgenticSearchError::AllWorkersFailed) => {
                        warn!("All workers failed for search operation");
                        stats.failed += worker_count;
                    }
                    Err(e) => {
                        warn!("Workers partially failed: {}", e);
                        stats.failed += 1;
                    }
                }
            }
        }

        // Execute graph traversals (need entities from searches to reference)
        // These run after searches complete since they may reference newly found entities
        for (entity_id, relationship) in graph_ops {
            stats.spawned += 1;
            match self
                .execute_graph_traversal(
                    &entity_id,
                    &relationship,
                    &request.repository_ids,
                    &all_entities,
                )
                .await
            {
                Ok(entities) => {
                    stats.succeeded += 1;
                    all_entities.extend(entities);
                }
                Err(e) => {
                    debug!("Graph traversal failed (silent fallback): {}", e);
                    stats.failed += 1;
                }
            }
        }

        Ok((all_entities, stats))
    }

    /// Execute graph traversal to find related entities
    async fn execute_graph_traversal(
        &self,
        entity_id: &str,
        relationship: &str,
        repository_ids: &[String],
        accumulated_entities: &[AgenticEntity],
    ) -> Result<Vec<AgenticEntity>> {
        let query_type = relationship_to_query_type(relationship).ok_or_else(|| {
            AgenticSearchError::GraphTraversal(format!("Unknown relationship type: {relationship}"))
        })?;

        // Find the entity in accumulated results to get its qualified_name
        let source_entity = accumulated_entities
            .iter()
            .find(|e| e.entity.entity_id == entity_id)
            .ok_or_else(|| {
                AgenticSearchError::GraphTraversal(format!(
                    "Entity not found in accumulated results: {entity_id}"
                ))
            })?;

        let qualified_name = source_entity.entity.qualified_name.clone();

        // Get repository_id - use first available or from source entity
        let repository_id = repository_ids
            .first()
            .and_then(|s| uuid::Uuid::parse_str(s).ok())
            .unwrap_or(source_entity.entity.repository_id);

        let request = GraphQueryRequest {
            repository_id,
            query_type,
            parameters: GraphQueryParameters {
                qualified_name: qualified_name.clone(),
                max_depth: Some(2),
            },
            return_entities: true,
            semantic_filter: None,
            limit: 10,
        };

        debug!(
            "Executing graph traversal: {} -> {} ({})",
            qualified_name, relationship, entity_id
        );

        let response = self
            .search_api
            .query_graph(request)
            .await
            .map_err(|e| AgenticSearchError::GraphTraversal(e.to_string()))?;

        // Convert GraphResult to AgenticEntity with Graph source
        let entities: Vec<AgenticEntity> = response
            .results
            .into_iter()
            .filter_map(|result| {
                result.entity.map(|entity| {
                    let source = RetrievalSource::Graph {
                        source_entity_id: entity_id.to_string(),
                        relationship: relationship.to_string(),
                    };
                    let mut agentic = AgenticEntity::from_search_result(entity, source);
                    agentic.relevance_justification =
                        format!("Found via {relationship} relationship from {qualified_name}");
                    agentic
                })
            })
            .collect();

        debug!(
            "Graph traversal found {} entities via {} from {}",
            entities.len(),
            relationship,
            entity_id
        );

        Ok(entities)
    }

    /// Synthesize final top 10 results with dual-track quality gate
    async fn synthesize_final_results(
        &self,
        entities: Vec<AgenticEntity>,
        query: &str,
    ) -> Result<Vec<AgenticEntity>> {
        // Separate direct matches from graph context
        let (direct_candidates, graph_context): (Vec<_>, Vec<_>) =
            entities.into_iter().partition(|e| e.is_direct_match());

        // If no graph context, use simple score-based ranking
        if graph_context.is_empty() {
            return self.simple_top_n_by_score(direct_candidates, 10);
        }

        // Use quality gate composition with Sonnet
        self.synthesize_with_quality_gate(direct_candidates, graph_context, query)
            .await
    }

    /// Simple top N by score (fallback when no graph context)
    fn simple_top_n_by_score(
        &self,
        entities: Vec<AgenticEntity>,
        n: usize,
    ) -> Result<Vec<AgenticEntity>> {
        let mut sorted = entities;
        sorted.sort_by(|a, b| {
            b.entity
                .score
                .partial_cmp(&a.entity.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        Ok(sorted.into_iter().take(n).collect())
    }

    /// Synthesize results using quality gate composition with dual-track support
    async fn synthesize_with_quality_gate(
        &self,
        direct_candidates: Vec<AgenticEntity>,
        graph_context: Vec<AgenticEntity>,
        query: &str,
    ) -> Result<Vec<AgenticEntity>> {
        // Format entities for prompt
        let direct_text = format_entities_for_prompt(&direct_candidates, 20);
        let graph_text = format_entities_for_prompt(&graph_context, 10);

        let prompt = prompts::format_prompt(
            prompts::QUALITY_GATE_COMPOSE,
            &[
                ("direct_candidates", &direct_text),
                ("graph_context", &graph_text),
                ("query", query),
            ],
        );

        // Call Sonnet for composition
        let mut params = claudius::MessageCreateParams::simple(
            claudius::MessageParam::user(prompt),
            self.sonnet_model.clone(),
        );
        params.max_tokens = 4096;
        params.temperature = Some(0.0);

        let response =
            self.sonnet_client.send(params).await.map_err(|e| {
                AgenticSearchError::QualityGate(format!("Sonnet API call failed: {e}"))
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

        // Parse the result_list XML block
        let json_str = match extract_result_list(&response_text) {
            Ok(json) => json,
            Err(e) => {
                warn!(
                    "Quality gate response parsing failed, falling back to score-based: {}",
                    e
                );
                // Fallback: combine direct and graph, sort by score
                let mut combined = direct_candidates;
                combined.extend(graph_context);
                return self.simple_top_n_by_score(combined, 10);
            }
        };

        let composition: Vec<QualityGateResult> = serde_json::from_str(&json_str).map_err(|e| {
            AgenticSearchError::QualityGate(format!(
                "Failed to parse quality gate JSON: {e}. Content: {json_str}"
            ))
        })?;

        // Build entity lookup map
        let all_entities: HashMap<&str, &AgenticEntity> = direct_candidates
            .iter()
            .chain(graph_context.iter())
            .map(|e| (e.entity.entity_id.as_str(), e))
            .collect();

        // Build final results from composition
        let mut final_results: Vec<AgenticEntity> = Vec::new();
        for entry in composition.into_iter().take(10) {
            if let Some(&entity) = all_entities.get(entry.entity_id.as_str()) {
                let mut result = entity.clone();
                result.relevance_justification = entry.relevance_justification;
                if let Some(ref mut reasoning) = result.entity.reasoning {
                    *reasoning = result.relevance_justification.clone();
                } else {
                    result.entity.reasoning = Some(result.relevance_justification.clone());
                }
                final_results.push(result);
            } else {
                warn!(
                    "Quality gate referenced unknown entity: {}",
                    entry.entity_id
                );
            }
        }

        // Fallback: if quality gate returned too few, fill from direct candidates
        if final_results.len() < 5 {
            warn!(
                "Quality gate returned only {} results, filling from direct candidates",
                final_results.len()
            );
            // Collect existing IDs as owned strings to avoid borrow conflict
            let existing_ids: std::collections::HashSet<String> = final_results
                .iter()
                .map(|e| e.entity.entity_id.clone())
                .collect();

            for entity in &direct_candidates {
                if final_results.len() >= 10 {
                    break;
                }
                if !existing_ids.contains(&entity.entity.entity_id) {
                    final_results.push(entity.clone());
                }
            }
        }

        Ok(final_results)
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
        entity_id: String,
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

    // ========================================================================
    // Phase 3 Unit Tests: Relationship Mapping
    // ========================================================================

    #[test]
    fn test_relationship_mapping_callers() {
        assert!(matches!(
            relationship_to_query_type("callers"),
            Some(GraphQueryType::FindFunctionCallers)
        ));
        assert!(matches!(
            relationship_to_query_type("called_by"),
            Some(GraphQueryType::FindFunctionCallers)
        ));
        // Case insensitive
        assert!(matches!(
            relationship_to_query_type("CALLERS"),
            Some(GraphQueryType::FindFunctionCallers)
        ));
        assert!(matches!(
            relationship_to_query_type("who_calls"),
            Some(GraphQueryType::FindFunctionCallers)
        ));
    }

    #[test]
    fn test_relationship_mapping_callees() {
        assert!(matches!(
            relationship_to_query_type("callees"),
            Some(GraphQueryType::FindFunctionCallees)
        ));
        assert!(matches!(
            relationship_to_query_type("calls"),
            Some(GraphQueryType::FindFunctionCallees)
        ));
        assert!(matches!(
            relationship_to_query_type("what_calls"),
            Some(GraphQueryType::FindFunctionCallees)
        ));
    }

    #[test]
    fn test_relationship_mapping_implementations() {
        assert!(matches!(
            relationship_to_query_type("implementations"),
            Some(GraphQueryType::FindTraitImplementations)
        ));
        assert!(matches!(
            relationship_to_query_type("implements"),
            Some(GraphQueryType::FindTraitImplementations)
        ));
        assert!(matches!(
            relationship_to_query_type("implementors"),
            Some(GraphQueryType::FindTraitImplementations)
        ));
    }

    #[test]
    fn test_relationship_mapping_hierarchy() {
        assert!(matches!(
            relationship_to_query_type("hierarchy"),
            Some(GraphQueryType::FindClassHierarchy)
        ));
        assert!(matches!(
            relationship_to_query_type("extends"),
            Some(GraphQueryType::FindClassHierarchy)
        ));
        assert!(matches!(
            relationship_to_query_type("inherits"),
            Some(GraphQueryType::FindClassHierarchy)
        ));
    }

    #[test]
    fn test_relationship_mapping_module_contents() {
        assert!(matches!(
            relationship_to_query_type("contains"),
            Some(GraphQueryType::FindModuleContents)
        ));
        assert!(matches!(
            relationship_to_query_type("module_contents"),
            Some(GraphQueryType::FindModuleContents)
        ));
        assert!(matches!(
            relationship_to_query_type("in_module"),
            Some(GraphQueryType::FindModuleContents)
        ));
    }

    #[test]
    fn test_relationship_mapping_dependencies() {
        assert!(matches!(
            relationship_to_query_type("dependencies"),
            Some(GraphQueryType::FindModuleDependencies)
        ));
        assert!(matches!(
            relationship_to_query_type("imports"),
            Some(GraphQueryType::FindModuleDependencies)
        ));
        assert!(matches!(
            relationship_to_query_type("uses"),
            Some(GraphQueryType::FindModuleDependencies)
        ));
    }

    #[test]
    fn test_relationship_mapping_unknown() {
        assert!(relationship_to_query_type("unknown_relationship").is_none());
        assert!(relationship_to_query_type("").is_none());
        assert!(relationship_to_query_type("foobar").is_none());
    }

    // ========================================================================
    // LLM Input Validation Tests
    // ========================================================================

    #[test]
    fn test_is_valid_entity_id() {
        assert!(is_valid_entity_id("abc-123"));
        assert!(is_valid_entity_id("a"));
        assert!(is_valid_entity_id(&"x".repeat(100)));

        // Invalid cases
        assert!(!is_valid_entity_id(""));
        assert!(!is_valid_entity_id(&"x".repeat(101)));
    }

    #[test]
    fn test_is_valid_relationship() {
        // Valid relationships
        assert!(is_valid_relationship("callers"));
        assert!(is_valid_relationship("CALLERS")); // case insensitive
        assert!(is_valid_relationship("callees"));
        assert!(is_valid_relationship("implementations"));
        assert!(is_valid_relationship("hierarchy"));
        assert!(is_valid_relationship("dependencies"));

        // Invalid relationships
        assert!(!is_valid_relationship("unknown"));
        assert!(!is_valid_relationship(""));
        assert!(!is_valid_relationship("drop_table"));
    }

    // ========================================================================
    // Phase 3 Unit Tests: XML Parsing
    // ========================================================================

    #[test]
    fn test_extract_result_list_clean() {
        let response = r#"
<result_list>
[{"entity_id": "e1", "relevance_justification": "Direct match"}]
</result_list>
"#;
        let result = extract_result_list(response).unwrap();
        assert!(result.contains("entity_id"));
        assert!(result.contains("e1"));
    }

    #[test]
    fn test_extract_result_list_with_chatty_prefix() {
        let response = r#"
Based on my analysis of the candidates, here is my composed result list:

<result_list>
[{"entity_id": "e1", "relevance_justification": "Primary implementation"}]
</result_list>

I prioritized semantic matches because they had the highest relevance scores.
"#;
        let result = extract_result_list(response).unwrap();
        assert!(result.contains("entity_id"));
        assert!(result.contains("e1"));
        // Should NOT contain the chatty text
        assert!(!result.contains("Based on my analysis"));
        assert!(!result.contains("I prioritized"));
    }

    #[test]
    fn test_extract_result_list_missing_start_tag() {
        let response = r#"
[{"entity_id": "e1"}]
</result_list>
"#;
        let result = extract_result_list(response);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Missing <result_list> tag"));
    }

    #[test]
    fn test_extract_result_list_missing_end_tag() {
        let response = r#"
<result_list>
[{"entity_id": "e1"}]
"#;
        let result = extract_result_list(response);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Missing </result_list> tag"));
    }

    #[test]
    fn test_extract_result_list_empty_array() {
        let response = r#"
<result_list>
[]
</result_list>
"#;
        let result = extract_result_list(response).unwrap();
        assert_eq!(result, "[]");
    }

    #[test]
    fn test_extract_result_list_multiline_json() {
        let response = r#"
<result_list>
[
  {
    "entity_id": "uuid-1",
    "relevance_justification": "Core implementation of the search functionality"
  },
  {
    "entity_id": "uuid-2",
    "relevance_justification": "Entry point that calls the search"
  }
]
</result_list>
"#;
        let result = extract_result_list(response).unwrap();
        // Parse it to verify it's valid JSON
        let parsed: Vec<crate::types::QualityGateResult> = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].entity_id, "uuid-1");
        assert_eq!(parsed[1].entity_id, "uuid-2");
    }

    // ========================================================================
    // Phase 3 Unit Tests: Graph Traversal Decision Parsing
    // ========================================================================

    #[test]
    fn test_graph_traversal_decision_parsing() {
        let json = r#"{
            "should_stop": false,
            "reason": "Need to find callers of the function",
            "operations": [
                {
                    "operation_type": "graph_traversal",
                    "entity_id": "uuid-123",
                    "relationship": "callers"
                }
            ]
        }"#;

        let decision: OrchestratorDecisionResponse = serde_json::from_str(json).unwrap();
        assert!(!decision.should_stop);
        assert_eq!(decision.operations.len(), 1);
        assert_eq!(decision.operations[0].operation_type, "graph_traversal");
        assert_eq!(
            decision.operations[0].entity_id.as_ref().unwrap(),
            "uuid-123"
        );
        assert_eq!(
            decision.operations[0].relationship.as_ref().unwrap(),
            "callers"
        );
    }

    #[test]
    fn test_mixed_operations_parsing() {
        let json = r#"{
            "should_stop": false,
            "reason": "Need both search and graph expansion",
            "operations": [
                {
                    "operation_type": "search",
                    "query": "JWT validation",
                    "search_types": ["semantic"]
                },
                {
                    "operation_type": "graph_traversal",
                    "entity_id": "uuid-456",
                    "relationship": "callees"
                }
            ]
        }"#;

        let decision: OrchestratorDecisionResponse = serde_json::from_str(json).unwrap();
        assert_eq!(decision.operations.len(), 2);
        assert_eq!(decision.operations[0].operation_type, "search");
        assert_eq!(decision.operations[1].operation_type, "graph_traversal");
    }

    // ========================================================================
    // Quality Gate Result Parsing Tests
    // ========================================================================

    #[test]
    fn test_quality_gate_result_parsing_valid() {
        let json = r#"[
            {"entity_id": "e1", "relevance_justification": "Main implementation"},
            {"entity_id": "e2", "relevance_justification": "Calls the main function"}
        ]"#;
        let parsed: Vec<crate::types::QualityGateResult> = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].entity_id, "e1");
        assert_eq!(parsed[0].relevance_justification, "Main implementation");
        assert_eq!(parsed[1].entity_id, "e2");
    }

    #[test]
    fn test_quality_gate_result_parsing_minimal() {
        let json = r#"[{"entity_id": "e1", "relevance_justification": "Direct match"}]"#;
        let parsed: Vec<crate::types::QualityGateResult> = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].entity_id, "e1");
        assert_eq!(parsed[0].relevance_justification, "Direct match");
    }

    #[test]
    fn test_quality_gate_result_parsing_empty_array() {
        let json = r#"[]"#;
        let parsed: Vec<crate::types::QualityGateResult> = serde_json::from_str(json).unwrap();
        assert!(parsed.is_empty());
    }

    #[test]
    fn test_extract_result_list_fallback_no_tags() {
        // When result_list tags are missing, extract_result_list should error
        let response = r#"Here are the results: [{"entity_id": "e1"}]"#;
        let result = extract_result_list(response);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Missing <result_list>"));
    }

    #[test]
    fn test_extract_result_list_with_chatty_llm_response() {
        // Simulates a chatty LLM that adds commentary before and after
        let response = r#"
Based on my analysis of the candidates, I've composed the following result list:

<result_list>
[
    {"entity_id": "primary-1", "relevance_justification": "Core functionality"}
]
</result_list>

I prioritized direct matches because they had the highest semantic relevance.
The graph context entities were less relevant for this particular query.
"#;
        let result = extract_result_list(response).unwrap();
        let parsed: Vec<crate::types::QualityGateResult> = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].entity_id, "primary-1");
    }
}
