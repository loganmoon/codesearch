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

use crate::rust::handlers::common::{
    build_entity, extract_common_components, node_to_text, require_capture_node,
};
use crate::rust::handlers::constants::capture_names;
use codesearch_core::entities::{EntityMetadata, EntityType};
use codesearch_core::error::Result;
use codesearch_core::CodeEntity;
use std::path::Path;
use tree_sitter::{Query, QueryMatch};

/// Process a macro definition query match and extract entity data
pub fn handle_macro(
    query_match: &QueryMatch,
    query: &Query,
    source: &str,
    file_path: &Path,
    repository_id: &str,
) -> Result<Vec<CodeEntity>> {
    // Extract the main macro node
    let main_node = require_capture_node(query_match, query, "macro")?;

    // Extract common components
    let components = extract_common_components(
        query_match,
        query,
        source,
        file_path,
        repository_id,
        capture_names::NAME,
        main_node,
    )?;

    // Check for #[macro_export] attribute
    // Use the first capture node like extract_derives does
    let check_node = query_match
        .captures
        .first()
        .map(|c| c.node)
        .unwrap_or(main_node);
    let is_exported = check_macro_export(check_node, source);

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

    // Build the entity using the common helper
    let entity = build_entity(components, EntityType::Macro, metadata, None)?;

    Ok(vec![entity])
}

/// Check if a macro has the #[macro_export] attribute
fn check_macro_export(node: tree_sitter::Node, source: &str) -> bool {
    // First try walking backwards through siblings
    let mut current = node.prev_sibling();
    while let Some(sibling) = current {
        if sibling.kind() == "attribute_item" {
            if let Ok(text) = node_to_text(sibling, source) {
                if text.contains("macro_export") {
                    return true;
                }
            }
        }
        current = sibling.prev_sibling();
    }

    // If that didn't work, try checking the parent's children
    if let Some(parent) = node.parent() {
        let mut cursor = parent.walk();
        for child in parent.children(&mut cursor) {
            // Stop when we reach this node
            if child.id() == node.id() {
                break;
            }
            // Check if this child is an attribute_item with macro_export
            if child.kind() == "attribute_item" {
                if let Ok(text) = node_to_text(child, source) {
                    if text.contains("macro_export") {
                        return true;
                    }
                }
            }
        }
    }

    false
}
