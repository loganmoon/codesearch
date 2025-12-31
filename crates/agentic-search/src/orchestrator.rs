//! Agentic search orchestrator with iterative loop

use crate::{
    config::AgenticSearchConfig,
    error::{truncate_for_error, AgenticSearchError, Result},
    prompts,
    types::{
        AgenticEntity, AgenticSearchMetadata, AgenticSearchRequest, AgenticSearchResponse,
        RerankingMethod, RetrievalSource,
    },
    worker::{execute_workers, WorkerQuery, WorkerType},
};
use codesearch_core::search_models::{GraphQueryParameters, GraphQueryRequest, GraphQueryType};
use codesearch_core::SearchApi;
use codesearch_reranking::{create_reranker_provider, RerankerProvider};
use serde::Deserialize;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;
use tracing::error;
use tracing::{debug, info, warn};

const MAX_ITERATIONS: usize = 5;
const MAX_ITERATIONS_SIMPLE_LOOKUP: usize = 1;
const MAX_LLM_QUERY_LENGTH: usize = 1000;

/// Detect if query is a simple entity lookup (vs complex relational query).
/// Simple lookups should complete in 1 iteration - no need for query reformulation.
fn is_simple_lookup_query(query: &str) -> bool {
    let query_lower = query.to_lowercase();

    // Pattern: "Find the X function/struct/trait/class/method"
    // Pattern: "Where is X defined"
    // Pattern: "Show me the X"
    // Pattern: "What is the X function"
    let simple_patterns = [
        "find the ",
        "find function ",
        "find struct ",
        "find trait ",
        "find class ",
        "find method ",
        "where is ",
        "show me the ",
        "what is the ",
        "locate the ",
        "get the ",
    ];

    // Check if starts with a simple lookup pattern
    let starts_with_simple = simple_patterns.iter().any(|p| query_lower.starts_with(p));

    // Check for relational keywords that indicate complex queries
    let relational_keywords = [
        "calls",
        "called by",
        "who calls",
        "callers",
        "callees",
        "implements",
        "implementors",
        "inherits",
        "extends",
        "uses",
        "used by",
        "dependencies",
        "imports",
        "related to",
        "connected to",
        "hierarchy",
        "all functions that",
        "all methods that",
        "everything that",
    ];

    let has_relational = relational_keywords.iter().any(|k| query_lower.contains(k));

    starts_with_simple && !has_relational
}

