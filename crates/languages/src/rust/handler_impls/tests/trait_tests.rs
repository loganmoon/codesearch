//! Tests for trait extraction handler

use super::*;
use crate::rust::handler_impls::type_handlers::handle_trait_impl;
use codesearch_core::entities::EntityType;

/// Helper to find the trait entity from extracted entities
fn find_trait_entity(entities: &[codesearch_core::CodeEntity]) -> &codesearch_core::CodeEntity {
    entities
        .iter()
        .find(|e| e.entity_type == EntityType::Trait)
        .expect("Should have a Trait entity")
}

/// Helper to count method entities
fn count_method_entities(entities: &[codesearch_core::CodeEntity]) -> usize {
    entities
        .iter()
        .filter(|e| e.entity_type == EntityType::Method)
        .count()
}

#[test]
fn test_simple_trait() {
    let source = r#"
trait SimpleTrait {
    fn method(&self);
}
"#;

    let entities = extract_with_handler(source, queries::TRAIT_QUERY, handle_trait_impl)
        .expect("Failed to extract trait");

    // Trait + 1 method entity
    assert_eq!(entities.len(), 2);
    assert_eq!(count_method_entities(&entities), 1);

    let entity = find_trait_entity(&entities);
    assert_eq!(entity.name, "SimpleTrait");
    assert_eq!(entity.entity_type, EntityType::Trait);

    // Check methods
    let methods = entity.metadata.attributes.get("methods");
    assert!(methods.is_some());
    let methods_str = methods.unwrap();
    assert!(methods_str.contains("method"));
}

#[test]
fn test_trait_with_generics() {
    let source = r#"
trait Container<T> {
    fn get(&self) -> &T;
    fn set(&mut self, value: T);
}
"#;

    let entities = extract_with_handler(source, queries::TRAIT_QUERY, handle_trait_impl)
        .expect("Failed to extract trait");

    // Trait + 2 method entities
    assert_eq!(entities.len(), 3);
    assert_eq!(count_method_entities(&entities), 2);

    let entity = find_trait_entity(&entities);
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

    let entities = extract_with_handler(source, queries::TRAIT_QUERY, handle_trait_impl)
        .expect("Failed to extract trait");

    // Trait + 1 method entity
    assert_eq!(entities.len(), 2);
    assert_eq!(count_method_entities(&entities), 1);

    let entity = find_trait_entity(&entities);
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

    let entities = extract_with_handler(source, queries::TRAIT_QUERY, handle_trait_impl)
        .expect("Failed to extract trait");

    // Trait + 3 method entities
    assert_eq!(entities.len(), 4);
    assert_eq!(count_method_entities(&entities), 3);

    let entity = find_trait_entity(&entities);
    assert_eq!(entity.name, "DefaultMethods");
    assert_eq!(entity.entity_type, EntityType::Trait);

    // Check methods (both required and with default impl)
    let methods = entity.metadata.attributes.get("methods");
    assert!(methods.is_some());
    let methods_str = methods.unwrap();
    assert!(methods_str.contains("required"));
    assert!(methods_str.contains("with_default"));
    assert!(methods_str.contains("another_default"));

    // Verify abstract vs non-abstract method entities
    let method_entities: Vec<_> = entities
        .iter()
        .filter(|e| e.entity_type == EntityType::Method)
        .collect();

    // required has no body -> is_abstract = true
    let required = method_entities
        .iter()
        .find(|e| e.name == "required")
        .expect("Should have required method");
    assert!(
        required.metadata.is_abstract,
        "required should be abstract (no body)"
    );

    // with_default has a body -> is_abstract = false
    let with_default = method_entities
        .iter()
        .find(|e| e.name == "with_default")
        .expect("Should have with_default method");
    assert!(
        !with_default.metadata.is_abstract,
        "with_default should not be abstract (has body)"
    );
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

    let entities = extract_with_handler(source, queries::TRAIT_QUERY, handle_trait_impl)
        .expect("Failed to extract trait");

    // 2 traits + 2 method entities total
    let trait_count = entities
        .iter()
        .filter(|e| e.entity_type == EntityType::Trait)
        .count();
    assert_eq!(trait_count, 2);

    let method_count = count_method_entities(&entities);
    assert_eq!(method_count, 2);

    // Check first trait (Display)
    let display_trait = entities
        .iter()
        .find(|e| e.entity_type == EntityType::Trait && e.name == "Display")
        .expect("Should have Display trait");
    assert_eq!(display_trait.name, "Display");

    // Check bounds (supertraits)
    let bounds = display_trait.metadata.attributes.get("bounds");
    assert!(bounds.is_some());
    let bounds_str = bounds.unwrap();
    assert!(bounds_str.contains("Debug"));
    assert!(bounds_str.contains("Clone"));

    // Check methods
    let methods = display_trait.metadata.attributes.get("methods");
    assert!(methods.is_some());
    let methods_str = methods.unwrap();
    assert!(methods_str.contains("fmt"));

    // Check second trait (Complex)
    let complex_trait = entities
        .iter()
        .find(|e| e.entity_type == EntityType::Trait && e.name == "Complex")
        .expect("Should have Complex trait");
    assert_eq!(complex_trait.name, "Complex");

    // Check bounds (supertraits)
    let bounds = complex_trait.metadata.attributes.get("bounds");
    assert!(bounds.is_some());
    let bounds_str = bounds.unwrap();
    assert!(bounds_str.contains("Display"));
    assert!(bounds_str.contains("Send"));
    assert!(bounds_str.contains("Sync"));

    // Check methods
    let methods = complex_trait.metadata.attributes.get("methods");
    assert!(methods.is_some());
    let methods_str = methods.unwrap();
    assert!(methods_str.contains("process"));
}

