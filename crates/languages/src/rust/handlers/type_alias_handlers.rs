//! Handlers for extracting Rust type alias definitions
//!
//! This module processes tree-sitter query matches for type alias definitions
//! and builds CodeEntity instances.

#![deny(warnings)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]

use crate::qualified_name::build_qualified_name_from_ast;
use crate::rust::handlers::common::{
    extract_generics_from_node, extract_preceding_doc_comments, extract_visibility,
    find_capture_node, node_to_text, require_capture_node,
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

/// Process a type alias query match and extract entity data
pub fn handle_type_alias(
    query_match: &QueryMatch,
    query: &Query,
    source: &str,
    file_path: &Path,
    repository_id: &str,
) -> Result<Vec<CodeEntity>> {
    // Extract name
    let name_node = require_capture_node(query_match, query, capture_names::NAME)?;
    let name = node_to_text(name_node, source)?;

    // Extract the main type_alias node
    let main_node = require_capture_node(query_match, query, "type_alias")?;

    // Extract aliased type
    let aliased_type_node = require_capture_node(query_match, query, capture_names::TYPE)?;
    let aliased_type = node_to_text(aliased_type_node, source)?;

    // Extract generics if present
    let generics = find_capture_node(query_match, query, capture_names::GENERICS)
        .map(|node| extract_generics_from_node(node, source))
        .unwrap_or_default();

    // Extract visibility
    let visibility = extract_visibility(query_match, query);

    // Extract documentation
    let documentation = extract_preceding_doc_comments(main_node, source);

    // Build qualified name via parent traversal
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
    let mut metadata = EntityMetadata {
        is_generic: !generics.is_empty(),
        generic_params: generics.clone(),
        ..Default::default()
    };

    // Store aliased type in attributes
    metadata
        .attributes
        .insert("aliased_type".to_string(), aliased_type);

    if !generics.is_empty() {
        metadata
            .attributes
            .insert("generic_params".to_string(), generics.join(","));
    }

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
        .entity_type(EntityType::TypeAlias)
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
