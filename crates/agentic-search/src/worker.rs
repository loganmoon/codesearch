//! Search worker implementations using Jina cross-encoder reranking

use crate::{
    error::{AgenticSearchError, Result},
    types::{AgenticEntity, RetrievalSource},
};
use codesearch_core::config::RerankingRequestConfig;
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
    /// Semantic search combining dense embeddings + BM25 sparse retrieval
    Semantic,
}

#[derive(Debug, Clone)]
pub struct WorkerQuery {
    pub worker_type: WorkerType,
    pub query: String,
    pub repository_ids: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct WorkerResult {
    pub entities: Vec<AgenticEntity>,
}

/// Execute a single worker: search with optional Jina reranking
pub async fn execute_worker(
    query: WorkerQuery,
    search_api: Arc<dyn SearchApi>,
    rerank_config: Option<RerankingRequestConfig>,
    semantic_candidates: usize,
) -> Result<WorkerResult> {
    info!("Worker {:?} executing: {}", query.worker_type, query.query);

    // Parse repository IDs
    let repository_ids: Vec<Uuid> = query
        .repository_ids
        .iter()
        .filter_map(|s| Uuid::parse_str(s).ok())
        .collect();

    // Execute semantic search with Jina reranking (if enabled)
    let request = SemanticSearchRequest {
        query: QuerySpec {
            text: query.query.clone(),
            instruction: None,
            embedding: None,
        },
        filters: None,
        limit: semantic_candidates,
        prefetch_multiplier: None,
        repository_ids: if repository_ids.is_empty() {
            None
        } else {
            Some(repository_ids.clone())
        },
        rerank: rerank_config,
    };

    let search_results = search_api
        .search_semantic(request)
        .await
        .map_err(|e| AgenticSearchError::SearchApi(e.to_string()))?
        .results;

    if search_results.is_empty() {
        debug!("Worker {:?} found no results", query.worker_type);
        return Ok(WorkerResult { entities: vec![] });
    }

    debug!(
        "Worker {:?} found {} results (reranked by Jina if enabled)",
        query.worker_type,
        search_results.len()
    );

    // Convert search results to AgenticEntity
    let entities: Vec<AgenticEntity> = search_results
        .into_iter()
        .map(|entity| {
            let justification = format!("Semantic match: {:.2}", entity.score);
            AgenticEntity {
                entity,
                source: RetrievalSource::Semantic,
                relevance_justification: justification,
            }
        })
        .collect();

    Ok(WorkerResult { entities })
}

/// Execute multiple workers concurrently with proper cancellation
///
/// CRITICAL: Uses join_all instead of tokio::spawn to ensure cancellation
/// when parent request is dropped (e.g., HTTP timeout). This prevents
/// cost overruns from orphaned LLM requests.
pub async fn execute_workers(
    queries: Vec<WorkerQuery>,
    search_api: Arc<dyn SearchApi>,
    rerank_config: Option<RerankingRequestConfig>,
    semantic_candidates: usize,
) -> Result<Vec<WorkerResult>> {
    info!("Executing {} workers concurrently", queries.len());

    // SAFE: These futures are NOT spawned, so they cancel when parent is dropped
    let worker_futures: Vec<_> = queries
        .into_iter()
        .map(|query| {
            execute_worker(
                query,
                search_api.clone(),
                rerank_config.clone(),
                semantic_candidates,
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
        let result = WorkerResult { entities: vec![] };

        assert_eq!(result.entities.len(), 0);
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
