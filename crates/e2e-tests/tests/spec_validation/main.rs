//! Specification-based graph validation tests
//!
//! These tests validate that the code graph extraction pipeline correctly
//! identifies entities and relationships from Rust source code by comparing
//! against hand-verified expected specifications.
//!
//! Run with: cargo test --manifest-path crates/e2e-tests/Cargo.toml spec_validation -- --ignored

mod fixtures;

use anyhow::Result;
use codesearch_e2e_tests::common::spec_validation::run_spec_validation;
use fixtures::*;

// =============================================================================
// Test Functions - Basic
// =============================================================================

#[tokio::test]
#[ignore] // Requires Docker
async fn test_spec_validation_basic_mod() -> Result<()> {
    run_spec_validation(&BASIC_MOD).await
}

#[tokio::test]
#[ignore]
async fn test_spec_validation_visibility() -> Result<()> {
    run_spec_validation(&VISIBILITY).await
}

#[tokio::test]
#[ignore]
async fn test_spec_validation_use_imports() -> Result<()> {
    run_spec_validation(&USE_IMPORTS).await
}

#[tokio::test]
#[ignore]
async fn test_spec_validation_reexports() -> Result<()> {
    run_spec_validation(&REEXPORTS).await
}

#[tokio::test]
#[ignore]
async fn test_spec_validation_free_functions() -> Result<()> {
    run_spec_validation(&FREE_FUNCTIONS).await
}

#[tokio::test]
#[ignore]
async fn test_spec_validation_methods() -> Result<()> {
    run_spec_validation(&METHODS).await
}

#[tokio::test]
#[ignore]
async fn test_spec_validation_cross_module_calls() -> Result<()> {
    run_spec_validation(&CROSS_MODULE_CALLS).await
}

#[tokio::test]
#[ignore]
async fn test_spec_validation_structs() -> Result<()> {
    run_spec_validation(&STRUCTS).await
}

#[tokio::test]
#[ignore]
async fn test_spec_validation_enums() -> Result<()> {
    run_spec_validation(&ENUMS).await
}

#[tokio::test]
#[ignore]
async fn test_spec_validation_type_aliases() -> Result<()> {
    run_spec_validation(&TYPE_ALIASES).await
}

#[tokio::test]
#[ignore]
async fn test_spec_validation_trait_def() -> Result<()> {
    run_spec_validation(&TRAIT_DEF).await
}

#[tokio::test]
#[ignore]
async fn test_spec_validation_trait_impl() -> Result<()> {
    run_spec_validation(&TRAIT_IMPL).await
}

#[tokio::test]
#[ignore]
async fn test_spec_validation_supertraits() -> Result<()> {
    run_spec_validation(&SUPERTRAITS).await
}

#[tokio::test]
#[ignore]
async fn test_spec_validation_constants() -> Result<()> {
    run_spec_validation(&CONSTANTS).await
}

#[tokio::test]
#[ignore]
async fn test_spec_validation_statics() -> Result<()> {
    run_spec_validation(&STATICS).await
}

#[tokio::test]
#[ignore]
async fn test_spec_validation_unions() -> Result<()> {
    run_spec_validation(&UNIONS).await
}

#[tokio::test]
#[ignore]
async fn test_spec_validation_extern_blocks() -> Result<()> {
    run_spec_validation(&EXTERN_BLOCKS).await
}

#[tokio::test]
#[ignore]
async fn test_spec_validation_associated_constants() -> Result<()> {
    run_spec_validation(&ASSOCIATED_CONSTANTS).await
}

#[tokio::test]
#[ignore]
async fn test_spec_validation_macro_rules() -> Result<()> {
    run_spec_validation(&MACRO_RULES).await
}

// =============================================================================
// Test Functions - Advanced Module System
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_spec_validation_deep_module_nesting() -> Result<()> {
    run_spec_validation(&DEEP_MODULE_NESTING).await
}

#[tokio::test]
#[ignore]
async fn test_spec_validation_mixed_module_structure() -> Result<()> {
    run_spec_validation(&MIXED_MODULE_STRUCTURE).await
}

#[tokio::test]
#[ignore]
async fn test_spec_validation_self_super_references() -> Result<()> {
    run_spec_validation(&SELF_SUPER_REFERENCES).await
}

// =============================================================================
// Test Functions - Advanced Functions & Calls
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_spec_validation_multiple_impl_blocks() -> Result<()> {
    run_spec_validation(&MULTIPLE_IMPL_BLOCKS).await
}

#[tokio::test]
#[ignore]
async fn test_spec_validation_async_functions() -> Result<()> {
    run_spec_validation(&ASYNC_FUNCTIONS).await
}

