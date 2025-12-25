//! Python-specific shared utilities for entity extraction

use crate::common::import_map::{resolve_reference, ImportMap};
use crate::common::node_to_text;
use codesearch_core::entities::{ReferenceType, SourceLocation, SourceReference};
use codesearch_core::error::Result;
use std::collections::HashSet;
use std::sync::OnceLock;
use streaming_iterator::StreamingIterator;
use tree_sitter::{Node, Query, QueryCursor};

// ============================================================================
// Cached Tree-Sitter Queries
// ============================================================================

/// Query source for extracting function calls
const PYTHON_FUNCTION_CALLS_QUERY_SOURCE: &str = r#"
    (call
      function: (identifier) @bare_callee)

    (call
      function: (attribute
        object: (identifier) @receiver
        attribute: (identifier) @method))
"#;

/// Query source for extracting type references
const PYTHON_TYPE_REFS_QUERY_SOURCE: &str = r#"
    ; Type identifiers in annotations
    (type (identifier) @type_ref)

    ; Subscript types like List[T], Dict[K, V]
    (type (subscript
      value: (identifier) @subscript_type))

    ; Attribute types like typing.Optional
    (type (attribute
      object: (identifier) @module
      attribute: (identifier) @attr_type))
"#;

/// Cached tree-sitter query for function call extraction
static PYTHON_FUNCTION_CALLS_QUERY: OnceLock<Option<Query>> = OnceLock::new();

/// Cached tree-sitter query for type reference extraction
static PYTHON_TYPE_REFS_QUERY: OnceLock<Option<Query>> = OnceLock::new();

/// Get or initialize the cached function calls query
fn python_function_calls_query() -> Option<&'static Query> {
    PYTHON_FUNCTION_CALLS_QUERY
        .get_or_init(|| {
            let language = tree_sitter_python::LANGUAGE.into();
            Query::new(&language, PYTHON_FUNCTION_CALLS_QUERY_SOURCE).ok()
        })
        .as_ref()
}

/// Get or initialize the cached type references query
fn python_type_refs_query() -> Option<&'static Query> {
    PYTHON_TYPE_REFS_QUERY
        .get_or_init(|| {
            let language = tree_sitter_python::LANGUAGE.into();
            Query::new(&language, PYTHON_TYPE_REFS_QUERY_SOURCE).ok()
        })
        .as_ref()
}

/// Extract parameters from a Python parameters node
///
/// Handles:
/// - Simple identifiers (self, name)
/// - Typed parameters (name: Type)
/// - Default parameters (name=value)
/// - Typed default parameters (name: Type = value)
/// - Variadic (*args, **kwargs)
/// - Positional-only separator (/) - Python 3.8+
/// - Keyword-only marker (*) - Python 3.0+
pub fn extract_python_parameters(
    params_node: Node,
    source: &str,
) -> Result<Vec<(String, Option<String>)>> {
    let mut parameters = Vec::new();

    for child in params_node.named_children(&mut params_node.walk()) {
        match child.kind() {
            "identifier" => {
                let name = node_to_text(child, source)?;
                parameters.push((name, None));
            }
            "typed_parameter" => {
                // In tree-sitter-python, typed_parameter has identifier as first child
                // and type annotation as the "type" field
                if let Some(name) = child.named_child(0).and_then(|n| {
                    if n.kind() == "identifier" {
                        node_to_text(n, source).ok()
                    } else {
                        None
                    }
                }) {
                    let type_hint = child
                        .child_by_field_name("type")
                        .and_then(|n| node_to_text(n, source).ok());
                    parameters.push((name, type_hint));
                }
            }
            "default_parameter" => {
                // default_parameter structure: identifier = value
                if let Some(name) = child.named_child(0).and_then(|n| {
                    if n.kind() == "identifier" {
                        node_to_text(n, source).ok()
                    } else {
                        None
                    }
                }) {
                    parameters.push((name, None));
                }
            }
            "typed_default_parameter" => {
                // typed_default_parameter structure: identifier : type = value
                if let Some(name) = child.named_child(0).and_then(|n| {
                    if n.kind() == "identifier" {
                        node_to_text(n, source).ok()
                    } else {
                        None
                    }
                }) {
                    let type_hint = child
                        .child_by_field_name("type")
                        .and_then(|n| node_to_text(n, source).ok());
                    parameters.push((name, type_hint));
                }
            }
            "list_splat_pattern" => {
                let name = child
                    .named_child(0)
                    .and_then(|n| node_to_text(n, source).ok())
                    .map(|n| format!("*{n}"))
                    .unwrap_or_else(|| "*args".to_string());
                parameters.push((name, None));
            }
            "dictionary_splat_pattern" => {
                let name = child
                    .named_child(0)
                    .and_then(|n| node_to_text(n, source).ok())
                    .map(|n| format!("**{n}"))
                    .unwrap_or_else(|| "**kwargs".to_string());
                parameters.push((name, None));
            }
            // Python 3.8+ positional-only separator: def f(a, /, b)
            // Parameters before / are positional-only, represented as "/" marker
            "positional_separator" => {
                parameters.push(("/".to_string(), None));
            }
            // Python 3.0+ keyword-only marker: def f(*, a, b)
            // A bare * without a name indicates keyword-only parameters follow
            "keyword_separator" => {
                parameters.push(("*".to_string(), None));
            }
            _ => {}
        }
    }

    Ok(parameters)
}

