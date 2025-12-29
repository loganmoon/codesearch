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
use codesearch_core::entities::{ReferenceType, SourceLocation, SourceReference, Visibility};
use codesearch_core::error::Result;
use streaming_iterator::StreamingIterator;
use tracing::{trace, warn};
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

use crate::common::import_map::{resolve_rust_reference, ImportMap};

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

/// Context for resolving Rust references to fully qualified names.
///
/// Bundles all the information needed to resolve bare identifiers and
/// Rust-relative paths (crate::, self::, super::) to absolute qualified names.
#[derive(Debug, Clone, Copy)]
pub struct RustResolutionContext<'a> {
    /// Import map from the current file's use declarations
    pub import_map: &'a ImportMap,
    /// Parent scope for unresolved identifiers (e.g., "my_module::MyStruct")
    pub parent_scope: Option<&'a str>,
    /// Package/crate name for normalizing crate:: paths (e.g., "anyhow")
    pub package_name: Option<&'a str>,
    /// Current module path for normalizing self::/super:: paths (e.g., "error::context")
    pub current_module: Option<&'a str>,
}

impl<'a> RustResolutionContext<'a> {
    /// Resolve a reference using this context
    pub fn resolve(&self, name: &str) -> String {
        resolve_rust_reference(
            name,
            self.import_map,
            self.parent_scope,
            self.package_name,
            self.current_module,
        )
    }
}

/// Extract generic parameters with bounds using tree-sitter queries.
///
/// Uses queries to capture type parameter names and their trait bounds in one pass,
/// rather than manually traversing the AST.
pub fn extract_generics_with_bounds(
    node: Node,
    source: &str,
    ctx: &RustResolutionContext,
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

    extract_params_with_bounds(node, source, query_source, ctx)
}

/// Extract bounds from a where_clause node using tree-sitter queries.
pub fn extract_where_clause_bounds(
    node: Node,
    source: &str,
    ctx: &RustResolutionContext,
) -> ParsedGenerics {
    let query_source = r#"
        (where_predicate
            left: (type_identifier) @param
            bounds: (trait_bounds
                [(type_identifier) (scoped_type_identifier) (generic_type type: (type_identifier))] @bound))
    "#;

    extract_params_with_bounds(node, source, query_source, ctx)
}

