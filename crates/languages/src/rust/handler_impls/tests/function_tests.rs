//! Tests for function extraction handler

use super::*;
use crate::rust::handler_impls::function_handlers::handle_function_impl;
use codesearch_core::entities::{EntityType, Visibility};

#[test]
fn test_simple_function() {
    let source = r#"
fn simple_function() {
    println!("Hello");
}
"#;

    let entities = extract_with_handler(source, queries::FUNCTION_QUERY, handle_function_impl)
        .expect("Failed to extract function");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.name, "simple_function");
    assert_eq!(entity.entity_type, EntityType::Function);

    // Check metadata
    assert!(!entity.metadata.is_async);
    assert!(!entity.metadata.is_const);
    assert_eq!(entity.metadata.attributes.get("unsafe"), None);

    // Check signature
    let sig = entity
        .signature
        .as_ref()
        .expect("Function should have signature");
    assert_eq!(sig.parameters.len(), 0);
}

#[test]
fn test_async_function() {
    let source = r#"
async fn fetch_data() -> Result<String, Error> {
    Ok("data".to_string())
}
"#;

    let entities = extract_with_handler(source, queries::FUNCTION_QUERY, handle_function_impl)
        .expect("Failed to extract function");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.entity_type, EntityType::Function);

    // Check async
    assert!(entity.metadata.is_async);

    // Check signature
    let sig = entity
        .signature
        .as_ref()
        .expect("Function should have signature");
    assert!(sig.is_async);
    assert_eq!(sig.return_type.as_deref(), Some("Result<String, Error>"));
}

#[test]
fn test_unsafe_function() {
    let source = r#"
unsafe fn dangerous_operation(ptr: *mut u8) {
    *ptr = 42;
}
"#;

    let entities = extract_with_handler(source, queries::FUNCTION_QUERY, handle_function_impl)
        .expect("Failed to extract function");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.entity_type, EntityType::Function);

    // Check unsafe
    assert_eq!(
        entity.metadata.attributes.get("unsafe").map(|s| s.as_str()),
        Some("true")
    );

    // Check parameters
    let sig = entity
        .signature
        .as_ref()
        .expect("Function should have signature");
    assert_eq!(sig.parameters.len(), 1);
    assert_eq!(sig.parameters[0].0, "ptr");
    assert_eq!(sig.parameters[0].1.as_deref(), Some("*mut u8"));
}

#[test]
fn test_const_function() {
    let source = r#"
const fn compile_time_computation(x: i32) -> i32 {
    x * 2
}
"#;

    let entities = extract_with_handler(source, queries::FUNCTION_QUERY, handle_function_impl)
        .expect("Failed to extract function");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.entity_type, EntityType::Function);

    // Check const
    assert!(entity.metadata.is_const);

    // Check signature
    let sig = entity
        .signature
        .as_ref()
        .expect("Function should have signature");
    assert_eq!(sig.parameters.len(), 1);
    assert_eq!(sig.return_type.as_deref(), Some("i32"));
}

#[test]
fn test_generic_function() {
    let source = r#"
fn generic_func<T: Clone, U>(item: T, other: U) -> (T, U)
where
    U: Debug,
{
    (item.clone(), other)
}
"#;

    let entities = extract_with_handler(source, queries::FUNCTION_QUERY, handle_function_impl)
        .expect("Failed to extract function");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.entity_type, EntityType::Function);

    // Check generics
    assert!(entity.metadata.is_generic);
    assert_eq!(entity.metadata.generic_params.len(), 2);
    // With bounds extraction, T has inline bound "Clone", U has where clause bound "Debug"
    assert!(
        entity
            .metadata
            .generic_params
            .iter()
            .any(|p| p.starts_with("T:") && p.contains("Clone")),
        "T should have Clone bound, got: {:?}",
        entity.metadata.generic_params
    );
    assert!(
        entity
            .metadata
            .generic_params
            .iter()
            .any(|p| p.starts_with("U:") && p.contains("Debug")),
        "U should have Debug bound from where clause, got: {:?}",
        entity.metadata.generic_params
    );

    // Check signature
    let sig = entity
        .signature
        .as_ref()
        .expect("Function should have signature");
    assert_eq!(sig.parameters.len(), 2);
    assert_eq!(sig.return_type.as_deref(), Some("(T, U)"));
    assert_eq!(sig.generics.len(), 2);
}

