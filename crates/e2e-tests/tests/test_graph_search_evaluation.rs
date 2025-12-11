//! Comprehensive Code Search Evaluation
//!
//! This test evaluates the effectiveness of ALL search modes on the rust-analyzer
//! codebase. It compares:
//! 1. Semantic - Vector embedding similarity search
//! 2. Fulltext - PostgreSQL GIN-indexed keyword search
//! 3. Unified - Hybrid semantic+fulltext with RRF fusion
//! 4. Agentic - Claude-orchestrated multi-agent search (requires ENABLE_AGENTIC=1)
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
//!
//! Note: The "has been running for over 60 seconds" warning cannot be disabled in cargo test
//! (rust-lang/rust#115989). Use cargo-nextest for configurable slow timeouts if needed.

#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]

use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

const API_BASE_URL: &str = "http://127.0.0.1:3000";

// === Name Matching Analysis (for investigating Issue 1) ===

/// Global collector for name matching analysis
static NAME_MATCHING_ANALYSIS: OnceLock<Mutex<Vec<QueryMatchingAnalysis>>> = OnceLock::new();

fn get_analysis_collector() -> &'static Mutex<Vec<QueryMatchingAnalysis>> {
    NAME_MATCHING_ANALYSIS.get_or_init(|| Mutex::new(Vec::new()))
}

