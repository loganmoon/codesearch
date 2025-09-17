#![deny(warnings)]
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]

mod rust;

// Unified entity builders
pub mod generic_entities;

// Transport model for intermediate entity representation
pub mod transport;

// Generic data-driven extractor framework
pub mod extraction_framework;

// Re-export commonly used types
// pub use language::Language;
