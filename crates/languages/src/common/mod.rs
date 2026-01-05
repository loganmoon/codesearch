//! Language-agnostic utilities for entity extraction
//!
//! These utilities work across all languages using tree-sitter.

pub(crate) mod edge_case_handlers;
pub mod entity_building;
pub mod import_map;
pub mod language_path;
pub mod module_utils;
pub mod path_config;
pub(crate) mod reference_resolution;

use codesearch_core::error::{Error, Result};
use tree_sitter::{Node, Query, QueryMatch};

/// Find a capture node by name in a query match
pub fn find_capture_node<'a>(
    query_match: &'a QueryMatch,
    query: &'a Query,
    name: &str,
) -> Option<Node<'a>> {
    query_match.captures.iter().find_map(|capture| {
        let capture_name = query.capture_names().get(capture.index as usize)?;
        if *capture_name == name {
            Some(capture.node)
        } else {
            None
        }
    })
}

/// Convert tree-sitter node to text
pub fn node_to_text(node: Node, source: &str) -> Result<String> {
    node.utf8_text(source.as_bytes())
        .map(|s| s.to_string())
        .map_err(|e| Error::entity_extraction(format!("Failed to convert node to text: {e}")))
}

/// Require a capture node by name (error if missing)
pub fn require_capture_node<'a>(
    query_match: &'a QueryMatch,
    query: &'a Query,
    name: &str,
) -> Result<Node<'a>> {
    find_capture_node(query_match, query, name)
        .ok_or_else(|| Error::entity_extraction(format!("Missing required capture: {name}")))
}