/// Analysis of name matching for a single query
#[derive(Debug, Clone, Serialize)]
struct QueryMatchingAnalysis {
    query_id: String,
    category: String,
    expected_count: usize,
    returned_count: usize,
    matched_count: usize,
    /// Expected names that matched (with what they matched to)
    matched_expected: Vec<MatchedPair>,
    /// Expected names that did NOT match anything
    unmatched_expected: Vec<UnmatchedExpected>,
    /// Returned names that were NOT expected
    unexpected_returned: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct MatchedPair {
    expected: String,
    expected_normalized: String,
    returned: String,
    returned_normalized: String,
    match_type: String, // "expected_contains_returned" or "returned_contains_expected"
}

#[derive(Debug, Clone, Serialize)]
struct UnmatchedExpected {
    expected: String,
    expected_normalized: String,
    /// Closest returned names by edit distance (for debugging)
    closest_returned: Vec<String>,
}

/// Analyze name matching for a structural query and collect results
fn analyze_name_matching(
    query_id: &str,
    category: &str,
    results: &[EntityResult],
    expected: &[String],
) {
    if expected.is_empty() {
        return;
    }

    let expected_normalized: Vec<(String, String)> = expected
        .iter()
        .map(|e| (e.clone(), normalize_qualified_name(e)))
        .collect();

    let returned_normalized: Vec<(String, String)> = results
        .iter()
        .map(|r| (r.qualified_name.clone(), normalize_qualified_name(&r.qualified_name)))
        .collect();

    let mut matched_expected = Vec::new();
    let mut unmatched_expected = Vec::new();
    let mut matched_returned_indices = HashSet::new();

    // For each expected, find if it matches any returned
    for (orig_expected, norm_expected) in &expected_normalized {
        let mut found_match = false;

        for (idx, (orig_returned, norm_returned)) in returned_normalized.iter().enumerate() {
            let match_type = if norm_returned.contains(norm_expected.as_str()) {
                Some("returned_contains_expected")
            } else if norm_expected.contains(norm_returned.as_str()) {
                Some("expected_contains_returned")
            } else {
                None
            };

            if let Some(mt) = match_type {
                matched_expected.push(MatchedPair {
                    expected: orig_expected.clone(),
                    expected_normalized: norm_expected.clone(),
                    returned: orig_returned.clone(),
                    returned_normalized: norm_returned.clone(),
                    match_type: mt.to_string(),
                });
                matched_returned_indices.insert(idx);
                found_match = true;
                break; // Only record first match
            }
        }

        if !found_match {
            // Find closest returned names for debugging
            let mut closest: Vec<(usize, String)> = returned_normalized
                .iter()
                .map(|(orig, norm)| {
                    let dist = levenshtein_distance(norm_expected, norm);
                    (dist, orig.clone())
                })
                .collect();
            closest.sort_by_key(|(dist, _)| *dist);
            let closest_returned: Vec<String> = closest.into_iter().take(3).map(|(_, s)| s).collect();

            unmatched_expected.push(UnmatchedExpected {
                expected: orig_expected.clone(),
                expected_normalized: norm_expected.clone(),
                closest_returned,
            });
        }
    }

    // Find returned that weren't expected
    let unexpected_returned: Vec<String> = returned_normalized
        .iter()
        .enumerate()
        .filter(|(idx, _)| !matched_returned_indices.contains(idx))
        .map(|(_, (orig, _))| orig.clone())
        .collect();

    let analysis = QueryMatchingAnalysis {
        query_id: query_id.to_string(),
        category: category.to_string(),
        expected_count: expected.len(),
        returned_count: results.len(),
        matched_count: matched_expected.len(),
        matched_expected,
        unmatched_expected,
        unexpected_returned,
    };

    // Add to global collector
    if let Ok(mut collector) = get_analysis_collector().lock() {
        collector.push(analysis);
    }
}

/// Simple Levenshtein distance for finding closest matches
fn levenshtein_distance(a: &str, b: &str) -> usize {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    let m = a_chars.len();
    let n = b_chars.len();

    if m == 0 {
        return n;
    }
    if n == 0 {
        return m;
    }

    let mut prev = (0..=n).collect::<Vec<_>>();
    let mut curr = vec![0; n + 1];

    for i in 1..=m {
        curr[0] = i;
        for j in 1..=n {
            let cost = if a_chars[i - 1] == b_chars[j - 1] { 0 } else { 1 };
            curr[j] = (prev[j] + 1).min(curr[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }

    prev[n]
}

/// Write collected name matching analysis to file
fn write_name_matching_analysis() -> Result<()> {
    let analysis = if let Ok(collector) = get_analysis_collector().lock() {
        collector.clone()
    } else {
        return Ok(());
    };

    if analysis.is_empty() {
        return Ok(());
    }

    // Calculate summary statistics
    let total_expected: usize = analysis.iter().map(|a| a.expected_count).sum();
    let total_matched: usize = analysis.iter().map(|a| a.matched_count).sum();
    let total_unmatched: usize = analysis.iter().map(|a| a.unmatched_expected.len()).sum();

    let summary = serde_json::json!({
        "summary": {
            "total_queries_analyzed": analysis.len(),
            "total_expected_names": total_expected,
            "total_matched": total_matched,
            "total_unmatched": total_unmatched,
            "match_rate": if total_expected > 0 { total_matched as f64 / total_expected as f64 } else { 0.0 },
        },
        "queries": analysis,
    });

    let json = serde_json::to_string_pretty(&summary)?;
    std::fs::write("name_mismatch_analysis.json", &json)?;
    println!("\nName matching analysis saved to: name_mismatch_analysis.json");

    Ok(())
}

/// Search type being evaluated
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchType {
    Semantic,
    Fulltext,
    Unified,
    Agentic,
}

impl std::fmt::Display for SearchType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SearchType::Semantic => write!(f, "semantic"),
            SearchType::Fulltext => write!(f, "fulltext"),
            SearchType::Unified => write!(f, "unified"),
            SearchType::Agentic => write!(f, "agentic"),
        }
    }
}

/// Query type for metric selection
///
/// Structural queries return exhaustive, unranked sets (e.g., "What functions call X?")
/// Ranked queries benefit from ranking (e.g., "Find authentication logic")
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueryType {
    /// Exhaustive, unranked results - use set-based metrics
    Structural,
    /// Order matters - use traditional IR metrics (Recall@K, MRR)
    Ranked,
}

impl std::fmt::Display for QueryType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            QueryType::Structural => write!(f, "structural"),
            QueryType::Ranked => write!(f, "ranked"),
        }
    }
}

