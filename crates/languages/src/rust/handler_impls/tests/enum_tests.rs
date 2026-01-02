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

    // Enum + 3 variants
    assert_eq!(entities.len(), 4);
    let enum_entity = &entities[0];
    assert_eq!(enum_entity.name, "SimpleEnum");
    assert_eq!(enum_entity.entity_type, EntityType::Enum);

    // Check variant entities
    let variant_entities: Vec<_> = entities
        .iter()
        .filter(|e| e.entity_type == EntityType::EnumVariant)
        .collect();
    assert_eq!(variant_entities.len(), 3);

    let variant_names: Vec<&str> = variant_entities.iter().map(|e| e.name.as_str()).collect();
    assert!(variant_names.contains(&"First"));
    assert!(variant_names.contains(&"Second"));
    assert!(variant_names.contains(&"Third"));

    // All variants should have enum as parent
    for variant in &variant_entities {
        assert_eq!(
            variant.parent_scope.as_deref(),
            Some("SimpleEnum"),
            "Variant should have enum as parent"
        );
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

    let entities = extract_with_handler(source, queries::ENUM_QUERY, handle_enum_impl)
        .expect("Failed to extract enum");

    // Enum + 3 variants
    assert_eq!(entities.len(), 4);
    let enum_entity = &entities[0];
    assert_eq!(enum_entity.name, "StatusCode");
    assert_eq!(enum_entity.entity_type, EntityType::Enum);

    // Check variant entities with discriminants
    let ok_variant = entities
        .iter()
        .find(|e| e.name == "Ok")
        .expect("Should have Ok variant");
    assert_eq!(ok_variant.entity_type, EntityType::EnumVariant);
    assert_eq!(
        ok_variant.metadata.attributes.get("discriminant"),
        Some(&"200".to_string())
    );

    let not_found_variant = entities
        .iter()
        .find(|e| e.name == "NotFound")
        .expect("Should have NotFound variant");
    assert_eq!(
        not_found_variant.metadata.attributes.get("discriminant"),
        Some(&"404".to_string())
    );
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

    // Enum + 3 variants
    assert_eq!(entities.len(), 4);
    let enum_entity = &entities[0];
    assert_eq!(enum_entity.name, "Message");
    assert_eq!(enum_entity.entity_type, EntityType::Enum);

    // Check tuple variant content
    let move_variant = entities
        .iter()
        .find(|e| e.name == "Move")
        .expect("Should have Move variant");
    assert_eq!(move_variant.entity_type, EntityType::EnumVariant);
    assert!(
        move_variant.content.as_ref().unwrap().contains("i32"),
        "Move variant content should include types"
    );

    let color_variant = entities
        .iter()
        .find(|e| e.name == "Color")
        .expect("Should have Color variant");
    assert!(
        color_variant.content.as_ref().unwrap().contains("u8"),
        "Color variant content should include types"
    );
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

    // Enum + 3 variants
    assert_eq!(entities.len(), 4);
    let enum_entity = &entities[0];
    assert_eq!(enum_entity.name, "Event");
    assert_eq!(enum_entity.entity_type, EntityType::Enum);

    // Check struct variant content
    let click_variant = entities
        .iter()
        .find(|e| e.name == "Click")
        .expect("Should have Click variant");
    assert_eq!(click_variant.entity_type, EntityType::EnumVariant);
    assert!(
        click_variant.content.as_ref().unwrap().contains("x:"),
        "Click variant content should include field names"
    );
    assert!(
        click_variant.content.as_ref().unwrap().contains("y:"),
        "Click variant content should include field names"
    );
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

    // Enum + 2 variants
    assert_eq!(entities.len(), 3);
    let enum_entity = &entities[0];
    assert_eq!(enum_entity.name, "Option");
    assert_eq!(enum_entity.entity_type, EntityType::Enum);

    // Check generics
    assert!(enum_entity.metadata.is_generic);
    assert_eq!(enum_entity.metadata.generic_params.len(), 1);
    assert!(enum_entity
        .metadata
        .generic_params
        .contains(&"T".to_string()));

    // Check variants
    let variant_entities: Vec<_> = entities
        .iter()
        .filter(|e| e.entity_type == EntityType::EnumVariant)
        .collect();
    assert_eq!(variant_entities.len(), 2);

    let variant_names: Vec<&str> = variant_entities.iter().map(|e| e.name.as_str()).collect();
    assert!(variant_names.contains(&"Some"));
    assert!(variant_names.contains(&"None"));
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

    // Enum + 3 variants
    assert_eq!(entities.len(), 4);
    let enum_entity = &entities[0];
    assert_eq!(enum_entity.name, "Comparison");
    assert_eq!(enum_entity.entity_type, EntityType::Enum);

    // Check derives stored as decorators
    assert!(enum_entity
        .metadata
        .decorators
        .contains(&"Debug".to_string()));
    assert!(enum_entity
        .metadata
        .decorators
        .contains(&"Clone".to_string()));
    assert!(enum_entity
        .metadata
        .decorators
        .contains(&"Copy".to_string()));
    assert!(enum_entity
        .metadata
        .decorators
        .contains(&"PartialEq".to_string()));
    assert!(enum_entity.metadata.decorators.contains(&"Eq".to_string()));

    // Check variants exist
    let variant_entities: Vec<_> = entities
        .iter()
        .filter(|e| e.entity_type == EntityType::EnumVariant)
        .collect();
    assert_eq!(variant_entities.len(), 3);
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

    // Result(1) + 2 variants + ComplexEnum(1) + 4 variants = 8
    assert_eq!(entities.len(), 8);

    // Find the ComplexEnum entity
    let complex_enum = entities
        .iter()
        .find(|e| e.name == "ComplexEnum" && e.entity_type == EntityType::Enum)
        .expect("Should have ComplexEnum");

    // Check generics with lifetime and trait bounds
    assert!(complex_enum.metadata.is_generic);
    assert_eq!(complex_enum.metadata.generic_params.len(), 2);

    // Check variants
    let complex_variants: Vec<_> = entities
        .iter()
        .filter(|e| {
            e.entity_type == EntityType::EnumVariant
                && e.parent_scope.as_deref() == Some("ComplexEnum")
        })
        .collect();
    assert_eq!(complex_variants.len(), 4);

    let variant_names: Vec<&str> = complex_variants.iter().map(|e| e.name.as_str()).collect();
    assert!(variant_names.contains(&"Simple"));
    assert!(variant_names.contains(&"Reference"));
    assert!(variant_names.contains(&"Tuple"));
    assert!(variant_names.contains(&"Struct"));
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

    // Enum + 4 variants
    assert_eq!(entities.len(), 5);
    let enum_entity = &entities[0];

    assert!(enum_entity.documentation_summary.is_some());
    let doc = enum_entity.documentation_summary.as_ref().unwrap();
    assert!(doc.contains("state of a connection"));
    assert!(doc.contains("lifecycle"));
}

