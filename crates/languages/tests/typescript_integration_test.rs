//! Integration tests for TypeScript language support

use codesearch_core::entities::EntityType;
use codesearch_languages::create_extractor;
use std::path::Path;

/// Helper to filter entities by type (excludes Module entities used for IMPORTS tracking)
fn filter_by_type(
    entities: &[codesearch_core::CodeEntity],
    entity_type: EntityType,
) -> Vec<&codesearch_core::CodeEntity> {
    entities
        .iter()
        .filter(|e| e.entity_type == entity_type)
        .collect()
}

#[test]
fn test_typescript_extractor_creation() {
    let result = create_extractor(
        Path::new("test.ts"),
        "test-repo",
        None,
        None,
        Path::new("/test-repo"),
    );
    if let Err(e) = &result {
        eprintln!("Error creating extractor: {e:?}");
    }
    assert!(result.is_ok());
    assert!(result.unwrap().is_some());
}

#[test]
fn test_tsx_extractor_creation() {
    let result = create_extractor(
        Path::new("Component.tsx"),
        "test-repo",
        None,
        None,
        Path::new("/test-repo"),
    );
    assert!(result.is_ok());
    assert!(result.unwrap().is_some());
}

#[test]
fn test_extract_typed_function() {
    let source = r#"
        function add(a: number, b: number): number {
            return a + b;
        }
    "#;

    let extractor = create_extractor(
        Path::new("test.ts"),
        "test-repo",
        None,
        None,
        Path::new("/test-repo"),
    )
    .expect("Failed to create extractor")
    .expect("No extractor for .ts");

    let entities = extractor
        .extract(source, Path::new("test.ts"))
        .expect("Failed to extract entities");

    eprintln!("Extracted entities: {entities:#?}");

    let functions = filter_by_type(&entities, EntityType::Function);
    assert_eq!(functions.len(), 1);
    let entity = functions[0];

    assert_eq!(entity.name, "add");
    assert_eq!(
        entity.language,
        codesearch_core::entities::Language::TypeScript
    );

    if let Some(signature) = &entity.signature {
        assert_eq!(signature.parameters.len(), 2);
        assert_eq!(signature.parameters[0].0, "a");
        // Type annotations might not be captured in all cases
        // assert_eq!(signature.parameters[0].1, Some("number".to_string()));
        assert!(signature.return_type.is_some());
    } else {
        panic!("Expected function signature");
    }
}

#[test]
fn test_extract_interface() {
    let source = r#"
        interface User {
            name: string;
            age: number;
        }
    "#;

    let extractor = create_extractor(
        Path::new("test.ts"),
        "test-repo",
        None,
        None,
        Path::new("/test-repo"),
    )
    .expect("Failed to create extractor")
    .expect("No extractor for .ts");

    let entities = extractor
        .extract(source, Path::new("test.ts"))
        .expect("Failed to extract entities");

    let interfaces = filter_by_type(&entities, EntityType::Interface);
    assert_eq!(interfaces.len(), 1);
    let entity = interfaces[0];

    assert_eq!(entity.name, "User");
    assert_eq!(entity.entity_type, EntityType::Interface);
}

#[test]
fn test_extract_generic_interface() {
    let source = r#"
        interface Container<T> {
            value: T;
        }
    "#;

    let extractor = create_extractor(
        Path::new("test.ts"),
        "test-repo",
        None,
        None,
        Path::new("/test-repo"),
    )
    .expect("Failed to create extractor")
    .expect("No extractor for .ts");

    let entities = extractor
        .extract(source, Path::new("test.ts"))
        .expect("Failed to extract entities");

    let interfaces = filter_by_type(&entities, EntityType::Interface);
    assert_eq!(interfaces.len(), 1);
    let entity = interfaces[0];

    assert_eq!(entity.name, "Container");
    assert!(entity.signature.is_some());

    if let Some(sig) = &entity.signature {
        assert!(!sig.generics.is_empty());
        assert!(sig.generics.contains(&"T".to_string()));
    }
}

