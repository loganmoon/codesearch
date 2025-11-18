//! REST API server for semantic code search
//!
//! This crate provides the REST API server for codesearch. It integrates filesystem
//! watching for real-time index updates.

#![deny(warnings)]
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]

// Public modules
pub mod graph_queries;
pub mod rest_server;

// Private modules
mod storage_init;

// Re-export error types from core
pub use codesearch_core::error::{Error, Result};
