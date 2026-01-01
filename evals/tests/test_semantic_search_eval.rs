//! Semantic search evaluation harness.
//!
//! Evaluates semantic search quality using single-answer ground truth.
//! Each query has exactly one correct entity, and we measure how often
//! and where that entity appears in search results.
//!
//! Usage:
//!   cargo test --manifest-path evals/Cargo.toml --test test_semantic_search_eval -- --ignored --nocapture
//!
//! To include agentic search comparison:
//!   EVAL_AGENTIC=1 cargo test --manifest-path evals/Cargo.toml --test test_semantic_search_eval -- --ignored --nocapture
//!
//! Requirements:
//! - codesearch server running on localhost:3000
//! - Nushell repository indexed at the specific commit noted in the eval JSON metadata
//!   (entity IDs are content-based hashes and will differ on other commits)

use anyhow::{Context, Result};
use codesearch_evals::EvaluationResults;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Base URL for the codesearch API
const API_BASE_URL: &str = "http://localhost:3000";

/// Path to the evaluation queries file
const EVAL_QUERIES_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/data/nushell_semantic_eval.json"
);

/// Evaluation query with ground truth
#[derive(Debug, Clone, Deserialize)]
struct EvalQuery {
    id: String,
    query: String,
    bge_instruction: Option<String>,
    ground_truth: GroundTruth,
}

/// Ground truth for a single query
#[derive(Debug, Clone, Deserialize)]
struct GroundTruth {
    entity_id: String,
    qualified_name: String,
    #[allow(dead_code)]
    entity_type: String,
}

/// Evaluation dataset metadata
#[derive(Debug, Clone, Deserialize)]
struct EvalMetadata {
    #[allow(dead_code)]
    created: String,
    #[allow(dead_code)]
    purpose: String,
    total_queries: usize,
    source_repository: String,
    /// Git commit hash the ground truth was extracted from
    source_commit: String,
    #[allow(dead_code)]
    generation_method: String,
}

/// Full evaluation dataset
#[derive(Debug, Clone, Deserialize)]
struct EvalDataset {
    metadata: EvalMetadata,
    queries: Vec<EvalQuery>,
}

/// Query specification for API request
#[derive(Debug, Serialize)]
struct QuerySpec {
    text: String,
    instruction: Option<String>,
}

/// Semantic search request
#[derive(Debug, Serialize)]
struct SemanticSearchRequest {
    repository_ids: Option<Vec<Uuid>>,
    query: QuerySpec,
    limit: usize,
}

/// Entity result from API response
#[derive(Debug, Deserialize)]
struct EntityResult {
    entity_id: String,
    qualified_name: String,
    #[allow(dead_code)]
    name: String,
    #[allow(dead_code)]
    entity_type: String,
    score: f32,
}

/// Response metadata
#[derive(Debug, Deserialize)]
struct ResponseMetadata {
    #[allow(dead_code)]
    total_results: usize,
    #[allow(dead_code)]
    repositories_searched: usize,
    #[allow(dead_code)]
    reranked: bool,
    query_time_ms: u64,
}

/// Semantic search response
#[derive(Debug, Deserialize)]
struct SemanticSearchResponse {
    results: Vec<EntityResult>,
    metadata: ResponseMetadata,
}

/// Agentic search request
#[derive(Debug, Serialize)]
struct AgenticSearchRequest {
    query: String,
    repository_ids: Vec<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    force_sonnet: bool,
}

/// Agentic search response metadata
#[derive(Debug, Deserialize)]
struct AgenticResponseMetadata {
    query_time_ms: u64,
    #[allow(dead_code)]
    iterations: usize,
    #[allow(dead_code)]
    workers_spawned: usize,
    #[allow(dead_code)]
    workers_succeeded: usize,
}

/// Agentic search response
#[derive(Debug, Deserialize)]
struct AgenticSearchResponse {
    results: Vec<AgenticEntityResult>,
    metadata: AgenticResponseMetadata,
}

/// Entity result from agentic search
#[derive(Debug, Deserialize)]
struct AgenticEntityResult {
    entity_id: String,
    qualified_name: String,
    #[allow(dead_code)]
    name: String,
    #[allow(dead_code)]
    entity_type: String,
    score: f32,
}

/// Repository info from API
#[derive(Debug, Deserialize)]
struct RepositoryInfo {
    repository_id: Uuid,
    repository_name: String,
    #[allow(dead_code)]
    collection_name: String,
}

