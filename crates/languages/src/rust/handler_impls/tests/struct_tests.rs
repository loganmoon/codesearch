//! Tests for struct extraction handler

use super::*;
use crate::rust::handler_impls::type_handlers::handle_struct_impl;
use codesearch_core::entities::{EntityType, Visibility};

#[test]
fn test_unit_struct() {
    let source = r#"
struct UnitStruct;
"#;

    let entities = extract_with_handler(source, queries::STRUCT_QUERY, handle_struct_impl)
        .expect("Failed to extract struct");

    // Unit struct has no fields, so just 1 entity
    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.name, "UnitStruct");
    assert_eq!(entity.entity_type, EntityType::Struct);

    // No field entities
    assert!(!entities
        .iter()
        .any(|e| e.entity_type == EntityType::Property));
}

#[test]
fn test_tuple_struct() {
    let source = r#"
struct TupleStruct(i32, String, bool);
"#;

    let entities = extract_with_handler(source, queries::STRUCT_QUERY, handle_struct_impl)
        .expect("Failed to extract struct");

    // Struct + 3 fields
    assert_eq!(entities.len(), 4);
    let struct_entity = &entities[0];
    assert_eq!(struct_entity.entity_type, EntityType::Struct);

    // Check it's marked as tuple
    assert_eq!(
        struct_entity
            .metadata
            .attributes
            .get("struct_type")
            .map(|s| s.as_str()),
        Some("tuple")
    );

    // Check field entities
    let field_entities: Vec<_> = entities
        .iter()
        .filter(|e| e.entity_type == EntityType::Property)
        .collect();
    assert_eq!(field_entities.len(), 3);

    // Tuple fields have numeric names
    let field_names: Vec<&str> = field_entities.iter().map(|e| e.name.as_str()).collect();
    assert!(field_names.contains(&"0"));
    assert!(field_names.contains(&"1"));
    assert!(field_names.contains(&"2"));

    // All fields should have struct as parent
    for field in &field_entities {
        assert_eq!(
            field.parent_scope.as_deref(),
            Some("TupleStruct"),
            "Field should have struct as parent"
        );
    }
}

#[test]
fn test_named_field_struct() {
    let source = r#"
struct User {
    id: u64,
    name: String,
    email: String,
    is_active: bool,
}
"#;

    let entities = extract_with_handler(source, queries::STRUCT_QUERY, handle_struct_impl)
        .expect("Failed to extract struct");

    // Struct + 4 fields
    assert_eq!(entities.len(), 5);
    let struct_entity = &entities[0];
    assert_eq!(struct_entity.entity_type, EntityType::Struct);

    // Not a tuple struct
    assert_eq!(struct_entity.metadata.attributes.get("struct_type"), None);

    // Check field entities
    let field_entities: Vec<_> = entities
        .iter()
        .filter(|e| e.entity_type == EntityType::Property)
        .collect();
    assert_eq!(field_entities.len(), 4);

    let field_names: Vec<&str> = field_entities.iter().map(|e| e.name.as_str()).collect();
    assert!(field_names.contains(&"id"));
    assert!(field_names.contains(&"name"));
    assert!(field_names.contains(&"email"));
    assert!(field_names.contains(&"is_active"));

    // Check field qualified names
    let email_field = field_entities
        .iter()
        .find(|e| e.name == "email")
        .expect("Should have email field");
    assert_eq!(email_field.qualified_name, "User::email");
    assert_eq!(email_field.parent_scope.as_deref(), Some("User"));
}

#[test]
fn test_generic_struct() {
    let source = r#"
struct Container<T, U>
where
    T: Clone,
    U: Debug,
{
    item: T,
    metadata: U,
    count: usize,
}
"#;

    let entities = extract_with_handler(source, queries::STRUCT_QUERY, handle_struct_impl)
        .expect("Failed to extract struct");

    // Struct + 3 fields
    assert_eq!(entities.len(), 4);
    let struct_entity = &entities[0];
    assert_eq!(struct_entity.entity_type, EntityType::Struct);

    // Check generics
    assert!(struct_entity.metadata.is_generic);
    assert_eq!(struct_entity.metadata.generic_params.len(), 2);
    // With bounds extraction, where clause bounds are merged into params
    assert!(
        struct_entity
            .metadata
            .generic_params
            .iter()
            .any(|p| p.starts_with("T:") && p.contains("Clone")),
        "T should have Clone bound from where clause, got: {:?}",
        struct_entity.metadata.generic_params
    );
    assert!(
        struct_entity
            .metadata
            .generic_params
            .iter()
            .any(|p| p.starts_with("U:") && p.contains("Debug")),
        "U should have Debug bound from where clause, got: {:?}",
        struct_entity.metadata.generic_params
    );

    // Check field entities
    let field_entities: Vec<_> = entities
        .iter()
        .filter(|e| e.entity_type == EntityType::Property)
        .collect();
    assert_eq!(field_entities.len(), 3);
}

