//! Tests for type alias extraction handler

use super::*;
use crate::rust::handler_impls::type_alias_handlers::handle_type_alias_impl;
use codesearch_core::entities::{EntityType, Visibility};

#[test]
fn test_simple_type_alias() {
    let source = r#"
type Result<T> = std::result::Result<T, Error>;
"#;

    let entities = extract_with_handler(source, queries::TYPE_ALIAS_QUERY, handle_type_alias_impl)
        .expect("Failed to extract type alias");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.name, "Result");
    assert_eq!(entity.entity_type, EntityType::TypeAlias);

    // Check it's generic
    assert!(entity.metadata.is_generic);
    assert_eq!(entity.metadata.generic_params.len(), 1);
    assert!(entity.metadata.generic_params.contains(&"T".to_string()));

    // Check aliased type is captured
    let aliased_type = entity
        .metadata
        .attributes
        .get("aliased_type")
        .expect("Should have aliased_type attribute");
    assert!(aliased_type.contains("std::result::Result"));
}

#[test]
fn test_type_alias_no_generics() {
    let source = r#"
type Callback = Box<dyn Fn()>;
"#;

    let entities = extract_with_handler(source, queries::TYPE_ALIAS_QUERY, handle_type_alias_impl)
        .expect("Failed to extract type alias");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.name, "Callback");
    assert_eq!(entity.entity_type, EntityType::TypeAlias);

    // Not generic
    assert!(!entity.metadata.is_generic);
    assert_eq!(entity.metadata.generic_params.len(), 0);

    // Check aliased type
    let aliased_type = entity
        .metadata
        .attributes
        .get("aliased_type")
        .expect("Should have aliased_type attribute");
    assert!(aliased_type.contains("Box"));
    assert!(aliased_type.contains("Fn()"));
}

#[test]
fn test_complex_type_alias() {
    let source = r#"
type Handler = Arc<Mutex<Box<dyn Fn() + Send>>>;
"#;

    let entities = extract_with_handler(source, queries::TYPE_ALIAS_QUERY, handle_type_alias_impl)
        .expect("Failed to extract type alias");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.name, "Handler");
    assert_eq!(entity.entity_type, EntityType::TypeAlias);

    // Check aliased type captures nested types
    let aliased_type = entity
        .metadata
        .attributes
        .get("aliased_type")
        .expect("Should have aliased_type attribute");
    assert!(aliased_type.contains("Arc"));
    assert!(aliased_type.contains("Mutex"));
    assert!(aliased_type.contains("Box"));
}

#[test]
fn test_generic_type_alias() {
    let source = r#"
type Pair<T, U> = (T, U);
"#;

    let entities = extract_with_handler(source, queries::TYPE_ALIAS_QUERY, handle_type_alias_impl)
        .expect("Failed to extract type alias");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.name, "Pair");
    assert_eq!(entity.entity_type, EntityType::TypeAlias);

    // Check generics
    assert!(entity.metadata.is_generic);
    assert_eq!(entity.metadata.generic_params.len(), 2);
    assert!(entity.metadata.generic_params.contains(&"T".to_string()));
    assert!(entity.metadata.generic_params.contains(&"U".to_string()));

    // Check aliased type
    let aliased_type = entity
        .metadata
        .attributes
        .get("aliased_type")
        .expect("Should have aliased_type attribute");
    assert!(aliased_type.contains("(T, U)") || aliased_type.contains("( T , U )"));
}

#[test]
fn test_public_type_alias() {
    let source = r#"
pub type PublicAlias = String;
"#;

    let entities = extract_with_handler(source, queries::TYPE_ALIAS_QUERY, handle_type_alias_impl)
        .expect("Failed to extract type alias");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.name, "PublicAlias");
    assert_eq!(entity.entity_type, EntityType::TypeAlias);
    assert_eq!(entity.visibility, Some(Visibility::Public));

    // Check aliased type
    let aliased_type = entity
        .metadata
        .attributes
        .get("aliased_type")
        .expect("Should have aliased_type attribute");
    assert!(aliased_type.contains("String"));
}

