//! Shared building blocks for Rust entity handlers
//!
//! This module provides reusable utilities for extracting Rust code entities:
//! - Qualified name builders for different entity types
//! - Metadata extraction helpers
//! - Visibility extraction
//! - Entity construction utilities

use crate::common::entity_building::{
    build_entity, compose_qualified_name, CommonEntityComponents, EntityDetails,
};
use crate::common::module_utils::derive_path_entity_identifier;
use crate::common::reference_resolution::{resolve_reference, ResolutionContext};
use crate::extract_context::ExtractContext;
use crate::qualified_name::{build_qualified_name_from_ast, derive_module_path_for_language};
use codesearch_core::entities::{
    EntityMetadata, EntityRelationshipData, EntityType, ReferenceType, SourceLocation,
    SourceReference, Visibility,
};
use codesearch_core::entity_id::generate_entity_id;
use codesearch_core::error::{Error, Result};
use codesearch_core::CodeEntity;
use std::collections::HashSet;
use std::sync::OnceLock;
use streaming_iterator::StreamingIterator;
use tree_sitter::{Node, Query, QueryCursor};

const RUST_SEPARATOR: &str = "::";

/// Build common entity components from ExtractContext
///
/// This is the primary entry point for building entity components from a handler.
/// It handles:
/// - Qualified name derivation from AST + module path + package name
/// - Parent scope calculation
/// - Entity ID generation
/// - Source location extraction
pub(crate) fn build_components_from_context(
    ctx: &ExtractContext,
    name: &str,
    entity_type: EntityType,
) -> Result<CommonEntityComponents> {
    // Build qualified name via parent traversal
    let scope_result = build_qualified_name_from_ast(ctx.node(), ctx.source(), "rust");
    let ast_scope = scope_result.parent_scope;

    // Derive module path from file path (if source_root is available)
    let module_prefix = ctx
        .source_root()
        .and_then(|root| derive_module_path_for_language(ctx.file_path(), root, "rust"));

    // Compose fully qualified name: package::module::ast_scope::name
    let qualified_name = compose_qualified_name(
        ctx.package_name(),
        module_prefix.as_deref(),
        &ast_scope,
        name,
        RUST_SEPARATOR,
    );

    // Calculate parent_scope (everything except the final name)
    let parent_scope = compose_qualified_name(
        ctx.package_name(),
        module_prefix.as_deref(),
        &ast_scope,
        "",
        RUST_SEPARATOR,
    );

    // Generate path_entity_identifier for import resolution
    let path_module =
        derive_path_entity_identifier(ctx.file_path(), ctx.repo_root(), RUST_SEPARATOR);
    let path_entity_identifier =
        compose_qualified_name(None, Some(&path_module), &ast_scope, name, RUST_SEPARATOR);

    // Generate entity ID
    let file_path_str = ctx
        .file_path()
        .to_str()
        .ok_or_else(|| Error::entity_extraction("Invalid file path"))?;
    let entity_id = generate_entity_id(
        ctx.repository_id(),
        file_path_str,
        &qualified_name,
        &entity_type.to_string(),
    );

    let location = SourceLocation::from_tree_sitter_node(ctx.node());

    Ok(CommonEntityComponents {
        entity_id,
        repository_id: ctx.repository_id().to_string(),
        name: name.to_string(),
        qualified_name,
        path_entity_identifier: Some(path_entity_identifier),
        parent_scope: if parent_scope.is_empty() {
            None
        } else {
            Some(parent_scope)
        },
        file_path: ctx.file_path().to_path_buf(),
        location,
    })
}