/// Core query execution for extracting parameters and bounds.
fn extract_params_with_bounds(
    node: Node,
    source: &str,
    query_source: &str,
    ctx: &RustResolutionContext,
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
                        let resolved = ctx.resolve(bound_text);

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
/// Returns SourceReferences with location data for disambiguation.
pub fn extract_function_calls(
    function_node: Node,
    source: &str,
    ctx: &RustResolutionContext,
    local_vars: &HashMap<String, String>, // var_name -> type_name for method resolution
    generic_bounds: &im::HashMap<String, Vec<String>>, // type_param -> [trait_bounds] for generic resolution
) -> Vec<SourceReference> {
    let query_source = r#"
        (call_expression
          function: (identifier) @bare_callee)

        (call_expression
          function: (scoped_identifier) @scoped_callee)

        (call_expression
          function: (field_expression
            value: (identifier) @receiver
            field: (field_identifier) @method))

        (call_expression
          function: (field_expression
            value: (call_expression) @chain_receiver
            field: (field_identifier) @chain_method))
    "#;

    let language = tree_sitter_rust::LANGUAGE.into();
    let query = match Query::new(&language, query_source) {
        Ok(q) => q,
        Err(e) => {
            warn!(
                "Failed to compile tree-sitter query for function call extraction: {e}. \
                 This indicates a bug in the query definition."
            );
            return Vec::new();
        }
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

        // Check for method call on identifier (receiver.method())
        let receiver = captures
            .iter()
            .find(|c| query.capture_names().get(c.index as usize).copied() == Some("receiver"));
        let method = captures
            .iter()
            .find(|c| query.capture_names().get(c.index as usize).copied() == Some("method"));

        // Check for chained method call (expr().method())
        let chain_receiver = captures.iter().find(|c| {
            query.capture_names().get(c.index as usize).copied() == Some("chain_receiver")
        });
        let chain_method = captures
            .iter()
            .find(|c| query.capture_names().get(c.index as usize).copied() == Some("chain_method"));

        if let Some(bare_cap) = bare_callee {
            // Bare identifier call like `foo()`
            if let Ok(name) = node_to_text(bare_cap.node, source) {
                let resolved = ctx.resolve(&name);
                calls.push(SourceReference {
                    target: resolved,
                    location: SourceLocation::from_tree_sitter_node(bare_cap.node),
                    ref_type: ReferenceType::Call,
                });
            }
        } else if let Some(scoped_cap) = scoped_callee {
            // Scoped call like `std::io::read()` or `crate::utils::helper()`
            if let Ok(full_path) = node_to_text(scoped_cap.node, source) {
                // Resolve to normalize crate::, self::, super:: paths
                let resolved = ctx.resolve(&full_path);
                calls.push(SourceReference {
                    target: resolved,
                    location: SourceLocation::from_tree_sitter_node(scoped_cap.node),
                    ref_type: ReferenceType::Call,
                });
            }
        } else if let (Some(recv_cap), Some(method_cap)) = (receiver, method) {
            // Method call like `x.bar()`
            if let (Ok(recv_name), Ok(method_name)) = (
                node_to_text(recv_cap.node, source),
                node_to_text(method_cap.node, source),
            ) {
                // Try to resolve receiver type from local variables
                if let Some(recv_type) = local_vars.get(&recv_name) {
                    // DEBUG: Log the lookup
                    #[cfg(debug_assertions)]
                    eprintln!(
                        "DEBUG extract_function_calls: recv_name={}, recv_type={}, generic_bounds keys={:?}, lookup={:?}",
                        recv_name,
                        recv_type,
                        generic_bounds.keys().collect::<Vec<_>>(),
                        generic_bounds.get(recv_type)
                    );
                    // Check if the receiver type is a generic type parameter with trait bounds
                    if let Some(bounds) = generic_bounds.get(recv_type) {
                        // For generic type parameters, add call targets for ALL trait bounds.
                        // We can't know at extraction time which trait provides the method,
                        // so we add all possibilities. The outbox processor's resolution
                        // will only create CALLS relationships for methods that exist as entities.
                        for bound in bounds {
                            calls.push(SourceReference {
                                target: format!("{bound}::{method_name}"),
                                location: SourceLocation::from_tree_sitter_node(method_cap.node),
                                ref_type: ReferenceType::Call,
                            });
                        }
                    } else {
                        // Not a generic type parameter, resolve through imports
                        let resolved_type = ctx.resolve(recv_type);
                        calls.push(SourceReference {
                            target: format!("{resolved_type}::{method_name}"),
                            location: SourceLocation::from_tree_sitter_node(method_cap.node),
                            ref_type: ReferenceType::Call,
                        });
                    }
                }
                // If receiver type unknown, skip this method call (can't resolve)
            }
        } else if let (Some(chain_recv_cap), Some(chain_method_cap)) =
            (chain_receiver, chain_method)
        {
            // Chained method call like `Type::new().method()` or `expr().method()`
            if let Ok(method_name) = node_to_text(chain_method_cap.node, source) {
                // Try to extract the chain head type from the receiver expression
                match extract_method_chain_head_type(chain_recv_cap.node, source, local_vars) {
                    Some(chain_head_type) => {
                        let resolved_type = ctx.resolve(&chain_head_type);
                        calls.push(SourceReference {
                            target: format!("{resolved_type}::{method_name}"),
                            location: SourceLocation::from_tree_sitter_node(chain_method_cap.node),
                            ref_type: ReferenceType::Call,
                        });
                    }
                    None => {
                        trace!(
                            "Could not resolve chain head type for method call '{method_name}' - skipping"
                        );
                    }
                }
            }
        }
    }

    calls
}

/// Extract the type from the head of a method chain.
///
/// For chains like `Type::new().method1().method2()`, this walks back to find `Type`.
/// For chains starting with a variable like `x.method()`, this looks up the variable's
/// type in `local_vars` and returns it if found, or None if the variable type is unknown.
fn extract_method_chain_head_type(
    call_expr_node: Node,
    source: &str,
    local_vars: &HashMap<String, String>,
) -> Option<String> {
    // The node is a call_expression. Check what kind of call it is.
    // Look for the "function" child to determine call type.
    let mut cursor = call_expr_node.walk();

    for child in call_expr_node.children(&mut cursor) {
        if child.kind() == "scoped_identifier" {
            // This is a scoped call like `Type::method()` or `module::func()`
            // Extract the type/path prefix (everything before the last ::segment)
            if let Ok(full_path) = node_to_text(child, source) {
                // Split by :: and get everything but the last segment
                let parts: Vec<&str> = full_path.split("::").collect();
                if parts.len() >= 2 {
                    // Return the type part (everything except the method name)
                    return Some(parts[..parts.len() - 1].join("::"));
                }
            }
        } else if child.kind() == "field_expression" {
            // This is a method call on something. Recursively find the chain head.
            // field_expression has "value" and "field" children
            for field_child in child.children(&mut child.walk()) {
                if field_child.kind() == "call_expression" {
                    // Recurse into the chain
                    return extract_method_chain_head_type(field_child, source, local_vars);
                } else if field_child.kind() == "identifier" {
                    // Chain starts with a variable - check local_vars
                    if let Ok(var_name) = node_to_text(field_child, source) {
                        return local_vars.get(&var_name).cloned();
                    }
                }
            }
        } else if child.kind() == "identifier" {
            // Bare function call - can't determine return type
            return None;
        }
    }

    None
}

/// Extract local variable and parameter types from a function body
///
/// This function scans a function for:
/// - `let` statements with type annotations
/// - Function parameter types
///
/// Returns a mapping of variable/parameter names to their declared types (base type only).
///
/// # Example
/// For code like:
/// ```text
/// fn foo(x: &Data, y: Config) {
///     let z: Foo = Foo::new();
/// }
/// ```
/// Returns: {"x" -> "Data", "y" -> "Config", "z" -> "Foo"}
///
/// # Note
/// - For generic types, extracts only the base type using AST traversal
/// - Reference types like `&T` extract to base type `T`
pub fn extract_local_var_types(function_node: Node, source: &str) -> HashMap<String, String> {
    let mut var_types = HashMap::new();

    // First, extract function parameter types
    extract_parameter_types_into(function_node, source, &mut var_types);

    // Then, extract let statement types from explicit type annotations
    let query_source = r#"
        (let_declaration
          pattern: (identifier) @var_name
          type: (_) @var_type)
    "#;

    let language = tree_sitter_rust::LANGUAGE.into();
    let query = match Query::new(&language, query_source) {
        Ok(q) => q,
        Err(e) => {
            warn!(
                "Failed to compile tree-sitter query for local var types: {e}. \
                 This indicates a bug in the query definition."
            );
            return var_types;
        }
    };

    let mut cursor = tree_sitter::QueryCursor::new();

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

    // Also extract types from constructor-style initializers like `let w = Widget::new()`
    // This handles cases where the type is inferred from a Type::method() call
    let constructor_query_source = r#"
        (let_declaration
          pattern: (identifier) @var_name
          value: (call_expression
            function: (scoped_identifier) @constructor))
    "#;

    let constructor_query = match Query::new(&language, constructor_query_source) {
        Ok(q) => q,
        Err(e) => {
            warn!(
                "Failed to compile constructor type query: {e}. \
                 This indicates a bug in the query definition."
            );
            return var_types;
        }
    };

    let mut cursor = tree_sitter::QueryCursor::new();
    let mut matches = cursor.matches(&constructor_query, function_node, source.as_bytes());
    while let Some(query_match) = matches.next() {
        let mut var_name: Option<String> = None;
        let mut constructor_path: Option<String> = None;

        for capture in query_match.captures {
            let capture_name = constructor_query
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
                "constructor" => {
                    if let Ok(path) = node_to_text(capture.node, source) {
                        constructor_path = Some(path);
                    }
                }
                _ => {}
            }
        }

        // Extract type from constructor path (e.g., "Widget::new" -> "Widget")
        if let (Some(name), Some(path)) = (var_name, constructor_path) {
            // Only add if we don't already have a type for this variable
            // (explicit annotations take precedence)
            if let std::collections::hash_map::Entry::Vacant(e) = var_types.entry(name) {
                // Extract the type part - everything before the last ::segment
                if let Some(type_part) = path.rsplit_once("::").map(|(prefix, _)| prefix) {
                    e.insert(type_part.to_string());
                }
            }
        }
    }

    var_types
}