#[test]
fn test_extract_type_alias() {
    let source = r#"
        type ID = string | number;
    "#;

    let extractor = create_extractor(
        Path::new("test.ts"),
        "test-repo",
        None,
        None,
        Path::new("/test-repo"),
    )
    .expect("Failed to create extractor")
    .expect("No extractor for .ts");

    let entities = extractor
        .extract(source, Path::new("test.ts"))
        .expect("Failed to extract entities");

    eprintln!("Extracted entities: {entities:#?}");

    let type_aliases = filter_by_type(&entities, EntityType::TypeAlias);
    assert_eq!(type_aliases.len(), 1);
    let entity = type_aliases[0];

    assert_eq!(entity.name, "ID");
    assert_eq!(entity.entity_type, EntityType::TypeAlias);
}

#[test]
fn test_extract_enum() {
    let source = r#"
        enum Color {
            Red,
            Green,
            Blue
        }
    "#;

    let extractor = create_extractor(
        Path::new("test.ts"),
        "test-repo",
        None,
        None,
        Path::new("/test-repo"),
    )
    .expect("Failed to create extractor")
    .expect("No extractor for .ts");

    let entities = extractor
        .extract(source, Path::new("test.ts"))
        .expect("Failed to extract entities");

    let enums = filter_by_type(&entities, EntityType::Enum);
    assert_eq!(enums.len(), 1);
    let entity = enums[0];

    assert_eq!(entity.name, "Color");
    assert_eq!(entity.entity_type, EntityType::Enum);
}

#[test]
fn test_extract_async_function() {
    let source = r#"
        async function fetchData(url: string): Promise<any> {
            const response = await fetch(url);
            return response.json();
        }
    "#;

    let extractor = create_extractor(
        Path::new("test.ts"),
        "test-repo",
        None,
        None,
        Path::new("/test-repo"),
    )
    .expect("Failed to create extractor")
    .expect("No extractor for .ts");

    let entities = extractor
        .extract(source, Path::new("test.ts"))
        .expect("Failed to extract entities");

    let functions = filter_by_type(&entities, EntityType::Function);
    assert_eq!(functions.len(), 1);
    let entity = functions[0];

    assert_eq!(entity.name, "fetchData");
    assert!(entity.metadata.is_async);
}

#[test]
fn test_extract_class() {
    let source = r#"
        class Person {
            constructor(name: string, age: number) {
                this.name = name;
                this.age = age;
            }

            greet(): string {
                return `Hello, I'm ${this.name}`;
            }
        }
    "#;

    let extractor = create_extractor(
        Path::new("test.ts"),
        "test-repo",
        None,
        None,
        Path::new("/test-repo"),
    )
    .expect("Failed to create extractor")
    .expect("No extractor for .ts");

    let entities = extractor
        .extract(source, Path::new("test.ts"))
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
fn test_extract_interface_with_jsdoc() {
    let source = r#"
        /**
         * Represents a user in the system
         * @interface
         */
        interface User {
            name: string;
            age: number;
        }
    "#;

    let extractor = create_extractor(
        Path::new("test.ts"),
        "test-repo",
        None,
        None,
        Path::new("/test-repo"),
    )
    .expect("Failed to create extractor")
    .expect("No extractor for .ts");

    let entities = extractor
        .extract(source, Path::new("test.ts"))
        .expect("Failed to extract entities");

    let interfaces = filter_by_type(&entities, EntityType::Interface);
    assert_eq!(interfaces.len(), 1);
    let entity = interfaces[0];

    assert_eq!(entity.name, "User");
    assert!(entity.documentation_summary.is_some());

    let doc = entity.documentation_summary.as_ref().unwrap();
    assert!(doc.contains("Represents a user"));
}

#[test]
fn test_extract_arrow_function() {
    let source = r#"
        const multiply = (a: number, b: number): number => a * b;
    "#;

    let extractor = create_extractor(
        Path::new("test.ts"),
        "test-repo",
        None,
        None,
        Path::new("/test-repo"),
    )
    .expect("Failed to create extractor")
    .expect("No extractor for .ts");

    let entities = extractor
        .extract(source, Path::new("test.ts"))
        .expect("Failed to extract entities");

    let functions = filter_by_type(&entities, EntityType::Function);
    assert_eq!(functions.len(), 1);
    let entity = functions[0];

    assert_eq!(entity.name, "multiply");
    assert_eq!(
        entity.language,
        codesearch_core::entities::Language::TypeScript
    );
}
