//! Hybrid Search Evaluation Test Suite
//!
//! This test evaluates hybrid search + reranking by comparing all 4 configurations:
//! 1. Baseline (dense-only, no reranking)
//! 2. Dense + Reranking
//! 3. Hybrid (no reranking)
//! 4. Hybrid + Reranking
//!
//! Prerequisites:
//! - The codesearch repository must be indexed
//! - Shared infrastructure (Postgres, Qdrant) must be running
//! - Config file should exist at ~/.codesearch/config.toml
//!
//! Run with: cargo test --package codesearch-e2e-tests --test test_hybrid_search_evaluation -- --ignored --nocapture

#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]
#![allow(clippy::uninlined_format_args)]
#![allow(dead_code)]

use anyhow::{Context, Result};
use codesearch_core::{
    config::{global_config_path, Config},
    CodeEntity,
};
use codesearch_embeddings::{
    create_embedding_manager_from_app_config, Bm25SparseProvider, EmbeddingManager,
    SparseEmbeddingProvider,
};
use codesearch_indexer::entity_processor::extract_embedding_content;
use codesearch_storage::{
    create_postgres_client, create_storage_client, PostgresClientTrait, StorageClient,
};
use serde::{Deserialize, Serialize};
use sqlx::{Executor, Row};
use std::{
    collections::HashMap,
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};
use uuid::Uuid;

/// Configuration type for 4-way comparison
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
enum ConfigurationType {
    Baseline,
    DenseReranking,
    Hybrid,
    HybridReranking,
}

impl ConfigurationType {
    fn name(&self) -> &str {
        match self {
            Self::Baseline => "baseline",
            Self::DenseReranking => "dense+reranking",
            Self::Hybrid => "hybrid",
            Self::HybridReranking => "hybrid+reranking",
        }
    }
}

/// Query type classification (from test_reranking_evaluation.rs)
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
enum QueryType {
    ExactName,
    Documentation,
}

/// Hybrid query type classification for diverse query generation
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
enum HybridQueryType {
    KeywordHeavy,
    Semantic,
    Mixed,
}

impl HybridQueryType {
    fn description(&self) -> &str {
        match self {
            Self::KeywordHeavy => "keyword-heavy (BM25-friendly)",
            Self::Semantic => "semantic (dense-embedding-friendly)",
            Self::Mixed => "mixed (benefits from hybrid)",
        }
    }
}

/// Per-configuration aggregate metrics
#[derive(Debug, Serialize)]
struct ConfigurationResults {
    config_type: ConfigurationType,
    name: String,
    avg_ndcg: f64,
    avg_precision_at_10: f64,
    avg_recall_at_10: f64,
    avg_mrr: f64,
    avg_latency_ms: f64,
    avg_dense_embedding_ms: f64,
    avg_sparse_embedding_ms: f64,
    avg_vector_search_ms: f64,
    avg_reranking_ms: f64,
    avg_sparse_vector_size: f64,
    avg_sparse_vector_density: f64,
    queries_evaluated: usize,
}

/// Serializable search result for reports
#[derive(Debug, Clone, Serialize)]
struct SerializableSearchResult {
    entity_id: String,
    entity_name: String,
    entity_type: String,
    score: f32,
}

/// Per-query results for all 4 configurations
#[derive(Debug, Serialize)]
struct QueryComparison4Way {
    query: String,
    query_type: QueryType,
    has_ground_truth: bool,

    // Baseline metrics
    baseline_ndcg: f64,
    baseline_precision: f64,
    baseline_recall: f64,
    baseline_mrr: f64,
    baseline_latency_ms: u64,
    baseline_metrics: SearchMetrics,
    baseline_results: Vec<SerializableSearchResult>,

    // Dense + reranking metrics
    dense_reranking_ndcg: f64,
    dense_reranking_precision: f64,
    dense_reranking_recall: f64,
    dense_reranking_mrr: f64,
    dense_reranking_latency_ms: u64,
    dense_reranking_metrics: SearchMetrics,
    dense_reranking_results: Vec<SerializableSearchResult>,

    // Hybrid metrics
    hybrid_ndcg: f64,
    hybrid_precision: f64,
    hybrid_recall: f64,
    hybrid_mrr: f64,
    hybrid_latency_ms: u64,
    hybrid_metrics: SearchMetrics,
    hybrid_results: Vec<SerializableSearchResult>,

    // Hybrid + reranking metrics
    hybrid_reranking_ndcg: f64,
    hybrid_reranking_precision: f64,
    hybrid_reranking_recall: f64,
    hybrid_reranking_mrr: f64,
    hybrid_reranking_latency_ms: u64,
    hybrid_reranking_metrics: SearchMetrics,
    hybrid_reranking_results: Vec<SerializableSearchResult>,

    best_config: String,
    best_ndcg: f64,
}

/// Pairwise comparison metrics
#[derive(Debug, Serialize)]
struct ComparisonMetrics {
    from_config: String,
    to_config: String,
    avg_ndcg_improvement: f64,
    queries_improved: usize,
    queries_degraded: usize,
    queries_unchanged: usize,
    avg_latency_increase_ms: f64,
}

/// Overall evaluation report
#[derive(Debug, Serialize)]
struct HybridSearchEvaluationReport {
    total_queries: usize,
    queries_with_ground_truth: usize,
    prefetch_multiplier: usize,
    configurations: Vec<ConfigurationResults>,
    comparisons: Vec<ComparisonMetrics>,
    best_ndcg_config: String,
    best_latency_config: String,
    query_comparisons: Vec<QueryComparison4Way>,
}

/// Prefetch sensitivity analysis report
#[derive(Debug, Serialize)]
struct PrefetchSensitivityReport {
    prefetch_multipliers_tested: Vec<usize>,
    sensitivity_results: Vec<PrefetchSensitivityResult>,
    optimal_for_ndcg: OptimalPrefetchConfig,
    optimal_for_latency: OptimalPrefetchConfig,
    optimal_balanced: OptimalPrefetchConfig,
}

/// Per-prefetch-multiplier results
#[derive(Debug, Serialize)]
struct PrefetchSensitivityResult {
    prefetch_multiplier: usize,

    // Aggregate metrics for hybrid config only
    hybrid_avg_ndcg: f64,
    hybrid_avg_precision: f64,
    hybrid_avg_recall: f64,
    hybrid_avg_latency_ms: f64,

    // Aggregate metrics for hybrid+reranking config only
    hybrid_rerank_avg_ndcg: f64,
    hybrid_rerank_avg_precision: f64,
    hybrid_rerank_avg_recall: f64,
    hybrid_rerank_avg_latency_ms: f64,

    // Efficiency metrics
    prefetch_efficiency: f64, // % of prefetched candidates in final top-10
}

/// Optimal prefetch configuration
#[derive(Debug, Serialize)]
struct OptimalPrefetchConfig {
    prefetch_multiplier: usize,
    ndcg: f64,
    latency_ms: f64,
    efficiency: f64,
}

/// Ground truth relevance label (from test_reranking_evaluation.rs)
#[derive(Debug, Clone, Serialize, Deserialize)]
struct GroundTruthLabel {
    query: String,
    query_type: QueryType,
    entity_relevance: HashMap<String, u8>,
}

/// Ground truth data (from test_reranking_evaluation.rs)
#[derive(Debug, Serialize, Deserialize)]
struct GroundTruthData {
    labels: Vec<GroundTruthLabel>,
}

/// Test query
#[derive(Debug, Clone)]
struct TestQuery {
    query_text: String,
    query_type: QueryType,
}

/// Hybrid test query with HybridQueryType
#[derive(Debug, Clone)]
struct HybridTestQuery {
    query_text: String,
    query_type: HybridQueryType,
}

/// Labeling data structures for hybrid queries
#[derive(Debug, Serialize)]
struct HybridLabelingData {
    query: String,
    query_type: HybridQueryType,
    dense_results: Vec<LabelingResult>,
    hybrid_results: Vec<LabelingResult>,
}

