//! Rust language extractor module
//!
//! This module previously contained the old handler-based extractor.
//! Rust extraction is now handled by the spec-driven engine in
//! `spec_driven::extractors::SpecDrivenRustExtractor`.
//!
//! The following submodules are still used by other parts of the system:
//! - `edge_case_handlers`: Rust-specific FQN edge case handling
//! - `import_resolution`: Import statement parsing for qualified name resolution
//! - `module_path`: Module path derivation from file paths

pub mod edge_case_handlers;
pub mod import_resolution;
pub mod module_path;
