//! Property entity handlers for JavaScript and TypeScript

use crate::common::entity_building::{
    build_entity, extract_common_components, EntityDetails, ExtractionContext,
};
use codesearch_core::entities::{EntityMetadata, EntityType, Language};
use codesearch_core::error::Result;
use codesearch_core::CodeEntity;
use im::HashMap as ImHashMap;
use std::path::Path;
use tree_sitter::{Query, QueryMatch};

use super::super::visibility::{extract_visibility, is_static_member};
use super::common::{
    extract_main_node, extract_name, extract_preceding_doc_comments, node_to_text,
};

/// Handle class property/field extraction
///
/// Handles:
/// - `field = value`
/// - `static field = value`
/// - `#privateField = value`
/// - `field` (no initializer)
#[allow(clippy::too_many_arguments)]
pub fn handle_property_impl(
    query_match: &QueryMatch,
    query: &Query,
    source: &str,
    file_path: &Path,
    repository_id: &str,
    package_name: Option<&str>,
    source_root: Option<&Path>,
    repo_root: &Path,
) -> Result<Vec<CodeEntity>> {
    let node = match extract_main_node(query_match, query, &["property"]) {
        Some(n) => n,
        None => return Ok(Vec::new()),
    };

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
    let components = extract_common_components(&ctx, "name", node, "javascript")?;

    // Extract JS-specific details
    let visibility = extract_visibility(node, source);
    let is_static = is_static_member(node);
    let documentation = extract_preceding_doc_comments(node, source);
    let content = node_to_text(node, source).ok();

    // Check if it's a private field (name starts with #)
    let name = extract_name(query_match, query, source);
    let is_private = name.map(|n| n.starts_with('#')).unwrap_or(false);

    // Check if there's an initializer
    let has_initializer = query
        .capture_index_for_name("value")
        .map(|idx| query_match.captures.iter().any(|c| c.index == idx))
        .unwrap_or(false);

    // Store JS-specific flags in attributes
    let mut attributes = ImHashMap::new();
    if is_private {
        attributes.insert("is_private".to_string(), "true".to_string());
    }
    if has_initializer {
        attributes.insert("has_initializer".to_string(), "true".to_string());
    }

    let metadata = EntityMetadata {
        is_static,
        attributes,
        ..Default::default()
    };

    let entity = build_entity(
        components,
        EntityDetails {
            entity_type: EntityType::Property,
            language: Language::JavaScript,
            visibility: Some(visibility),
            documentation,
            content,
            metadata,
            signature: None,
            relationships: Default::default(),
        },
    )?;

    Ok(vec![entity])
}
