//! Tests for struct extraction handler

use super::*;
use crate::rust::entities::RustEntityVariant;
use crate::rust::handlers::type_handlers::handle_struct;
use crate::transport::EntityVariant;

#[test]
fn test_unit_struct() {
    let source = r#"
struct UnitStruct;
"#;

    let entities = extract_with_handler(source, queries::STRUCT_QUERY, handle_struct)
        .expect("Failed to extract struct");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.name, "UnitStruct");

    if let EntityVariant::Rust(RustEntityVariant::Struct {
        fields, is_tuple, ..
    }) = &entity.variant
    {
        assert_eq!(fields.len(), 0);
        assert!(!is_tuple);
    } else {
        panic!("Expected struct variant");
    }
}

#[test]
fn test_tuple_struct() {
    let source = r#"
struct TupleStruct(i32, String, bool);
"#;

    let entities = extract_with_handler(source, queries::STRUCT_QUERY, handle_struct)
        .expect("Failed to extract struct");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];

    if let EntityVariant::Rust(RustEntityVariant::Struct {
        fields, is_tuple, ..
    }) = &entity.variant
    {
        assert!(is_tuple);
        assert_eq!(fields.len(), 3);
        assert_eq!(fields[0].field_type, "i32");
        assert_eq!(fields[1].field_type, "String");
        assert_eq!(fields[2].field_type, "bool");
    } else {
        panic!("Expected struct variant");
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

    let entities = extract_with_handler(source, queries::STRUCT_QUERY, handle_struct)
        .expect("Failed to extract struct");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];

    if let EntityVariant::Rust(RustEntityVariant::Struct {
        fields, is_tuple, ..
    }) = &entity.variant
    {
        assert!(!is_tuple);
        assert_eq!(fields.len(), 4);

        assert_eq!(fields[0].name, "id");
        assert_eq!(fields[0].field_type, "u64");

        assert_eq!(fields[1].name, "name");
        assert_eq!(fields[1].field_type, "String");

        assert_eq!(fields[2].name, "email");
        assert_eq!(fields[2].field_type, "String");

        assert_eq!(fields[3].name, "is_active");
        assert_eq!(fields[3].field_type, "bool");
    } else {
        panic!("Expected struct variant");
    }
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

    let entities = extract_with_handler(source, queries::STRUCT_QUERY, handle_struct)
        .expect("Failed to extract struct");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];

    if let EntityVariant::Rust(RustEntityVariant::Struct {
        generics, fields, ..
    }) = &entity.variant
    {
        assert_eq!(generics.len(), 2);
        assert!(generics.contains(&"T".to_string()));
        assert!(generics.contains(&"U".to_string()));
        assert_eq!(fields.len(), 3);
    } else {
        panic!("Expected struct variant");
    }
}

#[test]
fn test_struct_with_derives() {
    let source = r#"
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DerivedStruct {
    value: i32,
}
"#;

    let entities = extract_with_handler(source, queries::STRUCT_QUERY, handle_struct)
        .expect("Failed to extract struct");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];

    if let EntityVariant::Rust(RustEntityVariant::Struct { derives, .. }) = &entity.variant {
        assert!(derives.contains(&"Debug".to_string()));
        assert!(derives.contains(&"Clone".to_string()));
        assert!(derives.contains(&"PartialEq".to_string()));
        assert!(derives.contains(&"Serialize".to_string()));
        assert!(derives.contains(&"Deserialize".to_string()));
    } else {
        panic!("Expected struct variant");
    }
}

#[test]
fn test_struct_with_lifetime_parameters() {
    let source = r#"
struct Reference<'a, 'b: 'a> {
    first: &'a str,
    second: &'b str,
}
"#;

    let entities = extract_with_handler(source, queries::STRUCT_QUERY, handle_struct)
        .expect("Failed to extract struct");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];

    if let EntityVariant::Rust(RustEntityVariant::Struct {
        generics, fields, ..
    }) = &entity.variant
    {
        assert_eq!(generics.len(), 2);
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0].field_type, "&'a str");
        assert_eq!(fields[1].field_type, "&'b str");
    } else {
        panic!("Expected struct variant");
    }
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

    let entities = extract_with_handler(source, queries::STRUCT_QUERY, handle_struct)
        .expect("Failed to extract struct");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];

    assert!(entity.documentation.is_some());
    let doc = entity.documentation.as_ref().unwrap();
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

    let entities = extract_with_handler(source, queries::STRUCT_QUERY, handle_struct)
        .expect("Failed to extract struct");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];

    if let EntityVariant::Rust(RustEntityVariant::Struct { fields, .. }) = &entity.variant {
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0].field_type, "Vec<Option<Box<T>>>");
        assert_eq!(fields[1].field_type, "HashMap<String, T>");
    } else {
        panic!("Expected struct variant");
    }
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

    let entities = extract_with_handler(source, queries::STRUCT_QUERY, handle_struct)
        .expect("Failed to extract struct");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];

    use codesearch_core::entities::Visibility;
    assert_eq!(entity.visibility, Visibility::Public);

    if let EntityVariant::Rust(RustEntityVariant::Struct { fields, .. }) = &entity.variant {
        assert_eq!(fields.len(), 3);
        // Field visibility is tracked in the FieldInfo
        assert_eq!(fields[0].visibility, Visibility::Public);
        assert_eq!(fields[1].visibility, Visibility::Private);
        assert_eq!(fields[2].visibility, Visibility::Public); // pub(crate) is still public
    } else {
        panic!("Expected struct variant");
    }
}
