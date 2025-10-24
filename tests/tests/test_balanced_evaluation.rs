//! Balanced Evaluation: Semantic vs Keyword Queries
//!
//! This test evaluates hybrid search performance on two distinct query types:
//! 1. Semantic queries: Detailed, descriptive questions (benefit from dense embeddings)
//! 2. Keyword queries: Short, entity-name focused (benefit from BM25)
//!
//! Each query uses a tailored BGE instruction optimized for its query type.
//!
//! Run with: cargo test --test test_balanced_evaluation -- --ignored --nocapture

#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]

use anyhow::{Context, Result};
use codesearch_core::{
    config::{global_config_path, Config},
    CodeEntity,
};
use codesearch_embeddings::{
    create_embedding_manager_from_app_config, create_reranker_provider, Bm25SparseProvider,
    SparseEmbeddingProvider,
};
use codesearch_indexer::entity_processor::extract_embedding_content;
use codesearch_storage::{create_postgres_client, create_storage_client};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::Path, sync::Arc, time::Instant};

#[derive(Debug, Deserialize)]
struct BalancedQuerySet {
    metadata: QuerySetMetadata,
    queries: Vec<BalancedQuery>,
}

#[derive(Debug, Deserialize)]
struct QuerySetMetadata {
    #[allow(dead_code)]
    created: String,
    purpose: String,
    total_queries: usize,
    semantic_queries: usize,
    keyword_queries: usize,
}

#[derive(Debug, Clone, Deserialize)]
struct BalancedQuery {
    id: String,
    query_type: String,
    query: String,
    bge_instruction: String,
    expected_entity_types: Vec<String>,
    expected_concepts: Vec<String>,
}

#[derive(Debug, Serialize)]
struct QueryResult {
    id: String,
    query_type: String,
    query: String,
    bge_instruction: String,

    // Dense results
    dense_latency_ms: u64,
    dense_top5_entities: Vec<EntityInfo>,

    // Dense + Reranking results
    dense_rerank_latency_ms: u64,
    dense_rerank_top5_entities: Vec<EntityInfo>,

    // Hybrid results
    hybrid_latency_ms: u64,
    hybrid_top5_entities: Vec<EntityInfo>,

    // Hybrid + Reranking results
    hybrid_rerank_latency_ms: u64,
    hybrid_rerank_top5_entities: Vec<EntityInfo>,

    // IR Metrics - NDCG@10
    ndcg_dense: Option<f64>,
    ndcg_dense_rerank: Option<f64>,
    ndcg_hybrid: Option<f64>,
    ndcg_hybrid_rerank: Option<f64>,

    // IR Metrics - Precision@10
    precision_dense: Option<f64>,
    precision_dense_rerank: Option<f64>,
    precision_hybrid: Option<f64>,
    precision_hybrid_rerank: Option<f64>,

    // IR Metrics - Recall@10
    recall_dense: Option<f64>,
    recall_dense_rerank: Option<f64>,
    recall_hybrid: Option<f64>,
    recall_hybrid_rerank: Option<f64>,

    // IR Metrics - MRR
    mrr_dense: Option<f64>,
    mrr_dense_rerank: Option<f64>,
    mrr_hybrid: Option<f64>,
    mrr_hybrid_rerank: Option<f64>,

    // Analysis
    entity_type_coverage_dense: f64,
    entity_type_coverage_dense_rerank: f64,
    entity_type_coverage_hybrid: f64,
    entity_type_coverage_hybrid_rerank: f64,
    concept_coverage_dense: f64,
    concept_coverage_dense_rerank: f64,
    concept_coverage_hybrid: f64,
    concept_coverage_hybrid_rerank: f64,
}

#[derive(Debug, Clone, Serialize)]
struct EntityInfo {
    name: String,
    entity_type: String,
    score: f32,
    qualified_name: String,
}

#[derive(Debug, Serialize)]
struct BalancedEvaluationReport {
    semantic_queries: Vec<QueryResult>,
    keyword_queries: Vec<QueryResult>,

    // Aggregate metrics - Latency
    semantic_avg_dense_latency: f64,
    semantic_avg_dense_rerank_latency: f64,
    semantic_avg_hybrid_latency: f64,
    semantic_avg_hybrid_rerank_latency: f64,
    keyword_avg_dense_latency: f64,
    keyword_avg_dense_rerank_latency: f64,
    keyword_avg_hybrid_latency: f64,
    keyword_avg_hybrid_rerank_latency: f64,

    // Aggregate metrics - Entity Coverage
    semantic_avg_entity_coverage_dense: f64,
    semantic_avg_entity_coverage_dense_rerank: f64,
    semantic_avg_entity_coverage_hybrid: f64,
    semantic_avg_entity_coverage_hybrid_rerank: f64,
    keyword_avg_entity_coverage_dense: f64,
    keyword_avg_entity_coverage_dense_rerank: f64,
    keyword_avg_entity_coverage_hybrid: f64,
    keyword_avg_entity_coverage_hybrid_rerank: f64,