#[test]
fn test_struct_with_derives() {
    let source = r#"
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DerivedStruct {
    value: i32,
}
"#;

    let entities = extract_with_handler(source, queries::STRUCT_QUERY, handle_struct_impl)
        .expect("Failed to extract struct");

    // Struct + 1 field
    assert_eq!(entities.len(), 2);
    let struct_entity = &entities[0];
    assert_eq!(struct_entity.entity_type, EntityType::Struct);

    // Check derives stored as decorators
    assert!(struct_entity
        .metadata
        .decorators
        .contains(&"Debug".to_string()));
    assert!(struct_entity
        .metadata
        .decorators
        .contains(&"Clone".to_string()));
    assert!(struct_entity
        .metadata
        .decorators
        .contains(&"PartialEq".to_string()));
    assert!(struct_entity
        .metadata
        .decorators
        .contains(&"Serialize".to_string()));
    assert!(struct_entity
        .metadata
        .decorators
        .contains(&"Deserialize".to_string()));

    // Check field entity
    let field_entity = &entities[1];
    assert_eq!(field_entity.entity_type, EntityType::Property);
    assert_eq!(field_entity.name, "value");
}

#[test]
fn test_struct_with_lifetime_parameters() {
    let source = r#"
struct Reference<'a, 'b: 'a> {
    first: &'a str,
    second: &'b str,
}
"#;

    let entities = extract_with_handler(source, queries::STRUCT_QUERY, handle_struct_impl)
        .expect("Failed to extract struct");

    // Struct + 2 fields
    assert_eq!(entities.len(), 3);
    let struct_entity = &entities[0];
    assert_eq!(struct_entity.entity_type, EntityType::Struct);

    // Check generics (includes lifetimes)
    assert_eq!(struct_entity.metadata.generic_params.len(), 2);

    // Check field entities
    let field_entities: Vec<_> = entities
        .iter()
        .filter(|e| e.entity_type == EntityType::Property)
        .collect();
    assert_eq!(field_entities.len(), 2);
}

#[test]
fn test_struct_with_doc_comments() {
    let source = r#"
/// A user in the system
///
/// This struct represents a registered user
#[derive(Debug)]
pub struct DocumentedUser {
    /// The unique identifier
    pub id: u64,
    /// The user's display name
    pub name: String,
}
"#;

    let entities = extract_with_handler(source, queries::STRUCT_QUERY, handle_struct_impl)
        .expect("Failed to extract struct");

    // Struct + 2 fields
    assert_eq!(entities.len(), 3);
    let struct_entity = &entities[0];

    assert!(struct_entity.documentation_summary.is_some());
    let doc = struct_entity.documentation_summary.as_ref().unwrap();
    assert!(doc.contains("user in the system"));
    assert!(doc.contains("registered user"));
}

#[test]
fn test_nested_generic_struct() {
    let source = r#"
struct Complex<T>
where
    T: Iterator<Item = String>,
{
    data: Vec<Option<Box<T>>>,
    cache: HashMap<String, T>,
}
"#;

    let entities = extract_with_handler(source, queries::STRUCT_QUERY, handle_struct_impl)
        .expect("Failed to extract struct");

    // Struct + 2 fields
    assert_eq!(entities.len(), 3);
    let struct_entity = &entities[0];
    assert_eq!(struct_entity.entity_type, EntityType::Struct);

    // Check field entities
    let field_entities: Vec<_> = entities
        .iter()
        .filter(|e| e.entity_type == EntityType::Property)
        .collect();
    assert_eq!(field_entities.len(), 2);

    let field_names: Vec<&str> = field_entities.iter().map(|e| e.name.as_str()).collect();
    assert!(field_names.contains(&"data"));
    assert!(field_names.contains(&"cache"));

    // Check that field entities have uses_types for complex types
    let data_field = field_entities
        .iter()
        .find(|e| e.name == "data")
        .expect("Should have data field");
    // Vec, Option, Box are standard library types - they should be in uses_types
    assert!(
        !data_field.relationships.uses_types.is_empty(),
        "data field should have uses_types for Vec, Option, Box"
    );
}

