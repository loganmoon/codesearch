//! Tests for enum extraction handler

use super::*;
use crate::rust::handler_impls::type_handlers::handle_enum_impl;
use codesearch_core::entities::EntityType;

#[test]
fn test_simple_enum() {
    let source = r#"
enum SimpleEnum {
    First,
    Second,
    Third,
}
"#;

    let entities = extract_with_handler(source, queries::ENUM_QUERY, handle_enum_impl)
        .expect("Failed to extract enum");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.name, "SimpleEnum");
    assert_eq!(entity.entity_type, EntityType::Enum);

    // Check enum variants are captured in metadata
    let variants = entity.metadata.attributes.get("variants");
    assert!(variants.is_some());
    let variants_str = variants.unwrap();
    assert!(variants_str.contains("First"));
    assert!(variants_str.contains("Second"));
    assert!(variants_str.contains("Third"));
}

#[test]
fn test_enum_with_discriminants() {
    let source = r#"
enum StatusCode {
    Ok = 200,
    NotFound = 404,
    ServerError = 500,
}
"#;

    let entities = extract_with_handler(source, queries::ENUM_QUERY, handle_enum_impl)
        .expect("Failed to extract enum");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.name, "StatusCode");
    assert_eq!(entity.entity_type, EntityType::Enum);

    // Check enum variants with discriminants
    let variants = entity.metadata.attributes.get("variants");
    assert!(variants.is_some());
    let variants_str = variants.unwrap();
    assert!(variants_str.contains("Ok"));
    assert!(variants_str.contains("NotFound"));
    assert!(variants_str.contains("ServerError"));
}

#[test]
fn test_enum_with_tuple_variants() {
    let source = r#"
enum Message {
    Move(i32, i32),
    Write(String),
    Color(u8, u8, u8),
}
"#;

    let entities = extract_with_handler(source, queries::ENUM_QUERY, handle_enum_impl)
        .expect("Failed to extract enum");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.name, "Message");
    assert_eq!(entity.entity_type, EntityType::Enum);

    // Check tuple variants
    let variants = entity.metadata.attributes.get("variants");
    assert!(variants.is_some());
    let variants_str = variants.unwrap();
    assert!(variants_str.contains("Move"));
    assert!(variants_str.contains("Write"));
    assert!(variants_str.contains("Color"));
}

#[test]
fn test_enum_with_struct_variants() {
    let source = r#"
enum Event {
    Click { x: i32, y: i32 },
    KeyPress { key: char, modifiers: u8 },
    Scroll { delta: f32 },
}
"#;

    let entities = extract_with_handler(source, queries::ENUM_QUERY, handle_enum_impl)
        .expect("Failed to extract enum");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.name, "Event");
    assert_eq!(entity.entity_type, EntityType::Enum);

    // Check struct variants
    let variants = entity.metadata.attributes.get("variants");
    assert!(variants.is_some());
    let variants_str = variants.unwrap();
    assert!(variants_str.contains("Click"));
    assert!(variants_str.contains("KeyPress"));
    assert!(variants_str.contains("Scroll"));
}

#[test]
fn test_generic_enum() {
    let source = r#"
enum Option<T> {
    Some(T),
    None,
}
"#;

    let entities = extract_with_handler(source, queries::ENUM_QUERY, handle_enum_impl)
        .expect("Failed to extract enum");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.name, "Option");
    assert_eq!(entity.entity_type, EntityType::Enum);

    // Check generics
    assert!(entity.metadata.is_generic);
    assert_eq!(entity.metadata.generic_params.len(), 1);
    assert!(entity.metadata.generic_params.contains(&"T".to_string()));

    // Check variants
    let variants = entity.metadata.attributes.get("variants");
    assert!(variants.is_some());
    let variants_str = variants.unwrap();
    assert!(variants_str.contains("Some"));
    assert!(variants_str.contains("None"));
}

#[test]
fn test_enum_with_derives() {
    let source = r#"
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Comparison {
    Less,
    Equal,
    Greater,
}
"#;

    let entities = extract_with_handler(source, queries::ENUM_QUERY, handle_enum_impl)
        .expect("Failed to extract enum");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.name, "Comparison");
    assert_eq!(entity.entity_type, EntityType::Enum);

    // Check derives stored as decorators
    assert!(entity.metadata.decorators.contains(&"Debug".to_string()));
    assert!(entity.metadata.decorators.contains(&"Clone".to_string()));
    assert!(entity.metadata.decorators.contains(&"Copy".to_string()));
    assert!(entity
        .metadata
        .decorators
        .contains(&"PartialEq".to_string()));
    assert!(entity.metadata.decorators.contains(&"Eq".to_string()));

    // Check variants
    let variants = entity.metadata.attributes.get("variants");
    assert!(variants.is_some());
}

#[test]
fn test_complex_nested_enum() {
    let source = r#"
enum Result<T, E> {
    Ok(T),
    Err(E),
}

enum ComplexEnum<'a, T: Clone> {
    Simple,
    Reference(&'a str),
    Tuple(T, T),
    Struct {
        field: Vec<T>,
        reference: &'a [u8],
    },
}
"#;

    let entities = extract_with_handler(source, queries::ENUM_QUERY, handle_enum_impl)
        .expect("Failed to extract enum");

    // Should extract both enums
    assert_eq!(entities.len(), 2);

    // Check the second, more complex enum
    let entity = &entities[1];
    assert_eq!(entity.name, "ComplexEnum");
    assert_eq!(entity.entity_type, EntityType::Enum);

    // Check generics with lifetime and trait bounds
    assert!(entity.metadata.is_generic);
    assert_eq!(entity.metadata.generic_params.len(), 2);

    // Check variants
    let variants = entity.metadata.attributes.get("variants");
    assert!(variants.is_some());
    let variants_str = variants.unwrap();
    assert!(variants_str.contains("Simple"));
    assert!(variants_str.contains("Reference"));
    assert!(variants_str.contains("Tuple"));
    assert!(variants_str.contains("Struct"));
}

#[test]
fn test_enum_with_doc_comments() {
    let source = r#"
/// Represents the state of a connection
///
/// This enum tracks the lifecycle of a network connection
#[derive(Debug)]
pub enum ConnectionState {
    /// Initial state before connection attempt
    Disconnected,
    /// Currently attempting to connect
    Connecting,
    /// Successfully connected
    Connected,
    /// Connection lost, might reconnect
    Disconnecting,
}
"#;

    let entities = extract_with_handler(source, queries::ENUM_QUERY, handle_enum_impl)
        .expect("Failed to extract enum");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];

    assert!(entity.documentation_summary.is_some());
    let doc = entity.documentation_summary.as_ref().unwrap();
    assert!(doc.contains("state of a connection"));
    assert!(doc.contains("lifecycle"));
}
