//! Comprehensive Code Search Evaluation
//!
//! This test evaluates the effectiveness of ALL search modes on the rust-analyzer
//! codebase. It compares:
//! 1. Semantic - Vector embedding similarity search
//! 2. Fulltext - PostgreSQL GIN-indexed keyword search
//! 3. Unified - Hybrid semantic+fulltext with RRF fusion
//! 4. Graph - Neo4j relationship traversal queries
//! 5. Agentic - Claude-orchestrated multi-agent search (requires ENABLE_AGENTIC=1)
//!
//! Run with:
//!   # Without agentic (faster, no API costs)
//!   cargo test --manifest-path crates/e2e-tests/Cargo.toml --test test_graph_search_evaluation -- --ignored --nocapture
//!
//!   # With agentic search included
//!   ENABLE_AGENTIC=1 cargo test --manifest-path crates/e2e-tests/Cargo.toml --test test_graph_search_evaluation -- --ignored --nocapture
//!
//!   # Limit to first N samples (for quick testing)
//!   SAMPLE_LIMIT=5 cargo test --manifest-path crates/e2e-tests/Cargo.toml --test test_graph_search_evaluation -- --ignored --nocapture

#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]

use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::time::Instant;

const API_BASE_URL: &str = "http://127.0.0.1:3000";

/// Search type being evaluated
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchType {
    Semantic,
    Fulltext,
    Unified,
    Graph,
    Agentic,
}

impl std::fmt::Display for SearchType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SearchType::Semantic => write!(f, "semantic"),
            SearchType::Fulltext => write!(f, "fulltext"),
            SearchType::Unified => write!(f, "unified"),
            SearchType::Graph => write!(f, "graph"),
            SearchType::Agentic => write!(f, "agentic"),
        }
    }
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
    expected_top: Vec<String>,
    #[serde(default)]
    expected_entities: Vec<String>,
    relationship_type: Option<String>,
    #[serde(default)]
    relationship_chain: Vec<String>,
    notes: Option<String>,
}

/// Evaluation dataset structure
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct EvalDataset {
    repository: String,
    repository_path: String,
    description: String,
    queries: Vec<EvalQuery>,
}

// === Request/Response Types for Each Search Type ===

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
struct GraphQueryRequest {
    repository_id: String,
    query_type: String,
    entity_name: Option<String>,
    relationship_type: Option<String>,
    limit: usize,
}

#[derive(Debug, Serialize)]
struct AgenticSearchRequest {
    query: String,
    repository_ids: Vec<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    force_sonnet: bool,
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

// Response types for each search
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

#[derive(Debug, Deserialize)]
struct GraphQueryResponse {
    results: Vec<EntityResult>,
    #[allow(dead_code)]
    metadata: GraphMetadata,
}

#[derive(Debug, Deserialize)]
struct GraphMetadata {
    #[allow(dead_code)]
    total_results: usize,
    #[allow(dead_code)]
    query_time_ms: u64,
}

#[derive(Debug, Deserialize)]
struct AgenticSearchResponse {
    results: Vec<EntityResult>,
    metadata: AgenticMetadata,
}

#[derive(Debug, Deserialize)]
struct AgenticMetadata {
    query_time_ms: u64,
    #[allow(dead_code)]
    iterations: usize,
    #[allow(dead_code)]
    workers_spawned: usize,
    graph_traversal_used: bool,
    #[allow(dead_code)]
    graph_context_entities: usize,
    #[allow(dead_code)]
    graph_entities_in_results: usize,
}

/// Result from a single search type for a query
#[derive(Debug, Clone, Serialize)]
struct SearchTypeResult {
    search_type: SearchType,
    query_time_ms: u64,
    num_results: usize,
    recall_at_5: Option<f64>,
    recall_at_10: Option<f64>,
    precision_at_5: Option<f64>,
    precision_at_10: Option<f64>,
    mrr: Option<f64>,
    expected_found: usize,
    expected_total: usize,
    top_results: Vec<String>,
    error: Option<String>,
    graph_traversal_used: bool,
}

/// Metrics for a single query across all search types
#[derive(Debug, Clone, Serialize)]
struct QueryMetrics {
    query_id: String,
    category: String,
    results_by_type: HashMap<String, SearchTypeResult>,
}

/// Aggregate evaluation report
#[derive(Debug, Serialize)]
struct EvaluationReport {
    repository: String,
    total_queries: usize,
    search_types_evaluated: Vec<String>,