#[derive(Debug, Serialize)]
struct LabelingResult {
    entity_id: String,
    entity_name: String,
    entity_type: String,
    score: f32,
}

/// Search result
#[derive(Debug, Clone)]
struct SearchResult {
    entity_id: String,
    entity_name: String,
    score: f32,
    rank: usize,
}

/// Detailed metrics collected during search execution
#[derive(Debug, Clone, Serialize)]
struct SearchMetrics {
    sparse_vector_size: usize,
    sparse_vector_density: f64,
    dense_embedding_time_ms: u64,
    sparse_embedding_time_ms: u64,
    vector_search_time_ms: u64,
    reranking_time_ms: u64,
    candidates_retrieved: usize,
}

impl Default for SearchMetrics {
    fn default() -> Self {
        Self {
            sparse_vector_size: 0,
            sparse_vector_density: 0.0,
            dense_embedding_time_ms: 0,
            sparse_embedding_time_ms: 0,
            vector_search_time_ms: 0,
            reranking_time_ms: 0,
            candidates_retrieved: 0,
        }
    }
}

/// Search executor (reuses from test_reranking_evaluation.rs)
struct SearchExecutor {
    storage_client: Arc<dyn StorageClient>,
    postgres_client: Arc<dyn PostgresClientTrait>,
    embedding_manager: Arc<EmbeddingManager>,
    reranker: Option<Arc<dyn codesearch_embeddings::RerankerProvider>>,
    repository_id: Uuid,
    bge_instruction: String,
    sparse_provider: Option<Bm25SparseProvider>,
    prefetch_multiplier: usize,
}

impl SearchExecutor {
    async fn new(
        config: &Config,
        repository_id: Uuid,
        collection_name: &str,
        enable_reranking: bool,
        enable_hybrid: bool,
        prefetch_multiplier: usize,
    ) -> Result<Self> {
        let storage_client = create_storage_client(&config.storage, collection_name)
            .await
            .context("Failed to create storage client")?;

        let postgres_client = create_postgres_client(&config.storage)
            .await
            .context("Failed to create postgres client")?;

        let embedding_manager = create_embedding_manager_from_app_config(&config.embeddings)
            .await
            .context("Failed to create embedding manager")?;

        let reranker = if enable_reranking && config.reranking.enabled {
            let api_base_url = config
                .reranking
                .api_base_url
                .as_ref()
                .cloned()
                .or_else(|| config.embeddings.api_base_url.clone())
                .unwrap_or_else(|| "http://localhost:8001".to_string());

            Some(
                codesearch_embeddings::create_reranker_provider(
                    config.reranking.model.clone(),
                    api_base_url,
                    config.reranking.timeout_secs,
                )
                .await
                .context("Failed to create reranker")?,
            )
        } else {
            None
        };

        let bge_instruction = config.embeddings.default_bge_instruction.clone();

        let sparse_provider = if enable_hybrid {
            let bm25_stats = postgres_client
                .get_bm25_statistics(repository_id)
                .await
                .context("Failed to fetch BM25 statistics")?;

            Some(Bm25SparseProvider::new(bm25_stats.avgdl))
        } else {
            None
        };

        Ok(Self {
            storage_client,
            postgres_client,
            embedding_manager,
            reranker,
            repository_id,
            bge_instruction,
            sparse_provider,
            prefetch_multiplier,
        })
    }

    async fn search(
        &self,
        query: &str,
        limit: usize,
        use_reranking: bool,
    ) -> Result<(Vec<SearchResult>, Duration, SearchMetrics)> {
        let start = Instant::now();
        let mut metrics = SearchMetrics::default();

        let dense_start = Instant::now();
        let formatted_query = format!("<instruct>{}\n<query>{}", self.bge_instruction, query);

        let embeddings = self
            .embedding_manager
            .embed(vec![formatted_query])
            .await
            .context("Failed to generate embedding")?;

        let query_embedding = embeddings
            .into_iter()
            .next()
            .flatten()
            .context("Failed to generate embedding")?;
        metrics.dense_embedding_time_ms = dense_start.elapsed().as_millis() as u64;

        let candidates_limit = if use_reranking { 50 } else { limit };

        let search_start = Instant::now();
        let search_results = if let Some(ref sparse_provider) = self.sparse_provider {
            let sparse_start = Instant::now();
            let sparse_embeddings = sparse_provider
                .embed_sparse(vec![query])
                .await
                .context("Failed to generate sparse embedding")?;

            let sparse_embedding = sparse_embeddings
                .into_iter()
                .next()
                .flatten()
                .context("Failed to generate sparse embedding")?;

            metrics.sparse_embedding_time_ms = sparse_start.elapsed().as_millis() as u64;
            metrics.sparse_vector_size = sparse_embedding.len();
            metrics.sparse_vector_density = (sparse_embedding.len() as f64 / 100_000.0) * 100.0;

            self.storage_client
                .search_similar_hybrid(
                    query_embedding,
                    sparse_embedding,
                    candidates_limit,
                    None,
                    self.prefetch_multiplier,
                )
                .await
                .context("Hybrid search failed")?
        } else {
            self.storage_client
                .search_similar(query_embedding, candidates_limit, None)
                .await
                .context("Search failed")?
        };
        metrics.vector_search_time_ms = search_start.elapsed().as_millis() as u64;
        metrics.candidates_retrieved = search_results.len();

        let entity_refs: Vec<_> = search_results
            .iter()
            .map(|(eid, _, _)| (self.repository_id, eid.to_string()))
            .collect();

        let entities = self
            .postgres_client
            .get_entities_by_ids(&entity_refs)
            .await
            .context("Failed to fetch entities")?;

        let final_results = if use_reranking {
            if let Some(ref reranker) = self.reranker {
                let rerank_start = Instant::now();

                let entity_contents: Vec<(String, String)> = entities
                    .iter()
                    .map(|entity| (entity.entity_id.clone(), extract_embedding_content(entity)))
                    .collect();

                let documents: Vec<(String, &str)> = entity_contents
                    .iter()
                    .map(|(id, content)| (id.clone(), content.as_str()))
                    .collect();

                let reranked = reranker
                    .rerank(query, &documents, limit)
                    .await
                    .context("Reranking failed")?;

                metrics.reranking_time_ms = rerank_start.elapsed().as_millis() as u64;

                let entity_map: HashMap<String, &CodeEntity> =
                    entities.iter().map(|e| (e.entity_id.clone(), e)).collect();

                reranked
                    .into_iter()
                    .enumerate()
                    .filter_map(|(rank, (entity_id, score))| {
                        entity_map.get(&entity_id).map(|entity| SearchResult {
                            entity_id: entity_id.clone(),
                            entity_name: entity.name.clone(),
                            score,
                            rank: rank + 1,
                        })
                    })
                    .collect()
            } else {
                self.build_search_results(&search_results, &entities, limit)
            }
        } else {
            self.build_search_results(&search_results, &entities, limit)
        };

        let elapsed = start.elapsed();
        Ok((final_results, elapsed, metrics))
    }

    fn build_search_results(
        &self,
        search_results: &[(String, String, f32)],
        entities: &[CodeEntity],
        limit: usize,
    ) -> Vec<SearchResult> {
        let entity_map: HashMap<String, &CodeEntity> =
            entities.iter().map(|e| (e.entity_id.clone(), e)).collect();

        search_results
            .iter()
            .take(limit)
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
}

/// Hybrid query generator for diverse query types
struct HybridQueryGenerator {
    postgres_client: Arc<dyn PostgresClientTrait>,
    repository_id: Uuid,
}

impl HybridQueryGenerator {
    fn new(postgres_client: Arc<dyn PostgresClientTrait>, repository_id: Uuid) -> Self {
        Self {
            postgres_client,
            repository_id,
        }
    }

