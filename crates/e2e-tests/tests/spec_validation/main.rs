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

// =============================================================================
// Fixture Consistency Tests (no Docker required)
// =============================================================================
// These tests validate that fixture definitions are internally consistent.

use codesearch_e2e_tests::common::spec_validation::{EntityKind, Fixture, RelationshipKind};

/// All fixtures for validation
const ALL_FIXTURES: &[&Fixture] = &[
    &BASIC_MOD,
    &VISIBILITY,
    &USE_IMPORTS,
    &REEXPORTS,
    &FREE_FUNCTIONS,
    &METHODS,
    &CROSS_MODULE_CALLS,
    &STRUCTS,
    &ENUMS,
    &TYPE_ALIASES,
    &TRAIT_DEF,
    &TRAIT_IMPL,
    &SUPERTRAITS,
    &CONSTANTS,
    &STATICS,
    &UNIONS,
    &EXTERN_BLOCKS,
    &ASSOCIATED_CONSTANTS,
    &MACRO_RULES,
    &DEEP_MODULE_NESTING,
    &MIXED_MODULE_STRUCTURE,
    &SELF_SUPER_REFERENCES,
    &MULTIPLE_IMPL_BLOCKS,
    &ASYNC_FUNCTIONS,
    &BUILDER_PATTERN,
    &RECURSIVE_CALLS,
    &TUPLE_AND_UNIT_STRUCTS,
    &COMPLEX_ENUMS,
    &GENERIC_STRUCTS,
    &LIFETIMES,
    &ASSOCIATED_TYPES,
    &MULTIPLE_TRAIT_IMPLS,
    &GENERIC_TRAIT,
    &WORKSPACE_BASIC,
    &MULTI_HOP_REEXPORTS,
    &GLOB_REEXPORTS,
    &TRAIT_VS_INHERENT_METHOD,
    &SCATTERED_IMPL_BLOCKS,
    &ASSOCIATED_TYPES_RESOLUTION,
    &PRELUDE_SHADOWING,
    &GENERIC_BOUNDS_RESOLUTION,
    &TYPE_ALIAS_CHAINS,
    &NESTED_USE_RENAMING,
    &EXTENSION_TRAITS,
    &UFCS_EXPLICIT,
    &CONST_GENERICS,
    &BLANKET_IMPL,
    &PATTERN_MATCHING,
    &CUSTOM_MODULE_PATHS,
    &CLOSURES,
];

#[test]
fn test_all_fixtures_have_entities() {
    for fixture in ALL_FIXTURES {
        assert!(
            !fixture.entities.is_empty(),
            "Fixture '{}' should have at least one entity",
            fixture.name
        );
    }
}

#[test]
fn test_all_fixtures_have_files() {
    for fixture in ALL_FIXTURES {
        assert!(
            !fixture.files.is_empty(),
            "Fixture '{}' should have at least one file",
            fixture.name
        );
    }
}

#[test]
fn test_property_entities_have_struct_parent() {
    for fixture in ALL_FIXTURES {
        let has_property = fixture
            .entities
            .iter()
            .any(|e| e.kind == EntityKind::Property);

        if has_property {
            let has_struct_or_union = fixture
                .entities
                .iter()
                .any(|e| e.kind == EntityKind::Struct || e.kind == EntityKind::Union);

            assert!(
                has_struct_or_union,
                "Fixture '{}' has Property entity but no Struct or Union parent",
                fixture.name
            );
        }
    }
}

#[test]
fn test_enum_variant_entities_have_enum_parent() {
    for fixture in ALL_FIXTURES {
        let has_variant = fixture
            .entities
            .iter()
            .any(|e| e.kind == EntityKind::EnumVariant);

        if has_variant {
            let has_enum = fixture.entities.iter().any(|e| e.kind == EntityKind::Enum);

            assert!(
                has_enum,
                "Fixture '{}' has EnumVariant entity but no Enum parent",
                fixture.name
            );
        }
    }
}