#[test]
fn test_function_with_doc_comments() {
    let source = r#"
/// This is a well-documented function
/// It does something important
pub fn documented_function(x: i32) -> i32 {
    x + 1
}
"#;

    let entities = extract_with_handler(source, queries::FUNCTION_QUERY, handle_function_impl)
        .expect("Failed to extract function");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];

    assert_eq!(entity.name, "documented_function");
    assert!(entity.documentation_summary.is_some());
    let doc = entity.documentation_summary.as_ref().unwrap();
    assert!(doc.contains("well-documented"));
    assert!(doc.contains("important"));
}

#[test]
fn test_function_with_lifetime_parameters() {
    let source = r#"
fn lifetime_func<'a, 'b: 'a>(x: &'a str, y: &'b str) -> &'a str {
    if x.len() > y.len() { x } else { y }
}
"#;

    let entities = extract_with_handler(source, queries::FUNCTION_QUERY, handle_function_impl)
        .expect("Failed to extract function");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.entity_type, EntityType::Function);

    // Check generics (includes lifetimes)
    assert_eq!(entity.metadata.generic_params.len(), 2);

    // Check parameters
    let sig = entity
        .signature
        .as_ref()
        .expect("Function should have signature");
    assert_eq!(sig.parameters.len(), 2);
    assert_eq!(sig.parameters[0].0, "x");
    assert_eq!(sig.parameters[0].1.as_deref(), Some("&'a str"));
    assert_eq!(sig.return_type.as_deref(), Some("&'a str"));
}

#[test]
fn test_function_with_self_parameter() {
    let source = r#"
impl MyStruct {
    fn method(&self, x: i32) -> i32 {
        self.value + x
    }
}
"#;

    // Note: This might not match with FUNCTION_QUERY as it's inside an impl block
    // This test documents the current behavior
    let _entities = extract_with_handler(source, queries::FUNCTION_QUERY, handle_function_impl)
        .expect("Extraction should not fail");

    // Currently, functions inside impl blocks might not be matched by FUNCTION_QUERY
    // This is expected behavior - impl methods would need a different query
}

#[test]
fn test_function_with_complex_parameters() {
    let source = r#"
fn complex_params(
    (x, y): (i32, i32),
    MyStruct { field1, field2: renamed }: MyStruct,
    _ignored: bool,
) -> i32 {
    x + y + field1 + renamed
}
"#;

    let entities = extract_with_handler(source, queries::FUNCTION_QUERY, handle_function_impl)
        .expect("Failed to extract function");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.entity_type, EntityType::Function);

    // Check parameters
    let sig = entity
        .signature
        .as_ref()
        .expect("Function should have signature");
    assert_eq!(sig.parameters.len(), 3);
    // Parameter patterns are complex and might be simplified in extraction
    assert!(sig.parameters[0].0.contains("(x, y)") || sig.parameters[0].0 == "(x, y)");
}

#[test]
fn test_public_vs_private_functions() {
    let source = r#"
pub fn public_function() {}
fn private_function() {}
pub(crate) fn crate_public() {}
"#;

    let entities = extract_with_handler(source, queries::FUNCTION_QUERY, handle_function_impl)
        .expect("Failed to extract functions");

    // Should extract all three functions
    assert_eq!(entities.len(), 3);

    // Check visibility is properly extracted
    assert_eq!(entities[0].visibility, Some(Visibility::Public));
    assert_eq!(entities[1].visibility, Some(Visibility::Private));
    assert_eq!(entities[2].visibility, Some(Visibility::Internal)); // pub(crate) is now Internal
}

// ============================================================================
// Generic Bounds Extraction Tests
// ============================================================================

