//! Function handlers for Rust
//!
//! Handles extraction of free functions, associated functions, and constants/statics.

use crate::entity_handler;
use crate::extract_context::ExtractContext;
use crate::handler_registry::HandlerRegistration;
use crate::handlers::rust::building_blocks::{
    build_entity_with_custom_qn, build_inherent_method_qn, build_standard_entity,
    extract_documentation, extract_function_metadata, extract_function_relationships,
    extract_macro_visibility, extract_visibility,
};
use codesearch_core::entities::{EntityMetadata, EntityRelationshipData, EntityType};
use codesearch_core::error::Result;
use codesearch_core::CodeEntity;

/// Handler for free functions (not inside impl blocks)
#[entity_handler(entity_type = Function, capture = "func", language = "rust")]
fn free_function(#[capture] name: &str, ctx: &ExtractContext) -> Result<Option<CodeEntity>> {
    let metadata = extract_function_metadata(ctx);
    let visibility = extract_visibility(ctx);
    let documentation = extract_documentation(ctx);
    let relationships = extract_function_relationships(ctx, None);

    let entity = build_standard_entity(
        ctx,
        name,
        EntityType::Function,
        metadata,
        relationships,
        visibility,
        documentation,
    )?;

    Ok(Some(entity))
}

/// Handler for associated functions in inherent impl (no self parameter)
#[entity_handler(entity_type = Function, capture = "function", language = "rust")]
fn associated_function_in_inherent_impl(
    #[capture] name: &str,
    #[capture] impl_type: &str,
    ctx: &ExtractContext,
) -> Result<Option<CodeEntity>> {
    let metadata = extract_function_metadata(ctx);
    let visibility = extract_visibility(ctx);
    let documentation = extract_documentation(ctx);

    // Build qualified name: <Type>::function_name
    let qualified_name = build_inherent_method_qn(ctx, name, impl_type);
    let parent_scope = {
        // Parent is the module, not the impl block
        let parts: Vec<&str> = qualified_name.rsplitn(2, "::").collect();
        if parts.len() > 1 {
            Some(parts[1].to_string())
        } else {
            None
        }
    };

    let relationships = extract_function_relationships(ctx, parent_scope.as_deref());

    let entity = build_entity_with_custom_qn(
        ctx,
        name,
        qualified_name,
        parent_scope,
        EntityType::Function,
        metadata,
        relationships,
        visibility,
        documentation,
    )?;

    Ok(Some(entity))
}

/// Handler for constants
#[entity_handler(entity_type = Constant, capture = "const", language = "rust")]
fn constant(#[capture] name: &str, ctx: &ExtractContext) -> Result<Option<CodeEntity>> {
    let metadata = EntityMetadata::default();
    let visibility = extract_visibility(ctx);
    let documentation = extract_documentation(ctx);

    let entity = build_standard_entity(
        ctx,
        name,
        EntityType::Constant,
        metadata,
        EntityRelationshipData::default(),
        visibility,
        documentation,
    )?;

    Ok(Some(entity))
}

/// Handler for statics
#[entity_handler(entity_type = Static, capture = "static", language = "rust")]
fn static_item(#[capture] name: &str, ctx: &ExtractContext) -> Result<Option<CodeEntity>> {
    let metadata = EntityMetadata::default();
    let visibility = extract_visibility(ctx);
    let documentation = extract_documentation(ctx);

    let entity = build_standard_entity(
        ctx,
        name,
        EntityType::Static,
        metadata,
        EntityRelationshipData::default(),
        visibility,
        documentation,
    )?;

    Ok(Some(entity))
}

/// Handler for module declarations
#[entity_handler(entity_type = Module, capture = "module", language = "rust")]
fn module_declaration(#[capture] name: &str, ctx: &ExtractContext) -> Result<Option<CodeEntity>> {
    let metadata = EntityMetadata::default();
    let visibility = extract_visibility(ctx);
    let documentation = extract_documentation(ctx);

    let entity = build_standard_entity(
        ctx,
        name,
        EntityType::Module,
        metadata,
        EntityRelationshipData::default(),
        visibility,
        documentation,
    )?;

    Ok(Some(entity))
}

/// Handler for macro definitions
#[entity_handler(entity_type = Macro, capture = "macro", language = "rust")]
fn macro_definition(#[capture] name: &str, ctx: &ExtractContext) -> Result<Option<CodeEntity>> {
    let metadata = EntityMetadata::default();
    // Macros use #[macro_export] attribute instead of visibility modifiers
    let visibility = extract_macro_visibility(ctx);
    let documentation = extract_documentation(ctx);

    let entity = build_standard_entity(
        ctx,
        name,
        EntityType::Macro,
        metadata,
        EntityRelationshipData::default(),
        visibility,
        documentation,
    )?;

    Ok(Some(entity))
}
