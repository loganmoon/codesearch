//! Tests for TypeScript enum extraction handler

use super::extract_with_handler;
use crate::typescript::{handler_impls::handle_enum_impl, queries};
use codesearch_core::entities::EntityType;

#[test]
fn test_basic_enum() {
    let source = r#"
enum Status {
    Active,
    Inactive,
    Pending
}
"#;

    let entities = extract_with_handler(source, queries::ENUM_QUERY, handle_enum_impl)
        .expect("Failed to extract enum");

    // Enum + 3 members
    assert_eq!(entities.len(), 4);

    let enum_entity = &entities[0];
    assert_eq!(enum_entity.name, "Status");
    assert_eq!(enum_entity.entity_type, EntityType::Enum);

    // Check member entities
    let member_entities: Vec<_> = entities
        .iter()
        .filter(|e| e.entity_type == EntityType::EnumVariant)
        .collect();
    assert_eq!(member_entities.len(), 3);

    let member_names: Vec<&str> = member_entities.iter().map(|e| e.name.as_str()).collect();
    assert!(member_names.contains(&"Active"));
    assert!(member_names.contains(&"Inactive"));
    assert!(member_names.contains(&"Pending"));
}

#[test]
fn test_enum_member_parent_scope() {
    let source = r#"
enum Direction {
    Up,
    Down,
    Left,
    Right
}
"#;

    let entities = extract_with_handler(source, queries::ENUM_QUERY, handle_enum_impl)
        .expect("Failed to extract enum");

    // Enum + 4 members
    assert_eq!(entities.len(), 5);

    // All members should have enum as parent
    let member_entities: Vec<_> = entities
        .iter()
        .filter(|e| e.entity_type == EntityType::EnumVariant)
        .collect();

    for member in &member_entities {
        assert_eq!(
            member.parent_scope.as_deref(),
            Some("Direction"),
            "Member '{}' should have enum as parent",
            member.name
        );
    }
}

#[test]
fn test_enum_member_qualified_name() {
    let source = r#"
enum Color {
    Red
}
"#;

    let entities = extract_with_handler(source, queries::ENUM_QUERY, handle_enum_impl)
        .expect("Failed to extract enum");

    // Enum + 1 member
    assert_eq!(entities.len(), 2);

    let member_entity = entities
        .iter()
        .find(|e| e.entity_type == EntityType::EnumVariant)
        .expect("Should have member entity");

    assert_eq!(member_entity.qualified_name, "Color.Red");
}

#[test]
fn test_numeric_enum_values() {
    let source = r#"
enum HttpStatus {
    OK = 200,
    NotFound = 404,
    ServerError = 500
}
"#;

    let entities = extract_with_handler(source, queries::ENUM_QUERY, handle_enum_impl)
        .expect("Failed to extract enum");

    // Enum + 3 members
    assert_eq!(entities.len(), 4);

    // Check OK member has value
    let ok_member = entities
        .iter()
        .find(|e| e.name == "OK")
        .expect("Should have OK member");
    assert_eq!(
        ok_member.metadata.attributes.get("value"),
        Some(&"200".to_string())
    );

    // Check NotFound member has value
    let not_found_member = entities
        .iter()
        .find(|e| e.name == "NotFound")
        .expect("Should have NotFound member");
    assert_eq!(
        not_found_member.metadata.attributes.get("value"),
        Some(&"404".to_string())
    );

    // Check ServerError member has value
    let server_error_member = entities
        .iter()
        .find(|e| e.name == "ServerError")
        .expect("Should have ServerError member");
    assert_eq!(
        server_error_member.metadata.attributes.get("value"),
        Some(&"500".to_string())
    );
}

#[test]
fn test_string_enum_values() {
    let source = r#"
enum LogLevel {
    Debug = "DEBUG",
    Info = "INFO",
    Warn = "WARN",
    Error = "ERROR"
}
"#;

    let entities = extract_with_handler(source, queries::ENUM_QUERY, handle_enum_impl)
        .expect("Failed to extract enum");

    // Enum + 4 members
    assert_eq!(entities.len(), 5);

    // Check Debug member has string value
    let debug_member = entities
        .iter()
        .find(|e| e.name == "Debug")
        .expect("Should have Debug member");
    assert_eq!(
        debug_member.metadata.attributes.get("value"),
        Some(&"\"DEBUG\"".to_string())
    );

    // Check Error member has string value
    let error_member = entities
        .iter()
        .find(|e| e.name == "Error")
        .expect("Should have Error member");
    assert_eq!(
        error_member.metadata.attributes.get("value"),
        Some(&"\"ERROR\"".to_string())
    );
}

#[test]
fn test_enum_member_content() {
    let source = r#"
enum Priority {
    Low = 1,
    Medium = 2,
    High = 3
}
"#;

    let entities = extract_with_handler(source, queries::ENUM_QUERY, handle_enum_impl)
        .expect("Failed to extract enum");

    // Find High member
    let high_member = entities
        .iter()
        .find(|e| e.name == "High")
        .expect("Should have High member");

    // Content should include name and value
    let content = high_member.content.as_ref().expect("Should have content");
    assert!(
        content.contains("High"),
        "Content should contain member name"
    );
    assert!(content.contains("3"), "Content should contain member value");
}

#[test]
fn test_mixed_enum_values() {
    let source = r#"
enum Mixed {
    First,
    Second = 10,
    Third
}
"#;

    let entities = extract_with_handler(source, queries::ENUM_QUERY, handle_enum_impl)
        .expect("Failed to extract enum");

    // Enum + 3 members
    assert_eq!(entities.len(), 4);

    // First should have no value
    let first_member = entities
        .iter()
        .find(|e| e.name == "First")
        .expect("Should have First member");
    assert_eq!(first_member.metadata.attributes.get("value"), None);

    // Second should have value 10
    let second_member = entities
        .iter()
        .find(|e| e.name == "Second")
        .expect("Should have Second member");
    assert_eq!(
        second_member.metadata.attributes.get("value"),
        Some(&"10".to_string())
    );

    // Third should have no explicit value
    let third_member = entities
        .iter()
        .find(|e| e.name == "Third")
        .expect("Should have Third member");
    assert_eq!(third_member.metadata.attributes.get("value"), None);
}

#[test]
fn test_single_member_enum() {
    let source = r#"
enum Singleton {
    Instance
}
"#;

    let entities = extract_with_handler(source, queries::ENUM_QUERY, handle_enum_impl)
        .expect("Failed to extract enum");

    // Enum + 1 member
    assert_eq!(entities.len(), 2);

    let enum_entity = &entities[0];
    assert_eq!(enum_entity.entity_type, EntityType::Enum);

    let member_entity = &entities[1];
    assert_eq!(member_entity.entity_type, EntityType::EnumVariant);
    assert_eq!(member_entity.name, "Instance");
    assert_eq!(member_entity.qualified_name, "Singleton.Instance");
    assert_eq!(member_entity.parent_scope.as_deref(), Some("Singleton"));
}
