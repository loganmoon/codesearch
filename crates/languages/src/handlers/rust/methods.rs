//! Method handlers for Rust
//!
//! Handles extraction of methods in impl blocks and trait definitions.

use crate::entity_handler;
use crate::extract_context::ExtractContext;
use crate::handler_registry::HandlerRegistration;
use crate::handlers::rust::building_blocks::{
    build_entity_with_custom_qn, build_inherent_method_qn, build_trait_impl_method_qn,
    derive_parent_scope, extract_documentation, extract_function_metadata, extract_visibility,
};
use codesearch_core::entities::{EntityRelationshipData, EntityType};
use codesearch_core::error::Result;
use codesearch_core::CodeEntity;

/// Handler for methods with self parameter in inherent impl blocks
#[entity_handler(entity_type = Method, capture = "method", language = "rust")]
fn method_in_inherent_impl(
    #[capture] name: &str,
    #[capture] impl_type: &str,
    ctx: &ExtractContext,
) -> Result<Option<CodeEntity>> {
    let metadata = extract_function_metadata(ctx);
    let visibility = extract_visibility(ctx);
    let documentation = extract_documentation(ctx);

    // Build qualified name: <Type>::method_name
    let qualified_name = build_inherent_method_qn(ctx, name, impl_type);
    let parent_scope = derive_parent_scope(&qualified_name);

    let entity = build_entity_with_custom_qn(
        ctx,
        name,
        qualified_name,
        parent_scope,
        EntityType::Method,
        metadata,
        EntityRelationshipData::default(),
        visibility,
        documentation,
    )?;

    Ok(Some(entity))
}

/// Handler for methods in trait impl blocks
#[entity_handler(entity_type = Method, capture = "method", language = "rust")]
fn method_in_trait_impl(
    #[capture] name: &str,
    #[capture] impl_type: &str,
    #[capture] trait_name: &str,
    ctx: &ExtractContext,
) -> Result<Option<CodeEntity>> {
    let metadata = extract_function_metadata(ctx);
    let visibility = extract_visibility(ctx);
    let documentation = extract_documentation(ctx);

    // Build qualified name: <Type as Trait>::method_name
    let qualified_name = build_trait_impl_method_qn(ctx, name, impl_type, trait_name);
    let parent_scope = derive_parent_scope(&qualified_name);

    let entity = build_entity_with_custom_qn(
        ctx,
        name,
        qualified_name,
        parent_scope,
        EntityType::Method,
        metadata,
        EntityRelationshipData::default(),
        visibility,
        documentation,
    )?;

    Ok(Some(entity))
}

/// Handler for method signatures in trait definitions
#[entity_handler(entity_type = Method, capture = "method", language = "rust")]
fn method_in_trait_def(
    #[capture] name: &str,
    #[capture] trait_name: &str,
    ctx: &ExtractContext,
) -> Result<Option<CodeEntity>> {
    let metadata = extract_function_metadata(ctx);
    let documentation = extract_documentation(ctx);

    // Methods in trait definitions are always public (part of the trait interface)
    let visibility = None;

    // Build qualified name: Trait::method_name
    let base = crate::handlers::rust::building_blocks::build_function_qn(ctx, "");
    let qualified_name = if base.is_empty() {
        format!("{trait_name}::{name}")
    } else {
        format!("{base}::{trait_name}::{name}")
    };
    let parent_scope = derive_parent_scope(&qualified_name);

    let entity = build_entity_with_custom_qn(
        ctx,
        name,
        qualified_name,
        parent_scope,
        EntityType::Method,
        metadata,
        EntityRelationshipData::default(),
        visibility,
        documentation,
    )?;

    Ok(Some(entity))
}
