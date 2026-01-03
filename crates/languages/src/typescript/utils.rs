//! TypeScript-specific utilities
//!
//! This module contains TypeScript-specific extraction functions for
//! type references from type annotations.

use crate::common::import_map::{resolve_reference, ImportMap};
use crate::common::node_to_text;
use codesearch_core::entities::{ReferenceType, SourceLocation, SourceReference};
use std::collections::HashSet;
use std::sync::OnceLock;
use streaming_iterator::StreamingIterator;
use tree_sitter::{Node, Query, QueryCursor};

// ============================================================================
// Cached Tree-Sitter Queries
// ============================================================================

/// Query source for extracting type references
const TS_TYPE_REFS_QUERY_SOURCE: &str = r#"
    ; Type identifiers
    (type_identifier) @type_ref

    ; Nested type identifiers (qualified types like Foo.Bar)
    (nested_type_identifier) @scoped_type_ref
"#;

/// Cached tree-sitter query for type reference extraction
static TS_TYPE_REFS_QUERY: OnceLock<Query> = OnceLock::new();

/// Get or initialize the cached type references query.
/// Panics if the query fails to compile - this is a programmer error.
#[allow(clippy::expect_used)] // Query compilation failure is a programmer error
fn ts_type_refs_query() -> &'static Query {
    TS_TYPE_REFS_QUERY.get_or_init(|| {
        let language = tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();
        Query::new(&language, TS_TYPE_REFS_QUERY_SOURCE)
            .expect("TS_TYPE_REFS_QUERY_SOURCE should be a valid tree-sitter query")
    })
}

/// Extract the simple name from a nested_type_identifier node using AST traversal.
///
/// A nested_type_identifier like `Foo.Bar.Baz` has structure:
///   nested_type_identifier
///     scope: nested_type_identifier (or type_identifier)
///     name: type_identifier <- this is what we want
///
/// We find the rightmost type_identifier child (the "name" field).
fn extract_simple_name_from_nested_type<'a>(node: Node<'a>, source: &'a str) -> Option<String> {
    // The "name" field is the last type_identifier child
    for i in (0..node.child_count()).rev() {
        if let Some(child) = node.child(i) {
            if child.kind() == "type_identifier" {
                return node_to_text(child, source).ok();
            }
        }
    }
    // Fallback: use the full text
    node_to_text(node, source).ok()
}

/// Extract type references from TypeScript type annotations
///
/// This extracts type identifiers from:
/// - Parameter type annotations
/// - Return type annotations
/// - Variable type annotations
/// - Generic type arguments
///
/// Returns a list of `SourceReference` with resolved qualified names and locations.
pub fn extract_type_references(
    function_node: Node,
    source: &str,
    import_map: &ImportMap,
    parent_scope: Option<&str>,
) -> Vec<SourceReference> {
    let query = ts_type_refs_query();
    let mut cursor = QueryCursor::new();
    let mut type_refs = Vec::new();
    let mut seen = HashSet::new();

    let mut matches = cursor.matches(query, function_node, source.as_bytes());
    while let Some(query_match) = matches.next() {
        for capture in query_match.captures {
            let capture_name = query
                .capture_names()
                .get(capture.index as usize)
                .copied()
                .unwrap_or("");

            match capture_name {
                "type_ref" => {
                    if let Ok(type_name) = node_to_text(capture.node, source) {
                        // Skip TypeScript primitive types
                        if is_ts_primitive(&type_name) {
                            continue;
                        }
                        // Resolve through imports
                        let resolved = resolve_reference(&type_name, import_map, parent_scope, ".");
                        if seen.insert(resolved.clone()) {
                            if let Ok(source_ref) = SourceReference::builder()
                                .target(resolved)
                                .simple_name(type_name) // simple_name from AST node
                                .is_external(false) // TS doesn't track external refs
                                .location(SourceLocation::from_tree_sitter_node(capture.node))
                                .ref_type(ReferenceType::TypeUsage)
                                .build()
                            {
                                type_refs.push(source_ref);
                            }
                        }
                    }
                }
                "scoped_type_ref" => {
                    if let Ok(full_path) = node_to_text(capture.node, source) {
                        // Scoped types are already qualified
                        // Extract simple name from AST (last type_identifier child)
                        let simple_name =
                            extract_simple_name_from_nested_type(capture.node, source)
                                .unwrap_or_else(|| full_path.clone());
                        if seen.insert(full_path.clone()) {
                            if let Ok(source_ref) = SourceReference::builder()
                                .target(full_path)
                                .simple_name(simple_name)
                                .is_external(false) // TS doesn't track external refs
                                .location(SourceLocation::from_tree_sitter_node(capture.node))
                                .ref_type(ReferenceType::TypeUsage)
                                .build()
                            {
                                type_refs.push(source_ref);
                            }
                        }
                    }
                }
                _ => {
                    tracing::trace!(
                        kind = capture_name,
                        "Unhandled capture name in type refs query"
                    );
                }
            }
        }
    }

    type_refs
}

// ============================================================================
// Call Reference Extraction
// ============================================================================

/// Query source for extracting function call references
const TS_CALL_REFS_QUERY_SOURCE: &str = r#"
    ; Direct function call: foo()
    (call_expression
      function: (identifier) @bare_callee)

    ; Member access call: obj.method()
    (call_expression
      function: (member_expression
        object: (_) @receiver
        property: (property_identifier) @method))
"#;

/// Cached tree-sitter query for call reference extraction
static TS_CALL_REFS_QUERY: OnceLock<Query> = OnceLock::new();