    // By search type aggregates
    search_type_metrics: HashMap<String, SearchTypeAggregate>,

    // By category metrics (for each search type)
    category_metrics: Vec<CategoryMetrics>,

    // Individual query results
    query_results: Vec<QueryMetrics>,
}

#[derive(Debug, Serialize)]
struct SearchTypeAggregate {
    search_type: String,
    queries_evaluated: usize,
    queries_failed: usize,
    avg_recall_at_5: Option<f64>,
    avg_recall_at_10: Option<f64>,
    avg_precision_at_5: Option<f64>,
    avg_precision_at_10: Option<f64>,
    avg_mrr: Option<f64>,
    avg_query_time_ms: f64,
    graph_usage_rate: f64,
}

#[derive(Debug, Serialize)]
struct CategoryMetrics {
    category: String,
    query_count: usize,
    by_search_type: HashMap<String, CategorySearchTypeMetrics>,
}

#[derive(Debug, Serialize)]
struct CategorySearchTypeMetrics {
    avg_recall_at_5: Option<f64>,
    avg_recall_at_10: Option<f64>,
    avg_precision_at_5: Option<f64>,
    avg_mrr: Option<f64>,
    avg_query_time_ms: f64,
}

/// Calculate recall at k
fn recall_at_k(results: &[EntityResult], expected: &[String], k: usize) -> Option<f64> {
    if expected.is_empty() {
        return None;
    }

    let top_k: HashSet<_> = results
        .iter()
        .take(k)
        .map(|r| r.qualified_name.to_lowercase())
        .collect();

    let expected_set: HashSet<_> = expected.iter().map(|e| e.to_lowercase()).collect();

    let found = expected_set
        .iter()
        .filter(|e| {
            top_k
                .iter()
                .any(|r| r.contains(*e) || e.contains(r.as_str()))
        })
        .count();

    Some(found as f64 / expected.len() as f64)
}

/// Calculate precision at k
fn precision_at_k(results: &[EntityResult], expected: &[String], k: usize) -> Option<f64> {
    if expected.is_empty() || results.is_empty() {
        return None;
    }

    let top_k: Vec<_> = results.iter().take(k).collect();
    let expected_set: HashSet<_> = expected.iter().map(|e| e.to_lowercase()).collect();

    let relevant = top_k
        .iter()
        .filter(|r| {
            let qn = r.qualified_name.to_lowercase();
            expected_set
                .iter()
                .any(|e| qn.contains(e) || e.contains(&qn))
        })
        .count();

    Some(relevant as f64 / top_k.len() as f64)
}

/// Calculate Mean Reciprocal Rank
fn calculate_mrr(results: &[EntityResult], expected: &[String]) -> Option<f64> {
    if expected.is_empty() {
        return None;
    }

    let expected_set: HashSet<_> = expected.iter().map(|e| e.to_lowercase()).collect();

    for (i, result) in results.iter().enumerate() {
        let qn = result.qualified_name.to_lowercase();
        if expected_set
            .iter()
            .any(|e| qn.contains(e) || e.contains(&qn))
        {
            return Some(1.0 / (i + 1) as f64);
        }
    }

    Some(0.0)
}

/// Get expected entities from query
fn get_expected(query: &EvalQuery) -> Vec<String> {
    if !query.expected.is_empty() {
        query.expected.clone()
    } else if !query.expected_contains.is_empty() {
        query.expected_contains.clone()
    } else if !query.expected_top.is_empty() {
        query.expected_top.clone()
    } else if !query.expected_entities.is_empty() {
        query.expected_entities.clone()
    } else {
        vec![]
    }
}

/// Calculate metrics for results
fn calculate_metrics(
    results: &[EntityResult],
    expected: &[String],
    query_time_ms: u64,
    search_type: SearchType,
    graph_traversal_used: bool,
) -> SearchTypeResult {
    let expected_found = expected
        .iter()
        .filter(|e| {
            let e_lower = e.to_lowercase();
            results.iter().any(|r| {
                let qn = r.qualified_name.to_lowercase();
                qn.contains(&e_lower) || e_lower.contains(&qn)
            })
        })
        .count();

    SearchTypeResult {
        search_type,
        query_time_ms,
        num_results: results.len(),
        recall_at_5: recall_at_k(results, expected, 5),
        recall_at_10: recall_at_k(results, expected, 10),
        precision_at_5: precision_at_k(results, expected, 5),
        precision_at_10: precision_at_k(results, expected, 10),
        mrr: calculate_mrr(results, expected),
        expected_found,
        expected_total: expected.len(),
        top_results: results
            .iter()
            .take(5)
            .map(|r| r.qualified_name.clone())
            .collect(),
        error: None,
        graph_traversal_used,
    }
}

/// Execute semantic search
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
        .await
        .context("Failed to send semantic search request")?;

