//! Specification-based graph validation tests
//!
//! These tests validate that the code graph extraction pipeline correctly
//! identifies entities and relationships from source code by comparing
//! against hand-verified expected specifications.
//!
//! Run all:
//!   cargo test --manifest-path crates/e2e-tests/Cargo.toml spec_validation -- --ignored
//!
//! Run by language:
//!   cargo test --manifest-path crates/e2e-tests/Cargo.toml spec_validation::rust -- --ignored
//!   cargo test --manifest-path crates/e2e-tests/Cargo.toml spec_validation::typescript -- --ignored

mod rust;
mod typescript;
