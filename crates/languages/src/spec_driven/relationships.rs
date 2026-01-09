//! Relationship extraction for spec-driven entities
//!
//! This module provides extraction functions for different relationship types
//! based on the `RelationshipExtractor` enum variants.

use super::engine::SpecDrivenContext;
use crate::common::reference_resolution::{resolve_reference, ResolutionContext};
use codesearch_core::entities::{
    EntityRelationshipData, ReferenceType, SourceLocation, SourceReference,
};
use std::collections::HashSet;
use std::sync::OnceLock;
use streaming_iterator::StreamingIterator;
use tree_sitter::{Node, Query, QueryCursor};

// =============================================================================
// Query constants
// =============================================================================

/// Rust function call query
const RUST_CALL_QUERY: &str = r#"
[
  (call_expression
    function: (identifier) @callee)

  (call_expression
    function: (scoped_identifier) @callee)

  (call_expression
    function: (field_expression
      field: (field_identifier) @method_callee))
]
"#;

/// JavaScript/TypeScript function call query
const JS_CALL_QUERY: &str = r#"
[
  (call_expression
    function: (identifier) @callee)

  (call_expression
    function: (member_expression
      property: (property_identifier) @method_callee))
]
"#;

/// Rust type reference query
const RUST_TYPE_QUERY: &str = r#"
[
  (type_identifier) @type_ref
  (scoped_type_identifier) @scoped_type_ref
]
"#;

/// JavaScript/TypeScript type reference query
const JS_TYPE_QUERY: &str = r#"
[
  (type_identifier) @type_ref
  (generic_type
    name: (type_identifier) @generic_type_ref)
]
"#;

// =============================================================================
// Primitive type filters
// =============================================================================

/// Rust primitive types to filter out from type references
const RUST_PRIMITIVES: &[&str] = &[
    "i8", "i16", "i32", "i64", "i128", "isize", "u8", "u16", "u32", "u64", "u128", "usize", "f32",
    "f64", "bool", "char", "str", "String", "Self", "()", "Option", "Result", "Vec", "Box",
];

/// TypeScript/JavaScript primitive types to filter out
const JS_PRIMITIVES: &[&str] = &[
    "string",
    "number",
    "boolean",
    "void",
    "null",
    "undefined",
    "any",
    "unknown",
    "never",
    "object",
    "symbol",
    "bigint",
    "String",
    "Number",
    "Boolean",
    "Object",
    "Array",
    "Function",
    "Promise",
    "Map",
    "Set",
    "WeakMap",
    "WeakSet",
];

// =============================================================================
// Lazy query compilation
// =============================================================================

fn get_rust_call_query() -> Option<&'static Query> {
    static QUERY: OnceLock<Option<Query>> = OnceLock::new();
    QUERY
        .get_or_init(|| {
            let language = tree_sitter_rust::LANGUAGE.into();
            Query::new(&language, RUST_CALL_QUERY).ok()
        })
        .as_ref()
}

fn get_js_call_query() -> Option<&'static Query> {
    static QUERY: OnceLock<Option<Query>> = OnceLock::new();
    QUERY
        .get_or_init(|| {
            let language = tree_sitter_javascript::LANGUAGE.into();
            Query::new(&language, JS_CALL_QUERY).ok()
        })
        .as_ref()
}

fn get_ts_call_query() -> Option<&'static Query> {
    static QUERY: OnceLock<Option<Query>> = OnceLock::new();
    QUERY
        .get_or_init(|| {
            let language = tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();
            Query::new(&language, JS_CALL_QUERY).ok()
        })
        .as_ref()
}

fn get_rust_type_query() -> Option<&'static Query> {
    static QUERY: OnceLock<Option<Query>> = OnceLock::new();
    QUERY
        .get_or_init(|| {
            let language = tree_sitter_rust::LANGUAGE.into();
            Query::new(&language, RUST_TYPE_QUERY).ok()
        })
        .as_ref()
}

fn get_ts_type_query() -> Option<&'static Query> {
    static QUERY: OnceLock<Option<Query>> = OnceLock::new();
    QUERY
        .get_or_init(|| {
            let language = tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();
            Query::new(&language, JS_TYPE_QUERY).ok()
        })
        .as_ref()
}

// =============================================================================
// Helper functions
// =============================================================================

/// Extract the text content of a node
fn node_text<'a>(node: Node, source: &'a str) -> &'a str {
    node.utf8_text(source.as_bytes()).unwrap_or("")
}

