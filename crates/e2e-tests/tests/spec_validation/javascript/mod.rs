//! JavaScript spec validation tests
//!
//! Run with: cargo test --manifest-path crates/e2e-tests/Cargo.toml spec_validation::javascript -- --ignored

pub mod fixtures;

use anyhow::Result;
use codesearch_e2e_tests::common::spec_validation::run_spec_validation;
use fixtures::*;

// =============================================================================
// Basic Module Tests
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_basic_module() -> Result<()> {
    run_spec_validation(&BASIC_MODULE).await
}

// =============================================================================
// Class Tests
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_classes() -> Result<()> {
    run_spec_validation(&CLASSES).await
}

// =============================================================================
// Function Tests
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_functions() -> Result<()> {
    run_spec_validation(&FUNCTIONS).await
}

// =============================================================================
// Variable Tests
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_variables() -> Result<()> {
    run_spec_validation(&VARIABLES).await
}

// =============================================================================
// Structural Validation Tests (run without --ignored)
// =============================================================================

/// Ensure all fixtures have at least one expected entity
#[tokio::test]
async fn test_all_fixtures_have_entities() {
    let fixtures = [&BASIC_MODULE, &CLASSES, &FUNCTIONS, &VARIABLES];

    for fixture in fixtures {
        assert!(
            !fixture.entities.is_empty(),
            "Fixture {} should have at least one expected entity",
            fixture.name
        );
    }
}

/// Ensure all fixtures have at least one file
#[tokio::test]
async fn test_all_fixtures_have_files() {
    let fixtures = [&BASIC_MODULE, &CLASSES, &FUNCTIONS, &VARIABLES];

    for fixture in fixtures {
        assert!(
            !fixture.files.is_empty(),
            "Fixture {} should have at least one file",
            fixture.name
        );
    }
}

/// Ensure CONTAINS relationships reference entities that exist in the fixture
#[tokio::test]
async fn test_contains_relationships_have_matching_entities() {
    let fixtures = [&BASIC_MODULE, &CLASSES, &FUNCTIONS, &VARIABLES];

    for fixture in fixtures {
        let entity_names: std::collections::HashSet<_> =
            fixture.entities.iter().map(|e| e.qualified_name).collect();

        for rel in fixture.relationships {
            if matches!(
                rel.kind,
                codesearch_e2e_tests::common::spec_validation::RelationshipKind::Contains
            ) {
                assert!(
                    entity_names.contains(rel.from),
                    "Fixture {}: CONTAINS relationship source '{}' not found in entities",
                    fixture.name,
                    rel.from
                );
                assert!(
                    entity_names.contains(rel.to),
                    "Fixture {}: CONTAINS relationship target '{}' not found in entities",
                    fixture.name,
                    rel.to
                );
            }
        }
    }
}