/// Get or initialize the cached call references query.
/// Panics if the query fails to compile - this is a programmer error.
#[allow(clippy::expect_used)] // Query compilation failure is a programmer error
fn ts_call_refs_query() -> &'static Query {
    TS_CALL_REFS_QUERY.get_or_init(|| {
        let language = tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();
        Query::new(&language, TS_CALL_REFS_QUERY_SOURCE)
            .expect("TS_CALL_REFS_QUERY_SOURCE should be a valid tree-sitter query")
    })
}

/// Extract function call references from a TypeScript function body
///
/// This extracts function call sites from:
/// - Direct calls: `foo()`, `myFunction(x, y)`
/// - Member expression calls (only module-level): `Module.func()`
///
/// Note: Method calls on instance receivers (like `this.method()` or `obj.method()`)
/// are currently not fully resolved as we'd need type inference.
///
/// Returns a list of `SourceReference` with resolved qualified names and locations.
pub fn extract_call_references(
    function_node: Node,
    source: &str,
    import_map: &ImportMap,
    parent_scope: Option<&str>,
) -> Vec<SourceReference> {
    let query = ts_call_refs_query();
    let mut cursor = QueryCursor::new();
    let mut calls = Vec::new();
    let mut seen = HashSet::new();

    let mut matches = cursor.matches(query, function_node, source.as_bytes());
    while let Some(query_match) = matches.next() {
        // Collect captures by name
        let mut bare_callee = None;
        let mut _receiver = None;
        let mut _method = None;

        for capture in query_match.captures {
            let capture_name = query
                .capture_names()
                .get(capture.index as usize)
                .copied()
                .unwrap_or("");

            match capture_name {
                "bare_callee" => bare_callee = Some(capture.node),
                "receiver" => _receiver = Some(capture.node),
                "method" => _method = Some(capture.node),
                _ => {}
            }
        }

        // Handle bare function calls: `foo()`, `fetchData(url)`
        if let Some(callee_node) = bare_callee {
            if let Ok(name) = node_to_text(callee_node, source) {
                // Skip built-in functions and common globals
                if is_builtin_function(&name) {
                    continue;
                }

                // Resolve through imports and scope
                let resolved = resolve_reference(&name, import_map, parent_scope, ".");

                if seen.insert(resolved.clone()) {
                    if let Ok(source_ref) = SourceReference::builder()
                        .target(resolved)
                        .simple_name(name)
                        .is_external(false)
                        .location(SourceLocation::from_tree_sitter_node(callee_node))
                        .ref_type(ReferenceType::Call)
                        .build()
                    {
                        calls.push(source_ref);
                    }
                }
            }
        }

        // Note: We're not currently handling method calls like `obj.method()` or `this.method()`
        // because that would require type inference to resolve the receiver's type.
        // Module.func() calls are handled through imports (they appear as bare identifiers).
    }

    calls
}

/// Check if a function name is a JavaScript/TypeScript built-in
fn is_builtin_function(name: &str) -> bool {
    matches!(
        name,
        "console"
            | "fetch"
            | "setTimeout"
            | "setInterval"
            | "clearTimeout"
            | "clearInterval"
            | "parseInt"
            | "parseFloat"
            | "isNaN"
            | "isFinite"
            | "encodeURI"
            | "decodeURI"
            | "encodeURIComponent"
            | "decodeURIComponent"
            | "JSON"
            | "Math"
            | "Date"
            | "Array"
            | "Object"
            | "String"
            | "Number"
            | "Boolean"
            | "Symbol"
            | "BigInt"
            | "RegExp"
            | "Error"
            | "Promise"
            | "require"
            | "import"
    )
}

/// Find the variable name from a parent variable_declarator node.
///
/// When a function or class expression is assigned to a variable, this traverses
/// up the AST to find the variable name. For example:
///   `const onClick = function() {}` -> returns "onClick"
///   `const MyClass = class {}` -> returns "MyClass"
///
/// Returns `None` if no parent variable_declarator is found before hitting
/// a program or class_body boundary.
pub fn find_parent_variable_name(node: Node, source: &str) -> Option<String> {
    let mut current = node.parent();
    while let Some(parent) = current {
        if parent.kind() == "variable_declarator" {
            if let Some(name_node) = parent.child_by_field_name("name") {
                if name_node.kind() == "identifier" {
                    return node_to_text(name_node, source).ok();
                }
            }
        }
        // Stop if we hit something that shouldn't contain a variable declarator
        if parent.kind() == "program" || parent.kind() == "class_body" {
            break;
        }
        current = parent.parent();
    }
    None
}

/// Check if a type name is a TypeScript primitive type
pub fn is_ts_primitive(name: &str) -> bool {
    name.eq_ignore_ascii_case("string")
        || name.eq_ignore_ascii_case("number")
        || name.eq_ignore_ascii_case("boolean")
        || name.eq_ignore_ascii_case("any")
        || name.eq_ignore_ascii_case("never")
        || name.eq_ignore_ascii_case("void")
        || name.eq_ignore_ascii_case("object")
        || name.eq_ignore_ascii_case("null")
        || name.eq_ignore_ascii_case("undefined")
        || name.eq_ignore_ascii_case("symbol")
        || name.eq_ignore_ascii_case("bigint")
        || name.eq_ignore_ascii_case("unknown")
        || name.eq_ignore_ascii_case("array")
        || name.eq_ignore_ascii_case("function")
        || name.eq_ignore_ascii_case("promise")
        || name.eq_ignore_ascii_case("readonly")
        || name.eq_ignore_ascii_case("record")
        || name.eq_ignore_ascii_case("partial")
        || name.eq_ignore_ascii_case("required")
        || name.eq_ignore_ascii_case("pick")
        || name.eq_ignore_ascii_case("omit")
        || name.eq_ignore_ascii_case("exclude")
        || name.eq_ignore_ascii_case("extract")
        || name.eq_ignore_ascii_case("returntype")
        || name.eq_ignore_ascii_case("parameters")
}