    let elapsed = start.elapsed().as_millis() as u64;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("Semantic search returned error {}: {}", status, body);
    }

    let result: SemanticSearchResponse = response.json().await?;
    Ok((result.results, elapsed))
}

/// Execute fulltext search
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
        .await
        .context("Failed to send fulltext search request")?;

    let elapsed = start.elapsed().as_millis() as u64;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("Fulltext search returned error {}: {}", status, body);
    }

    let result: FulltextSearchResponse = response.json().await?;
    Ok((result.results, elapsed))
}

/// Execute unified search
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
        .await
        .context("Failed to send unified search request")?;

    let elapsed = start.elapsed().as_millis() as u64;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("Unified search returned error {}: {}", status, body);
    }

    let result: UnifiedSearchResponse = response.json().await?;
    Ok((result.results, elapsed.max(result.metadata.query_time_ms)))
}

/// Execute graph query
async fn execute_graph_search(
    client: &Client,
    query: &EvalQuery,
    repository_id: &str,
) -> Result<(Vec<EntityResult>, u64)> {
    // Extract entity name from query for graph traversal
    let entity_name = extract_entity_name(&query.query);

    let request = GraphQueryRequest {
        repository_id: repository_id.to_string(),
        query_type: "find_related".to_string(),
        entity_name,
        relationship_type: query.relationship_type.clone(),
        limit: 20,
    };

    let start = Instant::now();
    let response = client
        .post(format!("{API_BASE_URL}/api/v1/graph/query"))
        .json(&request)
        .send()
        .await
        .context("Failed to send graph query")?;

    let elapsed = start.elapsed().as_millis() as u64;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("Graph query returned error {}: {}", status, body);
    }

    let result: GraphQueryResponse = response.json().await?;
    Ok((result.results, elapsed))
}

/// Execute agentic search
async fn execute_agentic_search(
    client: &Client,
    query: &str,
    repository_id: &str,
) -> Result<(Vec<EntityResult>, u64, bool)> {
    let request = AgenticSearchRequest {
        query: query.to_string(),
        repository_ids: vec![repository_id.to_string()],
        force_sonnet: false,
    };

    let start = Instant::now();
    let response = client
        .post(format!("{API_BASE_URL}/api/v1/search/agentic"))
        .json(&request)
        .send()
        .await
        .context("Failed to send agentic search request")?;

    let elapsed = start.elapsed().as_millis() as u64;

    // Check for 501 Not Implemented (agentic not enabled)
    if response.status() == reqwest::StatusCode::NOT_IMPLEMENTED {
        anyhow::bail!("Agentic search not enabled on server");
    }

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("Agentic search returned error {}: {}", status, body);
    }

    let result: AgenticSearchResponse = response.json().await?;
    Ok((
        result.results,
        elapsed.max(result.metadata.query_time_ms),
        result.metadata.graph_traversal_used,
    ))
}