/// Build a ResolutionContext from SpecDrivenContext
fn build_resolution_context<'a>(
    ctx: &'a SpecDrivenContext<'a>,
    parent_scope: Option<&'a str>,
) -> ResolutionContext<'a> {
    // Derive current module path from qualified name parent
    let current_module = parent_scope;

    ResolutionContext {
        import_map: ctx.import_map,
        parent_scope,
        package_name: ctx.package_name,
        current_module,
        path_config: ctx.path_config,
        edge_case_handlers: ctx.edge_case_handlers,
    }
}

/// Build a SourceReference from a resolved reference
fn build_source_reference(
    target: String,
    simple_name: String,
    is_external: bool,
    node: Node,
    ref_type: ReferenceType,
) -> Option<SourceReference> {
    SourceReference::builder()
        .target(target)
        .simple_name(simple_name)
        .is_external(is_external)
        .location(SourceLocation::from_tree_sitter_node(node))
        .ref_type(ref_type)
        .build()
        .ok()
}

/// Extract simple name from a potentially qualified name
fn extract_simple_name(name: &str) -> &str {
    // Handle both Rust (::) and JS (.) separators
    name.rsplit("::")
        .next()
        .or_else(|| name.rsplit('.').next())
        .unwrap_or(name)
}

// =============================================================================
// Call extraction
// =============================================================================

/// Extract function calls from a node (typically function/method body)
pub fn extract_function_calls(
    node: Node,
    ctx: &SpecDrivenContext,
    parent_scope: Option<&str>,
) -> Vec<SourceReference> {
    let query = match ctx.language_str {
        "rust" => get_rust_call_query(),
        "javascript" => get_js_call_query(),
        "typescript" | "tsx" => get_ts_call_query(),
        _ => return Vec::new(),
    };

    let Some(query) = query else {
        return Vec::new();
    };

    let mut calls = Vec::new();
    let mut seen = HashSet::new();
    let mut cursor = QueryCursor::new();

    let mut matches = cursor.matches(query, node, ctx.source.as_bytes());

    while let Some(query_match) = matches.next() {
        for capture in query_match.captures {
            let callee_node = capture.node;
            let callee_text = node_text(callee_node, ctx.source);

            if callee_text.is_empty() {
                continue;
            }

            // Skip if we've already processed this call at this location
            let key = (callee_text.to_string(), callee_node.start_byte());
            if seen.contains(&key) {
                continue;
            }
            seen.insert(key);

            // Resolve the reference
            let simple_name = extract_simple_name(callee_text);
            let resolution_ctx = build_resolution_context(ctx, parent_scope);
            let resolved = resolve_reference(callee_text, simple_name, &resolution_ctx);

            if let Some(source_ref) = build_source_reference(
                resolved.target,
                resolved.simple_name,
                resolved.is_external,
                callee_node,
                ReferenceType::Call,
            ) {
                calls.push(source_ref);
            }
        }
    }

    calls
}

// =============================================================================
// Type reference extraction
// =============================================================================

/// Extract type references from a node
pub fn extract_type_references(
    node: Node,
    ctx: &SpecDrivenContext,
    parent_scope: Option<&str>,
) -> Vec<SourceReference> {
    let (query, primitives) = match ctx.language_str {
        "rust" => (get_rust_type_query(), RUST_PRIMITIVES),
        "typescript" | "tsx" => (get_ts_type_query(), JS_PRIMITIVES),
        _ => return Vec::new(),
    };

    let Some(query) = query else {
        return Vec::new();
    };

    let mut type_refs = Vec::new();
    let mut seen = HashSet::new();
    let mut cursor = QueryCursor::new();

    let mut matches = cursor.matches(query, node, ctx.source.as_bytes());

    while let Some(query_match) = matches.next() {
        for capture in query_match.captures {
            let type_node = capture.node;
            let type_text = node_text(type_node, ctx.source);

            if type_text.is_empty() {
                continue;
            }

            // Filter primitive types
            if primitives.contains(&type_text) {
                continue;
            }

            // Skip duplicates
            let key = (type_text.to_string(), type_node.start_byte());
            if seen.contains(&key) {
                continue;
            }
            seen.insert(key);

            // Resolve the reference
            let simple_name = extract_simple_name(type_text);
            let resolution_ctx = build_resolution_context(ctx, parent_scope);
            let resolved = resolve_reference(type_text, simple_name, &resolution_ctx);

            if let Some(source_ref) = build_source_reference(
                resolved.target,
                resolved.simple_name,
                resolved.is_external,
                type_node,
                ReferenceType::TypeUsage,
            ) {
                type_refs.push(source_ref);
            }
        }
    }

    type_refs
}

// =============================================================================
// Class relationship extraction (JS/TS)
// =============================================================================

