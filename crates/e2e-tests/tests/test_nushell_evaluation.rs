//! Nushell Code Search Evaluation Suite
//!
//! This test evaluates the effectiveness of codesearch against the Nushell codebase,
//! which provides excellent ground truth opportunities due to its rich trait hierarchy
//! (207+ Command implementations) and well-structured module organization.
//!
//! Run with:
//!   # Setup (clone and index Nushell)
//!   ./scripts/setup_nushell_eval.sh
//!
//!   # Run evaluation
//!   cargo test --manifest-path crates/e2e-tests/Cargo.toml --test test_nushell_evaluation -- --ignored --nocapture
//!
//!   # Quick iteration (limit to first N samples)
//!   SAMPLE_LIMIT=5 cargo test --manifest-path crates/e2e-tests/Cargo.toml --test test_nushell_evaluation -- --ignored --nocapture
//!
//! Success Criteria (from Issue #121):
//!   - Semantic retrieval precision@5: >80%
//!   - Graph traversal accuracy: >90%
//!   - Cross-file resolution bound rate: >95%

#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]

use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Instant;

const API_BASE_URL: &str = "http://127.0.0.1:3000";
const NUSHELL_REPO_PATH: &str = "/tmp/nushell-eval";

// === Query and Dataset Types ===

/// LSP query metadata for ground truth extraction
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
struct LspQueryMetadata {
    method: Option<String>,
    target: Option<String>,
    #[serde(rename = "type")]
    query_type: Option<String>,
    directory: Option<String>,
    module: Option<String>,
}

/// Query from the evaluation dataset
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
struct EvalQuery {
    id: String,
    category: String,
    query: String,
    #[serde(default)]
    expected: Vec<String>,
    #[serde(default)]
    expected_count_gte: Option<usize>,
    #[serde(default)]
    expected_contains: Vec<String>,
    #[serde(default)]
    expected_entities: Vec<String>,
    relationship_type: Option<String>,
    #[serde(default)]
    relationship_chain: Vec<String>,
    lsp_query: Option<LspQueryMetadata>,
    notes: Option<String>,
}

/// Evaluation dataset structure
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct EvalDataset {
    repository: String,
    repository_path: String,
    repository_commit: Option<String>,
    description: String,
    queries: Vec<EvalQuery>,
}

// === Request/Response Types ===

#[derive(Debug, Serialize)]
struct SemanticSearchRequest {
    query: QuerySpec,
    #[serde(skip_serializing_if = "Option::is_none")]
    repository_ids: Option<Vec<String>>,
    limit: usize,
}

#[derive(Debug, Serialize)]
struct FulltextSearchRequest {
    repository_id: String,
    query: String,
    limit: usize,
}

#[derive(Debug, Serialize)]
struct UnifiedSearchRequest {
    repository_id: String,
    query: QuerySpec,
    limit: usize,
    enable_fulltext: bool,
    enable_semantic: bool,
}

#[derive(Debug, Serialize)]
struct QuerySpec {
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    instruction: Option<String>,
}

/// Entity result from API
#[derive(Debug, Deserialize)]
struct EntityResult {
    #[allow(dead_code)]
    entity_id: String,
    #[allow(dead_code)]
    name: String,
    qualified_name: String,
    #[allow(dead_code)]
    entity_type: String,
    #[allow(dead_code)]
    score: f32,
    #[allow(dead_code)]
    file_path: String,
}

#[derive(Debug, Deserialize)]
struct SemanticSearchResponse {
    results: Vec<EntityResult>,
    #[allow(dead_code)]
    metadata: SemanticMetadata,
}

#[derive(Debug, Deserialize)]
struct SemanticMetadata {
    #[allow(dead_code)]
    total_results: usize,
    #[allow(dead_code)]
    query_time_ms: u64,
}

#[derive(Debug, Deserialize)]
struct FulltextSearchResponse {
    results: Vec<EntityResult>,
    #[allow(dead_code)]
    metadata: FulltextMetadata,
}

#[derive(Debug, Deserialize)]
struct FulltextMetadata {
    #[allow(dead_code)]
    total_results: usize,
    #[allow(dead_code)]
    query_time_ms: u64,
}

