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
// Import/Export query constants
// =============================================================================

/// JavaScript import statement query
const JS_IMPORT_QUERY: &str = r#"
(import_statement
  source: (string) @source)
"#;

/// TypeScript import statement query (same as JS)
const TS_IMPORT_QUERY: &str = r#"
(import_statement
  source: (string) @source)
"#;

/// JavaScript re-export query (export statements with a source)
const JS_REEXPORT_QUERY: &str = r#"
(export_statement
  source: (string) @source)
"#;

/// Rust use declaration query
const RUST_USE_QUERY: &str = r#"
(use_declaration) @use_decl
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
            match Query::new(&language, RUST_CALL_QUERY) {
                Ok(q) => Some(q),
                Err(e) => {
                    tracing::error!(
                        query = "RUST_CALL_QUERY",
                        error = %e,
                        "Failed to compile tree-sitter query - call extraction disabled for Rust"
                    );
                    None
                }
            }
        })
        .as_ref()
}

fn get_js_call_query() -> Option<&'static Query> {
    static QUERY: OnceLock<Option<Query>> = OnceLock::new();
    QUERY
        .get_or_init(|| {
            let language = tree_sitter_javascript::LANGUAGE.into();
            match Query::new(&language, JS_CALL_QUERY) {
                Ok(q) => Some(q),
                Err(e) => {
                    tracing::error!(
                        query = "JS_CALL_QUERY",
                        error = %e,
                        "Failed to compile tree-sitter query - call extraction disabled for JavaScript"
                    );
                    None
                }
            }
        })
        .as_ref()
}

fn get_ts_call_query() -> Option<&'static Query> {
    static QUERY: OnceLock<Option<Query>> = OnceLock::new();
    QUERY
        .get_or_init(|| {
            let language = tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();
            match Query::new(&language, JS_CALL_QUERY) {
                Ok(q) => Some(q),
                Err(e) => {
                    tracing::error!(
                        query = "JS_CALL_QUERY (TypeScript)",
                        error = %e,
                        "Failed to compile tree-sitter query - call extraction disabled for TypeScript"
                    );
                    None
                }
            }
        })
        .as_ref()
}

fn get_rust_type_query() -> Option<&'static Query> {
    static QUERY: OnceLock<Option<Query>> = OnceLock::new();
    QUERY
        .get_or_init(|| {
            let language = tree_sitter_rust::LANGUAGE.into();
            match Query::new(&language, RUST_TYPE_QUERY) {
                Ok(q) => Some(q),
                Err(e) => {
                    tracing::error!(
                        query = "RUST_TYPE_QUERY",
                        error = %e,
                        "Failed to compile tree-sitter query - type extraction disabled for Rust"
                    );
                    None
                }
            }
        })
        .as_ref()
}

fn get_ts_type_query() -> Option<&'static Query> {
    static QUERY: OnceLock<Option<Query>> = OnceLock::new();
    QUERY
        .get_or_init(|| {
            let language = tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();
            match Query::new(&language, JS_TYPE_QUERY) {
                Ok(q) => Some(q),
                Err(e) => {
                    tracing::error!(
                        query = "JS_TYPE_QUERY (TypeScript)",
                        error = %e,
                        "Failed to compile tree-sitter query - type extraction disabled for TypeScript"
                    );
                    None
                }
            }
        })
        .as_ref()
}

fn get_js_import_query() -> Option<&'static Query> {
    static QUERY: OnceLock<Option<Query>> = OnceLock::new();
    QUERY
        .get_or_init(|| {
            let language = tree_sitter_javascript::LANGUAGE.into();
            match Query::new(&language, JS_IMPORT_QUERY) {
                Ok(q) => Some(q),
                Err(e) => {
                    tracing::error!(
                        query = "JS_IMPORT_QUERY",
                        error = %e,
                        "Failed to compile tree-sitter query - import extraction disabled for JavaScript"
                    );
                    None
                }
            }
        })
        .as_ref()
}