/// Build common components with a custom qualified name
///
/// Used for entities requiring special qualified name formats like trait impl methods.
pub(crate) fn build_components_with_custom_qn(
    ctx: &ExtractContext,
    name: &str,
    qualified_name: String,
    parent_scope: Option<String>,
    entity_type: EntityType,
) -> Result<CommonEntityComponents> {
    // Generate path_entity_identifier for import resolution
    let scope_result = build_qualified_name_from_ast(ctx.node(), ctx.source(), "rust");
    let ast_scope = scope_result.parent_scope;
    let path_module =
        derive_path_entity_identifier(ctx.file_path(), ctx.repo_root(), RUST_SEPARATOR);
    let path_entity_identifier =
        compose_qualified_name(None, Some(&path_module), &ast_scope, name, RUST_SEPARATOR);

    let file_path_str = ctx
        .file_path()
        .to_str()
        .ok_or_else(|| Error::entity_extraction("Invalid file path"))?;
    let entity_id = generate_entity_id(
        ctx.repository_id(),
        file_path_str,
        &qualified_name,
        &entity_type.to_string(),
    );

    let location = SourceLocation::from_tree_sitter_node(ctx.node());

    Ok(CommonEntityComponents {
        entity_id,
        repository_id: ctx.repository_id().to_string(),
        name: name.to_string(),
        qualified_name,
        path_entity_identifier: Some(path_entity_identifier),
        parent_scope,
        file_path: ctx.file_path().to_path_buf(),
        location,
    })
}

/// Build a standard entity from components and details
///
/// Convenience wrapper around `build_entity` from common/entity_building.
pub(crate) fn build_standard_entity(
    ctx: &ExtractContext,
    name: &str,
    entity_type: EntityType,
    metadata: EntityMetadata,
    relationships: EntityRelationshipData,
    visibility: Option<Visibility>,
    documentation: Option<String>,
) -> Result<CodeEntity> {
    let components = build_components_from_context(ctx, name, entity_type)?;
    let content = ctx.node_text().ok().map(String::from);

    build_entity(
        components,
        EntityDetails {
            entity_type,
            language: ctx.language(),
            visibility,
            documentation,
            content,
            metadata,
            signature: None,
            relationships,
        },
    )
}

/// Build an entity with a custom qualified name
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_entity_with_custom_qn(
    ctx: &ExtractContext,
    name: &str,
    qualified_name: String,
    parent_scope: Option<String>,
    entity_type: EntityType,
    metadata: EntityMetadata,
    relationships: EntityRelationshipData,
    visibility: Option<Visibility>,
    documentation: Option<String>,
) -> Result<CodeEntity> {
    let components =
        build_components_with_custom_qn(ctx, name, qualified_name, parent_scope, entity_type)?;
    let content = ctx.node_text().ok().map(String::from);

    build_entity(
        components,
        EntityDetails {
            entity_type,
            language: ctx.language(),
            visibility,
            documentation,
            content,
            metadata,
            signature: None,
            relationships,
        },
    )
}

// === Qualified Name Builders ===

/// Build qualified name for a free function
pub(crate) fn build_function_qn(ctx: &ExtractContext, name: &str) -> String {
    let scope_result = build_qualified_name_from_ast(ctx.node(), ctx.source(), "rust");
    let module_prefix = ctx
        .source_root()
        .and_then(|root| derive_module_path_for_language(ctx.file_path(), root, "rust"));
    compose_qualified_name(
        ctx.package_name(),
        module_prefix.as_deref(),
        &scope_result.parent_scope,
        name,
        RUST_SEPARATOR,
    )
}

/// Build qualified name for methods in inherent impl: Type::method_name
///
/// Note: Uses the same format as struct fields (Type::name) since entity_id
/// includes entity_type to ensure uniqueness for same-named fields and methods.
pub(crate) fn build_inherent_method_qn(
    ctx: &ExtractContext,
    name: &str,
    impl_type: &str,
) -> String {
    let module_prefix = ctx
        .source_root()
        .and_then(|root| derive_module_path_for_language(ctx.file_path(), root, "rust"));
    // Use Type::method format (same as struct fields)
    compose_qualified_name(
        ctx.package_name(),
        module_prefix.as_deref(),
        impl_type,
        name,
        RUST_SEPARATOR,
    )
}