#[derive(Debug, Deserialize)]
struct UnifiedSearchResponse {
    results: Vec<EntityResult>,
    #[allow(dead_code)]
    metadata: UnifiedSearchMetadata,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct UnifiedSearchMetadata {
    #[serde(default)]
    total_results: usize,
    #[serde(default)]
    fulltext_count: usize,
    #[serde(default)]
    semantic_count: usize,
    #[serde(default)]
    merged_via_rrf: bool,
    #[serde(default)]
    reranked: bool,
    #[serde(default)]
    query_time_ms: u64,
}

// === Metrics Types ===

/// Metrics for structural (exhaustive, unranked) queries
#[derive(Debug, Clone, Serialize, Default)]
struct StructuralMetrics {
    set_coverage: Option<f64>,
    set_precision: Option<f64>,
    f1_score: Option<f64>,
    contains_coverage: Option<f64>,
    returned_count: usize,
}

/// Result from a single search type for a query
#[derive(Debug, Clone, Serialize)]
struct SearchTypeResult {
    search_type: String,
    query_time_ms: u64,
    num_results: usize,
    // Ranked metrics
    recall_at_5: Option<f64>,
    recall_at_10: Option<f64>,
    precision_at_5: Option<f64>,
    mrr: Option<f64>,
    // Structural metrics
    structural_metrics: Option<StructuralMetrics>,
    expected_found: usize,
    expected_total: usize,
    top_results: Vec<String>,
    error: Option<String>,
}

/// Results for a single query across all search types
#[derive(Debug, Clone, Serialize)]
struct QueryMetrics {
    query_id: String,
    category: String,
    query_type: String,
    results_by_type: HashMap<String, SearchTypeResult>,
}

/// Aggregated metrics for a search type
#[derive(Debug, Clone, Serialize, Default)]
struct SearchTypeAggregate {
    query_count: usize,
    avg_recall_at_5: Option<f64>,
    avg_recall_at_10: Option<f64>,
    avg_precision_at_5: Option<f64>,
    avg_mrr: Option<f64>,
    avg_query_time_ms: f64,
    // Structural aggregates
    avg_set_coverage: Option<f64>,
    avg_set_precision: Option<f64>,
    avg_f1_score: Option<f64>,
    avg_contains_coverage: Option<f64>,
}

/// Metrics grouped by category
#[derive(Debug, Clone, Serialize)]
struct CategoryMetrics {
    category: String,
    query_count: usize,
    by_search_type: HashMap<String, SearchTypeAggregate>,
}

/// Query type specific aggregates
#[derive(Debug, Clone, Serialize, Default)]
struct QueryTypeAggregate {
    query_count: usize,
    // Ranked metrics
    avg_recall_at_5: Option<f64>,
    avg_recall_at_10: Option<f64>,
    avg_precision_at_5: Option<f64>,
    avg_mrr: Option<f64>,
    // Structural metrics
    avg_set_coverage: Option<f64>,
    avg_set_precision: Option<f64>,
    avg_f1_score: Option<f64>,
    avg_contains_coverage: Option<f64>,
    avg_query_time_ms: f64,
}

/// Complete evaluation report
#[derive(Debug, Serialize)]
struct EvaluationReport {
    repository: String,
    total_queries: usize,
    search_types_evaluated: Vec<String>,
    search_type_metrics: HashMap<String, SearchTypeAggregate>,
    query_type_metrics: HashMap<String, HashMap<String, QueryTypeAggregate>>,
    category_metrics: Vec<CategoryMetrics>,
    query_results: Vec<QueryMetrics>,
    // Success criteria
    success_criteria: SuccessCriteria,
}

#[derive(Debug, Clone, Serialize)]
struct SuccessCriteria {
    semantic_precision_at_5_target: f64,
    semantic_precision_at_5_actual: Option<f64>,
    semantic_precision_at_5_pass: bool,
    graph_accuracy_target: f64,
    graph_accuracy_actual: Option<f64>,
    graph_accuracy_pass: bool,
}

// === Helper Functions ===

/// Determine if a query category is structural (exhaustive) or ranked
fn get_query_type(category: &str) -> &'static str {
    match category {
        "semantic" | "discovery" => "ranked",
        _ => "structural",
    }
}

/// Normalize a qualified name for comparison
fn normalize_qualified_name(name: &str) -> String {
    let name = name.to_lowercase();
    // Handle impl blocks like "impl Foo at line 123"
    if let Some(pos) = name.find(" at line ") {
        return name[..pos].to_string();
    }
    // Handle "impl TypeName at line X" format
    if name.starts_with("impl ") {
        if let Some(pos) = name.find(" at ") {
            return name[5..pos].to_string();
        }
    }
    name
}