/// Extract class inheritance (extends) and interface implementation (implements)
pub fn extract_class_relationships(
    node: Node,
    ctx: &SpecDrivenContext,
    parent_scope: Option<&str>,
) -> (Vec<SourceReference>, Vec<SourceReference>) {
    let mut extends = Vec::new();
    let mut implements = Vec::new();

    // Find heritage clause
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "class_heritage" => {
                // Process heritage clause children
                let mut heritage_cursor = child.walk();
                for heritage_child in child.children(&mut heritage_cursor) {
                    match heritage_child.kind() {
                        "extends_clause" => {
                            if let Some(type_ref) =
                                extract_extends_type(heritage_child, ctx, parent_scope)
                            {
                                extends.push(type_ref);
                            }
                        }
                        "implements_clause" => {
                            let impl_refs =
                                extract_implements_types(heritage_child, ctx, parent_scope);
                            implements.extend(impl_refs);
                        }
                        _ => {}
                    }
                }
            }
            // For simpler AST structures
            "extends_clause" => {
                if let Some(type_ref) = extract_extends_type(child, ctx, parent_scope) {
                    extends.push(type_ref);
                }
            }
            "implements_clause" => {
                let impl_refs = extract_implements_types(child, ctx, parent_scope);
                implements.extend(impl_refs);
            }
            _ => {}
        }
    }

    (extends, implements)
}

fn extract_extends_type(
    extends_clause: Node,
    ctx: &SpecDrivenContext,
    parent_scope: Option<&str>,
) -> Option<SourceReference> {
    // Find the type identifier in extends clause
    let mut cursor = extends_clause.walk();
    for child in extends_clause.children(&mut cursor) {
        if child.kind() == "identifier" || child.kind() == "type_identifier" {
            let type_text = node_text(child, ctx.source);
            if !type_text.is_empty() {
                let simple_name = extract_simple_name(type_text);
                let resolution_ctx = build_resolution_context(ctx, parent_scope);
                let resolved = resolve_reference(type_text, simple_name, &resolution_ctx);
                return build_source_reference(
                    resolved.target,
                    resolved.simple_name,
                    resolved.is_external,
                    child,
                    ReferenceType::Extends,
                );
            }
        }
    }
    None
}

fn extract_implements_types(
    implements_clause: Node,
    ctx: &SpecDrivenContext,
    parent_scope: Option<&str>,
) -> Vec<SourceReference> {
    let mut refs = Vec::new();

    let mut cursor = implements_clause.walk();
    for child in implements_clause.children(&mut cursor) {
        if child.kind() == "type_identifier" || child.kind() == "identifier" {
            let type_text = node_text(child, ctx.source);
            if !type_text.is_empty() {
                let simple_name = extract_simple_name(type_text);
                let resolution_ctx = build_resolution_context(ctx, parent_scope);
                let resolved = resolve_reference(type_text, simple_name, &resolution_ctx);
                if let Some(source_ref) = build_source_reference(
                    resolved.target,
                    resolved.simple_name,
                    resolved.is_external,
                    child,
                    ReferenceType::Implements,
                ) {
                    refs.push(source_ref);
                }
            }
        }
    }

    refs
}

// =============================================================================
// Trait relationship extraction (Rust)
// =============================================================================

/// Extract supertrait relationships from trait bounds
pub fn extract_trait_bounds(
    node: Node,
    ctx: &SpecDrivenContext,
    parent_scope: Option<&str>,
) -> Vec<SourceReference> {
    let mut refs = Vec::new();

    // Find trait_bounds child
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "trait_bounds" {
            extract_bounds_recursive(child, ctx, parent_scope, &mut refs);
        }
    }

    refs
}

fn extract_bounds_recursive(
    node: Node,
    ctx: &SpecDrivenContext,
    parent_scope: Option<&str>,
    refs: &mut Vec<SourceReference>,
) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "type_identifier" => {
                let type_text = node_text(child, ctx.source);
                if !type_text.is_empty() && !RUST_PRIMITIVES.contains(&type_text) {
                    let simple_name = extract_simple_name(type_text);
                    let resolution_ctx = build_resolution_context(ctx, parent_scope);
                    let resolved = resolve_reference(type_text, simple_name, &resolution_ctx);
                    if let Some(source_ref) = build_source_reference(
                        resolved.target,
                        resolved.simple_name,
                        resolved.is_external,
                        child,
                        ReferenceType::Extends,
                    ) {
                        refs.push(source_ref);
                    }
                }
            }
            "scoped_type_identifier" => {
                let type_text = node_text(child, ctx.source);
                if !type_text.is_empty() {
                    let simple_name = extract_simple_name(type_text);
                    let resolution_ctx = build_resolution_context(ctx, parent_scope);
                    let resolved = resolve_reference(type_text, simple_name, &resolution_ctx);
                    if let Some(source_ref) = build_source_reference(
                        resolved.target,
                        resolved.simple_name,
                        resolved.is_external,
                        child,
                        ReferenceType::Extends,
                    ) {
                        refs.push(source_ref);
                    }
                }
            }
            _ => {
                // Recurse into children
                extract_bounds_recursive(child, ctx, parent_scope, refs);
            }
        }
    }
}

