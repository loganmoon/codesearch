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
use crate::rust::entities::FieldInfo;
use crate::rust::handler_impls::common::{
    build_generic_bounds_map, extract_generics_with_bounds, extract_preceding_doc_comments,
    extract_visibility, extract_where_clause_bounds, find_capture_node, format_generic_param,
    get_file_import_map, merge_parsed_generics, node_to_text, require_capture_node, ParsedGenerics,
    RustResolutionContext,
};
use crate::rust::handler_impls::constants::node_kinds;
use codesearch_core::entities::{EntityMetadata, EntityType, Language, Visibility};
use codesearch_core::error::Result;
use codesearch_core::CodeEntity;
use std::path::Path;
use tree_sitter::{Node, Query, QueryMatch};

/// Process a union query match and extract entity data
#[allow(clippy::too_many_arguments)]
pub fn handle_union_impl(
    query_match: &QueryMatch,
    query: &Query,
    source: &str,
    file_path: &Path,
    repository_id: &str,
    package_name: Option<&str>,
    source_root: Option<&Path>,
    repo_root: &Path,
) -> Result<Vec<CodeEntity>> {
    // Get the union node
    let union_node = require_capture_node(query_match, query, "union")?;

    // Create extraction context
    let ctx = ExtractionContext {
        query_match,
        query,
        source,
        file_path,
        repository_id,
        package_name,
        source_root,
        repo_root,
    };

    // Build ImportMap from file's imports for type resolution
    let import_map = get_file_import_map(union_node, source);

    // Extract common components for parent_scope
    let components = extract_common_components(&ctx, "name", union_node, "rust")?;

    // Derive module path from file path for qualified name resolution
    let module_path =
        source_root.and_then(|root| crate::rust::module_path::derive_module_path(file_path, root));

    // Build resolution context for qualified name normalization
    let resolution_ctx = RustResolutionContext {
        import_map: &import_map,
        parent_scope: components.parent_scope.as_deref(),
        package_name,
        current_module: module_path.as_deref(),
    };

    // Extract Rust-specific: visibility, documentation, content
    let visibility = extract_visibility(query_match, query);
    let documentation = extract_preceding_doc_comments(union_node, source);
    let content = node_to_text(union_node, source).ok();

    // Extract generics with parsed bounds
    let parsed_generics = extract_generics_with_where(&ctx, &resolution_ctx);

    // Build backward-compatible generic_params
    let generics: Vec<String> = parsed_generics
        .params
        .iter()
        .map(format_generic_param)
        .collect();

    // Build generic_bounds map
    let generic_bounds = build_generic_bounds_map(&parsed_generics);

    // Extract fields
    let fields = find_capture_node(query_match, query, "fields")
        .map(|node| parse_named_fields(node, source))
        .unwrap_or_default();

    // Build metadata
    let mut metadata = EntityMetadata::default();
    metadata.generic_params = generics;
    metadata.generic_bounds = generic_bounds;
    metadata.is_generic = !metadata.generic_params.is_empty();

    // Store field info as JSON in attributes
    if !fields.is_empty() {
        if let Ok(json) = serde_json::to_string(&fields) {
            metadata.attributes.insert("fields".to_string(), json);
        }
    }

    // Build the entity using the shared helper
    let entity = build_entity(
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

    Ok(vec![entity])
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
