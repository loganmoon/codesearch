//! Tests for crate root extraction handler

use crate::rust::handler_impls::crate_root_handlers::handle_crate_root_impl;
use crate::rust::queries;
use codesearch_core::entities::{EntityType, Visibility};
use codesearch_core::{error::Result, CodeEntity};
use std::path::Path;
use streaming_iterator::StreamingIterator;
use tree_sitter::{Parser, Query, QueryCursor};

/// Helper to extract crate root entity from source code with a custom file path
fn extract_crate_root(source: &str, file_path: &Path) -> Result<Vec<CodeEntity>> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .unwrap();

    let tree = parser.parse(source, None).unwrap();
    let query = Query::new(
        &tree_sitter_rust::LANGUAGE.into(),
        queries::CRATE_ROOT_QUERY,
    )
    .unwrap();

    let mut cursor = QueryCursor::new();
    let mut matches_iter = cursor.matches(&query, tree.root_node(), source.as_bytes());

    let repository_id = "test-repo-id";
    let package_name = Some("test_crate");
    let repo_root = Path::new("/test-repo");

    let mut all_entities = Vec::new();
    while let Some(query_match) = matches_iter.next() {
        if let Ok(entities) = handle_crate_root_impl(
            query_match,
            &query,
            source,
            file_path,
            repository_id,
            package_name,
            None,
            repo_root,
        ) {
            all_entities.extend(entities);
        }
    }

    Ok(all_entities)
}

#[test]
fn test_crate_root_lib_rs() {
    let source = r#"
//! This is the crate root module.

pub mod utils;
pub mod config;

pub fn main_function() {}
"#;

    let entities = extract_crate_root(source, Path::new("/test-repo/src/lib.rs"))
        .expect("Failed to extract crate root");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.name, "test_crate");
    assert_eq!(entity.qualified_name, "test_crate");
    assert_eq!(entity.entity_type, EntityType::Module);
    assert_eq!(entity.visibility, Visibility::Public);
}

#[test]
fn test_crate_root_main_rs() {
    let source = r#"
fn main() {
    println!("Hello, world!");
}
"#;

    let entities = extract_crate_root(source, Path::new("/test-repo/src/main.rs"))
        .expect("Failed to extract crate root");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.name, "test_crate");
    assert_eq!(entity.qualified_name, "test_crate");
    assert_eq!(entity.entity_type, EntityType::Module);
    assert_eq!(entity.visibility, Visibility::Public);
}

#[test]
fn test_crate_root_not_for_regular_file() {
    let source = r#"
pub fn helper() {}
"#;

    let entities = extract_crate_root(source, Path::new("/test-repo/src/utils.rs"))
        .expect("Failed to extract from regular file");

    // Should not create a crate root entity for regular files
    assert_eq!(entities.len(), 0);
}

#[test]
fn test_crate_root_not_for_mod_rs() {
    let source = r#"
pub mod child;
"#;

    let entities = extract_crate_root(source, Path::new("/test-repo/src/network/mod.rs"))
        .expect("Failed to extract from mod.rs");

    // Should not create a crate root entity for mod.rs files
    assert_eq!(entities.len(), 0);
}

#[test]
fn test_crate_root_without_package_name() {
    let source = r#"
pub fn main_function() {}
"#;

    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .unwrap();

    let tree = parser.parse(source, None).unwrap();
    let query = Query::new(
        &tree_sitter_rust::LANGUAGE.into(),
        queries::CRATE_ROOT_QUERY,
    )
    .unwrap();

    let mut cursor = QueryCursor::new();
    let mut matches_iter = cursor.matches(&query, tree.root_node(), source.as_bytes());

    let file_path = Path::new("/test-repo/src/lib.rs");
    let repository_id = "test-repo-id";
    let repo_root = Path::new("/test-repo");

    let mut all_entities = Vec::new();
    while let Some(query_match) = matches_iter.next() {
        if let Ok(entities) = handle_crate_root_impl(
            query_match,
            &query,
            source,
            file_path,
            repository_id,
            None, // No package name
            None,
            repo_root,
        ) {
            all_entities.extend(entities);
        }
    }

    // Without a package name, no crate root entity should be created
    assert_eq!(all_entities.len(), 0);
}

#[test]
fn test_crate_root_has_no_parent_scope() {
    let source = r#"
pub mod utils;
"#;

    let entities = extract_crate_root(source, Path::new("/test-repo/src/lib.rs"))
        .expect("Failed to extract crate root");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    // Crate root should have no parent scope
    assert!(entity.parent_scope.is_none());
}