/// Calculate recall@k for ranked queries
fn recall_at_k(results: &[EntityResult], expected: &[String], k: usize) -> Option<f64> {
    if expected.is_empty() {
        return None;
    }

    let top_k: Vec<_> = results.iter().take(k).collect();
    let expected_normalized: Vec<String> = expected.iter().map(|e| normalize_qualified_name(e)).collect();

    let mut found_count = 0;
    for exp in &expected_normalized {
        for result in &top_k {
            let result_norm = normalize_qualified_name(&result.qualified_name);
            if result_norm.contains(exp) || exp.contains(&result_norm) {
                found_count += 1;
                break;
            }
        }
    }

    Some(found_count as f64 / expected.len() as f64)
}

/// Calculate precision@k for ranked queries
fn precision_at_k(results: &[EntityResult], expected: &[String], k: usize) -> Option<f64> {
    if expected.is_empty() || results.is_empty() {
        return None;
    }

    let top_k: Vec<_> = results.iter().take(k).collect();
    let expected_normalized: Vec<String> = expected.iter().map(|e| normalize_qualified_name(e)).collect();

    let mut relevant_count = 0;
    for result in &top_k {
        let result_norm = normalize_qualified_name(&result.qualified_name);
        for exp in &expected_normalized {
            if result_norm.contains(exp) || exp.contains(&result_norm) {
                relevant_count += 1;
                break;
            }
        }
    }

    Some(relevant_count as f64 / k.min(top_k.len()) as f64)
}

/// Calculate MRR (Mean Reciprocal Rank)
fn calculate_mrr(results: &[EntityResult], expected: &[String]) -> Option<f64> {
    if expected.is_empty() {
        return None;
    }

    let expected_normalized: Vec<String> = expected.iter().map(|e| normalize_qualified_name(e)).collect();

    for (rank, result) in results.iter().enumerate() {
        let result_norm = normalize_qualified_name(&result.qualified_name);
        for exp in &expected_normalized {
            if result_norm.contains(exp) || exp.contains(&result_norm) {
                return Some(1.0 / (rank as f64 + 1.0));
            }
        }
    }

    Some(0.0)
}

/// Calculate set coverage for structural queries
fn calculate_set_coverage(results: &[EntityResult], expected: &[String]) -> Option<f64> {
    if expected.is_empty() {
        return None;
    }

    let expected_normalized: Vec<String> = expected.iter().map(|e| normalize_qualified_name(e)).collect();
    let results_normalized: Vec<String> = results.iter().map(|r| normalize_qualified_name(&r.qualified_name)).collect();

    let mut found_count = 0;
    for exp in &expected_normalized {
        for result in &results_normalized {
            if result.contains(exp) || exp.contains(result) {
                found_count += 1;
                break;
            }
        }
    }

    Some(found_count as f64 / expected.len() as f64)
}

/// Calculate set precision for structural queries
fn calculate_set_precision(results: &[EntityResult], expected: &[String]) -> Option<f64> {
    if expected.is_empty() || results.is_empty() {
        return None;
    }

    let expected_normalized: Vec<String> = expected.iter().map(|e| normalize_qualified_name(e)).collect();
    let results_normalized: Vec<String> = results.iter().map(|r| normalize_qualified_name(&r.qualified_name)).collect();

    let mut found_count = 0;
    for result in &results_normalized {
        for exp in &expected_normalized {
            if result.contains(exp) || exp.contains(result) {
                found_count += 1;
                break;
            }
        }
    }

    Some(found_count as f64 / results.len() as f64)
}

/// Calculate contains coverage
fn calculate_contains_coverage(results: &[EntityResult], expected_contains: &[String]) -> Option<f64> {
    if expected_contains.is_empty() {
        return None;
    }

    let expected_normalized: Vec<String> = expected_contains.iter().map(|e| normalize_qualified_name(e)).collect();
    let results_normalized: Vec<String> = results.iter().map(|r| normalize_qualified_name(&r.qualified_name)).collect();

    let mut found_count = 0;
    for exp in &expected_normalized {
        for result in &results_normalized {
            if result.contains(exp) || exp.contains(result) {
                found_count += 1;
                break;
            }
        }
    }

    Some(found_count as f64 / expected_contains.len() as f64)
}

// === Search Execution ===

