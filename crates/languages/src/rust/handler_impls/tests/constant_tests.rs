//! Tests for constant and static extraction handler

use super::*;
use crate::rust::handler_impls::constant_handlers::handle_constant_impl;
use codesearch_core::entities::{EntityType, Visibility};

#[test]
fn test_simple_const() {
    let source = r#"
const X: i32 = 42;
"#;

    let entities = extract_with_handler(source, queries::CONSTANT_QUERY, handle_constant_impl)
        .expect("Failed to extract constant");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.name, "X");
    assert_eq!(entity.entity_type, EntityType::Constant);
    assert!(entity.metadata.is_const);
    assert!(!entity.metadata.is_static);
}

#[test]
fn test_const_with_type() {
    let source = r#"
const NAME: &str = "value";
"#;

    let entities = extract_with_handler(source, queries::CONSTANT_QUERY, handle_constant_impl)
        .expect("Failed to extract const with type");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.name, "NAME");
    assert_eq!(entity.entity_type, EntityType::Constant);

    // Check type attribute
    assert_eq!(
        entity.metadata.attributes.get("type").map(|s| s.as_str()),
        Some("&str")
    );
}

#[test]
fn test_static_item() {
    let source = r#"
static GLOBAL: AtomicI32 = AtomicI32::new(0);
"#;

    let entities = extract_with_handler(source, queries::CONSTANT_QUERY, handle_constant_impl)
        .expect("Failed to extract static");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.name, "GLOBAL");
    assert_eq!(entity.entity_type, EntityType::Constant);
    assert!(!entity.metadata.is_const);
    assert!(entity.metadata.is_static);
}

#[test]
fn test_static_mut() {
    let source = r#"
static mut COUNTER: i32 = 0;
"#;

    let entities = extract_with_handler(source, queries::CONSTANT_QUERY, handle_constant_impl)
        .expect("Failed to extract static mut");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.name, "COUNTER");
    assert_eq!(entity.entity_type, EntityType::Constant);
    assert!(entity.metadata.is_static);

    // Check mutable attribute
    assert_eq!(
        entity
            .metadata
            .attributes
            .get("mutable")
            .map(|s| s.as_str()),
        Some("true")
    );
}

#[test]
fn test_public_const() {
    let source = r#"
pub const MAX: usize = 100;
"#;

    let entities = extract_with_handler(source, queries::CONSTANT_QUERY, handle_constant_impl)
        .expect("Failed to extract public const");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.name, "MAX");
    assert_eq!(entity.visibility, Visibility::Public);
}

#[test]
fn test_const_with_complex_value() {
    let source = r#"
const CONFIG: Config = Config {
    host: "localhost",
    port: 8080,
};
"#;

    let entities = extract_with_handler(source, queries::CONSTANT_QUERY, handle_constant_impl)
        .expect("Failed to extract const with complex value");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.name, "CONFIG");
    assert_eq!(entity.entity_type, EntityType::Constant);

    // Value should be captured
    assert!(entity.metadata.attributes.get("value").is_some());
}

#[test]
fn test_const_function_call() {
    let source = r#"
const SIZE: usize = calculate_size();
"#;

    let entities = extract_with_handler(source, queries::CONSTANT_QUERY, handle_constant_impl)
        .expect("Failed to extract const with function call");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.name, "SIZE");
    assert_eq!(entity.entity_type, EntityType::Constant);

    // Value should include function call
    let value = entity.metadata.attributes.get("value");
    assert!(value.is_some());
    assert!(value.unwrap().contains("calculate_size"));
}

#[test]
fn test_const_with_doc_comments() {
    let source = r#"
/// Maximum number of retries
/// before giving up
pub const MAX_RETRIES: u32 = 3;
"#;

    let entities = extract_with_handler(source, queries::CONSTANT_QUERY, handle_constant_impl)
        .expect("Failed to extract const with doc comments");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.name, "MAX_RETRIES");

    assert!(entity.documentation_summary.is_some());
    let doc = entity.documentation_summary.as_ref().unwrap();
    assert!(doc.contains("Maximum number of retries"));
    assert!(doc.contains("giving up"));
}

#[test]
fn test_multiple_constants() {
    let source = r#"
const A: i32 = 1;
const B: i32 = 2;
static C: i32 = 3;
pub const D: i32 = 4;
"#;

    let entities = extract_with_handler(source, queries::CONSTANT_QUERY, handle_constant_impl)
        .expect("Failed to extract multiple constants");

    assert_eq!(entities.len(), 4);

    let names: Vec<&str> = entities.iter().map(|e| e.name.as_str()).collect();
    assert!(names.contains(&"A"));
    assert!(names.contains(&"B"));
    assert!(names.contains(&"C"));
    assert!(names.contains(&"D"));

    // Check const vs static
    let const_count = entities.iter().filter(|e| e.metadata.is_const).count();
    assert_eq!(const_count, 3); // A, B, D

    let static_count = entities.iter().filter(|e| e.metadata.is_static).count();
    assert_eq!(static_count, 1); // C
}

#[test]
fn test_const_with_type_annotation() {
    let source = r#"
pub const PI: f64 = 3.14159;
"#;

    let entities = extract_with_handler(source, queries::CONSTANT_QUERY, handle_constant_impl)
        .expect("Failed to extract const with type annotation");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.name, "PI");

    // Check that both type and value are captured
    assert_eq!(
        entity.metadata.attributes.get("type").map(|s| s.as_str()),
        Some("f64")
    );
    assert!(entity.metadata.attributes.get("value").is_some());
}