    async fn generate_diverse_queries(
        &self,
        keyword_count: usize,
        semantic_count: usize,
        mixed_count: usize,
    ) -> Result<Vec<HybridTestQuery>> {
        let all_entities = self.fetch_all_entities().await?;
        let mut queries = Vec::new();

        println!("Generating queries from {} entities...", all_entities.len());

        // Generate keyword-heavy queries (exact function names, specific terms)
        for entity in all_entities.iter().take(keyword_count) {
            queries.push(HybridTestQuery {
                query_text: entity.name.clone(),
                query_type: HybridQueryType::KeywordHeavy,
            });
        }

        // Generate semantic queries (natural language descriptions from docs)
        let mut semantic_generated = 0;
        for entity in all_entities.iter() {
            if semantic_generated >= semantic_count {
                break;
            }
            if let Some(doc) = &entity.documentation_summary {
                if let Some(semantic_query) = self.extract_semantic_query(doc) {
                    queries.push(HybridTestQuery {
                        query_text: semantic_query,
                        query_type: HybridQueryType::Semantic,
                    });
                    semantic_generated += 1;
                }
            }
        }

        // Generate mixed queries (entity name + key terms from documentation)
        let mut mixed_generated = 0;
        for entity in all_entities.iter() {
            if mixed_generated >= mixed_count {
                break;
            }
            if let Some(doc) = &entity.documentation_summary {
                let key_terms = self.extract_key_terms(doc);
                if !key_terms.is_empty() {
                    let mixed_query = format!("{} {}", entity.name, key_terms);
                    queries.push(HybridTestQuery {
                        query_text: mixed_query,
                        query_type: HybridQueryType::Mixed,
                    });
                    mixed_generated += 1;
                }
            }
        }

        println!(
            "Generated {} keyword-heavy, {} semantic, {} mixed queries (total: {})",
            queries
                .iter()
                .filter(|q| matches!(q.query_type, HybridQueryType::KeywordHeavy))
                .count(),
            queries
                .iter()
                .filter(|q| matches!(q.query_type, HybridQueryType::Semantic))
                .count(),
            queries
                .iter()
                .filter(|q| matches!(q.query_type, HybridQueryType::Mixed))
                .count(),
            queries.len()
        );

        Ok(queries)
    }

    fn extract_semantic_query(&self, doc: &str) -> Option<String> {
        // Extract first sentence, truncate at 100 chars
        let first_sentence = doc.split('.').next()?;
        if first_sentence.len() > 100 {
            if let Some(last_space) = first_sentence[..100].rfind(' ') {
                Some(first_sentence[..last_space].to_string())
            } else {
                Some(first_sentence[..100].to_string())
            }
        } else if first_sentence.len() > 10 {
            Some(first_sentence.to_string())
        } else {
            None
        }
    }

    fn extract_key_terms(&self, doc: &str) -> String {
        // Extract 3-5 key words from documentation
        doc.split_whitespace()
            .filter(|word| word.len() > 3)
            .take(4)
            .collect::<Vec<_>>()
            .join(" ")
    }