    // Aggregate metrics - Concept Coverage
    semantic_avg_concept_coverage_dense: f64,
    semantic_avg_concept_coverage_dense_rerank: f64,
    semantic_avg_concept_coverage_hybrid: f64,
    semantic_avg_concept_coverage_hybrid_rerank: f64,
    keyword_avg_concept_coverage_dense: f64,
    keyword_avg_concept_coverage_dense_rerank: f64,
    keyword_avg_concept_coverage_hybrid: f64,
    keyword_avg_concept_coverage_hybrid_rerank: f64,
}

fn calculate_entity_type_coverage(entities: &[EntityInfo], expected_types: &[String]) -> f64 {
    if expected_types.is_empty() {
        return 1.0;
    }

    let found_types: std::collections::HashSet<_> =
        entities.iter().map(|e| e.entity_type.as_str()).collect();

    let matches = expected_types
        .iter()
        .filter(|t| found_types.contains(t.as_str()))
        .count();

    matches as f64 / expected_types.len() as f64
}

fn calculate_concept_coverage(entities: &[EntityInfo], expected_concepts: &[String]) -> f64 {
    if expected_concepts.is_empty() {
        return 1.0;
    }

    let matches = expected_concepts
        .iter()
        .filter(|concept| {
            let concept_lower = concept.to_lowercase();
            entities.iter().any(|e| {
                e.name.to_lowercase().contains(&concept_lower)
                    || e.qualified_name.to_lowercase().contains(&concept_lower)
            })
        })
        .count();

    matches as f64 / expected_concepts.len() as f64
}

#[derive(Debug, Deserialize)]
struct GroundTruthLabel {
    query: String,
    entity_relevance: HashMap<String, u8>,
}

#[derive(Debug, Deserialize)]
struct GroundTruthData {
    labels: Vec<GroundTruthLabel>,
}

#[derive(Debug, Clone)]
struct SearchResult {
    entity_id: String,
    #[allow(dead_code)]
    entity_name: String,
    #[allow(dead_code)]
    score: f32,
    #[allow(dead_code)]
    rank: usize,
}

fn load_ground_truth(path: &Path) -> Result<GroundTruthData> {
    if !path.exists() {
        return Ok(GroundTruthData { labels: Vec::new() });
    }

    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read ground truth file: {}", path.display()))?;

    let data: GroundTruthData =
        serde_json::from_str(&content).context("Failed to parse ground truth JSON")?;

    Ok(data)
}

fn calculate_ndcg_at_k(
    results: &[SearchResult],
    ground_truth: &HashMap<String, u8>,
    k: usize,
) -> f64 {
    if results.is_empty() {
        return 0.0;
    }

    let dcg: f64 = results
        .iter()
        .take(k)
        .enumerate()
        .map(|(i, result)| {
            let relevance = ground_truth.get(&result.entity_id).copied().unwrap_or(0) as f64;
            relevance / (i as f64 + 2.0).log2()
        })
        .sum();

    let mut ideal_relevances: Vec<u8> = ground_truth.values().copied().collect();
    ideal_relevances.sort_unstable_by(|a, b| b.cmp(a));

    let idcg: f64 = ideal_relevances
        .iter()
        .take(k)
        .enumerate()
        .map(|(i, &relevance)| relevance as f64 / (i as f64 + 2.0).log2())
        .sum();

    if idcg == 0.0 {
        0.0
    } else {
        dcg / idcg
    }
}

fn calculate_precision_at_k(
    results: &[SearchResult],
    ground_truth: &HashMap<String, u8>,
    k: usize,
) -> f64 {
    if results.is_empty() {
        return 0.0;
    }

    let relevant_count = results
        .iter()
        .take(k)
        .filter(|r| ground_truth.get(&r.entity_id).is_some_and(|&rel| rel > 0))
        .count();

    relevant_count as f64 / k as f64
}

fn calculate_recall_at_k(
    results: &[SearchResult],
    ground_truth: &HashMap<String, u8>,
    k: usize,
) -> f64 {
    let total_relevant = ground_truth.values().filter(|&&rel| rel > 0).count();
    if total_relevant == 0 {
        return 0.0;
    }

    let relevant_in_topk = results
        .iter()
        .take(k)
        .filter(|r| ground_truth.get(&r.entity_id).is_some_and(|&rel| rel > 0))
        .count();

    relevant_in_topk as f64 / total_relevant as f64
}

fn calculate_mrr(results: &[SearchResult], ground_truth: &HashMap<String, u8>) -> f64 {
    for (idx, result) in results.iter().enumerate() {
        if ground_truth
            .get(&result.entity_id)
            .is_some_and(|&rel| rel > 0)
        {
            return 1.0 / (idx + 1) as f64;
        }
    }
    0.0
}

