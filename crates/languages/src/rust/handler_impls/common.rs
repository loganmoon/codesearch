//! Common utilities shared between handler modules
//!
//! This module provides shared functionality for AST traversal,
//! text extraction, and documentation processing.

#![deny(warnings)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]

use crate::rust::handler_impls::constants::{
    capture_names, node_kinds, punctuation, visibility_keywords,
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

/// Collect documentation lines from preceding siblings using AST traversal.
fn collect_doc_lines(node: Node, source: &str) -> Vec<String> {
    let mut doc_lines = Vec::new();
    let mut current = node.prev_sibling();

    while let Some(sibling) = current {
        // Prevent unbounded resource consumption
        if doc_lines.len() >= MAX_DOC_LINES {
            break;
        }

        match sibling.kind() {
            node_kinds::LINE_COMMENT | node_kinds::BLOCK_COMMENT => {
                if let Some(doc_text) = extract_doc_text(sibling, source) {
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

/// Extract documentation text from a comment node using AST traversal.
///
/// Tree-sitter parses doc comments like `/// text` or `/** text */` into:
/// - line_comment / block_comment
///   - outer_doc_comment_marker (or inner_doc_comment_marker)
///   - doc_comment (contains just the text content)
///
/// Returns None if the comment doesn't have a doc_comment child node.
fn extract_doc_text(node: Node, source: &str) -> Option<String> {
    let mut cursor = node.walk();
    let mut has_doc_marker = false;
    let mut doc_content = None;

    for child in node.children(&mut cursor) {
        match child.kind() {
            node_kinds::OUTER_DOC_COMMENT_MARKER | node_kinds::INNER_DOC_COMMENT_MARKER => {
                has_doc_marker = true;
            }
            node_kinds::DOC_COMMENT => {
                doc_content = node_to_text(child, source).ok();
            }
            _ => {}
        }
    }

    if has_doc_marker {
        doc_content
    } else {
        None
    }
}

// ============================================================================
// Generic Parameter Extraction
// ============================================================================

/// Extract generic parameters from a type_parameters node (raw strings for backward compat)
pub fn extract_generics_from_node(node: Node, source: &str) -> Vec<String> {
    // Query for all parameter types within type_parameters
    // Note: lifetime_parameter contains lifetime, const_parameter contains identifier
    let query_source = r#"
        (type_parameter) @type_param
        (lifetime_parameter) @lifetime_param
        (const_parameter) @const_param
    "#;

    run_capture_query(node, source, query_source)
}

// ============================================================================
// Structured Generic Bounds Extraction (Query-Based)
// ============================================================================

use crate::common::import_map::{resolve_reference, ImportMap};

/// A parsed generic parameter with its trait bounds
#[derive(Debug, Clone, Default)]
pub struct GenericParam {
    pub name: String,
    pub bounds: Vec<String>,
}

/// Combined result from parsing inline generics and where clauses
#[derive(Debug, Clone, Default)]
pub struct ParsedGenerics {
    pub params: Vec<GenericParam>,
    pub bound_trait_refs: Vec<String>,
}

/// Extract generic parameters with bounds using tree-sitter queries.
///
/// Uses queries to capture type parameter names and their trait bounds in one pass,
/// rather than manually traversing the AST.
pub fn extract_generics_with_bounds(
    node: Node,
    source: &str,
    import_map: &ImportMap,
    parent_scope: Option<&str>,
) -> ParsedGenerics {
    // Query captures: param names and their bounds
    // Note: type_parameter contains name field and optional bounds field
    //       lifetime_parameter contains lifetime child
    //       const_parameter contains name field
    let query_source = r#"
        (type_parameter
            name: (type_identifier) @param
            bounds: (trait_bounds
                [(type_identifier) (scoped_type_identifier) (generic_type type: (type_identifier))] @bound)?)
        (lifetime_parameter (lifetime) @lifetime)
        (const_parameter name: (identifier) @const_param)
    "#;

    extract_params_with_bounds(node, source, query_source, import_map, parent_scope)
}

/// Extract bounds from a where_clause node using tree-sitter queries.
pub fn extract_where_clause_bounds(
    node: Node,
    source: &str,
    import_map: &ImportMap,
    parent_scope: Option<&str>,
) -> ParsedGenerics {
    let query_source = r#"
        (where_predicate
            left: (type_identifier) @param
            bounds: (trait_bounds
                [(type_identifier) (scoped_type_identifier) (generic_type type: (type_identifier))] @bound))
    "#;

    extract_params_with_bounds(node, source, query_source, import_map, parent_scope)
}

/// Core query execution for extracting parameters and bounds.
fn extract_params_with_bounds(
    node: Node,
    source: &str,
    query_source: &str,
    import_map: &ImportMap,
    parent_scope: Option<&str>,
) -> ParsedGenerics {
    let language = tree_sitter_rust::LANGUAGE.into();
    let query = match Query::new(&language, query_source) {
        Ok(q) => q,
        Err(_) => return ParsedGenerics::default(),
    };

    let mut cursor = tree_sitter::QueryCursor::new();
    let mut params_map: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    let mut bound_trait_refs = Vec::new();
    let mut seen_traits = std::collections::HashSet::new();

    let mut matches = cursor.matches(&query, node, source.as_bytes());
    while let Some(m) = matches.next() {
        let mut current_param: Option<String> = None;

        for capture in m.captures {
            let capture_name = query.capture_names().get(capture.index as usize).copied();
            let text = capture
                .node
                .utf8_text(source.as_bytes())
                .unwrap_or_default();

            match capture_name {
                Some("param" | "lifetime" | "const_param" | "opt_param") => {
                    current_param = Some(text.to_string());
                    params_map.entry(text.to_string()).or_default();
                }
                Some("bound") => {
                    // Extract base type name for generic_type nodes
                    let bound_text = if capture.node.kind() == "generic_type" {
                        capture
                            .node
                            .child_by_field_name("type")
                            .and_then(|n| n.utf8_text(source.as_bytes()).ok())
                            .unwrap_or(text)
                    } else {
                        text
                    };

                    if !is_primitive_type(bound_text) {
                        let resolved =
                            resolve_reference(bound_text, import_map, parent_scope, "::");

                        if let Some(ref param) = current_param {
                            params_map
                                .entry(param.clone())
                                .or_default()
                                .push(resolved.clone());
                        }
                        if seen_traits.insert(resolved.clone()) {
                            bound_trait_refs.push(resolved);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    let params = params_map
        .into_iter()
        .map(|(name, bounds)| GenericParam { name, bounds })
        .collect();

    ParsedGenerics {
        params,
        bound_trait_refs,
    }
}

/// Run a simple capture query and return all captured text values.
fn run_capture_query(node: Node, source: &str, query_source: &str) -> Vec<String> {
    let language = tree_sitter_rust::LANGUAGE.into();
    let query = match Query::new(&language, query_source) {
        Ok(q) => q,
        Err(_) => return Vec::new(),
    };

    let mut cursor = tree_sitter::QueryCursor::new();
    let mut results = Vec::new();

    let mut matches = cursor.matches(&query, node, source.as_bytes());
    while let Some(m) = matches.next() {
        for capture in m.captures {
            if let Ok(text) = capture.node.utf8_text(source.as_bytes()) {
                results.push(text.to_string());
            }
        }
    }

    results
}

/// Merge where clause bounds into existing parsed generics.
///
/// If a type parameter already exists, its bounds are extended.
/// New type parameters from where clause are added.
pub fn merge_parsed_generics(base: &mut ParsedGenerics, additional: ParsedGenerics) {
    // Build set of existing trait refs (owned strings to avoid borrow conflicts)
    let seen_traits: std::collections::HashSet<String> =
        base.bound_trait_refs.iter().cloned().collect();

    // Add new trait refs that aren't already present
    for trait_ref in additional.bound_trait_refs {
        if !seen_traits.contains(&trait_ref) {
            base.bound_trait_refs.push(trait_ref);
        }
    }

    // Merge params - only extend existing params or add truly new type params
    // (skip Self since it's a keyword, not a user-defined type parameter)
    for new_param in additional.params {
        if new_param.name == "Self" {
            continue;
        }
        if let Some(existing) = base.params.iter_mut().find(|p| p.name == new_param.name) {
            // Extend existing param's bounds
            for bound in new_param.bounds {
                if !existing.bounds.contains(&bound) {
                    existing.bounds.push(bound);
                }
            }
        } else {
            // Add new param from where clause (e.g., `where U: Default` when U not in inline generics)
            base.params.push(new_param);
        }
    }
}

/// Format a GenericParam back to a string representation.
///
/// E.g., GenericParam { name: "T", bounds: ["Clone", "Send"] } -> "T: Clone + Send"
pub fn format_generic_param(param: &GenericParam) -> String {
    if param.bounds.is_empty() {
        param.name.clone()
    } else {
        format!("{}: {}", param.name, param.bounds.join(" + "))
    }
}

/// Build a generic_bounds map from ParsedGenerics.
///
/// Returns a map of type parameter names to their trait bounds.
pub fn build_generic_bounds_map(parsed: &ParsedGenerics) -> im::HashMap<String, Vec<String>> {
    parsed
        .params
        .iter()
        .filter(|p| !p.bounds.is_empty())
        .map(|p| (p.name.clone(), p.bounds.clone()))
        .collect()
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

/// Extract pattern and type parts from a parameter node using AST field access.
///
/// Uses tree-sitter's `child_by_field_name` to access the structured `pattern`
/// and `type` fields directly, rather than parsing the node text as a string.
pub fn extract_parameter_parts(node: Node, source: &str) -> Result<Option<(String, String)>> {
    use crate::rust::handler_impls::constants::field_names;

    // Access structured fields directly from AST
    let pattern_node = node.child_by_field_name(field_names::PATTERN);
    let type_node = node.child_by_field_name(field_names::TYPE);

    match (pattern_node, type_node) {
        (Some(pattern), Some(ty)) => {
            let pattern_text = node_to_text(pattern, source)?;
            let type_text = node_to_text(ty, source)?;
            Ok(Some((pattern_text, type_text)))
        }
        (Some(pattern), None) => {
            // Parameter without type annotation (rare in Rust)
            let pattern_text = node_to_text(pattern, source)?;
            Ok(Some((pattern_text, String::new())))
        }
        _ => Ok(None),
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
/// and returns a mapping of variable names to their declared types (base type only).
///
/// # Example
/// For code like:
/// ```text
/// let x: Foo = Foo::new();
/// let y: Bar<T> = Default::default();
/// ```
/// Returns: {"x" -> "Foo", "y" -> "Bar"}
///
/// # Note
/// - Only extracts types from explicit annotations (e.g., `let x: Type = ...`)
/// - Does not infer types from expressions
/// - For generic types, extracts only the base type using AST traversal
pub fn extract_local_var_types(function_node: Node, source: &str) -> HashMap<String, String> {
    let query_source = r#"
        (let_declaration
          pattern: (identifier) @var_name
          type: (_) @var_type)
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
        let mut var_name: Option<String> = None;
        let mut var_type_node: Option<Node> = None;

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
                    var_type_node = Some(capture.node);
                }
                _ => {}
            }
        }

        if let (Some(name), Some(type_node)) = (var_name, var_type_node) {
            // Extract base type using AST traversal instead of string splitting
            let base_type = extract_base_type_name(type_node, source);
            if let Some(base) = base_type {
                var_types.insert(name, base);
            }
        }
    }

    var_types
}

/// Extract the base type name from a type node.
///
/// For simple types like `Foo`, returns "Foo".
/// For generic types like `Vec<String>`, returns "Vec" by traversing the AST.
fn extract_base_type_name(type_node: Node, source: &str) -> Option<String> {
    use crate::rust::handler_impls::constants::field_names;

    match type_node.kind() {
        // Simple type identifier
        "type_identifier" => node_to_text(type_node, source).ok(),

        // Generic type like Vec<T> - extract the base type via AST field
        "generic_type" => type_node
            .child_by_field_name(field_names::TYPE)
            .and_then(|base| node_to_text(base, source).ok()),

        // Scoped type like std::vec::Vec
        "scoped_type_identifier" => node_to_text(type_node, source).ok(),

        // Reference type like &T or &mut T - get the inner type
        "reference_type" => type_node
            .child_by_field_name(field_names::TYPE)
            .and_then(|inner| extract_base_type_name(inner, source)),

        // For other types, just use the text
        _ => node_to_text(type_node, source).ok(),
    }
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
pub(crate) fn is_primitive_type(name: &str) -> bool {
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
            | "()"
    )
}
