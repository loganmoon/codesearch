//! LSP Validation Framework
//!
//! This crate provides tools to validate codesearch's relationship extraction
//! against Language Server Protocol (LSP) ground truth. It measures precision,
//! recall, and F1 scores by comparing Neo4j relationships against LSP
//! "find references" results.
//!
//! ## Supported Language Servers
//!
//! - TypeScript/JavaScript: `typescript-language-server`
//! - Python: `pyright-langserver`
//! - Rust: `rust-analyzer`
//!
//! ## Validation Approach
//!
//! For each target entity in the graph:
//! 1. Query LSP `textDocument/references` at the entity's definition location
//! 2. For each reference location LSP returns, find which of our entities contains it
//! 3. Check if we have an edge from that entity to the target in Neo4j
//!
//! This gives us:
//! - **Precision**: Of our edges, how many does LSP confirm?
//! - **Recall**: Of LSP's references, how many do we have edges for?

#![deny(warnings)]
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]

pub mod lsp_client;
pub mod metrics;
pub mod report;
pub mod validation;

pub use lsp_client::{LspClient, LspServer};
pub use metrics::RelationshipMetrics;
pub use report::{Discrepancy, ValidationReport};
pub use validation::{Neo4jEdge, ValidationEngine, ValidationResult};
