//! JavaScript and TypeScript shared utilities
//!
//! This module contains functions shared between JavaScript and TypeScript
//! entity extraction, including parameter extraction, JSDoc parsing,
//! function call extraction, and type reference extraction.

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
const JS_FUNCTION_CALLS_QUERY_SOURCE: &str = r#"
    (call_expression
      function: (identifier) @bare_callee)

    (call_expression
      function: (member_expression
        object: (identifier) @receiver
        property: (property_identifier) @method))
"#;

/// Cached tree-sitter query for function call extraction
static JS_FUNCTION_CALLS_QUERY: OnceLock<Option<Query>> = OnceLock::new();

/// Get or initialize the cached function calls query
fn js_function_calls_query() -> Option<&'static Query> {
    JS_FUNCTION_CALLS_QUERY
        .get_or_init(|| {
            let language = tree_sitter_javascript::LANGUAGE.into();
            Query::new(&language, JS_FUNCTION_CALLS_QUERY_SOURCE).ok()
        })
        .as_ref()
}

/// Extract parameters from a formal_parameters node (JavaScript-style)
///
/// This function handles JavaScript parameter patterns including:
/// - Simple identifiers: `function foo(a, b) {}`
/// - Default parameters: `function foo(a = 1) {}`
/// - Rest parameters: `function foo(...args) {}`
/// - Destructuring: `function foo({x, y}) {}`
pub fn extract_parameters(
    params_node: Node,
    source: &str,
) -> Result<Vec<(String, Option<String>)>> {
    let mut parameters = Vec::new();

    for child in params_node.named_children(&mut params_node.walk()) {
        match child.kind() {
            "identifier" => {
                let param_name = node_to_text(child, source)?;
                parameters.push((param_name, None));
            }
            "assignment_pattern" => {
                // Handle default parameters
                if let Some(name_node) = child.child_by_field_name("left") {
                    let param_name = node_to_text(name_node, source)?;
                    parameters.push((param_name, None));
                }
            }
            "rest_pattern" => {
                // Handle rest parameters (...args)
                if let Some(name_node) = child.named_child(0) {
                    let param_name = format!("...{}", node_to_text(name_node, source)?);
                    parameters.push((param_name, None));
                }
            }
            "object_pattern" | "array_pattern" => {
                // Handle destructuring parameters
                let param_text = node_to_text(child, source)?;
                parameters.push((param_text, None));
            }
            _ => {}
        }
    }

    Ok(parameters)
}

/// Extract JSDoc comments preceding a node
///
/// This function walks backward from the given node to find JSDoc-style
/// comments (/** ... */) and extracts their content.
pub fn extract_jsdoc_comments(node: Node, source: &str) -> Option<String> {
    let mut doc_lines = Vec::new();
    let mut current = node.prev_sibling();

    while let Some(sibling) = current {
        if sibling.kind() == "comment" {
            if let Ok(text) = node_to_text(sibling, source) {
                if text.starts_with("/**") && text.ends_with("*/") {
                    // Extract JSDoc content
                    let content = text
                        .trim_start_matches("/**")
                        .trim_end_matches("*/")
                        .lines()
                        .map(|line| line.trim().trim_start_matches('*').trim())
                        .filter(|line| !line.is_empty())
                        .collect::<Vec<_>>()
                        .join("\n");
                    doc_lines.push(content);
                    break;
                }
            }
        } else if sibling.kind() != "expression_statement" {
            break;
        }
        current = sibling.prev_sibling();
    }

    if doc_lines.is_empty() {
        None
    } else {
        Some(doc_lines.join("\n"))
    }
}

// ============================================================================
// Function Call Extraction
// ============================================================================