#[test]
fn test_unsafe_trait() {
    let source = r#"
unsafe trait UnsafeMarker {
    fn unsafe_method(&self);
}
"#;

    let entities = extract_with_handler(source, queries::TRAIT_QUERY, handle_trait_impl)
        .expect("Failed to extract trait");

    // Trait + 1 method entity
    assert_eq!(entities.len(), 2);
    assert_eq!(count_method_entities(&entities), 1);

    let entity = find_trait_entity(&entities);
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
use std::clone::Clone;
use std::fmt::Debug;

trait ComplexBounds<T>
where
    T: Clone + Debug,
    Self: Sized,
{
    fn process(value: T) -> Self;
}
"#;

    let entities = extract_with_handler(source, queries::TRAIT_QUERY, handle_trait_impl)
        .expect("Failed to extract trait");

    // Trait + 1 method entity
    assert_eq!(entities.len(), 2);
    assert_eq!(count_method_entities(&entities), 1);

    let entity = find_trait_entity(&entities);
    assert_eq!(entity.name, "ComplexBounds");
    assert_eq!(entity.entity_type, EntityType::Trait);

    // Check generics
    assert!(entity.metadata.is_generic);
    assert_eq!(entity.metadata.generic_params.len(), 1);

    // Check generic_bounds includes where clause bounds
    let bounds = &entity.metadata.generic_bounds;
    assert!(bounds.contains_key("T"), "Should have bounds for T");
    let t_bounds = bounds.get("T").unwrap();
    assert!(
        t_bounds.iter().any(|b| b.contains("Clone")),
        "T should have Clone bound from where clause"
    );
    assert!(
        t_bounds.iter().any(|b| b.contains("Debug")),
        "T should have Debug bound from where clause"
    );

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

    let entities = extract_with_handler(source, queries::TRAIT_QUERY, handle_trait_impl)
        .expect("Failed to extract trait");

    // Trait + 1 method entity
    assert_eq!(entities.len(), 2);
    assert_eq!(count_method_entities(&entities), 1);

    let entity = find_trait_entity(&entities);

    assert!(entity.documentation_summary.is_some());
    let doc = entity.documentation_summary.as_ref().unwrap();
    assert!(doc.contains("can be serialized"));
    assert!(doc.contains("enable serialization"));
}

// ============================================================================
// Generic Bounds Extraction Tests
// ============================================================================

#[test]
fn test_trait_with_generic_bounds() {
    let source = r#"
use std::clone::Clone;
use std::marker::Send;

trait Container<T: Clone + Send, U> {
    fn get(&self) -> &T;
    fn other(&self) -> U;
}
"#;

    let entities = extract_with_handler(source, queries::TRAIT_QUERY, handle_trait_impl)
        .expect("Failed to extract trait");

    // Trait + 2 method entities
    assert_eq!(entities.len(), 3);
    assert_eq!(count_method_entities(&entities), 2);

    let entity = find_trait_entity(&entities);
    assert_eq!(entity.name, "Container");

    // Check generic_params (backward-compat raw strings)
    assert!(entity.metadata.is_generic);
    assert_eq!(entity.metadata.generic_params.len(), 2);

    // Check generic_bounds (structured)
    let bounds = &entity.metadata.generic_bounds;
    assert!(bounds.contains_key("T"), "Should have bounds for T");
    let t_bounds = bounds.get("T").unwrap();
    assert!(
        t_bounds.iter().any(|b| b.contains("Clone")),
        "T should have Clone bound"
    );
    assert!(
        t_bounds.iter().any(|b| b.contains("Send")),
        "T should have Send bound"
    );
    // U has no bounds, so should not be in generic_bounds
    assert!(!bounds.contains_key("U"));

    // Check uses_types includes bound traits
    let uses_types_json = entity.metadata.attributes.get("uses_types");
    assert!(uses_types_json.is_some(), "Should have uses_types");
    let uses_types: Vec<String> =
        serde_json::from_str(uses_types_json.unwrap()).expect("Valid JSON");
    assert!(
        uses_types.iter().any(|t| t.contains("Clone")),
        "uses_types should include Clone"
    );
    assert!(
        uses_types.iter().any(|t| t.contains("Send")),
        "uses_types should include Send"
    );
}
