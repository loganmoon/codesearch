//! Common utilities shared between handler modules
//!
//! This module provides shared functionality for AST traversal,
//! text extraction, and documentation processing.

#![deny(warnings)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]

use crate::rust::handler_impls::constants::{
    capture_names, doc_prefixes, node_kinds, punctuation, visibility_keywords,
};
use codesearch_core::entities::Visibility;
use codesearch_core::error::Result;
use streaming_iterator::StreamingIterator;
use tree_sitter::{Node, Query, QueryMatch};

// Import shared utilities from common module
pub use crate::common::{find_capture_node, node_to_text, require_capture_node};

// ============================================================================
// Node Finding and Text Extraction
// ============================================================================
// The find_capture_node, node_to_text, and require_capture_node functions
// have been moved to the shared crate::common module and are re-exported here
// for backwards compatibility.

// ============================================================================
// Visibility Extraction
// ============================================================================

/// Extract visibility from a captured visibility modifier node
pub fn extract_visibility(query_match: &QueryMatch, query: &Query) -> Visibility {
    let Some(vis_node) = find_capture_node(query_match, query, capture_names::VIS) else {
        return Visibility::Private;
    };

    // Check if this is a visibility_modifier node
    if vis_node.kind() != node_kinds::VISIBILITY_MODIFIER {
        return Visibility::Private;
    }

    // Walk through the visibility modifier's children
    let mut cursor = vis_node.walk();
    let has_public_keyword = vis_node.children(&mut cursor).any(|child| {
        matches!(
            child.kind(),
            visibility_keywords::PUB
                | visibility_keywords::CRATE
                | visibility_keywords::SUPER
                | visibility_keywords::SELF
                | visibility_keywords::IN
                | node_kinds::SCOPED_IDENTIFIER
                | node_kinds::IDENTIFIER
        )
    });

    if has_public_keyword {
        Visibility::Public
    } else {
        Visibility::Private
    }
}

// ============================================================================
// Documentation Extraction
// ============================================================================

/// Extract documentation comments preceding a node
pub fn extract_preceding_doc_comments(node: Node, source: &str) -> Option<String> {
    let doc_lines = collect_doc_lines(node, source);

    if doc_lines.is_empty() {
        None
    } else {
        Some(doc_lines.join("\n"))
    }
}

/// Maximum number of documentation lines to collect to prevent unbounded resource consumption
const MAX_DOC_LINES: usize = 1000;

/// Collect documentation lines from preceding siblings
fn collect_doc_lines(node: Node, source: &str) -> Vec<String> {
    let mut doc_lines = Vec::new();
    let mut current = node.prev_sibling();

    while let Some(sibling) = current {
        // Prevent unbounded resource consumption
        if doc_lines.len() >= MAX_DOC_LINES {
            break;
        }

        match sibling.kind() {
            node_kinds::LINE_COMMENT => {
                if let Some(doc_text) = extract_line_doc_text(sibling, source) {
                    doc_lines.push(doc_text);
                }
            }
            node_kinds::BLOCK_COMMENT => {
                if let Some(doc_text) = extract_block_doc_text(sibling, source) {
                    doc_lines.push(doc_text);
                }
            }
            node_kinds::ATTRIBUTE_ITEM => {
                // Continue through attributes
            }
            _ => break, // Stop at non-doc/non-attribute nodes
        }
        current = sibling.prev_sibling();
    }

    // Reverse once at the end instead of inserting at position 0 each time
    doc_lines.reverse();
    doc_lines
}

/// Extract documentation text from a line comment
fn extract_line_doc_text(node: Node, source: &str) -> Option<String> {
    node_to_text(node, source).ok().and_then(|text| {
        if text.starts_with(doc_prefixes::LINE_OUTER) {
            Some(
                text.trim_start_matches(doc_prefixes::LINE_OUTER)
                    .trim()
                    .to_string(),
            )
        } else if text.starts_with(doc_prefixes::LINE_INNER) {
            Some(
                text.trim_start_matches(doc_prefixes::LINE_INNER)
                    .trim()
                    .to_string(),
            )
        } else {
            None
        }
    })
}