fn get_ts_import_query() -> Option<&'static Query> {
    static QUERY: OnceLock<Option<Query>> = OnceLock::new();
    QUERY
        .get_or_init(|| {
            let language = tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();
            match Query::new(&language, TS_IMPORT_QUERY) {
                Ok(q) => Some(q),
                Err(e) => {
                    tracing::error!(
                        query = "TS_IMPORT_QUERY",
                        error = %e,
                        "Failed to compile tree-sitter query - import extraction disabled for TypeScript"
                    );
                    None
                }
            }
        })
        .as_ref()
}

fn get_js_reexport_query() -> Option<&'static Query> {
    static QUERY: OnceLock<Option<Query>> = OnceLock::new();
    QUERY
        .get_or_init(|| {
            let language = tree_sitter_javascript::LANGUAGE.into();
            match Query::new(&language, JS_REEXPORT_QUERY) {
                Ok(q) => Some(q),
                Err(e) => {
                    tracing::error!(
                        query = "JS_REEXPORT_QUERY",
                        error = %e,
                        "Failed to compile tree-sitter query - reexport extraction disabled for JavaScript"
                    );
                    None
                }
            }
        })
        .as_ref()
}

fn get_ts_reexport_query() -> Option<&'static Query> {
    static QUERY: OnceLock<Option<Query>> = OnceLock::new();
    QUERY
        .get_or_init(|| {
            let language = tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();
            match Query::new(&language, JS_REEXPORT_QUERY) {
                Ok(q) => Some(q),
                Err(e) => {
                    tracing::error!(
                        query = "JS_REEXPORT_QUERY (TypeScript)",
                        error = %e,
                        "Failed to compile tree-sitter query - reexport extraction disabled for TypeScript"
                    );
                    None
                }
            }
        })
        .as_ref()
}

fn get_rust_use_query() -> Option<&'static Query> {
    static QUERY: OnceLock<Option<Query>> = OnceLock::new();
    QUERY
        .get_or_init(|| {
            let language = tree_sitter_rust::LANGUAGE.into();
            match Query::new(&language, RUST_USE_QUERY) {
                Ok(q) => Some(q),
                Err(e) => {
                    tracing::error!(
                        query = "RUST_USE_QUERY",
                        error = %e,
                        "Failed to compile tree-sitter query - use extraction disabled for Rust"
                    );
                    None
                }
            }
        })
        .as_ref()
}

// =============================================================================
// Helper functions
// =============================================================================

/// Extract the text content of a node
fn node_text<'a>(node: Node, source: &'a str) -> &'a str {
    match node.utf8_text(source.as_bytes()) {
        Ok(text) => text,
        Err(e) => {
            tracing::warn!(
                node_kind = node.kind(),
                start_byte = node.start_byte(),
                end_byte = node.end_byte(),
                error = %e,
                "Failed to extract node text as UTF-8, treating as empty"
            );
            ""
        }
    }
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
        _ => {
            tracing::trace!(
                language = ctx.language_str,
                "Function call extraction not supported for language"
            );
            return Vec::new();
        }
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

/// Node kinds that represent child entities - type references inside these should
/// be attributed to the child entity, not the parent container.
///
/// When extracting type references from a container (e.g., a struct or impl block),
/// we skip references inside child entities because those entities are extracted
/// separately and will have their own type references. This prevents duplicate
/// attribution and ensures each type reference is associated with its immediate owner.
const RUST_CHILD_ENTITY_KINDS: &[&str] = &[
    "field_declaration", // struct fields
    "enum_variant",      // enum variants
    "function_item",     // methods in impl blocks
    "const_item",        // associated consts
    "type_item",         // associated types
];

