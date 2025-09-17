//! Tests for trait extraction handler

use super::*;
use crate::rust::entities::RustEntityVariant;
use crate::rust::handlers::type_handlers::handle_trait;
use crate::transport::EntityVariant;

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
    assert_eq!(entity.name, "SimpleTrait");

    if let EntityVariant::Rust(RustEntityVariant::Trait { methods, .. }) = &entity.variant {
        assert_eq!(methods.len(), 1);
        assert_eq!(methods[0], "method");
    } else {
        panic!("Expected trait variant");
    }
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

    if let EntityVariant::Rust(RustEntityVariant::Trait {
        generics, methods, ..
    }) = &entity.variant
    {
        assert_eq!(generics.len(), 1);
        assert!(generics.contains(&"T".to_string()));
        assert_eq!(methods.len(), 2);
        assert_eq!(methods[0], "get");
        assert_eq!(methods[1], "set");
    } else {
        panic!("Expected trait variant");
    }
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

    if let EntityVariant::Rust(RustEntityVariant::Trait {
        associated_types,
        methods,
        ..
    }) = &entity.variant
    {
        assert_eq!(associated_types.len(), 2);
        assert!(associated_types.contains(&"Item".to_string()));
        assert!(associated_types.contains(&"IntoIter".to_string()));
        assert_eq!(methods.len(), 1);
        assert_eq!(methods[0], "next");
    } else {
        panic!("Expected trait variant");
    }
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

    if let EntityVariant::Rust(RustEntityVariant::Trait { methods, .. }) = &entity.variant {
        assert_eq!(methods.len(), 3);
        assert!(methods.contains(&"required".to_string()));
        assert!(methods.contains(&"with_default".to_string()));
        assert!(methods.contains(&"another_default".to_string()));
    } else {
        panic!("Expected trait variant");
    }
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
    assert_eq!(entity.name, "Display");
    if let EntityVariant::Rust(RustEntityVariant::Trait { bounds, .. }) = &entity.variant {
        assert!(bounds.contains(&"Debug".to_string()));
        assert!(bounds.contains(&"Clone".to_string()));
    } else {
        panic!("Expected trait variant");
    }

    // Check second trait
    let entity = &entities[1];
    assert_eq!(entity.name, "Complex");
    if let EntityVariant::Rust(RustEntityVariant::Trait { bounds, .. }) = &entity.variant {
        assert!(bounds.contains(&"Display".to_string()));
        assert!(bounds.contains(&"Send".to_string()));
        assert!(bounds.contains(&"Sync".to_string()));
    } else {
        panic!("Expected trait variant");
    }
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

    if let EntityVariant::Rust(RustEntityVariant::Trait { methods, .. }) = &entity.variant {
        // Note: is_unsafe is not tracked in the Trait variant currently
        assert_eq!(methods.len(), 1);
    } else {
        panic!("Expected trait variant");
    }
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

    if let EntityVariant::Rust(RustEntityVariant::Trait {
        generics, methods, ..
    }) = &entity.variant
    {
        assert_eq!(generics.len(), 1);
        assert_eq!(methods.len(), 1);
        assert_eq!(methods[0], "process");
    } else {
        panic!("Expected trait variant");
    }
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

    assert!(entity.documentation.is_some());
    let doc = entity.documentation.as_ref().unwrap();
    assert!(doc.contains("can be serialized"));
    assert!(doc.contains("enable serialization"));
}