async fn execute_semantic_search(
    client: &Client,
    query: &str,
    repository_id: &str,
) -> Result<(Vec<EntityResult>, u64)> {
    let request = SemanticSearchRequest {
        query: QuerySpec {
            text: query.to_string(),
            instruction: None,
        },
        repository_ids: Some(vec![repository_id.to_string()]),
        limit: 20,
    };

    let start = Instant::now();
    let response = client
        .post(format!("{API_BASE_URL}/api/v1/search/semantic"))
        .json(&request)
        .send()
        .await?;

    let elapsed = start.elapsed().as_millis() as u64;
    let parsed: SemanticSearchResponse = response.json().await?;
    Ok((parsed.results, elapsed))
}

async fn execute_fulltext_search(
    client: &Client,
    query: &str,
    repository_id: &str,
) -> Result<(Vec<EntityResult>, u64)> {
    let request = FulltextSearchRequest {
        repository_id: repository_id.to_string(),
        query: query.to_string(),
        limit: 20,
    };

    let start = Instant::now();
    let response = client
        .post(format!("{API_BASE_URL}/api/v1/search/fulltext"))
        .json(&request)
        .send()
        .await?;

    let elapsed = start.elapsed().as_millis() as u64;
    let parsed: FulltextSearchResponse = response.json().await?;
    Ok((parsed.results, elapsed))
}

async fn execute_unified_search(
    client: &Client,
    query: &str,
    repository_id: &str,
) -> Result<(Vec<EntityResult>, u64)> {
    let request = UnifiedSearchRequest {
        repository_id: repository_id.to_string(),
        query: QuerySpec {
            text: query.to_string(),
            instruction: None,
        },
        limit: 20,
        enable_fulltext: true,
        enable_semantic: true,
    };

    let start = Instant::now();
    let response = client
        .post(format!("{API_BASE_URL}/api/v1/search/unified"))
        .json(&request)
        .send()
        .await?;

    let elapsed = start.elapsed().as_millis() as u64;
    let parsed: UnifiedSearchResponse = response.json().await?;
    Ok((parsed.results, elapsed))
}

/// Evaluate a query against a specific search type
async fn evaluate_query_search_type(
    client: &Client,
    query: &EvalQuery,
    repository_id: &str,
    search_type: &str,
) -> SearchTypeResult {
    let search_result = match search_type {
        "semantic" => execute_semantic_search(client, &query.query, repository_id).await,
        "fulltext" => execute_fulltext_search(client, &query.query, repository_id).await,
        "unified" => execute_unified_search(client, &query.query, repository_id).await,
        _ => Err(anyhow::anyhow!("Unknown search type: {}", search_type)),
    };

    match search_result {
        Ok((results, query_time_ms)) => {
            let query_type = get_query_type(&query.category);

            // Get expected entities (try expected_entities first for semantic queries, fall back to expected)
            let expected = if !query.expected_entities.is_empty() {
                &query.expected_entities
            } else {
                &query.expected
            };

            let (recall_at_5, recall_at_10, precision_at_5, mrr) = if query_type == "ranked" {
                (
                    recall_at_k(&results, expected, 5),
                    recall_at_k(&results, expected, 10),
                    precision_at_k(&results, expected, 5),
                    calculate_mrr(&results, expected),
                )
            } else {
                (None, None, None, None)
            };

            let structural_metrics = if query_type == "structural" {
                // Use expected_contains as fallback when expected is empty
                let effective_expected = if expected.is_empty() && !query.expected_contains.is_empty() {
                    &query.expected_contains
                } else {
                    expected
                };

                let set_coverage = calculate_set_coverage(&results, effective_expected);
                let set_precision = calculate_set_precision(&results, effective_expected);
                let f1_score = match (set_coverage, set_precision) {
                    (Some(cov), Some(prec)) if cov + prec > 0.0 => {
                        Some(2.0 * cov * prec / (cov + prec))
                    }
                    _ => None,
                };
                let contains_coverage = calculate_contains_coverage(&results, &query.expected_contains);

                Some(StructuralMetrics {
                    set_coverage,
                    set_precision,
                    f1_score,
                    contains_coverage,
                    returned_count: results.len(),
                })
            } else {
                None
            };

            let expected_found = expected
                .iter()
                .filter(|exp| {
                    let exp_norm = normalize_qualified_name(exp);
                    results.iter().any(|r| {
                        let r_norm = normalize_qualified_name(&r.qualified_name);
                        r_norm.contains(&exp_norm) || exp_norm.contains(&r_norm)
                    })
                })
                .count();

            SearchTypeResult {
                search_type: search_type.to_string(),
                query_time_ms,
                num_results: results.len(),
                recall_at_5,
                recall_at_10,
                precision_at_5,
                mrr,
                structural_metrics,
                expected_found,
                expected_total: expected.len(),
                top_results: results.iter().take(5).map(|r| r.qualified_name.clone()).collect(),
                error: None,
            }
        }
        Err(e) => SearchTypeResult {
            search_type: search_type.to_string(),
            query_time_ms: 0,
            num_results: 0,
            recall_at_5: None,
            recall_at_10: None,
            precision_at_5: None,
            mrr: None,
            structural_metrics: None,
            expected_found: 0,
            expected_total: query.expected.len(),
            top_results: vec![],
            error: Some(e.to_string()),
        },
    }
}

