//! Evaluation metrics for semantic search.
//!
//! This module provides metrics computation for evaluating retrieval quality
//! in single-answer ground truth scenarios (one correct entity per query).
//!
//! Metrics implemented:
//! - **Recall@k**: Fraction of queries where the ground truth appears in top k results
//! - **MRR (Mean Reciprocal Rank)**: Average of 1/rank for found ground truth entities
//! - **Hit Rate@k**: Same as Recall@k for single-answer queries

use serde::{Deserialize, Serialize};

/// A single evaluation result recording the rank of the ground truth entity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryResult {
    /// Unique identifier for the query
    pub query_id: String,
    /// The rank at which the ground truth entity was found (1-indexed).
    /// None if the entity was not found in the results.
    pub rank: Option<usize>,
    /// Optional: the score of the ground truth entity if found
    pub score: Option<f32>,
}

/// Collection of evaluation results for computing metrics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EvaluationResults {
    /// Individual query results
    results: Vec<QueryResult>,
}

impl EvaluationResults {
    /// Create a new empty results collection.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a result for a query.
    ///
    /// # Arguments
    /// * `query_id` - Unique identifier for the query
    /// * `rank` - 1-indexed rank of ground truth entity, or None if not found
    /// * `score` - Optional relevance score
    pub fn record(&mut self, query_id: impl Into<String>, rank: Option<usize>, score: Option<f32>) {
        self.results.push(QueryResult {
            query_id: query_id.into(),
            rank,
            score,
        });
    }

    /// Record a result with just query_id and rank.
    pub fn record_simple(&mut self, query_id: impl Into<String>, rank: Option<usize>) {
        self.record(query_id, rank, None);
    }

    /// Get the number of queries evaluated.
    pub fn len(&self) -> usize {
        self.results.len()
    }

    /// Check if no queries have been evaluated.
    pub fn is_empty(&self) -> bool {
        self.results.is_empty()
    }

    /// Compute Recall@k: fraction of queries where ground truth appears in top k.
    ///
    /// For single-answer ground truth, this is equivalent to Hit Rate@k.
    ///
    /// # Arguments
    /// * `k` - The cutoff rank
    ///
    /// # Returns
    /// A value between 0.0 and 1.0
    pub fn recall_at(&self, k: usize) -> f64 {
        if self.results.is_empty() {
            return 0.0;
        }

        let hits = self
            .results
            .iter()
            .filter(|r| r.rank.is_some_and(|rank| rank <= k))
            .count();

        hits as f64 / self.results.len() as f64
    }

    /// Compute Mean Reciprocal Rank (MRR).
    ///
    /// MRR = (1/N) * sum(1/rank_i) for queries where ground truth was found.
    /// Queries where the ground truth was not found contribute 0 to the sum.
    ///
    /// # Returns
    /// A value between 0.0 and 1.0
    pub fn mrr(&self) -> f64 {
        if self.results.is_empty() {
            return 0.0;
        }

        let reciprocal_sum: f64 = self
            .results
            .iter()
            .filter_map(|r| r.rank)
            .map(|rank| 1.0 / rank as f64)
            .sum();

        reciprocal_sum / self.results.len() as f64
    }

    /// Alias for recall_at(k) - Hit Rate is the same for single-answer ground truth.
    pub fn hit_rate_at(&self, k: usize) -> f64 {
        self.recall_at(k)
    }

    /// Get the number of queries where ground truth was found.
    pub fn found_count(&self) -> usize {
        self.results.iter().filter(|r| r.rank.is_some()).count()
    }

    /// Get the number of queries where ground truth was NOT found.
    pub fn not_found_count(&self) -> usize {
        self.results.iter().filter(|r| r.rank.is_none()).count()
    }

    /// Get query IDs where the ground truth was not found.
    pub fn not_found_queries(&self) -> Vec<&str> {
        self.results
            .iter()
            .filter(|r| r.rank.is_none())
            .map(|r| r.query_id.as_str())
            .collect()
    }

    /// Compute all standard metrics and return as a struct.
    pub fn compute_metrics(&self) -> Metrics {
        Metrics {
            total_queries: self.len(),
            found_count: self.found_count(),
            not_found_count: self.not_found_count(),
            recall_at_1: self.recall_at(1),
            recall_at_5: self.recall_at(5),
            recall_at_10: self.recall_at(10),
            recall_at_20: self.recall_at(20),
            mrr: self.mrr(),
        }
    }

