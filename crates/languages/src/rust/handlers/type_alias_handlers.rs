//! Handlers for extracting Rust type alias definitions
//!
//! This module processes tree-sitter query matches for type alias definitions
//! and builds CodeEntity instances.

#![deny(warnings)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]

use crate::rust::handlers::common::{
    build_entity, extract_common_components, extract_generics_from_node, find_capture_node,
    node_to_text, require_capture_node,
};
use crate::rust::handlers::constants::capture_names;
use codesearch_core::entities::{EntityMetadata, EntityType};
use codesearch_core::error::Result;
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
    // Extract the main type_alias node
    let main_node = require_capture_node(query_match, query, "type_alias")?;

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

    // Build the entity using the common helper
    let entity = build_entity(components, EntityType::TypeAlias, metadata, None)?;

    Ok(vec![entity])
}