/// Build qualified name for methods in trait impl: <Type as Trait>::method_name
pub(crate) fn build_trait_impl_method_qn(
    ctx: &ExtractContext,
    name: &str,
    impl_type: &str,
    trait_name: &str,
) -> String {
    let module_prefix = ctx
        .source_root()
        .and_then(|root| derive_module_path_for_language(ctx.file_path(), root, "rust"));
    let base = compose_qualified_name(
        ctx.package_name(),
        module_prefix.as_deref(),
        "",
        "",
        RUST_SEPARATOR,
    );
    let qualified_trait = resolve_type_name(ctx, trait_name);
    let qualified_type = resolve_type_name(ctx, impl_type);
    if base.is_empty() {
        format!("<{qualified_type} as {qualified_trait}>::{name}")
    } else {
        format!("{base}::<{qualified_type} as {qualified_trait}>::{name}")
    }
}

/// Build qualified name for inherent impl block: impl Type
pub(crate) fn build_inherent_impl_qn(ctx: &ExtractContext, impl_type: &str) -> String {
    let module_prefix = ctx
        .source_root()
        .and_then(|root| derive_module_path_for_language(ctx.file_path(), root, "rust"));
    let base = compose_qualified_name(
        ctx.package_name(),
        module_prefix.as_deref(),
        "",
        "",
        RUST_SEPARATOR,
    );
    if base.is_empty() {
        format!("impl {impl_type}")
    } else {
        format!("{base}::impl {impl_type}")
    }
}

/// Build qualified name for trait impl block: <Type as Trait>
pub(crate) fn build_trait_impl_qn(
    ctx: &ExtractContext,
    impl_type: &str,
    trait_name: &str,
) -> String {
    let module_prefix = ctx
        .source_root()
        .and_then(|root| derive_module_path_for_language(ctx.file_path(), root, "rust"));
    let base = compose_qualified_name(
        ctx.package_name(),
        module_prefix.as_deref(),
        "",
        "",
        RUST_SEPARATOR,
    );
    let qualified_trait = resolve_type_name(ctx, trait_name);
    let qualified_type = resolve_type_name(ctx, impl_type);
    if base.is_empty() {
        format!("<{qualified_type} as {qualified_trait}>")
    } else {
        format!("{base}::<{qualified_type} as {qualified_trait}>")
    }
}

/// Resolve a type name to fully qualified if possible using import map
fn resolve_type_name(ctx: &ExtractContext, type_name: &str) -> String {
    ctx.import_map()
        .resolve(type_name)
        .map(String::from)
        .unwrap_or_else(|| type_name.to_string())
}

// === Scope Derivation ===

/// Derive parent scope from a qualified name by stripping the last `::` component
///
/// Generic utility for methods, functions, etc. that need to find their containing scope.
pub(crate) fn derive_parent_scope(qualified_name: &str) -> Option<String> {
    if let Some(pos) = qualified_name.rfind("::") {
        let parent = &qualified_name[..pos];
        if parent.is_empty() {
            None
        } else {
            Some(parent.to_string())
        }
    } else {
        None
    }
}

/// Derive module scope from an impl block qualified name
///
/// For impl blocks, we want the containing module, not the impl block itself.
/// The qualified name format is: "module::path::impl Type" or "module::path::<Type as Trait>"
pub(crate) fn derive_impl_module_scope(qualified_name: &str) -> Option<String> {
    // Find where the impl signature starts
    if let Some(impl_pos) = qualified_name.rfind("::impl ") {
        let parent = &qualified_name[..impl_pos];
        if parent.is_empty() {
            None
        } else {
            Some(parent.to_string())
        }
    } else if let Some(angle_pos) = qualified_name.rfind("::<") {
        let parent = &qualified_name[..angle_pos];
        if parent.is_empty() {
            None
        } else {
            Some(parent.to_string())
        }
    } else {
        None
    }
}

// === Metadata Extraction ===

/// Extract function/method metadata from a function_item node
pub(crate) fn extract_function_metadata(ctx: &ExtractContext) -> EntityMetadata {
    // Check for async keyword by iterating children
    // (async is not a named field in tree-sitter-rust, it's a direct child node)
    let mut is_async = false;
    let mut cursor = ctx.node().walk();
    for child in ctx.node().children(&mut cursor) {
        if child.kind() == "async" {
            is_async = true;
            break;
        }
    }
    EntityMetadata {
        is_async,
        ..Default::default()
    }
}

