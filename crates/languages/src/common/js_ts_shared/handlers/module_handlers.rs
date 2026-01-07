//! Handler for extracting JavaScript/TypeScript module definitions
//!
//! Each JS/TS file is treated as its own module, establishing the containment
//! hierarchy for entities defined within the file.

#![deny(warnings)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]

use crate::common::entity_building::{
    build_entity, CommonEntityComponents, EntityDetails, ExtractionContext,
};
use crate::common::module_utils;
use crate::common::{node_to_text, require_capture_node};
use codesearch_core::entities::{EntityMetadata, EntityType, Language, SourceLocation, Visibility};
use codesearch_core::entity_id::generate_entity_id;
use codesearch_core::error::Result;
use codesearch_core::CodeEntity;

use super::relationships::extract_module_relationships;

/// Handle JavaScript module as a Module entity
pub(crate) fn handle_module_impl(ctx: &ExtractionContext) -> Result<Vec<CodeEntity>> {
    handle_module_for_language(ctx, Language::JavaScript)
}

/// Handle TypeScript module as a Module entity
pub(crate) fn handle_ts_module_impl(ctx: &ExtractionContext) -> Result<Vec<CodeEntity>> {
    handle_module_for_language(ctx, Language::TypeScript)
}

/// Handle TSX module as a Module entity
pub(crate) fn handle_tsx_module_impl(ctx: &ExtractionContext) -> Result<Vec<CodeEntity>> {
    // TSX is TypeScript + JSX, so we use Language::TypeScript
    handle_module_for_language(ctx, Language::TypeScript)
}

/// Common module handling for all JS/TS languages
fn handle_module_for_language(
    ctx: &ExtractionContext,
    language: Language,
) -> Result<Vec<CodeEntity>> {
    let module_node = require_capture_node(ctx.query_match, ctx.query, "program")?;

    // Extract module name from file path
    let name = module_utils::derive_module_name(ctx.file_path);

    // Build qualified name from file path
    let qualified_name =
        module_utils::derive_qualified_name(ctx.file_path, ctx.source_root, ctx.repo_root, ".");

    // Build path_entity_identifier (repo-relative path for import resolution)
    let path_entity_identifier =
        module_utils::derive_path_entity_identifier(ctx.file_path, ctx.repo_root, ".");

    // Generate entity ID
    let file_path_str = ctx.file_path.to_string_lossy();
    let entity_id = generate_entity_id(ctx.repository_id, &file_path_str, &qualified_name);

    // Get location
    let location = SourceLocation::from_tree_sitter_node(module_node);

    // Create components
    let components = CommonEntityComponents {
        entity_id,
        repository_id: ctx.repository_id.to_string(),
        name,
        qualified_name,
        path_entity_identifier: Some(path_entity_identifier),
        parent_scope: None,
        file_path: ctx.file_path.to_path_buf(),
        location,
    };

    // Extract relationships (imports and reexports)
    let relationships = extract_module_relationships(module_node, ctx.source);

    // Build the entity
    let entity = build_entity(
        components,
        EntityDetails {
            entity_type: EntityType::Module,
            language,
            visibility: Some(Visibility::Public),
            documentation: None,
            content: node_to_text(module_node, ctx.source).ok(),
            metadata: EntityMetadata::default(),
            signature: None,
            relationships,
        },
    )?;

    Ok(vec![entity])
}
