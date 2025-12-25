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
// Structured Generic Bounds Extraction
// ============================================================================

use crate::common::import_map::{resolve_reference, ImportMap};

/// A parsed generic parameter with its trait bounds
#[derive(Debug, Clone, Default)]
pub struct GenericParam {
    /// The type parameter name (e.g., "T")
    pub name: String,
    /// Trait bounds for this parameter (e.g., ["Clone", "Send"])
    pub bounds: Vec<String>,
}

impl GenericParam {
    /// Returns true if this generic parameter is semantically valid.
    ///
    /// A valid parameter has a non-empty name and no empty bounds.
    #[allow(dead_code)]
    pub fn is_valid(&self) -> bool {
        !self.name.is_empty() && self.bounds.iter().all(|b| !b.is_empty())
    }
}

/// Combined result from parsing inline generics and where clauses
#[derive(Debug, Clone, Default)]
pub struct ParsedGenerics {
    /// Parsed generic parameters with their bounds
    pub params: Vec<GenericParam>,
    /// Resolved trait names for USES relationships (deduplicated)
    pub bound_trait_refs: Vec<String>,
}

/// Extract generic parameters with parsed bounds from a type_parameters node.
///
/// Handles:
/// - Simple params: `T` -> GenericParam { name: "T", bounds: [] }
/// - Constrained params: `T: Clone + Send` -> GenericParam { name: "T", bounds: ["Clone", "Send"] }
/// - Lifetime params: `'a` -> included with empty bounds
/// - Const params: `const N: usize` -> included with empty bounds
/// - Optional/default params: `T = Default` -> GenericParam { name: "T", bounds: [] }
pub fn extract_generics_with_bounds(
    node: Node,
    source: &str,
    import_map: &ImportMap,
    parent_scope: Option<&str>,
) -> ParsedGenerics {
    let mut params = Vec::new();
    let mut bound_trait_refs = Vec::new();
    let mut seen_traits = std::collections::HashSet::new();
    let mut cursor = node.walk();

    for child in node.children(&mut cursor) {
        match child.kind() {
            // Skip punctuation
            punctuation::OPEN_ANGLE | punctuation::CLOSE_ANGLE | punctuation::COMMA => continue,

            // Type parameter - may be simple or constrained
            // Tree-sitter wraps both `T` and `T: Clone` as type_parameter
            node_kinds::TYPE_PARAMETER => {
                // Check if this type_parameter has trait bounds
                if let Some((name, bounds)) =
                    parse_type_parameter_with_bounds(child, source, import_map, parent_scope)
                {
                    // Add resolved bounds to trait refs for USES relationships
                    for bound in &bounds {
                        if seen_traits.insert(bound.clone()) {
                            bound_trait_refs.push(bound.clone());
                        }
                    }
                    params.push(GenericParam { name, bounds });
                }
            }

            // Simple type identifier (shouldn't happen in type_parameters, but handle it)
            node_kinds::TYPE_IDENTIFIER => {
                if let Ok(name) = node_to_text(child, source) {
                    params.push(GenericParam {
                        name,
                        bounds: Vec::new(),
                    });
                }
            }

            // Lifetime parameter (no trait bounds, just track it)
            node_kinds::LIFETIME_PARAMETER | node_kinds::LIFETIME => {
                if let Ok(name) = node_to_text(child, source) {
                    params.push(GenericParam {
                        name,
                        bounds: Vec::new(),
                    });
                }
            }

            // Const parameter (no trait bounds)
            node_kinds::CONST_PARAMETER => {
                if let Ok(name) = node_to_text(child, source) {
                    params.push(GenericParam {
                        name,
                        bounds: Vec::new(),
                    });
                }
            }

            // Constrained type parameter: `T: Clone + Send` (legacy path)
            node_kinds::CONSTRAINED_TYPE_PARAMETER => {
                if let Some((name, bounds)) =
                    parse_constrained_type_param(child, source, import_map, parent_scope)
                {
                    // Add resolved bounds to trait refs for USES relationships
                    for bound in &bounds {
                        if seen_traits.insert(bound.clone()) {
                            bound_trait_refs.push(bound.clone());
                        }
                    }
                    params.push(GenericParam { name, bounds });
                }
            }

            // Optional type parameter: `T = Default`
            node_kinds::OPTIONAL_TYPE_PARAMETER => {
                // Extract just the name, ignoring the default
                if let Some(name) = extract_optional_param_name(child, source) {
                    params.push(GenericParam {
                        name,
                        bounds: Vec::new(),
                    });
                }
            }

            _ => {}
        }
    }

    ParsedGenerics {
        params,
        bound_trait_refs,
    }
}

/// Parse a type_parameter node that may or may not have bounds.
///
/// Handles both:
/// - Simple: `T` -> ("T", [])
/// - Constrained: `T: Clone + Send` -> ("T", ["Clone", "Send"])
fn parse_type_parameter_with_bounds(
    node: Node,
    source: &str,
    import_map: &ImportMap,
    parent_scope: Option<&str>,
) -> Option<(String, Vec<String>)> {
    let mut cursor = node.walk();
    let mut name = None;
    let mut bounds = Vec::new();

    for child in node.children(&mut cursor) {
        match child.kind() {
            // The type parameter name
            node_kinds::TYPE_IDENTIFIER | "identifier" => {
                if name.is_none() {
                    name = node_to_text(child, source).ok();
                }
            }

            // The trait bounds (if any)
            node_kinds::TRAIT_BOUNDS => {
                bounds = parse_trait_bounds(child, source, import_map, parent_scope);
            }

            // Skip punctuation (like ':')
            ":" => continue,

            _ => {}
        }
    }

    // If we found a name, return it with whatever bounds we found (may be empty)
    name.map(|n| (n, bounds))
}

