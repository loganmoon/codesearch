//! TypeScript-specific entity handlers

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

/// Handle interface declaration extraction
///
/// Handles:
/// - `interface Foo {}`
/// - `interface Foo extends Bar {}`
/// - `export interface Foo {}`
pub(crate) fn handle_interface_impl(ctx: &ExtractionContext) -> Result<Vec<CodeEntity>> {
    let node = match extract_main_node(ctx.query_match, ctx.query, &["interface"]) {
        Some(n) => n,
        None => return Ok(Vec::new()),
    };

    let components = extract_common_components(ctx, "name", node, "typescript")?;

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
            }
        }
    }

    let metadata = EntityMetadata::default();

    let entity = build_entity(
        components,
        EntityDetails {
            entity_type: EntityType::Interface,
            language: Language::TypeScript,
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

/// Handle type alias declaration extraction
///
/// Handles:
/// - `type Foo = string`
/// - `type Foo<T> = T[]`
/// - `export type Foo = Bar`
pub(crate) fn handle_type_alias_impl(ctx: &ExtractionContext) -> Result<Vec<CodeEntity>> {
    let node = match extract_main_node(ctx.query_match, ctx.query, &["type_alias"]) {
        Some(n) => n,
        None => return Ok(Vec::new()),
    };

    let components = extract_common_components(ctx, "name", node, "typescript")?;

    let visibility = extract_visibility(node, ctx.source);
    let documentation = extract_preceding_doc_comments(node, ctx.source);
    let content = node_to_text(node, ctx.source).ok();

    let metadata = EntityMetadata::default();

    let entity = build_entity(
        components,
        EntityDetails {
            entity_type: EntityType::TypeAlias,
            language: Language::TypeScript,
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

/// Handle enum declaration extraction
///
/// Handles:
/// - `enum Color { Red, Green, Blue }`
/// - `const enum Direction { Up, Down }`
/// - `export enum Status { Active, Inactive }`
pub(crate) fn handle_enum_impl(ctx: &ExtractionContext) -> Result<Vec<CodeEntity>> {
    let node = match extract_main_node(ctx.query_match, ctx.query, &["enum"]) {
        Some(n) => n,
        None => return Ok(Vec::new()),
    };

    let components = extract_common_components(ctx, "name", node, "typescript")?;

    let visibility = extract_visibility(node, ctx.source);
    let documentation = extract_preceding_doc_comments(node, ctx.source);
    let content = node_to_text(node, ctx.source).ok();

    // Check if it's a const enum
    let is_const = node.child_by_field_name("const").is_some()
        || ctx.source[node.byte_range()]
            .trim_start()
            .starts_with("const");

    let metadata = EntityMetadata {
        is_const,
        ..Default::default()
    };

    let entity = build_entity(
        components,
        EntityDetails {
            entity_type: EntityType::Enum,
            language: Language::TypeScript,
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

/// Handle namespace declaration extraction
///
/// Handles:
/// - `namespace Foo {}`
/// - `module Bar {}`
/// - `export namespace Foo {}`
pub(crate) fn handle_namespace_impl(ctx: &ExtractionContext) -> Result<Vec<CodeEntity>> {
    let node = match extract_main_node(ctx.query_match, ctx.query, &["namespace"]) {
        Some(n) => n,
        None => return Ok(Vec::new()),
    };

    let components = extract_common_components(ctx, "name", node, "typescript")?;

    let visibility = extract_visibility(node, ctx.source);
    let documentation = extract_preceding_doc_comments(node, ctx.source);
    let content = node_to_text(node, ctx.source).ok();

    let metadata = EntityMetadata::default();

    let entity = build_entity(
        components,
        EntityDetails {
            entity_type: EntityType::Module,
            language: Language::TypeScript,
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
