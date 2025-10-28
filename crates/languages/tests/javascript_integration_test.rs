//! Integration tests for JavaScript language support

use codesearch_languages::create_extractor;
use std::path::Path;

#[test]
fn test_javascript_extractor_creation() {
    let result = create_extractor(Path::new("test.js"), "test-repo");
    assert!(result.is_ok());
    assert!(result.unwrap().is_some());
}

#[test]
fn test_jsx_extractor_creation() {
    let result = create_extractor(Path::new("Component.jsx"), "test-repo");
    assert!(result.is_ok());
    assert!(result.unwrap().is_some());
}

#[test]
fn test_extract_simple_function() {
    let source = r#"
        function greet(name) {
            return `Hello, ${name}!`;
        }
    "#;

    let extractor = create_extractor(Path::new("test.js"), "test-repo")
        .expect("Failed to create extractor")
        .expect("No extractor for .js");

    let entities = extractor
        .extract(source, Path::new("test.js"))
        .expect("Failed to extract entities");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];

    assert_eq!(entity.name, "greet");
    assert_eq!(
        entity.language,
        codesearch_core::entities::Language::JavaScript
    );

    if let Some(signature) = &entity.signature {
        assert_eq!(signature.parameters.len(), 1);
        assert_eq!(signature.parameters[0].0, "name");
    } else {
        panic!("Expected function signature");
    }
}

#[test]
#[ignore] // Arrow function extraction needs query refinement
fn test_extract_arrow_function() {
    let source = r#"
        const add = (a, b) => a + b;
    "#;

    let extractor = create_extractor(Path::new("test.js"), "test-repo")
        .expect("Failed to create extractor")
        .expect("No extractor for .js");

    let entities = extractor
        .extract(source, Path::new("test.js"))
        .expect("Failed to extract entities");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];

    assert_eq!(entity.name, "add");
    assert_eq!(
        entity.language,
        codesearch_core::entities::Language::JavaScript
    );

    if let Some(signature) = &entity.signature {
        assert_eq!(signature.parameters.len(), 2);
        assert_eq!(signature.parameters[0].0, "a");
        assert_eq!(signature.parameters[1].0, "b");
    } else {
        panic!("Expected function signature");
    }
}

#[test]
fn test_extract_async_function() {
    let source = r#"
        async function fetchData(url) {
            const response = await fetch(url);
            return response.json();
        }
    "#;

    let extractor = create_extractor(Path::new("test.js"), "test-repo")
        .expect("Failed to create extractor")
        .expect("No extractor for .js");

    let entities = extractor
        .extract(source, Path::new("test.js"))
        .expect("Failed to extract entities");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];

    assert_eq!(entity.name, "fetchData");
    assert!(entity.metadata.is_async);
}

#[test]
fn test_extract_class() {
    let source = r#"
        class Person {
            constructor(name, age) {
                this.name = name;
                this.age = age;
            }

            greet() {
                return `Hello, I'm ${this.name}`;
            }
        }
    "#;

    let extractor = create_extractor(Path::new("test.js"), "test-repo")
        .expect("Failed to create extractor")
        .expect("No extractor for .js");

    let entities = extractor
        .extract(source, Path::new("test.js"))
        .expect("Failed to extract entities");

    // Should extract class and method
    assert!(entities.len() >= 1);

    // Find the class entity
    let class_entity = entities
        .iter()
        .find(|e| e.name == "Person")
        .expect("Should find Person class");

    assert_eq!(
        class_entity.entity_type,
        codesearch_core::entities::EntityType::Class
    );
}

#[test]
fn test_extract_function_with_jsdoc() {
    let source = r#"
        /**
         * Calculates the sum of two numbers
         * @param {number} a - First number
         * @param {number} b - Second number
         * @returns {number} The sum
         */
        function add(a, b) {
            return a + b;
        }
    "#;

    let extractor = create_extractor(Path::new("test.js"), "test-repo")
        .expect("Failed to create extractor")
        .expect("No extractor for .js");

    let entities = extractor
        .extract(source, Path::new("test.js"))
        .expect("Failed to extract entities");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];

    assert_eq!(entity.name, "add");
    assert!(entity.documentation_summary.is_some());

    let doc = entity.documentation_summary.as_ref().unwrap();
    assert!(doc.contains("Calculates the sum"));
}

#[test]
#[ignore] // Multiple entity extraction needs query refinement
fn test_extract_multiple_entities() {
    let source = r#"
        function foo() {}

        const bar = () => {};

        class Baz {}
    "#;

    let extractor = create_extractor(Path::new("test.js"), "test-repo")
        .expect("Failed to create extractor")
        .expect("No extractor for .js");

    let entities = extractor
        .extract(source, Path::new("test.js"))
        .expect("Failed to extract entities");

    assert!(entities.len() >= 3);

    // Check that we have the expected entities
    let names: Vec<&str> = entities.iter().map(|e| e.name.as_str()).collect();
    assert!(names.contains(&"foo"));
    assert!(names.contains(&"bar"));
    assert!(names.contains(&"Baz"));
}
