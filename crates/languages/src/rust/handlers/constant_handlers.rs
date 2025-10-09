//! Handler for extracting Rust constant and static items
//!
//! This module processes tree-sitter query matches for Rust const and static
//! declarations and builds EntityData instances.

#![deny(warnings)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]

use crate::qualified_name::build_qualified_name_from_ast;
use crate::rust::handlers::common::{
    extract_preceding_doc_comments, extract_visibility, find_capture_node, node_to_text,
    require_capture_node,
};
use crate::rust::handlers::constants::{capture_names, special_idents};
use codesearch_core::entities::{
    CodeEntityBuilder, EntityMetadata, EntityType, Language, SourceLocation,
};
use codesearch_core::entity_id::generate_entity_id;
use codesearch_core::error::{Error, Result};
use codesearch_core::CodeEntity;
use std::path::Path;
use tree_sitter::{Query, QueryMatch};

/// Process a constant or static query match and extract entity data
pub fn handle_constant(
    query_match: &QueryMatch,
    query: &Query,
    source: &str,
    file_path: &Path,
    repository_id: &str,
) -> Result<Vec<CodeEntity>> {
    // Extract name
    let name = find_capture_node(query_match, query, capture_names::NAME)
        .and_then(|node| node_to_text(node, source).ok())
        .unwrap_or_else(|| special_idents::ANONYMOUS.to_string());

    // Get the constant node
    let constant_node = require_capture_node(query_match, query, "constant")?;

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

    // Build qualified name via parent traversal
    let parent_scope = build_qualified_name_from_ast(constant_node, source, "rust");
    let qualified_name = if parent_scope.is_empty() {
        name.clone()
    } else {
        format!("{parent_scope}::{name}")
    };

    // Generate entity_id from repository + file_path + qualified name
    let file_path_str = file_path
        .to_str()
        .ok_or_else(|| Error::entity_extraction("Invalid file path"))?;
    let entity_id = generate_entity_id(repository_id, file_path_str, &qualified_name);

    // Extract visibility
    let visibility = extract_visibility(query_match, query);

    // Extract documentation
    let documentation = extract_preceding_doc_comments(constant_node, source);

    // Get location and content
    let location = SourceLocation::from_tree_sitter_node(constant_node);
    let content = node_to_text(constant_node, source).ok();

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

    // Build the entity
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
        .entity_type(EntityType::Constant)
        .location(location)
        .visibility(visibility)
        .documentation_summary(documentation)
        .content(content)
        .metadata(metadata)
        .language(Language::Rust)
        .file_path(file_path.to_path_buf())
        .build()
        .map_err(|e| Error::entity_extraction(format!("Failed to build constant entity: {e}")))?;

    Ok(vec![entity])
}
