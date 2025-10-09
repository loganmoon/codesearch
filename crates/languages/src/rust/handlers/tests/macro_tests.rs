//! Tests for macro extraction handler

use super::*;
use crate::rust::handlers::macro_handlers::handle_macro;
use codesearch_core::entities::EntityType;

#[test]
fn test_simple_macro() {
    let source = r#"
macro_rules! simple {
    () => {
        println!("Hello");
    };
}
"#;

    let entities = extract_with_handler(source, queries::MACRO_QUERY, handle_macro)
        .expect("Failed to extract macro");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.name, "simple");
    assert_eq!(entity.entity_type, EntityType::Macro);

    // Check macro type
    let macro_type = entity
        .metadata
        .attributes
        .get("macro_type")
        .expect("Should have macro_type attribute");
    assert_eq!(macro_type, "declarative");

    // Not exported by default
    let exported = entity
        .metadata
        .attributes
        .get("exported")
        .expect("Should have exported attribute");
    assert_eq!(exported, "false");
}

#[test]
fn test_exported_macro() {
    let source = r#"
#[macro_export]
macro_rules! exported {
    () => {
        42
    };
}
"#;

    let entities = extract_with_handler(source, queries::MACRO_QUERY, handle_macro)
        .expect("Failed to extract macro");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.name, "exported");
    assert_eq!(entity.entity_type, EntityType::Macro);

    // Check it's exported
    let exported = entity
        .metadata
        .attributes
        .get("exported")
        .expect("Should have exported attribute");
    assert_eq!(exported, "true");
}

#[test]
fn test_macro_with_multiple_rules() {
    let source = r#"
macro_rules! with_arms {
    (a) => {
        println!("arm a");
    };
    (b) => {
        println!("arm b");
    };
    (c) => {
        println!("arm c");
    };
}
"#;

    let entities = extract_with_handler(source, queries::MACRO_QUERY, handle_macro)
        .expect("Failed to extract macro");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.name, "with_arms");
    assert_eq!(entity.entity_type, EntityType::Macro);

    // Content should include all arms
    assert!(entity.content.is_some());
    let content = entity.content.as_ref().unwrap();
    assert!(content.contains("arm a"));
    assert!(content.contains("arm b"));
    assert!(content.contains("arm c"));
}

#[test]
fn test_macro_with_doc_comments() {
    let source = r#"
/// Helper macro for creating messages
///
/// This macro simplifies message creation
#[macro_export]
macro_rules! message {
    (text $content:expr) => {
        Message::Text($content.to_string())
    };
}
"#;

    let entities = extract_with_handler(source, queries::MACRO_QUERY, handle_macro)
        .expect("Failed to extract macro");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.name, "message");
    assert_eq!(entity.entity_type, EntityType::Macro);

    // Check documentation
    assert!(entity.documentation_summary.is_some());
    let doc = entity.documentation_summary.as_ref().unwrap();
    assert!(doc.contains("Helper macro for creating messages"));
}

#[test]
fn test_complex_macro_patterns() {
    let source = r#"
macro_rules! complex {
    ($name:ident, $value:expr) => {
        let $name = $value;
    };
    ($name:ident = $value:expr) => {
        let mut $name = $value;
    };
}
"#;

    let entities = extract_with_handler(source, queries::MACRO_QUERY, handle_macro)
        .expect("Failed to extract macro");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.name, "complex");
    assert_eq!(entity.entity_type, EntityType::Macro);

    // Content should include pattern matching syntax
    assert!(entity.content.is_some());
    let content = entity.content.as_ref().unwrap();
    assert!(content.contains("$name:ident"));
    assert!(content.contains("$value:expr"));
}

#[test]
fn test_macro_with_repetitions() {
    let source = r#"
macro_rules! vec_of {
    ($($x:expr),* $(,)?) => {
        vec![$($x),*]
    };
}
"#;

    let entities = extract_with_handler(source, queries::MACRO_QUERY, handle_macro)
        .expect("Failed to extract macro");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.name, "vec_of");
    assert_eq!(entity.entity_type, EntityType::Macro);

    // Content should include repetition syntax
    assert!(entity.content.is_some());
    let content = entity.content.as_ref().unwrap();
    assert!(content.contains("$($x:expr),*")); // repetition pattern
}

#[test]
fn test_multiple_macros() {
    let source = r#"
macro_rules! first {
    () => { 1 };
}

#[macro_export]
macro_rules! second {
    () => { 2 };
}

macro_rules! third {
    () => { 3 };
}
"#;

    let entities = extract_with_handler(source, queries::MACRO_QUERY, handle_macro)
        .expect("Failed to extract macros");

    assert_eq!(entities.len(), 3);

    let names: Vec<&str> = entities.iter().map(|e| e.name.as_str()).collect();
    assert!(names.contains(&"first"));
    assert!(names.contains(&"second"));
    assert!(names.contains(&"third"));

    // Check that all are macros
    for entity in &entities {
        assert_eq!(entity.entity_type, EntityType::Macro);
    }

    // Check that only second is exported
    let second_macro = entities.iter().find(|e| e.name == "second").unwrap();
    assert_eq!(
        second_macro
            .metadata
            .attributes
            .get("exported")
            .map(|s| s.as_str()),
        Some("true")
    );

    let first_macro = entities.iter().find(|e| e.name == "first").unwrap();
    assert_eq!(
        first_macro
            .metadata
            .attributes
            .get("exported")
            .map(|s| s.as_str()),
        Some("false")
    );
}

#[test]
fn test_macro_with_debug_assertions() {
    let source = r#"
macro_rules! debug_log {
    ($($arg:tt)*) => {
        #[cfg(debug_assertions)]
        println!($($arg)*);
    };
}
"#;

    let entities = extract_with_handler(source, queries::MACRO_QUERY, handle_macro)
        .expect("Failed to extract macro");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.name, "debug_log");
    assert_eq!(entity.entity_type, EntityType::Macro);

    // Content should include cfg attribute
    assert!(entity.content.is_some());
    let content = entity.content.as_ref().unwrap();
    assert!(content.contains("cfg(debug_assertions)"));
}
