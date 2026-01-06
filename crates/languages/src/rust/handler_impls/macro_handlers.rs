//! Handlers for extracting Rust macro definitions
//!
//! This module processes tree-sitter query matches for declarative macro definitions
//! (macro_rules!) and builds CodeEntity instances.
//!
//! Note: Procedural macros are function items with attributes and are
//! already extracted by the function handler.

#![deny(warnings)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]

use crate::common::entity_building::{
    build_entity, extract_common_components, EntityDetails, ExtractionContext,
};
use crate::rust::handler_impls::common::{
    extract_preceding_doc_comments, node_to_text, require_capture_node,
};
use crate::rust::handler_impls::constants::capture_names;
use codesearch_core::entities::{EntityMetadata, EntityType, Language, Visibility};
use codesearch_core::error::Result;
use codesearch_core::CodeEntity;

/// Process a macro definition query match and extract entity data
///
/// Detects #[macro_export] by checking the immediate preceding sibling node.
pub(crate) fn handle_macro_impl(ctx: &ExtractionContext) -> Result<Vec<CodeEntity>> {
    // Extract the main macro node
    let main_node = require_capture_node(ctx.query_match, ctx.query, "macro")?;

    // Check if immediate preceding sibling is #[macro_export] attribute
    let is_exported = check_immediate_macro_export(main_node, ctx.source);

    // Extract common components
    let components = extract_common_components(ctx, capture_names::NAME, main_node, "rust")?;

    // Extract Rust-specific: documentation, content
    let documentation = extract_preceding_doc_comments(main_node, ctx.source);
    let content = node_to_text(main_node, ctx.source).ok();

    // Macros with #[macro_export] are effectively public
    let visibility = if is_exported {
        Visibility::Public
    } else {
        Visibility::Private
    };

    // Build metadata
    let mut metadata = EntityMetadata::default();

    // Store macro type (declarative for macro_rules!)
    metadata
        .attributes
        .insert("macro_type".to_string(), "declarative".to_string());

    // Store export status
    metadata
        .attributes
        .insert("exported".to_string(), is_exported.to_string());

    // Build the entity using the shared helper
    let entity = build_entity(
        components,
        EntityDetails {
            entity_type: EntityType::Macro,
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

/// Check if a macro has #[macro_export] as its immediate preceding sibling
fn check_immediate_macro_export(node: tree_sitter::Node, source: &str) -> bool {
    // Check only the immediately preceding sibling
    if let Some(sibling) = node.prev_sibling() {
        if sibling.kind() == "attribute_item" {
            if let Ok(text) = node_to_text(sibling, source) {
                return text.contains("macro_export");
            }
        }
    }
    false
}
