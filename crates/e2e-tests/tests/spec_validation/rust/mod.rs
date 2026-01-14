//! Rust spec validation tests
//!
//! Run with: cargo test --manifest-path crates/e2e-tests/Cargo.toml spec_validation::rust -- --ignored

pub mod fixtures;

use anyhow::Result;
use codesearch_core::QualifiedName;
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

                // Use structured QualifiedName for proper containment checking
                // This handles impl blocks, trait impls, and other special cases
                let parent_qn = QualifiedName::parse(rel.from)
                    .expect(&format!("Failed to parse parent qualified name: {}", rel.from));
                let child_qn = QualifiedName::parse(rel.to)
                    .expect(&format!("Failed to parse child qualified name: {}", rel.to));

                assert!(
                    child_qn.is_child_of(&parent_qn),
                    "Fixture '{}': CONTAINS child '{}' should be contained by parent '{}'",
                    fixture.name,
                    rel.to,
                    rel.from
                );
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