/// Extract struct metadata
pub(crate) fn extract_struct_metadata(ctx: &ExtractContext) -> EntityMetadata {
    EntityMetadata {
        is_generic: ctx.has_child_field("type_parameters"),
        ..Default::default()
    }
}

/// Extract trait metadata
pub(crate) fn extract_trait_metadata(ctx: &ExtractContext) -> EntityMetadata {
    EntityMetadata {
        is_generic: ctx.has_child_field("type_parameters"),
        ..Default::default()
    }
}

// === Visibility Extraction ===

/// Extract visibility from the @visibility capture
///
/// Uses the tree-sitter query capture for visibility_modifier.
/// If no capture exists, defaults to Private.
pub(crate) fn extract_visibility(ctx: &ExtractContext) -> Option<Visibility> {
    ctx.capture_text_opt("visibility")
        .map(visibility_from_text)
        .or(Some(Visibility::Private))
}

/// Convert visibility modifier text to Visibility enum
fn visibility_from_text(text: &str) -> Visibility {
    match text.trim() {
        "pub" => Visibility::Public,
        s if s.starts_with("pub(crate)") => Visibility::Internal,
        s if s.starts_with("pub(super)") => Visibility::Internal,
        s if s.starts_with("pub(in") => Visibility::Internal,
        s if s.starts_with("pub(self)") => Visibility::Private,
        _ => Visibility::Private,
    }
}

/// Extract visibility for macro definitions
///
/// Macros use `#[macro_export]` attribute instead of visibility modifiers.
/// A macro with `#[macro_export]` is considered Public.
pub(crate) fn extract_macro_visibility(ctx: &ExtractContext) -> Option<Visibility> {
    extract_macro_visibility_from_node(ctx.node(), ctx.source())
}

/// Extract visibility from a macro definition node
///
/// Uses tree-sitter node structure to find the attribute identifier,
/// avoiding brittle string matching.
pub(crate) fn extract_macro_visibility_from_node(node: Node, source: &str) -> Option<Visibility> {
    // Check preceding siblings for macro_export attribute
    let mut current = node;
    while let Some(prev) = current.prev_sibling() {
        current = prev;
        if prev.kind() == "attribute_item" {
            if is_macro_export_attribute(prev, source) {
                return Some(Visibility::Public);
            }
        } else if prev.kind() != "line_comment" && prev.kind() != "block_comment" {
            // Stop if we hit a non-attribute, non-comment node
            break;
        }
    }
    // Default to private for macros without #[macro_export]
    Some(Visibility::Private)
}

/// Check if an attribute_item is `#[macro_export]`
fn is_macro_export_attribute(attr_item: Node, source: &str) -> bool {
    // Find the attribute child
    let mut cursor = attr_item.walk();
    for child in attr_item.named_children(&mut cursor) {
        if child.kind() == "attribute" {
            // Find the identifier child of the attribute
            let mut attr_cursor = child.walk();
            for attr_child in child.named_children(&mut attr_cursor) {
                if attr_child.kind() == "identifier" {
                    if let Ok(text) = attr_child.utf8_text(source.as_bytes()) {
                        return text == "macro_export";
                    }
                }
            }
        }
    }
    false
}

// === Documentation Extraction ===

/// Extract documentation comment from preceding siblings
pub(crate) fn extract_documentation(ctx: &ExtractContext) -> Option<String> {
    extract_doc_from_node(ctx.node(), ctx.source())
}