#[tokio::test]
#[ignore]
async fn test_spec_validation_builder_pattern() -> Result<()> {
    run_spec_validation(&BUILDER_PATTERN).await
}

#[tokio::test]
#[ignore]
async fn test_spec_validation_recursive_calls() -> Result<()> {
    run_spec_validation(&RECURSIVE_CALLS).await
}

// =============================================================================
// Test Functions - Advanced Types
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_spec_validation_tuple_and_unit_structs() -> Result<()> {
    run_spec_validation(&TUPLE_AND_UNIT_STRUCTS).await
}

#[tokio::test]
#[ignore]
async fn test_spec_validation_complex_enums() -> Result<()> {
    run_spec_validation(&COMPLEX_ENUMS).await
}

#[tokio::test]
#[ignore]
async fn test_spec_validation_generic_structs() -> Result<()> {
    run_spec_validation(&GENERIC_STRUCTS).await
}

#[tokio::test]
#[ignore]
async fn test_spec_validation_lifetimes() -> Result<()> {
    run_spec_validation(&LIFETIMES).await
}

// =============================================================================
// Test Functions - Advanced Traits
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_spec_validation_associated_types() -> Result<()> {
    run_spec_validation(&ASSOCIATED_TYPES).await
}

#[tokio::test]
#[ignore]
async fn test_spec_validation_multiple_trait_impls() -> Result<()> {
    run_spec_validation(&MULTIPLE_TRAIT_IMPLS).await
}

#[tokio::test]
#[ignore]
async fn test_spec_validation_generic_trait() -> Result<()> {
    run_spec_validation(&GENERIC_TRAIT).await
}

// =============================================================================
// Test Functions - Workspace
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_spec_validation_workspace_basic() -> Result<()> {
    run_spec_validation(&WORKSPACE_BASIC).await
}

// =============================================================================
// Test Functions - Hard but Feasible Resolution
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_spec_validation_multi_hop_reexports() -> Result<()> {
    run_spec_validation(&MULTI_HOP_REEXPORTS).await
}

#[tokio::test]
#[ignore]
async fn test_spec_validation_glob_reexports() -> Result<()> {
    run_spec_validation(&GLOB_REEXPORTS).await
}

#[tokio::test]
#[ignore]
async fn test_spec_validation_trait_vs_inherent_method() -> Result<()> {
    run_spec_validation(&TRAIT_VS_INHERENT_METHOD).await
}

#[tokio::test]
#[ignore]
async fn test_spec_validation_scattered_impl_blocks() -> Result<()> {
    run_spec_validation(&SCATTERED_IMPL_BLOCKS).await
}

#[tokio::test]
#[ignore]
async fn test_spec_validation_associated_types_resolution() -> Result<()> {
    run_spec_validation(&ASSOCIATED_TYPES_RESOLUTION).await
}

#[tokio::test]
#[ignore]
async fn test_spec_validation_prelude_shadowing() -> Result<()> {
    run_spec_validation(&PRELUDE_SHADOWING).await
}

#[tokio::test]
#[ignore]
async fn test_spec_validation_generic_bounds_resolution() -> Result<()> {
    run_spec_validation(&GENERIC_BOUNDS_RESOLUTION).await
}

#[tokio::test]
#[ignore]
async fn test_spec_validation_type_alias_chains() -> Result<()> {
    run_spec_validation(&TYPE_ALIAS_CHAINS).await
}

#[tokio::test]
#[ignore]
async fn test_spec_validation_nested_use_renaming() -> Result<()> {
    run_spec_validation(&NESTED_USE_RENAMING).await
}

#[tokio::test]
#[ignore]
async fn test_spec_validation_extension_traits() -> Result<()> {
    run_spec_validation(&EXTENSION_TRAITS).await
}

// =============================================================================
// Test Functions - Edge Cases
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_spec_validation_ufcs_explicit() -> Result<()> {
    run_spec_validation(&UFCS_EXPLICIT).await
}

#[tokio::test]
#[ignore]
async fn test_spec_validation_const_generics() -> Result<()> {
    run_spec_validation(&CONST_GENERICS).await
}

#[tokio::test]
#[ignore]
async fn test_spec_validation_blanket_impl() -> Result<()> {
    run_spec_validation(&BLANKET_IMPL).await
}

#[tokio::test]
#[ignore]
async fn test_spec_validation_pattern_matching() -> Result<()> {
    run_spec_validation(&PATTERN_MATCHING).await
}

#[tokio::test]
#[ignore]
async fn test_spec_validation_custom_module_paths() -> Result<()> {
    run_spec_validation(&CUSTOM_MODULE_PATHS).await
}

#[tokio::test]
#[ignore]
async fn test_spec_validation_closures() -> Result<()> {
    run_spec_validation(&CLOSURES).await
}