/// Extract entity name from query text
fn extract_entity_name(query: &str) -> Option<String> {
    // Look for patterns like "call X", "implement X", "use X"
    let patterns = [
        (r"call\s+(\w+::\w+)", 1),
        (r"calls\s+(\w+::\w+)", 1),
        (r"implement\s+(\w+)", 1),
        (r"implements\s+(\w+)", 1),
        (r"use\s+(\w+)", 1),
        (r"uses\s+(\w+)", 1),
        (r"`([^`]+)`", 1), // Backtick quoted names
    ];

    for (pattern, group) in patterns {
        if let Ok(re) = regex::Regex::new(pattern) {
            if let Some(caps) = re.captures(query) {
                if let Some(m) = caps.get(group) {
                    return Some(m.as_str().to_string());
                }
            }
        }
    }
    None
}

/// Evaluate a single query against all search types
async fn evaluate_query_all_types(
    client: &Client,
    query: &EvalQuery,
    repository_id: &str,
    include_agentic: bool,
) -> QueryMetrics {
    let expected = get_expected(query);
    let mut results_by_type = HashMap::new();

    // 1. Semantic search
    match execute_semantic_search(client, &query.query, repository_id).await {
        Ok((results, time_ms)) => {
            let metrics =
                calculate_metrics(&results, &expected, time_ms, SearchType::Semantic, false);
            results_by_type.insert(SearchType::Semantic.to_string(), metrics);
        }
        Err(e) => {
            results_by_type.insert(
                SearchType::Semantic.to_string(),
                SearchTypeResult {
                    search_type: SearchType::Semantic,
                    query_time_ms: 0,
                    num_results: 0,
                    recall_at_5: None,
                    recall_at_10: None,
                    precision_at_5: None,
                    precision_at_10: None,
                    mrr: None,
                    expected_found: 0,
                    expected_total: expected.len(),
                    top_results: vec![],
                    error: Some(e.to_string()),
                    graph_traversal_used: false,
                },
            );
        }
    }

    // 2. Fulltext search
    match execute_fulltext_search(client, &query.query, repository_id).await {
        Ok((results, time_ms)) => {
            let metrics =
                calculate_metrics(&results, &expected, time_ms, SearchType::Fulltext, false);
            results_by_type.insert(SearchType::Fulltext.to_string(), metrics);
        }
        Err(e) => {
            results_by_type.insert(
                SearchType::Fulltext.to_string(),
                SearchTypeResult {
                    search_type: SearchType::Fulltext,
                    query_time_ms: 0,
                    num_results: 0,
                    recall_at_5: None,
                    recall_at_10: None,
                    precision_at_5: None,
                    precision_at_10: None,
                    mrr: None,
                    expected_found: 0,
                    expected_total: expected.len(),
                    top_results: vec![],
                    error: Some(e.to_string()),
                    graph_traversal_used: false,
                },
            );
        }
    }

    // 3. Unified search
    match execute_unified_search(client, &query.query, repository_id).await {
        Ok((results, time_ms)) => {
            let metrics =
                calculate_metrics(&results, &expected, time_ms, SearchType::Unified, false);
            results_by_type.insert(SearchType::Unified.to_string(), metrics);
        }
        Err(e) => {
            results_by_type.insert(
                SearchType::Unified.to_string(),
                SearchTypeResult {
                    search_type: SearchType::Unified,
                    query_time_ms: 0,
                    num_results: 0,
                    recall_at_5: None,
                    recall_at_10: None,
                    precision_at_5: None,
                    precision_at_10: None,
                    mrr: None,
                    expected_found: 0,
                    expected_total: expected.len(),
                    top_results: vec![],
                    error: Some(e.to_string()),
                    graph_traversal_used: false,
                },
            );
        }
    }

    // 4. Graph search (only for queries with relationship type)
    if query.relationship_type.is_some() {
        match execute_graph_search(client, query, repository_id).await {
            Ok((results, time_ms)) => {
                let metrics =
                    calculate_metrics(&results, &expected, time_ms, SearchType::Graph, true);
                results_by_type.insert(SearchType::Graph.to_string(), metrics);
            }
            Err(e) => {
                results_by_type.insert(
                    SearchType::Graph.to_string(),
                    SearchTypeResult {
                        search_type: SearchType::Graph,
                        query_time_ms: 0,
                        num_results: 0,
                        recall_at_5: None,
                        recall_at_10: None,
                        precision_at_5: None,
                        precision_at_10: None,
                        mrr: None,
                        expected_found: 0,
                        expected_total: expected.len(),
                        top_results: vec![],
                        error: Some(e.to_string()),
                        graph_traversal_used: true,
                    },
                );
            }
        }
    }

    // 5. Agentic search (if enabled)
    if include_agentic {
        match execute_agentic_search(client, &query.query, repository_id).await {
            Ok((results, time_ms, graph_used)) => {
                let metrics = calculate_metrics(
                    &results,
                    &expected,
                    time_ms,
                    SearchType::Agentic,
                    graph_used,
                );
                results_by_type.insert(SearchType::Agentic.to_string(), metrics);
            }
            Err(e) => {
                results_by_type.insert(
                    SearchType::Agentic.to_string(),
                    SearchTypeResult {
                        search_type: SearchType::Agentic,
                        query_time_ms: 0,
                        num_results: 0,
                        recall_at_5: None,
                        recall_at_10: None,
                        precision_at_5: None,
                        precision_at_10: None,
                        mrr: None,
                        expected_found: 0,
                        expected_total: expected.len(),
                        top_results: vec![],
                        error: Some(e.to_string()),
                        graph_traversal_used: false,
                    },
                );
            }
        }
    }

    QueryMetrics {
        query_id: query.id.clone(),
        category: query.category.clone(),
        results_by_type,
    }
}

