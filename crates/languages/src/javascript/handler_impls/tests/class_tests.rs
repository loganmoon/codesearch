//! Tests for JavaScript class extraction handlers

use super::*;
use crate::javascript::handler_impls::{handle_class_impl, handle_method_impl};
use codesearch_core::entities::EntityType;

#[test]
fn test_simple_class() {
    let source = r#"
class Person {
    constructor(name) {
        this.name = name;
    }
}
"#;

    let entities = extract_with_handler(source, queries::CLASS_QUERY, handle_class_impl)
        .expect("Failed to extract class");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.name, "Person");
    assert_eq!(entity.qualified_name, "Person");
    assert_eq!(entity.entity_type, EntityType::Class);
    assert!(entity.parent_scope.is_none());
}

#[test]
fn test_class_with_extends() {
    let source = r#"
class Employee extends Person {
    constructor(name, employeeId) {
        super(name);
        this.employeeId = employeeId;
    }
}
"#;

    let entities = extract_with_handler(source, queries::CLASS_QUERY, handle_class_impl)
        .expect("Failed to extract class");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.name, "Employee");
    assert_eq!(entity.entity_type, EntityType::Class);
    assert!(entity.metadata.attributes.contains_key("extends"));
}

#[test]
fn test_class_with_jsdoc() {
    let source = r#"
/**
 * Represents a user in the system.
 * @class
 */
class User {
    constructor(name) {
        this.name = name;
    }
}
"#;

    let entities = extract_with_handler(source, queries::CLASS_QUERY, handle_class_impl)
        .expect("Failed to extract class");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.name, "User");
    assert!(entity.documentation_summary.is_some());
    let doc = entity.documentation_summary.as_ref().unwrap();
    assert!(doc.contains("Represents a user"));
}

#[test]
fn test_class_qualified_name() {
    let source = r#"
class SimpleClass {
    constructor() {}
}
"#;

    let entities = extract_with_handler(source, queries::CLASS_QUERY, handle_class_impl)
        .expect("Failed to extract class");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.qualified_name, "SimpleClass");
    assert!(entity.parent_scope.is_none());
}

#[test]
fn test_simple_method() {
    let source = r#"
class Calculator {
    add(a, b) {
        return a + b;
    }
}
"#;

    let entities = extract_with_handler(source, queries::METHOD_QUERY, handle_method_impl)
        .expect("Failed to extract method");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.name, "add");
    assert_eq!(entity.entity_type, EntityType::Method);

    let sig = entity.signature.as_ref().expect("Should have signature");
    assert_eq!(sig.parameters.len(), 2);
}

#[test]
fn test_method_qualified_name_in_class() {
    let source = r#"
class Calculator {
    multiply(a, b) {
        return a * b;
    }
}
"#;

    let entities = extract_with_handler(source, queries::METHOD_QUERY, handle_method_impl)
        .expect("Failed to extract method");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.name, "multiply");
    assert_eq!(entity.qualified_name, "Calculator.multiply");
    assert_eq!(entity.parent_scope.as_deref(), Some("Calculator"));
}

#[test]
fn test_async_method() {
    let source = r#"
class DataService {
    async fetchData(url) {
        const response = await fetch(url);
        return response.json();
    }
}
"#;

    let entities = extract_with_handler(source, queries::METHOD_QUERY, handle_method_impl)
        .expect("Failed to extract method");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.name, "fetchData");
    assert!(entity.metadata.is_async);

    let sig = entity.signature.as_ref().expect("Should have signature");
    assert!(sig.is_async);
}

#[test]
fn test_static_method() {
    let source = r#"
class MathUtils {
    static square(x) {
        return x * x;
    }
}
"#;

    let entities = extract_with_handler(source, queries::METHOD_QUERY, handle_method_impl)
        .expect("Failed to extract method");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.name, "square");
    assert_eq!(
        entity.metadata.attributes.get("static").map(|s| s.as_str()),
        Some("true")
    );
}

#[test]
fn test_multiple_methods() {
    let source = r#"
class Calculator {
    add(a, b) {
        return a + b;
    }
    subtract(a, b) {
        return a - b;
    }
    multiply(a, b) {
        return a * b;
    }
}
"#;

    let entities = extract_with_handler(source, queries::METHOD_QUERY, handle_method_impl)
        .expect("Failed to extract methods");

    assert_eq!(entities.len(), 3);
    assert_eq!(entities[0].name, "add");
    assert_eq!(entities[1].name, "subtract");
    assert_eq!(entities[2].name, "multiply");

    for entity in &entities {
        assert_eq!(entity.parent_scope.as_deref(), Some("Calculator"));
        assert!(entity.qualified_name.starts_with("Calculator."));
    }
}

#[test]
fn test_method_with_jsdoc() {
    let source = r#"
class Calculator {
    /**
     * Adds two numbers together.
     * @param {number} a - First number
     * @param {number} b - Second number
     * @returns {number} The sum
     */
    add(a, b) {
        return a + b;
    }
}
"#;

    let entities = extract_with_handler(source, queries::METHOD_QUERY, handle_method_impl)
        .expect("Failed to extract method");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert!(entity.documentation_summary.is_some());
    let doc = entity.documentation_summary.as_ref().unwrap();
    assert!(doc.contains("Adds two numbers"));
}
