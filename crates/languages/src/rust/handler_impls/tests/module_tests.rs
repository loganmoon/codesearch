//! Tests for module extraction handler

use super::*;
use crate::rust::handler_impls::module_handlers::handle_module_impl;
use codesearch_core::entities::{EntityType, Visibility};

#[test]
fn test_simple_inline_module() {
    let source = r#"
mod utils {
    fn helper() -> i32 {
        42
    }
}
"#;

    let entities = extract_with_handler(source, queries::MODULE_QUERY, handle_module_impl)
        .expect("Failed to extract module");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.name, "utils");
    assert_eq!(entity.entity_type, EntityType::Module);
    assert_eq!(entity.visibility, Some(Visibility::Private));
}

#[test]
fn test_file_module() {
    let source = r#"
mod network;
"#;

    let entities = extract_with_handler(source, queries::MODULE_QUERY, handle_module_impl)
        .expect("Failed to extract file module");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.name, "network");
    assert_eq!(entity.entity_type, EntityType::Module);

    // File modules don't have a body in the source
    assert!(entity.content.is_some());
}

#[test]
fn test_public_module() {
    let source = r#"
pub mod api {
    pub fn endpoint() {}
}
"#;

    let entities = extract_with_handler(source, queries::MODULE_QUERY, handle_module_impl)
        .expect("Failed to extract public module");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.name, "api");
    assert_eq!(entity.visibility, Some(Visibility::Public));
}

#[test]
fn test_nested_modules() {
    let source = r#"
mod outer {
    mod inner {
        fn function() {}
    }
}
"#;

    let entities = extract_with_handler(source, queries::MODULE_QUERY, handle_module_impl)
        .expect("Failed to extract nested modules");

    assert_eq!(entities.len(), 2);

    let outer_module = entities.iter().find(|e| e.name == "outer");
    assert!(outer_module.is_some());

    let inner_module = entities.iter().find(|e| e.name == "inner");
    assert!(inner_module.is_some());

    // Check that inner module has qualified name including outer
    let inner = inner_module.unwrap();
    assert!(
        inner.qualified_name.contains("outer"),
        "Inner module qualified name should include outer: {}",
        inner.qualified_name
    );
}

#[test]
fn test_module_with_pub_crate() {
    let source = r#"
pub(crate) mod internal {
    pub fn function() {}
}
"#;

    let entities = extract_with_handler(source, queries::MODULE_QUERY, handle_module_impl)
        .expect("Failed to extract pub(crate) module");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.name, "internal");
    // pub(crate) is treated as Internal visibility
    assert_eq!(entity.visibility, Some(Visibility::Internal));
}

#[test]
fn test_module_with_doc_comments() {
    let source = r#"
/// This module contains utility functions
/// for data processing
pub mod utils {
    pub fn process() {}
}
"#;

    let entities = extract_with_handler(source, queries::MODULE_QUERY, handle_module_impl)
        .expect("Failed to extract module with doc comments");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.name, "utils");

    assert!(entity.documentation_summary.is_some());
    let doc = entity.documentation_summary.as_ref().unwrap();
    assert!(doc.contains("utility functions"));
    assert!(doc.contains("data processing"));
}

#[test]
fn test_module_qualified_names() {
    let source = r#"
mod parent {
    pub mod child {
        pub mod grandchild {
            fn function() {}
        }
    }
}
"#;

    let entities = extract_with_handler(source, queries::MODULE_QUERY, handle_module_impl)
        .expect("Failed to extract modules for qualified names");

    assert_eq!(entities.len(), 3);

    // Find grandchild module
    let grandchild = entities
        .iter()
        .find(|e| e.name == "grandchild")
        .expect("Should find grandchild module");

    // Check qualified name includes full path
    assert!(
        grandchild.qualified_name.contains("parent") && grandchild.qualified_name.contains("child"),
        "Grandchild qualified name should include parent::child: {}",
        grandchild.qualified_name
    );
}

#[test]
fn test_multiple_modules() {
    let source = r#"
mod module1 {
    fn func1() {}
}

mod module2 {
    fn func2() {}
}

pub mod module3;
"#;

    let entities = extract_with_handler(source, queries::MODULE_QUERY, handle_module_impl)
        .expect("Failed to extract multiple modules");

    assert_eq!(entities.len(), 3);

    let module_names: Vec<&str> = entities.iter().map(|e| e.name.as_str()).collect();
    assert!(module_names.contains(&"module1"));
    assert!(module_names.contains(&"module2"));
    assert!(module_names.contains(&"module3"));

    // Check visibility
    let public_count = entities
        .iter()
        .filter(|e| e.visibility == Some(Visibility::Public))
        .count();
    assert_eq!(public_count, 1); // Only module3 is public
}

#[test]
fn test_cfg_test_module() {
    let source = r#"
#[cfg(test)]
mod tests {
    #[test]
    fn test_something() {}
}
"#;

    let entities = extract_with_handler(source, queries::MODULE_QUERY, handle_module_impl)
        .expect("Failed to extract cfg test module");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.name, "tests");
    assert_eq!(entity.entity_type, EntityType::Module);
}

#[test]
fn test_module_with_attributes() {
    let source = r#"
#[allow(dead_code)]
#[cfg(feature = "advanced")]
pub mod advanced {
    pub fn feature() {}
}
"#;

    let entities = extract_with_handler(source, queries::MODULE_QUERY, handle_module_impl)
        .expect("Failed to extract module with attributes");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.name, "advanced");
    assert_eq!(entity.entity_type, EntityType::Module);
}