fn calculate_avg(values: &[Option<f64>]) -> Option<f64> {
    let valid: Vec<f64> = values.iter().filter_map(|v| *v).collect();
    if valid.is_empty() {
        None
    } else {
        Some(valid.iter().sum::<f64>() / valid.len() as f64)
    }
}

fn aggregate_search_type_metrics(
    results: &[QueryMetrics],
    search_type: &str,
) -> SearchTypeAggregate {
    let type_results: Vec<&SearchTypeResult> = results
        .iter()
        .filter_map(|q| q.results_by_type.get(search_type))
        .collect();

    let queries_evaluated = type_results.len();
    let queries_failed = type_results.iter().filter(|r| r.error.is_some()).count();

    let successful: Vec<_> = type_results.iter().filter(|r| r.error.is_none()).collect();

    let avg_recall_5 = calculate_avg(&successful.iter().map(|r| r.recall_at_5).collect::<Vec<_>>());
    let avg_recall_10 = calculate_avg(
        &successful
            .iter()
            .map(|r| r.recall_at_10)
            .collect::<Vec<_>>(),
    );
    let avg_precision_5 = calculate_avg(
        &successful
            .iter()
            .map(|r| r.precision_at_5)
            .collect::<Vec<_>>(),
    );
    let avg_precision_10 = calculate_avg(
        &successful
            .iter()
            .map(|r| r.precision_at_10)
            .collect::<Vec<_>>(),
    );
    let avg_mrr = calculate_avg(&successful.iter().map(|r| r.mrr).collect::<Vec<_>>());

    let avg_query_time = if successful.is_empty() {
        0.0
    } else {
        successful
            .iter()
            .map(|r| r.query_time_ms as f64)
            .sum::<f64>()
            / successful.len() as f64
    };

    let graph_usage = if successful.is_empty() {
        0.0
    } else {
        successful.iter().filter(|r| r.graph_traversal_used).count() as f64
            / successful.len() as f64
    };

    SearchTypeAggregate {
        search_type: search_type.to_string(),
        queries_evaluated,
        queries_failed,
        avg_recall_at_5: avg_recall_5,
        avg_recall_at_10: avg_recall_10,
        avg_precision_at_5: avg_precision_5,
        avg_precision_at_10: avg_precision_10,
        avg_mrr,
        avg_query_time_ms: avg_query_time,
        graph_usage_rate: graph_usage,
    }
}