/// Repositories list response
#[derive(Debug, Deserialize)]
struct ListRepositoriesResponse {
    repositories: Vec<RepositoryInfo>,
}

/// Load evaluation queries from JSON file
fn load_eval_queries(path: &str) -> Result<EvalDataset> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read evaluation file: {path}"))?;
    let dataset: EvalDataset =
        serde_json::from_str(&content).context("Failed to parse evaluation JSON")?;
    Ok(dataset)
}

/// Find repository ID by name pattern
async fn find_repository(client: &Client, name_pattern: &str) -> Result<Uuid> {
    let url = format!("{API_BASE_URL}/api/v1/repositories");
    let response = client
        .get(&url)
        .send()
        .await
        .context("Failed to fetch repositories")?;

    if !response.status().is_success() {
        anyhow::bail!("Failed to list repositories: {}", response.status());
    }

    let body: ListRepositoriesResponse = response
        .json()
        .await
        .context("Failed to parse repositories response")?;

    let repo = body
        .repositories
        .iter()
        .find(|r| {
            r.repository_name
                .to_lowercase()
                .contains(&name_pattern.to_lowercase())
        })
        .with_context(|| format!("No repository found matching '{name_pattern}'"))?;

    println!(
        "Found repository: {} (id: {})",
        repo.repository_name, repo.repository_id
    );
    Ok(repo.repository_id)
}

/// Execute a single semantic search query
async fn execute_search(
    client: &Client,
    query: &EvalQuery,
    repository_id: Uuid,
    limit: usize,
) -> Result<SemanticSearchResponse> {
    let url = format!("{API_BASE_URL}/api/v1/search/semantic");

    let request = SemanticSearchRequest {
        repository_ids: Some(vec![repository_id]),
        query: QuerySpec {
            text: query.query.clone(),
            instruction: query.bge_instruction.clone(),
        },
        limit,
    };

    let response = client
        .post(&url)
        .json(&request)
        .send()
        .await
        .with_context(|| format!("Failed to execute search for query '{}'", query.id))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("Search failed for '{}': {} - {}", query.id, status, body);
    }

    response
        .json()
        .await
        .with_context(|| format!("Failed to parse search response for '{}'", query.id))
}

/// Find the rank of the ground truth entity in search results
fn find_ground_truth_rank(
    response: &SemanticSearchResponse,
    ground_truth: &GroundTruth,
) -> Option<usize> {
    for (i, result) in response.results.iter().enumerate() {
        // Match by entity_id (primary) or qualified_name (fallback)
        if result.entity_id == ground_truth.entity_id
            || result.qualified_name == ground_truth.qualified_name
        {
            return Some(i + 1); // 1-indexed rank
        }
    }
    None
}

/// Execute a single agentic search query
async fn execute_agentic_search(
    client: &Client,
    query: &EvalQuery,
    repository_id: Uuid,
) -> Result<AgenticSearchResponse> {
    let url = format!("{API_BASE_URL}/api/v1/search/agentic");

    let request = AgenticSearchRequest {
        query: query.query.clone(),
        repository_ids: vec![repository_id.to_string()],
        force_sonnet: false,
    };

    let response = client
        .post(&url)
        .json(&request)
        .send()
        .await
        .with_context(|| format!("Failed to execute agentic search for query '{}'", query.id))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!(
            "Agentic search failed for '{}': {} - {}",
            query.id,
            status,
            body
        );
    }

    response
        .json()
        .await
        .with_context(|| format!("Failed to parse agentic search response for '{}'", query.id))
}

/// Find the rank of the ground truth entity in agentic search results
fn find_ground_truth_rank_agentic(
    response: &AgenticSearchResponse,
    ground_truth: &GroundTruth,
) -> Option<usize> {
    for (i, result) in response.results.iter().enumerate() {
        // Match by entity_id (primary) or qualified_name (fallback)
        if result.entity_id == ground_truth.entity_id
            || result.qualified_name == ground_truth.qualified_name
        {
            return Some(i + 1); // 1-indexed rank
        }
    }
    None
}

/// Check if agentic evaluation is enabled via environment variable
fn agentic_eval_enabled() -> bool {
    std::env::var("EVAL_AGENTIC")
        .map(|v| v == "1" || v.to_lowercase() == "true")
        .unwrap_or(false)
}