/// Classify query type based on category
fn classify_query_type(category: &str) -> QueryType {
    match category {
        "call_graph" | "trait_impl" | "type_usage" | "module_structure" | "complex" => {
            QueryType::Structural
        }
        "semantic" | "discovery" => QueryType::Ranked,
        _ => QueryType::Ranked,
    }
}

/// LSP query metadata for ground truth extraction
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
struct LspQueryMetadata {
    /// LSP method: "textDocument/references", "textDocument/implementation", etc.
    method: Option<String>,
    /// Target location: "path/to/file.rs:line:col"
    target: Option<String>,
    /// Query type: "single_hop" or "chain"
    #[serde(rename = "type")]
    query_type: Option<String>,
    /// Chain of LSP queries for multi-hop
    chain: Option<Vec<LspChainHop>>,
}

/// A single hop in a chained LSP query
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
struct LspChainHop {
    method: String,
    target: Option<String>,
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
    /// Top K results by semantic relevance (for ranking evaluation within structural queries)
    #[serde(default)]
    expected_top_k: Vec<String>,
    relationship_type: Option<String>,
    #[serde(default)]
    relationship_chain: Vec<String>,
    /// LSP query metadata for ground truth extraction
    lsp_query: Option<LspQueryMetadata>,
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

/// Metrics for structural (exhaustive, unranked) queries
#[derive(Debug, Clone, Serialize, Default)]
struct StructuralMetrics {
    /// |returned ∩ expected| / |expected| - for queries with expected list
    set_coverage: Option<f64>,
    /// |returned ∩ expected| / |returned| - for queries with expected list
    set_precision: Option<f64>,
    /// Harmonic mean of coverage and precision
    f1_score: Option<f64>,
    /// |returned ∩ expected_contains| / |expected_contains|
    contains_coverage: Option<f64>,
    /// Number of results returned
    returned_count: usize,
}

/// Result from a single search type for a query
#[derive(Debug, Clone, Serialize)]
struct SearchTypeResult {
    search_type: SearchType,
    query_time_ms: u64,
    num_results: usize,
    // Ranked metrics (populated for ranked queries)
    recall_at_5: Option<f64>,
    recall_at_10: Option<f64>,
    precision_at_5: Option<f64>,
    precision_at_10: Option<f64>,
    mrr: Option<f64>,
    // Structural metrics (populated for structural queries)
    structural_metrics: Option<StructuralMetrics>,
    // Common fields
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

