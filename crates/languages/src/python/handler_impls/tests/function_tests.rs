//! Tests for Python function extraction handlers

use super::*;
use crate::python::handler_impls::handle_function_impl;
use codesearch_core::entities::{EntityType, SourceReference};

#[test]
fn test_simple_function() {
    let source = r#"
def greet(name):
    return f"Hello, {name}!"
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
async def fetch_data(url):
    response = await aiohttp.get(url)
    return await response.json()
"#;

    let entities = extract_with_handler(source, queries::FUNCTION_QUERY, handle_function_impl)
        .expect("Failed to extract function");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.name, "fetch_data");
    assert!(entity.metadata.is_async);

    let sig = entity.signature.as_ref().expect("Should have signature");
    assert!(sig.is_async);
}

#[test]
fn test_function_with_multiple_parameters() {
    let source = r#"
def add(a, b, c):
    return a + b + c
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
fn test_function_with_type_annotations() {
    let source = r#"
def calculate(x: int, y: int) -> int:
    return x + y
"#;

    let entities = extract_with_handler(source, queries::FUNCTION_QUERY, handle_function_impl)
        .expect("Failed to extract function");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.name, "calculate");

    let sig = entity.signature.as_ref().expect("Should have signature");
    assert_eq!(sig.parameters.len(), 2);
    assert_eq!(sig.parameters[0].0, "x");
    assert_eq!(sig.parameters[0].1.as_deref(), Some("int"));
    assert_eq!(sig.return_type.as_deref(), Some("int"));
}

#[test]
fn test_function_with_docstring() {
    let source = r#"
def calculate_sum(a, b):
    """
    Calculate the sum of two numbers.

    Args:
        a: First number
        b: Second number

    Returns:
        The sum of a and b
    """
    return a + b
"#;

    let entities = extract_with_handler(source, queries::FUNCTION_QUERY, handle_function_impl)
        .expect("Failed to extract function");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.name, "calculate_sum");
    assert!(entity.documentation_summary.is_some());
    let doc = entity.documentation_summary.as_ref().unwrap();
    assert!(doc.contains("Calculate the sum"));
}

#[test]
fn test_multiple_functions() {
    let source = r#"
def first():
    pass

def second():
    pass

def third():
    pass
"#;

    let entities = extract_with_handler(source, queries::FUNCTION_QUERY, handle_function_impl)
        .expect("Failed to extract functions");

    assert_eq!(entities.len(), 3);
    assert_eq!(entities[0].name, "first");
    assert_eq!(entities[1].name, "second");
    assert_eq!(entities[2].name, "third");
}

#[test]
fn test_function_with_decorator() {
    let source = r#"
@decorator
def decorated_function():
    pass
"#;

    let entities = extract_with_handler(source, queries::FUNCTION_QUERY, handle_function_impl)
        .expect("Failed to extract function");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.name, "decorated_function");
    assert!(entity
        .metadata
        .decorators
        .contains(&"decorator".to_string()));
}

#[test]
fn test_function_with_multiple_decorators() {
    let source = r#"
@first_decorator
@second_decorator
@third_decorator
def multi_decorated():
    pass
"#;

    let entities = extract_with_handler(source, queries::FUNCTION_QUERY, handle_function_impl)
        .expect("Failed to extract function");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.metadata.decorators.len(), 3);
}

#[test]
fn test_function_with_default_parameters() {
    let source = r#"
def greet(name, greeting="Hello"):
    return f"{greeting}, {name}!"
"#;

    let entities = extract_with_handler(source, queries::FUNCTION_QUERY, handle_function_impl)
        .expect("Failed to extract function");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    let sig = entity.signature.as_ref().expect("Should have signature");
    assert_eq!(sig.parameters.len(), 2);
}

#[test]
fn test_function_qualified_name_top_level() {
    let source = r#"
def top_level_function():
    return 42
"#;

    let entities = extract_with_handler(source, queries::FUNCTION_QUERY, handle_function_impl)
        .expect("Failed to extract function");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.qualified_name, "top_level_function");
    assert!(entity.parent_scope.is_none());
}

#[test]
fn test_function_with_args_kwargs() {
    let source = r#"
def variadic_function(*args, **kwargs):
    pass
"#;

    let entities = extract_with_handler(source, queries::FUNCTION_QUERY, handle_function_impl)
        .expect("Failed to extract function");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    let sig = entity.signature.as_ref().expect("Should have signature");
    assert_eq!(sig.parameters.len(), 2);
}

// ============================================================================
// Tests for function calls extraction
// ============================================================================

#[test]
fn test_function_extracts_calls() {
    let source = r#"
def process():
    helper()
    print("done")
"#;

    let entities = extract_with_handler(source, queries::FUNCTION_QUERY, handle_function_impl)
        .expect("Failed to extract function");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];

    let calls_attr = entity.metadata.attributes.get("calls");
    assert!(calls_attr.is_some(), "Should have calls attribute");

    let calls: Vec<SourceReference> =
        serde_json::from_str(calls_attr.unwrap()).expect("Should parse calls JSON");
    // Should extract both function calls
    assert!(calls.iter().any(|c| c.target().contains("helper")));
    assert!(calls.iter().any(|c| c.target().contains("print")));
}

#[test]
fn test_function_with_import_resolves_calls() {
    let source = r#"
from utils import helper

def process():
    helper()
"#;

    let entities = extract_with_handler(source, queries::FUNCTION_QUERY, handle_function_impl)
        .expect("Failed to extract function");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];

    let calls_attr = entity.metadata.attributes.get("calls");
    assert!(calls_attr.is_some(), "Should have calls attribute");

    let calls: Vec<SourceReference> =
        serde_json::from_str(calls_attr.unwrap()).expect("Should parse calls JSON");
    // Should resolve through import - absolute imports are marked with external. prefix
    assert!(calls.iter().any(|c| c.target() == "external.utils.helper"));
}

// ============================================================================
// Tests for type reference extraction from type hints
// ============================================================================

#[test]
fn test_function_extracts_uses_types_from_hints() {
    let source = r#"
def process_user(user: User, request: Request) -> Response:
    return Response()
"#;

    let entities = extract_with_handler(source, queries::FUNCTION_QUERY, handle_function_impl)
        .expect("Failed to extract function");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];

    let uses_types_attr = entity.metadata.attributes.get("uses_types");
    assert!(
        uses_types_attr.is_some(),
        "Should have uses_types attribute"
    );

    let uses_types: Vec<SourceReference> =
        serde_json::from_str(uses_types_attr.unwrap()).expect("Should parse uses_types JSON");

    // Should extract non-primitive types from type hints
    assert!(uses_types.iter().any(|t| t.target().contains("User")));
    assert!(uses_types.iter().any(|t| t.target().contains("Request")));
    assert!(uses_types.iter().any(|t| t.target().contains("Response")));
}

#[test]
fn test_function_type_hints_filters_primitives() {
    let source = r#"
def add(a: int, b: str) -> bool:
    return True
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