#[test]
fn test_generic_function_with_inline_bounds() {
    // Include imports so trait resolution works
    let source = r#"
use std::clone::Clone;
use std::marker::Send;

fn process<T: Clone + Send, U>(item: T, other: U) -> T {
    item.clone()
}
"#;

    let entities = extract_with_handler(source, queries::FUNCTION_QUERY, handle_function_impl)
        .expect("Failed to extract function");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];

    // Check generic_params (backward-compat raw strings)
    assert!(entity.metadata.is_generic);
    assert_eq!(entity.metadata.generic_params.len(), 2);

    // Check generic_bounds (structured) - traits resolved to full paths
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

    // Check uses_types includes bound traits (now in typed relationships)
    let uses_types = &entity.relationships.uses_types;
    assert!(!uses_types.is_empty(), "Should have uses_types");
    assert!(
        uses_types.iter().any(|t| t.target().contains("Clone")),
        "uses_types should include Clone"
    );
    assert!(
        uses_types.iter().any(|t| t.target().contains("Send")),
        "uses_types should include Send"
    );
}

#[test]
fn test_generic_function_with_where_clause() {
    let source = r#"
use std::fmt::Debug;
use std::clone::Clone;
use std::marker::Sync;

fn process<T, U>(item: T, other: U) -> T
where
    T: Debug,
    U: Clone + Sync,
{
    item
}
"#;

    let entities = extract_with_handler(source, queries::FUNCTION_QUERY, handle_function_impl)
        .expect("Failed to extract function");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];

    // Check generic_bounds includes where clause bounds
    let bounds = &entity.metadata.generic_bounds;
    assert!(bounds.contains_key("T"), "Should have bounds for T");
    assert!(bounds.contains_key("U"), "Should have bounds for U");

    let t_bounds = bounds.get("T").unwrap();
    assert!(
        t_bounds.iter().any(|b| b.contains("Debug")),
        "T should have Debug bound"
    );

    let u_bounds = bounds.get("U").unwrap();
    assert!(
        u_bounds.iter().any(|b| b.contains("Clone")),
        "U should have Clone bound"
    );
    assert!(
        u_bounds.iter().any(|b| b.contains("Sync")),
        "U should have Sync bound"
    );

    // Check uses_types (now in typed relationships)
    let uses_types = &entity.relationships.uses_types;
    assert!(!uses_types.is_empty(), "Should have uses_types");
    assert!(
        uses_types.iter().any(|t| t.target().contains("Debug")),
        "uses_types should include Debug"
    );
    assert!(
        uses_types.iter().any(|t| t.target().contains("Clone")),
        "uses_types should include Clone"
    );
    assert!(
        uses_types.iter().any(|t| t.target().contains("Sync")),
        "uses_types should include Sync"
    );
}

#[test]
fn test_generic_function_with_inline_and_where_clause() {
    let source = r#"
fn process<T: Clone>(item: T) -> T
where
    T: Debug,
{
    item.clone()
}
"#;

    let entities = extract_with_handler(source, queries::FUNCTION_QUERY, handle_function_impl)
        .expect("Failed to extract function");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];

    // Check generic_bounds merges inline and where clause bounds
    let bounds = &entity.metadata.generic_bounds;
    let t_bounds = bounds.get("T").unwrap();
    // Bounds may be unresolved (just "Clone") or resolved (with prefix) depending on imports
    assert!(
        t_bounds.iter().any(|b| b.contains("Clone")),
        "Should have inline bound, got: {:?}",
        t_bounds
    );
    assert!(
        t_bounds.iter().any(|b| b.contains("Debug")),
        "Should have where clause bound, got: {:?}",
        t_bounds
    );

    // Check generic_params reflects merged bounds
    let t_param = entity
        .metadata
        .generic_params
        .iter()
        .find(|p| p.starts_with("T:"))
        .expect("Should have T param with bounds");
    assert!(t_param.contains("Clone"));
    assert!(t_param.contains("Debug"));
}

