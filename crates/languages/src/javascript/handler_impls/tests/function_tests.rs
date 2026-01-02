//! Tests for JavaScript function extraction handlers

use super::*;
use crate::javascript::handler_impls::{handle_arrow_function_impl, handle_function_impl};
use codesearch_core::entities::EntityType;

#[test]
fn test_simple_function() {
    let source = r#"
function greet(name) {
    return `Hello, ${name}!`;
}
"#;

    let entities = extract_with_handler(source, queries::FUNCTION_QUERY, handle_function_impl)
        .expect("Failed to extract function");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.name, "greet");
    assert_eq!(entity.qualified_name, "greet");
    assert_eq!(entity.entity_type, EntityType::Function);
    assert!(entity.parent_scope.is_none());
}

#[test]
fn test_async_function() {
    let source = r#"
async function fetchData(url) {
    const response = await fetch(url);
    return response.json();
}
"#;

    let entities = extract_with_handler(source, queries::FUNCTION_QUERY, handle_function_impl)
        .expect("Failed to extract function");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.name, "fetchData");
    assert!(entity.metadata.is_async);

    let sig = entity.signature.as_ref().expect("Should have signature");
    assert!(sig.is_async);
    assert_eq!(sig.parameters.len(), 1);
    assert_eq!(sig.parameters[0].0, "url");
}

#[test]
fn test_function_with_multiple_parameters() {
    let source = r#"
function add(a, b, c) {
    return a + b + c;
}
"#;

    let entities = extract_with_handler(source, queries::FUNCTION_QUERY, handle_function_impl)
        .expect("Failed to extract function");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.name, "add");

    let sig = entity.signature.as_ref().expect("Should have signature");
    assert_eq!(sig.parameters.len(), 3);
    assert_eq!(sig.parameters[0].0, "a");
    assert_eq!(sig.parameters[1].0, "b");
    assert_eq!(sig.parameters[2].0, "c");
}

#[test]
fn test_function_with_jsdoc() {
    let source = r#"
/**
 * Calculates the sum of two numbers.
 * @param {number} a - First number
 * @param {number} b - Second number
 * @returns {number} The sum
 */
function sum(a, b) {
    return a + b;
}
"#;

    let entities = extract_with_handler(source, queries::FUNCTION_QUERY, handle_function_impl)
        .expect("Failed to extract function");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.name, "sum");
    assert!(entity.documentation_summary.is_some());
    let doc = entity.documentation_summary.as_ref().unwrap();
    assert!(doc.contains("Calculates the sum"));
}

#[test]
fn test_multiple_functions() {
    let source = r#"
function first() {}
function second() {}
function third() {}
"#;

    let entities = extract_with_handler(source, queries::FUNCTION_QUERY, handle_function_impl)
        .expect("Failed to extract functions");

    assert_eq!(entities.len(), 3);
    assert_eq!(entities[0].name, "first");
    assert_eq!(entities[1].name, "second");
    assert_eq!(entities[2].name, "third");
}

#[test]
fn test_arrow_function_in_variable() {
    let source = r#"
const add = (a, b) => a + b;
"#;

    let entities = extract_with_handler(
        source,
        queries::ARROW_FUNCTION_QUERY,
        handle_arrow_function_impl,
    )
    .expect("Failed to extract arrow function");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.name, "add");
    assert_eq!(entity.qualified_name, "add");
    assert_eq!(entity.entity_type, EntityType::Function);
}

#[test]
fn test_async_arrow_function() {
    let source = r#"
const fetchUser = async (id) => {
    const response = await fetch(`/users/${id}`);
    return response.json();
};
"#;

    let entities = extract_with_handler(
        source,
        queries::ARROW_FUNCTION_QUERY,
        handle_arrow_function_impl,
    )
    .expect("Failed to extract arrow function");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.name, "fetchUser");
    assert!(entity.metadata.is_async);
}

#[test]
fn test_arrow_function_single_param_no_parens() {
    let source = r#"
const double = x => x * 2;
"#;

    let entities = extract_with_handler(
        source,
        queries::ARROW_FUNCTION_QUERY,
        handle_arrow_function_impl,
    )
    .expect("Failed to extract arrow function");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.name, "double");

    let sig = entity.signature.as_ref().expect("Should have signature");
    assert_eq!(sig.parameters.len(), 1);
    assert_eq!(sig.parameters[0].0, "x");
}

#[test]
fn test_arrow_function_no_params() {
    let source = r#"
const getTimestamp = () => Date.now();
"#;

    let entities = extract_with_handler(
        source,
        queries::ARROW_FUNCTION_QUERY,
        handle_arrow_function_impl,
    )
    .expect("Failed to extract arrow function");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.name, "getTimestamp");

    let sig = entity.signature.as_ref().expect("Should have signature");
    assert_eq!(sig.parameters.len(), 0);
}

