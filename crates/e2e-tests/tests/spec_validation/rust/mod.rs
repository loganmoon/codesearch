//! Rust spec validation tests
//!
//! Run with: cargo test --manifest-path crates/e2e-tests/Cargo.toml spec_validation::rust -- --ignored

pub mod fixtures;

use anyhow::Result;
use codesearch_e2e_tests::common::spec_validation::run_spec_validation;
use fixtures::*;

// =============================================================================
// Basic Tests
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_basic_mod() -> Result<()> {
    run_spec_validation(&BASIC_MOD).await
}

#[tokio::test]
#[ignore]
async fn test_visibility() -> Result<()> {
    run_spec_validation(&VISIBILITY).await
}

#[tokio::test]
#[ignore]
async fn test_use_imports() -> Result<()> {
    run_spec_validation(&USE_IMPORTS).await
}

#[tokio::test]
#[ignore]
async fn test_reexports() -> Result<()> {
    run_spec_validation(&REEXPORTS).await
}

#[tokio::test]
#[ignore]
async fn test_free_functions() -> Result<()> {
    run_spec_validation(&FREE_FUNCTIONS).await
}

#[tokio::test]
#[ignore]
async fn test_methods() -> Result<()> {
    run_spec_validation(&METHODS).await
}

#[tokio::test]
#[ignore]
async fn test_cross_module_calls() -> Result<()> {
    run_spec_validation(&CROSS_MODULE_CALLS).await
}

#[tokio::test]
#[ignore]
async fn test_structs() -> Result<()> {
    run_spec_validation(&STRUCTS).await
}

#[tokio::test]
#[ignore]
async fn test_enums() -> Result<()> {
    run_spec_validation(&ENUMS).await
}

#[tokio::test]
#[ignore]
async fn test_type_aliases() -> Result<()> {
    run_spec_validation(&TYPE_ALIASES).await
}

#[tokio::test]
#[ignore]
async fn test_trait_def() -> Result<()> {
    run_spec_validation(&TRAIT_DEF).await
}

#[tokio::test]
#[ignore]
async fn test_trait_impl() -> Result<()> {
    run_spec_validation(&TRAIT_IMPL).await
}

#[tokio::test]
#[ignore]
async fn test_supertraits() -> Result<()> {
    run_spec_validation(&SUPERTRAITS).await
}

#[tokio::test]
#[ignore]
async fn test_constants() -> Result<()> {
    run_spec_validation(&CONSTANTS).await
}

#[tokio::test]
#[ignore]
async fn test_statics() -> Result<()> {
    run_spec_validation(&STATICS).await
}

#[tokio::test]
#[ignore]
async fn test_unions() -> Result<()> {
    run_spec_validation(&UNIONS).await
}

#[tokio::test]
#[ignore]
async fn test_extern_blocks() -> Result<()> {
    run_spec_validation(&EXTERN_BLOCKS).await
}

#[tokio::test]
#[ignore]
async fn test_associated_constants() -> Result<()> {
    run_spec_validation(&ASSOCIATED_CONSTANTS).await
}

#[tokio::test]
#[ignore]
async fn test_macro_rules() -> Result<()> {
    run_spec_validation(&MACRO_RULES).await
}

// =============================================================================
// Advanced Module System
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_deep_module_nesting() -> Result<()> {
    run_spec_validation(&DEEP_MODULE_NESTING).await
}

#[tokio::test]
#[ignore]
async fn test_mixed_module_structure() -> Result<()> {
    run_spec_validation(&MIXED_MODULE_STRUCTURE).await
}

#[tokio::test]
#[ignore]
async fn test_self_super_references() -> Result<()> {
    run_spec_validation(&SELF_SUPER_REFERENCES).await
}

// =============================================================================
// Advanced Functions & Calls
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_multiple_impl_blocks() -> Result<()> {
    run_spec_validation(&MULTIPLE_IMPL_BLOCKS).await
}

#[tokio::test]
#[ignore]
async fn test_async_functions() -> Result<()> {
    run_spec_validation(&ASYNC_FUNCTIONS).await
}

#[tokio::test]
#[ignore]
async fn test_builder_pattern() -> Result<()> {
    run_spec_validation(&BUILDER_PATTERN).await
}

