//! Unified search service combining full-text and semantic search with RRF fusion
//!
//! Uses Reciprocal Rank Fusion (RRF) to merge results from PostgreSQL full-text
//! search and Qdrant semantic search for better recall and precision.

use super::models::{
    BackendClients, EntityResult, SearchConfig, SearchFilters, UnifiedResponseMetadata,
    UnifiedSearchRequest, UnifiedSearchResponse,
};
use super::query_preprocessing::{preprocess_query, PreprocessedQuery};
use super::reranking_helpers::{extract_embedding_content, prepare_documents_for_reranking};
use codesearch_core::error::Result;
use codesearch_core::CodeEntity;
use codesearch_embeddings::EmbeddingTask;
use std::collections::HashMap;
use std::time::Instant;
use tracing::warn;

/// Execute unified search combining full-text and semantic search with RRF fusion
pub async fn search_unified(
    mut request: UnifiedSearchRequest,
    clients: &BackendClients,
    config: &SearchConfig,
) -> Result<UnifiedSearchResponse> {
    let start_time = Instant::now();

    // Validate and clamp input parameters to prevent resource exhaustion
    request.limit = request.limit.clamp(1, 1000);
    if let Some(ref mut ft_limit) = request.fulltext_limit {
        *ft_limit = (*ft_limit).clamp(1, 1000);
    }
    if let Some(ref mut sem_limit) = request.semantic_limit {
        *sem_limit = (*sem_limit).clamp(1, 1000);
    }

    // Preprocess query to extract identifiers and infer entity types
    let preprocessed = preprocess_query(&request.query.text, &config.query_preprocessing);

    // Merge inferred entity types with explicit filters
    let effective_filters = merge_inferred_filters(&request.filters, &preprocessed);

    // Determine if fulltext should be skipped based on intent
    let enable_fulltext = request.enable_fulltext && !preprocessed.skip_fulltext;

    // Compute rerank config early to determine search limits
    // When reranking is enabled, fetch more candidates so the reranker has a meaningful pool
    let rerank_config = request
        .rerank
        .as_ref()
        .map(|r| r.merge_with(&config.reranking))
        .unwrap_or_else(|| config.reranking.clone());

    let semantic_limit_for_search = if rerank_config.enabled && clients.reranker.is_some() {
        rerank_config.candidates
    } else {
        request.semantic_limit.unwrap_or(100)
    };

    // Choose query for fulltext (extracted identifiers or original)
    let fulltext_query = preprocessed
        .fulltext_query
        .as_ref()
        .unwrap_or(&request.query.text);

    let (fulltext_results, semantic_results) = tokio::try_join!(
        // Full-text search via PostgreSQL
        async {
            if enable_fulltext {
                clients
                    .postgres
                    .search_entities_fulltext(
                        request.repository_id,
                        fulltext_query,
                        request.fulltext_limit.unwrap_or(100) as i64,
                        false,
                    )
                    .await
            } else {
                Ok(vec![])
            }
        },
        // Semantic search via Qdrant
        async {
            if request.enable_semantic {
                execute_semantic_search(
                    &request,
                    clients,
                    config,
                    &effective_filters,
                    semantic_limit_for_search,
                )
                .await
            } else {
                Ok(vec![])
            }
        }
    )?;

    let fulltext_count = fulltext_results.len();
    let semantic_count = semantic_results.len();

    let merged_results = apply_rrf_fusion(
        fulltext_results,
        semantic_results,
        request.rrf_k.unwrap_or(60),
    );

    // Skip specificity boost - it was having negative effects on search quality
    let boosted_results = merged_results;

    // rerank_config was computed earlier to determine search limits
    tracing::debug!(
        rerank_enabled = rerank_config.enabled,
        reranker_available = clients.reranker.is_some(),
        candidates = rerank_config.candidates,
        "Reranking config check"
    );

    let (final_results, reranked) = if rerank_config.enabled && clients.reranker.is_some() {
        let rerank_start = Instant::now();
        tracing::info!(candidates = boosted_results.len(), "Starting reranking");
        let result =
            rerank_merged_results(boosted_results, &request, clients, &rerank_config).await?;
        tracing::info!(
            rerank_time_ms = rerank_start.elapsed().as_millis() as u64,
            "Reranking completed"
        );
        result
    } else {
        tracing::info!(
            rerank_enabled = rerank_config.enabled,
            reranker_available = clients.reranker.is_some(),
            "Skipping reranking"
        );
        (boosted_results, false)
    };

    let truncated: Vec<EntityResult> = final_results
        .into_iter()
        .take(request.limit)
        .map(|(entity, score)| {
            let result: Result<EntityResult> = entity.try_into();
            result.map(|mut r| {
                r.score = score;
                r.reranked = reranked;
                r
            })
        })
        .collect::<Result<Vec<_>>>()?;

    let query_time_ms = start_time.elapsed().as_millis() as u64;
    let total_results = truncated.len();

    Ok(UnifiedSearchResponse {
        results: truncated,
        metadata: UnifiedResponseMetadata {
            total_results,
            fulltext_count,
            semantic_count,
            merged_via_rrf: true,
            reranked,
            query_time_ms,
        },
    })
}