/// Extract docstring from a function or class body
///
/// Python docstrings are the first expression in the body if it's a string literal.
pub fn extract_docstring(node: Node, source: &str) -> Option<String> {
    let body = node.child_by_field_name("body")?;
    let first_stmt = body.named_child(0)?;

    if first_stmt.kind() == "expression_statement" {
        let expr = first_stmt.named_child(0)?;
        if expr.kind() == "string" {
            let text = node_to_text(expr, source).ok()?;
            return Some(normalize_docstring(&text));
        }
    }

    None
}

/// Normalize a docstring by stripping quotes and whitespace
fn normalize_docstring(text: &str) -> String {
    text.trim_start_matches("\"\"\"")
        .trim_start_matches("'''")
        .trim_start_matches('"')
        .trim_start_matches('\'')
        .trim_end_matches("\"\"\"")
        .trim_end_matches("'''")
        .trim_end_matches('"')
        .trim_end_matches('\'')
        .trim()
        .to_string()
}

/// Extract decorators from a function or class definition
///
/// Python decorators appear on the parent `decorated_definition` node.
pub fn extract_decorators(node: Node, source: &str) -> Vec<String> {
    let mut decorators = Vec::new();

    // Check if parent is a decorated_definition
    if let Some(parent) = node.parent() {
        if parent.kind() == "decorated_definition" {
            for child in parent.named_children(&mut parent.walk()) {
                if child.kind() == "decorator" {
                    if let Ok(text) = node_to_text(child, source) {
                        let decorator = text.trim_start_matches('@').trim().to_string();
                        decorators.push(decorator);
                    }
                }
            }
        }
    }

    decorators
}

/// Check if a function node is async
pub fn is_async_function(node: Node) -> bool {
    for child in node.children(&mut node.walk()) {
        if child.kind() == "async" {
            return true;
        }
        // Stop after reaching the function keyword
        if child.kind() == "def" {
            break;
        }
    }
    false
}

/// Extract base classes from a class definition
pub fn extract_base_classes(node: Node, source: &str) -> Vec<String> {
    let mut bases = Vec::new();

    if let Some(superclasses) = node.child_by_field_name("superclasses") {
        for child in superclasses.named_children(&mut superclasses.walk()) {
            if let Ok(text) = node_to_text(child, source) {
                bases.push(text);
            }
        }
    }

    bases
}

/// Extract return type annotation from a function
pub fn extract_return_type(node: Node, source: &str) -> Option<String> {
    node.child_by_field_name("return_type")
        .and_then(|n| node_to_text(n, source).ok())
}

/// Filter self/cls from method parameters for display
pub fn filter_self_parameter(
    params: Vec<(String, Option<String>)>,
) -> Vec<(String, Option<String>)> {
    params
        .into_iter()
        .filter(|(name, _)| name != "self" && name != "cls")
        .collect()
}

// ============================================================================
// Function Call Extraction
// ============================================================================

