//! Tests for struct extraction handler

use super::*;
use crate::rust::entities::FieldInfo;
use crate::rust::handler_impls::type_handlers::handle_struct_impl;
use codesearch_core::entities::{EntityType, Visibility};

#[test]
fn test_unit_struct() {
    let source = r#"
struct UnitStruct;
"#;

    let entities = extract_with_handler(source, queries::STRUCT_QUERY, handle_struct_impl)
        .expect("Failed to extract struct");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.name, "UnitStruct");
    assert_eq!(entity.entity_type, EntityType::Struct);

    // Unit struct has no fields
    assert_eq!(entity.metadata.attributes.get("fields"), None);
    assert_eq!(entity.metadata.attributes.get("struct_type"), None);
}

#[test]
fn test_tuple_struct() {
    let source = r#"
struct TupleStruct(i32, String, bool);
"#;

    let entities = extract_with_handler(source, queries::STRUCT_QUERY, handle_struct_impl)
        .expect("Failed to extract struct");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.entity_type, EntityType::Struct);

    // Check it's marked as tuple
    assert_eq!(
        entity
            .metadata
            .attributes
            .get("struct_type")
            .map(|s| s.as_str()),
        Some("tuple")
    );

    // Check fields (stored as JSON in attributes)
    let fields_str = entity
        .metadata
        .attributes
        .get("fields")
        .expect("Tuple struct should have fields");
    let fields: Vec<FieldInfo> =
        serde_json::from_str(fields_str).expect("Failed to parse fields JSON");
    assert_eq!(fields.len(), 3);
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

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.entity_type, EntityType::Struct);

    // Not a tuple struct
    assert_eq!(entity.metadata.attributes.get("struct_type"), None);

    // Check fields
    let fields_str = entity
        .metadata
        .attributes
        .get("fields")
        .expect("Struct should have fields");
    let fields: Vec<FieldInfo> =
        serde_json::from_str(fields_str).expect("Failed to parse fields JSON");
    assert_eq!(fields.len(), 4);

    let field_names: Vec<&str> = fields.iter().map(|f| f.name.as_str()).collect();
    assert!(field_names.contains(&"id"));
    assert!(field_names.contains(&"name"));
    assert!(field_names.contains(&"email"));
    assert!(field_names.contains(&"is_active"));
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

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.entity_type, EntityType::Struct);

    // Check generics
    assert!(entity.metadata.is_generic);
    assert_eq!(entity.metadata.generic_params.len(), 2);
    assert!(entity.metadata.generic_params.contains(&"T".to_string()));
    assert!(entity.metadata.generic_params.contains(&"U".to_string()));

    // Check fields
    let fields_str = entity
        .metadata
        .attributes
        .get("fields")
        .expect("Struct should have fields");
    let fields: Vec<FieldInfo> =
        serde_json::from_str(fields_str).expect("Failed to parse fields JSON");
    assert_eq!(fields.len(), 3);
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

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.entity_type, EntityType::Struct);

    // Check derives stored as decorators
    assert!(entity.metadata.decorators.contains(&"Debug".to_string()));
    assert!(entity.metadata.decorators.contains(&"Clone".to_string()));
    assert!(entity
        .metadata
        .decorators
        .contains(&"PartialEq".to_string()));
    assert!(entity
        .metadata
        .decorators
        .contains(&"Serialize".to_string()));
    assert!(entity
        .metadata
        .decorators
        .contains(&"Deserialize".to_string()));
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

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.entity_type, EntityType::Struct);

    // Check generics (includes lifetimes)
    assert_eq!(entity.metadata.generic_params.len(), 2);

    // Check fields
    let fields_str = entity
        .metadata
        .attributes
        .get("fields")
        .expect("Struct should have fields");
    let fields: Vec<FieldInfo> =
        serde_json::from_str(fields_str).expect("Failed to parse fields JSON");
    assert_eq!(fields.len(), 2);
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

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];

    assert!(entity.documentation_summary.is_some());
    let doc = entity.documentation_summary.as_ref().unwrap();
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

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.entity_type, EntityType::Struct);

    // Check fields
    let fields_str = entity
        .metadata
        .attributes
        .get("fields")
        .expect("Struct should have fields");
    let fields: Vec<FieldInfo> =
        serde_json::from_str(fields_str).expect("Failed to parse fields JSON");
    assert_eq!(fields.len(), 2);

    let field_names: Vec<&str> = fields.iter().map(|f| f.name.as_str()).collect();
    assert!(field_names.contains(&"data"));
    assert!(field_names.contains(&"cache"));
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

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.entity_type, EntityType::Struct);
    assert_eq!(entity.visibility, Visibility::Public);

    // Check fields
    let fields_str = entity
        .metadata
        .attributes
        .get("fields")
        .expect("Struct should have fields");
    let fields: Vec<FieldInfo> =
        serde_json::from_str(fields_str).expect("Failed to parse fields JSON");
    assert_eq!(fields.len(), 3);

    // Now we CAN check individual field visibility since we're storing FieldInfo!
    let public_field = fields.iter().find(|f| f.name == "public_field").unwrap();
    assert_eq!(public_field.visibility, Visibility::Public);

    let private_field = fields.iter().find(|f| f.name == "private_field").unwrap();
    assert_eq!(private_field.visibility, Visibility::Private);

    let crate_field = fields.iter().find(|f| f.name == "crate_field").unwrap();
    assert_eq!(crate_field.visibility, Visibility::Public); // pub(crate) is captured as Public
}