/// Extract documentation from a node by walking preceding siblings
///
/// Uses tree-sitter's `doc` field on comment nodes to identify doc comments,
/// avoiding brittle string prefix matching.
pub(crate) fn extract_doc_from_node(node: Node, source: &str) -> Option<String> {
    let mut docs = Vec::new();
    let mut current = node;

    // Look at preceding siblings
    while let Some(prev) = current.prev_sibling() {
        current = prev;
        let kind = prev.kind();

        match kind {
            "line_comment" | "block_comment" => {
                // Use tree-sitter's doc field to identify doc comments
                if let Some(doc_node) = prev.child_by_field_name("doc") {
                    if let Ok(content) = doc_node.utf8_text(source.as_bytes()) {
                        docs.push(content.trim().to_string());
                    }
                }
            }
            // Skip attribute nodes (like #[derive(...)])
            "attribute_item" | "inner_attribute_item" => continue,
            // Stop at non-comment nodes
            _ => break,
        }
    }

    if docs.is_empty() {
        None
    } else {
        docs.reverse();
        Some(docs.join("\n"))
    }
}

// === Relationship Extraction ===

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

/// Rust type reference query
const RUST_TYPE_QUERY: &str = r#"
[
  (type_identifier) @type_ref
  (scoped_type_identifier) @scoped_type_ref
]
"#;

/// Common Rust types to filter out from type references.
///
/// Includes both language primitives (i32, bool, etc.) and standard library types
/// (String, Option, Result, Vec, Box) that are ubiquitous enough to be noise.
const RUST_FILTERED_TYPES: &[&str] = &[
    "i8", "i16", "i32", "i64", "i128", "isize", "u8", "u16", "u32", "u64", "u128", "usize", "f32",
    "f64", "bool", "char", "str", "String", "Self", "()", "Option", "Result", "Vec", "Box",
];

/// Node kinds that represent child entities - type references inside these should
/// be attributed to the child entity, not the parent container.
const RUST_CHILD_ENTITY_KINDS: &[&str] = &[
    "field_declaration", // struct fields
    "enum_variant",      // enum variants
    "function_item",     // methods in impl blocks
    "const_item",        // associated consts
    "type_item",         // associated types
];

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
                        "Failed to compile tree-sitter query - call extraction disabled"
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
                        "Failed to compile tree-sitter query - type extraction disabled"
                    );
                    None
                }
            }
        })
        .as_ref()
}

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

/// Extract simple name from a potentially qualified name
fn extract_simple_name(name: &str) -> &str {
    name.rsplit("::").next().unwrap_or(name)
}

