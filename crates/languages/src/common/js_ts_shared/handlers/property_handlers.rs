//! Property entity handlers for JavaScript and TypeScript

use crate::common::entity_building::{
    build_entity, extract_common_components, EntityDetails, ExtractionContext,
};
use codesearch_core::entities::{EntityMetadata, EntityType, Language};
use codesearch_core::error::Result;
use codesearch_core::CodeEntity;
use im::HashMap as ImHashMap;

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
pub(crate) fn handle_property_impl(ctx: &ExtractionContext) -> Result<Vec<CodeEntity>> {
    let node = match extract_main_node(ctx.query_match, ctx.query, &["property"]) {
        Some(n) => n,
        None => return Ok(Vec::new()),
    };

    let components = extract_common_components(ctx, "name", node, "javascript")?;

    let visibility = extract_visibility(node, ctx.source);
    let is_static = is_static_member(node);
    let documentation = extract_preceding_doc_comments(node, ctx.source);
    let content = node_to_text(node, ctx.source).ok();

    // Check if it's a private field (name starts with #)
    let name = extract_name(ctx.query_match, ctx.query, ctx.source);
    let is_private = name.is_some_and(|n| n.starts_with('#'));

    // Check if there's an initializer
    let has_initializer = ctx
        .query
        .capture_index_for_name("value")
        .is_some_and(|idx| ctx.query_match.captures.iter().any(|c| c.index == idx));

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