fn group_by_category(results: &[QueryMetrics], search_types: &[&str]) -> Vec<CategoryMetrics> {
    let mut categories: HashMap<String, Vec<&QueryMetrics>> = HashMap::new();

    for result in results {
        categories
            .entry(result.category.clone())
            .or_default()
            .push(result);
    }

    categories
        .into_iter()
        .map(|(category, metrics)| {
            let query_count = metrics.len();
            let mut by_search_type = HashMap::new();

            for search_type in search_types {
                let type_results: Vec<&SearchTypeResult> = metrics
                    .iter()
                    .filter_map(|q| q.results_by_type.get(*search_type))
                    .filter(|r| r.error.is_none())
                    .collect();

                if !type_results.is_empty() {
                    let avg_recall_5 = calculate_avg(
                        &type_results
                            .iter()
                            .map(|r| r.recall_at_5)
                            .collect::<Vec<_>>(),
                    );
                    let avg_recall_10 = calculate_avg(
                        &type_results
                            .iter()
                            .map(|r| r.recall_at_10)
                            .collect::<Vec<_>>(),
                    );
                    let avg_precision_5 = calculate_avg(
                        &type_results
                            .iter()
                            .map(|r| r.precision_at_5)
                            .collect::<Vec<_>>(),
                    );
                    let avg_mrr =
                        calculate_avg(&type_results.iter().map(|r| r.mrr).collect::<Vec<_>>());
                    let avg_query_time = type_results
                        .iter()
                        .map(|r| r.query_time_ms as f64)
                        .sum::<f64>()
                        / type_results.len() as f64;

                    by_search_type.insert(
                        (*search_type).to_string(),
                        CategorySearchTypeMetrics {
                            avg_recall_at_5: avg_recall_5,
                            avg_recall_at_10: avg_recall_10,
                            avg_precision_at_5: avg_precision_5,
                            avg_mrr,
                            avg_query_time_ms: avg_query_time,
                        },
                    );
                }
            }

            CategoryMetrics {
                category,
                query_count,
                by_search_type,
            }
        })
        .collect()
}

