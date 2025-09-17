//! Tests for enum extraction handler

use super::*;
use crate::rust::entities::RustEntityVariant;
use crate::rust::handlers::type_handlers::handle_enum;
use crate::transport::EntityVariant;

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
    assert_eq!(entity.name, "SimpleEnum");

    if let EntityVariant::Rust(RustEntityVariant::Enum { variants, .. }) = &entity.variant {
        assert_eq!(variants.len(), 3);
        assert_eq!(variants[0].name, "First");
        assert_eq!(variants[1].name, "Second");
        assert_eq!(variants[2].name, "Third");
    } else {
        panic!("Expected enum variant");
    }
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
    let entity = &entities[0];

    if let EntityVariant::Rust(RustEntityVariant::Enum { variants, .. }) = &entity.variant {
        assert_eq!(variants.len(), 3);
        assert_eq!(variants[0].discriminant, Some("200".to_string()));
        assert_eq!(variants[1].discriminant, Some("404".to_string()));
        assert_eq!(variants[2].discriminant, Some("500".to_string()));
    } else {
        panic!("Expected enum variant");
    }
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
    let entity = &entities[0];

    if let EntityVariant::Rust(RustEntityVariant::Enum { variants, .. }) = &entity.variant {
        assert_eq!(variants.len(), 3);

        // Check tuple fields
        assert_eq!(variants[0].fields.len(), 2);
        assert_eq!(variants[0].fields[0].field_type, "i32");

        assert_eq!(variants[1].fields.len(), 1);
        assert_eq!(variants[1].fields[0].field_type, "String");

        assert_eq!(variants[2].fields.len(), 3);
        assert_eq!(variants[2].fields[0].field_type, "u8");
    } else {
        panic!("Expected enum variant");
    }
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
    let entity = &entities[0];

    if let EntityVariant::Rust(RustEntityVariant::Enum { variants, .. }) = &entity.variant {
        assert_eq!(variants.len(), 3);

        // Check struct fields
        assert_eq!(variants[0].fields.len(), 2);
        assert_eq!(variants[0].fields[0].name, "x");
        assert_eq!(variants[0].fields[0].field_type, "i32");

        assert_eq!(variants[1].fields.len(), 2);
        assert_eq!(variants[1].fields[0].name, "key");
        assert_eq!(variants[1].fields[0].field_type, "char");
    } else {
        panic!("Expected enum variant");
    }
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
    let entity = &entities[0];

    if let EntityVariant::Rust(RustEntityVariant::Enum {
        generics, variants, ..
    }) = &entity.variant
    {
        assert_eq!(generics.len(), 1);
        assert!(generics.contains(&"T".to_string()));
        assert_eq!(variants.len(), 2);
        assert_eq!(variants[0].name, "Some");
        assert_eq!(variants[1].name, "None");
    } else {
        panic!("Expected enum variant");
    }
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
    let entity = &entities[0];

    if let EntityVariant::Rust(RustEntityVariant::Enum { derives, .. }) = &entity.variant {
        assert!(derives.contains(&"Debug".to_string()));
        assert!(derives.contains(&"Clone".to_string()));
        assert!(derives.contains(&"Copy".to_string()));
        assert!(derives.contains(&"PartialEq".to_string()));
        assert!(derives.contains(&"Eq".to_string()));
    } else {
        panic!("Expected enum variant");
    }
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
    assert_eq!(entity.name, "ComplexEnum");

    if let EntityVariant::Rust(RustEntityVariant::Enum {
        generics, variants, ..
    }) = &entity.variant
    {
        assert_eq!(generics.len(), 2);
        assert_eq!(variants.len(), 4);
        assert_eq!(variants[0].name, "Simple");
        assert_eq!(variants[1].name, "Reference");
        assert_eq!(variants[2].name, "Tuple");
        assert_eq!(variants[3].name, "Struct");
    } else {
        panic!("Expected enum variant");
    }
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

    assert!(entity.documentation.is_some());
    let doc = entity.documentation.as_ref().unwrap();
    assert!(doc.contains("state of a connection"));
    assert!(doc.contains("lifecycle"));
}
