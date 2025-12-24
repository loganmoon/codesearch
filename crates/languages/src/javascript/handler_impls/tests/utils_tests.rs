//! Tests for JavaScript utility functions
//!
//! Tests the shared utility functions used by JavaScript entity extraction,
//! including parameter extraction, JSDoc parsing, and primitive detection.

use crate::common::import_map::ImportMap;
use crate::javascript::utils::{
    extract_function_calls, extract_jsdoc_comments, extract_parameters,
    extract_type_references_from_jsdoc, is_js_primitive,
};
use tree_sitter::Parser;

/// Helper to parse JavaScript source and get the root node
fn parse_js(source: &str) -> tree_sitter::Tree {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_javascript::LANGUAGE.into())
        .expect("Failed to set JavaScript language");
    parser.parse(source, None).expect("Failed to parse source")
}

/// Find a node of a specific kind in the tree
fn find_node<'a>(node: tree_sitter::Node<'a>, kind: &str) -> Option<tree_sitter::Node<'a>> {
    if node.kind() == kind {
        return Some(node);
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if let Some(found) = find_node(child, kind) {
            return Some(found);
        }
    }
    None
}

// ============================================================================
// is_js_primitive tests
// ============================================================================

#[test]
fn test_is_js_primitive_basic_types() {
    assert!(is_js_primitive("string"));
    assert!(is_js_primitive("number"));
    assert!(is_js_primitive("boolean"));
    assert!(is_js_primitive("object"));
    assert!(is_js_primitive("void"));
    assert!(is_js_primitive("null"));
    assert!(is_js_primitive("undefined"));
}

#[test]
fn test_is_js_primitive_case_insensitive() {
    assert!(is_js_primitive("String"));
    assert!(is_js_primitive("NUMBER"));
    assert!(is_js_primitive("Boolean"));
    assert!(is_js_primitive("VOID"));
}

#[test]
fn test_is_js_primitive_additional_types() {
    assert!(is_js_primitive("any"));
    assert!(is_js_primitive("symbol"));
    assert!(is_js_primitive("bigint"));
    assert!(is_js_primitive("never"));
    assert!(is_js_primitive("array"));
    assert!(is_js_primitive("function"));
    assert!(is_js_primitive("promise"));
    assert!(is_js_primitive("*"));
}

#[test]
fn test_is_js_primitive_non_primitives() {
    assert!(!is_js_primitive("MyClass"));
    assert!(!is_js_primitive("CustomType"));
    assert!(!is_js_primitive("User"));
    assert!(!is_js_primitive("Result"));
}

// ============================================================================
// extract_parameters tests
// ============================================================================

#[test]
fn test_extract_parameters_simple() {
    let source = "function foo(a, b, c) {}";
    let tree = parse_js(source);
    let params_node =
        find_node(tree.root_node(), "formal_parameters").expect("Should find formal_parameters");

    let params = extract_parameters(params_node, source).expect("Should extract parameters");
    assert_eq!(params.len(), 3);
    assert_eq!(params[0].0, "a");
    assert_eq!(params[1].0, "b");
    assert_eq!(params[2].0, "c");
}

#[test]
fn test_extract_parameters_with_defaults() {
    let source = "function foo(a, b = 10, c = 'default') {}";
    let tree = parse_js(source);
    let params_node =
        find_node(tree.root_node(), "formal_parameters").expect("Should find formal_parameters");

    let params = extract_parameters(params_node, source).expect("Should extract parameters");
    assert_eq!(params.len(), 3);
    assert_eq!(params[0].0, "a");
    assert_eq!(params[1].0, "b");
    assert_eq!(params[2].0, "c");
}

#[test]
fn test_extract_parameters_rest() {
    let source = "function foo(a, ...rest) {}";
    let tree = parse_js(source);
    let params_node =
        find_node(tree.root_node(), "formal_parameters").expect("Should find formal_parameters");

    let params = extract_parameters(params_node, source).expect("Should extract parameters");
    assert_eq!(params.len(), 2);
    assert_eq!(params[0].0, "a");
    assert_eq!(params[1].0, "...rest");
}

#[test]
fn test_extract_parameters_destructuring() {
    let source = "function foo({x, y}, [a, b]) {}";
    let tree = parse_js(source);
    let params_node =
        find_node(tree.root_node(), "formal_parameters").expect("Should find formal_parameters");

    let params = extract_parameters(params_node, source).expect("Should extract parameters");
    assert_eq!(params.len(), 2);
    assert_eq!(params[0].0, "{x, y}");
    assert_eq!(params[1].0, "[a, b]");
}

// ============================================================================
// extract_jsdoc_comments tests
// ============================================================================

#[test]
fn test_extract_jsdoc_comments_simple() {
    let source = r#"
/** This is a JSDoc comment */
function foo() {}
"#;
    let tree = parse_js(source);
    let func_node = find_node(tree.root_node(), "function_declaration")
        .expect("Should find function_declaration");

    let doc = extract_jsdoc_comments(func_node, source);
    assert!(doc.is_some());
    assert!(doc.unwrap().contains("This is a JSDoc comment"));
}

