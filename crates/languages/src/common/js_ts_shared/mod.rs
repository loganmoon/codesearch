//! Shared infrastructure for JavaScript and TypeScript extractors
//!
//! This module contains code that is shared between JavaScript and TypeScript
//! language extractors, including:
//!
//! - **Scope patterns**: AST node patterns that contribute to qualified names
//! - **Module path**: Logic to derive module paths from file paths

#![deny(warnings)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]

pub mod module_path;
pub mod scope_patterns;

// Scope patterns are needed by the spec-driven extractors
pub use scope_patterns::{SCOPE_PATTERNS, TS_SCOPE_PATTERNS};