/// Use explicit filters only - no automatic entity type inference
///
/// Entity type inference was removed because it filtered out valid results.
/// For example, "What functions call X?" would filter to [Function, Method],
/// which excludes other entity types that might call X.
fn merge_inferred_filters(
    filters: &Option<SearchFilters>,
    _preprocessed: &PreprocessedQuery,
) -> Option<SearchFilters> {
    // Return explicit filters only - do not add inferred entity types
    filters.clone()
}

async fn execute_semantic_search(
    request: &UnifiedSearchRequest,
    clients: &BackendClients,
    config: &SearchConfig,
    effective_filters: &Option<SearchFilters>,
    semantic_limit: usize,
) -> Result<Vec<CodeEntity>> {
    let embeddings = clients
        .embedding_manager
        .embed_for_task(vec![request.query.text.clone()], None, EmbeddingTask::Query)
        .await?;

    let dense_embedding = embeddings.into_iter().next().flatten().ok_or_else(|| {
        codesearch_core::error::Error::config("No embedding returned".to_string())
    })?;

    let stats = clients
        .postgres
        .get_bm25_statistics(request.repository_id)
        .await?;

    let sparse_manager = codesearch_embeddings::create_sparse_manager_from_config(
        &config.sparse_embeddings,
        stats.avgdl,
    )
    .await?;
    let sparse_embeddings = sparse_manager
        .embed_sparse(vec![request.query.text.as_str()])
        .await?;

    let sparse_embedding = sparse_embeddings
        .into_iter()
        .next()
        .flatten()
        .ok_or_else(|| codesearch_core::error::Error::config("No sparse embedding".to_string()))?;

    // Use effective filters (with inferred entity types merged in)
    let filters = super::models::build_storage_filters(effective_filters);

    let candidates = clients
        .qdrant
        .search_similar_hybrid(
            dense_embedding,
            sparse_embedding,
            semantic_limit,
            filters,
            config.hybrid_search.prefetch_multiplier,
        )
        .await?;

    let entity_refs: Vec<_> = candidates
        .iter()
        .map(|(entity_id, _repo_id, _score)| (request.repository_id, entity_id.clone()))
        .collect();

    clients.postgres.get_entities_by_ids(&entity_refs).await
}