#[test]
fn test_extract_jsdoc_comments_multiline() {
    let source = r#"
/**
 * A multiline JSDoc comment
 * @param x - The x parameter
 * @returns The result
 */
function foo(x) {}
"#;
    let tree = parse_js(source);
    let func_node = find_node(tree.root_node(), "function_declaration")
        .expect("Should find function_declaration");

    let doc = extract_jsdoc_comments(func_node, source);
    assert!(doc.is_some());
    let doc_text = doc.unwrap();
    assert!(doc_text.contains("multiline JSDoc comment"));
    assert!(doc_text.contains("@param"));
    assert!(doc_text.contains("@returns"));
}

#[test]
fn test_extract_jsdoc_comments_none() {
    let source = "function foo() {}";
    let tree = parse_js(source);
    let func_node = find_node(tree.root_node(), "function_declaration")
        .expect("Should find function_declaration");

    let doc = extract_jsdoc_comments(func_node, source);
    assert!(doc.is_none());
}

#[test]
fn test_extract_jsdoc_comments_regular_comment_ignored() {
    let source = r#"
// This is a regular comment
function foo() {}
"#;
    let tree = parse_js(source);
    let func_node = find_node(tree.root_node(), "function_declaration")
        .expect("Should find function_declaration");

    let doc = extract_jsdoc_comments(func_node, source);
    assert!(doc.is_none());
}

// ============================================================================
// extract_type_references_from_jsdoc tests
// ============================================================================

#[test]
fn test_extract_type_references_simple() {
    let jsdoc = "@param {User} user - The user object";
    let import_map = ImportMap::new(".");

    let types = extract_type_references_from_jsdoc(Some(jsdoc), &import_map, None);
    assert!(types.iter().any(|t| t.contains("User")));
}

#[test]
fn test_extract_type_references_union() {
    let jsdoc = "@param {User|Admin} person - A person";
    let import_map = ImportMap::new(".");

    let types = extract_type_references_from_jsdoc(Some(jsdoc), &import_map, None);
    assert!(types.iter().any(|t| t.contains("User")));
    assert!(types.iter().any(|t| t.contains("Admin")));
}

#[test]
fn test_extract_type_references_generic() {
    let jsdoc = "@returns {Array<User>} List of users";
    let import_map = ImportMap::new(".");

    let types = extract_type_references_from_jsdoc(Some(jsdoc), &import_map, None);
    // Array is primitive, User is not
    assert!(types.iter().any(|t| t.contains("User")));
}

#[test]
fn test_extract_type_references_filters_primitives() {
    let jsdoc = "@param {string} name - The name";
    let import_map = ImportMap::new(".");

    let types = extract_type_references_from_jsdoc(Some(jsdoc), &import_map, None);
    // string is primitive, should be filtered out
    assert!(types.is_empty());
}

#[test]
fn test_extract_type_references_none() {
    let import_map = ImportMap::new(".");
    let types = extract_type_references_from_jsdoc(None, &import_map, None);
    assert!(types.is_empty());
}

// ============================================================================
// extract_function_calls tests
// ============================================================================

#[test]
fn test_extract_function_calls_bare() {
    let source = r#"
function foo() {
    bar();
    baz();
}
"#;
    let tree = parse_js(source);
    let func_node = find_node(tree.root_node(), "function_declaration")
        .expect("Should find function_declaration");
    let import_map = ImportMap::new(".");

    let calls = extract_function_calls(func_node, source, &import_map, None);
    assert!(calls.iter().any(|c| c.target.contains("bar")));
    assert!(calls.iter().any(|c| c.target.contains("baz")));
}

#[test]
fn test_extract_function_calls_method() {
    let source = r#"
function foo() {
    obj.method();
    console.log();
}
"#;
    let tree = parse_js(source);
    let func_node = find_node(tree.root_node(), "function_declaration")
        .expect("Should find function_declaration");
    let import_map = ImportMap::new(".");

    let calls = extract_function_calls(func_node, source, &import_map, None);
    assert!(calls.iter().any(|c| c.target.contains("method")));
    assert!(calls.iter().any(|c| c.target.contains("log")));
}

#[test]
fn test_extract_function_calls_dedup() {
    let source = r#"
function foo() {
    bar();
    bar();
    bar();
}
"#;
    let tree = parse_js(source);
    let func_node = find_node(tree.root_node(), "function_declaration")
        .expect("Should find function_declaration");
    let import_map = ImportMap::new(".");

    let calls = extract_function_calls(func_node, source, &import_map, None);
    // Should only have one entry for bar
    let bar_count = calls.iter().filter(|c| c.target.contains("bar")).count();
    assert_eq!(bar_count, 1);
}