/// Evaluate a query against all search types
async fn evaluate_query_all_types(
    client: &Client,
    query: &EvalQuery,
    repository_id: &str,
) -> QueryMetrics {
    let search_types = vec!["semantic", "fulltext", "unified"];
    let mut results_by_type = HashMap::new();

    for st in search_types {
        let result = evaluate_query_search_type(client, query, repository_id, st).await;
        results_by_type.insert(st.to_string(), result);
    }

    QueryMetrics {
        query_id: query.id.clone(),
        category: query.category.clone(),
        query_type: get_query_type(&query.category).to_string(),
        results_by_type,
    }
}

// === Aggregation Functions ===

fn aggregate_search_type_metrics(results: &[QueryMetrics], search_type: &str) -> SearchTypeAggregate {
    let mut recall_5_values = Vec::new();
    let mut recall_10_values = Vec::new();
    let mut precision_5_values = Vec::new();
    let mut mrr_values = Vec::new();
    let mut set_coverage_values = Vec::new();
    let mut set_precision_values = Vec::new();
    let mut f1_values = Vec::new();
    let mut contains_coverage_values = Vec::new();
    let mut query_times = Vec::new();

    for result in results {
        if let Some(st_result) = result.results_by_type.get(search_type) {
            query_times.push(st_result.query_time_ms as f64);

            if let Some(v) = st_result.recall_at_5 {
                recall_5_values.push(v);
            }
            if let Some(v) = st_result.recall_at_10 {
                recall_10_values.push(v);
            }
            if let Some(v) = st_result.precision_at_5 {
                precision_5_values.push(v);
            }
            if let Some(v) = st_result.mrr {
                mrr_values.push(v);
            }

            if let Some(ref sm) = st_result.structural_metrics {
                if let Some(v) = sm.set_coverage {
                    set_coverage_values.push(v);
                }
                if let Some(v) = sm.set_precision {
                    set_precision_values.push(v);
                }
                if let Some(v) = sm.f1_score {
                    f1_values.push(v);
                }
                if let Some(v) = sm.contains_coverage {
                    contains_coverage_values.push(v);
                }
            }
        }
    }

    let avg = |values: &[f64]| -> Option<f64> {
        if values.is_empty() {
            None
        } else {
            Some(values.iter().sum::<f64>() / values.len() as f64)
        }
    };

    SearchTypeAggregate {
        query_count: results.len(),
        avg_recall_at_5: avg(&recall_5_values),
        avg_recall_at_10: avg(&recall_10_values),
        avg_precision_at_5: avg(&precision_5_values),
        avg_mrr: avg(&mrr_values),
        avg_query_time_ms: query_times.iter().sum::<f64>() / query_times.len().max(1) as f64,
        avg_set_coverage: avg(&set_coverage_values),
        avg_set_precision: avg(&set_precision_values),
        avg_f1_score: avg(&f1_values),
        avg_contains_coverage: avg(&contains_coverage_values),
    }
}