/// Extract documentation text from a block comment
fn extract_block_doc_text(node: Node, source: &str) -> Option<String> {
    node_to_text(node, source).ok().and_then(|text| {
        if text.starts_with(doc_prefixes::BLOCK_OUTER_START) {
            Some(
                text.trim_start_matches(doc_prefixes::BLOCK_OUTER_START)
                    .trim_end_matches(doc_prefixes::BLOCK_END)
                    .trim()
                    .to_string(),
            )
        } else if text.starts_with(doc_prefixes::BLOCK_INNER_START) {
            Some(
                text.trim_start_matches(doc_prefixes::BLOCK_INNER_START)
                    .trim_end_matches(doc_prefixes::BLOCK_END)
                    .trim()
                    .to_string(),
            )
        } else {
            None
        }
    })
}

// ============================================================================
// Generic Parameter Extraction
// ============================================================================

/// Extract generic parameters from a type_parameters node
pub fn extract_generics_from_node(node: Node, source: &str) -> Vec<String> {
    let mut generics = Vec::new();
    let mut cursor = node.walk();

    for child in node.children(&mut cursor) {
        match child.kind() {
            // Skip punctuation
            punctuation::OPEN_ANGLE | punctuation::CLOSE_ANGLE | punctuation::COMMA => continue,

            // Handle various parameter types
            node_kinds::TYPE_PARAMETER
            | node_kinds::LIFETIME_PARAMETER
            | node_kinds::CONST_PARAMETER
            | node_kinds::TYPE_IDENTIFIER
            | node_kinds::LIFETIME
            | node_kinds::CONSTRAINED_TYPE_PARAMETER
            | node_kinds::OPTIONAL_TYPE_PARAMETER => {
                if let Ok(text) = node_to_text(child, source) {
                    generics.push(text);
                }
            }

            _ => {}
        }
    }

    generics
}

// ============================================================================
// Function Parameter and Modifier Extraction
// ============================================================================

/// Extract parameters from a function parameters node
pub fn extract_function_parameters(
    params_node: Node,
    source: &str,
) -> Result<Vec<(String, String)>> {
    use crate::rust::handler_impls::constants::{keywords, special_idents};

    let mut parameters = Vec::new();
    let mut cursor = params_node.walk();

    for child in params_node.children(&mut cursor) {
        // Skip punctuation like parentheses and commas
        if matches!(
            child.kind(),
            punctuation::OPEN_PAREN | punctuation::CLOSE_PAREN | punctuation::COMMA
        ) {
            continue;
        }

        // Handle different parameter types
        match child.kind() {
            node_kinds::PARAMETER => {
                if let Some((pattern, param_type)) = extract_parameter_parts(child, source)? {
                    parameters.push((pattern, param_type));
                }
            }
            node_kinds::SELF_PARAMETER => {
                let text = node_to_text(child, source)?;
                parameters.push((keywords::SELF.to_string(), text));
            }
            node_kinds::VARIADIC_PARAMETER => {
                let text = node_to_text(child, source)?;
                parameters.push((special_idents::VARIADIC.to_string(), text));
            }
            _ => {}
        }
    }

    Ok(parameters)
}

/// Extract pattern and type parts from a parameter node
///
/// # UTF-8 Safety
/// Uses `split_once(':')` instead of byte-index splitting to ensure UTF-8
/// character boundaries are respected. This prevents panics when parameter
/// names or types contain multi-byte Unicode characters.
///
/// For example, with a parameter like `名前: String`, using byte indices from
/// `find(':')` could panic if the split point falls within the multi-byte
/// character sequence. `split_once` safely handles this by operating on
/// character boundaries.
pub fn extract_parameter_parts(node: Node, source: &str) -> Result<Option<(String, String)>> {
    let full_text = node_to_text(node, source)?;

    // Use split_once for safe UTF-8 boundary handling
    if let Some((pattern, param_type)) = full_text.split_once(':') {
        return Ok(Some((
            pattern.trim().to_string(),
            param_type.trim().to_string(),
        )));
    }

    // No colon means no type annotation (rare in Rust)
    if !full_text.trim().is_empty() {
        Ok(Some((full_text, String::new())))
    } else {
        Ok(None)
    }
}

