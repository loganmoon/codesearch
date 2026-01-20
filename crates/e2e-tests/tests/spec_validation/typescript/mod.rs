//! TypeScript spec validation tests
//!
//! Run with: cargo test --manifest-path crates/e2e-tests/Cargo.toml spec_validation::typescript -- --ignored

pub mod fixtures;

use anyhow::Result;
use codesearch_core::QualifiedName;
use codesearch_e2e_tests::common::spec_validation::run_spec_validation;
use fixtures::*;

// =============================================================================
// Modules
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_basic_module() -> Result<()> {
    run_spec_validation(&BASIC_MODULE).await
}

#[tokio::test]
#[ignore]
async fn test_imports_exports() -> Result<()> {
    run_spec_validation(&IMPORTS_EXPORTS).await
}

#[tokio::test]
#[ignore]
async fn test_namespaces() -> Result<()> {
    run_spec_validation(&NAMESPACES).await
}

#[tokio::test]
#[ignore]
async fn test_nested_namespaces() -> Result<()> {
    run_spec_validation(&NESTED_NAMESPACES).await
}

#[tokio::test]
#[ignore]
async fn test_namespace_merging() -> Result<()> {
    run_spec_validation(&NAMESPACE_MERGING).await
}

#[tokio::test]
#[ignore]
async fn test_reexports() -> Result<()> {
    run_spec_validation(&REEXPORTS).await
}

#[tokio::test]
#[ignore]
async fn test_barrel_exports() -> Result<()> {
    run_spec_validation(&BARREL_EXPORTS).await
}

#[tokio::test]
#[ignore]
async fn test_default_exports() -> Result<()> {
    run_spec_validation(&DEFAULT_EXPORTS).await
}

// =============================================================================
// Classes
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_classes() -> Result<()> {
    run_spec_validation(&CLASSES).await
}

#[tokio::test]
#[ignore]
async fn test_abstract_classes() -> Result<()> {
    run_spec_validation(&ABSTRACT_CLASSES).await
}

#[tokio::test]
#[ignore]
async fn test_class_expressions() -> Result<()> {
    run_spec_validation(&CLASS_EXPRESSIONS).await
}

#[tokio::test]
#[ignore]
async fn test_class_inheritance() -> Result<()> {
    run_spec_validation(&CLASS_INHERITANCE).await
}

#[tokio::test]
#[ignore]
async fn test_class_implements() -> Result<()> {
    run_spec_validation(&CLASS_IMPLEMENTS).await
}

#[tokio::test]
#[ignore]
async fn test_class_fields() -> Result<()> {
    run_spec_validation(&CLASS_FIELDS).await
}

#[tokio::test]
#[ignore]
async fn test_parameter_properties() -> Result<()> {
    run_spec_validation(&PARAMETER_PROPERTIES).await
}

#[tokio::test]
#[ignore]
async fn test_private_fields() -> Result<()> {
    run_spec_validation(&PRIVATE_FIELDS).await
}

#[tokio::test]
#[ignore]
async fn test_static_members() -> Result<()> {
    run_spec_validation(&STATIC_MEMBERS).await
}

#[tokio::test]
#[ignore]
async fn test_accessors() -> Result<()> {
    run_spec_validation(&ACCESSORS).await
}

// =============================================================================
// Interfaces
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_interfaces() -> Result<()> {
    run_spec_validation(&INTERFACES).await
}

#[tokio::test]
#[ignore]
async fn test_interface_extends() -> Result<()> {
    run_spec_validation(&INTERFACE_EXTENDS).await
}

#[tokio::test]
#[ignore]
async fn test_interface_merging() -> Result<()> {
    run_spec_validation(&INTERFACE_MERGING).await
}

#[tokio::test]
#[ignore]
async fn test_index_signatures() -> Result<()> {
    run_spec_validation(&INDEX_SIGNATURES).await
}

#[tokio::test]
#[ignore]
async fn test_call_construct_signatures() -> Result<()> {
    run_spec_validation(&CALL_CONSTRUCT_SIGNATURES).await
}