/// Parse a constrained_type_parameter node like `T: Clone + Send` (legacy path)
fn parse_constrained_type_param(
    node: Node,
    source: &str,
    import_map: &ImportMap,
    parent_scope: Option<&str>,
) -> Option<(String, Vec<String>)> {
    parse_type_parameter_with_bounds(node, source, import_map, parent_scope)
}

/// Extract the name from an optional_type_parameter node like `T = Default`
fn extract_optional_param_name(node: Node, source: &str) -> Option<String> {
    let mut cursor = node.walk();

    for child in node.children(&mut cursor) {
        if child.kind() == node_kinds::TYPE_IDENTIFIER {
            return node_to_text(child, source).ok();
        }
    }

    None
}

/// Parse trait bounds from a trait_bounds node.
///
/// Handles: `Clone + Send + 'a`
/// Returns: vec of resolved trait names (lifetimes filtered out)
pub fn parse_trait_bounds(
    bounds_node: Node,
    source: &str,
    import_map: &ImportMap,
    parent_scope: Option<&str>,
) -> Vec<String> {
    let mut bounds = Vec::new();
    let mut cursor = bounds_node.walk();

    for child in bounds_node.children(&mut cursor) {
        match child.kind() {
            // Skip punctuation and lifetimes
            punctuation::PLUS | ":" | node_kinds::LIFETIME => continue,

            // Simple trait reference
            node_kinds::TYPE_IDENTIFIER => {
                if let Ok(trait_name) = node_to_text(child, source) {
                    // Skip primitive types that might appear in bounds
                    if !is_primitive_type(&trait_name) {
                        let resolved =
                            resolve_reference(&trait_name, import_map, parent_scope, "::");
                        bounds.push(resolved);
                    }
                }
            }

            // Scoped trait reference like `std::fmt::Debug`
            node_kinds::SCOPED_TYPE_IDENTIFIER => {
                if let Ok(full_path) = node_to_text(child, source) {
                    bounds.push(full_path);
                }
            }

            // Generic type like `Iterator<Item = T>` - extract just the base trait
            "generic_type" => {
                if let Some(base) =
                    extract_generic_type_base(child, source, import_map, parent_scope)
                {
                    bounds.push(base);
                }
            }

            _ => {}
        }
    }

    bounds
}

/// Extract the base trait from a generic_type node like `Iterator<Item = T>`
fn extract_generic_type_base(
    node: Node,
    source: &str,
    import_map: &ImportMap,
    parent_scope: Option<&str>,
) -> Option<String> {
    let mut cursor = node.walk();

    for child in node.children(&mut cursor) {
        match child.kind() {
            node_kinds::TYPE_IDENTIFIER => {
                if let Ok(name) = node_to_text(child, source) {
                    return Some(resolve_reference(&name, import_map, parent_scope, "::"));
                }
            }
            node_kinds::SCOPED_TYPE_IDENTIFIER => {
                return node_to_text(child, source).ok();
            }
            _ => {}
        }
    }

    None
}

/// Extract bounds from a where_clause node.
///
/// Parses: `where T: Debug, U: Clone + Sync`
/// Returns additional bounds to merge with inline bounds.
pub fn extract_where_clause_bounds(
    node: Node,
    source: &str,
    import_map: &ImportMap,
    parent_scope: Option<&str>,
) -> ParsedGenerics {
    let mut params = Vec::new();
    let mut bound_trait_refs = Vec::new();
    let mut seen_traits = std::collections::HashSet::new();
    let mut cursor = node.walk();

    for child in node.children(&mut cursor) {
        if child.kind() == node_kinds::WHERE_PREDICATE {
            if let Some((name, bounds)) =
                parse_where_predicate(child, source, import_map, parent_scope)
            {
                // Add resolved bounds to trait refs
                for bound in &bounds {
                    if seen_traits.insert(bound.clone()) {
                        bound_trait_refs.push(bound.clone());
                    }
                }
                params.push(GenericParam { name, bounds });
            }
        }
    }

    ParsedGenerics {
        params,
        bound_trait_refs,
    }
}

/// Parse a where_predicate node like `T: Debug + Clone`
fn parse_where_predicate(
    node: Node,
    source: &str,
    import_map: &ImportMap,
    parent_scope: Option<&str>,
) -> Option<(String, Vec<String>)> {
    let mut cursor = node.walk();
    let mut name = None;
    let mut bounds = Vec::new();

    for child in node.children(&mut cursor) {
        match child.kind() {
            // The type being constrained (first type_identifier is the param name)
            node_kinds::TYPE_IDENTIFIER => {
                if name.is_none() {
                    name = node_to_text(child, source).ok();
                }
            }

            // The trait bounds
            node_kinds::TRAIT_BOUNDS => {
                bounds = parse_trait_bounds(child, source, import_map, parent_scope);
            }

            _ => {}
        }
    }

    name.map(|n| (n, bounds))
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