    // By query type aggregates (structural vs ranked, for each search type)
    query_type_metrics: HashMap<String, HashMap<String, QueryTypeAggregate>>,

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

/// Aggregate metrics for queries grouped by query type (structural vs ranked)
#[derive(Debug, Serialize)]
struct QueryTypeAggregate {
    query_type: String,
    query_count: usize,
    // Structural metrics (for structural queries)
    avg_set_coverage: Option<f64>,
    avg_set_precision: Option<f64>,
    avg_f1_score: Option<f64>,
    avg_contains_coverage: Option<f64>,
    // Ranked metrics (for ranked queries)
    avg_recall_at_5: Option<f64>,
    avg_recall_at_10: Option<f64>,
    avg_precision_at_5: Option<f64>,
    avg_mrr: Option<f64>,
    // Common
    avg_query_time_ms: f64,
}

/// Normalize a qualified_name for matching by stripping implementation details
///
/// Handles formats like:
/// - "Fn::const_token (impl at line 566)" -> "fn::const_token"
/// - "impl Fn at line 566" -> "fn"
/// - "AssocItemList::l_curly_token" -> "associtemlist::l_curly_token"
fn normalize_qualified_name(qn: &str) -> String {
    let qn = qn.to_lowercase();

    // Strip "(impl at line X)" suffix
    if let Some(idx) = qn.find(" (impl at line") {
        return qn[..idx].to_string();
    }

    // Handle "impl TypeName at line X" format - extract just the type name
    if qn.starts_with("impl ") {
        if let Some(at_idx) = qn.find(" at line") {
            return qn[5..at_idx].to_string();
        }
        // "impl TypeName" without line number
        return qn[5..].to_string();
    }

    qn
}

/// Calculate recall at k
fn recall_at_k(results: &[EntityResult], expected: &[String], k: usize) -> Option<f64> {
    if expected.is_empty() {
        return None;
    }

    let top_k: HashSet<_> = results
        .iter()
        .take(k)
        .map(|r| normalize_qualified_name(&r.qualified_name))
        .collect();

    let expected_set: HashSet<_> = expected
        .iter()
        .map(|e| normalize_qualified_name(e))
        .collect();

    let found = expected_set
        .iter()
        .filter(|e| {
            top_k
                .iter()
                .any(|r| r.contains(e.as_str()) || e.contains(r.as_str()))
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
    let expected_set: HashSet<_> = expected
        .iter()
        .map(|e| normalize_qualified_name(e))
        .collect();

    let relevant = top_k
        .iter()
        .filter(|r| {
            let qn = normalize_qualified_name(&r.qualified_name);
            expected_set
                .iter()
                .any(|e| qn.contains(e.as_str()) || e.contains(qn.as_str()))
        })
        .count();

    Some(relevant as f64 / top_k.len() as f64)
}

/// Calculate Mean Reciprocal Rank
fn calculate_mrr(results: &[EntityResult], expected: &[String]) -> Option<f64> {
    if expected.is_empty() {
        return None;
    }

    let expected_set: HashSet<_> = expected
        .iter()
        .map(|e| normalize_qualified_name(e))
        .collect();

    for (i, result) in results.iter().enumerate() {
        let qn = normalize_qualified_name(&result.qualified_name);
        if expected_set
            .iter()
            .any(|e| qn.contains(e.as_str()) || e.contains(qn.as_str()))
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

// === Structural Query Metrics ===

/// Calculate set coverage for structural queries
/// Returns |returned ∩ expected| / |expected|
fn calculate_set_coverage(results: &[EntityResult], expected: &[String]) -> Option<f64> {
    if expected.is_empty() {
        return None;
    }

    let expected_set: HashSet<_> = expected
        .iter()
        .map(|e| normalize_qualified_name(e))
        .collect();

    // Coverage is based on how many expected items we found in results
    let matched_expected = expected_set
        .iter()
        .filter(|e| {
            results.iter().any(|r| {
                let qn = normalize_qualified_name(&r.qualified_name);
                qn.contains(e.as_str()) || e.contains(qn.as_str())
            })
        })
        .count();

    Some(matched_expected as f64 / expected.len() as f64)
}

/// Calculate set precision for structural queries
/// Returns |returned ∩ expected| / |returned|
fn calculate_set_precision(results: &[EntityResult], expected: &[String]) -> Option<f64> {
    if expected.is_empty() || results.is_empty() {
        return None;
    }

    let expected_set: HashSet<_> = expected
        .iter()
        .map(|e| normalize_qualified_name(e))
        .collect();

    let relevant = results
        .iter()
        .filter(|r| {
            let qn = normalize_qualified_name(&r.qualified_name);
            expected_set
                .iter()
                .any(|e| qn.contains(e.as_str()) || e.contains(qn.as_str()))
        })
        .count();

    Some(relevant as f64 / results.len() as f64)
}

/// Calculate F1 score from coverage and precision
fn calculate_f1(coverage: Option<f64>, precision: Option<f64>) -> Option<f64> {
    match (coverage, precision) {
        (Some(c), Some(p)) if c + p > 0.0 => Some(2.0 * c * p / (c + p)),
        _ => None,
    }
}

/// Calculate structural metrics for a query
fn calculate_structural_metrics(results: &[EntityResult], query: &EvalQuery) -> StructuralMetrics {
    // Collect name matching analysis for investigation
    if !query.expected.is_empty() {
        analyze_name_matching(&query.id, &query.category, results, &query.expected);
    }

    let set_coverage = if !query.expected.is_empty() {
        calculate_set_coverage(results, &query.expected)
    } else {
        None
    };

    let set_precision = if !query.expected.is_empty() {
        calculate_set_precision(results, &query.expected)
    } else {
        None
    };

    let f1_score = calculate_f1(set_coverage, set_precision);

    let contains_coverage = if !query.expected_contains.is_empty() {
        calculate_set_coverage(results, &query.expected_contains)
    } else {
        None
    };

    StructuralMetrics {
        set_coverage,
        set_precision,
        f1_score,
        contains_coverage,
        returned_count: results.len(),
    }
}

/// Calculate metrics for results based on query type
fn calculate_metrics(
    results: &[EntityResult],
    query: &EvalQuery,
    query_time_ms: u64,
    search_type: SearchType,
    graph_traversal_used: bool,
) -> SearchTypeResult {
    let expected = get_expected(query);
    let query_type = classify_query_type(&query.category);

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

    // Calculate metrics based on query type
    let (ranked_metrics, structural_metrics) = match query_type {
        QueryType::Ranked => {
            // Use traditional IR metrics for ranked queries
            (
                (
                    recall_at_k(results, &expected, 5),
                    recall_at_k(results, &expected, 10),
                    precision_at_k(results, &expected, 5),
                    precision_at_k(results, &expected, 10),
                    calculate_mrr(results, &expected),
                ),
                None,
            )
        }
        QueryType::Structural => {
            // Use set-based metrics for structural queries
            let structural = calculate_structural_metrics(results, query);
            ((None, None, None, None, None), Some(structural))
        }
    };

    SearchTypeResult {
        search_type,
        query_time_ms,
        num_results: results.len(),
        recall_at_5: ranked_metrics.0,
        recall_at_10: ranked_metrics.1,
        precision_at_5: ranked_metrics.2,
        precision_at_10: ranked_metrics.3,
        mrr: ranked_metrics.4,
        structural_metrics,
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

/// Evaluate a single query against all search types
async fn evaluate_query_all_types(
    client: &Client,
    query: &EvalQuery,
    repository_id: &str,
    include_agentic: bool,
) -> QueryMetrics {
    let expected = get_expected(query);
    let mut results_by_type = HashMap::new();

    // Helper to create error result
    let make_error_result = |search_type: SearchType, e: anyhow::Error| SearchTypeResult {
        search_type,
        query_time_ms: 0,
        num_results: 0,
        recall_at_5: None,
        recall_at_10: None,
        precision_at_5: None,
        precision_at_10: None,
        mrr: None,
        structural_metrics: None,
        expected_found: 0,
        expected_total: expected.len(),
        top_results: vec![],
        error: Some(e.to_string()),
        graph_traversal_used: false,
    };

    // 1. Semantic search
    match execute_semantic_search(client, &query.query, repository_id).await {
        Ok((results, time_ms)) => {
            let metrics = calculate_metrics(&results, query, time_ms, SearchType::Semantic, false);
            results_by_type.insert(SearchType::Semantic.to_string(), metrics);
        }
        Err(e) => {
            results_by_type.insert(
                SearchType::Semantic.to_string(),
                make_error_result(SearchType::Semantic, e),
            );
        }
    }

    // 2. Fulltext search
    match execute_fulltext_search(client, &query.query, repository_id).await {
        Ok((results, time_ms)) => {
            let metrics = calculate_metrics(&results, query, time_ms, SearchType::Fulltext, false);
            results_by_type.insert(SearchType::Fulltext.to_string(), metrics);
        }
        Err(e) => {
            results_by_type.insert(
                SearchType::Fulltext.to_string(),
                make_error_result(SearchType::Fulltext, e),
            );
        }
    }

    // 3. Unified search
    match execute_unified_search(client, &query.query, repository_id).await {
        Ok((results, time_ms)) => {
            let metrics = calculate_metrics(&results, query, time_ms, SearchType::Unified, false);
            results_by_type.insert(SearchType::Unified.to_string(), metrics);
        }
        Err(e) => {
            results_by_type.insert(
                SearchType::Unified.to_string(),
                make_error_result(SearchType::Unified, e),
            );
        }
    }

    // 4. Agentic search (if enabled)
    if include_agentic {
        match execute_agentic_search(client, &query.query, repository_id).await {
            Ok((results, time_ms, graph_used)) => {
                let metrics =
                    calculate_metrics(&results, query, time_ms, SearchType::Agentic, graph_used);
                results_by_type.insert(SearchType::Agentic.to_string(), metrics);
            }
            Err(e) => {
                results_by_type.insert(
                    SearchType::Agentic.to_string(),
                    make_error_result(SearchType::Agentic, e),
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

/// Aggregate metrics by query type (structural vs ranked) for a given search type
fn aggregate_by_query_type(
    results: &[QueryMetrics],
    search_type: &str,
) -> HashMap<String, QueryTypeAggregate> {
    let mut by_type: HashMap<QueryType, Vec<&SearchTypeResult>> = HashMap::new();

    for query_metrics in results {
        let qt = classify_query_type(&query_metrics.category);
        if let Some(result) = query_metrics.results_by_type.get(search_type) {
            by_type.entry(qt).or_default().push(result);
        }
    }

    let mut aggregates = HashMap::new();

    for (query_type, type_results) in by_type {
        let successful: Vec<_> = type_results.iter().filter(|r| r.error.is_none()).collect();

        let avg_query_time = if successful.is_empty() {
            0.0
        } else {
            successful
                .iter()
                .map(|r| r.query_time_ms as f64)
                .sum::<f64>()
                / successful.len() as f64
        };

        let aggregate = match query_type {
            QueryType::Structural => {
                // Collect structural metrics from results
                let structural_results: Vec<_> = successful
                    .iter()
                    .filter_map(|r| r.structural_metrics.as_ref())
                    .collect();

                // Calculate averages for structural metrics
                let avg_set_coverage = {
                    let values: Vec<f64> = structural_results
                        .iter()
                        .filter_map(|s| s.set_coverage)
                        .collect();
                    if values.is_empty() {
                        None
                    } else {
                        Some(values.iter().sum::<f64>() / values.len() as f64)
                    }
                };

                let avg_set_precision = {
                    let values: Vec<f64> = structural_results
                        .iter()
                        .filter_map(|s| s.set_precision)
                        .collect();
                    if values.is_empty() {
                        None
                    } else {
                        Some(values.iter().sum::<f64>() / values.len() as f64)
                    }
                };

                let avg_f1_score = {
                    let values: Vec<f64> = structural_results
                        .iter()
                        .filter_map(|s| s.f1_score)
                        .collect();
                    if values.is_empty() {
                        None
                    } else {
                        Some(values.iter().sum::<f64>() / values.len() as f64)
                    }
                };

                let avg_contains_coverage = {
                    let values: Vec<f64> = structural_results
                        .iter()
                        .filter_map(|s| s.contains_coverage)
                        .collect();
                    if values.is_empty() {
                        None
                    } else {
                        Some(values.iter().sum::<f64>() / values.len() as f64)
                    }
                };

                QueryTypeAggregate {
                    query_type: query_type.to_string(),
                    query_count: type_results.len(),
                    avg_set_coverage,
                    avg_set_precision,
                    avg_f1_score,
                    avg_contains_coverage,
                    avg_recall_at_5: None,
                    avg_recall_at_10: None,
                    avg_precision_at_5: None,
                    avg_mrr: None,
                    avg_query_time_ms: avg_query_time,
                }
            }
            QueryType::Ranked => {
                // Use traditional IR metrics for ranked queries
                QueryTypeAggregate {
                    query_type: query_type.to_string(),
                    query_count: type_results.len(),
                    avg_set_coverage: None,
                    avg_set_precision: None,
                    avg_f1_score: None,
                    avg_contains_coverage: None,
                    avg_recall_at_5: calculate_avg(
                        &successful.iter().map(|r| r.recall_at_5).collect::<Vec<_>>(),
                    ),
                    avg_recall_at_10: calculate_avg(
                        &successful
                            .iter()
                            .map(|r| r.recall_at_10)
                            .collect::<Vec<_>>(),
                    ),
                    avg_precision_at_5: calculate_avg(
                        &successful
                            .iter()
                            .map(|r| r.precision_at_5)
                            .collect::<Vec<_>>(),
                    ),
                    avg_mrr: calculate_avg(&successful.iter().map(|r| r.mrr).collect::<Vec<_>>()),
                    avg_query_time_ms: avg_query_time,
                }
            }
        };

        aggregates.insert(query_type.to_string(), aggregate);
    }

    aggregates
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
            println!(
                "Sample limit: none (running all {} queries)\n",
                dataset.queries.len()
            );
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

    // Generate query type metrics (structural vs ranked)
    let mut query_type_metrics = HashMap::new();
    for st in &search_types_evaluated {
        query_type_metrics.insert(st.clone(), aggregate_by_query_type(&results, st));
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
        query_type_metrics,
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

    println!("\n=== Metrics by Query Type ===\n");
    for st in &search_types_evaluated {
        println!("Search Type: {}", st);
        if let Some(qt_metrics) = report.query_type_metrics.get(st) {
            // Structural queries
            if let Some(structural) = qt_metrics.get("structural") {
                println!("  STRUCTURAL (n={}):", structural.query_count);
                println!(
                    "    SetCov={} SetPrec={} F1={} ContCov={} Time={:.0}ms",
                    structural
                        .avg_set_coverage
                        .map(|v| format!("{:.3}", v))
                        .unwrap_or_else(|| "N/A".into()),
                    structural
                        .avg_set_precision
                        .map(|v| format!("{:.3}", v))
                        .unwrap_or_else(|| "N/A".into()),
                    structural
                        .avg_f1_score
                        .map(|v| format!("{:.3}", v))
                        .unwrap_or_else(|| "N/A".into()),
                    structural
                        .avg_contains_coverage
                        .map(|v| format!("{:.3}", v))
                        .unwrap_or_else(|| "N/A".into()),
                    structural.avg_query_time_ms,
                );
            }

            // Ranked queries
            if let Some(ranked) = qt_metrics.get("ranked") {
                println!("  RANKED (n={}):", ranked.query_count);
                println!(
                    "    Recall@5={} Recall@10={} Prec@5={} MRR={} Time={:.0}ms",
                    ranked
                        .avg_recall_at_5
                        .map(|v| format!("{:.3}", v))
                        .unwrap_or_else(|| "N/A".into()),
                    ranked
                        .avg_recall_at_10
                        .map(|v| format!("{:.3}", v))
                        .unwrap_or_else(|| "N/A".into()),
                    ranked
                        .avg_precision_at_5
                        .map(|v| format!("{:.3}", v))
                        .unwrap_or_else(|| "N/A".into()),
                    ranked
                        .avg_mrr
                        .map(|v| format!("{:.3}", v))
                        .unwrap_or_else(|| "N/A".into()),
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

    // Save name matching analysis for investigating Issue 1
    write_name_matching_analysis()?;

    Ok(())
}