#[test]
fn test_public_vs_private_fields() {
    let source = r#"
pub struct MixedVisibility {
    pub public_field: i32,
    private_field: String,
    pub(crate) crate_field: bool,
}
"#;

    let entities = extract_with_handler(source, queries::STRUCT_QUERY, handle_struct_impl)
        .expect("Failed to extract struct");

    // Struct + 3 fields
    assert_eq!(entities.len(), 4);
    let struct_entity = &entities[0];
    assert_eq!(struct_entity.entity_type, EntityType::Struct);
    assert_eq!(struct_entity.visibility, Some(Visibility::Public));

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
    assert_eq!(crate_field.visibility, Some(Visibility::Internal)); // pub(crate) is captured as Internal
}

// ============================================================================
// Generic Bounds Extraction Tests
// ============================================================================

#[test]
fn test_generic_struct_with_where_clause() {
    let source = r#"
pub struct Container<T, U>
where
    T: Clone,
    U: Debug + Send,
{
    data: T,
    meta: U,
}
"#;

    let entities = extract_with_handler(source, queries::STRUCT_QUERY, handle_struct_impl)
        .expect("Failed to extract struct");

    // Struct + 2 fields
    assert_eq!(entities.len(), 3);
    let struct_entity = &entities[0];

    // Check generic_bounds includes where clause bounds
    let bounds = &struct_entity.metadata.generic_bounds;
    assert!(bounds.contains_key("T"), "Should have bounds for T");
    assert!(bounds.contains_key("U"), "Should have bounds for U");

    let t_bounds = bounds.get("T").unwrap();
    assert!(
        t_bounds.iter().any(|b| b.contains("Clone")),
        "T should have Clone bound, got: {:?}",
        t_bounds
    );

    let u_bounds = bounds.get("U").unwrap();
    assert!(
        u_bounds.iter().any(|b| b.contains("Debug")),
        "U should have Debug bound, got: {:?}",
        u_bounds
    );
    assert!(
        u_bounds.iter().any(|b| b.contains("Send")),
        "U should have Send bound, got: {:?}",
        u_bounds
    );

    // Check struct-level uses_types includes bound traits (now in typed relationships)
    let uses_types = &struct_entity.relationships.uses_types;
    assert!(!uses_types.is_empty(), "Should have uses_types");
    assert!(
        uses_types.iter().any(|t| t.target().contains("Clone")),
        "uses_types should include Clone, got: {:?}",
        uses_types
    );
    assert!(
        uses_types.iter().any(|t| t.target().contains("Debug")),
        "uses_types should include Debug, got: {:?}",
        uses_types
    );
    assert!(
        uses_types.iter().any(|t| t.target().contains("Send")),
        "uses_types should include Send, got: {:?}",
        uses_types
    );
}

#[test]
fn test_field_entity_structure() {
    let source = r#"
pub struct Person {
    pub name: String,
    age: u32,
}
"#;

    let entities = extract_with_handler(source, queries::STRUCT_QUERY, handle_struct_impl)
        .expect("Failed to extract struct");

    // Struct + 2 fields
    assert_eq!(entities.len(), 3);

    // Find the name field
    let name_field = entities
        .iter()
        .find(|e| e.name == "name")
        .expect("Should have name field");

    // Verify Property entity structure
    assert_eq!(name_field.entity_type, EntityType::Property);
    assert_eq!(name_field.qualified_name, "Person::name");
    assert_eq!(name_field.parent_scope.as_deref(), Some("Person"));
    assert_eq!(name_field.visibility, Some(Visibility::Public));
    assert!(name_field.content.is_some());
    assert!(
        name_field.content.as_ref().unwrap().contains("name"),
        "Content should include field name"
    );
    assert!(
        name_field.content.as_ref().unwrap().contains("String"),
        "Content should include field type"
    );
}