/// Extract function parameter types into a map
fn extract_parameter_types_into(
    function_node: Node,
    source: &str,
    var_types: &mut HashMap<String, String>,
) {
    let query_source = r#"
        (parameters
          (parameter
            pattern: (identifier) @param_name
            type: (_) @param_type))
    "#;

    let language = tree_sitter_rust::LANGUAGE.into();
    let query = match Query::new(&language, query_source) {
        Ok(q) => q,
        Err(e) => {
            warn!(
                "Failed to compile tree-sitter query for parameter types: {e}. \
                 This indicates a bug in the query definition."
            );
            return;
        }
    };

    let mut cursor = tree_sitter::QueryCursor::new();
    let mut matches = cursor.matches(&query, function_node, source.as_bytes());

    while let Some(query_match) = matches.next() {
        let mut param_name: Option<String> = None;
        let mut param_type_node: Option<Node> = None;

        for capture in query_match.captures {
            let capture_name = query
                .capture_names()
                .get(capture.index as usize)
                .copied()
                .unwrap_or("");

            match capture_name {
                "param_name" => {
                    if let Ok(name) = node_to_text(capture.node, source) {
                        param_name = Some(name);
                    }
                }
                "param_type" => {
                    param_type_node = Some(capture.node);
                }
                _ => {}
            }
        }

        if let (Some(name), Some(type_node)) = (param_name, param_type_node) {
            let base_type = extract_base_type_name(type_node, source);
            if let Some(base) = base_type {
                var_types.insert(name, base);
            }
        }
    }
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
/// Returns SourceReferences with location data for disambiguation.
pub fn extract_type_references(
    function_node: Node,
    source: &str,
    ctx: &RustResolutionContext,
) -> Vec<SourceReference> {
    let query_source = r#"
        ; Type identifiers in all contexts
        (type_identifier) @type_ref

        ; Scoped type identifiers (e.g., std::io::Result)
        (scoped_type_identifier) @scoped_type_ref
    "#;

    let language = tree_sitter_rust::LANGUAGE.into();
    let query = match Query::new(&language, query_source) {
        Ok(q) => q,
        Err(e) => {
            warn!(
                "Failed to compile tree-sitter query for type references: {e}. \
                 This indicates a bug in the query definition."
            );
            return Vec::new();
        }
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
                        let resolved = ctx.resolve(&type_name);
                        if seen.insert(resolved.clone()) {
                            type_refs.push(SourceReference {
                                target: resolved,
                                location: SourceLocation::from_tree_sitter_node(capture.node),
                                ref_type: ReferenceType::TypeUsage,
                            });
                        }
                    }
                }
                "scoped_type_ref" => {
                    if let Ok(full_path) = node_to_text(capture.node, source) {
                        // Resolve to normalize crate::, self::, super:: paths
                        let resolved = ctx.resolve(&full_path);
                        if seen.insert(resolved.clone()) {
                            type_refs.push(SourceReference {
                                target: resolved,
                                location: SourceLocation::from_tree_sitter_node(capture.node),
                                ref_type: ReferenceType::TypeUsage,
                            });
                        }
                    }
                }
                _ => {}
            }
        }
    }

    type_refs
}