#[tokio::test]
#[ignore] // Requires running codesearch server with indexed Nushell repository
async fn test_semantic_search_evaluation() -> Result<()> {
    // Load evaluation queries
    let dataset = match load_eval_queries(EVAL_QUERIES_PATH) {
        Ok(d) => d,
        Err(e) => {
            println!("Note: Could not load evaluation queries: {e}");
            println!("This is expected if the ground truth file hasn't been created yet.");
            println!("Run extract_eval_candidates and manually curate queries first.");
            return Ok(());
        }
    };

    println!(
        "Loaded {} evaluation queries",
        dataset.metadata.total_queries
    );
    println!("Source repository: {}", dataset.metadata.source_repository);
    println!("Expected commit:   {}", dataset.metadata.source_commit);
    println!();
    println!("NOTE: Entity IDs are content-based hashes. The indexed repository");
    println!("      must be at the exact commit above for ground truth to match.");

    let run_agentic = agentic_eval_enabled();
    if run_agentic {
        println!("Agentic search comparison: ENABLED");
    }

    // Create HTTP client (longer timeout for agentic search which uses LLM)
    let timeout_secs = if run_agentic { 120 } else { 30 };
    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(timeout_secs))
        .build()
        .context("Failed to create HTTP client")?;

    // Find the Nushell repository
    let repo_name = dataset
        .metadata
        .source_repository
        .split('/')
        .next_back()
        .unwrap_or(&dataset.metadata.source_repository);
    let repository_id = find_repository(&client, repo_name).await?;

    // Run semantic search evaluation
    let mut semantic_results = EvaluationResults::new();
    let mut agentic_results = EvaluationResults::new();
    let search_limit = 20;
    let mut semantic_time_ms = 0u64;
    let mut agentic_time_ms = 0u64;

    println!("\n{:=<70}", "");
    println!("SEMANTIC SEARCH EVALUATION");
    println!("{:=<70}", "");

    for query in &dataset.queries {
        let search_response = execute_search(&client, query, repository_id, search_limit).await?;
        let rank = find_ground_truth_rank(&search_response, &query.ground_truth);

        semantic_time_ms += search_response.metadata.query_time_ms;

        // Record result with score if found
        let score = rank.and_then(|r| search_response.results.get(r - 1).map(|e| e.score));
        semantic_results.record(&query.id, rank, score);

        // Print per-query result
        let status = format_rank_status(rank);
        println!(
            "[{:8}] {} - {}",
            status,
            query.id,
            truncate(&query.query, 45)
        );
    }

    println!("{:-<70}", "");
    println!();

    // Print semantic search metrics
    println!("SEMANTIC SEARCH METRICS:");
    semantic_results.print_metrics();

    let semantic_metrics = semantic_results.compute_metrics();
    println!(
        "\n  Avg query time:    {:.0} ms",
        semantic_time_ms as f64 / semantic_metrics.total_queries as f64
    );
    println!("  Search limit:      {search_limit}");

    // Run agentic search evaluation if enabled
    if run_agentic {
        println!("\n{:=<70}", "");
        println!("AGENTIC SEARCH EVALUATION");
        println!("{:=<70}", "");

        for query in &dataset.queries {
            match execute_agentic_search(&client, query, repository_id).await {
                Ok(search_response) => {
                    let rank =
                        find_ground_truth_rank_agentic(&search_response, &query.ground_truth);
                    agentic_time_ms += search_response.metadata.query_time_ms;

                    // Record result with score if found
                    let score =
                        rank.and_then(|r| search_response.results.get(r - 1).map(|e| e.score));
                    agentic_results.record(&query.id, rank, score);

                    let status = format_rank_status(rank);
                    println!(
                        "[{:8}] {} - {}",
                        status,
                        query.id,
                        truncate(&query.query, 45)
                    );
                }
                Err(e) => {
                    println!(
                        "[  ERROR ] {} - {} ({})",
                        query.id,
                        truncate(&query.query, 35),
                        e
                    );
                    agentic_results.record(&query.id, None, None);
                }
            }
        }

        println!("{:-<70}", "");
        println!();

        // Print agentic search metrics
        println!("AGENTIC SEARCH METRICS:");
        agentic_results.print_metrics();

        let agentic_metrics = agentic_results.compute_metrics();
        if agentic_metrics.total_queries > 0 {
            println!(
                "\n  Avg query time:    {:.0} ms",
                agentic_time_ms as f64 / agentic_metrics.total_queries as f64
            );
        }

        // Print comparison
        println!("\n{:=<70}", "");
        println!("COMPARISON: SEMANTIC vs AGENTIC");
        println!("{:=<70}", "");
        println!(
            "{:<20} {:>15} {:>15} {:>12}",
            "Metric", "Semantic", "Agentic", "Delta"
        );
        println!("{:-<70}", "");
        print_comparison_row(
            "Recall@1",
            semantic_metrics.recall_at_1,
            agentic_metrics.recall_at_1,
        );
        print_comparison_row(
            "Recall@5",
            semantic_metrics.recall_at_5,
            agentic_metrics.recall_at_5,
        );
        print_comparison_row(
            "Recall@10",
            semantic_metrics.recall_at_10,
            agentic_metrics.recall_at_10,
        );
        print_comparison_row(
            "Recall@20",
            semantic_metrics.recall_at_20,
            agentic_metrics.recall_at_20,
        );
        print_comparison_row("MRR", semantic_metrics.mrr, agentic_metrics.mrr);
        println!("{:-<70}", "");
        println!(
            "{:<20} {:>12.0} ms {:>12.0} ms {:>11.1}x",
            "Avg Query Time",
            semantic_time_ms as f64 / semantic_metrics.total_queries as f64,
            agentic_time_ms as f64 / agentic_metrics.total_queries.max(1) as f64,
            (agentic_time_ms as f64 / agentic_metrics.total_queries.max(1) as f64)
                / (semantic_time_ms as f64 / semantic_metrics.total_queries as f64).max(1.0)
        );
    }

    // Assert minimum quality thresholds (can be adjusted)
    // These are intentionally low initially - raise them as the system improves
    if semantic_metrics.total_queries >= 10 {
        assert!(
            semantic_metrics.recall_at_10 >= 0.3,
            "Recall@10 ({:.1}%) below minimum threshold (30%)",
            semantic_metrics.recall_at_10 * 100.0
        );
    }

    Ok(())
}

