//! Tests for union extraction handler

use super::*;
use crate::rust::handler_impls::union_handlers::handle_union_impl;
use codesearch_core::entities::{EntityType, Visibility};

#[test]
fn test_basic_union_with_fields() {
    let source = r#"
union Data {
    i: i32,
    f: f32,
}
"#;

    let entities = extract_with_handler(source, queries::UNION_QUERY, handle_union_impl)
        .expect("Failed to extract union");

    // Union + 2 fields
    assert_eq!(entities.len(), 3);

    let union_entity = &entities[0];
    assert_eq!(union_entity.name, "Data");
    assert_eq!(union_entity.entity_type, EntityType::Union);

    // Check field entities
    let field_entities: Vec<_> = entities
        .iter()
        .filter(|e| e.entity_type == EntityType::Property)
        .collect();
    assert_eq!(field_entities.len(), 2);

    let field_names: Vec<&str> = field_entities.iter().map(|e| e.name.as_str()).collect();
    assert!(field_names.contains(&"i"));
    assert!(field_names.contains(&"f"));
}

#[test]
fn test_union_field_parent_scope() {
    let source = r#"
union NumberUnion {
    integer: i64,
    floating: f64,
    unsigned: u64,
}
"#;

    let entities = extract_with_handler(source, queries::UNION_QUERY, handle_union_impl)
        .expect("Failed to extract union");

    // Union + 3 fields
    assert_eq!(entities.len(), 4);

    // All fields should have union as parent
    let field_entities: Vec<_> = entities
        .iter()
        .filter(|e| e.entity_type == EntityType::Property)
        .collect();

    for field in &field_entities {
        assert_eq!(
            field.parent_scope.as_deref(),
            Some("NumberUnion"),
            "Field '{}' should have union as parent",
            field.name
        );
    }
}

#[test]
fn test_union_field_qualified_name() {
    let source = r#"
union MyUnion {
    value: u32,
}
"#;

    let entities = extract_with_handler(source, queries::UNION_QUERY, handle_union_impl)
        .expect("Failed to extract union");

    // Union + 1 field
    assert_eq!(entities.len(), 2);

    let field_entity = entities
        .iter()
        .find(|e| e.entity_type == EntityType::Property)
        .expect("Should have field entity");

    assert_eq!(field_entity.qualified_name, "MyUnion::value");
}

#[test]
fn test_union_field_uses_types_for_non_primitive() {
    let source = r#"
struct Config {
    name: String,
}

union DataHolder {
    config: Config,
    raw: u64,
}
"#;

    // Extract only union (struct is a different query)
    let entities = extract_with_handler(source, queries::UNION_QUERY, handle_union_impl)
        .expect("Failed to extract union");

    // Union + 2 fields
    assert_eq!(entities.len(), 3);

    // Find the config field
    let config_field = entities
        .iter()
        .find(|e| e.entity_type == EntityType::Property && e.name == "config")
        .expect("Should have config field");

    // Should have Config in uses_types
    assert!(
        config_field
            .relationships
            .uses_types
            .iter()
            .any(|t| t.target().contains("Config")),
        "config field should have uses_types for Config, got: {:?}",
        config_field.relationships.uses_types
    );

    // The raw field has primitive type, no uses_types
    let raw_field = entities
        .iter()
        .find(|e| e.entity_type == EntityType::Property && e.name == "raw")
        .expect("Should have raw field");

    assert!(
        raw_field.relationships.uses_types.is_empty(),
        "raw field should have no uses_types for primitive u64"
    );
}

#[test]
fn test_union_field_visibility() {
    let source = r#"
pub union MixedVisibility {
    pub public_field: i32,
    private_field: u32,
    pub(crate) crate_field: f32,
}
"#;

    let entities = extract_with_handler(source, queries::UNION_QUERY, handle_union_impl)
        .expect("Failed to extract union");

    // Union + 3 fields
    assert_eq!(entities.len(), 4);

    let union_entity = &entities[0];
    assert_eq!(union_entity.visibility, Some(Visibility::Public));

    // Check field visibilities
    let public_field = entities
        .iter()
        .find(|e| e.name == "public_field")
        .expect("Should have public_field");
    assert_eq!(public_field.visibility, Some(Visibility::Public));

    let private_field = entities
        .iter()
        .find(|e| e.name == "private_field")
        .expect("Should have private_field");
    assert_eq!(private_field.visibility, Some(Visibility::Private));

    let crate_field = entities
        .iter()
        .find(|e| e.name == "crate_field")
        .expect("Should have crate_field");
    assert_eq!(crate_field.visibility, Some(Visibility::Internal));
}

#[test]
fn test_generic_union() {
    let source = r#"
union GenericUnion<T> {
    typed: T,
    raw: u64,
}
"#;

    let entities = extract_with_handler(source, queries::UNION_QUERY, handle_union_impl)
        .expect("Failed to extract union");

    // Union + 2 fields
    assert_eq!(entities.len(), 3);

    let union_entity = &entities[0];
    assert!(union_entity.metadata.is_generic);
    assert_eq!(union_entity.metadata.generic_params.len(), 1);

    // Check field entities exist
    let field_entities: Vec<_> = entities
        .iter()
        .filter(|e| e.entity_type == EntityType::Property)
        .collect();
    assert_eq!(field_entities.len(), 2);
}

#[test]
fn test_union_with_complex_types() {
    let source = r#"
union ComplexUnion {
    boxed: Box<String>,
    array: [u8; 16],
    ptr: *const u8,
}
"#;

    let entities = extract_with_handler(source, queries::UNION_QUERY, handle_union_impl)
        .expect("Failed to extract union");

    // Union + 3 fields
    assert_eq!(entities.len(), 4);

    // Check that boxed field has uses_types for Box and String
    let boxed_field = entities
        .iter()
        .find(|e| e.name == "boxed")
        .expect("Should have boxed field");

    assert!(
        !boxed_field.relationships.uses_types.is_empty(),
        "boxed field should have uses_types for Box<String>"
    );
}