#[test]
fn test_contains_relationships_have_matching_entities() {
    for fixture in ALL_FIXTURES {
        for rel in fixture.relationships {
            if rel.kind == RelationshipKind::Contains {
                let from_exists = fixture
                    .entities
                    .iter()
                    .any(|e| e.qualified_name == rel.from);
                let to_exists = fixture.entities.iter().any(|e| e.qualified_name == rel.to);

                assert!(
                    from_exists,
                    "Fixture '{}': CONTAINS from '{}' not found in entities",
                    fixture.name, rel.from
                );
                assert!(
                    to_exists,
                    "Fixture '{}': CONTAINS to '{}' not found in entities",
                    fixture.name, rel.to
                );

                // Skip prefix check for impl blocks and extern blocks - they use special naming
                // that doesn't follow the parent::child pattern
                let from_is_special = fixture.entities.iter().any(|e| {
                    e.qualified_name == rel.from
                        && (e.kind == EntityKind::ImplBlock || e.kind == EntityKind::ExternBlock)
                });

                if !from_is_special {
                    assert!(
                        rel.to.starts_with(rel.from),
                        "Fixture '{}': CONTAINS child '{}' should be prefixed by parent '{}'",
                        fixture.name,
                        rel.to,
                        rel.from
                    );
                }
            }
        }
    }
}

#[test]
fn test_uses_relationships_source_exists() {
    for fixture in ALL_FIXTURES {
        for rel in fixture.relationships {
            if rel.kind == RelationshipKind::Uses {
                let from_exists = fixture
                    .entities
                    .iter()
                    .any(|e| e.qualified_name == rel.from);

                assert!(
                    from_exists,
                    "Fixture '{}': USES source '{}' not found in entities",
                    fixture.name, rel.from
                );
            }
        }
    }
}

#[test]
fn test_structs_fixture_has_property_entities() {
    let structs_fixture = ALL_FIXTURES
        .iter()
        .find(|f| f.name == "structs")
        .expect("Should have structs fixture");

    let property_count = structs_fixture
        .entities
        .iter()
        .filter(|e| e.kind == EntityKind::Property)
        .count();

    assert!(
        property_count >= 2,
        "structs fixture should have at least 2 Property entities, found {property_count}"
    );

    let contains_property_count = structs_fixture
        .relationships
        .iter()
        .filter(|r| {
            r.kind == RelationshipKind::Contains
                && structs_fixture
                    .entities
                    .iter()
                    .any(|e| e.qualified_name == r.to && e.kind == EntityKind::Property)
        })
        .count();

    assert!(
        contains_property_count >= 2,
        "structs fixture should have at least 2 CONTAINS->Property relationships, found {contains_property_count}"
    );
}

#[test]
fn test_enums_fixture_has_enum_variant_entities() {
    let enums_fixture = ALL_FIXTURES
        .iter()
        .find(|f| f.name == "enums")
        .expect("Should have enums fixture");

    let variant_count = enums_fixture
        .entities
        .iter()
        .filter(|e| e.kind == EntityKind::EnumVariant)
        .count();

    assert!(
        variant_count >= 3,
        "enums fixture should have at least 3 EnumVariant entities, found {variant_count}"
    );

    let contains_variant_count = enums_fixture
        .relationships
        .iter()
        .filter(|r| {
            r.kind == RelationshipKind::Contains
                && enums_fixture
                    .entities
                    .iter()
                    .any(|e| e.qualified_name == r.to && e.kind == EntityKind::EnumVariant)
        })
        .count();

    assert!(
        contains_variant_count >= 3,
        "enums fixture should have at least 3 CONTAINS->EnumVariant relationships, found {contains_variant_count}"
    );
}

#[test]
fn test_complex_enums_fixture_has_uses_relationships() {
    let complex_enums_fixture = ALL_FIXTURES
        .iter()
        .find(|f| f.name == "complex_enums")
        .expect("Should have complex_enums fixture");

    let has_variant_uses = complex_enums_fixture.relationships.iter().any(|r| {
        r.kind == RelationshipKind::Uses
            && complex_enums_fixture
                .entities
                .iter()
                .any(|e| e.qualified_name == r.from && e.kind == EntityKind::EnumVariant)
    });

    assert!(
        has_variant_uses,
        "complex_enums fixture should have USES relationships from EnumVariant entities"
    );
}

#[test]
fn test_structs_fixture_has_property_uses_relationships() {
    let structs_fixture = ALL_FIXTURES
        .iter()
        .find(|f| f.name == "structs")
        .expect("Should have structs fixture");

    let has_property_uses = structs_fixture.relationships.iter().any(|r| {
        r.kind == RelationshipKind::Uses
            && structs_fixture
                .entities
                .iter()
                .any(|e| e.qualified_name == r.from && e.kind == EntityKind::Property)
    });

    assert!(
        has_property_uses,
        "structs fixture should have USES relationships from Property entities"
    );
}
