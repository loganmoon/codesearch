//! Graph evaluation tests for TSG resolution
//!
//! This module contains integration tests that evaluate TSG extraction and
//! cross-file resolution on real codebases, targeting 80%+ resolution rates.
//!
//! Run all tests:
//!   cargo test -p codesearch-languages --test graph_eval -- --ignored --nocapture
//!
//! Run specific language:
//!   cargo test -p codesearch-languages --test graph_eval rust -- --ignored --nocapture
//!   cargo test -p codesearch-languages --test graph_eval javascript -- --ignored --nocapture
//!   cargo test -p codesearch-languages --test graph_eval typescript -- --ignored --nocapture
//!   cargo test -p codesearch-languages --test graph_eval python -- --ignored --nocapture

#[path = "graph_eval/common.rs"]
mod common;

#[path = "graph_eval/rust_eval.rs"]
mod rust_eval;

#[path = "graph_eval/javascript_eval.rs"]
mod javascript_eval;

#[path = "graph_eval/typescript_eval.rs"]
mod typescript_eval;

#[path = "graph_eval/python_eval.rs"]
mod python_eval;