/// Node kinds that represent child entities for JS/TS - type references inside these
/// should be attributed to the child entity, not the parent container.
///
/// See [`RUST_CHILD_ENTITY_KINDS`] for rationale.
const JS_CHILD_ENTITY_KINDS: &[&str] = &[
    "public_field_definition",  // class fields
    "private_field_definition", // private class fields
    "field_definition",         // generic field
    "method_definition",        // class methods
    "property_signature",       // interface properties
    "method_signature",         // interface methods
];

/// Check if a type node is inside a child entity declaration (relative to the parent).
///
/// Walks up from `type_node` toward `parent_node`, returning true if any intermediate
/// ancestor is a child entity kind. Returns false if we reach `parent_node` without
/// encountering a child entity.
fn is_inside_child_entity(type_node: Node, parent_node: Node, child_kinds: &[&str]) -> bool {
    let mut current = type_node;
    while let Some(ancestor) = current.parent() {
        // Stop if we've reached the parent node
        if ancestor.id() == parent_node.id() {
            return false;
        }
        // Check if this ancestor is a child entity kind
        if child_kinds.contains(&ancestor.kind()) {
            return true;
        }
        current = ancestor;
    }
    false
}

/// Extract type references from a node
pub fn extract_type_references(
    node: Node,
    ctx: &SpecDrivenContext,
    parent_scope: Option<&str>,
) -> Vec<SourceReference> {
    let (query, primitives, child_kinds) = match ctx.language_str {
        "rust" => (
            get_rust_type_query(),
            RUST_PRIMITIVES,
            RUST_CHILD_ENTITY_KINDS,
        ),
        "typescript" | "tsx" => (get_ts_type_query(), JS_PRIMITIVES, JS_CHILD_ENTITY_KINDS),
        _ => {
            tracing::trace!(
                language = ctx.language_str,
                "Type reference extraction not supported for language"
            );
            return Vec::new();
        }
    };

    let Some(query) = query else {
        // Query compilation already logged an error
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

            // Skip type references inside child entity declarations
            // These will be attributed to the child entity, not the parent
            if is_inside_child_entity(type_node, node, child_kinds) {
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
// Impl block relationship extraction (Rust)
// =============================================================================

/// Extract IMPLEMENTS relationship from a Rust trait impl block.
///
/// For `impl Trait for Type`, extracts a reference to the Trait.
/// Note: The implementing Type is not extracted here as a relationship;
/// the impl block's qualified name already encodes this via the type name.
pub fn extract_impl_trait_reference(
    node: Node,
    ctx: &SpecDrivenContext,
    parent_scope: Option<&str>,
) -> Vec<SourceReference> {
    let mut refs = Vec::new();

    // Find the trait field in impl_item
    match node.child_by_field_name("trait") {
        Some(trait_node) => {
            let type_text = node_text(trait_node, ctx.source);
            if type_text.is_empty() {
                tracing::trace!(
                    node_kind = node.kind(),
                    "Trait node found but text is empty"
                );
            } else {
                let simple_name = extract_simple_name(type_text);
                let resolution_ctx = build_resolution_context(ctx, parent_scope);
                let resolved = resolve_reference(type_text, simple_name, &resolution_ctx);
                if let Some(source_ref) = build_source_reference(
                    resolved.target,
                    resolved.simple_name,
                    resolved.is_external,
                    trait_node,
                    ReferenceType::Implements,
                ) {
                    refs.push(source_ref);
                }
            }
        }
        None => {
            tracing::trace!(
                node_kind = node.kind(),
                "extract_impl_trait_reference called on node without trait field"
            );
        }
    }

    refs
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
// Module relationship extraction (imports/reexports)
// =============================================================================

/// Extract import relationships from a module
pub fn extract_module_imports(
    node: Node,
    ctx: &SpecDrivenContext,
    parent_scope: Option<&str>,
) -> Vec<SourceReference> {
    match ctx.language_str {
        "javascript" => extract_js_imports(node, ctx, parent_scope),
        "typescript" | "tsx" => extract_ts_imports(node, ctx, parent_scope),
        "rust" => extract_rust_imports(node, ctx, parent_scope),
        _ => Vec::new(),
    }
}

/// Extract re-export relationships from a module
pub fn extract_module_reexports(
    node: Node,
    ctx: &SpecDrivenContext,
    parent_scope: Option<&str>,
) -> Vec<SourceReference> {
    match ctx.language_str {
        "javascript" => extract_js_reexports(node, ctx, parent_scope),
        "typescript" | "tsx" => extract_ts_reexports(node, ctx, parent_scope),
        "rust" => extract_rust_reexports(node, ctx, parent_scope),
        _ => Vec::new(),
    }
}

/// Extract JavaScript imports
fn extract_js_imports(
    node: Node,
    ctx: &SpecDrivenContext,
    parent_scope: Option<&str>,
) -> Vec<SourceReference> {
    let Some(query) = get_js_import_query() else {
        return Vec::new();
    };
    extract_js_ts_imports_with_query(node, ctx, parent_scope, query)
}

/// Extract TypeScript imports
fn extract_ts_imports(
    node: Node,
    ctx: &SpecDrivenContext,
    parent_scope: Option<&str>,
) -> Vec<SourceReference> {
    let Some(query) = get_ts_import_query() else {
        return Vec::new();
    };
    extract_js_ts_imports_with_query(node, ctx, parent_scope, query)
}

/// Check if the file is a folder module (index.ts/index.js in a subdirectory)
///
/// A folder module is an index file that represents its containing directory,
/// like `models/index.ts` representing the `models` module. Root-level index
/// files (e.g., `src/index.ts` at the source root) are NOT folder modules.
///
/// This matters for relative import resolution: folder modules resolve `./foo`
/// relative to the folder they represent, not their parent directory.
fn is_folder_module(ctx: &SpecDrivenContext) -> bool {
    // Must be named "index"
    let is_index = ctx
        .file_path
        .file_stem()
        .and_then(|s| s.to_str())
        .is_some_and(|name| name == "index");

    if !is_index {
        return false;
    }

    // Must have a parent directory relative to source root
    // (i.e., not be at the root level)
    let Some(source_root) = ctx.source_root else {
        tracing::debug!(
            file_path = ?ctx.file_path,
            "Cannot determine folder module status: no source_root configured for index file"
        );
        return false;
    };

    match ctx.file_path.strip_prefix(source_root) {
        Ok(rel) => rel
            .parent()
            .is_some_and(|parent| !parent.as_os_str().is_empty()),
        Err(e) => {
            tracing::debug!(
                file_path = ?ctx.file_path,
                source_root = ?source_root,
                error = %e,
                "Index file path not under source root - folder module detection failed"
            );
            false
        }
    }
}

/// Common implementation for JS/TS import extraction
fn extract_js_ts_imports_with_query(
    node: Node,
    ctx: &SpecDrivenContext,
    parent_scope: Option<&str>,
    query: &Query,
) -> Vec<SourceReference> {
    let mut imports = Vec::new();
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(query, node, ctx.source.as_bytes());
    let folder_module = is_folder_module(ctx);

    while let Some(query_match) = matches.next() {
        for capture in query_match.captures {
            let source_node = capture.node;
            let source_text = node_text(source_node, ctx.source);
            // Remove quotes from source path
            let source_path = source_text.trim_matches(|c| c == '"' || c == '\'');

            if source_path.is_empty() {
                continue;
            }

            // Get the parent import_statement to extract specifiers
            let Some(import_stmt) = source_node.parent() else {
                continue;
            };

            // Extract each import specifier
            let specifiers = extract_js_import_specifiers_from_stmt(import_stmt, ctx.source);

            for (local_name, original_name, spec_node) in specifiers {
                // Resolve the import path
                let resolved_path =
                    resolve_js_import_path(source_path, parent_scope, folder_module);

                // Build target qualified name
                let target = if original_name == "*" {
                    // Namespace import: import * as Utils from './utils' -> utils
                    resolved_path.clone()
                } else if original_name == "default" {
                    // Default import: import Foo from './utils' -> utils.default
                    format!("{resolved_path}.default")
                } else {
                    // Named import: import { foo } from './utils' -> utils.foo
                    format!("{resolved_path}.{original_name}")
                };

                if let Some(source_ref) = build_source_reference(
                    target,
                    local_name,
                    resolved_path.starts_with("external."),
                    spec_node,
                    ReferenceType::Import,
                ) {
                    imports.push(source_ref);
                }
            }
        }
    }

    imports
}

/// Extract import specifiers from a JS/TS import statement
/// Returns Vec<(local_name, original_name, node)>
fn extract_js_import_specifiers_from_stmt<'a>(
    import_stmt: Node<'a>,
    source: &str,
) -> Vec<(String, String, Node<'a>)> {
    let mut specifiers = Vec::new();
    let mut cursor = import_stmt.walk();

    for child in import_stmt.children(&mut cursor) {
        if child.kind() == "import_clause" {
            extract_js_import_clause_specifiers(child, source, &mut specifiers);
        }
    }

    specifiers
}

/// Extract specifiers from an import clause
fn extract_js_import_clause_specifiers<'a>(
    clause: Node<'a>,
    source: &str,
    specifiers: &mut Vec<(String, String, Node<'a>)>,
) {
    let mut cursor = clause.walk();

    for child in clause.children(&mut cursor) {
        match child.kind() {
            "identifier" => {
                // Default import: import foo from './bar'
                let name = node_text(child, source).to_string();
                specifiers.push((name, "default".to_string(), child));
            }
            "named_imports" => {
                // Named imports: import { foo, bar as baz } from './mod'
                let mut inner_cursor = child.walk();
                for spec in child.children(&mut inner_cursor) {
                    if spec.kind() == "import_specifier" {
                        if let Some((local, orig)) = extract_single_js_specifier(spec, source) {
                            specifiers.push((local, orig, spec));
                        }
                    }
                }
            }
            "namespace_import" => {
                // Namespace import: import * as foo from './bar'
                let mut inner_cursor = child.walk();
                for inner in child.children(&mut inner_cursor) {
                    if inner.kind() == "identifier" {
                        let name = node_text(inner, source).to_string();
                        specifiers.push((name, "*".to_string(), child));
                    }
                }
            }
            _ => {}
        }
    }
}

/// Extract a single import specifier's local and original names
fn extract_single_js_specifier(spec: Node, source: &str) -> Option<(String, String)> {
    let original = spec
        .child_by_field_name("name")
        .map(|n| node_text(n, source).to_string())?;
    let local = spec
        .child_by_field_name("alias")
        .map(|n| node_text(n, source).to_string())
        .unwrap_or_else(|| original.clone());
    Some((local, original))
}

/// Resolve JS/TS import path to absolute module path
///
/// The `is_folder_module` flag indicates whether the importing module is a folder
/// entry point (index.ts/index.js). For folder modules, relative imports like `./foo`
/// resolve differently because the module path already represents the folder.
fn resolve_js_import_path(
    source_path: &str,
    parent_scope: Option<&str>,
    is_folder_module: bool,
) -> String {
    // Handle relative imports
    if source_path.starts_with('.') {
        match parent_scope {
            Some(scope) => {
                // Use folder-aware resolution for index.ts/index.js files
                let resolved = if is_folder_module {
                    crate::common::import_map::resolve_relative_import_for_folder_module(
                        scope,
                        source_path,
                    )
                } else {
                    crate::common::import_map::resolve_relative_import(scope, source_path)
                };
                match resolved {
                    Some(path) => return path,
                    None => {
                        tracing::debug!(
                            source_path = source_path,
                            parent_scope = scope,
                            is_folder_module = is_folder_module,
                            "Relative import resolution failed, treating as external"
                        );
                        return format!("external.{source_path}");
                    }
                }
            }
            None => {
                tracing::debug!(
                    source_path = source_path,
                    "Relative import path has no parent_scope context, treating as external"
                );
            }
        }
    }

    // Bare specifier (external package)
    format!("external.{source_path}")
}

/// Extract JavaScript re-exports
fn extract_js_reexports(
    node: Node,
    ctx: &SpecDrivenContext,
    parent_scope: Option<&str>,
) -> Vec<SourceReference> {
    let Some(query) = get_js_reexport_query() else {
        return Vec::new();
    };
    extract_js_ts_reexports_with_query(node, ctx, parent_scope, query)
}

/// Extract TypeScript re-exports
fn extract_ts_reexports(
    node: Node,
    ctx: &SpecDrivenContext,
    parent_scope: Option<&str>,
) -> Vec<SourceReference> {
    let Some(query) = get_ts_reexport_query() else {
        return Vec::new();
    };
    extract_js_ts_reexports_with_query(node, ctx, parent_scope, query)
}

/// Common implementation for JS/TS re-export extraction
fn extract_js_ts_reexports_with_query(
    node: Node,
    ctx: &SpecDrivenContext,
    parent_scope: Option<&str>,
    query: &Query,
) -> Vec<SourceReference> {
    let mut reexports = Vec::new();
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(query, node, ctx.source.as_bytes());

    while let Some(query_match) = matches.next() {
        for capture in query_match.captures {
            let source_node = capture.node;
            let source_text = node_text(source_node, ctx.source);
            let source_path = source_text.trim_matches(|c| c == '"' || c == '\'');

            if source_path.is_empty() {
                continue;
            }

            let Some(export_stmt) = source_node.parent() else {
                continue;
            };

            // Resolve the source module path
            let resolved_path =
                resolve_js_import_path(source_path, parent_scope, is_folder_module(ctx));

            // Check if it's a star re-export or named re-export
            let specifiers = extract_js_export_specifiers(export_stmt, ctx.source);

            if specifiers.is_empty() {
                // Star re-export: export * from './module' -> re-export the whole module
                if let Some(source_ref) = build_source_reference(
                    resolved_path.clone(),
                    source_path.to_string(),
                    resolved_path.starts_with("external."),
                    export_stmt,
                    ReferenceType::Reexport,
                ) {
                    reexports.push(source_ref);
                }
            } else {
                // Named re-exports
                for (local_name, original_name, spec_node) in specifiers {
                    let target = if original_name == "default" {
                        format!("{resolved_path}.default")
                    } else {
                        format!("{resolved_path}.{original_name}")
                    };

                    if let Some(source_ref) = build_source_reference(
                        target,
                        local_name,
                        resolved_path.starts_with("external."),
                        spec_node,
                        ReferenceType::Reexport,
                    ) {
                        reexports.push(source_ref);
                    }
                }
            }
        }
    }

    reexports
}

/// Extract export specifiers from an export statement
fn extract_js_export_specifiers<'a>(
    export_stmt: Node<'a>,
    source: &str,
) -> Vec<(String, String, Node<'a>)> {
    let mut specifiers = Vec::new();
    let mut cursor = export_stmt.walk();

    for child in export_stmt.children(&mut cursor) {
        if child.kind() == "export_clause" {
            let mut inner_cursor = child.walk();
            for spec in child.children(&mut inner_cursor) {
                if spec.kind() == "export_specifier" {
                    if let Some((local, orig)) = extract_single_js_export_specifier(spec, source) {
                        specifiers.push((local, orig, spec));
                    }
                }
            }
        } else if child.kind() == "namespace_export" {
            // export * as Namespace from './module'
            let mut inner_cursor = child.walk();
            for inner in child.children(&mut inner_cursor) {
                if inner.kind() == "identifier" {
                    let name = node_text(inner, source).to_string();
                    specifiers.push((name.clone(), "*".to_string(), child));
                }
            }
        }
    }

    specifiers
}

/// Extract local and original names from an export specifier
fn extract_single_js_export_specifier(spec: Node, source: &str) -> Option<(String, String)> {
    let original = spec
        .child_by_field_name("name")
        .map(|n| node_text(n, source).to_string())?;
    let local = spec
        .child_by_field_name("alias")
        .map(|n| node_text(n, source).to_string())
        .unwrap_or_else(|| original.clone());
    Some((local, original))
}

/// Extract Rust imports (use declarations)
fn extract_rust_imports(
    node: Node,
    ctx: &SpecDrivenContext,
    _parent_scope: Option<&str>,
) -> Vec<SourceReference> {
    let Some(query) = get_rust_use_query() else {
        return Vec::new();
    };

    let mut imports = Vec::new();
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(query, node, ctx.source.as_bytes());

    while let Some(query_match) = matches.next() {
        for capture in query_match.captures {
            let use_decl = capture.node;

            // Check visibility - if public, it's a re-export, not an import
            let is_pub = has_pub_visibility(use_decl);
            if is_pub {
                continue; // Skip pub use, handled by reexports
            }

            // Extract the imported paths
            let paths = extract_rust_use_paths(use_decl, ctx.source);
            for (qualified_path, simple_name, path_node) in paths {
                // Determine if external (not starting with crate:: or self::)
                let is_external = !qualified_path.starts_with("crate::")
                    && !qualified_path.starts_with("self::")
                    && !qualified_path.starts_with("super::");

                if let Some(source_ref) = build_source_reference(
                    qualified_path,
                    simple_name,
                    is_external,
                    path_node,
                    ReferenceType::Import,
                ) {
                    imports.push(source_ref);
                }
            }
        }
    }

    imports
}

/// Extract Rust re-exports (pub use declarations)
fn extract_rust_reexports(
    node: Node,
    ctx: &SpecDrivenContext,
    _parent_scope: Option<&str>,
) -> Vec<SourceReference> {
    let Some(query) = get_rust_use_query() else {
        return Vec::new();
    };

    let mut reexports = Vec::new();
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(query, node, ctx.source.as_bytes());

    while let Some(query_match) = matches.next() {
        for capture in query_match.captures {
            let use_decl = capture.node;

            // Only process pub use
            if !has_pub_visibility(use_decl) {
                continue;
            }

            // Extract the re-exported paths
            let paths = extract_rust_use_paths(use_decl, ctx.source);
            for (qualified_path, simple_name, path_node) in paths {
                let is_external = !qualified_path.starts_with("crate::")
                    && !qualified_path.starts_with("self::")
                    && !qualified_path.starts_with("super::");

                if let Some(source_ref) = build_source_reference(
                    qualified_path,
                    simple_name,
                    is_external,
                    path_node,
                    ReferenceType::Reexport,
                ) {
                    reexports.push(source_ref);
                }
            }
        }
    }

    reexports
}

/// Check if a use declaration has pub visibility
fn has_pub_visibility(use_decl: Node) -> bool {
    let mut cursor = use_decl.walk();
    for child in use_decl.children(&mut cursor) {
        if child.kind() == "visibility_modifier" {
            return true;
        }
    }
    false
}

/// Extract paths from a Rust use declaration
/// Returns Vec<(qualified_path, simple_name, node)>
fn extract_rust_use_paths<'a>(use_decl: Node<'a>, source: &str) -> Vec<(String, String, Node<'a>)> {
    let mut paths = Vec::new();
    let mut cursor = use_decl.walk();

    for child in use_decl.children(&mut cursor) {
        match child.kind() {
            "use_as_clause" => {
                // use foo::bar as baz;
                if let Some((path, alias)) = extract_rust_use_as_clause(child, source) {
                    paths.push((path, alias, child));
                }
            }
            "scoped_use_list" => {
                // use foo::{bar, baz};
                extract_rust_scoped_use_list(child, source, "", &mut paths);
            }
            "scoped_identifier" | "identifier" => {
                // Simple use: use foo::bar;
                let path = node_text(child, source).to_string();
                let simple = extract_simple_name(&path).to_string();
                paths.push((path, simple, child));
            }
            "use_wildcard" => {
                // use foo::*;
                if let Some(scope) = child.child_by_field_name("path") {
                    let path = node_text(scope, source).to_string();
                    paths.push((format!("{path}::*"), "*".to_string(), child));
                }
            }
            _ => {}
        }
    }

    paths
}

/// Extract path and alias from a use_as_clause
fn extract_rust_use_as_clause(clause: Node, source: &str) -> Option<(String, String)> {
    let path = clause.child_by_field_name("path")?;
    let alias = clause.child_by_field_name("alias")?;
    Some((
        node_text(path, source).to_string(),
        node_text(alias, source).to_string(),
    ))
}

/// Extract paths from a scoped use list
fn extract_rust_scoped_use_list<'a>(
    list: Node<'a>,
    source: &str,
    prefix: &str,
    paths: &mut Vec<(String, String, Node<'a>)>,
) {
    // Get the path prefix
    let full_prefix = if let Some(path_node) = list.child_by_field_name("path") {
        let path_text = node_text(path_node, source);
        if prefix.is_empty() {
            path_text.to_string()
        } else {
            format!("{prefix}::{path_text}")
        }
    } else {
        prefix.to_string()
    };

    // Find the use_list child
    let mut cursor = list.walk();
    for child in list.children(&mut cursor) {
        if child.kind() == "use_list" {
            extract_rust_use_list_items(child, source, &full_prefix, paths);
        }
    }
}

/// Extract items from a use_list
fn extract_rust_use_list_items<'a>(
    list: Node<'a>,
    source: &str,
    prefix: &str,
    paths: &mut Vec<(String, String, Node<'a>)>,
) {
    let mut cursor = list.walk();
    for child in list.children(&mut cursor) {
        match child.kind() {
            "identifier" | "scoped_identifier" => {
                let name = node_text(child, source);
                let full_path = if prefix.is_empty() {
                    name.to_string()
                } else {
                    format!("{prefix}::{name}")
                };
                let simple = extract_simple_name(name).to_string();
                paths.push((full_path, simple, child));
            }
            "use_as_clause" => {
                if let Some(path_node) = child.child_by_field_name("path") {
                    if let Some(alias_node) = child.child_by_field_name("alias") {
                        let path = node_text(path_node, source);
                        let alias = node_text(alias_node, source);
                        let full_path = if prefix.is_empty() {
                            path.to_string()
                        } else {
                            format!("{prefix}::{path}")
                        };
                        paths.push((full_path, alias.to_string(), child));
                    }
                }
            }
            "scoped_use_list" => {
                extract_rust_scoped_use_list(child, source, prefix, paths);
            }
            "use_wildcard" | "self" => {
                // Glob import or self
                let full_path = if prefix.is_empty() {
                    "*".to_string()
                } else {
                    format!("{prefix}::*")
                };
                paths.push((full_path, "*".to_string(), child));
            }
            _ => {}
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
            let imports = extract_module_imports(node, ctx, parent_scope);
            let reexports = extract_module_reexports(node, ctx, parent_scope);
            EntityRelationshipData {
                imports,
                reexports,
                ..Default::default()
            }
        }
        RelationshipExtractor::ExtractImplRelationships => {
            let implements = extract_impl_trait_reference(node, ctx, parent_scope);
            EntityRelationshipData {
                implements,
                ..Default::default()
            }
        }
    }
}
