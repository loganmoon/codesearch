//! Test for verifying Rust extractor works correctly

use codesearch_core::entities::{EntityType, Visibility};
use codesearch_languages::create_extractor;
use std::path::Path;

#[test]
fn test_rust_extractor_creates_and_extracts() {
    let extractor = create_extractor(
        Path::new("/tmp/test.rs"),
        "test-repo",
        None,
        None,
        Path::new("/tmp"),
    )
    .expect("Should not error")
    .expect("Should have Rust extractor");

    let source = r#"
fn test_function() -> i32 {
    42
}

pub struct TestStruct {
    field: i32,
}
"#;

    let entities = extractor
        .extract(source, Path::new("/tmp/test.rs"))
        .expect("Should extract entities");

    println!("Extracted {} entities:", entities.len());
    for e in &entities {
        println!("  - {} ({:?})", e.qualified_name, e.entity_type);
    }

    // Should extract at least the function and struct
    assert!(
        entities.len() >= 2,
        "Expected at least 2 entities (function + struct), got {}",
        entities.len()
    );
}

#[test]
fn test_rust_extractor_macro_visibility() {
    let extractor = create_extractor(
        Path::new("/tmp/lib.rs"),
        "test-repo",
        Some("test_crate"),
        Some(Path::new("/tmp")),
        Path::new("/tmp"),
    )
    .expect("Should not error")
    .expect("Should have Rust extractor");

    // This is the exact same content as the e2e fixture
    let source = r#"
#[macro_export]
macro_rules! my_macro {
    () => {};
    ($x:expr) => { $x };
}

macro_rules! private_macro {
    () => {};
}
"#;

    let entities = extractor
        .extract(source, Path::new("/tmp/lib.rs"))
        .expect("Should extract entities");

    println!("Extracted {} entities:", entities.len());
    for e in &entities {
        println!(
            "  - {} ({:?}, vis={:?})",
            e.qualified_name, e.entity_type, e.visibility
        );
    }

    // Find the macros
    let my_macro = entities.iter().find(|e| e.name == "my_macro");
    let private_macro = entities.iter().find(|e| e.name == "private_macro");

    assert!(my_macro.is_some(), "Should find my_macro");
    assert!(private_macro.is_some(), "Should find private_macro");

    let my_macro = my_macro.unwrap();
    let private_macro = private_macro.unwrap();

    // Check visibility
    assert_eq!(
        my_macro.visibility,
        Some(Visibility::Public),
        "my_macro (with #[macro_export]) should be Public, got {:?}",
        my_macro.visibility
    );
    assert_eq!(
        private_macro.visibility,
        Some(Visibility::Private),
        "private_macro (without #[macro_export]) should be Private, got {:?}",
        private_macro.visibility
    );
}

/// Test that struct fields and methods with the same name get distinct entity_ids.
///
/// This verifies the fix for the bug where Property and Method entities with the
/// same qualified name (e.g., `ConfigBuilder::name` field vs `ConfigBuilder::name` method)
/// would get the same entity_id, causing deduplication to drop one of them.
#[test]
fn test_property_and_method_same_name_distinct_entity_ids() {
    let extractor = create_extractor(
        Path::new("/tmp/lib.rs"),
        "test-repo",
        Some("test_crate"),
        Some(Path::new("/tmp")),
        Path::new("/tmp"),
    )
    .expect("Should not error")
    .expect("Should have Rust extractor");

    // Builder pattern: struct fields have same names as builder methods
    let source = r#"
pub struct ConfigBuilder {
    name: Option<String>,
    value: Option<i32>,
}

impl ConfigBuilder {
    pub fn name(mut self, name: &str) -> Self {
        self.name = Some(name.to_string());
        self
    }

    pub fn value(mut self, value: i32) -> Self {
        self.value = Some(value);
        self
    }
}
"#;

    let entities = extractor
        .extract(source, Path::new("/tmp/lib.rs"))
        .expect("Should extract entities");

    println!("Extracted {} entities:", entities.len());
    for e in &entities {
        println!(
            "  - {} ({:?}, id={})",
            e.qualified_name, e.entity_type, e.entity_id
        );
    }

    // Find Property entities with name "name" and "value"
    let name_property = entities
        .iter()
        .find(|e| e.name == "name" && e.entity_type == EntityType::Property);
    let value_property = entities
        .iter()
        .find(|e| e.name == "value" && e.entity_type == EntityType::Property);

    // Find Method entities with name "name" and "value"
    let name_method = entities
        .iter()
        .find(|e| e.name == "name" && e.entity_type == EntityType::Method);
    let value_method = entities
        .iter()
        .find(|e| e.name == "value" && e.entity_type == EntityType::Method);

    // All four should exist
    assert!(
        name_property.is_some(),
        "Should find Property entity for 'name' field"
    );
    assert!(
        value_property.is_some(),
        "Should find Property entity for 'value' field"
    );
    assert!(
        name_method.is_some(),
        "Should find Method entity for 'name' method"
    );
    assert!(
        value_method.is_some(),
        "Should find Method entity for 'value' method"
    );

    let name_property = name_property.unwrap();
    let value_property = value_property.unwrap();
    let name_method = name_method.unwrap();
    let value_method = value_method.unwrap();

    // Property and Method with same name should have DIFFERENT entity_ids
    assert_ne!(
        name_property.entity_id, name_method.entity_id,
        "Property 'name' and Method 'name' should have different entity_ids"
    );
    assert_ne!(
        value_property.entity_id, value_method.entity_id,
        "Property 'value' and Method 'value' should have different entity_ids"
    );

    // Verify qualified names are the same (that's the point - same qualified name, different types)
    assert_eq!(
        name_property.qualified_name, name_method.qualified_name,
        "Property and Method 'name' should have same qualified_name"
    );
    assert_eq!(
        value_property.qualified_name, value_method.qualified_name,
        "Property and Method 'value' should have same qualified_name"
    );
}