/// Tests that method calls on generic type parameters generate CALLS references
/// to the trait methods. This is the key test for Bug #153 Phase 5.
#[test]
fn test_generic_bounds_calls_extraction() {
    // This test mimics the e2e test scenario: a function with a generic parameter
    // bounded by a trait, where we call a method from that trait.
    let source = r#"
pub trait Processor {
    fn process(&self) -> i32;
}

pub fn process_item<T: Processor>(item: &T) -> i32 {
    item.process()
}
"#;

    let entities = extract_with_handler(source, queries::FUNCTION_QUERY, handle_function_impl)
        .expect("Failed to extract function");

    // Should extract process_item (trait method is inside trait, not matched by FUNCTION_QUERY)
    assert!(!entities.is_empty(), "Should extract at least one function");

    let process_item = entities
        .iter()
        .find(|e| e.name == "process_item")
        .expect("Should find process_item function");

    // Check that generic_bounds has T -> [Processor]
    let bounds = &process_item.metadata.generic_bounds;
    assert!(
        bounds.contains_key("T"),
        "Should have bounds for T, got: {:?}",
        bounds
    );
    let t_bounds = bounds.get("T").unwrap();
    assert!(
        t_bounds.iter().any(|b| b.contains("Processor")),
        "T should be bounded by Processor, got: {:?}",
        t_bounds
    );

    // KEY TEST: Check that calls (now in typed relationships) contains a call to Processor::process
    let calls = &process_item.relationships.calls;
    assert!(
        !calls.is_empty(),
        "process_item should have calls - this is the bug!"
    );

    assert!(
        calls
            .iter()
            .any(|c| c.target().contains("Processor::process")),
        "Should have a call to Processor::process, got calls: {:?}",
        calls
    );
}

/// Tests that method calls on generic type parameters resolve to fully qualified names
/// when package_name is provided (simulates e2e test conditions).
#[test]
fn test_generic_bounds_calls_with_package_name() {
    use std::path::Path;
    use streaming_iterator::StreamingIterator;
    use tree_sitter::{Parser, Query, QueryCursor};

    let source = r#"
pub trait Processor {
    fn process(&self) -> i32;
}

pub fn process_item<T: Processor>(item: &T) -> i32 {
    item.process()
}
"#;

    // Setup parser and query - same as extract_with_handler but with package_name
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(source, None).unwrap();
    let query = Query::new(&tree_sitter_rust::LANGUAGE.into(), queries::FUNCTION_QUERY).unwrap();

    let mut cursor = QueryCursor::new();
    let mut matches_iter = cursor.matches(&query, tree.root_node(), source.as_bytes());

    let path = Path::new("lib.rs");
    let repository_id = "test-repo-id";
    let repo_root = Path::new("/test-repo");
    let package_name = Some("test_crate"); // KEY: provide package name like e2e test

    let mut all_entities = Vec::new();
    while let Some(query_match) = matches_iter.next() {
        if let Ok(entities) = handle_function_impl(
            query_match,
            &query,
            source,
            path,
            repository_id,
            package_name,
            None,
            repo_root,
        ) {
            all_entities.extend(entities);
        }
    }

    // Find process_item function
    let process_item = all_entities
        .iter()
        .find(|e| e.name == "process_item")
        .expect("Should find process_item function");

    eprintln!(
        "DEBUG with package: qualified_name = {}",
        process_item.qualified_name
    );
    eprintln!(
        "DEBUG with package: generic_bounds = {:?}",
        process_item.metadata.generic_bounds
    );

    // Check that generic_bounds resolves Processor to test_crate::Processor
    let bounds = &process_item.metadata.generic_bounds;
    let t_bounds = bounds.get("T").expect("Should have bounds for T");
    assert!(
        t_bounds.iter().any(|b| *b == "test_crate::Processor"),
        "T should be bounded by test_crate::Processor, got: {:?}",
        t_bounds
    );

    // Check that calls (now in typed relationships) resolves to test_crate::Processor::process
    let calls = &process_item.relationships.calls;
    assert!(!calls.is_empty(), "Should have calls");

    eprintln!("DEBUG with package: calls = {:?}", calls);

    assert!(
        calls
            .iter()
            .any(|c| c.target() == "test_crate::Processor::process"),
        "Should have a call to test_crate::Processor::process, got calls: {:?}",
        calls
    );
}
