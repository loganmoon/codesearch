//! Reranking Evaluation Test Suite
//!
//! This test evaluates the reranking feature by:
//! 1. Generating synthetic queries from indexed entities
//! 2. Running searches with and without reranking
//! 3. Comparing results and computing metrics
//! 4. Generating a report
//!
//! Prerequisites:
//! - The codesearch repository must be indexed
//! - Shared infrastructure (Postgres, Qdrant) must be running
//! - Config file should exist at ~/.codesearch/config.toml
//!
//! Run with: cargo test --package codesearch-e2e-tests --test test_reranking_evaluation -- --ignored --nocapture

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
use std::{
    collections::HashMap,
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};
use uuid::Uuid;

/// Query type classification
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
enum QueryType {
    ExactName,
    Documentation,
}

/// Ground truth relevance label for a single query
#[derive(Debug, Clone, Serialize, Deserialize)]
struct GroundTruthLabel {
    query: String,
    query_type: QueryType,
    /// Map of entity_id -> relevance score (0-3 scale)
    /// 0 = not relevant, 1 = marginally relevant, 2 = relevant, 3 = highly relevant
    entity_relevance: HashMap<String, u8>,
}

/// Collection of ground truth labels for evaluation
#[derive(Debug, Clone, Serialize, Deserialize)]
struct GroundTruthDataset {
    labels: Vec<GroundTruthLabel>,
}

/// Generated test query
#[derive(Debug, Clone)]
struct TestQuery {
    query_text: String,
    query_type: QueryType,
}

/// Search result for a single query
#[derive(Debug, Clone)]
struct SearchResult {
    entity_id: String,
    entity_name: String,
    score: f32,
    rank: usize,
}

/// Comparison between baseline and reranking for a single query
#[derive(Debug, Serialize)]
struct QueryComparison {
    query: String,
    query_type: QueryType,
    // NDCG metrics
    has_ground_truth: bool,
    baseline_ndcg: f64,
    reranking_ndcg: f64,
    ndcg_improvement: f64, // reranking_ndcg - baseline_ndcg
    // Latency metrics
    baseline_latency_ms: u64,
    reranking_latency_ms: u64,
}

/// Overall evaluation report
#[derive(Debug, Serialize)]
struct EvaluationReport {
    total_queries: usize,
    // NDCG metrics
    queries_with_ground_truth: usize,
    avg_baseline_ndcg: f64,
    avg_reranking_ndcg: f64,
    ndcg_improvement: f64, // avg_reranking_ndcg - avg_baseline_ndcg
    queries_with_ndcg_improvement: usize,
    queries_with_ndcg_degradation: usize,
    queries_with_ndcg_unchanged: usize,
    // Latency metrics
    average_baseline_latency_ms: f64,
    average_reranking_latency_ms: f64,
    latency_overhead_percent: f64,
    // Detailed comparisons
    query_comparisons: Vec<QueryComparison>,
}

/// Query generator that creates synthetic queries from indexed entities
struct QueryGenerator {
    postgres_client: Arc<dyn PostgresClientTrait>,
    repository_id: Uuid,
}

impl QueryGenerator {
    fn new(postgres_client: Arc<dyn PostgresClientTrait>, repository_id: Uuid) -> Self {
        Self {
            postgres_client,
            repository_id,
        }
    }

