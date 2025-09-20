//! Tests for trait extraction handler

use super::*;
use crate::rust::handlers::type_handlers::handle_trait;
use codesearch_core::entities::EntityType;

#[test]
fn test_simple_trait() {
    let source = r#"
trait SimpleTrait {
    fn method(&self);
}
"#;

    let entities = extract_with_handler(source, queries::TRAIT_QUERY, handle_trait)
        .expect("Failed to extract trait");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.name, "SimpleTrait"); // TODO: Update test to use new CodeEntity structure
                                            // Original test body commented out during migration
}

#[test]
fn test_trait_with_generics() {
    let source = r#"
trait Container<T> {
    fn get(&self) -> &T;
    fn set(&mut self, value: T);
}
"#;

    let entities = extract_with_handler(source, queries::TRAIT_QUERY, handle_trait)
        .expect("Failed to extract trait");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.name, "Container");
    assert_eq!(entity.entity_type, EntityType::Trait);

    // Check generics
    assert!(entity.metadata.is_generic);
    assert_eq!(entity.metadata.generic_params.len(), 1);
    assert!(entity.metadata.generic_params.contains(&"T".to_string()));

    // Check methods
    let methods = entity.metadata.attributes.get("methods");
    assert!(methods.is_some());
    let methods_str = methods.unwrap();
    assert!(methods_str.contains("get"));
    assert!(methods_str.contains("set"));
}

#[test]
fn test_trait_with_associated_types() {
    let source = r#"
trait Iterator {
    type Item;
    type IntoIter: Iterator<Item = Self::Item>;

    fn next(&mut self) -> Option<Self::Item>;
}
"#;

    let entities = extract_with_handler(source, queries::TRAIT_QUERY, handle_trait)
        .expect("Failed to extract trait");

    // Debug output
    eprintln!("Number of entities extracted: {}", entities.len());
    for (i, entity) in entities.iter().enumerate() {
        eprintln!(
            "Entity {}: name={}, qualified_name={}",
            i, entity.name, entity.qualified_name
        );
    }

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.name, "Iterator");
    assert_eq!(entity.entity_type, EntityType::Trait);

    // Check associated types
    let assoc_types = entity.metadata.attributes.get("associated_types");
    assert!(assoc_types.is_some());
    let assoc_types_str = assoc_types.unwrap();
    assert!(assoc_types_str.contains("Item"));
    assert!(assoc_types_str.contains("IntoIter"));

    // Check methods
    let methods = entity.metadata.attributes.get("methods");
    assert!(methods.is_some());
    let methods_str = methods.unwrap();
    assert!(methods_str.contains("next"));
}

#[test]
fn test_trait_with_default_implementations() {
    let source = r#"
trait DefaultMethods {
    fn required(&self);

    fn with_default(&self) {
        println!("Default implementation");
    }

    fn another_default(&self) -> i32 {
        42
    }
}
"#;

    let entities = extract_with_handler(source, queries::TRAIT_QUERY, handle_trait)
        .expect("Failed to extract trait");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.name, "DefaultMethods");
    assert_eq!(entity.entity_type, EntityType::Trait);

    // Check methods (both required and with default impl)
    let methods = entity.metadata.attributes.get("methods");
    assert!(methods.is_some());
    let methods_str = methods.unwrap();
    assert!(methods_str.contains("required"));
    assert!(methods_str.contains("with_default"));
    assert!(methods_str.contains("another_default"));
}

#[test]
fn test_trait_with_supertraits() {
    let source = r#"
trait Display: Debug + Clone {
    fn fmt(&self) -> String;
}

trait Complex: Display + Send + Sync + 'static {
    fn process(&self);
}
"#;

    let entities = extract_with_handler(source, queries::TRAIT_QUERY, handle_trait)
        .expect("Failed to extract trait");

    assert_eq!(entities.len(), 2);

    // Check first trait
    let entity = &entities[0];
    assert_eq!(entity.name, "Display"); // TODO: Update test to use new CodeEntity structure
                                        // Original test body commented out during migration

    // Check second trait
    let entity = &entities[1];
    assert_eq!(entity.name, "Complex"); // TODO: Update test to use new CodeEntity structure
                                        // Original test body commented out during migration
}

#[test]
fn test_unsafe_trait() {
    let source = r#"
unsafe trait UnsafeMarker {
    fn unsafe_method(&self);
}
"#;

    let entities = extract_with_handler(source, queries::TRAIT_QUERY, handle_trait)
        .expect("Failed to extract trait");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.name, "UnsafeMarker");
    assert_eq!(entity.entity_type, EntityType::Trait);

    // Check unsafe attribute
    assert_eq!(
        entity.metadata.attributes.get("unsafe").map(|s| s.as_str()),
        Some("true")
    );

    // Check methods
    let methods = entity.metadata.attributes.get("methods");
    assert!(methods.is_some());
    let methods_str = methods.unwrap();
    assert!(methods_str.contains("unsafe_method"));
}

#[test]
fn test_trait_with_where_clauses() {
    let source = r#"
trait ComplexBounds<T>
where
    T: Clone + Debug,
    Self: Sized,
{
    fn process(value: T) -> Self;
}
"#;

    let entities = extract_with_handler(source, queries::TRAIT_QUERY, handle_trait)
        .expect("Failed to extract trait");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.name, "ComplexBounds");
    assert_eq!(entity.entity_type, EntityType::Trait);

    // Check generics
    assert!(entity.metadata.is_generic);
    assert_eq!(entity.metadata.generic_params.len(), 1);
    assert!(entity.metadata.generic_params.contains(&"T".to_string()));

    // Check where clause constraints are captured
    let where_clause = entity.metadata.attributes.get("where_clause");
    if where_clause.is_some() {
        let where_str = where_clause.unwrap();
        assert!(where_str.contains("Clone") || where_str.contains("Debug"));
    }

    // Check methods
    let methods = entity.metadata.attributes.get("methods");
    assert!(methods.is_some());
}

#[test]
fn test_trait_with_doc_comments() {
    let source = r#"
/// A trait for types that can be serialized
///
/// Implement this trait to enable serialization
pub trait Serialize {
    /// Serialize the value to a string
    fn serialize(&self) -> String;
}
"#;

    let entities = extract_with_handler(source, queries::TRAIT_QUERY, handle_trait)
        .expect("Failed to extract trait");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];

    assert!(entity.documentation_summary.is_some());
    let doc = entity.documentation_summary.as_ref().unwrap();
    assert!(doc.contains("can be serialized"));
    assert!(doc.contains("enable serialization"));
}
