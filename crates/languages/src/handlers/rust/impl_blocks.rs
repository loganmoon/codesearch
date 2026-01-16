//! Impl block handlers for Rust
//!
//! Handles extraction of inherent impl blocks and trait impl blocks.

use crate::entity_handler;
use crate::extract_context::ExtractContext;
use crate::handler_registry::HandlerRegistration;
use crate::handlers::rust::building_blocks::{
    build_entity_with_custom_qn, build_inherent_impl_qn, build_trait_impl_qn,
    derive_impl_module_scope, extract_documentation, extract_impl_relationships,
};
use codesearch_core::entities::{EntityMetadata, EntityRelationshipData, EntityType};
use codesearch_core::error::Result;
use codesearch_core::CodeEntity;

/// Handler for inherent impl blocks (impl Type { ... })
#[entity_handler(entity_type = Impl, capture = "impl", language = "rust")]
fn inherent_impl(#[capture] impl_type: &str, ctx: &ExtractContext) -> Result<Option<CodeEntity>> {
    let metadata = EntityMetadata::default();
    let documentation = extract_documentation(ctx);

    // Impl blocks don't have visibility modifiers in Rust
    let visibility = None;

    // Build qualified name: impl Type
    let qualified_name = build_inherent_impl_qn(ctx, impl_type);

    // For impl blocks, parent_scope is the module
    let parent_scope = derive_impl_module_scope(&qualified_name);

    // Use "impl Type" as the name
    let name = format!("impl {impl_type}");

    let entity = build_entity_with_custom_qn(
        ctx,
        &name,
        qualified_name,
        parent_scope,
        EntityType::Impl,
        metadata,
        EntityRelationshipData::default(),
        visibility,
        documentation,
    )?;

    Ok(Some(entity))
}

/// Handler for trait impl blocks (impl Trait for Type { ... })
#[entity_handler(entity_type = Impl, capture = "impl", language = "rust")]
fn trait_impl(
    #[capture] impl_type: &str,
    #[capture] trait_name: &str,
    ctx: &ExtractContext,
) -> Result<Option<CodeEntity>> {
    let metadata = EntityMetadata::default();
    let documentation = extract_documentation(ctx);

    // Impl blocks don't have visibility modifiers in Rust
    let visibility = None;

    // Build qualified name: <Type as Trait>
    let qualified_name = build_trait_impl_qn(ctx, impl_type, trait_name);

    // For impl blocks, parent_scope is the module
    let parent_scope = derive_impl_module_scope(&qualified_name);

    // Extract IMPLEMENTS relationship (reference to the implemented trait)
    let relationships = extract_impl_relationships(ctx, parent_scope.as_deref());

    // Use "<Type as Trait>" as the name
    let name = format!("<{impl_type} as {trait_name}>");

    let entity = build_entity_with_custom_qn(
        ctx,
        &name,
        qualified_name,
        parent_scope,
        EntityType::Impl,
        metadata,
        relationships,
        visibility,
        documentation,
    )?;

    Ok(Some(entity))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_derive_impl_module_scope_inherent() {
        assert_eq!(
            derive_impl_module_scope("my_crate::module::impl MyStruct"),
            Some("my_crate::module".to_string())
        );
        assert_eq!(derive_impl_module_scope("impl MyStruct"), None);
    }

    #[test]
    fn test_derive_impl_module_scope_trait_impl() {
        assert_eq!(
            derive_impl_module_scope("my_crate::module::<MyStruct as MyTrait>"),
            Some("my_crate::module".to_string())
        );
        assert_eq!(derive_impl_module_scope("<MyStruct as MyTrait>"), None);
    }
}
