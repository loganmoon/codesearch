//! Handler for extracting Rust constant and static items
//!
//! This module processes tree-sitter query matches for Rust const and static
//! declarations and builds EntityData instances.

#![deny(warnings)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]

use crate::rust::handler_impls::common::{
    build_entity, extract_common_components, find_capture_node, node_to_text, require_capture_node,
};
use crate::rust::handler_impls::constants::capture_names;
use codesearch_core::entities::{EntityMetadata, EntityType};
use codesearch_core::error::Result;
use codesearch_core::CodeEntity;
use std::path::Path;
use tree_sitter::{Query, QueryMatch};

/// Process a constant or static query match and extract entity data
pub fn handle_constant_impl(
    query_match: &QueryMatch,
    query: &Query,
    source: &str,
    file_path: &Path,
    repository_id: &str,
) -> Result<Vec<CodeEntity>> {
    // Get the constant node
    let constant_node = require_capture_node(query_match, query, "constant")?;

    // Skip constants inside impl blocks - those are handled by the impl extractor
    if let Some(parent) = constant_node.parent() {
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
    let components = extract_common_components(
        query_match,
        query,
        source,
        file_path,
        repository_id,
        capture_names::NAME,
        constant_node,
    )?;

    // Determine if this is const or static
    let is_const = find_capture_node(query_match, query, "const").is_some();
    let is_static = find_capture_node(query_match, query, "static").is_some();

    // Check for mutable_specifier (static mut)
    let is_mut = find_capture_node(query_match, query, "mut").is_some();

    // Extract type
    let const_type = find_capture_node(query_match, query, "type")
        .and_then(|node| node_to_text(node, source).ok());

    // Extract value
    let value = find_capture_node(query_match, query, "value")
        .and_then(|node| node_to_text(node, source).ok());

    // Build metadata
    let mut metadata = EntityMetadata {
        is_const,
        is_static,
        ..Default::default()
    };

    // Add type if present
    if let Some(ty) = const_type {
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

    // Build the entity using the common helper
    let entity = build_entity(components, EntityType::Constant, metadata, None)?;

    Ok(vec![entity])
}