// =============================================================================
// Functions
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_functions() -> Result<()> {
    run_spec_validation(&FUNCTIONS).await
}

#[tokio::test]
#[ignore]
async fn test_function_expressions() -> Result<()> {
    run_spec_validation(&FUNCTION_EXPRESSIONS).await
}

#[tokio::test]
#[ignore]
async fn test_arrow_functions() -> Result<()> {
    run_spec_validation(&ARROW_FUNCTIONS).await
}

#[tokio::test]
#[ignore]
async fn test_async_functions() -> Result<()> {
    run_spec_validation(&ASYNC_FUNCTIONS).await
}

#[tokio::test]
#[ignore]
async fn test_generator_functions() -> Result<()> {
    run_spec_validation(&GENERATOR_FUNCTIONS).await
}

#[tokio::test]
#[ignore]
async fn test_overloaded_functions() -> Result<()> {
    run_spec_validation(&OVERLOADED_FUNCTIONS).await
}

#[tokio::test]
#[ignore]
async fn test_methods() -> Result<()> {
    run_spec_validation(&METHODS).await
}

#[tokio::test]
#[ignore]
async fn test_function_calls() -> Result<()> {
    run_spec_validation(&FUNCTION_CALLS).await
}

// =============================================================================
// Types
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_type_aliases() -> Result<()> {
    run_spec_validation(&TYPE_ALIASES).await
}

#[tokio::test]
#[ignore]
async fn test_generic_type_aliases() -> Result<()> {
    run_spec_validation(&GENERIC_TYPE_ALIASES).await
}

#[tokio::test]
#[ignore]
async fn test_enums() -> Result<()> {
    run_spec_validation(&ENUMS).await
}

#[tokio::test]
#[ignore]
async fn test_string_enums() -> Result<()> {
    run_spec_validation(&STRING_ENUMS).await
}

#[tokio::test]
#[ignore]
async fn test_const_enums() -> Result<()> {
    run_spec_validation(&CONST_ENUMS).await
}

#[tokio::test]
#[ignore]
async fn test_constants_variables() -> Result<()> {
    run_spec_validation(&CONSTANTS_VARIABLES).await
}

// =============================================================================
// Advanced
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_generics() -> Result<()> {
    run_spec_validation(&GENERICS).await
}

#[tokio::test]
#[ignore]
async fn test_decorators() -> Result<()> {
    run_spec_validation(&DECORATORS).await
}

#[tokio::test]
#[ignore]
async fn test_ambient_declarations() -> Result<()> {
    run_spec_validation(&AMBIENT_DECLARATIONS).await
}

#[tokio::test]
#[ignore]
async fn test_global_augmentation() -> Result<()> {
    run_spec_validation(&GLOBAL_AUGMENTATION).await
}

#[tokio::test]
#[ignore]
async fn test_type_usage() -> Result<()> {
    run_spec_validation(&TYPE_USAGE).await
}

#[tokio::test]
#[ignore]
async fn test_visibility() -> Result<()> {
    run_spec_validation(&VISIBILITY).await
}

#[tokio::test]
#[ignore]
async fn test_jsx_components() -> Result<()> {
    run_spec_validation(&JSX_COMPONENTS).await
}

#[tokio::test]
#[ignore]
async fn test_arrow_field_properties() -> Result<()> {
    run_spec_validation(&ARROW_FIELD_PROPERTIES).await
}

#[tokio::test]
#[ignore]
async fn test_optional_readonly() -> Result<()> {
    run_spec_validation(&OPTIONAL_READONLY).await
}

// =============================================================================
// Fixture Consistency Tests (no Docker required)
// =============================================================================
// These tests validate that fixture definitions are internally consistent.

use codesearch_e2e_tests::common::spec_validation::{EntityKind, Fixture, RelationshipKind};