/// Extract function modifiers (async, unsafe, const) from a modifiers node
pub fn extract_function_modifiers(modifiers_node: Node) -> (bool, bool, bool) {
    use crate::rust::handler_impls::constants::function_modifiers;

    let mut has_async = false;
    let mut has_unsafe = false;
    let mut has_const = false;
    let mut cursor = modifiers_node.walk();

    for child in modifiers_node.children(&mut cursor) {
        match child.kind() {
            function_modifiers::ASYNC => has_async = true,
            function_modifiers::UNSAFE => has_unsafe = true,
            function_modifiers::CONST => has_const = true,
            _ => {}
        }
    }

    (has_async, has_unsafe, has_const)
}

// ============================================================================
// Function Call Extraction
// ============================================================================

use crate::common::import_map::{resolve_reference, ImportMap};
use std::collections::HashMap;

/// Extract function calls from a function body using tree-sitter queries
///
/// This version resolves bare identifiers to qualified names using imports and scope.
pub fn extract_function_calls(
    function_node: Node,
    source: &str,
    import_map: &ImportMap,
    parent_scope: Option<&str>,
    local_vars: &HashMap<String, String>, // var_name -> type_name for method resolution
) -> Vec<String> {
    let query_source = r#"
        (call_expression
          function: (identifier) @bare_callee)

        (call_expression
          function: (scoped_identifier) @scoped_callee)

        (call_expression
          function: (field_expression
            value: (identifier) @receiver
            field: (field_identifier) @method))
    "#;

    let language = tree_sitter_rust::LANGUAGE.into();
    let query = match Query::new(&language, query_source) {
        Ok(q) => q,
        Err(_) => return Vec::new(),
    };

    let mut cursor = tree_sitter::QueryCursor::new();
    let mut calls = Vec::new();

    let mut matches = cursor.matches(&query, function_node, source.as_bytes());
    while let Some(query_match) = matches.next() {
        // Collect captures by name for this match
        let captures: Vec<_> = query_match.captures.iter().collect();

        // Check for bare callee (identifier)
        let bare_callee = captures
            .iter()
            .find(|c| query.capture_names().get(c.index as usize).copied() == Some("bare_callee"));

        // Check for scoped callee (already qualified)
        let scoped_callee = captures.iter().find(|c| {
            query.capture_names().get(c.index as usize).copied() == Some("scoped_callee")
        });

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
                let resolved = resolve_reference(&name, import_map, parent_scope, "::");
                calls.push(resolved);
            }
        } else if let Some(scoped_cap) = scoped_callee {
            // Already scoped call like `std::io::read()`
            if let Ok(full_path) = node_to_text(scoped_cap.node, source) {
                calls.push(full_path);
            }
        } else if let (Some(recv_cap), Some(method_cap)) = (receiver, method) {
            // Method call like `x.bar()`
            if let (Ok(recv_name), Ok(method_name)) = (
                node_to_text(recv_cap.node, source),
                node_to_text(method_cap.node, source),
            ) {
                // Try to resolve receiver type from local variables
                if let Some(recv_type) = local_vars.get(&recv_name) {
                    // Resolve the type name through imports
                    let resolved_type =
                        resolve_reference(recv_type, import_map, parent_scope, "::");
                    calls.push(format!("{resolved_type}::{method_name}"));
                }
                // If receiver type unknown, skip this method call (can't resolve)
            }
        }
    }

    calls
}

