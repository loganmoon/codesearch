//! Search worker implementations with Stage 1 reranking

use crate::{
    content_selection::{select_content_for_reranking, RerankStage},
    error::{truncate_for_error, AgenticSearchError, Result},
    extract_json, prompts,
    types::{AgenticEntity, RetrievalSource},
};
use codesearch_core::search_models::*;
use codesearch_core::SearchApi;
use futures::future::join_all;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{debug, info, warn};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkerType {
    Semantic,
    Fulltext,
    Unified,
}

#[derive(Debug, Clone)]
pub struct WorkerQuery {
    pub worker_type: WorkerType,
    pub query: String,
    pub repository_ids: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct WorkerResult {
    #[allow(dead_code)]
    pub worker_type: WorkerType,
    pub entities: Vec<AgenticEntity>,
    #[allow(dead_code)]
    pub reranking_cost_usd: f32,
}

/// Execute a single worker: search + Stage 1 reranking
pub async fn execute_worker(
    query: WorkerQuery,
    search_api: Arc<dyn SearchApi>,
    haiku_client: Arc<claudius::Anthropic>,
    haiku_model: claudius::Model,
) -> Result<WorkerResult> {
    info!("Worker {:?} executing: {}", query.worker_type, query.query);

    // Parse repository IDs
    let repository_ids: Vec<Uuid> = query
        .repository_ids
        .iter()
        .filter_map(|s| Uuid::parse_str(s).ok())
        .collect();

    // Execute search based on worker type
    let search_results = match query.worker_type {
        WorkerType::Semantic => {
            let request = SemanticSearchRequest {
                query: QuerySpec {
                    text: query.query.clone(),
                    instruction: None,
                    embedding: None,
                },
                filters: None,
                limit: 15,
                prefetch_multiplier: None,
                repository_ids: if repository_ids.is_empty() {
                    None
                } else {
                    Some(repository_ids.clone())
                },
                rerank: None,
            };

            search_api
                .search_semantic(request)
                .await
                .map_err(|e| AgenticSearchError::SearchApi(e.to_string()))?
                .results
        }
        WorkerType::Fulltext => {
            // Fulltext only supports single repository - use first or return empty
            let repository_id = if repository_ids.is_empty() {
                warn!("Fulltext search requires a repository ID, skipping");
                return Ok(WorkerResult {
                    worker_type: query.worker_type,
                    entities: vec![],
                    reranking_cost_usd: 0.0,
                });
            } else {
                repository_ids[0]
            };

            let request = FulltextSearchRequest {
                repository_id,
                query: query.query.clone(),
                limit: 15,
            };

            search_api
                .search_fulltext(request)
                .await
                .map_err(|e| AgenticSearchError::SearchApi(e.to_string()))?
                .results
        }
        WorkerType::Unified => {
            // Unified only supports single repository - use first or return empty
            let repository_id = if repository_ids.is_empty() {
                warn!("Unified search requires a repository ID, skipping");
                return Ok(WorkerResult {
                    worker_type: query.worker_type,
                    entities: vec![],
                    reranking_cost_usd: 0.0,
                });
            } else {
                repository_ids[0]
            };

            let request = UnifiedSearchRequest {
                repository_id,
                query: QuerySpec {
                    text: query.query.clone(),
                    instruction: None,
                    embedding: None,
                },
                filters: None,
                limit: 15,
                enable_fulltext: true,
                enable_semantic: true,
                fulltext_limit: None,
                semantic_limit: None,
                rrf_k: None,
                rerank: None,
            };

            search_api
                .search_unified(request)
                .await
                .map_err(|e| AgenticSearchError::SearchApi(e.to_string()))?
                .results
        }
    };

    if search_results.is_empty() {
        debug!("Worker {:?} found no results", query.worker_type);
        return Ok(WorkerResult {
            worker_type: query.worker_type,
            entities: vec![],
            reranking_cost_usd: 0.0,
        });
    }

    debug!(
        "Worker {:?} found {} results, starting Stage 1 reranking",
        query.worker_type,
        search_results.len()
    );

    // Stage 1 reranking with stratified content
    let reranked = rerank_worker_results(
        &query.query,
        search_results,
        query.worker_type,
        haiku_client,
        haiku_model,
    )
    .await?;

    Ok(WorkerResult {
        worker_type: query.worker_type,
        entities: reranked,
        reranking_cost_usd: 0.0025,
    })
}

#[derive(Debug, Deserialize)]
struct RerankingResponse {
    entity_id: String,
    score: f32,
    reasoning: String,
}

async fn rerank_worker_results(
    query: &str,
    results: Vec<EntityResult>,
    worker_type: WorkerType,
    haiku_client: Arc<claudius::Anthropic>,
    haiku_model: claudius::Model,
) -> Result<Vec<AgenticEntity>> {
    // Format results with stratified content
    let results_text = results
        .iter()
        .map(|entity| {
            let content = select_content_for_reranking(entity, RerankStage::Worker);
            format!("[{}]\n{}", entity.entity_id, content)
        })
        .collect::<Vec<_>>()
        .join("\n\n---\n\n");

    let prompt = prompts::format_prompt(
        prompts::WORKER_RERANK,
        &[("query", query), ("results", &results_text)],
    );

    // Call Haiku for reranking
    let mut params =
        claudius::MessageCreateParams::simple(claudius::MessageParam::user(prompt), haiku_model);
    params.max_tokens = 4096;
    params.temperature = Some(0.0);

    let response = haiku_client
        .send(params)
        .await
        .map_err(|e| AgenticSearchError::Claudius(format!("Haiku API call failed: {e}")))?;

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

    // Parse JSON array response - extract JSON from potentially chatty LLM response
    let json_str = extract_json(&response_text).ok_or_else(|| {
        AgenticSearchError::Reranking(format!(
            "No valid JSON found in Haiku response: {}",
            truncate_for_error(&response_text)
        ))
    })?;

    let reranked_list: Vec<RerankingResponse> = serde_json::from_str(json_str).map_err(|e| {
        AgenticSearchError::Reranking(format!(
            "Failed to parse Haiku reranking response: {e}. Response: {}",
            truncate_for_error(&response_text)
        ))
    })?;

    // Build HashMap index for O(1) lookup instead of O(n) per entity
    let results_map: std::collections::HashMap<&str, &EntityResult> =
        results.iter().map(|e| (e.entity_id.as_str(), e)).collect();

    // Map back to AgenticEntity with updated scores and reasoning
    let mut reranked_entities = Vec::new();
    let retrieval_source = match worker_type {
        WorkerType::Semantic => RetrievalSource::Semantic,
        WorkerType::Fulltext => RetrievalSource::Fulltext,
        WorkerType::Unified => RetrievalSource::Unified,
    };

    for reranked in reranked_list.iter().take(10) {
        if let Some(&entity_ref) = results_map.get(reranked.entity_id.as_str()) {
            // Clone and update score/reasoning
            let mut entity = entity_ref.clone();
            entity.score = reranked.score;
            entity.reasoning = Some(reranked.reasoning.clone());

            let mut agentic_entity =
                AgenticEntity::from_search_result(entity, retrieval_source.clone());
            agentic_entity.relevance_justification = reranked.reasoning.clone();

            reranked_entities.push(agentic_entity);
        } else {
            warn!(
                "Reranking returned unknown entity_id '{}', skipping (possible LLM hallucination)",
                reranked.entity_id
            );
        }
    }

    debug!(
        "Worker {:?} reranked {} -> {} results",
        worker_type,
        results.len(),
        reranked_entities.len()
    );

    Ok(reranked_entities)
}

/// Execute multiple workers concurrently with proper cancellation
///
/// CRITICAL: Uses join_all instead of tokio::spawn to ensure cancellation
/// when parent request is dropped (e.g., HTTP timeout). This prevents
/// cost overruns from orphaned LLM requests.
pub async fn execute_workers(
    queries: Vec<WorkerQuery>,
    search_api: Arc<dyn SearchApi>,
    haiku_client: Arc<claudius::Anthropic>,
    haiku_model: claudius::Model,
) -> Result<Vec<WorkerResult>> {
    info!("Executing {} workers concurrently", queries.len());

    // SAFE: These futures are NOT spawned, so they cancel when parent is dropped
    let worker_futures: Vec<_> = queries
        .into_iter()
        .map(|query| {
            execute_worker(
                query,
                search_api.clone(),
                haiku_client.clone(),
                haiku_model.clone(),
            )
        })
        .collect();

    // All futures execute concurrently, but cancel together if parent drops
    let results = join_all(worker_futures).await;

    // Filter successes
    let mut successes = Vec::new();
    let mut failures = 0;

    for result in results {
        match result {
            Ok(worker_result) => successes.push(worker_result),
            Err(e) => {
                warn!("Worker failed: {}", e);
                failures += 1;
            }
        }
    }

    if successes.is_empty() {
        return Err(AgenticSearchError::AllWorkersFailed);
    }

    if failures > 0 {
        warn!(
            "Partial worker failure: {} succeeded, {} failed",
            successes.len(),
            failures
        );
    }

    Ok(successes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_worker_type_serialization() {
        let semantic = WorkerType::Semantic;
        let json = serde_json::to_string(&semantic).unwrap();
        assert_eq!(json, "\"semantic\"");

        let deserialized: WorkerType = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, WorkerType::Semantic);
    }

    #[test]
    fn test_worker_result_construction() {
        let result = WorkerResult {
            worker_type: WorkerType::Unified,
            entities: vec![],
            reranking_cost_usd: 0.0025,
        };

        assert_eq!(result.worker_type, WorkerType::Unified);
        assert_eq!(result.entities.len(), 0);
        assert_eq!(result.reranking_cost_usd, 0.0025);
    }

    #[test]
    fn test_reranking_response_parsing_valid() {
        let json = r#"[
            {"entity_id": "e1", "score": 0.95, "reasoning": "Direct implementation"},
            {"entity_id": "e2", "score": 0.82, "reasoning": "Helper function"}
        ]"#;
        let parsed: Vec<RerankingResponse> = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].entity_id, "e1");
        assert_eq!(parsed[0].score, 0.95);
        assert_eq!(parsed[0].reasoning, "Direct implementation");
        assert_eq!(parsed[1].entity_id, "e2");
        assert_eq!(parsed[1].score, 0.82);
    }

    #[test]
    fn test_reranking_response_parsing_empty_array() {
        let json = r#"[]"#;
        let parsed: Vec<RerankingResponse> = serde_json::from_str(json).unwrap();
        assert!(parsed.is_empty());
    }

    #[test]
    fn test_reranking_response_parsing_malformed_missing_fields() {
        // Missing 'reasoning' field
        let json = r#"[{"entity_id": "e1", "score": 0.95}]"#;
        let result: std::result::Result<Vec<RerankingResponse>, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_reranking_response_parsing_wrong_format() {
        // Old format (array of strings) should fail
        let json = r#"["entity_id_1", "entity_id_2"]"#;
        let result: std::result::Result<Vec<RerankingResponse>, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_reranking_response_parsing_single_item() {
        let json = r#"[{"entity_id": "single", "score": 0.99, "reasoning": "Only match"}]"#;
        let parsed: Vec<RerankingResponse> = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].entity_id, "single");
    }

    #[test]
    fn test_all_workers_failed_error() {
        // Verify the AllWorkersFailed error type works correctly
        let err = AgenticSearchError::AllWorkersFailed;
        assert!(err.to_string().contains("All workers failed"));
    }

    #[test]
    fn test_partial_worker_failure_error() {
        let err = AgenticSearchError::PartialWorkerFailure {
            successful: 2,
            total: 3,
        };
        assert!(err.to_string().contains("2/3"));
    }
}