    /// Print a summary of metrics to stdout.
    pub fn print_metrics(&self) {
        let m = self.compute_metrics();
        println!("Semantic Search Evaluation Results (n={}):", m.total_queries);
        println!(
            "  Found: {} ({:.1}%)",
            m.found_count,
            (m.found_count as f64 / m.total_queries as f64) * 100.0
        );
        println!("  Not found: {}", m.not_found_count);
        println!("  Recall@1:  {:.1}%", m.recall_at_1 * 100.0);
        println!("  Recall@5:  {:.1}%", m.recall_at_5 * 100.0);
        println!("  Recall@10: {:.1}%", m.recall_at_10 * 100.0);
        println!("  Recall@20: {:.1}%", m.recall_at_20 * 100.0);
        println!("  MRR:       {:.3}", m.mrr);

        if !self.not_found_queries().is_empty() {
            println!("\nQueries where ground truth was not found:");
            for qid in self.not_found_queries() {
                println!("  - {qid}");
            }
        }
    }

    /// Get detailed results for analysis.
    pub fn results(&self) -> &[QueryResult] {
        &self.results
    }
}

/// Computed metrics summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Metrics {
    /// Total number of queries evaluated
    pub total_queries: usize,
    /// Number of queries where ground truth was found
    pub found_count: usize,
    /// Number of queries where ground truth was NOT found
    pub not_found_count: usize,
    /// Recall@1 (Hit Rate@1)
    pub recall_at_1: f64,
    /// Recall@5 (Hit Rate@5)
    pub recall_at_5: f64,
    /// Recall@10 (Hit Rate@10)
    pub recall_at_10: f64,
    /// Recall@20 (Hit Rate@20)
    pub recall_at_20: f64,
    /// Mean Reciprocal Rank
    pub mrr: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_results() {
        let results = EvaluationResults::new();
        assert_eq!(results.len(), 0);
        assert!(results.is_empty());
        assert_eq!(results.recall_at(10), 0.0);
        assert_eq!(results.mrr(), 0.0);
    }

    #[test]
    fn test_perfect_recall() {
        let mut results = EvaluationResults::new();
        results.record_simple("q1", Some(1));
        results.record_simple("q2", Some(1));
        results.record_simple("q3", Some(1));

        assert_eq!(results.recall_at(1), 1.0);
        assert_eq!(results.recall_at(5), 1.0);
        assert_eq!(results.mrr(), 1.0);
    }

    #[test]
    fn test_partial_recall() {
        let mut results = EvaluationResults::new();
        results.record_simple("q1", Some(1)); // Found at rank 1
        results.record_simple("q2", Some(3)); // Found at rank 3
        results.record_simple("q3", Some(7)); // Found at rank 7
        results.record_simple("q4", None); // Not found

        // Recall@1: 1/4 = 0.25
        assert!((results.recall_at(1) - 0.25).abs() < 0.001);

        // Recall@5: 2/4 = 0.5 (ranks 1 and 3)
        assert!((results.recall_at(5) - 0.5).abs() < 0.001);

        // Recall@10: 3/4 = 0.75 (ranks 1, 3, and 7)
        assert!((results.recall_at(10) - 0.75).abs() < 0.001);

        // MRR: (1/1 + 1/3 + 1/7 + 0) / 4 = (1 + 0.333 + 0.143) / 4 = 0.369
        let expected_mrr = (1.0 + 1.0 / 3.0 + 1.0 / 7.0) / 4.0;
        assert!((results.mrr() - expected_mrr).abs() < 0.001);
    }

    #[test]
    fn test_none_found() {
        let mut results = EvaluationResults::new();
        results.record_simple("q1", None);
        results.record_simple("q2", None);

        assert_eq!(results.recall_at(10), 0.0);
        assert_eq!(results.mrr(), 0.0);
        assert_eq!(results.not_found_count(), 2);
    }

    #[test]
    fn test_not_found_queries() {
        let mut results = EvaluationResults::new();
        results.record_simple("q1", Some(1));
        results.record_simple("q2", None);
        results.record_simple("q3", Some(5));
        results.record_simple("q4", None);

        let not_found = results.not_found_queries();
        assert_eq!(not_found.len(), 2);
        assert!(not_found.contains(&"q2"));
        assert!(not_found.contains(&"q4"));
    }

    #[test]
    fn test_metrics_struct() {
        let mut results = EvaluationResults::new();
        results.record_simple("q1", Some(1));
        results.record_simple("q2", Some(3));

        let metrics = results.compute_metrics();
        assert_eq!(metrics.total_queries, 2);
        assert_eq!(metrics.found_count, 2);
        assert_eq!(metrics.not_found_count, 0);
    }
}