/// Format rank as a status string for display
fn format_rank_status(rank: Option<usize>) -> String {
    match rank {
        Some(1) => "HIT@1".to_string(),
        Some(r) if r <= 5 => format!("HIT@{r}"),
        Some(r) if r <= 10 => format!("hit@{r}"),
        Some(r) => format!("found@{r}"),
        None => "MISS".to_string(),
    }
}

/// Print a comparison row with delta
fn print_comparison_row(metric: &str, semantic: f64, agentic: f64) {
    let delta = agentic - semantic;
    let delta_str = if delta > 0.001 {
        format!("+{:.1}%", delta * 100.0)
    } else if delta < -0.001 {
        format!("{:.1}%", delta * 100.0)
    } else {
        "0.0%".to_string()
    };
    println!(
        "{:<20} {:>14.1}% {:>14.1}% {:>12}",
        metric,
        semantic * 100.0,
        agentic * 100.0,
        delta_str
    );
}

/// Truncate a string to max length with ellipsis
fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len - 3])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate() {
        assert_eq!(truncate("short", 10), "short");
        assert_eq!(truncate("this is a longer string", 10), "this is...");
    }

    #[test]
    fn test_find_ground_truth_rank() {
        let response = SemanticSearchResponse {
            results: vec![
                EntityResult {
                    entity_id: "e1".to_string(),
                    qualified_name: "foo::bar".to_string(),
                    name: "bar".to_string(),
                    entity_type: "function".to_string(),
                    score: 0.9,
                },
                EntityResult {
                    entity_id: "e2".to_string(),
                    qualified_name: "baz::qux".to_string(),
                    name: "qux".to_string(),
                    entity_type: "function".to_string(),
                    score: 0.8,
                },
            ],
            metadata: ResponseMetadata {
                total_results: 2,
                repositories_searched: 1,
                reranked: false,
                query_time_ms: 100,
            },
        };

        let gt1 = GroundTruth {
            entity_id: "e1".to_string(),
            qualified_name: "foo::bar".to_string(),
            entity_type: "function".to_string(),
        };
        assert_eq!(find_ground_truth_rank(&response, &gt1), Some(1));

        let gt2 = GroundTruth {
            entity_id: "e2".to_string(),
            qualified_name: "baz::qux".to_string(),
            entity_type: "function".to_string(),
        };
        assert_eq!(find_ground_truth_rank(&response, &gt2), Some(2));

        let gt_missing = GroundTruth {
            entity_id: "e3".to_string(),
            qualified_name: "not::found".to_string(),
            entity_type: "function".to_string(),
        };
        assert_eq!(find_ground_truth_rank(&response, &gt_missing), None);
    }
}
