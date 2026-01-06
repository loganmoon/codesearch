//! Variable and constant entity handlers for JavaScript and TypeScript

use crate::common::entity_building::{
    build_entity, extract_common_components, EntityDetails, ExtractionContext,
};
use codesearch_core::entities::{EntityMetadata, EntityType, Language};
use codesearch_core::error::Result;
use codesearch_core::CodeEntity;

use super::super::visibility::extract_visibility;
use super::common::{extract_main_node, extract_preceding_doc_comments, node_to_text};

/// Handle const declaration extraction
///
/// Handles:
/// - `const foo = 1`
/// - `export const foo = 1`
///
/// Note: Function expressions and arrow functions are handled separately.
pub(crate) fn handle_const_impl(ctx: &ExtractionContext) -> Result<Vec<CodeEntity>> {
    let node = match extract_main_node(ctx.query_match, ctx.query, &["const"]) {
        Some(n) => n,
        None => return Ok(Vec::new()),
    };

    let components = extract_common_components(ctx, "name", node, "javascript")?;

    let visibility = extract_visibility(node, ctx.source);
    let documentation = extract_preceding_doc_comments(node, ctx.source);
    let content = node_to_text(node, ctx.source).ok();

    let metadata = EntityMetadata {
        is_const: true,
        ..Default::default()
    };

    let entity = build_entity(
        components,
        EntityDetails {
            entity_type: EntityType::Constant,
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

/// Handle let declaration extraction
///
/// Handles:
/// - `let foo = 1`
/// - `let foo`
/// - `export let foo = 1`
pub(crate) fn handle_let_impl(ctx: &ExtractionContext) -> Result<Vec<CodeEntity>> {
    let node = match extract_main_node(ctx.query_match, ctx.query, &["let"]) {
        Some(n) => n,
        None => return Ok(Vec::new()),
    };

    let components = extract_common_components(ctx, "name", node, "javascript")?;

    let visibility = extract_visibility(node, ctx.source);
    let documentation = extract_preceding_doc_comments(node, ctx.source);
    let content = node_to_text(node, ctx.source).ok();

    let metadata = EntityMetadata::default();

    let entity = build_entity(
        components,
        EntityDetails {
            entity_type: EntityType::Variable,
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

/// Handle var declaration extraction
///
/// Handles:
/// - `var foo = 1`
/// - `var foo`
/// - `export var foo = 1`
pub(crate) fn handle_var_impl(ctx: &ExtractionContext) -> Result<Vec<CodeEntity>> {
    let node = match extract_main_node(ctx.query_match, ctx.query, &["var"]) {
        Some(n) => n,
        None => return Ok(Vec::new()),
    };

    let components = extract_common_components(ctx, "name", node, "javascript")?;

    let visibility = extract_visibility(node, ctx.source);
    let documentation = extract_preceding_doc_comments(node, ctx.source);
    let content = node_to_text(node, ctx.source).ok();

    let metadata = EntityMetadata::default();

    let entity = build_entity(
        components,
        EntityDetails {
            entity_type: EntityType::Variable,
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