/// Apply Reciprocal Rank Fusion (RRF) to merge two result lists
///
/// RRF Score = 1 / (k + rank), where k is a constant (typically 60)
/// This algorithm merges results from multiple ranking systems by combining
/// their reciprocal ranks, giving higher weight to items that rank well in multiple systems.
///
/// Reference: https://plg.uwaterloo.ca/~gvcormac/cormacksigir09-rrf.pdf
pub fn apply_rrf_fusion(
    fulltext_results: Vec<CodeEntity>,
    semantic_results: Vec<CodeEntity>,
    k: usize,
) -> Vec<(CodeEntity, f32)> {
    let mut scores: HashMap<String, (CodeEntity, f32)> = HashMap::new();

    // Log first few fulltext results for debugging
    for (i, entity) in fulltext_results.iter().take(3).enumerate() {
        tracing::debug!(
            "RRF fulltext[{}]: {} ({})",
            i,
            entity.qualified_name,
            entity.entity_id
        );
    }

    for (rank, entity) in fulltext_results.into_iter().enumerate() {
        let rrf_score = 1.0 / ((k + rank + 1) as f32);
        scores.insert(entity.entity_id.clone(), (entity, rrf_score));
    }

    for (rank, entity) in semantic_results.into_iter().enumerate() {
        let rrf_score = 1.0 / ((k + rank + 1) as f32);
        scores
            .entry(entity.entity_id.clone())
            .and_modify(|(_, score)| *score += rrf_score)
            .or_insert((entity, rrf_score));
    }

    let mut results: Vec<_> = scores.into_values().collect();
    results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    results
}

