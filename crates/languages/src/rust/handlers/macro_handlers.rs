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

use crate::qualified_name::build_qualified_name_from_ast;
use crate::rust::handlers::common::{
    extract_preceding_doc_comments, extract_visibility, node_to_text, require_capture_node,
};
use crate::rust::handlers::constants::capture_names;
use codesearch_core::entities::{
    CodeEntityBuilder, EntityMetadata, EntityType, Language, SourceLocation,
};
use codesearch_core::entity_id::generate_entity_id;
use codesearch_core::error::{Error, Result};
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
    // Extract name
    let name_node = require_capture_node(query_match, query, capture_names::NAME)?;
    let name = node_to_text(name_node, source)?;

    // Extract the main macro node
    let main_node = require_capture_node(query_match, query, "macro")?;

    // Check for #[macro_export] attribute
    // Use the first capture node like extract_derives does
    let check_node = query_match
        .captures
        .first()
        .map(|c| c.node)
        .unwrap_or(main_node);
    let is_exported = check_macro_export(check_node, source);

    // Extract visibility (macros typically don't have visibility modifiers directly)
    let visibility = extract_visibility(query_match, query);

    // Extract documentation
    let documentation = extract_preceding_doc_comments(main_node, source);

    // Build qualified name via parent traversal (macros are typically top-level)
    let parent_scope = build_qualified_name_from_ast(main_node, source, "rust");
    let qualified_name = if parent_scope.is_empty() {
        name.clone()
    } else {
        format!("{parent_scope}::{name}")
    };

    // Generate entity_id
    let file_path_str = file_path
        .to_str()
        .ok_or_else(|| Error::entity_extraction("Invalid file path"))?;
    let entity_id = generate_entity_id(repository_id, file_path_str, &qualified_name);

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

    // Get location and content
    let location = SourceLocation::from_tree_sitter_node(main_node);
    let content = node_to_text(main_node, source).ok();

    // Build entity
    let entity = CodeEntityBuilder::default()
        .entity_id(entity_id)
        .repository_id(repository_id.to_string())
        .name(name)
        .qualified_name(qualified_name)
        .parent_scope(if parent_scope.is_empty() {
            None
        } else {
            Some(parent_scope)
        })
        .entity_type(EntityType::Macro)
        .location(location)
        .visibility(visibility)
        .documentation_summary(documentation)
        .content(content)
        .metadata(metadata)
        .language(Language::Rust)
        .file_path(file_path.to_path_buf())
        .build()
        .map_err(|e| Error::entity_extraction(format!("Failed to build CodeEntity: {e}")))?;

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
