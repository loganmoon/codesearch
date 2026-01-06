//! Shared infrastructure for JavaScript and TypeScript extractors
//!
//! This module contains code that is shared between JavaScript and TypeScript
//! language extractors, including:
//!
//! - **Scope patterns**: AST node patterns that contribute to qualified names
//! - **Visibility extraction**: Logic to determine entity visibility from exports
//! - **Queries**: Tree-sitter query patterns for entity extraction
//! - **Handlers**: Entity handler implementations

#![deny(warnings)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]

pub mod extractors;
pub(crate) mod handlers;
pub(crate) mod queries;
pub mod scope_patterns;
pub(crate) mod visibility;

// Re-export language extractors for use with define_handler! macro
pub use extractors::{JavaScript, TypeScript};

// Scope patterns are needed by the macro (public)
pub use scope_patterns::{SCOPE_PATTERNS, TS_SCOPE_PATTERNS};
