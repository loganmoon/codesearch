//! Retrieval and search evaluation tools for codesearch.
//!
//! This crate provides:
//! - Metrics computation for evaluating retrieval quality (Recall@k, MRR, Hit Rate)
//! - Tools for extracting evaluation candidates from indexed repositories
//! - Harnesses for running semantic and agentic search evaluations

pub mod eval_metrics;

pub use eval_metrics::{EvaluationResults, Metrics, QueryResult};