/// Find a direct child node by its kind.
///
/// Returns the first matching child node, or None if no child matches.
pub fn find_child_by_kind<'a>(node: Node<'a>, kind: &str) -> Option<Node<'a>> {
    let mut cursor = node.walk();
    let result = node
        .children(&mut cursor)
        .find(|child| child.kind() == kind);
    result
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

// ============================================================================
// Type Alias Resolution
// ============================================================================

/// Extract type aliases from an AST and build a resolution map.
///
/// Returns a map from alias name to aliased type (the target of the alias).
/// This allows following type alias chains to find the underlying concrete type.
///
/// Example:
/// ```text
/// type Settings = RawConfig;
/// type AppConfig = Settings;
/// ```
/// Returns: {"Settings" -> "RawConfig", "AppConfig" -> "Settings"}
pub fn extract_type_alias_map(root: Node, source: &str) -> HashMap<String, String> {
    let mut aliases = HashMap::new();

    let query_source = r#"
        (type_item
          name: (type_identifier) @name
          type: (_) @type)
    "#;

    let language = tree_sitter_rust::LANGUAGE.into();
    let query = match tree_sitter::Query::new(&language, query_source) {
        Ok(q) => q,
        Err(_) => return aliases,
    };

    let mut cursor = tree_sitter::QueryCursor::new();
    let mut matches = cursor.matches(&query, root, source.as_bytes());

    while let Some(query_match) = matches.next() {
        let mut alias_name = None;
        let mut aliased_type = None;

        for capture in query_match.captures {
            let capture_name = query
                .capture_names()
                .get(capture.index as usize)
                .copied()
                .unwrap_or("");

            match capture_name {
                "name" => {
                    alias_name = capture.node.utf8_text(source.as_bytes()).ok();
                }
                "type" => {
                    // Get the type text, stripping any generics
                    if let Ok(type_text) = capture.node.utf8_text(source.as_bytes()) {
                        // Strip generics: "Foo<T>" -> "Foo"
                        let base_type = type_text.split('<').next().unwrap_or(type_text).trim();
                        aliased_type = Some(base_type.to_string());
                    }
                }
                _ => {}
            }
        }

        if let (Some(name), Some(target)) = (alias_name, aliased_type) {
            aliases.insert(name.to_string(), target);
        }
    }

    aliases
}

