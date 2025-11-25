//! Handler for extracting Rust module definitions
//!
//! This module processes tree-sitter query matches for Rust module
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
use tree_sitter::{Node, Query, QueryMatch};

/// Extract use statements from a module node
fn extract_use_statements(node: Node, source: &str) -> Vec<String> {
    let mut imports = Vec::new();
    let mut cursor = node.walk();

    for child in node.children(&mut cursor) {
        if child.kind() == "use_declaration" {
            if let Ok(import_text) = node_to_text(child, source) {
                // Parse "use std::collections::HashMap;" -> "std::collections::HashMap"
                let import_path = import_text
                    .trim_start_matches("use ")
                    .trim_end_matches(';')
                    .trim()
                    .to_string();
                imports.push(import_path);
            }
        }
    }

    imports
}

/// Process a module query match and extract entity data
pub fn handle_module_impl(
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

    // Extract imports from the module
    let imports = extract_use_statements(module_node, source);

    // Build metadata
    let mut metadata = EntityMetadata::default();

    // Store whether this is an inline or file module
    metadata.attributes.insert(
        "is_inline".to_string(),
        if has_body { "true" } else { "false" }.to_string(),
    );

    // Store imports if any exist
    if !imports.is_empty() {
        metadata
            .attributes
            .insert("imports".to_string(), imports.join(","));
    }

    // Build the entity using the common helper
    let entity = build_entity(components, EntityType::Module, metadata, None)?;

    Ok(vec![entity])
}
