//! Class entity handlers for JavaScript and TypeScript

use crate::common::entity_building::{
    build_entity, extract_common_components, EntityDetails, ExtractionContext,
};
use codesearch_core::entities::{
    EntityMetadata, EntityRelationshipData, EntityType, Language, SourceReference,
};
use codesearch_core::error::Result;
use codesearch_core::CodeEntity;

use super::super::visibility::extract_visibility;
use super::common::{extract_main_node, extract_preceding_doc_comments, node_to_text};

/// Handle class declaration extraction
///
/// Handles:
/// - `class Foo {}`
/// - `class Foo extends Bar {}`
/// - `export class Foo {}`
pub(crate) fn handle_class_declaration_impl(ctx: &ExtractionContext) -> Result<Vec<CodeEntity>> {
    let node = match extract_main_node(ctx.query_match, ctx.query, &["class"]) {
        Some(n) => n,
        None => return Ok(Vec::new()),
    };

    let components = extract_common_components(ctx, "name", node, "javascript")?;

    let visibility = extract_visibility(node, ctx.source);
    let documentation = extract_preceding_doc_comments(node, ctx.source);
    let content = node_to_text(node, ctx.source).ok();

    // Extract extends clause if present
    let mut relationships = EntityRelationshipData::default();
    if let Some(extends_index) = ctx.query.capture_index_for_name("extends") {
        for capture in ctx.query_match.captures {
            if capture.index == extends_index {
                let extends_name = &ctx.source[capture.node.byte_range()];
                if let Ok(source_ref) = SourceReference::builder()
                    .target(extends_name.to_string())
                    .simple_name(extends_name.to_string())
                    .build()
                {
                    relationships.extends.push(source_ref);
                }
                break;
            }
        }
    }

    let metadata = EntityMetadata::default();

    let entity = build_entity(
        components,
        EntityDetails {
            entity_type: EntityType::Class,
            language: Language::JavaScript,
            visibility: Some(visibility),
            documentation,
            content,
            metadata,
            signature: None,
            relationships,
        },
    )?;

    Ok(vec![entity])
}

/// Handle class expression extraction
///
/// Handles:
/// - `const Foo = class {}`
/// - `const Foo = class Bar {}`
/// - `let Foo = class extends Base {}`
pub(crate) fn handle_class_expression_impl(ctx: &ExtractionContext) -> Result<Vec<CodeEntity>> {
    let node = match extract_main_node(ctx.query_match, ctx.query, &["class"]) {
        Some(n) => n,
        None => return Ok(Vec::new()),
    };

    // For class expressions, the name comes from the variable, not the class
    let components = extract_common_components(ctx, "name", node, "javascript")?;

    let visibility = extract_visibility(node, ctx.source);
    let documentation = extract_preceding_doc_comments(node, ctx.source);
    let content = node_to_text(node, ctx.source).ok();

    // Extract extends clause if present
    let mut relationships = EntityRelationshipData::default();
    if let Some(extends_index) = ctx.query.capture_index_for_name("extends") {
        for capture in ctx.query_match.captures {
            if capture.index == extends_index {
                let extends_name = &ctx.source[capture.node.byte_range()];
                if let Ok(source_ref) = SourceReference::builder()
                    .target(extends_name.to_string())
                    .simple_name(extends_name.to_string())
                    .build()
                {
                    relationships.extends.push(source_ref);
                }
                break;
            }
        }
    }

    let metadata = EntityMetadata::default();

    let entity = build_entity(
        components,
        EntityDetails {
            entity_type: EntityType::Class,
            language: Language::JavaScript,
            visibility: Some(visibility),
            documentation,
            content,
            metadata,
            signature: None,
            relationships,
        },
    )?;

    Ok(vec![entity])
}
