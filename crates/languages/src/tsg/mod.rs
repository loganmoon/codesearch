//! Tree-sitter-graph based extraction for cross-file FQN resolution
//!
//! This module provides a uniform DSL-based approach to extracting:
//! - Definition nodes (struct, fn, trait, etc.)
//! - Export nodes (pub use re-exports)
//! - Import nodes (use declarations)
//! - Reference nodes (identifier usages)
//!
//! These nodes form a resolution graph where cross-file FQN resolution
//! follows edges: Reference → Import → Definition
//! (Note: Export nodes are handled via `is_public` attribute on Import nodes)

#![deny(warnings)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]

pub mod evaluation;
pub mod executor;
pub mod graph_types;
pub mod resolution;

pub mod cross_file_eval;

#[cfg(test)]
pub mod codebase_eval;

pub use cross_file_eval::{
    evaluate_cross_file_resolution, evaluate_cross_file_resolution_with_config,
    CrossFileEvalConfig, CrossFileEvalStats,
};
pub use evaluation::{build_intra_file_edges, categorize_unresolved, EvaluationResult};
pub use executor::{
    TsgExecutor, JAVASCRIPT_TSG_RULES, PYTHON_TSG_RULES, RUST_TSG_RULES, TYPESCRIPT_TSG_RULES,
};
pub use graph_types::{ResolutionEdge, ResolutionEdgeKind, ResolutionNode, ResolutionNodeKind};
pub use resolution::{
    queries as resolution_queries, ResolutionResult, ResolutionSession, ResolutionStats,
};
