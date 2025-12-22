//! Handlers for extracting Rust type alias definitions
//!
//! This module processes tree-sitter query matches for type alias definitions
//! and builds CodeEntity instances.

#![deny(warnings)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]

use crate::common::entity_building::{
    build_entity, extract_common_components, EntityDetails, ExtractionContext,
};
use crate::rust::handler_impls::common::{
    extract_generics_from_node, extract_preceding_doc_comments, extract_visibility,
    find_capture_node, node_to_text, require_capture_node,
};
use crate::rust::handler_impls::constants::capture_names;
use codesearch_core::entities::{EntityMetadata, EntityType, Language};
use codesearch_core::error::Result;
use codesearch_core::CodeEntity;
use std::path::Path;
use tree_sitter::{Query, QueryMatch};

/// Process a type alias query match and extract entity data
#[allow(clippy::too_many_arguments)]
pub fn handle_type_alias_impl(
    query_match: &QueryMatch,
    query: &Query,
    source: &str,
    file_path: &Path,
    repository_id: &str,
    package_name: Option<&str>,
    source_root: Option<&Path>,
    repo_root: &Path,
) -> Result<Vec<CodeEntity>> {
    // Extract the main type_alias node
    let main_node = require_capture_node(query_match, query, "type_alias")?;

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

    // Extract Rust-specific: visibility, documentation, content
    let visibility = extract_visibility(query_match, query);
    let documentation = extract_preceding_doc_comments(main_node, source);
    let content = node_to_text(main_node, source).ok();

    // Extract aliased type
    let aliased_type_node = require_capture_node(query_match, query, capture_names::TYPE)?;
    let aliased_type = node_to_text(aliased_type_node, source)?;

    // Extract generics if present
    let generics = find_capture_node(query_match, query, capture_names::GENERICS)
        .map(|node| extract_generics_from_node(node, source))
        .unwrap_or_default();

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

    // Build the entity using the shared helper
    let entity = build_entity(
        components,
        EntityDetails {
            entity_type: EntityType::TypeAlias,
            language: Language::Rust,
            visibility,
            documentation,
            content,
            metadata,
            signature: None,
        },
    )?;

    Ok(vec![entity])
}