/// Resolve a type name through the type alias map, following chains until
/// we find a non-alias type or hit a cycle.
///
/// Example:
/// ```text
/// // aliases: {"AppConfig" -> "Settings", "Settings" -> "RawConfig"}
/// resolve_type_alias_chain("AppConfig", &aliases) // -> Some("RawConfig")
/// resolve_type_alias_chain("RawConfig", &aliases) // -> None (not an alias)
/// ```
pub fn resolve_type_alias_chain(
    type_name: &str,
    aliases: &HashMap<String, String>,
    max_depth: usize,
) -> Option<String> {
    let mut current = type_name;
    let mut resolved = None;
    let mut depth = 0;

    while let Some(target) = aliases.get(current) {
        resolved = Some(target.clone());
        current = target;
        depth += 1;
        if depth >= max_depth {
            // Prevent infinite loops from cyclic aliases
            break;
        }
    }

    resolved
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use tree_sitter::Parser;

    /// Parse Rust source code and return the root node
    fn parse_rust(source: &str) -> tree_sitter::Tree {
        let mut parser = Parser::new();
        parser.set_language(&tree_sitter_rust::LANGUAGE.into()).ok();
        parser.parse(source, None).unwrap()
    }

    /// Find a function node in the parsed tree
    fn find_function_node(tree: &tree_sitter::Tree) -> Option<tree_sitter::Node<'_>> {
        let root = tree.root_node();
        let mut cursor = root.walk();
        let result = root
            .children(&mut cursor)
            .find(|child| child.kind() == "function_item");
        result
    }

    // =========================================================================
    // Tests for extract_local_var_types (tests extract_parameter_types_into)
    // =========================================================================

    #[test]
    fn test_extract_local_var_types_simple_parameters() {
        let source = r#"
            fn process(data: Data, config: Config) {
            }
        "#;

        let tree = parse_rust(source);
        let func_node = find_function_node(&tree).unwrap();
        let types = extract_local_var_types(func_node, source);

        assert_eq!(types.get("data"), Some(&"Data".to_string()));
        assert_eq!(types.get("config"), Some(&"Config".to_string()));
    }

    #[test]
    fn test_extract_local_var_types_reference_parameters() {
        let source = r#"
            fn process(data: &Data, config: &mut Config) {
            }
        "#;

        let tree = parse_rust(source);
        let func_node = find_function_node(&tree).unwrap();
        let types = extract_local_var_types(func_node, source);

        // Reference types should extract to base type
        assert_eq!(types.get("data"), Some(&"Data".to_string()));
        assert_eq!(types.get("config"), Some(&"Config".to_string()));
    }

    #[test]
    fn test_extract_local_var_types_generic_parameters() {
        let source = r#"
            fn process(items: Vec<Item>, map: HashMap<String, Value>) {
            }
        "#;

        let tree = parse_rust(source);
        let func_node = find_function_node(&tree).unwrap();
        let types = extract_local_var_types(func_node, source);

        // Generic types should extract to base type only
        assert_eq!(types.get("items"), Some(&"Vec".to_string()));
        assert_eq!(types.get("map"), Some(&"HashMap".to_string()));
    }

    #[test]
    fn test_extract_local_var_types_let_statements() {
        let source = r#"
            fn process() {
                let x: Foo = Foo::new();
                let y: &Bar = get_bar();
            }
        "#;

        let tree = parse_rust(source);
        let func_node = find_function_node(&tree).unwrap();
        let types = extract_local_var_types(func_node, source);

        assert_eq!(types.get("x"), Some(&"Foo".to_string()));
        assert_eq!(types.get("y"), Some(&"Bar".to_string()));
    }

    #[test]
    fn test_extract_local_var_types_mixed() {
        let source = r#"
            fn process(input: Input) {
                let result: Result<Output, Error> = input.transform();
                let count: usize = 0;
            }
        "#;

        let tree = parse_rust(source);
        let func_node = find_function_node(&tree).unwrap();
        let types = extract_local_var_types(func_node, source);

        assert_eq!(types.get("input"), Some(&"Input".to_string()));
        assert_eq!(types.get("result"), Some(&"Result".to_string()));
        // Primitive types are also captured
        assert_eq!(types.get("count"), Some(&"usize".to_string()));
    }

    #[test]
    fn test_extract_local_var_types_no_type_annotation() {
        let source = r#"
            fn process() {
                let x = foo();
            }
        "#;

        let tree = parse_rust(source);
        let func_node = find_function_node(&tree).unwrap();
        let types = extract_local_var_types(func_node, source);

        // Variables without type annotations are not included
        assert!(!types.contains_key("x"));
    }

    // =========================================================================
    // Tests for is_primitive_type
    // =========================================================================

    #[test]
    fn test_is_primitive_type_integer_types() {
        assert!(is_primitive_type("i8"));
        assert!(is_primitive_type("i16"));
        assert!(is_primitive_type("i32"));
        assert!(is_primitive_type("i64"));
        assert!(is_primitive_type("i128"));
        assert!(is_primitive_type("isize"));
        assert!(is_primitive_type("u8"));
        assert!(is_primitive_type("u16"));
        assert!(is_primitive_type("u32"));
        assert!(is_primitive_type("u64"));
        assert!(is_primitive_type("u128"));
        assert!(is_primitive_type("usize"));
    }

    #[test]
    fn test_is_primitive_type_other_primitives() {
        assert!(is_primitive_type("bool"));
        assert!(is_primitive_type("char"));
        assert!(is_primitive_type("str"));
        assert!(is_primitive_type("f32"));
        assert!(is_primitive_type("f64"));
        assert!(is_primitive_type("Self"));
        assert!(is_primitive_type("()"));
    }

    #[test]
    fn test_is_primitive_type_non_primitives() {
        assert!(!is_primitive_type("String"));
        assert!(!is_primitive_type("Vec"));
        assert!(!is_primitive_type("Option"));
        assert!(!is_primitive_type("Result"));
        assert!(!is_primitive_type("MyStruct"));
    }

    // =========================================================================
    // Tests for find_child_by_kind
    // =========================================================================

    #[test]
    fn test_find_child_by_kind_exists() {
        let source = "fn foo() {}";
        let tree = parse_rust(source);
        let func_node = find_function_node(&tree).unwrap();

        let params = find_child_by_kind(func_node, "parameters");
        assert!(params.is_some());
    }

    #[test]
    fn test_find_child_by_kind_not_exists() {
        let source = "fn foo() {}";
        let tree = parse_rust(source);
        let func_node = find_function_node(&tree).unwrap();

        let where_clause = find_child_by_kind(func_node, "where_clause");
        assert!(where_clause.is_none());
    }

    // =========================================================================
    // Tests for extract_function_calls with import resolution
    // =========================================================================

    #[test]
    fn test_extract_function_calls_with_renamed_imports() {
        use crate::common::import_map::parse_rust_imports;

        // Source code with nested use statement and calls
        let source = r#"
use network::{
    http::{get as http_get, post as http_post},
    tcp::connect as tcp_connect,
};

pub fn make_requests() {
    http_get();
    http_post();
    tcp_connect();
}
        "#;

        let tree = parse_rust(source);
        let root = tree.root_node();

        // Build import map
        let import_map = parse_rust_imports(root, source);
        println!("Import map contents:");
        for (k, v) in import_map.mappings() {
            println!("  {} -> {}", k, v);
        }

        // Find the make_requests function
        let func_node = find_function_node(&tree).unwrap();
        println!("Function node kind: {}", func_node.kind());
        println!(
            "Function text: {}",
            func_node.utf8_text(source.as_bytes()).unwrap()
        );

        // Build resolution context
        let ctx = RustResolutionContext {
            import_map: &import_map,
            parent_scope: None,
            package_name: Some("test_crate"),
            current_module: None,
        };

        let local_vars = std::collections::HashMap::new();
        let generic_bounds = im::HashMap::new();
        let calls = extract_function_calls(func_node, source, &ctx, &local_vars, &generic_bounds);

        println!("Extracted calls:");
        for call in &calls {
            println!("  target: {}", call.target);
        }

        // Should have 3 calls with fully qualified names
        assert_eq!(calls.len(), 3);

        let targets: Vec<&str> = calls.iter().map(|c| c.target.as_str()).collect();
        assert!(
            targets.contains(&"test_crate::network::http::get"),
            "Expected test_crate::network::http::get in {:?}",
            targets
        );
        assert!(
            targets.contains(&"test_crate::network::http::post"),
            "Expected test_crate::network::http::post in {:?}",
            targets
        );
        assert!(
            targets.contains(&"test_crate::network::tcp::connect"),
            "Expected test_crate::network::tcp::connect in {:?}",
            targets
        );
    }

    #[test]
    fn test_extract_function_calls_with_generic_bounds() {
        let source = r#"
            fn process_item<T: Processor>(item: &T) {
                item.process();
            }
        "#;

        let tree = parse_rust(source);
        let func_node = find_function_node(&tree).unwrap();

        // Extract local vars - should map "item" -> "T"
        let local_vars = extract_local_var_types(func_node, source);
        eprintln!("local_vars: {:?}", local_vars);
        assert_eq!(local_vars.get("item"), Some(&"T".to_string()));

        // Build generic bounds - should map "T" -> ["test_crate::Processor"]
        let mut generic_bounds = im::HashMap::new();
        generic_bounds.insert("T".to_string(), vec!["test_crate::Processor".to_string()]);

        // Create a resolution context
        let import_map = crate::common::import_map::ImportMap::default();
        let ctx = RustResolutionContext {
            import_map: &import_map,
            parent_scope: Some("test_crate"),
            package_name: Some("test_crate"),
            current_module: None,
        };

        // Extract function calls
        let calls = extract_function_calls(func_node, source, &ctx, &local_vars, &generic_bounds);
        eprintln!(
            "calls: {:?}",
            calls.iter().map(|c| &c.target).collect::<Vec<_>>()
        );

        // Should resolve item.process() to test_crate::Processor::process
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].target, "test_crate::Processor::process");
    }

    #[test]
    fn test_extract_generics_with_bounds_resolves_correctly() {
        let source = r#"
            fn process_item<T: Processor>(item: &T) {
                item.process();
            }
        "#;

        let tree = parse_rust(source);
        let func_node = find_function_node(&tree).unwrap();

        // Find the type_parameters node
        let mut cursor = func_node.walk();
        let type_params_node = func_node
            .children(&mut cursor)
            .find(|c| c.kind() == "type_parameters");

        assert!(
            type_params_node.is_some(),
            "Should find type_parameters node"
        );
        let type_params_node = type_params_node.unwrap();

        // Create a resolution context
        let import_map = crate::common::import_map::ImportMap::default();
        let ctx = RustResolutionContext {
            import_map: &import_map,
            parent_scope: Some("test_crate"),
            package_name: Some("test_crate"),
            current_module: None,
        };

        // Extract generics with bounds - this should resolve "Processor" to "test_crate::Processor"
        let parsed = extract_generics_with_bounds(type_params_node, source, &ctx);
        eprintln!("parsed_generics params: {:?}", parsed.params);

        // Build the generic bounds map
        let generic_bounds = build_generic_bounds_map(&parsed);
        eprintln!("generic_bounds: {:?}", generic_bounds);

        // Should have T with bounds ["test_crate::Processor"]
        assert!(
            generic_bounds.contains_key("T"),
            "Should have T in generic_bounds"
        );
        let bounds = generic_bounds.get("T").unwrap();
        assert_eq!(bounds.len(), 1, "T should have one bound");
        assert_eq!(
            bounds[0], "test_crate::Processor",
            "Bound should be resolved to test_crate::Processor"
        );
    }
}
