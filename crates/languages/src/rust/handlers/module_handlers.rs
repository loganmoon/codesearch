//! Handler for extracting Rust module definitions
//!
//! This module processes tree-sitter query matches for Rust module
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

/// Process a module query match and extract entity data
pub fn handle_module(
    query_match: &QueryMatch,
    query: &Query,
    source: &str,
    file_path: &Path,
    repository_id: &str,
) -> Result<Vec<CodeEntity>> {
    // Extract module name
    let name = find_capture_node(query_match, query, capture_names::NAME)
        .and_then(|node| node_to_text(node, source).ok())
        .unwrap_or_else(|| special_idents::ANONYMOUS.to_string());

    // Get the module node for location and content
    let module_node = require_capture_node(query_match, query, "module")?;

    // Check if this is an inline module (has body) or file module
    let has_body = find_capture_node(query_match, query, "mod_body").is_some();

    // Build qualified name via parent traversal
    let parent_scope = build_qualified_name_from_ast(module_node, source, "rust");
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
    let documentation = extract_preceding_doc_comments(module_node, source);

    // Get location and content
    let location = SourceLocation::from_tree_sitter_node(module_node);
    let content = node_to_text(module_node, source).ok();

    // Build metadata
    let mut metadata = EntityMetadata::default();

    // Store whether this is an inline or file module
    metadata.attributes.insert(
        "is_inline".to_string(),
        if has_body { "true" } else { "false" }.to_string(),
    );

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
        .entity_type(EntityType::Module)
        .location(location)
        .visibility(visibility)
        .documentation_summary(documentation)
        .content(content)
        .metadata(metadata)
        .language(Language::Rust)
        .file_path(file_path.to_path_buf())
        .build()
        .map_err(|e| Error::entity_extraction(format!("Failed to build module entity: {e}")))?;

    Ok(vec![entity])
}
