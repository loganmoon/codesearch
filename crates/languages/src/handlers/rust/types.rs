//! Type definition handlers for Rust
//!
//! Handles extraction of structs, enums, traits, and unions.

use crate::entity_handler;
use crate::extract_context::ExtractContext;
use crate::handler_registry::HandlerRegistration;
use crate::handlers::rust::building_blocks::{
    build_standard_entity, extract_documentation, extract_struct_metadata,
    extract_trait_bounds_relationships, extract_trait_metadata, extract_type_relationships,
    extract_visibility,
};
use codesearch_core::entities::{EntityMetadata, EntityRelationshipData, EntityType};
use codesearch_core::error::Result;
use codesearch_core::CodeEntity;

/// Handler for struct definitions
#[entity_handler(entity_type = Struct, capture = "struct", language = "rust")]
fn struct_definition(#[capture] name: &str, ctx: &ExtractContext) -> Result<Option<CodeEntity>> {
    let metadata = extract_struct_metadata(ctx);
    let visibility = extract_visibility(ctx);
    let documentation = extract_documentation(ctx);
    // Extract type references from generic bounds (not field types - those belong to field entities)
    let relationships = extract_type_relationships(ctx, None);

    let entity = build_standard_entity(
        ctx,
        name,
        EntityType::Struct,
        metadata,
        relationships,
        visibility,
        documentation,
    )?;

    Ok(Some(entity))
}

/// Handler for enum definitions
#[entity_handler(entity_type = Enum, capture = "enum", language = "rust")]
fn enum_definition(#[capture] name: &str, ctx: &ExtractContext) -> Result<Option<CodeEntity>> {
    let metadata = EntityMetadata::default();
    let visibility = extract_visibility(ctx);
    let documentation = extract_documentation(ctx);

    let entity = build_standard_entity(
        ctx,
        name,
        EntityType::Enum,
        metadata,
        EntityRelationshipData::default(),
        visibility,
        documentation,
    )?;

    Ok(Some(entity))
}

/// Handler for trait definitions
#[entity_handler(entity_type = Trait, capture = "trait", language = "rust")]
fn trait_definition(#[capture] name: &str, ctx: &ExtractContext) -> Result<Option<CodeEntity>> {
    let metadata = extract_trait_metadata(ctx);
    let visibility = extract_visibility(ctx);
    let documentation = extract_documentation(ctx);
    // Extract supertrait bounds (trait Foo: Bar + Baz)
    let relationships = extract_trait_bounds_relationships(ctx, None);

    let entity = build_standard_entity(
        ctx,
        name,
        EntityType::Trait,
        metadata,
        relationships,
        visibility,
        documentation,
    )?;

    Ok(Some(entity))
}

/// Handler for union definitions
#[entity_handler(entity_type = Union, capture = "union", language = "rust")]
fn union_definition(#[capture] name: &str, ctx: &ExtractContext) -> Result<Option<CodeEntity>> {
    let metadata = EntityMetadata::default();
    let visibility = extract_visibility(ctx);
    let documentation = extract_documentation(ctx);

    let entity = build_standard_entity(
        ctx,
        name,
        EntityType::Union,
        metadata,
        EntityRelationshipData::default(),
        visibility,
        documentation,
    )?;

    Ok(Some(entity))
}

/// Handler for struct fields
#[entity_handler(entity_type = Property, capture = "field", language = "rust")]
fn struct_field(#[capture] name: &str, ctx: &ExtractContext) -> Result<Option<CodeEntity>> {
    let metadata = EntityMetadata::default();
    let visibility = extract_visibility(ctx);

    let entity = build_standard_entity(
        ctx,
        name,
        EntityType::Property,
        metadata,
        EntityRelationshipData::default(),
        visibility,
        None,
    )?;

    Ok(Some(entity))
}

/// Handler for enum variants
#[entity_handler(entity_type = EnumVariant, capture = "variant", language = "rust")]
fn enum_variant(#[capture] name: &str, ctx: &ExtractContext) -> Result<Option<CodeEntity>> {
    let metadata = EntityMetadata::default();

    let entity = build_standard_entity(
        ctx,
        name,
        EntityType::EnumVariant,
        metadata,
        EntityRelationshipData::default(),
        None, // Enum variants inherit visibility from enum
        None,
    )?;

    Ok(Some(entity))
}

/// Handler for type aliases
#[entity_handler(entity_type = TypeAlias, capture = "type_alias", language = "rust")]
fn type_alias(#[capture] name: &str, ctx: &ExtractContext) -> Result<Option<CodeEntity>> {
    let metadata = EntityMetadata::default();
    let visibility = extract_visibility(ctx);
    let documentation = extract_documentation(ctx);

    let entity = build_standard_entity(
        ctx,
        name,
        EntityType::TypeAlias,
        metadata,
        EntityRelationshipData::default(),
        visibility,
        documentation,
    )?;

    Ok(Some(entity))
}
