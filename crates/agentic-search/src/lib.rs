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
//! - [`AgenticSearchRequest`] - Input request model
//! - [`AgenticSearchResponse`] - Output response model
//! - [`AgenticSearchConfig`] - Configuration
//! - [`AgenticSearchError`] - Error types
//! - [`Result`] - Result type alias
//!
//! All implementation details (content selection, prompts, internal types) are
//! private and not exported.

#![deny(warnings)]
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]

// Public modules - exported API
pub mod config;
pub mod error;
pub mod orchestrator;
pub mod types;

// Public re-exports - narrow API surface
pub use config::AgenticSearchConfig;
pub use error::{AgenticSearchError, Result};
pub use orchestrator::AgenticSearchOrchestrator;
pub use types::{AgenticEntity, AgenticSearchRequest, AgenticSearchResponse, RetrievalSource};

// Private modules - implementation details not exported
mod content_selection;
mod prompts;
mod worker;