#[test]
fn test_function_qualified_name_top_level() {
    let source = r#"
function topLevelFunction() {
    return 42;
}
"#;

    let entities = extract_with_handler(source, queries::FUNCTION_QUERY, handle_function_impl)
        .expect("Failed to extract function");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.qualified_name, "topLevelFunction");
    assert!(entity.parent_scope.is_none());
}

#[test]
fn test_export_default_function() {
    // Test that export default function is extracted
    let source = r#"
export default function pLimit(concurrency) {
    const queue = [];
    let activeCount = 0;

    const next = () => {
        activeCount--;
    };

    return next;
}
"#;

    let entities = extract_with_handler(source, queries::FUNCTION_QUERY, handle_function_impl)
        .expect("Failed to extract function");

    // Should extract pLimit
    assert_eq!(entities.len(), 1, "Should extract the exported function");
    let entity = &entities[0];
    assert_eq!(entity.name, "pLimit");
    assert_eq!(entity.qualified_name, "pLimit");
}

#[test]
fn test_nested_arrow_functions_have_parent_scope() {
    // Test that arrow functions inside a function have parent_scope set
    let source = r#"
function outer() {
    const inner = () => {
        return 42;
    };
    return inner;
}
"#;

    // First extract the outer function
    let outer_entities =
        extract_with_handler(source, queries::FUNCTION_QUERY, handle_function_impl)
            .expect("Failed to extract outer function");
    assert_eq!(outer_entities.len(), 1);
    assert_eq!(outer_entities[0].name, "outer");

    // Now extract the arrow function
    let inner_entities = extract_with_handler(
        source,
        queries::ARROW_FUNCTION_QUERY,
        handle_arrow_function_impl,
    )
    .expect("Failed to extract arrow function");
    assert_eq!(inner_entities.len(), 1);
    let inner = &inner_entities[0];
    assert_eq!(inner.name, "inner");
    assert_eq!(
        inner.parent_scope.as_deref(),
        Some("outer"),
        "Arrow function inside function_declaration should have parent_scope"
    );
    assert_eq!(inner.qualified_name, "outer.inner");
}

// ============================================================================
// Tests for function calls extraction
// ============================================================================

#[test]
fn test_function_extracts_calls() {
    let source = r#"
function process() {
    helper();
    console.log("done");
}
"#;

    let entities = extract_with_handler(source, queries::FUNCTION_QUERY, handle_function_impl)
        .expect("Failed to extract function");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.name, "process");

    // Check that calls are extracted in relationships
    assert!(!entity.relationships.calls.is_empty(), "Should have calls");
    let call_targets: Vec<&str> = entity
        .relationships
        .calls
        .iter()
        .map(|c| c.target())
        .collect();
    assert!(call_targets.contains(&"external.helper"));
    assert!(call_targets.contains(&"external.console.log"));
}

#[test]
fn test_function_with_import_resolves_calls() {
    let source = r#"
import { helper } from './utils';

function process() {
    helper();
}
"#;

    let entities = extract_with_handler(source, queries::FUNCTION_QUERY, handle_function_impl)
        .expect("Failed to extract function");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];

    // Check that calls are extracted in relationships
    assert!(!entity.relationships.calls.is_empty(), "Should have calls");
    let call_targets: Vec<&str> = entity
        .relationships
        .calls
        .iter()
        .map(|c| c.target())
        .collect();
    // Should resolve through import
    assert!(call_targets.contains(&"./utils.helper"));
}

// ============================================================================
// Tests for JSDoc type references extraction
// ============================================================================

#[test]
fn test_function_extracts_uses_types_from_jsdoc() {
    let source = r#"
/**
 * Process a user request
 * @param {User} user - The user object
 * @param {Request} request - The request object
 * @returns {Response} The response
 */
function processRequest(user, request) {
    return new Response();
}
"#;

    let entities = extract_with_handler(source, queries::FUNCTION_QUERY, handle_function_impl)
        .expect("Failed to extract function");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];

    // Check that uses_types are extracted in relationships
    assert!(
        !entity.relationships.uses_types.is_empty(),
        "Should have uses_types"
    );

    // Should extract non-primitive types from JSDoc
    assert!(entity
        .relationships
        .uses_types
        .iter()
        .any(|t| t.target().contains("User")));
    assert!(entity
        .relationships
        .uses_types
        .iter()
        .any(|t| t.target().contains("Request")));
    assert!(entity
        .relationships
        .uses_types
        .iter()
        .any(|t| t.target().contains("Response")));
}

#[test]
fn test_function_jsdoc_filters_primitives() {
    let source = r#"
/**
 * Add two numbers
 * @param {number} a - First number
 * @param {string} b - Second value
 * @returns {boolean} Result
 */
function add(a, b) {
    return true;
}
"#;

    let entities = extract_with_handler(source, queries::FUNCTION_QUERY, handle_function_impl)
        .expect("Failed to extract function");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];

    // Should NOT have uses_types since all types are primitives
    let uses_types_attr = entity.metadata.attributes.get("uses_types");
    assert!(
        uses_types_attr.is_none(),
        "Should not have uses_types for primitives only"
    );
}