const VALID_RELATIONSHIPS: &[&str] = &[
    "callers",
    "called_by",
    "who_calls",
    "callees",
    "calls",
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

/// Validate entity_id matches the expected format from entity_id.rs:
/// - Named entities: "entity-{32 lowercase hex chars}" (39 chars total)
/// - Anonymous entities: "entity-anon-{32 lowercase hex chars}" (44 chars total)
fn is_valid_entity_id(entity_id: &str) -> bool {
    let is_named = entity_id.starts_with("entity-")
        && !entity_id.starts_with("entity-anon-")
        && entity_id.len() == 39;
    let is_anon = entity_id.starts_with("entity-anon-") && entity_id.len() == 44;

    if !is_named && !is_anon {
        return false;
    }

    // Verify the hex portion contains only valid hex characters
    let hex_start = if is_anon { 12 } else { 7 }; // "entity-anon-" = 12, "entity-" = 7
    entity_id[hex_start..]
        .chars()
        .all(|c| c.is_ascii_hexdigit())
}

/// Check if relationship is in the allowed whitelist
fn is_valid_relationship(relationship: &str) -> bool {
    VALID_RELATIONSHIPS.contains(&relationship.to_lowercase().as_str())
}

/// Map relationship strings from orchestrator to GraphQueryType
fn relationship_to_query_type(relationship: &str) -> Option<GraphQueryType> {
    match relationship.to_lowercase().as_str() {
        "callers" | "called_by" | "who_calls" => Some(GraphQueryType::FindFunctionCallers),
        "callees" | "calls" => Some(GraphQueryType::FindFunctionCallees),
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
/// Entity ID is made prominent to help the LLM copy it correctly for graph_traversal
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
                "[{entity_id}] {entity_type}: {qualified_name}{source_info}\n\
                 Score: {score:.2}\n\
                 Justification: {justification}\n\
                 Content: {content}",
                entity_id = e.entity.entity_id,
                entity_type = e.entity.entity_type,
                qualified_name = e.entity.qualified_name,
                source_info = source_info,
                score = e.entity.score,
                justification = e.relevance_justification,
                content = e.entity.content.as_ref().map_or("N/A".to_string(), |c| {
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
    sonnet_model: claudius::Model,
    config: AgenticSearchConfig,
    /// Optional reranker for final result synthesis against original query
    reranker: Option<Arc<dyn RerankerProvider>>,
}

impl std::fmt::Debug for AgenticSearchOrchestrator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgenticSearchOrchestrator")
            .field("sonnet_client", &"<Anthropic>")
            .field("sonnet_model", &self.sonnet_model)
            .field("config", &self.config)
            .field("reranker", &self.reranker.is_some())
            .finish()
    }
}

impl AgenticSearchOrchestrator {
    pub async fn new(search_api: Arc<dyn SearchApi>, config: AgenticSearchConfig) -> Result<Self> {
        config.validate().map_err(AgenticSearchError::Config)?;

        let api_key = config
            .resolve_api_key()
            .ok_or(AgenticSearchError::MissingApiKey)?;

        let sonnet_client = Arc::new(claudius::Anthropic::new(Some(api_key)).map_err(|e| {
            AgenticSearchError::Config(format!("Failed to create Sonnet client: {e}"))
        })?);

        let sonnet_model = claudius::Model::Custom(config.orchestrator_model.clone());

        // Create reranker from config if reranking is configured
        let reranker = if let Some(ref reranking_config) = config.reranking {
            match create_reranker_provider(reranking_config).await {
                Ok(r) => {
                    info!(
                        "Created {} reranker for final synthesis",
                        reranking_config.provider
                    );
                    Some(r)
                }
                Err(e) => {
                    error!("Failed to create reranker, continuing without: {e}");
                    None
                }
            }
        } else {
            None
        };

        Ok(Self {
            search_api,
            sonnet_client,
            sonnet_model,
            config,
            reranker,
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
        let mut workers_spawned = 0;
        let mut workers_succeeded = 0;
        let mut partial_outage = false;

        // Detect simple lookup queries and limit iterations accordingly
        let is_simple = is_simple_lookup_query(&request.query);
        let max_iterations = if is_simple {
            info!(
                "Detected simple lookup query, limiting to {} iteration(s)",
                MAX_ITERATIONS_SIMPLE_LOOKUP
            );
            MAX_ITERATIONS_SIMPLE_LOOKUP
        } else {
            MAX_ITERATIONS
        };

        info!("Starting agentic search: {}", request.query);

        // Agentic loop
        loop {
            iteration += 1;
            info!("Iteration {}/{}", iteration, max_iterations);

            // Call orchestrator to evaluate and plan next operations
            let decision = self
                .orchestrator_loop_iteration(&request.query, &accumulated_entities, iteration)
                .await?;

            // Check stop conditions BEFORE executing (orchestrator said to stop)
            if decision.should_stop {
                info!("Orchestrator decided to stop: {}", decision.reason);
                break;
            }

            // Prevent infinite loop when LLM returns should_stop=false but all operations are invalid
            if decision.operations.is_empty() {
                warn!(
                    "Orchestrator returned no valid operations despite should_stop=false, forcing stop"
                );
                break;
            }

            // Execute planned operations (pass accumulated_entities for graph traversals)
            let (new_entities, worker_stats) = self
                .execute_operations(&decision.operations, &request, &accumulated_entities)
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

            // Check iteration limit AFTER executing (so we complete at least one iteration)
            if iteration >= max_iterations {
                info!("Reached max iterations ({})", max_iterations);
                break;
            }
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

        // Determine reranking method based on actual reranker availability
        // (reranker creation can fail even if config is present)
        let reranking_method = if self.reranker.is_some() {
            RerankingMethod::CrossEncoder
        } else {
            RerankingMethod::None
        };

        Ok(AgenticSearchResponse {
            results: final_results,
            metadata: AgenticSearchMetadata {
                query_time_ms,
                iterations: iteration,
                workers_spawned,
                workers_succeeded,
                partial_outage,
                total_direct_candidates: direct_candidates,
                graph_context_entities: graph_context,
                graph_entities_in_results: graph_in_results,
                reranking_method,
                graph_traversal_used: graph_context > 0,
                estimated_cost_usd: 0.0,
                cache_read_tokens: 0,
                cache_creation_tokens: 0,
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

        // Format accumulated context with entity IDs prominently displayed
        // so the LLM can copy them for graph_traversal operations
        let context = if accumulated_entities.is_empty() {
            "No entities found yet.".to_string()
        } else {
            format_entities_for_prompt(accumulated_entities, 20)
        };

        // Debug: log first 3 entities in context
        for (i, entity) in accumulated_entities.iter().take(3).enumerate() {
            debug!(
                "Context entity[{}]: {} ({}) - {}",
                i, entity.entity.entity_type, entity.entity.entity_id, entity.entity.qualified_name
            );
        }

        // Create system prompt with cache control for cost reduction
        let system_block = claudius::TextBlock::new(prompts::ORCHESTRATOR_PLAN_SYSTEM.to_string())
            .with_cache_control(claudius::CacheControlEphemeral::new());

        // Format user message with dynamic content
        let user_prompt = prompts::format_prompt(
            prompts::ORCHESTRATOR_PLAN_USER,
            &[
                ("query", query),
                ("context", &context),
                ("iteration", &iteration.to_string()),
                ("max_iterations", &MAX_ITERATIONS.to_string()),
            ],
        );

        // Define tool schema for structured output
        let tool_schema = serde_json::json!({
            "type": "object",
            "properties": {
                "should_stop": {
                    "type": "boolean",
                    "description": "Whether to stop the search loop"
                },
                "reason": {
                    "type": "string",
                    "description": "Brief explanation of the decision"
                },
                "operations": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "operation_type": {
                                "type": "string",
                                "enum": ["search", "graph_traversal"]
                            },
                            "query": {
                                "type": "string",
                                "description": "Search query (for search operations)"
                            },
                            "entity_id": {
                                "type": "string",
                                "description": "Entity ID to traverse from (for graph_traversal)"
                            },
                            "relationship": {
                                "type": "string",
                                "description": "Relationship type (for graph_traversal)"
                            }
                        },
                        "required": ["operation_type"]
                    }
                }
            },
            "required": ["should_stop", "reason", "operations"]
        });

        let tool = claudius::ToolUnionParam::new_custom_tool(
            "orchestrator_decision".to_string(),
            tool_schema,
        );

        // Call Sonnet with cached system prompt and forced tool use
        let params = claudius::MessageCreateParams::new(
            4096,
            vec![claudius::MessageParam::user(user_prompt)],
            self.sonnet_model.clone(),
        )
        .with_system_blocks(vec![system_block])
        .with_temperature(0.0)
        .map_err(|e| AgenticSearchError::Orchestrator(format!("Invalid temperature: {e}")))?
        .with_tools(vec![tool])
        .with_tool_choice(claudius::ToolChoice::tool("orchestrator_decision"));

        let response = self.sonnet_client.send(params).await.map_err(|e| {
            AgenticSearchError::Orchestrator(format!("Sonnet API call failed: {e}"))
        })?;

        // Extract structured decision from tool use block
        let tool_use_block = response
            .content
            .iter()
            .find_map(|block| block.as_tool_use())
            .ok_or_else(|| {
                // Fallback: extract text for error message
                let response_text = response
                    .content
                    .iter()
                    .filter_map(|block| match block {
                        claudius::ContentBlock::Text(text_block) => Some(text_block.text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                AgenticSearchError::Orchestrator(format!(
                    "No tool use block in Sonnet response: {}",
                    truncate_for_error(&response_text)
                ))
            })?;

        let decision: OrchestratorDecisionResponse =
            serde_json::from_value(tool_use_block.input.clone()).map_err(|e| {
                AgenticSearchError::Orchestrator(format!(
                    "Failed to parse tool input: {e}. Input: {}",
                    truncate_for_error(&tool_use_block.input.to_string())
                ))
            })?;

        // Log raw LLM decision before validation
        debug!(
            "Orchestrator LLM raw decision: should_stop={}, reason='{}', operations={:?}",
            decision.should_stop,
            decision.reason,
            decision
                .operations
                .iter()
                .map(|op| format!(
                    "{}({})",
                    op.operation_type,
                    if op.operation_type == "graph_traversal" {
                        format!(
                            "entity_id={}, rel={}",
                            op.entity_id.as_deref().unwrap_or("none"),
                            op.relationship.as_deref().unwrap_or("none")
                        )
                    } else {
                        op.query.as_deref().unwrap_or("none").to_string()
                    }
                ))
                .collect::<Vec<_>>()
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

                    // All search operations now use semantic search only
                    // (which combines dense embeddings + BM25 sparse retrieval)
                    Some(PlannedOperation::Search {
                        query: truncated_query,
                        search_types: vec![WorkerType::Semantic],
                    })
                }
                "graph_traversal" => {
                    let entity_id = op.entity_id.unwrap_or_default();
                    let relationship = op.relationship.unwrap_or_default();

                    let valid_id = is_valid_entity_id(&entity_id);
                    let valid_rel = is_valid_relationship(&relationship);

                    debug!(
                        "Graph traversal validation: entity_id='{}' (valid={}), relationship='{}' (valid={})",
                        truncate_for_error(&entity_id),
                        valid_id,
                        relationship,
                        valid_rel
                    );

                    // Validate entity_id
                    if !valid_id {
                        warn!(
                            "LLM returned invalid entity_id '{}', skipping graph traversal. Expected format: entity-{{32 hex chars}}",
                            truncate_for_error(&entity_id)
                        );
                        return None;
                    }

                    // Validate relationship against whitelist
                    if !valid_rel {
                        warn!(
                            "LLM returned unknown relationship '{}', skipping graph traversal. Valid: {:?}",
                            relationship,
                            VALID_RELATIONSHIPS
                        );
                        return None;
                    }

                    debug!(
                        "Graph traversal operation accepted: entity_id={}, relationship={}",
                        entity_id, relationship
                    );

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
                        search_types: vec![WorkerType::Semantic],
                    })
                }
            })
            .collect();

        Ok(OrchestratorDecision {
            should_stop: decision.should_stop,
            reason: decision.reason,
            operations,
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
        accumulated_entities: &[AgenticEntity],
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
                    let rerank_config = self.config.reranking_request.clone();
                    let semantic_candidates = self.config.semantic_candidates;
                    async move {
                        let result = execute_workers(
                            worker_queries,
                            search_api,
                            rerank_config,
                            semantic_candidates,
                        )
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

            // Log entities found from search for debugging graph traversal issues
            info!(
                "Search operations found {} entities this iteration",
                all_entities.len()
            );
            for (i, e) in all_entities.iter().take(10).enumerate() {
                debug!(
                    "  [{}] {} -> {}",
                    i + 1,
                    e.entity.entity_id,
                    e.entity.qualified_name
                );
            }
        }

        // Execute graph traversals (need entities from previous iterations + current)
        // Entity IDs from LLM come from accumulated_entities (previous iterations)
        // Build combined entity list once before the loop to avoid O(G × (A + N)) cloning
        let combined_entities: Vec<&AgenticEntity> = accumulated_entities
            .iter()
            .chain(all_entities.iter())
            .collect();

        info!(
            "Processing {} graph traversal operations with {} accumulated + {} new entities",
            graph_ops.len(),
            accumulated_entities.len(),
            all_entities.len()
        );

        // Collect graph results separately to avoid borrow conflicts
        let mut graph_entities: Vec<AgenticEntity> = Vec::new();
        for (entity_id, relationship) in graph_ops {
            stats.spawned += 1;
            match self
                .execute_graph_traversal(
                    &entity_id,
                    &relationship,
                    &request.repository_ids,
                    &combined_entities,
                )
                .await
            {
                Ok(entities) => {
                    stats.succeeded += 1;
                    info!(
                        "Graph traversal {} -> {} found {} entities",
                        entity_id,
                        relationship,
                        entities.len()
                    );
                    graph_entities.extend(entities);
                }
                Err(e) => {
                    warn!("Graph traversal failed: {}", e);
                    stats.failed += 1;
                }
            }
        }
        all_entities.extend(graph_entities);

        Ok((all_entities, stats))
    }

    /// Execute graph traversal to find related entities
    async fn execute_graph_traversal(
        &self,
        entity_id: &str,
        relationship: &str,
        repository_ids: &[String],
        accumulated_entities: &[&AgenticEntity],
    ) -> Result<Vec<AgenticEntity>> {
        debug!(
            "Looking for entity_id='{}' in {} accumulated entities to execute {} traversal",
            entity_id,
            accumulated_entities.len(),
            relationship
        );

        // Log available entity IDs for debugging
        if accumulated_entities.len() <= 20 {
            for e in accumulated_entities.iter() {
                debug!(
                    "  Available: {} -> {}",
                    e.entity.entity_id, e.entity.qualified_name
                );
            }
        } else {
            debug!(
                "  First 10 available: {:?}",
                accumulated_entities
                    .iter()
                    .take(10)
                    .map(|e| format!("{} -> {}", e.entity.entity_id, e.entity.qualified_name))
                    .collect::<Vec<_>>()
            );
        }

        let query_type = relationship_to_query_type(relationship).ok_or_else(|| {
            AgenticSearchError::GraphTraversal(format!("Unknown relationship type: {relationship}"))
        })?;

        // Find the entity in accumulated results to get its qualified_name
        let source_entity = accumulated_entities
            .iter()
            .find(|e| e.entity.entity_id == entity_id)
            .ok_or_else(|| {
                warn!(
                    "Entity '{}' not found in {} accumulated entities. This indicates the LLM \
                     referenced an entity_id that doesn't exist in the search results.",
                    entity_id,
                    accumulated_entities.len()
                );
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
            // Don't artificially limit graph traversals - let all relationships be returned
            // The graph query itself has depth limits that naturally bound results
            limit: 1000,
        };

        debug!(
            "Executing graph traversal: qualified_name='{}' relationship='{}' entity_id='{}'",
            qualified_name, relationship, entity_id
        );

        let response = self
            .search_api
            .query_graph(request)
            .await
            .map_err(|e| AgenticSearchError::GraphTraversal(e.to_string()))?;

        // Convert GraphResult to AgenticEntity with Graph source
        // Strip content to avoid context window blowup - graph results provide
        // structural context, not code content
        let entities: Vec<AgenticEntity> = response
            .results
            .into_iter()
            .filter_map(|result| {
                result.entity.map(|mut entity| {
                    // Clear content to reduce context size - graph traversal provides
                    // structural relationships, not code for detailed inspection
                    entity.content = None;

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

    /// Synthesize final results
    ///
    /// Graph results are exhaustive and unranked (e.g., ALL callers of X), so they
    /// should not be filtered. Direct candidates get quality-gated to pick the best.
    ///
    /// When a reranker is available, direct candidates are reranked against the
    /// original query to ensure consistent scoring across multiple iterations.
    async fn synthesize_final_results(
        &self,
        entities: Vec<AgenticEntity>,
        query: &str,
    ) -> Result<Vec<AgenticEntity>> {
        // Separate direct matches from graph context
        let (direct_candidates, graph_context): (Vec<_>, Vec<_>) =
            entities.into_iter().partition(|e| e.is_direct_match());

        // If no graph context, rerank and return top direct candidates
        if graph_context.is_empty() {
            let reranked = self
                .rerank_against_original_query(direct_candidates, query)
                .await?;
            return self.simple_top_n_by_score(reranked, 10);
        }

        // Graph context exists: quality-gate direct candidates, include ALL graph results
        // Graph results are exhaustive (all callers, all implementations, etc.) and
        // already passed quality gate when orchestrator decided to traverse them
        let quality_gated_direct = self
            .quality_gate_direct_candidates(direct_candidates, &graph_context, query)
            .await?;

        // Combine: quality-gated direct + ALL graph results (unfiltered)
        let mut final_results = quality_gated_direct;
        final_results.extend(graph_context);

        Ok(final_results)
    }

    /// Rerank entities against the original query using cross-encoder.
    ///
    /// This ensures consistent scoring when multiple search iterations with
    /// different queries have accumulated results. If no reranker is configured,
    /// returns entities unchanged.
    async fn rerank_against_original_query(
        &self,
        mut entities: Vec<AgenticEntity>,
        query: &str,
    ) -> Result<Vec<AgenticEntity>> {
        let reranker = match &self.reranker {
            Some(r) => r,
            None => return Ok(entities),
        };

        if entities.is_empty() {
            return Ok(entities);
        }

        info!(
            "Reranking {} entities against original query for final synthesis",
            entities.len()
        );

        // Prepare documents for reranking: (entity_id, content)
        let documents: Vec<(String, &str)> = entities
            .iter()
            .map(|e| {
                let content = e.entity.content.as_deref().unwrap_or("");
                (e.entity.entity_id.clone(), content)
            })
            .collect();

        // Call reranker
        let rerank_results = reranker
            .rerank(query, &documents)
            .await
            .map_err(|e| AgenticSearchError::Reranking(e.to_string()))?;

        // Build a map of entity_id -> new score
        let score_map: std::collections::HashMap<String, f32> =
            rerank_results.into_iter().collect();

        // Update entity scores
        for entity in &mut entities {
            if let Some(&new_score) = score_map.get(&entity.entity.entity_id) {
                entity.entity.score = new_score;
            }
        }

        Ok(entities)
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

    /// Quality gate for direct candidates only (when graph context exists)
    ///
    /// When we have graph results, we know the query is relationship-focused
    /// (e.g., "What calls X?"). The direct candidates likely include the target
    /// entity itself, which should be included. We use score-based ranking here
    /// to avoid an extra LLM call - the orchestrator already validated these
    /// entities are relevant.
    async fn quality_gate_direct_candidates(
        &self,
        direct_candidates: Vec<AgenticEntity>,
        _graph_context: &[AgenticEntity],
        _query: &str,
    ) -> Result<Vec<AgenticEntity>> {
        // For graph-focused queries, direct candidates usually include the target
        // entity (e.g., the function X when asking "What calls X?"). Keep top
        // candidates by score - they've already been validated by the orchestrator.
        self.simple_top_n_by_score(direct_candidates, 5)
    }
}

/// Decision from orchestrator loop iteration
#[derive(Debug)]
struct OrchestratorDecision {
    should_stop: bool,
    reason: String,
    operations: Vec<PlannedOperation>,
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
                    "query": "JWT validation implementation"
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
    // Simple Lookup Query Detection Tests
    // ========================================================================

    #[test]
    fn test_simple_lookup_detection() {
        // Simple lookup queries - should return true
        assert!(is_simple_lookup_query(
            "Find the lex_item function that reads characters"
        ));
        assert!(is_simple_lookup_query(
            "Find the math_result_type function that determines types"
        ));
        assert!(is_simple_lookup_query("Where is the Config struct defined"));
        assert!(is_simple_lookup_query("Show me the main function"));
        assert!(is_simple_lookup_query("What is the Parser trait"));
        assert!(is_simple_lookup_query("Locate the error handling module"));
        assert!(is_simple_lookup_query("Get the configuration struct"));

        // Complex/relational queries - should return false
        assert!(!is_simple_lookup_query("What functions call parse_token"));
        assert!(!is_simple_lookup_query(
            "Find all functions that implement the Parser trait"
        ));
        assert!(!is_simple_lookup_query("Show me the callers of main"));
        assert!(!is_simple_lookup_query(
            "What uses the Config struct and how is it used by other modules"
        ));
        assert!(!is_simple_lookup_query(
            "Find the dependencies of the lexer module"
        ));
        assert!(!is_simple_lookup_query("Show the class hierarchy for Node"));

        // Non-matching patterns - should return false
        assert!(!is_simple_lookup_query("How does the parser work"));
        assert!(!is_simple_lookup_query("Explain the architecture"));
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
        // Valid named entity IDs (entity- + 32 hex chars = 39 total)
        assert!(is_valid_entity_id(
            "entity-a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6"
        ));
        assert!(is_valid_entity_id(
            "entity-00000000000000000000000000000000"
        ));
        assert!(is_valid_entity_id(
            "entity-ffffffffffffffffffffffffffffffff"
        ));

        // Valid anonymous entity IDs (entity-anon- + 32 hex chars = 44 total)
        assert!(is_valid_entity_id(
            "entity-anon-a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6"
        ));

        // Invalid cases - wrong format
        assert!(!is_valid_entity_id("")); // empty
        assert!(!is_valid_entity_id("abc-123")); // wrong prefix
        assert!(!is_valid_entity_id("entity-")); // missing hash
        assert!(!is_valid_entity_id("entity-a1b2c3d4")); // too short
        assert!(!is_valid_entity_id(
            "entity-a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6extra"
        )); // too long
        assert!(!is_valid_entity_id(
            "entity-g1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6"
        )); // non-hex char 'g'
        assert!(!is_valid_entity_id("entity-test_ty")); // hallucinated name-based ID
        assert!(!is_valid_entity_id("b6830516fc831c8b98529312fca2a5d9")); // missing prefix
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
    // Entity Lookup Tests (for graph traversal bug fix)
    // ========================================================================

    /// Helper to create a mock AgenticEntity for testing
    fn create_test_agentic_entity(entity_id: &str, name: &str) -> AgenticEntity {
        use codesearch_core::entities::{EntityType, Language, SourceLocation, Visibility};
        use codesearch_core::search_models::EntityResult;

        let entity = EntityResult {
            entity_id: entity_id.to_string(),
            repository_id: uuid::Uuid::new_v4(),
            qualified_name: name.to_string(),
            name: name.to_string(),
            entity_type: EntityType::Function,
            language: Language::Rust,
            file_path: format!("src/{name}.rs"),
            location: SourceLocation {
                start_line: 1,
                end_line: 10,
                start_column: 0,
                end_column: 0,
            },
            content: Some(format!("fn {name}() {{}}")),
            signature: None,
            documentation_summary: None,
            visibility: Some(Visibility::Public),
            score: 0.9,
            reranked: false,
            reasoning: None,
        };

        AgenticEntity {
            entity,
            source: crate::types::RetrievalSource::Semantic,
            relevance_justification: "Test entity".to_string(),
        }
    }

    #[test]
    fn test_combined_entity_lookup_finds_accumulated_entities() {
        // This test verifies the fix for the bug where graph traversals couldn't
        // find entities from previous iterations. The fix combines accumulated_entities
        // (from previous iterations) with all_entities (current iteration) before lookup.

        // Create entities from "previous iteration" (accumulated)
        let accumulated_entity = create_test_agentic_entity(
            "entity-a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6",
            "function_from_iteration_1",
        );
        let accumulated_entities = [accumulated_entity];

        // Create entities from "current iteration"
        let current_entity = create_test_agentic_entity(
            "entity-ffffffffffffffffffffffffffffffff",
            "function_from_iteration_2",
        );
        let all_entities = [current_entity];

        // Combine them as done in execute_operations
        let combined_entities: Vec<&AgenticEntity> = accumulated_entities
            .iter()
            .chain(all_entities.iter())
            .collect();

        // Verify we can find entity from accumulated (previous iteration)
        let found_accumulated = combined_entities
            .iter()
            .find(|e| e.entity.entity_id == "entity-a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6");
        assert!(
            found_accumulated.is_some(),
            "Should find entity from accumulated_entities (previous iteration)"
        );
        assert_eq!(
            found_accumulated.unwrap().entity.qualified_name,
            "function_from_iteration_1"
        );

        // Verify we can find entity from current iteration
        let found_current = combined_entities
            .iter()
            .find(|e| e.entity.entity_id == "entity-ffffffffffffffffffffffffffffffff");
        assert!(
            found_current.is_some(),
            "Should find entity from all_entities (current iteration)"
        );
        assert_eq!(
            found_current.unwrap().entity.qualified_name,
            "function_from_iteration_2"
        );

        // Verify non-existent entity is not found
        let not_found = combined_entities
            .iter()
            .find(|e| e.entity.entity_id == "entity-00000000000000000000000000000000");
        assert!(not_found.is_none(), "Should not find non-existent entity");
    }
}
