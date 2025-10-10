//! Handler for extracting Rust module definitions
//!
//! This module processes tree-sitter query matches for Rust module
//! declarations and builds EntityData instances.

#![deny(warnings)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]

use crate::rust::handlers::common::{
    build_entity, extract_common_components, find_capture_node, require_capture_node,
};
use crate::rust::handlers::constants::capture_names;
use codesearch_core::entities::{EntityMetadata, EntityType};
use codesearch_core::error::Result;
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
    // Get the module node for location and content
    let module_node = require_capture_node(query_match, query, "module")?;

    // Extract common components
    let components = extract_common_components(
        query_match,
        query,
        source,
        file_path,
        repository_id,
        capture_names::NAME,
        module_node,
    )?;

    // Check if this is an inline module (has body) or file module
    let has_body = find_capture_node(query_match, query, "mod_body").is_some();

    // Build metadata
    let mut metadata = EntityMetadata::default();

    // Store whether this is an inline or file module
    metadata.attributes.insert(
        "is_inline".to_string(),
        if has_body { "true" } else { "false" }.to_string(),
    );

    // Build the entity using the common helper
    let entity = build_entity(components, EntityType::Module, metadata, None)?;

    Ok(vec![entity])
}