/// Extract function calls from a function body using tree-sitter queries
///
/// This extracts:
/// - Bare function calls: `foo()`
/// - Attribute calls: `obj.method()`
///
/// Returns SourceReferences with location data for disambiguation.
pub fn extract_function_calls(
    function_node: Node,
    source: &str,
    import_map: &ImportMap,
    parent_scope: Option<&str>,
) -> Vec<SourceReference> {
    let Some(query) = python_function_calls_query() else {
        return Vec::new();
    };

    let mut cursor = QueryCursor::new();
    let mut calls = Vec::new();
    let mut seen = HashSet::new();

    let mut matches = cursor.matches(query, function_node, source.as_bytes());
    while let Some(query_match) = matches.next() {
        let captures: Vec<_> = query_match.captures.iter().collect();

        // Check for bare callee (identifier)
        let bare_callee = captures
            .iter()
            .find(|c| query.capture_names().get(c.index as usize).copied() == Some("bare_callee"));

        // Check for method call (receiver.method())
        let receiver = captures
            .iter()
            .find(|c| query.capture_names().get(c.index as usize).copied() == Some("receiver"));
        let method = captures
            .iter()
            .find(|c| query.capture_names().get(c.index as usize).copied() == Some("method"));

        if let Some(bare_cap) = bare_callee {
            // Bare identifier call like `foo()`
            if let Ok(name) = node_to_text(bare_cap.node, source) {
                let resolved = resolve_reference(&name, import_map, parent_scope, ".");
                if seen.insert(resolved.clone()) {
                    calls.push(SourceReference {
                        target: resolved,
                        location: SourceLocation::from_tree_sitter_node(bare_cap.node),
                        ref_type: ReferenceType::Call,
                    });
                }
            }
        } else if let (Some(recv_cap), Some(method_cap)) = (receiver, method) {
            // Method call like `obj.method()`
            if let (Ok(recv_name), Ok(method_name)) = (
                node_to_text(recv_cap.node, source),
                node_to_text(method_cap.node, source),
            ) {
                // Try to resolve receiver through imports (e.g., imported module)
                let resolved_recv = resolve_reference(&recv_name, import_map, parent_scope, ".");
                let call_ref = format!("{resolved_recv}.{method_name}");
                if seen.insert(call_ref.clone()) {
                    calls.push(SourceReference {
                        target: call_ref,
                        location: SourceLocation::from_tree_sitter_node(method_cap.node),
                        ref_type: ReferenceType::Call,
                    });
                }
            }
        }
    }

    calls
}

// ============================================================================
// Type Reference Extraction from Type Hints
// ============================================================================

/// Extract type references from Python type hints
///
/// This extracts type identifiers from:
/// - Parameter type annotations
/// - Return type annotations
/// - Variable annotations
///
/// Returns SourceReferences with location data for disambiguation.
pub fn extract_type_references(
    function_node: Node,
    source: &str,
    import_map: &ImportMap,
    parent_scope: Option<&str>,
) -> Vec<SourceReference> {
    let Some(query) = python_type_refs_query() else {
        return Vec::new();
    };

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
                "type_ref" | "subscript_type" => {
                    if let Ok(type_name) = node_to_text(capture.node, source) {
                        // Skip Python primitive types
                        if is_python_primitive(&type_name) {
                            continue;
                        }
                        // Resolve through imports
                        let resolved = resolve_reference(&type_name, import_map, parent_scope, ".");
                        if seen.insert(resolved.clone()) {
                            type_refs.push(SourceReference {
                                target: resolved,
                                location: SourceLocation::from_tree_sitter_node(capture.node),
                                ref_type: ReferenceType::TypeUsage,
                            });
                        }
                    }
                }
                "attr_type" => {
                    // For attribute types like typing.Optional, we might want the full path
                    // But for now, just extract the attribute name
                    if let Ok(type_name) = node_to_text(capture.node, source) {
                        if !is_python_primitive(&type_name) {
                            let resolved =
                                resolve_reference(&type_name, import_map, parent_scope, ".");
                            if seen.insert(resolved.clone()) {
                                type_refs.push(SourceReference {
                                    target: resolved,
                                    location: SourceLocation::from_tree_sitter_node(capture.node),
                                    ref_type: ReferenceType::TypeUsage,
                                });
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    type_refs
}

/// Check if a type name is a Python primitive/builtin type
pub fn is_python_primitive(name: &str) -> bool {
    matches!(
        name,
        "str"
            | "int"
            | "float"
            | "bool"
            | "bytes"
            | "None"
            | "list"
            | "dict"
            | "tuple"
            | "set"
            | "frozenset"
            | "type"
            | "Any"
            | "object"
            | "List"
            | "Dict"
            | "Tuple"
            | "Set"
            | "FrozenSet"
            | "Optional"
            | "Union"
            | "Callable"
            | "Type"
            | "Sequence"
            | "Mapping"
            | "Iterable"
            | "Iterator"
            | "Generator"
            | "Coroutine"
            | "AsyncIterator"
            | "AsyncGenerator"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_docstring_triple_double() {
        assert_eq!(
            normalize_docstring("\"\"\"Hello world\"\"\""),
            "Hello world"
        );
    }

    #[test]
    fn test_normalize_docstring_triple_single() {
        assert_eq!(normalize_docstring("'''Hello world'''"), "Hello world");
    }

    #[test]
    fn test_normalize_docstring_with_whitespace() {
        assert_eq!(
            normalize_docstring("\"\"\"  \n  Hello world  \n  \"\"\""),
            "Hello world"
        );
    }
}
