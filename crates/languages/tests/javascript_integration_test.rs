//! Integration tests for JavaScript language support

use codesearch_languages::create_extractor;
use std::path::Path;

#[test]
fn test_javascript_extractor_creation() {
    let result = create_extractor(Path::new("test.js"), "test-repo", None, None);
    assert!(result.is_ok());
    assert!(result.unwrap().is_some());
}

#[test]
fn test_jsx_extractor_creation() {
    let result = create_extractor(Path::new("Component.jsx"), "test-repo", None, None);
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

    let extractor = create_extractor(Path::new("test.js"), "test-repo", None, None)
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
fn test_extract_arrow_function() {
    let source = r#"
        const add = (a, b) => a + b;
    "#;

    let extractor = create_extractor(Path::new("test.js"), "test-repo", None, None)
        .expect("Failed to create extractor")
        .expect("No extractor for .js");

    let entities = extractor
        .extract(source, Path::new("test.js"))
        .expect("Failed to extract entities");

    eprintln!("Extracted {} entities", entities.len());
    for (i, entity) in entities.iter().enumerate() {
        eprintln!("Entity {}: {} ({})", i, entity.name, entity.entity_type);
    }

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

    let extractor = create_extractor(Path::new("test.js"), "test-repo", None, None)
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

    let extractor = create_extractor(Path::new("test.js"), "test-repo", None, None)
        .expect("Failed to create extractor")
        .expect("No extractor for .js");

    let entities = extractor
        .extract(source, Path::new("test.js"))
        .expect("Failed to extract entities");

    // Should extract class and method
    assert!(!entities.is_empty());

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

    let extractor = create_extractor(Path::new("test.js"), "test-repo", None, None)
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
fn test_extract_multiple_entities() {
    let source = r#"
        function foo() {}

        const bar = () => {};

        class Baz {}
    "#;

    let extractor = create_extractor(Path::new("test.js"), "test-repo", None, None)
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

#[test]
fn test_extract_plimit_structure() {
    // Test extraction of p-limit style code (export default function with nested arrows)
    let source = r#"
import Queue from 'yocto-queue';

export default function pLimit(concurrency) {
    const queue = new Queue();
    let activeCount = 0;

    const next = () => {
        activeCount--;
        if (queue.size > 0) {
            queue.dequeue()();
        }
    };

    const run = async (fn, resolve, args) => {
        activeCount++;
        const result = (async () => fn(...args))();
        resolve(result);
        try {
            await result;
        } catch {}
        next();
    };

    const enqueue = (fn, resolve, args) => {
        queue.enqueue(run.bind(undefined, fn, resolve, args));
    };

    const generator = (fn, ...args) => new Promise(resolve => {
        enqueue(fn, resolve, args);
    });

    return generator;
}
"#;

    let extractor = create_extractor(Path::new("index.js"), "test-repo", None, None)
        .expect("Failed to create extractor")
        .expect("No extractor for .js");

    let entities = extractor
        .extract(source, Path::new("index.js"))
        .expect("Failed to extract entities");

    // Print all extracted entities for debugging
    eprintln!(
        "\nExtracted {} entities from p-limit structure:",
        entities.len()
    );
    for entity in &entities {
        eprintln!(
            "  - {} ({:?}) parent_scope={:?}",
            entity.name, entity.entity_type, entity.parent_scope
        );
    }

    // Should extract pLimit function
    let plimit = entities.iter().find(|e| e.name == "pLimit");
    assert!(plimit.is_some(), "Should extract pLimit function");
    assert!(
        plimit.unwrap().parent_scope.is_none(),
        "pLimit should have no parent"
    );

    // Should extract nested arrow functions with parent_scope
    let next = entities.iter().find(|e| e.name == "next");
    assert!(next.is_some(), "Should extract 'next' arrow function");
    assert_eq!(
        next.unwrap().parent_scope.as_deref(),
        Some("pLimit"),
        "'next' should have pLimit as parent"
    );

    let run = entities.iter().find(|e| e.name == "run");
    assert!(run.is_some(), "Should extract 'run' arrow function");
    assert_eq!(
        run.unwrap().parent_scope.as_deref(),
        Some("pLimit"),
        "'run' should have pLimit as parent"
    );
}
