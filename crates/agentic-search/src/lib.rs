//! Agentic search with multi-agent orchestration and dual-track pipeline
//!
//! This crate provides intelligent code search using multiple AI agents for
//! orchestration, parallel search execution, and multi-stage reranking.
//!
//! # Public API
//!
//! This crate exports a minimal public API following the principle of limiting
//! public exports to traits, models, errors, and factory functions:
//!
//! ## Main Entry Point
//! - [`AgenticSearchOrchestrator`] - Main orchestrator that executes agentic search
//!
//! ## Request/Response Models
//! - [`AgenticSearchRequest`] - Input request model
//! - [`AgenticSearchResponse`] - Output response model with results and metadata
//! - [`AgenticSearchMetadata`] - Execution metadata (iterations, cost, etc.)
//! - [`AgenticEntity`] - Entity result enriched with retrieval source
//! - [`RetrievalSource`] - How an entity was retrieved (semantic, fulltext, graph)
//! - [`RerankingMethod`] - Which reranking method was used
//!
//! ## Configuration
//! - [`AgenticSearchConfig`] - Main configuration
//! - [`QualityGateConfig`] - Quality gate thresholds
//!
//! ## Error Handling
//! - [`AgenticSearchError`] - Error types
//! - [`Result`] - Result type alias
//!
//! All implementation details (content selection, prompts, internal types) are
//! private and not exported.

#![deny(warnings)]
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]

// Private modules - implementation details
mod config;
mod content_selection;
mod error;
mod orchestrator;
mod prompts;
mod types;
mod worker;

// Public re-exports - narrow API surface
pub use config::{AgenticSearchConfig, QualityGateConfig};
pub use error::{AgenticSearchError, Result};
pub use orchestrator::AgenticSearchOrchestrator;
pub use types::{
    AgenticEntity, AgenticSearchMetadata, AgenticSearchRequest, AgenticSearchResponse,
    RerankingMethod, RetrievalSource,
};