#[test]
fn test_type_alias_with_bounds() {
    let source = r#"
type Bounded<T: Clone> = Vec<T>;
"#;

    let entities = extract_with_handler(source, queries::TYPE_ALIAS_QUERY, handle_type_alias_impl)
        .expect("Failed to extract type alias");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.name, "Bounded");
    assert_eq!(entity.entity_type, EntityType::TypeAlias);

    // Check generics with bounds
    assert!(entity.metadata.is_generic);
    // The generic params may include bounds like "T: Clone" or just "T"
    // Different versions of tree-sitter or extract_generics_from_node may handle this differently
    assert!(!entity.metadata.generic_params.is_empty());

    // Check that at least one param contains T
    let has_t = entity
        .metadata
        .generic_params
        .iter()
        .any(|p| p.contains('T'));
    assert!(
        has_t,
        "Generic params should contain T, got: {:?}",
        entity.metadata.generic_params
    );

    // Check aliased type
    let aliased_type = entity
        .metadata
        .attributes
        .get("aliased_type")
        .expect("Should have aliased_type attribute");
    assert!(aliased_type.contains("Vec"));
}

#[test]
fn test_type_alias_with_doc_comments() {
    let source = r#"
/// Standard result type for this module
///
/// This type simplifies error handling
pub type Result<T> = std::result::Result<T, Error>;
"#;

    let entities = extract_with_handler(source, queries::TYPE_ALIAS_QUERY, handle_type_alias_impl)
        .expect("Failed to extract type alias");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.name, "Result");
    assert_eq!(entity.entity_type, EntityType::TypeAlias);

    // Check documentation
    assert!(entity.documentation_summary.is_some());
    let doc = entity.documentation_summary.as_ref().unwrap();
    assert!(doc.contains("Standard result type"));
}

#[test]
fn test_nested_generic_alias() {
    let source = r#"
type Complex<T> = HashMap<String, Vec<T>>;
"#;

    let entities = extract_with_handler(source, queries::TYPE_ALIAS_QUERY, handle_type_alias_impl)
        .expect("Failed to extract type alias");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.name, "Complex");
    assert_eq!(entity.entity_type, EntityType::TypeAlias);

    // Check generics
    assert!(entity.metadata.is_generic);
    assert_eq!(entity.metadata.generic_params.len(), 1);
    assert!(entity.metadata.generic_params.contains(&"T".to_string()));

    // Check aliased type captures nested structure
    let aliased_type = entity
        .metadata
        .attributes
        .get("aliased_type")
        .expect("Should have aliased_type attribute");
    assert!(aliased_type.contains("HashMap"));
    assert!(aliased_type.contains("String"));
    assert!(aliased_type.contains("Vec"));
}

#[test]
fn test_multiple_type_aliases() {
    let source = r#"
type First = i32;
type Second<T> = Option<T>;
pub type Third = String;
"#;

    let entities = extract_with_handler(source, queries::TYPE_ALIAS_QUERY, handle_type_alias_impl)
        .expect("Failed to extract type aliases");

    assert_eq!(entities.len(), 3);

    let names: Vec<&str> = entities.iter().map(|e| e.name.as_str()).collect();
    assert!(names.contains(&"First"));
    assert!(names.contains(&"Second"));
    assert!(names.contains(&"Third"));

    // Check that all are type aliases
    for entity in &entities {
        assert_eq!(entity.entity_type, EntityType::TypeAlias);
    }
}

#[test]
fn test_type_alias_with_lifetime() {
    let source = r#"
type StringRef<'a> = &'a str;
"#;

    let entities = extract_with_handler(source, queries::TYPE_ALIAS_QUERY, handle_type_alias_impl)
        .expect("Failed to extract type alias");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.name, "StringRef");
    assert_eq!(entity.entity_type, EntityType::TypeAlias);

    // Should be marked as generic (lifetimes are generic params)
    assert!(entity.metadata.is_generic);
    assert_eq!(entity.metadata.generic_params.len(), 1);
}