/// Extract function calls from a function body using tree-sitter queries
///
/// This extracts:
/// - Bare function calls: `foo()`
/// - Member expression calls: `obj.method()`
///
/// Returns a list of `SourceReference` with resolved qualified names and locations.
pub fn extract_function_calls(
    function_node: Node,
    source: &str,
    import_map: &ImportMap,
    parent_scope: Option<&str>,
) -> Vec<SourceReference> {
    let Some(query) = js_function_calls_query() else {
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
            // For JS, we can't easily resolve the receiver type, so we store as "receiver.method"
            if let (Ok(recv_name), Ok(method_name)) = (
                node_to_text(recv_cap.node, source),
                node_to_text(method_cap.node, source),
            ) {
                // Try to resolve receiver through imports (e.g., imported module)
                let resolved_recv = resolve_reference(&recv_name, import_map, parent_scope, ".");
                let call_ref = format!("{resolved_recv}.{method_name}");
                if seen.insert(call_ref.clone()) {
                    // Use method node for the location (more specific than receiver)
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
// Type Reference Extraction from JSDoc
// ============================================================================

/// Extract type references from JSDoc comments
///
/// Parses `@param {Type}`, `@returns {Type}`, `@type {Type}` patterns
/// and extracts the type names, filtering out primitives.
///
/// Returns `SourceReference` with default locations since JSDoc is parsed from text,
/// not AST nodes. The location field will have default (0,0) positions.
pub fn extract_type_references_from_jsdoc(
    jsdoc: Option<&str>,
    import_map: &ImportMap,
    parent_scope: Option<&str>,
) -> Vec<SourceReference> {
    let Some(doc) = jsdoc else {
        return Vec::new();
    };

    let mut type_refs = Vec::new();
    let mut seen = HashSet::new();

    // Match patterns like {Type}, {Type|OtherType}, {Array<Type>}
    // Simple regex-like parsing for type annotations in braces
    let mut in_braces = false;
    let mut current_type = String::new();

    for ch in doc.chars() {
        match ch {
            '{' => {
                in_braces = true;
                current_type.clear();
            }
            '}' => {
                if in_braces {
                    // Parse the type string
                    extract_types_from_jsdoc_string(
                        &current_type,
                        &mut type_refs,
                        &mut seen,
                        import_map,
                        parent_scope,
                    );
                    in_braces = false;
                }
            }
            _ if in_braces => {
                current_type.push(ch);
            }
            _ => {}
        }
    }

    type_refs
}

/// Parse individual type names from a JSDoc type string like "Type|OtherType" or "Array<Type>"
fn extract_types_from_jsdoc_string(
    type_str: &str,
    type_refs: &mut Vec<SourceReference>,
    seen: &mut HashSet<String>,
    import_map: &ImportMap,
    parent_scope: Option<&str>,
) {
    // Split by | for union types, and handle generics
    for part in type_str.split('|') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }

        // Handle generic types like Array<T> or Map<K, V>
        if let Some(angle_pos) = part.find('<') {
            // Extract base type
            let base_type = part[..angle_pos].trim();
            if !is_js_primitive(base_type) && !base_type.is_empty() {
                let resolved = resolve_reference(base_type, import_map, parent_scope, ".");
                if seen.insert(resolved.clone()) {
                    type_refs.push(SourceReference {
                        target: resolved,
                        location: SourceLocation::default(),
                        ref_type: ReferenceType::TypeUsage,
                    });
                }
            }

            // Extract generic parameters
            if let Some(close_pos) = part.rfind('>') {
                let generics = &part[angle_pos + 1..close_pos];
                for generic_part in generics.split(',') {
                    let generic_type = generic_part.trim();
                    if !is_js_primitive(generic_type) && !generic_type.is_empty() {
                        let resolved =
                            resolve_reference(generic_type, import_map, parent_scope, ".");
                        if seen.insert(resolved.clone()) {
                            type_refs.push(SourceReference {
                                target: resolved,
                                location: SourceLocation::default(),
                                ref_type: ReferenceType::TypeUsage,
                            });
                        }
                    }
                }
            }
        } else {
            // Simple type
            if !is_js_primitive(part) {
                let resolved = resolve_reference(part, import_map, parent_scope, ".");
                if seen.insert(resolved.clone()) {
                    type_refs.push(SourceReference {
                        target: resolved,
                        location: SourceLocation::default(),
                        ref_type: ReferenceType::TypeUsage,
                    });
                }
            }
        }
    }
}

/// Check if a type name is a JavaScript primitive type
pub fn is_js_primitive(name: &str) -> bool {
    name.eq_ignore_ascii_case("string")
        || name.eq_ignore_ascii_case("number")
        || name.eq_ignore_ascii_case("boolean")
        || name.eq_ignore_ascii_case("object")
        || name.eq_ignore_ascii_case("any")
        || name.eq_ignore_ascii_case("void")
        || name.eq_ignore_ascii_case("null")
        || name.eq_ignore_ascii_case("undefined")
        || name.eq_ignore_ascii_case("symbol")
        || name.eq_ignore_ascii_case("bigint")
        || name.eq_ignore_ascii_case("never")
        || name.eq_ignore_ascii_case("array")
        || name.eq_ignore_ascii_case("function")
        || name.eq_ignore_ascii_case("promise")
        || name == "*"
}