// =============================================================================
// Interface relationship extraction (TypeScript)
// =============================================================================

/// Extract interface extends relationships
pub fn extract_interface_extends(
    node: Node,
    ctx: &SpecDrivenContext,
    parent_scope: Option<&str>,
) -> Vec<SourceReference> {
    let mut refs = Vec::new();

    // Find extends_type_clause or extends_clause
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "extends_type_clause" || child.kind() == "extends_clause" {
            extract_type_list(child, ctx, parent_scope, &mut refs, ReferenceType::Extends);
        }
    }

    refs
}

fn extract_type_list(
    node: Node,
    ctx: &SpecDrivenContext,
    parent_scope: Option<&str>,
    refs: &mut Vec<SourceReference>,
    ref_type: ReferenceType,
) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "type_identifier" | "identifier" => {
                let type_text = node_text(child, ctx.source);
                if !type_text.is_empty() && !JS_PRIMITIVES.contains(&type_text) {
                    let simple_name = extract_simple_name(type_text);
                    let resolution_ctx = build_resolution_context(ctx, parent_scope);
                    let resolved = resolve_reference(type_text, simple_name, &resolution_ctx);
                    if let Some(source_ref) = build_source_reference(
                        resolved.target,
                        resolved.simple_name,
                        resolved.is_external,
                        child,
                        ref_type,
                    ) {
                        refs.push(source_ref);
                    }
                }
            }
            "generic_type" => {
                // Extract base type from generic
                if let Some(name_child) = child.child_by_field_name("name") {
                    let type_text = node_text(name_child, ctx.source);
                    if !type_text.is_empty() && !JS_PRIMITIVES.contains(&type_text) {
                        let simple_name = extract_simple_name(type_text);
                        let resolution_ctx = build_resolution_context(ctx, parent_scope);
                        let resolved = resolve_reference(type_text, simple_name, &resolution_ctx);
                        if let Some(source_ref) = build_source_reference(
                            resolved.target,
                            resolved.simple_name,
                            resolved.is_external,
                            name_child,
                            ref_type,
                        ) {
                            refs.push(source_ref);
                        }
                    }
                }
            }
            _ => {
                // Recurse for nested structures
                extract_type_list(child, ctx, parent_scope, refs, ref_type);
            }
        }
    }
}

// =============================================================================
// Main dispatch function
// =============================================================================

use super::RelationshipExtractor;

/// Extract relationships using the specified extractor
pub fn extract_relationships(
    extractor: RelationshipExtractor,
    node: Node,
    ctx: &SpecDrivenContext,
    parent_scope: Option<&str>,
) -> EntityRelationshipData {
    match extractor {
        RelationshipExtractor::ExtractFunctionRelationships => EntityRelationshipData {
            calls: extract_function_calls(node, ctx, parent_scope),
            uses_types: extract_type_references(node, ctx, parent_scope),
            ..Default::default()
        },
        RelationshipExtractor::ExtractClassRelationships => {
            let (extends, implements) = extract_class_relationships(node, ctx, parent_scope);
            EntityRelationshipData {
                extends,
                implements,
                ..Default::default()
            }
        }
        RelationshipExtractor::ExtractTraitRelationships => EntityRelationshipData {
            extended_types: extract_trait_bounds(node, ctx, parent_scope),
            ..Default::default()
        },
        RelationshipExtractor::ExtractInterfaceRelationships => {
            let extended_types = extract_interface_extends(node, ctx, parent_scope);
            let uses_types = extract_type_references(node, ctx, parent_scope);
            EntityRelationshipData {
                extended_types,
                uses_types,
                ..Default::default()
            }
        }
        RelationshipExtractor::ExtractTypeRelationships => EntityRelationshipData {
            uses_types: extract_type_references(node, ctx, parent_scope),
            ..Default::default()
        },
        RelationshipExtractor::ExtractModuleRelationships => {
            // Module relationships (imports/reexports) require more complex handling
            // For now, return empty - will implement in future phase
            EntityRelationshipData::default()
        }
    }
}
