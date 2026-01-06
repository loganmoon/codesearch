//! Handler for extracting Rust static items
//!
//! This module processes tree-sitter query matches for Rust static
//! declarations and builds EntityData instances.

#![deny(warnings)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]

use crate::common::entity_building::{
    build_entity, extract_common_components, EntityDetails, ExtractionContext,
};
use crate::rust::handler_impls::common::{
    extract_preceding_doc_comments, extract_visibility, find_capture_node, node_to_text,
    require_capture_node,
};
use codesearch_core::entities::{EntityMetadata, EntityType, Language};
use codesearch_core::error::Result;
use codesearch_core::CodeEntity;

/// Process a static query match and extract entity data
pub(crate) fn handle_static_impl(ctx: &ExtractionContext) -> Result<Vec<CodeEntity>> {
    // Get the static node
    let static_node = require_capture_node(ctx.query_match, ctx.query, "static")?;

    // Skip statics inside impl blocks - those are handled by the impl extractor
    if let Some(parent) = static_node.parent() {
        if parent.kind() == "declaration_list" {
            // Check if the declaration_list is inside an impl_item
            if let Some(grandparent) = parent.parent() {
                if grandparent.kind() == "impl_item" {
                    return Ok(Vec::new());
                }
            }
        }
    }

    // Extract common components
    let components = extract_common_components(ctx, "name", static_node, "rust")?;

    // Extract Rust-specific: visibility, documentation, content
    let visibility = extract_visibility(ctx.query_match, ctx.query);
    let documentation = extract_preceding_doc_comments(static_node, ctx.source);
    let content = node_to_text(static_node, ctx.source).ok();

    // Check for mutable_specifier (static mut)
    let is_mut = find_capture_node(ctx.query_match, ctx.query, "mut").is_some();

    // Extract type
    let static_type = find_capture_node(ctx.query_match, ctx.query, "type")
        .and_then(|node| node_to_text(node, ctx.source).ok());

    // Extract value
    let value = find_capture_node(ctx.query_match, ctx.query, "value")
        .and_then(|node| node_to_text(node, ctx.source).ok());

    // Build metadata
    let mut metadata = EntityMetadata {
        is_static: true,
        ..Default::default()
    };

    // Add type if present
    if let Some(ty) = static_type {
        metadata.attributes.insert("type".to_string(), ty);
    }

    // Add value if present
    if let Some(val) = value {
        metadata.attributes.insert("value".to_string(), val);
    }

    // Add mutable flag for static mut
    if is_mut {
        metadata
            .attributes
            .insert("mutable".to_string(), "true".to_string());
    }

    // Build the entity using the shared helper
    let entity = build_entity(
        components,
        EntityDetails {
            entity_type: EntityType::Static,
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
