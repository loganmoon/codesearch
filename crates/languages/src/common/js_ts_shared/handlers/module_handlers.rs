//! Handler for extracting JavaScript/TypeScript module definitions
//!
//! Each JS/TS file is treated as its own module, establishing the containment
//! hierarchy for entities defined within the file.

use crate::common::{
    entity_building::{build_entity, CommonEntityComponents, EntityDetails, ExtractionContext},
    module_utils::{derive_module_name, derive_qualified_name},
    require_capture_node,
};
use codesearch_core::{
    entities::{EntityMetadata, EntityType, Language, SourceLocation, Visibility},
    entity_id::generate_entity_id,
    error::Result,
    CodeEntity,
};

/// Handle JavaScript/TypeScript program node as a Module entity
///
/// Creates a Module entity for the file to establish the containment hierarchy.
/// All top-level entities in the file will have this module as their parent scope.
fn handle_module_impl_inner(
    ctx: &ExtractionContext,
    language: Language,
) -> Result<Vec<CodeEntity>> {
    let program_node = require_capture_node(ctx.query_match, ctx.query, "program")?;

    // Extract module name from file path
    let name = derive_module_name(ctx.file_path);

    // Build qualified name from file path
    let qualified_name = derive_qualified_name(ctx.file_path, ctx.source_root, ctx.repo_root, ".");

    // Build path_entity_identifier (repo-relative path for import resolution)
    let path_entity_identifier = crate::common::module_utils::derive_path_entity_identifier(
        ctx.file_path,
        ctx.repo_root,
        ".",
    );

    // Generate entity ID
    let file_path_str = ctx.file_path.to_string_lossy();
    let entity_id = generate_entity_id(ctx.repository_id, &file_path_str, &qualified_name);

    // Get location
    let location = SourceLocation::from_tree_sitter_node(program_node);

    // Create components
    let components = CommonEntityComponents {
        entity_id,
        repository_id: ctx.repository_id.to_string(),
        name,
        qualified_name,
        path_entity_identifier: Some(path_entity_identifier),
        parent_scope: None, // Module is the top-level entity
        file_path: ctx.file_path.to_path_buf(),
        location,
    };

    // Build metadata
    let metadata = EntityMetadata::default();

    // Build the entity - always create a Module entity for JS/TS files
    // to establish the containment hierarchy
    let entity = build_entity(
        components,
        EntityDetails {
            entity_type: EntityType::Module,
            language,
            visibility: Some(Visibility::Public),
            documentation: None,
            content: None, // Don't include full file content
            metadata,
            signature: None,
            relationships: Default::default(),
        },
    )?;

    Ok(vec![entity])
}

/// JavaScript module handler
pub(crate) fn handle_module_impl(ctx: &ExtractionContext) -> Result<Vec<CodeEntity>> {
    handle_module_impl_inner(ctx, Language::JavaScript)
}

/// TypeScript module handler
pub(crate) fn handle_ts_module_impl(ctx: &ExtractionContext) -> Result<Vec<CodeEntity>> {
    handle_module_impl_inner(ctx, Language::TypeScript)
}
