//! TypeScript-specific entity handlers

use crate::common::entity_building::{
    build_entity, extract_common_components, EntityDetails, ExtractionContext,
};
use crate::common::js_ts_shared::TypeScript;
use crate::common::language_extractors::extract_main_node;
use crate::common::node_to_text;
use crate::define_handler;
use codesearch_core::entities::{EntityMetadata, EntityType, Language};
use codesearch_core::error::Result;
use codesearch_core::CodeEntity;

use super::super::visibility::extract_visibility;
use super::common::{extract_extends_relationships, extract_preceding_doc_comments};

define_handler!(TypeScript, handle_interface_impl, "interface", Interface, relationships: extract_extends_relationships);
define_handler!(TypeScript, handle_type_alias_impl, "type_alias", TypeAlias);
define_handler!(TypeScript, handle_namespace_impl, "namespace", Module);

/// Handle enum declaration extraction
///
/// This handler has custom logic to detect const enums,
/// so it cannot use the macro.
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