fn build_search_results(
    search_results: &[(String, String, f32)],
    entities: &[CodeEntity],
) -> Vec<SearchResult> {
    let entity_map: HashMap<String, &CodeEntity> =
        entities.iter().map(|e| (e.entity_id.clone(), e)).collect();

    search_results
        .iter()
        .enumerate()
        .filter_map(|(rank, (entity_id, _, score))| {
            entity_map.get(entity_id).map(|entity| SearchResult {
                entity_id: entity_id.clone(),
                entity_name: entity.name.clone(),
                score: *score,
                rank: rank + 1,
            })
        })
        .collect()
}

#[tokio::test]
#[ignore]
async fn test_balanced_evaluation() -> Result<()> {
    println!("\n=== Balanced Evaluation: Semantic vs Keyword Queries ===\n");

    // Load config
    let config_path = global_config_path()?;
    let config = Config::from_file(&config_path)?;

    // Load balanced queries
    let queries_path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("data/balanced_evaluation_queries_20.json");
    let content =
        std::fs::read_to_string(&queries_path).context("Failed to read balanced queries")?;
    let query_set: BalancedQuerySet =
        serde_json::from_str(&content).context("Failed to parse balanced queries")?;

    println!("Query Set: {}", query_set.metadata.purpose);
    println!("Total queries: {}", query_set.metadata.total_queries);
    println!("  Semantic: {}", query_set.metadata.semantic_queries);
    println!("  Keyword: {}", query_set.metadata.keyword_queries);
    println!();

    // Load ground truth
    let ground_truth_path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("data/balanced_evaluation_ground_truth_20.json");
    let ground_truth = load_ground_truth(&ground_truth_path)?;
    let ground_truth_map: HashMap<String, HashMap<String, u8>> = ground_truth
        .labels
        .into_iter()
        .map(|label| (label.query, label.entity_relevance))
        .collect();
    println!("Loaded {} ground truth labels\n", ground_truth_map.len());

    // Setup clap repository
    let repo_path = std::path::PathBuf::from("/tmp/clap-eval-v4.5.0");
    let postgres_client = create_postgres_client(&config.storage).await?;
    let (repository_id, collection_name) = postgres_client
        .get_repository_by_path(&repo_path)
        .await?
        .context("clap-eval repository not found")?;

    println!("Repository: {}", repo_path.display());
    println!("Collection: {collection_name}\n");

    // Create clients
    let storage_client = create_storage_client(&config.storage, &collection_name).await?;
    let embedding_manager = create_embedding_manager_from_app_config(&config.embeddings).await?;

    // Setup BM25 for hybrid search
    let bm25_stats = postgres_client.get_bm25_statistics(repository_id).await?;
    let sparse_provider = Bm25SparseProvider::new(bm25_stats.avgdl);

    // Setup reranker (always enabled for evaluation)
    let api_base_url = config
        .reranking
        .api_base_url
        .as_ref()
        .cloned()
        .or_else(|| config.embeddings.api_base_url.clone())
        .unwrap_or_else(|| "http://localhost:8001".to_string());

    let reranker = Arc::new(
        create_reranker_provider(
            config.reranking.model.clone(),
            api_base_url,
            config.reranking.timeout_secs,
        )
        .await
        .context("Failed to create reranker")?,
    );

    println!("Running all 4 configurations: Dense, Dense+Rerank, Hybrid, Hybrid+Rerank");
    println!();

    let mut semantic_results = Vec::new();
    let mut keyword_results = Vec::new();

    for (idx, query) in query_set.queries.iter().enumerate() {
        println!(
            "[{}/{}] Testing: \"{}\"",
            idx + 1,
            query_set.queries.len(),
            if query.query.len() > 60 {
                format!("{}...", &query.query[..60])
            } else {
                query.query.clone()
            }
        );
        println!("  Type: {}", query.query_type);
        let instruction_preview = if query.bge_instruction.len() > 80 {
            format!("{}...", &query.bge_instruction[..80])
        } else {
            query.bge_instruction.clone()
        };
        println!("  Instruction: {instruction_preview}");

        // Run dense search (with embedding generation)
        let dense_start = Instant::now();
        let formatted_query = format!(
            "<instruct>{}\n<query>{}",
            query.bge_instruction, query.query
        );
        let embeddings = embedding_manager
            .embed(vec![formatted_query.clone()])
            .await?;
        let dense_embedding = embeddings
            .into_iter()
            .next()
            .flatten()
            .context("Failed to generate dense embedding")?;
        let dense_results = storage_client
            .search_similar(dense_embedding, 5, None)
            .await?;
        let dense_latency = dense_start.elapsed().as_millis() as u64;

        // Run hybrid search (with BOTH embedding generations)
        let hybrid_start = Instant::now();
        // Generate dense embedding (don't reuse from dense test!)
        let embeddings2 = embedding_manager
            .embed(vec![formatted_query.clone()])
            .await?;
        let hybrid_dense_embedding = embeddings2
            .into_iter()
            .next()
            .flatten()
            .context("Failed to generate dense embedding for hybrid")?;
        // Generate sparse embedding
        let sparse_embeddings = sparse_provider
            .embed_sparse(vec![query.query.as_str()])
            .await?;
        let sparse_embedding = sparse_embeddings
            .into_iter()
            .next()
            .flatten()
            .context("Failed to generate sparse embedding")?;
        // Run hybrid search
        let hybrid_results = storage_client
            .search_similar_hybrid(hybrid_dense_embedding, sparse_embedding, 5, None, 5)
            .await?;
        let hybrid_latency = hybrid_start.elapsed().as_millis() as u64;

        // Run dense + reranking
        let dense_rerank_start = Instant::now();

        // Get top-50 candidates from dense search
        let embeddings3 = embedding_manager
            .embed(vec![formatted_query.clone()])
            .await?;
        let dense_embed3 = embeddings3
            .into_iter()
            .next()
            .flatten()
            .context("Failed to generate dense embedding for reranking")?;
        let candidates = storage_client
            .search_similar(dense_embed3, 50, None)
            .await?;

        // Fetch entities and extract content
        let entity_refs: Vec<_> = candidates
            .iter()
            .map(|(eid, _, _)| (repository_id, eid.clone()))
            .collect();
        let candidate_entities = postgres_client.get_entities_by_ids(&entity_refs).await?;
        let entity_contents: Vec<(String, String)> = candidate_entities
            .iter()
            .map(|e| (e.entity_id.clone(), extract_embedding_content(e)))
            .collect();
        let documents: Vec<(String, &str)> = entity_contents
            .iter()
            .map(|(id, content)| (id.clone(), content.as_str()))
            .collect();

        // Rerank
        let reranked = reranker.rerank(&query.query, &documents, 5).await?;

        let dense_rerank_latency = dense_rerank_start.elapsed().as_millis() as u64;

        // Convert to (entity_id, collection_name, score) format
        let dense_rerank_results: Vec<(String, String, f32)> = reranked
            .iter()
            .map(|(doc_id, score)| (doc_id.clone(), collection_name.clone(), *score))
            .collect();

        // Run hybrid + reranking
        let hybrid_rerank_start = Instant::now();

        // Get top-50 candidates from hybrid search
        let embeddings4 = embedding_manager
            .embed(vec![formatted_query.clone()])
            .await?;
        let hybrid_dense_embed4 = embeddings4
            .into_iter()
            .next()
            .flatten()
            .context("Failed to generate dense embedding for hybrid reranking")?;
        let sparse_embeddings4 = sparse_provider
            .embed_sparse(vec![query.query.as_str()])
            .await?;
        let sparse_embed4 = sparse_embeddings4
            .into_iter()
            .next()
            .flatten()
            .context("Failed to generate sparse embedding for hybrid reranking")?;
        let candidates = storage_client
            .search_similar_hybrid(hybrid_dense_embed4, sparse_embed4, 50, None, 5)
            .await?;

        // Fetch entities and extract content
        let entity_refs: Vec<_> = candidates
            .iter()
            .map(|(eid, _, _)| (repository_id, eid.clone()))
            .collect();
        let candidate_entities = postgres_client.get_entities_by_ids(&entity_refs).await?;
        let entity_contents: Vec<(String, String)> = candidate_entities
            .iter()
            .map(|e| (e.entity_id.clone(), extract_embedding_content(e)))
            .collect();
        let documents: Vec<(String, &str)> = entity_contents
            .iter()
            .map(|(id, content)| (id.clone(), content.as_str()))
            .collect();

        // Rerank
        let reranked = reranker.rerank(&query.query, &documents, 5).await?;

        let hybrid_rerank_latency = hybrid_rerank_start.elapsed().as_millis() as u64;

        // Convert to (entity_id, collection_name, score) format
        let hybrid_rerank_results: Vec<(String, String, f32)> = reranked
            .iter()
            .map(|(doc_id, score)| (doc_id.clone(), collection_name.clone(), *score))
            .collect();

        // Fetch entity details for all configurations
        let all_entity_ids: std::collections::HashSet<_> = dense_results
            .iter()
            .map(|(eid, _, _)| eid.clone())
            .chain(hybrid_results.iter().map(|(eid, _, _)| eid.clone()))
            .chain(dense_rerank_results.iter().map(|(eid, _, _)| eid.clone()))
            .chain(hybrid_rerank_results.iter().map(|(eid, _, _)| eid.clone()))
            .collect();

        let entity_refs: Vec<_> = all_entity_ids
            .iter()
            .map(|eid| (repository_id, eid.clone()))
            .collect();

        let entities = postgres_client.get_entities_by_ids(&entity_refs).await?;
        let entity_map: HashMap<String, &CodeEntity> =
            entities.iter().map(|e| (e.entity_id.clone(), e)).collect();

        // Convert to EntityInfo
        let dense_entity_infos: Vec<_> = dense_results
            .iter()
            .filter_map(|(eid, _, score)| {
                entity_map.get(eid).map(|e| EntityInfo {
                    name: e.name.clone(),
                    entity_type: format!("{:?}", e.entity_type),
                    score: *score,
                    qualified_name: e.qualified_name.clone(),
                })
            })
            .collect();

        let dense_rerank_entity_infos: Vec<_> = dense_rerank_results
            .iter()
            .filter_map(|(eid, _, score)| {
                entity_map.get(eid).map(|e| EntityInfo {
                    name: e.name.clone(),
                    entity_type: format!("{:?}", e.entity_type),
                    score: *score,
                    qualified_name: e.qualified_name.clone(),
                })
            })
            .collect();

        let hybrid_entity_infos: Vec<_> = hybrid_results
            .iter()
            .filter_map(|(eid, _, score)| {
                entity_map.get(eid).map(|e| EntityInfo {
                    name: e.name.clone(),
                    entity_type: format!("{:?}", e.entity_type),
                    score: *score,
                    qualified_name: e.qualified_name.clone(),
                })
            })
            .collect();

        let hybrid_rerank_entity_infos: Vec<_> = hybrid_rerank_results
            .iter()
            .filter_map(|(eid, _, score)| {
                entity_map.get(eid).map(|e| EntityInfo {
                    name: e.name.clone(),
                    entity_type: format!("{:?}", e.entity_type),
                    score: *score,
                    qualified_name: e.qualified_name.clone(),
                })
            })
            .collect();

        // Calculate coverage
        let entity_coverage_dense =
            calculate_entity_type_coverage(&dense_entity_infos, &query.expected_entity_types);
        let entity_coverage_dense_rerank = calculate_entity_type_coverage(
            &dense_rerank_entity_infos,
            &query.expected_entity_types,
        );
        let entity_coverage_hybrid =
            calculate_entity_type_coverage(&hybrid_entity_infos, &query.expected_entity_types);
        let entity_coverage_hybrid_rerank = calculate_entity_type_coverage(
            &hybrid_rerank_entity_infos,
            &query.expected_entity_types,
        );

        let concept_coverage_dense =
            calculate_concept_coverage(&dense_entity_infos, &query.expected_concepts);
        let concept_coverage_dense_rerank =
            calculate_concept_coverage(&dense_rerank_entity_infos, &query.expected_concepts);
        let concept_coverage_hybrid =
            calculate_concept_coverage(&hybrid_entity_infos, &query.expected_concepts);
        let concept_coverage_hybrid_rerank =
            calculate_concept_coverage(&hybrid_rerank_entity_infos, &query.expected_concepts);

        // Calculate IR metrics if ground truth exists
        let query_ground_truth = ground_truth_map.get(&query.query);
        let (ndcg_dense, precision_dense, recall_dense, mrr_dense) =
            if let Some(gt) = query_ground_truth {
                let search_results = build_search_results(&dense_results, &entities);
                (
                    Some(calculate_ndcg_at_k(&search_results, gt, 10)),
                    Some(calculate_precision_at_k(&search_results, gt, 10)),
                    Some(calculate_recall_at_k(&search_results, gt, 10)),
                    Some(calculate_mrr(&search_results, gt)),
                )
            } else {
                (None, None, None, None)
            };

        let (ndcg_dense_rerank, precision_dense_rerank, recall_dense_rerank, mrr_dense_rerank) =
            if let Some(gt) = query_ground_truth {
                let search_results = build_search_results(&dense_rerank_results, &entities);
                (
                    Some(calculate_ndcg_at_k(&search_results, gt, 10)),
                    Some(calculate_precision_at_k(&search_results, gt, 10)),
                    Some(calculate_recall_at_k(&search_results, gt, 10)),
                    Some(calculate_mrr(&search_results, gt)),
                )
            } else {
                (None, None, None, None)
            };

        let (ndcg_hybrid, precision_hybrid, recall_hybrid, mrr_hybrid) =
            if let Some(gt) = query_ground_truth {
                let search_results = build_search_results(&hybrid_results, &entities);
                (
                    Some(calculate_ndcg_at_k(&search_results, gt, 10)),
                    Some(calculate_precision_at_k(&search_results, gt, 10)),
                    Some(calculate_recall_at_k(&search_results, gt, 10)),
                    Some(calculate_mrr(&search_results, gt)),
                )
            } else {
                (None, None, None, None)
            };

        let (ndcg_hybrid_rerank, precision_hybrid_rerank, recall_hybrid_rerank, mrr_hybrid_rerank) =
            if let Some(gt) = query_ground_truth {
                let search_results = build_search_results(&hybrid_rerank_results, &entities);
                (
                    Some(calculate_ndcg_at_k(&search_results, gt, 10)),
                    Some(calculate_precision_at_k(&search_results, gt, 10)),
                    Some(calculate_recall_at_k(&search_results, gt, 10)),
                    Some(calculate_mrr(&search_results, gt)),
                )
            } else {
                (None, None, None, None)
            };

        println!(
            "  Dense:   Latency={}ms, EntityCov={:.0}%, ConceptCov={:.0}%",
            dense_latency,
            entity_coverage_dense * 100.0,
            concept_coverage_dense * 100.0
        );
        println!(
            "  Dense+R: Latency={}ms, EntityCov={:.0}%, ConceptCov={:.0}%",
            dense_rerank_latency,
            entity_coverage_dense_rerank * 100.0,
            concept_coverage_dense_rerank * 100.0
        );
        println!(
            "  Hybrid:  Latency={}ms, EntityCov={:.0}%, ConceptCov={:.0}%",
            hybrid_latency,
            entity_coverage_hybrid * 100.0,
            concept_coverage_hybrid * 100.0
        );
        println!(
            "  Hybrid+R: Latency={}ms, EntityCov={:.0}%, ConceptCov={:.0}%",
            hybrid_rerank_latency,
            entity_coverage_hybrid_rerank * 100.0,
            concept_coverage_hybrid_rerank * 100.0
        );

        if !dense_entity_infos.is_empty() {
            println!(
                "  Dense top: {} ({})",
                dense_entity_infos[0].name, dense_entity_infos[0].entity_type
            );
        }
        if !hybrid_entity_infos.is_empty() {
            println!(
                "  Hybrid top: {} ({})",
                hybrid_entity_infos[0].name, hybrid_entity_infos[0].entity_type
            );
        }
        println!();

        let result = QueryResult {
            id: query.id.clone(),
            query_type: query.query_type.clone(),
            query: query.query.clone(),
            bge_instruction: query.bge_instruction.clone(),
            dense_latency_ms: dense_latency,
            dense_top5_entities: dense_entity_infos,
            dense_rerank_latency_ms: dense_rerank_latency,
            dense_rerank_top5_entities: dense_rerank_entity_infos,
            hybrid_latency_ms: hybrid_latency,
            hybrid_top5_entities: hybrid_entity_infos,
            hybrid_rerank_latency_ms: hybrid_rerank_latency,
            hybrid_rerank_top5_entities: hybrid_rerank_entity_infos,
            ndcg_dense,
            ndcg_dense_rerank,
            ndcg_hybrid,
            ndcg_hybrid_rerank,
            precision_dense,
            precision_dense_rerank,
            precision_hybrid,
            precision_hybrid_rerank,
            recall_dense,
            recall_dense_rerank,
            recall_hybrid,
            recall_hybrid_rerank,
            mrr_dense,
            mrr_dense_rerank,
            mrr_hybrid,
            mrr_hybrid_rerank,
            entity_type_coverage_dense: entity_coverage_dense,
            entity_type_coverage_dense_rerank: entity_coverage_dense_rerank,
            entity_type_coverage_hybrid: entity_coverage_hybrid,
            entity_type_coverage_hybrid_rerank: entity_coverage_hybrid_rerank,
            concept_coverage_dense,
            concept_coverage_dense_rerank,
            concept_coverage_hybrid,
            concept_coverage_hybrid_rerank,
        };

        if query.query_type == "semantic" {
            semantic_results.push(result);
        } else {
            keyword_results.push(result);
        }
    }

    // Calculate aggregate metrics - Latency
    let semantic_avg_dense_latency = semantic_results
        .iter()
        .map(|r| r.dense_latency_ms as f64)
        .sum::<f64>()
        / semantic_results.len() as f64;
    let semantic_avg_dense_rerank_latency = semantic_results
        .iter()
        .map(|r| r.dense_rerank_latency_ms as f64)
        .sum::<f64>()
        / semantic_results.len() as f64;
    let semantic_avg_hybrid_latency = semantic_results
        .iter()
        .map(|r| r.hybrid_latency_ms as f64)
        .sum::<f64>()
        / semantic_results.len() as f64;
    let semantic_avg_hybrid_rerank_latency = semantic_results
        .iter()
        .map(|r| r.hybrid_rerank_latency_ms as f64)
        .sum::<f64>()
        / semantic_results.len() as f64;

    let keyword_avg_dense_latency = keyword_results
        .iter()
        .map(|r| r.dense_latency_ms as f64)
        .sum::<f64>()
        / keyword_results.len() as f64;
    let keyword_avg_dense_rerank_latency = keyword_results
        .iter()
        .map(|r| r.dense_rerank_latency_ms as f64)
        .sum::<f64>()
        / keyword_results.len() as f64;
    let keyword_avg_hybrid_latency = keyword_results
        .iter()
        .map(|r| r.hybrid_latency_ms as f64)
        .sum::<f64>()
        / keyword_results.len() as f64;
    let keyword_avg_hybrid_rerank_latency = keyword_results
        .iter()
        .map(|r| r.hybrid_rerank_latency_ms as f64)
        .sum::<f64>()
        / keyword_results.len() as f64;

    // Calculate aggregate metrics - Entity Coverage
    let semantic_avg_entity_coverage_dense = semantic_results
        .iter()
        .map(|r| r.entity_type_coverage_dense)
        .sum::<f64>()
        / semantic_results.len() as f64;
    let semantic_avg_entity_coverage_dense_rerank = semantic_results
        .iter()
        .map(|r| r.entity_type_coverage_dense_rerank)
        .sum::<f64>()
        / semantic_results.len() as f64;
    let semantic_avg_entity_coverage_hybrid = semantic_results
        .iter()
        .map(|r| r.entity_type_coverage_hybrid)
        .sum::<f64>()
        / semantic_results.len() as f64;
    let semantic_avg_entity_coverage_hybrid_rerank = semantic_results
        .iter()
        .map(|r| r.entity_type_coverage_hybrid_rerank)
        .sum::<f64>()
        / semantic_results.len() as f64;

    let keyword_avg_entity_coverage_dense = keyword_results
        .iter()
        .map(|r| r.entity_type_coverage_dense)
        .sum::<f64>()
        / keyword_results.len() as f64;
    let keyword_avg_entity_coverage_dense_rerank = keyword_results
        .iter()
        .map(|r| r.entity_type_coverage_dense_rerank)
        .sum::<f64>()
        / keyword_results.len() as f64;
    let keyword_avg_entity_coverage_hybrid = keyword_results
        .iter()
        .map(|r| r.entity_type_coverage_hybrid)
        .sum::<f64>()
        / keyword_results.len() as f64;
    let keyword_avg_entity_coverage_hybrid_rerank = keyword_results
        .iter()
        .map(|r| r.entity_type_coverage_hybrid_rerank)
        .sum::<f64>()
        / keyword_results.len() as f64;

    // Calculate aggregate metrics - Concept Coverage
    let semantic_avg_concept_coverage_dense = semantic_results
        .iter()
        .map(|r| r.concept_coverage_dense)
        .sum::<f64>()
        / semantic_results.len() as f64;
    let semantic_avg_concept_coverage_dense_rerank = semantic_results
        .iter()
        .map(|r| r.concept_coverage_dense_rerank)
        .sum::<f64>()
        / semantic_results.len() as f64;
    let semantic_avg_concept_coverage_hybrid = semantic_results
        .iter()
        .map(|r| r.concept_coverage_hybrid)
        .sum::<f64>()
        / semantic_results.len() as f64;
    let semantic_avg_concept_coverage_hybrid_rerank = semantic_results
        .iter()
        .map(|r| r.concept_coverage_hybrid_rerank)
        .sum::<f64>()
        / semantic_results.len() as f64;

    let keyword_avg_concept_coverage_dense = keyword_results
        .iter()
        .map(|r| r.concept_coverage_dense)
        .sum::<f64>()
        / keyword_results.len() as f64;
    let keyword_avg_concept_coverage_dense_rerank = keyword_results
        .iter()
        .map(|r| r.concept_coverage_dense_rerank)
        .sum::<f64>()
        / keyword_results.len() as f64;
    let keyword_avg_concept_coverage_hybrid = keyword_results
        .iter()
        .map(|r| r.concept_coverage_hybrid)
        .sum::<f64>()
        / keyword_results.len() as f64;
    let keyword_avg_concept_coverage_hybrid_rerank = keyword_results
        .iter()
        .map(|r| r.concept_coverage_hybrid_rerank)
        .sum::<f64>()
        / keyword_results.len() as f64;

    let report = BalancedEvaluationReport {
        semantic_queries: semantic_results,
        keyword_queries: keyword_results,
        semantic_avg_dense_latency,
        semantic_avg_dense_rerank_latency,
        semantic_avg_hybrid_latency,
        semantic_avg_hybrid_rerank_latency,
        keyword_avg_dense_latency,
        keyword_avg_dense_rerank_latency,
        keyword_avg_hybrid_latency,
        keyword_avg_hybrid_rerank_latency,
        semantic_avg_entity_coverage_dense,
        semantic_avg_entity_coverage_dense_rerank,
        semantic_avg_entity_coverage_hybrid,
        semantic_avg_entity_coverage_hybrid_rerank,
        keyword_avg_entity_coverage_dense,
        keyword_avg_entity_coverage_dense_rerank,
        keyword_avg_entity_coverage_hybrid,
        keyword_avg_entity_coverage_hybrid_rerank,
        semantic_avg_concept_coverage_dense,
        semantic_avg_concept_coverage_dense_rerank,
        semantic_avg_concept_coverage_hybrid,
        semantic_avg_concept_coverage_hybrid_rerank,
        keyword_avg_concept_coverage_dense,
        keyword_avg_concept_coverage_dense_rerank,
        keyword_avg_concept_coverage_hybrid,
        keyword_avg_concept_coverage_hybrid_rerank,
    };

    // Print report
    println!("\n{}", "=".repeat(80));
    println!("BALANCED EVALUATION REPORT");
    println!("{}", "=".repeat(80));
    println!();

    println!("SEMANTIC QUERIES (n={}):", report.semantic_queries.len());
    println!(
        "  Dense:   Avg Latency={:.0}ms, EntityCov={:.0}%, ConceptCov={:.0}%",
        report.semantic_avg_dense_latency,
        report.semantic_avg_entity_coverage_dense * 100.0,
        report.semantic_avg_concept_coverage_dense * 100.0
    );
    println!(
        "  Dense+R: Avg Latency={:.0}ms, EntityCov={:.0}%, ConceptCov={:.0}%",
        report.semantic_avg_dense_rerank_latency,
        report.semantic_avg_entity_coverage_dense_rerank * 100.0,
        report.semantic_avg_concept_coverage_dense_rerank * 100.0
    );
    println!(
        "  Hybrid:  Avg Latency={:.0}ms, EntityCov={:.0}%, ConceptCov={:.0}%",
        report.semantic_avg_hybrid_latency,
        report.semantic_avg_entity_coverage_hybrid * 100.0,
        report.semantic_avg_concept_coverage_hybrid * 100.0
    );
    println!(
        "  Hybrid+R: Avg Latency={:.0}ms, EntityCov={:.0}%, ConceptCov={:.0}%",
        report.semantic_avg_hybrid_rerank_latency,
        report.semantic_avg_entity_coverage_hybrid_rerank * 100.0,
        report.semantic_avg_concept_coverage_hybrid_rerank * 100.0
    );
    println!();

    println!("KEYWORD QUERIES (n={}):", report.keyword_queries.len());
    println!(
        "  Dense:   Avg Latency={:.0}ms, EntityCov={:.0}%, ConceptCov={:.0}%",
        report.keyword_avg_dense_latency,
        report.keyword_avg_entity_coverage_dense * 100.0,
        report.keyword_avg_concept_coverage_dense * 100.0
    );
    println!(
        "  Dense+R: Avg Latency={:.0}ms, EntityCov={:.0}%, ConceptCov={:.0}%",
        report.keyword_avg_dense_rerank_latency,
        report.keyword_avg_entity_coverage_dense_rerank * 100.0,
        report.keyword_avg_concept_coverage_dense_rerank * 100.0
    );
    println!(
        "  Hybrid:  Avg Latency={:.0}ms, EntityCov={:.0}%, ConceptCov={:.0}%",
        report.keyword_avg_hybrid_latency,
        report.keyword_avg_entity_coverage_hybrid * 100.0,
        report.keyword_avg_concept_coverage_hybrid * 100.0
    );
    println!(
        "  Hybrid+R: Avg Latency={:.0}ms, EntityCov={:.0}%, ConceptCov={:.0}%",
        report.keyword_avg_hybrid_rerank_latency,
        report.keyword_avg_entity_coverage_hybrid_rerank * 100.0,
        report.keyword_avg_concept_coverage_hybrid_rerank * 100.0
    );
    println!();

    println!("ANALYSIS:");

    // Find best configuration for semantic queries
    let semantic_configs = [
        ("DENSE", report.semantic_avg_entity_coverage_dense),
        (
            "DENSE+RERANK",
            report.semantic_avg_entity_coverage_dense_rerank,
        ),
        ("HYBRID", report.semantic_avg_entity_coverage_hybrid),
        (
            "HYBRID+RERANK",
            report.semantic_avg_entity_coverage_hybrid_rerank,
        ),
    ];
    let best_semantic = semantic_configs
        .iter()
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
        .unwrap();
    println!("  Semantic queries benefit from: {}", best_semantic.0);

    // Find best configuration for keyword queries
    let keyword_configs = [
        ("DENSE", report.keyword_avg_entity_coverage_dense),
        (
            "DENSE+RERANK",
            report.keyword_avg_entity_coverage_dense_rerank,
        ),
        ("HYBRID", report.keyword_avg_entity_coverage_hybrid),
        (
            "HYBRID+RERANK",
            report.keyword_avg_entity_coverage_hybrid_rerank,
        ),
    ];
    let best_keyword = keyword_configs
        .iter()
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
        .unwrap();
    println!("  Keyword queries benefit from: {}", best_keyword.0);
    println!();

    // Save report
    let report_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("target/balanced_evaluation_report.json");
    let json = serde_json::to_string_pretty(&report)?;
    std::fs::write(&report_path, json)?;
    println!("Detailed report saved to: {}", report_path.display());

    Ok(())
}