    async fn fetch_all_entities(&self) -> Result<Vec<CodeEntity>> {
        let pool = self.postgres_client.get_pool();

        #[derive(sqlx::FromRow)]
        struct EntityRow {
            entity_id: String,
            entity_data: sqlx::types::JsonValue,
        }

        let rows = sqlx::query_as::<_, EntityRow>(
            "SELECT entity_id, entity_data
             FROM entity_metadata
             WHERE repository_id = $1 AND deleted_at IS NULL
             LIMIT 500",
        )
        .bind(self.repository_id)
        .fetch_all(pool)
        .await
        .context("Failed to fetch entity rows")?;

        let entities: Result<Vec<CodeEntity>> = rows
            .into_iter()
            .map(|row| {
                serde_json::from_value(row.entity_data)
                    .context(format!("Failed to deserialize entity {}", row.entity_id))
            })
            .collect();

        entities
    }
}

/// Calculate NDCG@k (from test_reranking_evaluation.rs)
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

/// Calculate Precision@k
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

/// Calculate Recall@k
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

/// Calculate Mean Reciprocal Rank
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

/// Load ground truth from file
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

/// Helper functions for calculating aggregate metrics
fn calculate_avg_ndcg<F>(comparisons: &[&QueryComparison4Way], extractor: F) -> f64
where
    F: Fn(&QueryComparison4Way) -> f64,
{
    if comparisons.is_empty() {
        return 0.0;
    }
    comparisons.iter().map(|c| extractor(c)).sum::<f64>() / comparisons.len() as f64
}

fn calculate_avg_latency<F>(comparisons: &[QueryComparison4Way], extractor: F) -> f64
where
    F: Fn(&QueryComparison4Way) -> u64,
{
    if comparisons.is_empty() {
        return 0.0;
    }
    comparisons.iter().map(|c| extractor(c) as f64).sum::<f64>() / comparisons.len() as f64
}

/// Calculate average of a u64 field from SearchMetrics
fn calculate_avg_metrics_u64<FMetrics, FField>(
    comparisons: &[QueryComparison4Way],
    metrics_extractor: FMetrics,
    field_extractor: FField,
) -> f64
where
    FMetrics: Fn(&QueryComparison4Way) -> &SearchMetrics,
    FField: Fn(&SearchMetrics) -> u64,
{
    if comparisons.is_empty() {
        return 0.0;
    }
    comparisons
        .iter()
        .map(|c| field_extractor(metrics_extractor(c)) as f64)
        .sum::<f64>()
        / comparisons.len() as f64
}

/// Calculate average of a f64 field from SearchMetrics
fn calculate_avg_metrics_f64<FMetrics, FField>(
    comparisons: &[QueryComparison4Way],
    metrics_extractor: FMetrics,
    field_extractor: FField,
) -> f64
where
    FMetrics: Fn(&QueryComparison4Way) -> &SearchMetrics,
    FField: Fn(&SearchMetrics) -> f64,
{
    if comparisons.is_empty() {
        return 0.0;
    }
    comparisons
        .iter()
        .map(|c| field_extractor(metrics_extractor(c)))
        .sum::<f64>()
        / comparisons.len() as f64
}

/// Calculate average of a usize field from SearchMetrics
fn calculate_avg_metrics_usize<FMetrics, FField>(
    comparisons: &[QueryComparison4Way],
    metrics_extractor: FMetrics,
    field_extractor: FField,
) -> f64
where
    FMetrics: Fn(&QueryComparison4Way) -> &SearchMetrics,
    FField: Fn(&SearchMetrics) -> usize,
{
    if comparisons.is_empty() {
        return 0.0;
    }
    comparisons
        .iter()
        .map(|c| field_extractor(metrics_extractor(c)) as f64)
        .sum::<f64>()
        / comparisons.len() as f64
}

#[allow(clippy::too_many_arguments)]
fn calculate_comparison_metrics<F1, F2, F3, F4>(
    from_name: &str,
    to_name: &str,
    comparisons_with_gt: &[&QueryComparison4Way],
    from_ndcg_extractor: F1,
    to_ndcg_extractor: F2,
    all_comparisons: &[QueryComparison4Way],
    from_latency_extractor: F3,
    to_latency_extractor: F4,
) -> ComparisonMetrics
where
    F1: Fn(&QueryComparison4Way) -> f64,
    F2: Fn(&QueryComparison4Way) -> f64,
    F3: Fn(&QueryComparison4Way) -> u64,
    F4: Fn(&QueryComparison4Way) -> u64,
{
    let avg_from_ndcg = calculate_avg_ndcg(comparisons_with_gt, &from_ndcg_extractor);
    let avg_to_ndcg = calculate_avg_ndcg(comparisons_with_gt, &to_ndcg_extractor);
    let avg_ndcg_improvement = avg_to_ndcg - avg_from_ndcg;

    let queries_improved = comparisons_with_gt
        .iter()
        .filter(|c| to_ndcg_extractor(c) - from_ndcg_extractor(c) > 0.001)
        .count();
    let queries_degraded = comparisons_with_gt
        .iter()
        .filter(|c| to_ndcg_extractor(c) - from_ndcg_extractor(c) < -0.001)
        .count();
    let queries_unchanged = comparisons_with_gt
        .iter()
        .filter(|c| (to_ndcg_extractor(c) - from_ndcg_extractor(c)).abs() <= 0.001)
        .count();

    let avg_from_latency = calculate_avg_latency(all_comparisons, &from_latency_extractor);
    let avg_to_latency = calculate_avg_latency(all_comparisons, &to_latency_extractor);
    let avg_latency_increase_ms = avg_to_latency - avg_from_latency;

    ComparisonMetrics {
        from_config: from_name.to_string(),
        to_config: to_name.to_string(),
        avg_ndcg_improvement,
        queries_improved,
        queries_degraded,
        queries_unchanged,
        avg_latency_increase_ms,
    }
}

/// Main evaluation function
async fn evaluate_hybrid_search(
    config: &Config,
    repository_id: Uuid,
    collection_name: &str,
    prefetch_multiplier: usize,
) -> Result<HybridSearchEvaluationReport> {
    println!(
        "\n=== Hybrid Search Evaluation (prefetch_multiplier={}) ===\n",
        prefetch_multiplier
    );

    // Load evaluation queries
    let queries = load_evaluation_queries()?;
    println!("Loaded {} evaluation queries\n", queries.len());

    // Load ground truth labels
    let ground_truth_path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("data/ground_truth_evaluation.json");
    let ground_truth_data = load_ground_truth(&ground_truth_path)?;

    // Convert to HashMap for efficient lookup
    let ground_truth_map: HashMap<String, HashMap<String, u8>> = ground_truth_data
        .labels
        .into_iter()
        .map(|label| (label.query, label.entity_relevance))
        .collect();

    if ground_truth_map.is_empty() {
        println!("NOTE: Running without ground truth labels - metrics will be 0.0");
        println!("Use this run to collect results for manual labeling\n");
    } else {
        println!(
            "Loaded {} queries with ground truth labels\n",
            ground_truth_map.len()
        );
    }

    println!("Creating search executors...");
    let baseline_executor = SearchExecutor::new(
        config,
        repository_id,
        collection_name,
        false,
        false,
        prefetch_multiplier,
    )
    .await?;

    let dense_reranking_executor = SearchExecutor::new(
        config,
        repository_id,
        collection_name,
        true,
        false,
        prefetch_multiplier,
    )
    .await?;

    let hybrid_executor = SearchExecutor::new(
        config,
        repository_id,
        collection_name,
        false,
        true,
        prefetch_multiplier,
    )
    .await?;

    let hybrid_reranking_executor = SearchExecutor::new(
        config,
        repository_id,
        collection_name,
        true,
        true,
        prefetch_multiplier,
    )
    .await?;

    println!("Running searches...\n");

    let mut comparisons = Vec::new();
    let limit = 10;

    for (idx, query) in queries.iter().enumerate() {
        println!(
            "[{}/{}] Testing: \"{}\"",
            idx + 1,
            queries.len(),
            query.query_text
        );

        let (baseline_results, baseline_latency, baseline_metrics) = baseline_executor
            .search(&query.query_text, limit, false)
            .await?;
        let (dense_rerank_results, dense_rerank_latency, dense_rerank_metrics) =
            dense_reranking_executor
                .search(&query.query_text, limit, true)
                .await?;
        let (hybrid_results, hybrid_latency, hybrid_metrics) = hybrid_executor
            .search(&query.query_text, limit, false)
            .await?;
        let (hybrid_rerank_results, hybrid_rerank_latency, hybrid_rerank_metrics) =
            hybrid_reranking_executor
                .search(&query.query_text, limit, true)
                .await?;

        let has_ground_truth = ground_truth_map.contains_key(&query.query_text);

        // Calculate all quality metrics for each configuration
        let (
            baseline_ndcg,
            baseline_precision,
            baseline_recall,
            baseline_mrr,
            dense_rerank_ndcg,
            dense_rerank_precision,
            dense_rerank_recall,
            dense_rerank_mrr,
            hybrid_ndcg,
            hybrid_precision,
            hybrid_recall,
            hybrid_mrr,
            hybrid_rerank_ndcg,
            hybrid_rerank_precision,
            hybrid_rerank_recall,
            hybrid_rerank_mrr,
        ) = if let Some(relevance_map) = ground_truth_map.get(&query.query_text) {
            (
                calculate_ndcg_at_k(&baseline_results, relevance_map, limit),
                calculate_precision_at_k(&baseline_results, relevance_map, limit),
                calculate_recall_at_k(&baseline_results, relevance_map, limit),
                calculate_mrr(&baseline_results, relevance_map),
                calculate_ndcg_at_k(&dense_rerank_results, relevance_map, limit),
                calculate_precision_at_k(&dense_rerank_results, relevance_map, limit),
                calculate_recall_at_k(&dense_rerank_results, relevance_map, limit),
                calculate_mrr(&dense_rerank_results, relevance_map),
                calculate_ndcg_at_k(&hybrid_results, relevance_map, limit),
                calculate_precision_at_k(&hybrid_results, relevance_map, limit),
                calculate_recall_at_k(&hybrid_results, relevance_map, limit),
                calculate_mrr(&hybrid_results, relevance_map),
                calculate_ndcg_at_k(&hybrid_rerank_results, relevance_map, limit),
                calculate_precision_at_k(&hybrid_rerank_results, relevance_map, limit),
                calculate_recall_at_k(&hybrid_rerank_results, relevance_map, limit),
                calculate_mrr(&hybrid_rerank_results, relevance_map),
            )
        } else {
            (
                0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
            )
        };

        let ndcgs = [
            (ConfigurationType::Baseline, baseline_ndcg),
            (ConfigurationType::DenseReranking, dense_rerank_ndcg),
            (ConfigurationType::Hybrid, hybrid_ndcg),
            (ConfigurationType::HybridReranking, hybrid_rerank_ndcg),
        ];
        let (best_config_type, best_ndcg) = ndcgs
            .iter()
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            .unwrap();

        comparisons.push(QueryComparison4Way {
            query: query.query_text.clone(),
            query_type: query.query_type,
            has_ground_truth,
            baseline_ndcg,
            baseline_precision,
            baseline_recall,
            baseline_mrr,
            baseline_latency_ms: baseline_latency.as_millis() as u64,
            baseline_metrics,
            baseline_results: vec![], // Empty for now - not used in evaluation
            dense_reranking_ndcg: dense_rerank_ndcg,
            dense_reranking_precision: dense_rerank_precision,
            dense_reranking_recall: dense_rerank_recall,
            dense_reranking_mrr: dense_rerank_mrr,
            dense_reranking_latency_ms: dense_rerank_latency.as_millis() as u64,
            dense_reranking_metrics: dense_rerank_metrics,
            dense_reranking_results: vec![],
            hybrid_ndcg,
            hybrid_precision,
            hybrid_recall,
            hybrid_mrr,
            hybrid_latency_ms: hybrid_latency.as_millis() as u64,
            hybrid_metrics,
            hybrid_results: vec![],
            hybrid_reranking_ndcg: hybrid_rerank_ndcg,
            hybrid_reranking_precision: hybrid_rerank_precision,
            hybrid_reranking_recall: hybrid_rerank_recall,
            hybrid_reranking_mrr: hybrid_rerank_mrr,
            hybrid_reranking_latency_ms: hybrid_rerank_latency.as_millis() as u64,
            hybrid_reranking_metrics: hybrid_rerank_metrics,
            hybrid_reranking_results: vec![],
            best_config: best_config_type.name().to_string(),
            best_ndcg: *best_ndcg,
        });
    }

    let total_queries = comparisons.len();
    let comparisons_with_gt: Vec<_> = comparisons.iter().filter(|c| c.has_ground_truth).collect();
    let queries_with_ground_truth = comparisons_with_gt.len();

    let configurations = vec![
        ConfigurationResults {
            config_type: ConfigurationType::Baseline,
            name: "baseline".to_string(),
            avg_ndcg: calculate_avg_ndcg(&comparisons_with_gt, |c| c.baseline_ndcg),
            avg_precision_at_10: calculate_avg_ndcg(&comparisons_with_gt, |c| c.baseline_precision),
            avg_recall_at_10: calculate_avg_ndcg(&comparisons_with_gt, |c| c.baseline_recall),
            avg_mrr: calculate_avg_ndcg(&comparisons_with_gt, |c| c.baseline_mrr),
            avg_latency_ms: calculate_avg_latency(&comparisons, |c| c.baseline_latency_ms),
            avg_dense_embedding_ms: calculate_avg_metrics_u64(
                &comparisons,
                |c| &c.baseline_metrics,
                |m| m.dense_embedding_time_ms,
            ),
            avg_sparse_embedding_ms: calculate_avg_metrics_u64(
                &comparisons,
                |c| &c.baseline_metrics,
                |m| m.sparse_embedding_time_ms,
            ),
            avg_vector_search_ms: calculate_avg_metrics_u64(
                &comparisons,
                |c| &c.baseline_metrics,
                |m| m.vector_search_time_ms,
            ),
            avg_reranking_ms: calculate_avg_metrics_u64(
                &comparisons,
                |c| &c.baseline_metrics,
                |m| m.reranking_time_ms,
            ),
            avg_sparse_vector_size: calculate_avg_metrics_usize(
                &comparisons,
                |c| &c.baseline_metrics,
                |m| m.sparse_vector_size,
            ),
            avg_sparse_vector_density: calculate_avg_metrics_f64(
                &comparisons,
                |c| &c.baseline_metrics,
                |m| m.sparse_vector_density,
            ),
            queries_evaluated: queries_with_ground_truth,
        },
        ConfigurationResults {
            config_type: ConfigurationType::DenseReranking,
            name: "dense+reranking".to_string(),
            avg_ndcg: calculate_avg_ndcg(&comparisons_with_gt, |c| c.dense_reranking_ndcg),
            avg_precision_at_10: calculate_avg_ndcg(&comparisons_with_gt, |c| {
                c.dense_reranking_precision
            }),
            avg_recall_at_10: calculate_avg_ndcg(&comparisons_with_gt, |c| {
                c.dense_reranking_recall
            }),
            avg_mrr: calculate_avg_ndcg(&comparisons_with_gt, |c| c.dense_reranking_mrr),
            avg_latency_ms: calculate_avg_latency(&comparisons, |c| c.dense_reranking_latency_ms),
            avg_dense_embedding_ms: calculate_avg_metrics_u64(
                &comparisons,
                |c| &c.dense_reranking_metrics,
                |m| m.dense_embedding_time_ms,
            ),
            avg_sparse_embedding_ms: calculate_avg_metrics_u64(
                &comparisons,
                |c| &c.dense_reranking_metrics,
                |m| m.sparse_embedding_time_ms,
            ),
            avg_vector_search_ms: calculate_avg_metrics_u64(
                &comparisons,
                |c| &c.dense_reranking_metrics,
                |m| m.vector_search_time_ms,
            ),
            avg_reranking_ms: calculate_avg_metrics_u64(
                &comparisons,
                |c| &c.dense_reranking_metrics,
                |m| m.reranking_time_ms,
            ),
            avg_sparse_vector_size: calculate_avg_metrics_usize(
                &comparisons,
                |c| &c.dense_reranking_metrics,
                |m| m.sparse_vector_size,
            ),
            avg_sparse_vector_density: calculate_avg_metrics_f64(
                &comparisons,
                |c| &c.dense_reranking_metrics,
                |m| m.sparse_vector_density,
            ),
            queries_evaluated: queries_with_ground_truth,
        },
        ConfigurationResults {
            config_type: ConfigurationType::Hybrid,
            name: "hybrid".to_string(),
            avg_ndcg: calculate_avg_ndcg(&comparisons_with_gt, |c| c.hybrid_ndcg),
            avg_precision_at_10: calculate_avg_ndcg(&comparisons_with_gt, |c| c.hybrid_precision),
            avg_recall_at_10: calculate_avg_ndcg(&comparisons_with_gt, |c| c.hybrid_recall),
            avg_mrr: calculate_avg_ndcg(&comparisons_with_gt, |c| c.hybrid_mrr),
            avg_latency_ms: calculate_avg_latency(&comparisons, |c| c.hybrid_latency_ms),
            avg_dense_embedding_ms: calculate_avg_metrics_u64(
                &comparisons,
                |c| &c.hybrid_metrics,
                |m| m.dense_embedding_time_ms,
            ),
            avg_sparse_embedding_ms: calculate_avg_metrics_u64(
                &comparisons,
                |c| &c.hybrid_metrics,
                |m| m.sparse_embedding_time_ms,
            ),
            avg_vector_search_ms: calculate_avg_metrics_u64(
                &comparisons,
                |c| &c.hybrid_metrics,
                |m| m.vector_search_time_ms,
            ),
            avg_reranking_ms: calculate_avg_metrics_u64(
                &comparisons,
                |c| &c.hybrid_metrics,
                |m| m.reranking_time_ms,
            ),
            avg_sparse_vector_size: calculate_avg_metrics_usize(
                &comparisons,
                |c| &c.hybrid_metrics,
                |m| m.sparse_vector_size,
            ),
            avg_sparse_vector_density: calculate_avg_metrics_f64(
                &comparisons,
                |c| &c.hybrid_metrics,
                |m| m.sparse_vector_density,
            ),
            queries_evaluated: queries_with_ground_truth,
        },
        ConfigurationResults {
            config_type: ConfigurationType::HybridReranking,
            name: "hybrid+reranking".to_string(),
            avg_ndcg: calculate_avg_ndcg(&comparisons_with_gt, |c| c.hybrid_reranking_ndcg),
            avg_precision_at_10: calculate_avg_ndcg(&comparisons_with_gt, |c| {
                c.hybrid_reranking_precision
            }),
            avg_recall_at_10: calculate_avg_ndcg(&comparisons_with_gt, |c| {
                c.hybrid_reranking_recall
            }),
            avg_mrr: calculate_avg_ndcg(&comparisons_with_gt, |c| c.hybrid_reranking_mrr),
            avg_latency_ms: calculate_avg_latency(&comparisons, |c| c.hybrid_reranking_latency_ms),
            avg_dense_embedding_ms: calculate_avg_metrics_u64(
                &comparisons,
                |c| &c.hybrid_reranking_metrics,
                |m| m.dense_embedding_time_ms,
            ),
            avg_sparse_embedding_ms: calculate_avg_metrics_u64(
                &comparisons,
                |c| &c.hybrid_reranking_metrics,
                |m| m.sparse_embedding_time_ms,
            ),
            avg_vector_search_ms: calculate_avg_metrics_u64(
                &comparisons,
                |c| &c.hybrid_reranking_metrics,
                |m| m.vector_search_time_ms,
            ),
            avg_reranking_ms: calculate_avg_metrics_u64(
                &comparisons,
                |c| &c.hybrid_reranking_metrics,
                |m| m.reranking_time_ms,
            ),
            avg_sparse_vector_size: calculate_avg_metrics_usize(
                &comparisons,
                |c| &c.hybrid_reranking_metrics,
                |m| m.sparse_vector_size,
            ),
            avg_sparse_vector_density: calculate_avg_metrics_f64(
                &comparisons,
                |c| &c.hybrid_reranking_metrics,
                |m| m.sparse_vector_density,
            ),
            queries_evaluated: queries_with_ground_truth,
        },
    ];

    let pairwise_comparisons = vec![
        calculate_comparison_metrics(
            "baseline",
            "dense+reranking",
            &comparisons_with_gt,
            |c| c.baseline_ndcg,
            |c| c.dense_reranking_ndcg,
            &comparisons,
            |c| c.baseline_latency_ms,
            |c| c.dense_reranking_latency_ms,
        ),
        calculate_comparison_metrics(
            "baseline",
            "hybrid",
            &comparisons_with_gt,
            |c| c.baseline_ndcg,
            |c| c.hybrid_ndcg,
            &comparisons,
            |c| c.baseline_latency_ms,
            |c| c.hybrid_latency_ms,
        ),
        calculate_comparison_metrics(
            "baseline",
            "hybrid+reranking",
            &comparisons_with_gt,
            |c| c.baseline_ndcg,
            |c| c.hybrid_reranking_ndcg,
            &comparisons,
            |c| c.baseline_latency_ms,
            |c| c.hybrid_reranking_latency_ms,
        ),
        calculate_comparison_metrics(
            "hybrid",
            "hybrid+reranking",
            &comparisons_with_gt,
            |c| c.hybrid_ndcg,
            |c| c.hybrid_reranking_ndcg,
            &comparisons,
            |c| c.hybrid_latency_ms,
            |c| c.hybrid_reranking_latency_ms,
        ),
        calculate_comparison_metrics(
            "dense+reranking",
            "hybrid+reranking",
            &comparisons_with_gt,
            |c| c.dense_reranking_ndcg,
            |c| c.hybrid_reranking_ndcg,
            &comparisons,
            |c| c.dense_reranking_latency_ms,
            |c| c.hybrid_reranking_latency_ms,
        ),
    ];

    let best_ndcg_config = configurations
        .iter()
        .max_by(|a, b| {
            a.avg_ndcg
                .partial_cmp(&b.avg_ndcg)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|c| c.name.clone())
        .unwrap_or_default();

    let best_latency_config = configurations
        .iter()
        .min_by(|a, b| {
            a.avg_latency_ms
                .partial_cmp(&b.avg_latency_ms)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|c| c.name.clone())
        .unwrap_or_default();

    Ok(HybridSearchEvaluationReport {
        total_queries,
        queries_with_ground_truth,
        prefetch_multiplier,
        configurations,
        comparisons: pairwise_comparisons,
        best_ndcg_config,
        best_latency_config,
        query_comparisons: comparisons,
    })
}

/// Print 4-way report to console
fn print_4way_report(report: &HybridSearchEvaluationReport) {
    println!("\n=== 4-Way Hybrid Search Evaluation Report ===\n");
    println!("Total Queries: {}", report.total_queries);
    println!(
        "Queries with Ground Truth: {}",
        report.queries_with_ground_truth
    );
    println!("Prefetch Multiplier: {}", report.prefetch_multiplier);

    println!("\n--- Configuration Results ---");
    for config in &report.configurations {
        println!("{:20} NDCG@10: {:.4}  Precision@10: {:.4}  Recall@10: {:.4}  MRR: {:.4}  Latency: {:.1}ms",
            config.name,
            config.avg_ndcg,
            config.avg_precision_at_10,
            config.avg_recall_at_10,
            config.avg_mrr,
            config.avg_latency_ms
        );
    }

    println!("\n--- Best Configurations ---");
    println!("Best NDCG@10: {}", report.best_ndcg_config);
    println!("Best Latency: {}", report.best_latency_config);

    println!("\n--- Pairwise Comparisons ---");
    for comp in &report.comparisons {
        println!("{} -> {}:", comp.from_config, comp.to_config);
        println!("  NDCG Improvement: {:+.4}", comp.avg_ndcg_improvement);
        println!(
            "  Improved: {}  Degraded: {}  Unchanged: {}",
            comp.queries_improved, comp.queries_degraded, comp.queries_unchanged
        );
        println!("  Latency Increase: {:+.1}ms", comp.avg_latency_increase_ms);
    }
}

/// Save report to JSON file
fn save_hybrid_report(report: &HybridSearchEvaluationReport, path: &str) -> Result<()> {
    let json = serde_json::to_string_pretty(report)?;
    std::fs::write(path, json)?;
    println!("\nReport saved to: {}", path);
    Ok(())
}

/// Run prefetch sensitivity analysis across multiple prefetch_multiplier values
async fn run_prefetch_sensitivity_analysis(
    config: &Config,
    repository_id: Uuid,
    collection_name: &str,
) -> Result<PrefetchSensitivityReport> {
    println!("\n=== Prefetch Multiplier Sensitivity Analysis ===\n");

    // Test these prefetch_multiplier values
    let prefetch_multipliers = vec![3, 4, 5, 6, 7, 8, 9, 10, 12, 15];

    let mut sensitivity_results = Vec::new();

    for &prefetch_multiplier in &prefetch_multipliers {
        println!(
            "\n--- Testing prefetch_multiplier = {} ---",
            prefetch_multiplier
        );

        // Run full evaluation for this prefetch value
        let report =
            evaluate_hybrid_search(config, repository_id, collection_name, prefetch_multiplier)
                .await?;

        // Extract hybrid and hybrid+reranking results
        let hybrid_config = report
            .configurations
            .iter()
            .find(|c| c.config_type == ConfigurationType::Hybrid)
            .unwrap();
        let hybrid_rerank_config = report
            .configurations
            .iter()
            .find(|c| c.config_type == ConfigurationType::HybridReranking)
            .unwrap();

        // Calculate prefetch efficiency (placeholder for now)
        let prefetch_efficiency = 0.0;

        sensitivity_results.push(PrefetchSensitivityResult {
            prefetch_multiplier,
            hybrid_avg_ndcg: hybrid_config.avg_ndcg,
            hybrid_avg_precision: hybrid_config.avg_precision_at_10,
            hybrid_avg_recall: hybrid_config.avg_recall_at_10,
            hybrid_avg_latency_ms: hybrid_config.avg_latency_ms,
            hybrid_rerank_avg_ndcg: hybrid_rerank_config.avg_ndcg,
            hybrid_rerank_avg_precision: hybrid_rerank_config.avg_precision_at_10,
            hybrid_rerank_avg_recall: hybrid_rerank_config.avg_recall_at_10,
            hybrid_rerank_avg_latency_ms: hybrid_rerank_config.avg_latency_ms,
            prefetch_efficiency,
        });
    }

    // Find optimal configurations
    let optimal_for_ndcg = sensitivity_results
        .iter()
        .max_by(|a, b| {
            a.hybrid_rerank_avg_ndcg
                .partial_cmp(&b.hybrid_rerank_avg_ndcg)
                .unwrap()
        })
        .map(|r| OptimalPrefetchConfig {
            prefetch_multiplier: r.prefetch_multiplier,
            ndcg: r.hybrid_rerank_avg_ndcg,
            latency_ms: r.hybrid_rerank_avg_latency_ms,
            efficiency: r.prefetch_efficiency,
        })
        .unwrap();

    let optimal_for_latency = sensitivity_results
        .iter()
        .min_by(|a, b| {
            a.hybrid_rerank_avg_latency_ms
                .partial_cmp(&b.hybrid_rerank_avg_latency_ms)
                .unwrap()
        })
        .map(|r| OptimalPrefetchConfig {
            prefetch_multiplier: r.prefetch_multiplier,
            ndcg: r.hybrid_rerank_avg_ndcg,
            latency_ms: r.hybrid_rerank_avg_latency_ms,
            efficiency: r.prefetch_efficiency,
        })
        .unwrap();

    // Find balanced optimum (maximize NDCG while keeping latency reasonable)
    // Use a simple scoring function: NDCG / (latency_ms / 100)
    let optimal_balanced = sensitivity_results
        .iter()
        .max_by(|a, b| {
            let score_a = a.hybrid_rerank_avg_ndcg / (a.hybrid_rerank_avg_latency_ms / 100.0);
            let score_b = b.hybrid_rerank_avg_ndcg / (b.hybrid_rerank_avg_latency_ms / 100.0);
            score_a.partial_cmp(&score_b).unwrap()
        })
        .map(|r| OptimalPrefetchConfig {
            prefetch_multiplier: r.prefetch_multiplier,
            ndcg: r.hybrid_rerank_avg_ndcg,
            latency_ms: r.hybrid_rerank_avg_latency_ms,
            efficiency: r.prefetch_efficiency,
        })
        .unwrap();

    Ok(PrefetchSensitivityReport {
        prefetch_multipliers_tested: prefetch_multipliers,
        sensitivity_results,
        optimal_for_ndcg,
        optimal_for_latency,
        optimal_balanced,
    })
}

/// Print sensitivity analysis report to console
fn print_sensitivity_report(report: &PrefetchSensitivityReport) {
    println!("\n=== Prefetch Multiplier Sensitivity Analysis ===\n");
    println!("Tested values: {:?}", report.prefetch_multipliers_tested);

    println!("\n--- Results (Hybrid + Reranking Configuration) ---");
    println!(
        "{:8} {:10} {:10} {:10} {:12}",
        "Prefetch", "NDCG@10", "Precision", "Recall", "Latency (ms)"
    );
    for result in &report.sensitivity_results {
        println!(
            "{:8} {:10.4} {:10.4} {:10.4} {:12.1}",
            result.prefetch_multiplier,
            result.hybrid_rerank_avg_ndcg,
            result.hybrid_rerank_avg_precision,
            result.hybrid_rerank_avg_recall,
            result.hybrid_rerank_avg_latency_ms,
        );
    }

    println!("\n--- Optimal Configurations ---");
    println!(
        "Best NDCG@10: prefetch_multiplier = {} (NDCG: {:.4}, Latency: {:.1}ms)",
        report.optimal_for_ndcg.prefetch_multiplier,
        report.optimal_for_ndcg.ndcg,
        report.optimal_for_ndcg.latency_ms,
    );
    println!(
        "Best Latency: prefetch_multiplier = {} (NDCG: {:.4}, Latency: {:.1}ms)",
        report.optimal_for_latency.prefetch_multiplier,
        report.optimal_for_latency.ndcg,
        report.optimal_for_latency.latency_ms,
    );
    println!(
        "Best Balanced: prefetch_multiplier = {} (NDCG: {:.4}, Latency: {:.1}ms)",
        report.optimal_balanced.prefetch_multiplier,
        report.optimal_balanced.ndcg,
        report.optimal_balanced.latency_ms,
    );
}

/// Save sensitivity analysis report to JSON file
fn save_sensitivity_report(report: &PrefetchSensitivityReport, path: &str) -> Result<()> {
    let json = serde_json::to_string_pretty(report)?;
    std::fs::write(path, json)?;
    println!("\nReport saved to: {}", path);
    Ok(())
}

/// Test prefetch sensitivity analysis
#[tokio::test]
#[ignore]
async fn test_prefetch_sensitivity_analysis() -> Result<()> {
    // Load config
    let config_path = global_config_path()?;
    let config = Config::from_file(&config_path)?;

    // Get workspace root
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let workspace_root = Path::new(manifest_dir).parent().unwrap();
    let repo_path = workspace_root.to_path_buf();

    // Prepare report output path
    let report_path = workspace_root.join("target/prefetch_sensitivity_report.json");

    // Look up repository
    let postgres_client = create_postgres_client(&config.storage).await?;
    let (repository_id, collection_name) = postgres_client
        .get_repository_by_path(&repo_path)
        .await?
        .context("Repository not indexed")?;

    println!("Found repository: {}", repository_id);
    println!("Collection name: {}", collection_name);

    // Run sensitivity analysis
    let report =
        run_prefetch_sensitivity_analysis(&config, repository_id, &collection_name).await?;

    // Print and save report
    print_sensitivity_report(&report);
    save_sensitivity_report(&report, report_path.to_str().unwrap())?;

    Ok(())
}

/// Set up clap repository for evaluation
async fn setup_clap_repository(config: &Config) -> Result<(std::path::PathBuf, Uuid, String)> {
    const CLAP_VERSION: &str = "v4.5.0";
    const CLAP_REPO_URL: &str = "https://github.com/clap-rs/clap.git";

    let repo_path = std::path::PathBuf::from(format!("/tmp/clap-eval-{}", CLAP_VERSION));

    println!("\n=== Setting up clap repository for evaluation ===");
    println!("Repository path: {}", repo_path.display());
    println!("Version: {}", CLAP_VERSION);

    // Check if repository needs to be cloned or updated
    if !repo_path.exists() {
        println!("Cloning clap repository...");
        let output = std::process::Command::new("git")
            .args([
                "clone",
                "--depth",
                "1",
                "--branch",
                CLAP_VERSION,
                CLAP_REPO_URL,
                repo_path.to_str().unwrap(),
            ])
            .output()
            .context("Failed to clone clap repository")?;

        if !output.status.success() {
            anyhow::bail!(
                "Failed to clone clap: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        println!("✓ Cloned clap {}", CLAP_VERSION);
    } else {
        println!(
            "✓ clap repository already exists at {}",
            repo_path.display()
        );
    }

    // Check if repository is already indexed
    let postgres_client = create_postgres_client(&config.storage).await?;

    match postgres_client.get_repository_by_path(&repo_path).await? {
        Some((repository_id, collection_name)) => {
            println!("✓ clap is already indexed");
            println!("  Repository ID: {}", repository_id);
            println!("  Collection: {}", collection_name);

            // Verify entity count
            let count_result = postgres_client
                .get_pool()
                .fetch_one(
                    sqlx::query(
                        "SELECT COUNT(*) as count FROM entity_metadata WHERE repository_id = $1",
                    )
                    .bind(repository_id),
                )
                .await?;
            let entity_count: i64 = count_result.try_get("count")?;

            println!("  Entities indexed: {}", entity_count);

            if entity_count == 0 {
                anyhow::bail!(
                    "Repository is indexed but has 0 entities. Please re-index: \
                     cd {} && codesearch index",
                    repo_path.display()
                );
            }

            Ok((repo_path, repository_id, collection_name))
        }
        None => {
            anyhow::bail!(
                "clap repository is not indexed. Please index it first:\n  \
                 cd {} && codesearch index",
                repo_path.display()
            );
        }
    }
}

/// Load evaluation queries from JSON file
fn load_evaluation_queries() -> Result<Vec<TestQuery>> {
    #[derive(Deserialize)]
    struct QueryEntry {
        query: String,
        query_type: String,
        #[allow(dead_code)]
        category: String,
    }

    let queries_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("data/evaluation_queries.json");

    let content = std::fs::read_to_string(&queries_path)
        .with_context(|| format!("Failed to read queries from {}", queries_path.display()))?;

    let evaluation_queries: Vec<QueryEntry> =
        serde_json::from_str(&content).context("Failed to parse evaluation_queries.json")?;

    let queries = evaluation_queries
        .into_iter()
        .map(|entry| TestQuery {
            query_text: entry.query,
            // Map realistic to documentation, entity_focused to exact name
            query_type: if entry.query_type == "realistic" {
                QueryType::Documentation
            } else {
                QueryType::ExactName
            },
        })
        .collect();

    Ok(queries)
}

/// Test entry point
#[tokio::test]
#[ignore]
async fn test_hybrid_search_evaluation() -> Result<()> {
    let config_path = global_config_path()?;
    let config = Config::from_file(&config_path)?;

    // Set up clap repository
    let (repo_path, repository_id, collection_name) = setup_clap_repository(&config).await?;

    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let workspace_root = Path::new(manifest_dir).parent().unwrap();
    let report_path = workspace_root.join("target/hybrid_search_evaluation_report.json");

    println!("\nRepository: {}", repo_path.display());
    println!("Repository ID: {}", repository_id);
    println!("Collection: {}", collection_name);

    let prefetch_multiplier = 5;
    let report = evaluate_hybrid_search(
        &config,
        repository_id,
        &collection_name,
        prefetch_multiplier,
    )
    .await?;

    print_4way_report(&report);
    save_hybrid_report(&report, report_path.to_str().unwrap())?;

    Ok(())
}

/// Tool to collect diverse queries for hybrid search evaluation
#[tokio::test]
#[ignore]
async fn test_collect_hybrid_labeling_data() -> Result<()> {
    println!("\n=== Collecting Hybrid Search Query/Result Data for Labeling ===\n");

    let config_path = global_config_path()?;
    let config = Config::from_file(&config_path)?;

    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let workspace_root = Path::new(manifest_dir).parent().unwrap();
    let repo_path = workspace_root.to_path_buf();

    let postgres_client = create_postgres_client(&config.storage).await?;
    let (repository_id, collection_name) = postgres_client
        .get_repository_by_path(&repo_path)
        .await?
        .context("Repository not indexed")?;

    println!("Found repository: {}", repository_id);
    println!("Collection name: {}\n", collection_name);

    // Generate diverse queries
    let query_generator = HybridQueryGenerator::new(postgres_client.clone(), repository_id);
    let queries = query_generator.generate_diverse_queries(40, 40, 20).await?;

    println!("\nGenerated {} total queries\n", queries.len());

    // Create both dense and hybrid executors
    let dense_executor =
        SearchExecutor::new(&config, repository_id, &collection_name, false, false, 5).await?;
    let hybrid_executor =
        SearchExecutor::new(&config, repository_id, &collection_name, false, true, 5).await?;

    let output_path = Path::new(manifest_dir).join("data/hybrid_labeling_data.json");

    let mut all_data = Vec::new();

    for (idx, query) in queries.iter().enumerate() {
        println!(
            "[{}/{}] {:?}  Processing: \"{}\"",
            idx + 1,
            queries.len(),
            query.query_type.description(),
            query.query_text
        );

        // Run both dense and hybrid searches
        let (dense_results, _, _) = dense_executor.search(&query.query_text, 20, false).await?;
        let (hybrid_results, _, _) = hybrid_executor.search(&query.query_text, 20, false).await?;

        if dense_results.is_empty() && hybrid_results.is_empty() {
            println!("  No results from either method, skipping\n");
            continue;
        }

        // Fetch entity details for type information
        let all_entity_ids: std::collections::HashSet<String> = dense_results
            .iter()
            .map(|r| r.entity_id.clone())
            .chain(hybrid_results.iter().map(|r| r.entity_id.clone()))
            .collect();

        let entity_refs: Vec<_> = all_entity_ids
            .iter()
            .map(|eid| (repository_id, eid.clone()))
            .collect();

        let entities = postgres_client.get_entities_by_ids(&entity_refs).await?;
        let entity_map: HashMap<String, &CodeEntity> =
            entities.iter().map(|e| (e.entity_id.clone(), e)).collect();

        all_data.push(HybridLabelingData {
            query: query.query_text.clone(),
            query_type: query.query_type,
            dense_results: dense_results
                .iter()
                .map(|r| LabelingResult {
                    entity_id: r.entity_id.clone(),
                    entity_name: r.entity_name.clone(),
                    entity_type: entity_map
                        .get(&r.entity_id)
                        .map(|e| format!("{:?}", e.entity_type))
                        .unwrap_or_else(|| "Unknown".to_string()),
                    score: r.score,
                })
                .collect(),
            hybrid_results: hybrid_results
                .iter()
                .map(|r| LabelingResult {
                    entity_id: r.entity_id.clone(),
                    entity_name: r.entity_name.clone(),
                    entity_type: entity_map
                        .get(&r.entity_id)
                        .map(|e| format!("{:?}", e.entity_type))
                        .unwrap_or_else(|| "Unknown".to_string()),
                    score: r.score,
                })
                .collect(),
        });

        println!(
            "  Dense: {} results, Hybrid: {} results\n",
            dense_results.len(),
            hybrid_results.len()
        );
    }

    // Save to JSON
    let json = serde_json::to_string_pretty(&all_data)?;
    std::fs::write(&output_path, json)?;

    println!("\n{}", "=".repeat(70));
    println!("Data collection complete!");
    println!("Collected {} queries with results", all_data.len());
    println!("Data saved to: {}", output_path.display());
    println!(
        "\nNext steps:\n  1. Review the file and assign relevance scores (0-3) for each entity\n  2. Save labeled data as data/ground_truth_hybrid.json\n  3. Update evaluate_hybrid_search() to use the new ground truth"
    );
    println!("{}\n", "=".repeat(70));

    Ok(())
}

/// Collect realistic query results for clap evaluation
#[tokio::test]
#[ignore]
async fn test_collect_realistic_query_results() -> Result<()> {
    println!("\n=== Collecting Realistic Query Results for Labeling ===\n");

    let config_path = global_config_path()?;
    let config = Config::from_file(&config_path)?;

    // Use clap-eval repository
    let (repo_path, repository_id, collection_name) = setup_clap_repository(&config).await?;

    println!("Repository: {}", repo_path.display());
    println!("Repository ID: {}", repository_id);
    println!("Collection: {}\n", collection_name);

    // Load realistic queries
    let queries = load_evaluation_queries()?;
    println!("Loaded {} realistic queries\n", queries.len());

    let postgres_client = create_postgres_client(&config.storage).await?;

    // Create both dense and hybrid executors
    let dense_executor =
        SearchExecutor::new(&config, repository_id, &collection_name, false, false, 5).await?;
    let hybrid_executor =
        SearchExecutor::new(&config, repository_id, &collection_name, false, true, 5).await?;

    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let output_path = Path::new(manifest_dir).join("data/realistic_query_results.json");

    let mut all_data = Vec::new();

    for (idx, query) in queries.iter().enumerate() {
        println!(
            "[{}/{}] Processing: \"{}\"",
            idx + 1,
            queries.len(),
            query.query_text
        );

        // Run both dense and hybrid searches (20 results for labeling)
        let (dense_results, _, _) = dense_executor.search(&query.query_text, 20, false).await?;
        let (hybrid_results, _, _) = hybrid_executor.search(&query.query_text, 20, false).await?;

        if dense_results.is_empty() && hybrid_results.is_empty() {
            println!("  No results from either method, skipping\n");
            continue;
        }

        // Fetch entity details for type information
        let all_entity_ids: std::collections::HashSet<String> = dense_results
            .iter()
            .map(|r| r.entity_id.clone())
            .chain(hybrid_results.iter().map(|r| r.entity_id.clone()))
            .collect();

        let entity_refs: Vec<_> = all_entity_ids
            .iter()
            .map(|eid| (repository_id, eid.clone()))
            .collect();

        let entities = postgres_client.get_entities_by_ids(&entity_refs).await?;
        let entity_map: HashMap<String, &CodeEntity> =
            entities.iter().map(|e| (e.entity_id.clone(), e)).collect();

        all_data.push(HybridLabelingData {
            query: query.query_text.clone(),
            query_type: HybridQueryType::Semantic, // All realistic queries are semantic
            dense_results: dense_results
                .iter()
                .map(|r| LabelingResult {
                    entity_id: r.entity_id.clone(),
                    entity_name: r.entity_name.clone(),
                    entity_type: entity_map
                        .get(&r.entity_id)
                        .map(|e| format!("{:?}", e.entity_type))
                        .unwrap_or_else(|| "Unknown".to_string()),
                    score: r.score,
                })
                .collect(),
            hybrid_results: hybrid_results
                .iter()
                .map(|r| LabelingResult {
                    entity_id: r.entity_id.clone(),
                    entity_name: r.entity_name.clone(),
                    entity_type: entity_map
                        .get(&r.entity_id)
                        .map(|e| format!("{:?}", e.entity_type))
                        .unwrap_or_else(|| "Unknown".to_string()),
                    score: r.score,
                })
                .collect(),
        });

        println!(
            "  Dense: {} results, Hybrid: {} results\n",
            dense_results.len(),
            hybrid_results.len()
        );
    }

    // Save to JSON
    let json = serde_json::to_string_pretty(&all_data)?;
    std::fs::write(&output_path, json)?;

    println!("\n{}", "=".repeat(70));
    println!("Data collection complete!");
    println!("Collected {} queries with results", all_data.len());
    println!("Data saved to: {}", output_path.display());
    println!(
        "\nNext steps:\n  1. Use LLM to label entities with binary relevance (0 or 1)\n  2. Save labeled data as data/ground_truth_hybrid.json\n  3. Run evaluation with labels"
    );
    println!("{}\n", "=".repeat(70));

    Ok(())
}