#[tokio::test]
#[ignore]
async fn test_recursive_calls() -> Result<()> {
    run_spec_validation(&RECURSIVE_CALLS).await
}

// =============================================================================
// Advanced Types
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_tuple_and_unit_structs() -> Result<()> {
    run_spec_validation(&TUPLE_AND_UNIT_STRUCTS).await
}

#[tokio::test]
#[ignore]
async fn test_complex_enums() -> Result<()> {
    run_spec_validation(&COMPLEX_ENUMS).await
}

#[tokio::test]
#[ignore]
async fn test_generic_structs() -> Result<()> {
    run_spec_validation(&GENERIC_STRUCTS).await
}

#[tokio::test]
#[ignore]
async fn test_lifetimes() -> Result<()> {
    run_spec_validation(&LIFETIMES).await
}

// =============================================================================
// Advanced Traits
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_associated_types() -> Result<()> {
    run_spec_validation(&ASSOCIATED_TYPES).await
}

#[tokio::test]
#[ignore]
async fn test_multiple_trait_impls() -> Result<()> {
    run_spec_validation(&MULTIPLE_TRAIT_IMPLS).await
}

#[tokio::test]
#[ignore]
async fn test_generic_trait() -> Result<()> {
    run_spec_validation(&GENERIC_TRAIT).await
}

// =============================================================================
// Workspace
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_workspace_basic() -> Result<()> {
    run_spec_validation(&WORKSPACE_BASIC).await
}

// =============================================================================
// Hard but Feasible Resolution
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_multi_hop_reexports() -> Result<()> {
    run_spec_validation(&MULTI_HOP_REEXPORTS).await
}

#[tokio::test]
#[ignore]
async fn test_glob_reexports() -> Result<()> {
    run_spec_validation(&GLOB_REEXPORTS).await
}

#[tokio::test]
#[ignore]
async fn test_trait_vs_inherent_method() -> Result<()> {
    run_spec_validation(&TRAIT_VS_INHERENT_METHOD).await
}

#[tokio::test]
#[ignore]
async fn test_scattered_impl_blocks() -> Result<()> {
    run_spec_validation(&SCATTERED_IMPL_BLOCKS).await
}

#[tokio::test]
#[ignore]
async fn test_associated_types_resolution() -> Result<()> {
    run_spec_validation(&ASSOCIATED_TYPES_RESOLUTION).await
}

#[tokio::test]
#[ignore]
async fn test_prelude_shadowing() -> Result<()> {
    run_spec_validation(&PRELUDE_SHADOWING).await
}

#[tokio::test]
#[ignore]
async fn test_generic_bounds_resolution() -> Result<()> {
    run_spec_validation(&GENERIC_BOUNDS_RESOLUTION).await
}

#[tokio::test]
#[ignore]
async fn test_type_alias_chains() -> Result<()> {
    run_spec_validation(&TYPE_ALIAS_CHAINS).await
}

#[tokio::test]
#[ignore]
async fn test_nested_use_renaming() -> Result<()> {
    run_spec_validation(&NESTED_USE_RENAMING).await
}

#[tokio::test]
#[ignore]
async fn test_extension_traits() -> Result<()> {
    run_spec_validation(&EXTENSION_TRAITS).await
}

// =============================================================================
// Edge Cases
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_ufcs_explicit() -> Result<()> {
    run_spec_validation(&UFCS_EXPLICIT).await
}

#[tokio::test]
#[ignore]
async fn test_const_generics() -> Result<()> {
    run_spec_validation(&CONST_GENERICS).await
}

#[tokio::test]
#[ignore]
async fn test_blanket_impl() -> Result<()> {
    run_spec_validation(&BLANKET_IMPL).await
}

#[tokio::test]
#[ignore]
async fn test_pattern_matching() -> Result<()> {
    run_spec_validation(&PATTERN_MATCHING).await
}

#[tokio::test]
#[ignore]
async fn test_custom_module_paths() -> Result<()> {
    run_spec_validation(&CUSTOM_MODULE_PATHS).await
}

#[tokio::test]
#[ignore]
async fn test_closures() -> Result<()> {
    run_spec_validation(&CLOSURES).await
}