// ============================================================================
// Generic Bounds Extraction Tests
// ============================================================================

#[test]
fn test_enum_with_generic_bounds() {
    let source = r#"
use std::clone::Clone;
use std::fmt::Debug;

enum Container<T: Clone, U>
where
    U: Debug,
{
    Value(T),
    Debug(U),
    Empty,
}
"#;

    let entities = extract_with_handler(source, queries::ENUM_QUERY, handle_enum_impl)
        .expect("Failed to extract enum");

    // Enum + 3 variants
    assert_eq!(entities.len(), 4);
    let enum_entity = &entities[0];
    assert_eq!(enum_entity.name, "Container");
    assert_eq!(enum_entity.entity_type, EntityType::Enum);

    // Check generic_params (backward-compat raw strings)
    assert!(enum_entity.metadata.is_generic);
    assert_eq!(enum_entity.metadata.generic_params.len(), 2);

    // Check generic_bounds (structured) - T has inline, U has where clause
    let bounds = &enum_entity.metadata.generic_bounds;
    assert!(bounds.contains_key("T"), "Should have bounds for T");
    let t_bounds = bounds.get("T").unwrap();
    assert!(
        t_bounds.iter().any(|b| b.contains("Clone")),
        "T should have Clone bound from inline generic"
    );

    assert!(bounds.contains_key("U"), "Should have bounds for U");
    let u_bounds = bounds.get("U").unwrap();
    assert!(
        u_bounds.iter().any(|b| b.contains("Debug")),
        "U should have Debug bound from where clause"
    );

    // Check uses_types includes bound traits (now in typed relationships)
    let uses_types = &enum_entity.relationships.uses_types;
    assert!(!uses_types.is_empty(), "Should have uses_types");
    assert!(
        uses_types.iter().any(|t| t.target().contains("Clone")),
        "uses_types should include Clone"
    );
    assert!(
        uses_types.iter().any(|t| t.target().contains("Debug")),
        "uses_types should include Debug"
    );
}

#[test]
fn test_variant_entity_structure() {
    let source = r#"
pub enum Color {
    Red,
    Green,
    Blue,
}
"#;

    let entities = extract_with_handler(source, queries::ENUM_QUERY, handle_enum_impl)
        .expect("Failed to extract enum");

    // Enum + 3 variants
    assert_eq!(entities.len(), 4);

    // Find the Red variant
    let red_variant = entities
        .iter()
        .find(|e| e.name == "Red")
        .expect("Should have Red variant");

    // Verify EnumVariant entity structure
    assert_eq!(red_variant.entity_type, EntityType::EnumVariant);
    assert_eq!(red_variant.qualified_name, "Color::Red");
    assert_eq!(red_variant.parent_scope.as_deref(), Some("Color"));
    assert_eq!(red_variant.visibility, None); // Variants inherit visibility from parent
    assert!(red_variant.content.is_some());
    assert_eq!(red_variant.content.as_ref().unwrap(), "Red");
}

#[test]
fn test_variant_with_uses_types() {
    let source = r#"
struct Point { x: i32, y: i32 }

enum Shape {
    Circle { center: Point, radius: f32 },
    Rectangle { corners: Vec<Point> },
}
"#;

    let entities = extract_with_handler(source, queries::ENUM_QUERY, handle_enum_impl)
        .expect("Failed to extract enum");

    // Enum + 2 variants = 3
    assert_eq!(entities.len(), 3);

    // Check Circle variant has uses_types for Point
    let circle_variant = entities
        .iter()
        .find(|e| e.name == "Circle")
        .expect("Should have Circle variant");
    assert!(
        !circle_variant.relationships.uses_types.is_empty(),
        "Circle variant should have uses_types for Point"
    );
    assert!(
        circle_variant
            .relationships
            .uses_types
            .iter()
            .any(|t| t.target().contains("Point")),
        "Circle should reference Point type"
    );
}