    /// Fetch all entities for the repository from the database
    async fn fetch_all_entities(&self) -> Result<Vec<CodeEntity>> {
        // Use SQL query to fetch all entities for the repository
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
             LIMIT 1000",
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

    /// Generate test queries from indexed entities
    async fn generate_queries(&self, target_count: usize) -> Result<Vec<TestQuery>> {
        println!(
            "Generating {} test queries from indexed entities...",
            target_count
        );

        // Fetch all entities for the repository using SQL query
        let all_entities = self
            .fetch_all_entities()
            .await
            .context("Failed to fetch entities")?;

        println!("Found {} indexed entities", all_entities.len());

        // Filter entities to those with substantial content
        let entities_with_content: Vec<_> = all_entities
            .iter()
            .filter(|entity| {
                entity.content.as_ref().is_some_and(|c| c.len() > 100)
                    || entity
                        .documentation_summary
                        .as_ref()
                        .is_some_and(|d| d.len() > 20)
            })
            .collect();

        println!(
            "Found {} entities with substantial content",
            entities_with_content.len()
        );

        let mut queries = Vec::new();

        // Mix of query types for balanced evaluation:
        // - Exact names: High match rate, good for measuring reranking impact
        // - Documentation: Realistic semantic queries, test real-world scenarios

        let exact_name_target = target_count / 2; // 50% exact name queries
        let doc_target = target_count - exact_name_target; // 50% documentation queries

        // Generate exact name queries (realistic search queries)
        for entity in entities_with_content.iter().take(exact_name_target) {
            queries.push(TestQuery {
                query_text: entity.name.clone(),
                query_type: QueryType::ExactName,
            });
        }

        // Generate documentation queries (semantic search queries)
        let doc_entities: Vec<_> = entities_with_content
            .iter()
            .filter(|e| e.documentation_summary.is_some())
            .collect();

        println!("Found {} entities with documentation", doc_entities.len());

        for entity in doc_entities.iter().take(doc_target) {
            if let Some(doc_query) = self.entity_to_documentation_query(entity) {
                queries.push(TestQuery {
                    query_text: doc_query,
                    query_type: QueryType::Documentation,
                });
            }

            if queries.len() >= target_count {
                break;
            }
        }

        println!(
            "Generated {} test queries ({} exact names, {} documentation)",
            queries.len(),
            queries
                .iter()
                .filter(|q| matches!(q.query_type, QueryType::ExactName))
                .count(),
            queries
                .iter()
                .filter(|q| matches!(q.query_type, QueryType::Documentation))
                .count()
        );

        Ok(queries)
    }

    /// Generate paraphrased natural language descriptions from entity
    fn generate_paraphrased_query(&self, entity: &CodeEntity) -> Option<String> {
        let content = entity.content.as_ref()?;
        let content_lower = content.to_lowercase();
        let name = &entity.name;

        // Convert name to readable form (snake_case/camelCase -> words)
        let name_words = self.name_to_words(name);

        // Try to determine what the entity does based on patterns and name
        if content_lower.contains("check")
            || content_lower.contains("is_")
            || name.starts_with("is_")
            || name.starts_with("has_")
        {
            Some(format!("check {}", name_words))
        } else if content_lower.contains("create")
            || content_lower.contains("new")
            || name.starts_with("create_")
            || name.starts_with("new_")
            || name == "new"
        {
            Some(format!("create {}", name_words))
        } else if content_lower.contains("build") || name.starts_with("build_") {
            Some(format!("build {}", name_words))
        } else if content_lower.contains("get")
            || content_lower.contains("fetch")
            || name.starts_with("get_")
            || name.starts_with("fetch_")
        {
            Some(format!("get {}", name_words))
        } else if content_lower.contains("update") || name.starts_with("update_") {
            Some(format!("update {}", name_words))
        } else if content_lower.contains("delete")
            || content_lower.contains("remove")
            || name.starts_with("delete_")
            || name.starts_with("remove_")
        {
            Some(format!("remove {}", name_words))
        } else if content_lower.contains("handle")
            || content_lower.contains("process")
            || name.starts_with("handle_")
            || name.starts_with("process_")
        {
            Some(format!("handle {}", name_words))
        } else if content_lower.contains("extract") || name.starts_with("extract_") {
            Some(format!("extract {}", name_words))
        } else if content_lower.contains("run") || name.starts_with("run_") {
            Some(format!("run {}", name_words))
        } else {
            None
        }
    }

    /// Convert entity name to readable words
    fn name_to_words(&self, name: &str) -> String {
        let mut words = Vec::new();
        let mut current_word = String::new();

        for ch in name.chars() {
            if ch == '_' {
                if !current_word.is_empty() {
                    words.push(current_word.clone());
                    current_word.clear();
                }
            } else if ch.is_uppercase() {
                if !current_word.is_empty() {
                    words.push(current_word.clone());
                    current_word.clear();
                }
                current_word.push(ch);
            } else {
                current_word.push(ch);
            }
        }

        if !current_word.is_empty() {
            words.push(current_word);
        }

        words.join(" ").to_lowercase()
    }

    /// Extract content-based queries with entity context
    fn extract_contextual_queries(&self, entity: &CodeEntity) -> Vec<String> {
        let content = match &entity.content {
            Some(c) if !c.is_empty() => c,
            _ => return Vec::new(),
        };

        let mut queries = Vec::new();

        // Extract meaningful multi-line context snippets (not single lines)
        // Take first few lines that together form a meaningful unit
        let lines: Vec<&str> = content
            .lines()
            .map(|line| line.trim())
            .filter(|line| {
                !line.is_empty()
                    && !line
                        .chars()
                        .all(|c| c.is_whitespace() || c == '{' || c == '}')
            })
            .collect();

        // Create a contextual snippet: entity name + first 2-3 meaningful lines
        if lines.len() >= 2 {
            let snippet_lines: Vec<&str> = lines.iter().take(3).copied().collect();
            let snippet = snippet_lines.join(" ");

            // Truncate at word boundary if needed
            let truncated = if snippet.len() > 150 {
                if let Some(last_space) = snippet[..150].rfind(' ') {
                    &snippet[..last_space]
                } else {
                    &snippet[..150]
                }
            } else {
                &snippet
            };

            if truncated.len() > 20 {
                // Include entity name for context
                queries.push(format!("{} {}", entity.name, truncated));
            }
        }

        queries
    }

    /// Extract documentation-based query from entity
    fn entity_to_documentation_query(&self, entity: &CodeEntity) -> Option<String> {
        entity.documentation_summary.as_ref().and_then(|doc| {
            // Extract first sentence
            let first_sentence = doc.split('.').next().unwrap_or(doc);

            // Truncate at word boundary if needed
            let query = if first_sentence.len() > 100 {
                // Find last space within first 100 chars
                if let Some(last_space) = first_sentence[..100].rfind(' ') {
                    &first_sentence[..last_space]
                } else {
                    // No space found, truncate at 100 chars as fallback
                    &first_sentence[..100]
                }
            } else {
                first_sentence
            };

            if query.len() > 10 {
                Some(query.to_string())
            } else {
                None
            }
        })
    }
}

/// Search executor that runs queries in different modes
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