/// Extract local variable types from let statements in a function body
///
/// This function scans a function body for `let` statements with type annotations
/// and returns a mapping of variable names to their declared types.
///
/// # Example
/// For code like:
/// ```text
/// let x: Foo = Foo::new();
/// let y: Bar<T> = Default::default();
/// ```
/// Returns: {"x" -> "Foo", "y" -> "Bar<T>"}
///
/// # Note
/// - Only extracts types from explicit annotations (e.g., `let x: Type = ...`)
/// - Does not infer types from expressions
/// - Handles destructuring patterns (extracts individual bindings)
pub fn extract_local_var_types(function_node: Node, source: &str) -> HashMap<String, String> {
    let query_source = r#"
        (let_declaration
          pattern: (identifier) @var_name
          type: (_) @var_type)

        (let_declaration
          pattern: (tuple_pattern
            (identifier) @tuple_var)
          type: (tuple_type
            (_) @tuple_type))
    "#;

    let language = tree_sitter_rust::LANGUAGE.into();
    let query = match Query::new(&language, query_source) {
        Ok(q) => q,
        Err(_) => return HashMap::new(),
    };

    let mut cursor = tree_sitter::QueryCursor::new();
    let mut var_types = HashMap::new();

    let mut matches = cursor.matches(&query, function_node, source.as_bytes());
    while let Some(query_match) = matches.next() {
        // Find var_name and var_type captures in this match
        let mut var_name: Option<String> = None;
        let mut var_type: Option<String> = None;

        for capture in query_match.captures {
            let capture_name = query
                .capture_names()
                .get(capture.index as usize)
                .copied()
                .unwrap_or("");

            match capture_name {
                "var_name" => {
                    if let Ok(name) = node_to_text(capture.node, source) {
                        var_name = Some(name);
                    }
                }
                "var_type" => {
                    if let Ok(type_text) = node_to_text(capture.node, source) {
                        var_type = Some(type_text);
                    }
                }
                _ => {}
            }
        }

        // If we found both name and type, add to map
        if let (Some(name), Some(ty)) = (var_name, var_type) {
            // Extract the base type name (strip generics for resolution)
            let base_type = ty.split('<').next().unwrap_or(&ty).trim().to_string();
            var_types.insert(name, base_type);
        }
    }

    var_types
}

/// Extract type references from a function for USES relationships
///
/// This function extracts all type references found in:
/// - Parameter types
/// - Return type
/// - Local variable type annotations
/// - Generic type arguments
///
/// Returns a list of type names (resolved to qualified names via ImportMap)
pub fn extract_type_references(
    function_node: Node,
    source: &str,
    import_map: &ImportMap,
    parent_scope: Option<&str>,
) -> Vec<String> {
    let query_source = r#"
        ; Type identifiers in all contexts
        (type_identifier) @type_ref

        ; Scoped type identifiers (e.g., std::io::Result)
        (scoped_type_identifier) @scoped_type_ref
    "#;

    let language = tree_sitter_rust::LANGUAGE.into();
    let query = match Query::new(&language, query_source) {
        Ok(q) => q,
        Err(_) => return Vec::new(),
    };

    let mut cursor = tree_sitter::QueryCursor::new();
    let mut type_refs = Vec::new();
    let mut seen = std::collections::HashSet::new();

    let mut matches = cursor.matches(&query, function_node, source.as_bytes());
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
                        // Skip primitive types
                        if is_primitive_type(&type_name) {
                            continue;
                        }
                        // Resolve through imports
                        let resolved =
                            resolve_reference(&type_name, import_map, parent_scope, "::");
                        if seen.insert(resolved.clone()) {
                            type_refs.push(resolved);
                        }
                    }
                }
                "scoped_type_ref" => {
                    if let Ok(full_path) = node_to_text(capture.node, source) {
                        // Scoped types are already qualified
                        if seen.insert(full_path.clone()) {
                            type_refs.push(full_path);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    type_refs
}

/// Check if a type name is a Rust primitive type
fn is_primitive_type(name: &str) -> bool {
    matches!(
        name,
        "bool"
            | "char"
            | "str"
            | "u8"
            | "u16"
            | "u32"
            | "u64"
            | "u128"
            | "usize"
            | "i8"
            | "i16"
            | "i32"
            | "i64"
            | "i128"
            | "isize"
            | "f32"
            | "f64"
            | "Self"
    )
}