/// Build a ResolutionContext from ExtractContext
fn build_resolution_context<'a>(
    ctx: &'a ExtractContext<'a>,
    parent_scope: Option<&'a str>,
) -> ResolutionContext<'a> {
    ResolutionContext {
        import_map: ctx.import_map(),
        parent_scope,
        package_name: ctx.package_name(),
        current_module: parent_scope,
        path_config: ctx.path_config(),
        edge_case_handlers: ctx.edge_case_handlers(),
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

/// Check if a type node is inside a child entity declaration (relative to the parent).
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

/// Extract function call relationships from a function or method body
///
/// Returns an EntityRelationshipData with the `calls` field populated.
pub(crate) fn extract_function_relationships(
    ctx: &ExtractContext,
    parent_scope: Option<&str>,
) -> EntityRelationshipData {
    let Some(query) = get_rust_call_query() else {
        return EntityRelationshipData::default();
    };

    let mut calls = Vec::new();
    let mut seen = HashSet::new();
    let mut cursor = QueryCursor::new();

    let mut matches = cursor.matches(query, ctx.node(), ctx.source().as_bytes());

    while let Some(query_match) = matches.next() {
        for capture in query_match.captures {
            let callee_node = capture.node;
            let callee_text = node_text(callee_node, ctx.source());

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

    EntityRelationshipData {
        calls,
        ..Default::default()
    }
}

/// Extract type reference relationships from a type definition (struct, enum, etc.)
///
/// Returns an EntityRelationshipData with the `uses_types` field populated.
pub(crate) fn extract_type_relationships(
    ctx: &ExtractContext,
    parent_scope: Option<&str>,
) -> EntityRelationshipData {
    let Some(query) = get_rust_type_query() else {
        return EntityRelationshipData::default();
    };

    let mut type_refs = Vec::new();
    let mut seen = HashSet::new();
    let mut cursor = QueryCursor::new();

    let mut matches = cursor.matches(query, ctx.node(), ctx.source().as_bytes());

    while let Some(query_match) = matches.next() {
        for capture in query_match.captures {
            let type_node = capture.node;
            let type_text = node_text(type_node, ctx.source());

            if type_text.is_empty() {
                continue;
            }

            // Filter primitive types
            if RUST_FILTERED_TYPES.contains(&type_text) {
                continue;
            }

            // Skip type references inside child entity declarations
            if is_inside_child_entity(type_node, ctx.node(), RUST_CHILD_ENTITY_KINDS) {
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

    EntityRelationshipData {
        uses_types: type_refs,
        ..Default::default()
    }
}

/// Extract IMPLEMENTS relationship from a Rust trait impl block.
///
/// For `impl Trait for Type`, extracts a reference to the Trait.
pub(crate) fn extract_impl_relationships(
    ctx: &ExtractContext,
    parent_scope: Option<&str>,
) -> EntityRelationshipData {
    let mut implements = Vec::new();

    // Find the trait field in impl_item
    if let Some(trait_node) = ctx.node().child_by_field_name("trait") {
        let type_text = node_text(trait_node, ctx.source());
        if !type_text.is_empty() {
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
                implements.push(source_ref);
            }
        }
    }

    EntityRelationshipData {
        implements,
        ..Default::default()
    }
}

/// Extract trait bound (supertrait) relationships from a trait definition
///
/// For `trait Foo: Bar + Baz`, extracts references to Bar and Baz.
pub(crate) fn extract_trait_bounds_relationships(
    ctx: &ExtractContext,
    parent_scope: Option<&str>,
) -> EntityRelationshipData {
    let mut extends = Vec::new();

    // Find trait_bounds child
    let mut cursor = ctx.node().walk();
    for child in ctx.node().children(&mut cursor) {
        if child.kind() == "trait_bounds" {
            extract_bounds_recursive(child, ctx, parent_scope, &mut extends);
        }
    }

    EntityRelationshipData {
        extended_types: extends,
        ..Default::default()
    }
}

/// Process a type bound node (type_identifier or scoped_type_identifier) and add to refs.
fn process_bound_type(
    node: Node,
    ctx: &ExtractContext,
    parent_scope: Option<&str>,
    refs: &mut Vec<SourceReference>,
) {
    let type_text = node_text(node, ctx.source());
    if type_text.is_empty() || RUST_FILTERED_TYPES.contains(&type_text) {
        return;
    }
    let simple_name = extract_simple_name(type_text);
    let resolution_ctx = build_resolution_context(ctx, parent_scope);
    let resolved = resolve_reference(type_text, simple_name, &resolution_ctx);
    if let Some(source_ref) = build_source_reference(
        resolved.target,
        resolved.simple_name,
        resolved.is_external,
        node,
        ReferenceType::Extends,
    ) {
        refs.push(source_ref);
    }
}

fn extract_bounds_recursive(
    node: Node,
    ctx: &ExtractContext,
    parent_scope: Option<&str>,
    refs: &mut Vec<SourceReference>,
) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "type_identifier" | "scoped_type_identifier" => {
                process_bound_type(child, ctx, parent_scope, refs);
            }
            _ => {
                // Recurse into children
                extract_bounds_recursive(child, ctx, parent_scope, refs);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_visibility_from_text_public() {
        assert_eq!(visibility_from_text("pub"), Visibility::Public);
    }

    #[test]
    fn test_visibility_from_text_private() {
        // No visibility modifier text results in Private
        assert_eq!(visibility_from_text(""), Visibility::Private);
    }

    #[test]
    fn test_visibility_from_text_internal() {
        assert_eq!(visibility_from_text("pub(crate)"), Visibility::Internal);
        assert_eq!(visibility_from_text("pub(super)"), Visibility::Internal);
        assert_eq!(
            visibility_from_text("pub(in crate::module)"),
            Visibility::Internal
        );
    }

    #[test]
    fn test_visibility_from_text_pub_self() {
        assert_eq!(visibility_from_text("pub(self)"), Visibility::Private);
    }
}