/// Check if ENABLE_AGENTIC environment variable is set
fn is_agentic_enabled() -> bool {
    std::env::var("ENABLE_AGENTIC")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

/// Get optional sample limit from SAMPLE_LIMIT environment variable
fn get_sample_limit() -> Option<usize> {
    std::env::var("SAMPLE_LIMIT")
        .ok()
        .and_then(|v| v.parse().ok())
}

/// Check if agentic search is enabled on the server
async fn is_agentic_enabled_on_server(client: &Client) -> bool {
    match client.get(format!("{API_BASE_URL}/health")).send().await {
        Ok(response) => {
            if let Ok(json) = response.json::<serde_json::Value>().await {
                json.get("dependencies")
                    .and_then(|d| d.get("agentic_search"))
                    .and_then(|a| a.get("status"))
                    .and_then(|s| s.as_str())
                    .map(|s| s == "enabled")
                    .unwrap_or(false)
            } else {
                false
            }
        }
        Err(_) => false,
    }
}

#[tokio::test]
#[ignore]
async fn test_graph_search_evaluation() -> Result<()> {
    // Load evaluation dataset
    let dataset_path = "fixtures/graph_eval_queries.json";
    let dataset_content = std::fs::read_to_string(dataset_path)
        .context("Failed to read evaluation dataset. Run from crates/e2e-tests directory.")?;
    let dataset: EvalDataset =
        serde_json::from_str(&dataset_content).context("Failed to parse evaluation dataset")?;

    println!("\n=== Comprehensive Code Search Evaluation ===");
    println!("Repository: {}", dataset.repository);
    println!("Total queries: {}\n", dataset.queries.len());

    let client = Client::new();

    // Check if ENABLE_AGENTIC environment variable is set
    let agentic_requested = is_agentic_enabled();
    let include_agentic = if agentic_requested {
        // User wants agentic - verify server supports it
        let server_supports_agentic = is_agentic_enabled_on_server(&client).await;
        if !server_supports_agentic {
            anyhow::bail!(
                "Agentic search requested via ENABLE_AGENTIC=1, but server does not have it enabled.\n\
                 Start the server with: ANTHROPIC_API_KEY=... codesearch serve --enable-agentic"
            );
        }
        println!("Agentic search: enabled (ENABLE_AGENTIC=1)");
        true
    } else {
        println!("Agentic search: disabled (set ENABLE_AGENTIC=1 to enable)");
        false
    };

    // Get repository ID
    let repos_response = client
        .get(format!("{API_BASE_URL}/api/v1/repositories"))
        .send()
        .await
        .context("Failed to get repositories")?;

    let repos: serde_json::Value = repos_response.json().await?;
    let repository_id = repos
        .get("repositories")
        .and_then(|arr| arr.as_array())
        .and_then(|arr| {
            arr.iter().find(|r| {
                r.get("repository_path")
                    .and_then(|p| p.as_str())
                    .map(|p| p.contains(&dataset.repository))
                    .unwrap_or(false)
            })
        })
        .and_then(|r| r.get("repository_id"))
        .and_then(|id| id.as_str())
        .context("Repository not found")?;

    println!("Repository ID: {}\n", repository_id);

    // Determine which search types to evaluate
    let mut search_types = vec!["semantic", "fulltext", "unified"];
    // Graph search is included when query has relationship_type
    search_types.push("graph");
    if include_agentic {
        search_types.push("agentic");
    }
    println!("Search types to evaluate: {:?}\n", search_types);

    // Check for sample limit
    let sample_limit = get_sample_limit();
    let queries_to_run: Vec<_> = match sample_limit {
        Some(limit) => {
            println!(
                "Sample limit: {} (set SAMPLE_LIMIT env var, running {} of {} queries)\n",
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

    // Evaluate each query against all search types
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

        let metrics =
            evaluate_query_all_types(&client, query, repository_id, include_agentic).await;

        // Print summary for this query
        let unified_result = metrics.results_by_type.get("unified");
        let recall_str = unified_result
            .and_then(|r| r.recall_at_5)
            .map(|r| format!("{:.2}", r))
            .unwrap_or_else(|| "N/A".to_string());
        println!("OK (unified recall@5={})", recall_str);

        results.push(metrics);
    }

    // Generate report
    let search_types_evaluated: Vec<String> = search_types.iter().map(|s| s.to_string()).collect();

    let mut search_type_metrics = HashMap::new();
    for st in &search_types_evaluated {
        search_type_metrics.insert(st.clone(), aggregate_search_type_metrics(&results, st));
    }

    let category_metrics = group_by_category(
        &results,
        &search_types.iter().map(|s| *s).collect::<Vec<_>>(),
    );

    let report = EvaluationReport {
        repository: dataset.repository.clone(),
        total_queries: results.len(),
        search_types_evaluated: search_types_evaluated.clone(),
        search_type_metrics,
        category_metrics,
        query_results: results,
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
                metrics
                    .avg_recall_at_5
                    .map(|v| format!("{:.3}", v))
                    .unwrap_or_else(|| "N/A".to_string()),
                metrics
                    .avg_recall_at_10
                    .map(|v| format!("{:.3}", v))
                    .unwrap_or_else(|| "N/A".to_string()),
                metrics
                    .avg_precision_at_5
                    .map(|v| format!("{:.3}", v))
                    .unwrap_or_else(|| "N/A".to_string()),
                metrics
                    .avg_mrr
                    .map(|v| format!("{:.3}", v))
                    .unwrap_or_else(|| "N/A".to_string()),
                metrics.avg_query_time_ms
            );
        }
    }

    println!("\n=== Metrics by Category ===\n");
    for cat in &report.category_metrics {
        println!("{}:", cat.category);
        println!("  Queries: {}", cat.query_count);
        for (st, metrics) in &cat.by_search_type {
            println!(
                "    {}: recall@5={}, mrr={}, time={:.0}ms",
                st,
                metrics
                    .avg_recall_at_5
                    .map(|v| format!("{:.3}", v))
                    .unwrap_or_else(|| "N/A".to_string()),
                metrics
                    .avg_mrr
                    .map(|v| format!("{:.3}", v))
                    .unwrap_or_else(|| "N/A".to_string()),
                metrics.avg_query_time_ms
            );
        }
        println!();
    }

    // Save report to file
    let report_json = serde_json::to_string_pretty(&report)?;
    let report_path = "graph_eval_report.json";
    std::fs::write(report_path, &report_json)?;
    println!("\nReport saved to: {}", report_path);

    Ok(())
}
