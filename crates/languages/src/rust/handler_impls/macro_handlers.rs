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
use std::path::Path;
use tree_sitter::{Query, QueryMatch};

/// Process a macro definition query match and extract entity data
#[allow(clippy::too_many_arguments)]
pub fn handle_macro_impl(
    query_match: &QueryMatch,
    query: &Query,
    source: &str,
    file_path: &Path,
    repository_id: &str,
    package_name: Option<&str>,
    source_root: Option<&Path>,
    repo_root: &Path,
) -> Result<Vec<CodeEntity>> {
    // Extract the main macro node
    let main_node = require_capture_node(query_match, query, "macro")?;

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

    // Extract common components
    let components = extract_common_components(&ctx, capture_names::NAME, main_node, "rust")?;

    // Extract Rust-specific: documentation, content
    // Macros use #[macro_export] for visibility, not pub keyword
    let documentation = extract_preceding_doc_comments(main_node, source);
    let content = node_to_text(main_node, source).ok();

    // Check for #[macro_export] attribute
    // Use the first capture node like extract_derives does
    let check_node = query_match
        .captures
        .first()
        .map(|c| c.node)
        .unwrap_or(main_node);
    let is_exported = check_macro_export(check_node, source);

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
            visibility,
            documentation,
            content,
            metadata,
            signature: None,
            relationships: Default::default(),
        },
    )?;

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