    /// Execute a search query and return top results
    async fn search(
        &self,
        query: &str,
        limit: usize,
        use_reranking: bool,
    ) -> Result<(Vec<SearchResult>, Duration)> {
        let start = Instant::now();

        // Format query with BGE instruction
        let formatted_query = format!("<instruct>{}\n<query>{}", self.bge_instruction, query);

        // Generate query embedding
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

        // Determine candidate limit based on reranking config
        let candidates_limit = if use_reranking { 50 } else { limit };

        // Perform either hybrid or dense search based on sparse_provider presence
        let search_results = if let Some(ref sparse_provider) = self.sparse_provider {
            // Generate sparse query embedding
            let sparse_embeddings = sparse_provider
                .embed_sparse(vec![query])
                .await
                .context("Failed to generate sparse embedding")?;

            let sparse_embedding = sparse_embeddings
                .into_iter()
                .next()
                .flatten()
                .context("Failed to generate sparse embedding")?;

            // Hybrid search with RRF fusion
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
            // Dense-only search
            self.storage_client
                .search_similar(query_embedding, candidates_limit, None)
                .await
                .context("Search failed")?
        };

        // Fetch entities from Postgres
        let entity_refs: Vec<_> = search_results
            .iter()
            .map(|(eid, _, _)| (self.repository_id, eid.to_string()))
            .collect();

        let entities = self
            .postgres_client
            .get_entities_by_ids(&entity_refs)
            .await
            .context("Failed to fetch entities")?;

        // Apply reranking if enabled
        let final_results = if use_reranking {
            if let Some(ref reranker) = self.reranker {
                // Build documents for reranking
                let entity_contents: Vec<(String, String)> = entities
                    .iter()
                    .map(|entity| (entity.entity_id.clone(), extract_embedding_content(entity)))
                    .collect();

                let documents: Vec<(String, &str)> = entity_contents
                    .iter()
                    .map(|(id, content)| (id.clone(), content.as_str()))
                    .collect();

                // Rerank
                let reranked = reranker
                    .rerank(query, &documents, limit)
                    .await
                    .context("Reranking failed")?;

                // Build entity map for lookup
                let entity_map: HashMap<String, &CodeEntity> =
                    entities.iter().map(|e| (e.entity_id.clone(), e)).collect();

                // Convert to SearchResult
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
                // No reranker available, use vector scores
                self.build_search_results(&search_results, &entities, limit)
            }
        } else {
            // Use vector scores
            self.build_search_results(&search_results, &entities, limit)
        };

        let elapsed = start.elapsed();
        Ok((final_results, elapsed))
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

/// Evaluate reranking by comparing baseline vs reranking results
async fn evaluate_reranking(
    config: &Config,
    repository_id: Uuid,
    collection_name: &str,
    _target_query_count: usize,
) -> Result<EvaluationReport> {
    println!("\n=== Reranking Evaluation Test ===\n");

    // Step 1: Load ground truth labels first
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let ground_truth_path =
        std::path::Path::new(manifest_dir).join("data/ground_truth_reranking.json");
    let ground_truth_dataset =
        load_ground_truth(&ground_truth_path).context("Failed to load ground truth data")?;

    if ground_truth_dataset.labels.is_empty() {
        anyhow::bail!("No ground truth labels found. Please run: cargo test test_collect_labeling_data -- --ignored --nocapture");
    }

    // Use the exact queries from ground truth to ensure we evaluate what we labeled
    let queries: Vec<TestQuery> = ground_truth_dataset
        .labels
        .iter()
        .map(|label| TestQuery {
            query_text: label.query.clone(),
            query_type: label.query_type,
        })
        .collect();

    println!("Loaded {} queries from ground truth\n", queries.len());

    // Build lookup map: query -> entity_id -> relevance
    let mut ground_truth_map: std::collections::HashMap<
        String,
        std::collections::HashMap<String, u8>,
    > = std::collections::HashMap::new();
    for label in &ground_truth_dataset.labels {
        ground_truth_map.insert(label.query.clone(), label.entity_relevance.clone());
    }

    println!("\n=== Running Searches ===\n");

    // Step 2: Create search executors
    let baseline_executor =
        SearchExecutor::new(config, repository_id, collection_name, false, false, 5).await?;
    let reranking_executor =
        SearchExecutor::new(config, repository_id, collection_name, true, false, 5).await?;

    // Step 3: Run searches and compare
    let mut comparisons = Vec::new();
    let limit = 10;

    for (idx, query) in queries.iter().enumerate() {
        println!(
            "[{}/{}] Testing query: \"{}\"",
            idx + 1,
            queries.len(),
            query.query_text
        );

        // Baseline search
        let (baseline_results, baseline_latency) = baseline_executor
            .search(&query.query_text, limit, false)
            .await?;

        // Reranking search
        let (reranking_results, reranking_latency) = reranking_executor
            .search(&query.query_text, limit, true)
            .await?;

        // Calculate NDCG@10 if ground truth exists for this query
        let has_ground_truth = ground_truth_map.contains_key(&query.query_text);
        let (baseline_ndcg, reranking_ndcg) =
            if let Some(relevance_map) = ground_truth_map.get(&query.query_text) {
                let baseline_ndcg = calculate_ndcg_at_k(&baseline_results, relevance_map, limit);
                let reranking_ndcg = calculate_ndcg_at_k(&reranking_results, relevance_map, limit);
                (baseline_ndcg, reranking_ndcg)
            } else {
                (0.0, 0.0)
            };

        let ndcg_improvement = reranking_ndcg - baseline_ndcg;

        comparisons.push(QueryComparison {
            query: query.query_text.clone(),
            query_type: query.query_type,
            has_ground_truth,
            baseline_ndcg,
            reranking_ndcg,
            ndcg_improvement,
            baseline_latency_ms: baseline_latency.as_millis() as u64,
            reranking_latency_ms: reranking_latency.as_millis() as u64,
        });
    }

    // Step 4: Compute aggregate metrics
    let total_queries = comparisons.len();

    // Filter to only queries with ground truth for NDCG calculation
    let comparisons_with_gt: Vec<_> = comparisons.iter().filter(|c| c.has_ground_truth).collect();

    let queries_with_ground_truth = comparisons_with_gt.len();

    // NDCG-based quality metrics (only for queries with ground truth)
    let avg_baseline_ndcg = if queries_with_ground_truth > 0 {
        comparisons_with_gt
            .iter()
            .map(|c| c.baseline_ndcg)
            .sum::<f64>()
            / queries_with_ground_truth as f64
    } else {
        0.0
    };

    let avg_reranking_ndcg = if queries_with_ground_truth > 0 {
        comparisons_with_gt
            .iter()
            .map(|c| c.reranking_ndcg)
            .sum::<f64>()
            / queries_with_ground_truth as f64
    } else {
        0.0
    };

    let ndcg_improvement = avg_reranking_ndcg - avg_baseline_ndcg;

    let queries_with_ndcg_improvement = comparisons_with_gt
        .iter()
        .filter(|c| c.ndcg_improvement > 0.001)
        .count();

    let queries_with_ndcg_degradation = comparisons_with_gt
        .iter()
        .filter(|c| c.ndcg_improvement < -0.001)
        .count();

    let queries_with_ndcg_unchanged = comparisons_with_gt
        .iter()
        .filter(|c| c.ndcg_improvement.abs() <= 0.001)
        .count();

    // Latency metrics
    let average_baseline_latency_ms = comparisons
        .iter()
        .map(|c| c.baseline_latency_ms as f64)
        .sum::<f64>()
        / comparisons.len().max(1) as f64;

    let average_reranking_latency_ms = comparisons
        .iter()
        .map(|c| c.reranking_latency_ms as f64)
        .sum::<f64>()
        / comparisons.len().max(1) as f64;

    let latency_overhead_percent = ((average_reranking_latency_ms - average_baseline_latency_ms)
        / average_baseline_latency_ms)
        * 100.0;

    Ok(EvaluationReport {
        total_queries,
        queries_with_ground_truth,
        avg_baseline_ndcg,
        avg_reranking_ndcg,
        ndcg_improvement,
        queries_with_ndcg_improvement,
        queries_with_ndcg_degradation,
        queries_with_ndcg_unchanged,
        average_baseline_latency_ms,
        average_reranking_latency_ms,
        latency_overhead_percent,
        query_comparisons: comparisons,
    })
}

/// Print evaluation report to console
fn print_report(report: &EvaluationReport) {
    println!("\n=== Evaluation Report ===\n");
    println!("Total Queries: {}", report.total_queries);
    println!(
        "Queries with Ground Truth: {} ({:.1}%)",
        report.queries_with_ground_truth,
        (report.queries_with_ground_truth as f64 / report.total_queries as f64) * 100.0
    );

    if report.queries_with_ground_truth == 0 {
        println!("\nNo ground truth labels found. Please run the labeling tool first:");
        println!("  cargo test test_generate_ground_truth_labels -- --ignored --nocapture");
        return;
    }

    println!("\n--- NDCG@10 Metrics ---");
    println!("Average Baseline NDCG@10: {:.4}", report.avg_baseline_ndcg);
    println!(
        "Average Reranking NDCG@10: {:.4}",
        report.avg_reranking_ndcg
    );
    println!("NDCG Improvement: {:+.4}", report.ndcg_improvement);

    println!(
        "\nQueries with NDCG Improvement: {} ({:.1}%)",
        report.queries_with_ndcg_improvement,
        (report.queries_with_ndcg_improvement as f64 / report.queries_with_ground_truth as f64)
            * 100.0
    );
    println!(
        "Queries with NDCG Degradation: {} ({:.1}%)",
        report.queries_with_ndcg_degradation,
        (report.queries_with_ndcg_degradation as f64 / report.queries_with_ground_truth as f64)
            * 100.0
    );
    println!(
        "Queries with NDCG Unchanged: {} ({:.1}%)",
        report.queries_with_ndcg_unchanged,
        (report.queries_with_ndcg_unchanged as f64 / report.queries_with_ground_truth as f64)
            * 100.0
    );

    println!("\n--- Latency Metrics ---");
    println!(
        "Average Baseline Latency: {:.1}ms",
        report.average_baseline_latency_ms
    );
    println!(
        "Average Reranking Latency: {:.1}ms",
        report.average_reranking_latency_ms
    );
    println!("Latency Overhead: {:.1}%", report.latency_overhead_percent);

    println!("\n=== Top 10 NDCG Improvements ===");
    let mut sorted_improvements: Vec<_> = report
        .query_comparisons
        .iter()
        .filter(|c| c.has_ground_truth && c.ndcg_improvement > 0.001)
        .collect();
    sorted_improvements.sort_by(|a, b| {
        b.ndcg_improvement
            .partial_cmp(&a.ndcg_improvement)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    for comp in sorted_improvements.iter().take(10) {
        println!(
            "  {:.4} -> {:.4} (improvement: +{:.4}) - {}",
            comp.baseline_ndcg, comp.reranking_ndcg, comp.ndcg_improvement, comp.query
        );
    }

    println!("\n=== Top 10 NDCG Degradations ===");
    let mut sorted_degradations: Vec<_> = report
        .query_comparisons
        .iter()
        .filter(|c| c.has_ground_truth && c.ndcg_improvement < -0.001)
        .collect();
    sorted_degradations.sort_by(|a, b| {
        a.ndcg_improvement
            .partial_cmp(&b.ndcg_improvement)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    for comp in sorted_degradations.iter().take(10) {
        println!(
            "  {:.4} -> {:.4} (degradation: {:.4}) - {}",
            comp.baseline_ndcg, comp.reranking_ndcg, comp.ndcg_improvement, comp.query
        );
    }
}

/// Save report to JSON file
fn save_report(report: &EvaluationReport, path: &str) -> Result<()> {
    let json = serde_json::to_string_pretty(report).context("Failed to serialize report")?;
    std::fs::write(path, json).context("Failed to write report file")?;
    println!("\nReport saved to: {path}");
    Ok(())
}

/// Calculate NDCG@k (Normalized Discounted Cumulative Gain)
///
/// NDCG measures ranking quality by considering both relevance and position.
/// Higher scores indicate better rankings.
///
/// Formula:
/// - DCG = Σ (2^relevance - 1) / log2(position + 1)
/// - IDCG = DCG of ideal ranking (sorted by relevance descending)
/// - NDCG = DCG / IDCG
///
/// # Arguments
/// * `results` - Search results in ranked order
/// * `ground_truth` - Map of entity_id -> relevance score (0-3)
/// * `k` - Number of top results to consider
///
/// # Returns
/// NDCG@k score in range [0, 1], where 1 is perfect ranking
fn calculate_ndcg_at_k(
    results: &[SearchResult],
    ground_truth: &HashMap<String, u8>,
    k: usize,
) -> f64 {
    if results.is_empty() || ground_truth.is_empty() {
        return 0.0;
    }

    // Calculate DCG for actual ranking
    let mut dcg = 0.0;
    for (i, result) in results.iter().take(k).enumerate() {
        if let Some(&relevance) = ground_truth.get(&result.entity_id) {
            let gain = (2_f64.powi(relevance as i32)) - 1.0;
            let discount = (i as f64 + 2.0).log2(); // position starts at 1, so i+2
            dcg += gain / discount;
        }
    }

    // Calculate IDCG (ideal DCG with perfect ranking)
    let mut relevances: Vec<u8> = ground_truth.values().copied().collect();
    relevances.sort_by(|a, b| b.cmp(a)); // Sort descending

    let mut idcg = 0.0;
    for (i, &relevance) in relevances.iter().take(k).enumerate() {
        let gain = (2_f64.powi(relevance as i32)) - 1.0;
        let discount = (i as f64 + 2.0).log2();
        idcg += gain / discount;
    }

    // Return NDCG
    if idcg == 0.0 {
        0.0
    } else {
        dcg / idcg
    }
}

/// Load ground truth labels from JSON file
fn load_ground_truth(path: &Path) -> Result<GroundTruthDataset> {
    if !path.exists() {
        // Return empty dataset if file doesn't exist
        return Ok(GroundTruthDataset { labels: Vec::new() });
    }

    let json = std::fs::read_to_string(path).context(format!(
        "Failed to read ground truth file: {}",
        path.display()
    ))?;

    let dataset: GroundTruthDataset =
        serde_json::from_str(&json).context("Failed to parse ground truth JSON")?;

    Ok(dataset)
}

/// Save ground truth labels to JSON file
fn save_ground_truth(dataset: &GroundTruthDataset, path: &Path) -> Result<()> {
    let json = serde_json::to_string_pretty(dataset)
        .context("Failed to serialize ground truth dataset")?;

    // Ensure parent directory exists
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .context(format!("Failed to create directory: {}", parent.display()))?;
    }

    std::fs::write(path, json).context(format!(
        "Failed to write ground truth file: {}",
        path.display()
    ))?;

    println!("Ground truth saved to: {}", path.display());
    Ok(())
}

/// Tool to collect queries and results for Claude to label
#[tokio::test]
#[ignore]
async fn test_collect_labeling_data() -> Result<()> {
    println!("\n=== Collecting Query/Result Data for Labeling ===\n");

    // Load config
    let config_path = global_config_path()?;
    let config = Config::from_file(&config_path).context("Failed to load config")?;

    // Get workspace root
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let workspace_root = std::path::Path::new(manifest_dir)
        .parent()
        .context("tests directory should have a parent")?;
    let repo_path = workspace_root.to_path_buf();

    // Look up repository
    let postgres_client = create_postgres_client(&config.storage).await?;
    let (repository_id, collection_name) = postgres_client
        .get_repository_by_path(&repo_path)
        .await?
        .context("Repository not indexed. Run 'codesearch index' first.")?;

    println!("Found repository: {repository_id}");
    println!("Collection name: {collection_name}\n");

    // Generate queries (50 queries for labeling)
    let query_generator = QueryGenerator::new(postgres_client.clone(), repository_id);
    let queries = query_generator.generate_queries(50).await?;

    println!("Generated {} queries\n", queries.len());

    // Create search executor
    let search_executor =
        SearchExecutor::new(&config, repository_id, &collection_name, false, false, 5).await?;

    // Collect all queries and results
    let output_path = std::path::Path::new(manifest_dir).join("data/labeling_data.json");

    #[derive(Serialize)]
    struct LabelingData {
        query: String,
        query_type: QueryType,
        results: Vec<LabelingResult>,
    }

    #[derive(Serialize)]
    struct LabelingResult {
        entity_id: String,
        entity_name: String,
        score: f32,
    }

    let mut all_data = Vec::new();

    for (idx, query) in queries.iter().enumerate() {
        println!(
            "[{}/{}] {:?}  Processing: \"{}\"",
            idx + 1,
            queries.len(),
            query.query_type,
            query.query_text
        );

        // Run search to get top 20 results
        let (results, _) = search_executor.search(&query.query_text, 20, false).await?;

        if results.is_empty() {
            println!("  No results, skipping\n");
            continue;
        }

        let labeling_results: Vec<LabelingResult> = results
            .iter()
            .map(|r| LabelingResult {
                entity_id: r.entity_id.clone(),
                entity_name: r.entity_name.clone(),
                score: r.score,
            })
            .collect();

        all_data.push(LabelingData {
            query: query.query_text.clone(),
            query_type: query.query_type,
            results: labeling_results,
        });
    }

    // Save to JSON
    let json = serde_json::to_string_pretty(&all_data)?;
    std::fs::write(&output_path, json)?;

    println!("\n{}", "=".repeat(60));
    println!("Data collection complete!");
    println!("Collected {} queries with results", all_data.len());
    println!("Data saved to: {}", output_path.display());
    println!("{}\n", "=".repeat(60));

    Ok(())
}

#[tokio::test]
#[ignore]
async fn test_reranking_evaluation() -> Result<()> {
    // Load config
    let config_path = global_config_path()?;
    let config = Config::from_file(&config_path).context("Failed to load config")?;

    // Get workspace root (where the repository should be indexed)
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let workspace_root = std::path::Path::new(manifest_dir)
        .parent()
        .context("tests directory should have a parent")?;
    let repo_path = workspace_root.to_path_buf();

    // Prepare report output path
    let report_path = workspace_root.join("target/reranking_evaluation_report.json");

    // Look up repository in database
    let postgres_client = create_postgres_client(&config.storage).await?;
    let (repository_id, collection_name) = postgres_client
        .get_repository_by_path(&repo_path)
        .await?
        .context("Repository not indexed. Run 'codesearch index' first.")?;

    println!("Found repository: {repository_id}");
    println!("Collection name: {collection_name}");

    // Run evaluation with balanced mix of query types
    let target_query_count = 100;
    let report =
        evaluate_reranking(&config, repository_id, &collection_name, target_query_count).await?;

    // Print and save report
    print_report(&report);
    save_report(
        &report,
        report_path.to_str().context("Invalid report path")?,
    )?;

    Ok(())
}