fn aggregate_by_query_type(
    results: &[QueryMetrics],
    search_type: &str,
) -> HashMap<String, QueryTypeAggregate> {
    let mut structural_results: Vec<&QueryMetrics> = Vec::new();
    let mut ranked_results: Vec<&QueryMetrics> = Vec::new();

    for result in results {
        if result.query_type == "structural" {
            structural_results.push(result);
        } else {
            ranked_results.push(result);
        }
    }

    let mut aggregates = HashMap::new();

    // Structural
    if !structural_results.is_empty() {
        let agg = aggregate_search_type_metrics(
            &structural_results.iter().cloned().cloned().collect::<Vec<_>>(),
            search_type,
        );
        aggregates.insert(
            "structural".to_string(),
            QueryTypeAggregate {
                query_count: structural_results.len(),
                avg_recall_at_5: None,
                avg_recall_at_10: None,
                avg_precision_at_5: None,
                avg_mrr: None,
                avg_set_coverage: agg.avg_set_coverage,
                avg_set_precision: agg.avg_set_precision,
                avg_f1_score: agg.avg_f1_score,
                avg_contains_coverage: agg.avg_contains_coverage,
                avg_query_time_ms: agg.avg_query_time_ms,
            },
        );
    }

    // Ranked
    if !ranked_results.is_empty() {
        let agg = aggregate_search_type_metrics(
            &ranked_results.iter().cloned().cloned().collect::<Vec<_>>(),
            search_type,
        );
        aggregates.insert(
            "ranked".to_string(),
            QueryTypeAggregate {
                query_count: ranked_results.len(),
                avg_recall_at_5: agg.avg_recall_at_5,
                avg_recall_at_10: agg.avg_recall_at_10,
                avg_precision_at_5: agg.avg_precision_at_5,
                avg_mrr: agg.avg_mrr,
                avg_set_coverage: None,
                avg_set_precision: None,
                avg_f1_score: None,
                avg_contains_coverage: None,
                avg_query_time_ms: agg.avg_query_time_ms,
            },
        );
    }

    aggregates
}

fn group_by_category(results: &[QueryMetrics], search_types: &[&str]) -> Vec<CategoryMetrics> {
    let mut by_category: HashMap<String, Vec<&QueryMetrics>> = HashMap::new();

    for result in results {
        by_category
            .entry(result.category.clone())
            .or_default()
            .push(result);
    }

    let mut category_metrics = Vec::new();
    for (category, cat_results) in by_category {
        let mut by_search_type = HashMap::new();
        for st in search_types {
            let agg = aggregate_search_type_metrics(
                &cat_results.iter().cloned().cloned().collect::<Vec<_>>(),
                st,
            );
            by_search_type.insert(st.to_string(), agg);
        }
        category_metrics.push(CategoryMetrics {
            category,
            query_count: cat_results.len(),
            by_search_type,
        });
    }

    category_metrics.sort_by(|a, b| a.category.cmp(&b.category));
    category_metrics
}

// === Environment Helpers ===

fn get_sample_limit() -> Option<usize> {
    std::env::var("SAMPLE_LIMIT")
        .ok()
        .and_then(|v| v.parse().ok())
}

// === Main Test ===

