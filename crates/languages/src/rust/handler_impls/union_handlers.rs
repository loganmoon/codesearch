//! Handler for extracting Rust union definitions
//!
//! This module processes tree-sitter query matches for Rust union
//! declarations and builds EntityData instances.

#![deny(warnings)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]

use crate::common::entity_building::{
    build_entity, extract_common_components, EntityDetails, ExtractionContext,
};
use crate::common::path_config::RUST_PATH_CONFIG;
use crate::rust::entities::FieldInfo;
use crate::rust::handler_impls::common::{
    build_generic_bounds_map, extract_generics_with_bounds, extract_preceding_doc_comments,
    extract_visibility, extract_where_clause_bounds, find_capture_node, format_generic_param,
    get_file_import_map, get_rust_edge_case_registry, merge_parsed_generics, node_to_text,
    require_capture_node, ParsedGenerics, RustResolutionContext,
};
use crate::rust::handler_impls::constants::node_kinds;
use crate::rust::handler_impls::type_handlers::build_field_entities;
use codesearch_core::entities::{EntityMetadata, EntityType, Language, Visibility};
use codesearch_core::error::Result;
use codesearch_core::CodeEntity;
use tree_sitter::Node;

/// Process a union query match and extract entity data
pub(crate) fn handle_union_impl(ctx: &ExtractionContext) -> Result<Vec<CodeEntity>> {
    // Get the union node
    let union_node = require_capture_node(ctx.query_match, ctx.query, "union")?;

    // Build ImportMap from file's imports for type resolution
    let import_map = get_file_import_map(union_node, ctx.source);

    // Extract common components for parent_scope and qualified_name
    let components = extract_common_components(ctx, "name", union_node, "rust")?;
    let union_qualified_name = components.qualified_name.clone();
    let union_location = components.location.clone();

    // Derive module path from file path for qualified name resolution
    let module_path = ctx
        .source_root
        .and_then(|root| crate::rust::module_path::derive_module_path(ctx.file_path, root));

    // Build resolution context for qualified name normalization
    // Clone parent_scope to avoid borrow conflict with components consumed later
    let parent_scope_clone = components.parent_scope.clone();
    let edge_case_registry = get_rust_edge_case_registry();
    let resolution_ctx = RustResolutionContext {
        import_map: &import_map,
        parent_scope: parent_scope_clone.as_deref(),
        package_name: ctx.package_name,
        current_module: module_path.as_deref(),
        path_config: &RUST_PATH_CONFIG,
        edge_case_handlers: Some(&edge_case_registry),
    };

    // Extract Rust-specific: visibility, documentation, content
    let visibility = extract_visibility(ctx.query_match, ctx.query);
    let documentation = extract_preceding_doc_comments(union_node, ctx.source);
    let content = node_to_text(union_node, ctx.source).ok();

    // Extract generics with parsed bounds
    let parsed_generics = extract_generics_with_where(ctx, &resolution_ctx);

    // Build backward-compatible generic_params
    let generics: Vec<String> = parsed_generics
        .params
        .iter()
        .map(format_generic_param)
        .collect();

    // Build generic_bounds map
    let generic_bounds = build_generic_bounds_map(&parsed_generics);

    // Extract fields
    let fields = find_capture_node(ctx.query_match, ctx.query, "fields")
        .map(|node| parse_named_fields(node, ctx.source))
        .unwrap_or_default();

    // Build metadata (no longer stores fields as JSON)
    let mut metadata = EntityMetadata::default();
    metadata.generic_params = generics;
    metadata.generic_bounds = generic_bounds;
    metadata.is_generic = !metadata.generic_params.is_empty();

    // Build the union entity using the shared helper
    let union_entity = build_entity(
        components,
        EntityDetails {
            entity_type: EntityType::Union,
            language: Language::Rust,
            visibility: Some(visibility),
            documentation,
            content,
            metadata,
            signature: None,
            relationships: Default::default(),
        },
    )?;

    // Build field entities as children of the union
    let field_entities = build_field_entities(
        &fields,
        &union_qualified_name,
        ctx.file_path,
        ctx.repository_id,
        &resolution_ctx,
        &union_location,
    );

    // Return union followed by its fields
    let mut entities = vec![union_entity];
    entities.extend(field_entities);
    Ok(entities)
}

/// Extract generic parameters with parsed bounds
fn extract_generics_with_where(
    ctx: &ExtractionContext,
    resolution_ctx: &RustResolutionContext,
) -> ParsedGenerics {
    // Extract inline generics
    let mut parsed_generics = find_capture_node(ctx.query_match, ctx.query, "generics")
        .map(|node| extract_generics_with_bounds(node, ctx.source, resolution_ctx))
        .unwrap_or_default();

    // Merge where clause bounds if present
    if let Some(where_node) = find_capture_node(ctx.query_match, ctx.query, "where") {
        let where_bounds = extract_where_clause_bounds(where_node, ctx.source, resolution_ctx);
        merge_parsed_generics(&mut parsed_generics, where_bounds);
    }

    parsed_generics
}

/// Parse named fields from a union
fn parse_named_fields(node: Node, source: &str) -> Vec<FieldInfo> {
    let mut cursor = node.walk();

    node.children(&mut cursor)
        .filter(|child| child.kind() == node_kinds::FIELD_DECLARATION)
        .filter_map(|child| {
            // Get the full field text and parse it
            node_to_text(child, source).ok().and_then(|text| {
                // Check for visibility - distinguish pub, pub(crate), pub(super), etc.
                let trimmed = text.trim_start();
                let visibility = if trimmed.starts_with("pub(") {
                    // pub(crate), pub(super), pub(in path) -> Internal
                    Visibility::Internal
                } else if trimmed.starts_with("pub") {
                    // Just pub -> Public
                    Visibility::Public
                } else {
                    Visibility::Private
                };

                // Find field name and type separated by colon
                if let Some((name_part, type_part)) = text.split_once(':') {
                    // Extract the field name by taking the last word before the colon
                    let field_name = name_part
                        .split_whitespace()
                        .last()
                        .unwrap_or(name_part.trim())
                        .to_string();
                    let type_part = type_part.trim().trim_end_matches(',');

                    Some(FieldInfo {
                        name: field_name,
                        field_type: type_part.to_string(),
                        visibility,
                        attributes: Vec::new(),
                    })
                } else {
                    None
                }
            })
        })
        .collect()
}
