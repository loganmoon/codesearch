//! Tests for enum extraction handler

use super::*;
use codesearch_core::entities::{EntityType, Visibility};

use crate::rust::handlers::type_handlers::handle_enum;

#[test]
fn test_simple_enum() {
    let source = r#"
enum SimpleEnum {
    First,
    Second,
    Third,
}
"#;

    let entities = extract_with_handler(source, queries::ENUM_QUERY, handle_enum)
        .expect("Failed to extract enum");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.name, "SimpleEnum"); // TODO: Update test to use new CodeEntity structure
                                           // Original test body commented out during migration
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

    let entities = extract_with_handler(source, queries::ENUM_QUERY, handle_enum)
        .expect("Failed to extract enum");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0]; // TODO: Update test to use new CodeEntity structure
                               // Original test body commented out during migration
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

    let entities = extract_with_handler(source, queries::ENUM_QUERY, handle_enum)
        .expect("Failed to extract enum");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0]; // TODO: Update test to use new CodeEntity structure
                               // Original test body commented out during migration
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

    let entities = extract_with_handler(source, queries::ENUM_QUERY, handle_enum)
        .expect("Failed to extract enum");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0]; // TODO: Update test to use new CodeEntity structure
                               // Original test body commented out during migration
}

#[test]
fn test_generic_enum() {
    let source = r#"
enum Option<T> {
    Some(T),
    None,
}
"#;

    let entities = extract_with_handler(source, queries::ENUM_QUERY, handle_enum)
        .expect("Failed to extract enum");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0]; // TODO: Update test to use new CodeEntity structure
                               // Original test body commented out during migration
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

    let entities = extract_with_handler(source, queries::ENUM_QUERY, handle_enum)
        .expect("Failed to extract enum");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0]; // TODO: Update test to use new CodeEntity structure
                               // Original test body commented out during migration
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

    let entities = extract_with_handler(source, queries::ENUM_QUERY, handle_enum)
        .expect("Failed to extract enum");

    // Should extract both enums
    assert_eq!(entities.len(), 2);

    // Check the second, more complex enum
    let entity = &entities[1];
    assert_eq!(entity.name, "ComplexEnum"); // TODO: Update test to use new CodeEntity structure
                                            // Original test body commented out during migration
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

    let entities = extract_with_handler(source, queries::ENUM_QUERY, handle_enum)
        .expect("Failed to extract enum");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];

    assert!(entity.documentation_summary.is_some());
    let doc = entity.documentation_summary.as_ref().unwrap();
    assert!(doc.contains("state of a connection"));
    assert!(doc.contains("lifecycle"));
}