async fn rerank_merged_results(
    merged_results: Vec<(CodeEntity, f32)>,
    request: &UnifiedSearchRequest,
    clients: &BackendClients,
    rerank_config: &codesearch_core::config::RerankingConfig,
) -> Result<(Vec<(CodeEntity, f32)>, bool)> {
    let reranker = match &clients.reranker {
        Some(r) => r,
        None => return Ok((merged_results, false)),
    };

    let candidates_limit = rerank_config.candidates.min(merged_results.len());
    let candidates: Vec<_> = merged_results.into_iter().take(candidates_limit).collect();

    let entity_contents: Vec<(String, String)> = candidates
        .iter()
        .map(|(entity, _score)| (entity.entity_id.clone(), extract_embedding_content(entity)))
        .collect();

    let documents = prepare_documents_for_reranking(&entity_contents);

    match reranker.rerank(&request.query.text, &documents).await {
        Ok(reranked) => {
            let entities_map: HashMap<String, CodeEntity> = candidates
                .into_iter()
                .map(|(entity, _)| (entity.entity_id.clone(), entity))
                .collect();

            // Reranker returns all documents sorted by relevance; caller truncates via request.limit
            let results = reranked
                .into_iter()
                .filter_map(|(entity_id, score)| {
                    entities_map
                        .get(&entity_id)
                        .map(|entity| (entity.clone(), score))
                })
                .collect();

            Ok((results, true))
        }
        Err(e) => {
            warn!("Reranking failed: {e}, returning RRF-merged results");
            Ok((candidates, false))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codesearch_core::entities::{EntityType, Language, SourceLocation, Visibility};
    use std::path::PathBuf;
    use uuid::Uuid;

    fn create_test_entity(id: &str, name: &str) -> CodeEntity {
        let repo_uuid = Uuid::new_v4();
        CodeEntity {
            entity_id: id.to_string(),
            repository_id: repo_uuid.to_string(),
            qualified_name: name.to_string(),
            name: name.to_string(),
            parent_scope: None,
            entity_type: EntityType::Function,
            dependencies: Vec::new(),
            language: Language::Rust,
            file_path: PathBuf::from("test.rs"),
            location: SourceLocation {
                start_line: 1,
                start_column: 0,
                end_line: 10,
                end_column: 0,
            },
            content: Some("fn test() {}".to_string()),
            documentation_summary: None,
            signature: None,
            visibility: Visibility::Public,
            metadata: Default::default(),
        }
    }

    #[test]
    fn test_rrf_fusion_combines_scores() {
        let fulltext = vec![
            create_test_entity("2", "entity_2"),
            create_test_entity("1", "entity_1"),
            create_test_entity("3", "entity_3"),
        ];

        let semantic = vec![
            create_test_entity("2", "entity_2"),
            create_test_entity("1", "entity_1"),
            create_test_entity("4", "entity_4"),
        ];

        let k = 60;
        let results = apply_rrf_fusion(fulltext, semantic, k);

        assert_eq!(results.len(), 4);

        let entity_2 = results.iter().find(|(e, _)| e.entity_id == "2");
        let entity_1 = results.iter().find(|(e, _)| e.entity_id == "1");

        assert!(entity_2.is_some());
        assert!(entity_1.is_some());

        let score_2 = entity_2.map(|(_, s)| *s).unwrap_or(0.0);
        let score_1 = entity_1.map(|(_, s)| *s).unwrap_or(0.0);

        assert!(score_2 > score_1);
    }

    #[test]
    fn test_rrf_fusion_empty_lists() {
        let fulltext = vec![];
        let semantic = vec![];

        let results = apply_rrf_fusion(fulltext, semantic, 60);

        assert_eq!(results.len(), 0);
    }

    #[test]
    fn test_rrf_fusion_one_empty_list() {
        let fulltext = vec![
            create_test_entity("1", "entity_1"),
            create_test_entity("2", "entity_2"),
        ];
        let semantic = vec![];

        let results = apply_rrf_fusion(fulltext, semantic, 60);

        assert_eq!(results.len(), 2);
        assert_eq!(results[0].0.entity_id, "1");
    }

    #[test]
    fn test_rrf_score_calculation() {
        let fulltext = vec![create_test_entity("1", "entity_1")];
        let semantic = vec![create_test_entity("1", "entity_1")];

        let k = 60;
        let results = apply_rrf_fusion(fulltext, semantic, k);

        assert_eq!(results.len(), 1);

        let expected_score = (1.0 / ((k + 1) as f32)) + (1.0 / ((k + 1) as f32));
        let actual_score = results[0].1;

        assert!((actual_score - expected_score).abs() < 0.0001);
    }

    #[test]
    fn test_rrf_different_k_values() {
        let fulltext = vec![
            create_test_entity("1", "entity_1"),
            create_test_entity("2", "entity_2"),
        ];
        let semantic = vec![];

        let results_k10 = apply_rrf_fusion(fulltext.clone(), semantic.clone(), 10);
        let results_k100 = apply_rrf_fusion(fulltext, semantic, 100);

        let score_k10 = results_k10[0].1;
        let score_k100 = results_k100[0].1;

        assert!(score_k10 > score_k100);
    }

    #[test]
    fn test_merge_inferred_filters_ignores_inferred_types() {
        // Regression test: verify that inferred entity types are NOT applied to filters.
        // Bug context: Previously, queries like "What functions call X?" would infer
        // entity_types=[Function, Method] and filter results, excluding valid results
        // like modules or structs that might call X.

        use crate::api::query_preprocessing::{PreprocessedQuery, QueryIntent};

        // Create preprocessed query with inferred entity types
        let preprocessed = PreprocessedQuery {
            original: "What functions call X?".to_string(),
            identifiers: vec!["X".to_string()],
            entity_types: vec![EntityType::Function, EntityType::Method],
            intent: QueryIntent::CallGraph,
            fulltext_query: Some("X".to_string()),
            skip_fulltext: true,
        };

        // Case 1: No explicit filters - should remain None, NOT add inferred types
        let no_filters: Option<SearchFilters> = None;
        let result = merge_inferred_filters(&no_filters, &preprocessed);
        assert!(
            result.is_none(),
            "Should not create filters from inferred entity types"
        );

        // Case 2: Explicit filters with different types - should keep explicit, NOT merge inferred
        let explicit_filters = Some(SearchFilters {
            entity_type: Some(vec![EntityType::Struct]),
            ..Default::default()
        });
        let result = merge_inferred_filters(&explicit_filters, &preprocessed);
        assert!(result.is_some());
        let types = result.unwrap().entity_type.unwrap();
        assert_eq!(types.len(), 1);
        assert_eq!(types[0], EntityType::Struct);
        // Should NOT contain Function or Method from inferred types
    }
}