/// All TypeScript fixtures for validation
const ALL_FIXTURES: &[&Fixture] = &[
    // Modules
    &BASIC_MODULE,
    &IMPORTS_EXPORTS,
    &NAMESPACES,
    &NESTED_NAMESPACES,
    &NAMESPACE_MERGING,
    &REEXPORTS,
    &BARREL_EXPORTS,
    &DEFAULT_EXPORTS,
    // Classes
    &CLASSES,
    &ABSTRACT_CLASSES,
    &CLASS_EXPRESSIONS,
    &CLASS_INHERITANCE,
    &CLASS_IMPLEMENTS,
    &CLASS_FIELDS,
    &PARAMETER_PROPERTIES,
    &PRIVATE_FIELDS,
    &STATIC_MEMBERS,
    &ACCESSORS,
    // Interfaces
    &INTERFACES,
    &INTERFACE_EXTENDS,
    &INTERFACE_MERGING,
    &INDEX_SIGNATURES,
    &CALL_CONSTRUCT_SIGNATURES,
    // Functions
    &FUNCTIONS,
    &FUNCTION_EXPRESSIONS,
    &ARROW_FUNCTIONS,
    &ASYNC_FUNCTIONS,
    &GENERATOR_FUNCTIONS,
    &OVERLOADED_FUNCTIONS,
    &METHODS,
    &FUNCTION_CALLS,
    // Types
    &TYPE_ALIASES,
    &GENERIC_TYPE_ALIASES,
    &ENUMS,
    &STRING_ENUMS,
    &CONST_ENUMS,
    &CONSTANTS_VARIABLES,
    // Advanced
    &GENERICS,
    &DECORATORS,
    &AMBIENT_DECLARATIONS,
    &GLOBAL_AUGMENTATION,
    &TYPE_USAGE,
    &VISIBILITY,
    &JSX_COMPONENTS,
    &ARROW_FIELD_PROPERTIES,
    &OPTIONAL_READONLY,
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
fn test_property_entities_have_class_parent() {
    for fixture in ALL_FIXTURES {
        let has_property = fixture
            .entities
            .iter()
            .any(|e| e.kind == EntityKind::Property);

        if has_property {
            let has_class = fixture
                .entities
                .iter()
                .any(|e| e.kind == EntityKind::Class || e.kind == EntityKind::Interface);

            assert!(
                has_class,
                "Fixture '{}' has Property entity but no Class or Interface parent",
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
                let parent_qn = QualifiedName::parse(rel.from).expect(&format!(
                    "Failed to parse parent qualified name: {}",
                    rel.from
                ));
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
fn test_class_implements_fixture_has_implements_relationships() {
    let fixture = ALL_FIXTURES
        .iter()
        .find(|f| f.name == "ts_class_implements")
        .expect("Should have ts_class_implements fixture");

    let implements_count = fixture
        .relationships
        .iter()
        .filter(|r| r.kind == RelationshipKind::Implements)
        .count();

    assert!(
        implements_count >= 1,
        "ts_class_implements fixture should have at least 1 IMPLEMENTS relationship, found {implements_count}"
    );
}

#[test]
fn test_class_inheritance_fixture_has_inherits_from_relationships() {
    let fixture = ALL_FIXTURES
        .iter()
        .find(|f| f.name == "ts_class_inheritance")
        .expect("Should have ts_class_inheritance fixture");

    let inherits_count = fixture
        .relationships
        .iter()
        .filter(|r| r.kind == RelationshipKind::InheritsFrom)
        .count();

    assert!(
        inherits_count >= 1,
        "ts_class_inheritance fixture should have at least 1 INHERITS_FROM relationship, found {inherits_count}"
    );
}

#[test]
fn test_interface_extends_fixture_has_extends_interface_relationships() {
    let fixture = ALL_FIXTURES
        .iter()
        .find(|f| f.name == "ts_interface_extends")
        .expect("Should have ts_interface_extends fixture");

    let extends_count = fixture
        .relationships
        .iter()
        .filter(|r| r.kind == RelationshipKind::ExtendsInterface)
        .count();

    assert!(
        extends_count >= 1,
        "ts_interface_extends fixture should have at least 1 EXTENDS_INTERFACE relationship, found {extends_count}"
    );
}