#[tokio::test]
#[ignore]
async fn test_nushell_evaluation() -> Result<()> {
    // Check if Nushell is set up
    if !std::path::Path::new(NUSHELL_REPO_PATH).exists() {
        anyhow::bail!(
            "Nushell repository not found at {}.\n\
             Run: ./scripts/setup_nushell_eval.sh",
            NUSHELL_REPO_PATH
        );
    }

    // Load evaluation dataset
    let dataset_path = "fixtures/nushell_eval_queries.json";
    let dataset_content = std::fs::read_to_string(dataset_path)
        .context("Failed to read evaluation dataset. Run from crates/e2e-tests directory.")?;
    let dataset: EvalDataset =
        serde_json::from_str(&dataset_content).context("Failed to parse evaluation dataset")?;

    println!("\n=== Nushell Code Search Evaluation ===");
    println!("Repository: {}", dataset.repository);
    if let Some(commit) = &dataset.repository_commit {
        println!("Commit: {}", commit);
    }
    println!("Total queries: {}\n", dataset.queries.len());

    let client = Client::new();

    // Get repository ID
    let repos_response = client
        .get(format!("{API_BASE_URL}/api/v1/repositories"))
        .send()
        .await
        .context("Failed to get repositories. Is the server running?")?;

    let repos: serde_json::Value = repos_response.json().await?;
    let repository_id = repos
        .get("repositories")
        .and_then(|arr| arr.as_array())
        .and_then(|arr| {
            arr.iter().find(|r| {
                r.get("repository_path")
                    .and_then(|p| p.as_str())
                    .map(|p| p.contains("nushell"))
                    .unwrap_or(false)
            })
        })
        .and_then(|r| r.get("repository_id"))
        .and_then(|id| id.as_str())
        .context("Nushell repository not found in codesearch. Run: ./scripts/setup_nushell_eval.sh")?;

    println!("Repository ID: {}\n", repository_id);

    let search_types = vec!["semantic", "fulltext", "unified"];
    println!("Search types to evaluate: {:?}\n", search_types);

    // Check for sample limit
    let sample_limit = get_sample_limit();
    let queries_to_run: Vec<_> = match sample_limit {
        Some(limit) => {
            println!(
                "Sample limit: {} (running {} of {} queries)\n",
                limit,
                limit.min(dataset.queries.len()),
                dataset.queries.len()
            );
            dataset.queries.iter().take(limit).collect()
        }
        None => {
            println!("Sample limit: none (running all {} queries)\n", dataset.queries.len());
            dataset.queries.iter().collect()
        }
    };

    // Evaluate each query
    let mut results = Vec::new();
    let total_queries = queries_to_run.len();

    for (i, query) in queries_to_run.iter().enumerate() {
        print!(
            "[{}/{}] Evaluating {} ({})... ",
            i + 1,
            total_queries,
            query.id,
            query.category
        );

        let metrics = evaluate_query_all_types(&client, query, repository_id).await;

        // Print summary
        let unified_result = metrics.results_by_type.get("unified");
        let metric_str = if metrics.query_type == "ranked" {
            unified_result
                .and_then(|r| r.recall_at_5)
                .map(|r| format!("recall@5={:.2}", r))
                .unwrap_or_else(|| "N/A".to_string())
        } else {
            unified_result
                .and_then(|r| r.structural_metrics.as_ref())
                .and_then(|sm| sm.set_coverage)
                .map(|c| format!("coverage={:.2}", c))
                .unwrap_or_else(|| "N/A".to_string())
        };
        println!("OK (unified {})", metric_str);

        results.push(metrics);
    }

    // Generate report
    let search_types_evaluated: Vec<String> = search_types.iter().map(|s| s.to_string()).collect();

    let mut search_type_metrics = HashMap::new();
    for st in &search_types_evaluated {
        search_type_metrics.insert(st.clone(), aggregate_search_type_metrics(&results, st));
    }

    let mut query_type_metrics = HashMap::new();
    for st in &search_types_evaluated {
        query_type_metrics.insert(st.clone(), aggregate_by_query_type(&results, st));
    }

    let category_metrics = group_by_category(&results, &search_types);

    // Calculate success criteria
    let unified_ranked = query_type_metrics
        .get("unified")
        .and_then(|m| m.get("ranked"));
    let unified_structural = query_type_metrics
        .get("unified")
        .and_then(|m| m.get("structural"));

    let semantic_precision_at_5 = unified_ranked.and_then(|r| r.avg_precision_at_5);
    let graph_accuracy = unified_structural.and_then(|s| s.avg_set_coverage);

    let success_criteria = SuccessCriteria {
        semantic_precision_at_5_target: 0.80,
        semantic_precision_at_5_actual: semantic_precision_at_5,
        semantic_precision_at_5_pass: semantic_precision_at_5.map(|v| v >= 0.80).unwrap_or(false),
        graph_accuracy_target: 0.90,
        graph_accuracy_actual: graph_accuracy,
        graph_accuracy_pass: graph_accuracy.map(|v| v >= 0.90).unwrap_or(false),
    };

    let report = EvaluationReport {
        repository: dataset.repository.clone(),
        total_queries: results.len(),
        search_types_evaluated: search_types_evaluated.clone(),
        search_type_metrics,
        query_type_metrics,
        category_metrics,
        query_results: results,
        success_criteria: success_criteria.clone(),
    };

    // Print summary
    println!("\n========================================");
    println!("        EVALUATION SUMMARY");
    println!("========================================\n");
    println!("Total queries evaluated: {}", report.total_queries);
    println!("Search types: {:?}\n", report.search_types_evaluated);

    println!("=== Metrics by Search Type ===\n");
    println!(
        "{:<12} {:>10} {:>10} {:>10} {:>10} {:>10}",
        "Type", "Recall@5", "Recall@10", "Prec@5", "MRR", "Time(ms)"
    );
    println!("{}", "-".repeat(72));

    for st in &search_types_evaluated {
        if let Some(metrics) = report.search_type_metrics.get(st) {
            println!(
                "{:<12} {:>10} {:>10} {:>10} {:>10} {:>10.0}",
                st,
                metrics.avg_recall_at_5.map(|v| format!("{:.3}", v)).unwrap_or_else(|| "N/A".to_string()),
                metrics.avg_recall_at_10.map(|v| format!("{:.3}", v)).unwrap_or_else(|| "N/A".to_string()),
                metrics.avg_precision_at_5.map(|v| format!("{:.3}", v)).unwrap_or_else(|| "N/A".to_string()),
                metrics.avg_mrr.map(|v| format!("{:.3}", v)).unwrap_or_else(|| "N/A".to_string()),
                metrics.avg_query_time_ms
            );
        }
    }

    println!("\n=== Metrics by Query Type ===\n");
    for st in &search_types_evaluated {
        println!("Search Type: {}", st);
        if let Some(qt_metrics) = report.query_type_metrics.get(st) {
            if let Some(structural) = qt_metrics.get("structural") {
                println!("  STRUCTURAL (n={}):", structural.query_count);
                println!(
                    "    SetCov={} SetPrec={} F1={} ContCov={} Time={:.0}ms",
                    structural.avg_set_coverage.map(|v| format!("{:.3}", v)).unwrap_or_else(|| "N/A".into()),
                    structural.avg_set_precision.map(|v| format!("{:.3}", v)).unwrap_or_else(|| "N/A".into()),
                    structural.avg_f1_score.map(|v| format!("{:.3}", v)).unwrap_or_else(|| "N/A".into()),
                    structural.avg_contains_coverage.map(|v| format!("{:.3}", v)).unwrap_or_else(|| "N/A".into()),
                    structural.avg_query_time_ms,
                );
            }
            if let Some(ranked) = qt_metrics.get("ranked") {
                println!("  RANKED (n={}):", ranked.query_count);
                println!(
                    "    Recall@5={} Recall@10={} Prec@5={} MRR={} Time={:.0}ms",
                    ranked.avg_recall_at_5.map(|v| format!("{:.3}", v)).unwrap_or_else(|| "N/A".into()),
                    ranked.avg_recall_at_10.map(|v| format!("{:.3}", v)).unwrap_or_else(|| "N/A".into()),
                    ranked.avg_precision_at_5.map(|v| format!("{:.3}", v)).unwrap_or_else(|| "N/A".into()),
                    ranked.avg_mrr.map(|v| format!("{:.3}", v)).unwrap_or_else(|| "N/A".into()),
                    ranked.avg_query_time_ms,
                );
            }
        }
        println!();
    }

    println!("=== Metrics by Category ===\n");
    for cat in &report.category_metrics {
        println!("{}:", cat.category);
        println!("  Queries: {}", cat.query_count);
        for (st, metrics) in &cat.by_search_type {
            println!(
                "    {}: recall@5={}, mrr={}, time={:.0}ms",
                st,
                metrics.avg_recall_at_5.map(|v| format!("{:.3}", v)).unwrap_or_else(|| "N/A".to_string()),
                metrics.avg_mrr.map(|v| format!("{:.3}", v)).unwrap_or_else(|| "N/A".to_string()),
                metrics.avg_query_time_ms
            );
        }
        println!();
    }

    // Print success criteria
    println!("========================================");
    println!("        SUCCESS CRITERIA");
    println!("========================================\n");
    println!(
        "Semantic precision@5: {} (target: >{:.0}%, actual: {})",
        if success_criteria.semantic_precision_at_5_pass { "PASS" } else { "FAIL" },
        success_criteria.semantic_precision_at_5_target * 100.0,
        success_criteria.semantic_precision_at_5_actual
            .map(|v| format!("{:.1}%", v * 100.0))
            .unwrap_or_else(|| "N/A".to_string())
    );
    println!(
        "Graph accuracy: {} (target: >{:.0}%, actual: {})",
        if success_criteria.graph_accuracy_pass { "PASS" } else { "FAIL" },
        success_criteria.graph_accuracy_target * 100.0,
        success_criteria.graph_accuracy_actual
            .map(|v| format!("{:.1}%", v * 100.0))
            .unwrap_or_else(|| "N/A".to_string())
    );

    // Save report to file
    let report_json = serde_json::to_string_pretty(&report)?;
    let report_path = "nushell_eval_report.json";
    std::fs::write(report_path, &report_json)?;
    println!("\nReport saved to: {}", report_path);

    Ok(())
}
