//! TypeScript spec validation tests
//!
//! Run with: cargo test --manifest-path crates/e2e-tests/Cargo.toml spec_validation::typescript -- --ignored

pub mod fixtures;

use anyhow::Result;
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
async fn test_ambient_modules() -> Result<()> {
    run_spec_validation(&AMBIENT_MODULES).await
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
