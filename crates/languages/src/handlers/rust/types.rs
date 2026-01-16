//! Type definition handlers for Rust
//!
//! Handles extraction of structs, enums, traits, and unions.
//!
//! Most handlers use declarative form with `#[entity_handler]` attributes.

use crate::entity_handler;
use crate::extract_context::ExtractContext;
use crate::handler_registry::HandlerRegistration;
use crate::handlers::rust::building_blocks::{
    extract_documentation, extract_struct_metadata, extract_trait_bounds_relationships,
    extract_trait_metadata, extract_type_relationships, extract_visibility,
};
use codesearch_core::entities::{EntityMetadata, EntityRelationshipData, EntityType};
use codesearch_core::error::Result;
use codesearch_core::CodeEntity;

/// Handler for struct definitions (declarative form)
/// Extracts type references from generic bounds (not field types - those belong to field entities)
#[entity_handler(entity_type = Struct, capture = "struct", language = "rust")]
#[name(capture = "name")]
#[qualified_name(standard)]
#[metadata(extract_struct_metadata)]
#[relationships(extract_type_relationships)]
#[visibility(extract_visibility)]
#[documentation(extract_documentation)]
fn struct_definition() {}

/// Handler for enum definitions (declarative form)
#[entity_handler(entity_type = Enum, capture = "enum", language = "rust")]
#[name(capture = "name")]
#[qualified_name(standard)]
#[metadata(default)]
#[relationships(none)]
#[visibility(extract_visibility)]
#[documentation(extract_documentation)]
fn enum_definition() {}

/// Handler for trait definitions (declarative form)
/// Extracts supertrait bounds (trait Foo: Bar + Baz)
#[entity_handler(entity_type = Trait, capture = "trait", language = "rust")]
#[name(capture = "name")]
#[qualified_name(standard)]
#[metadata(extract_trait_metadata)]
#[relationships(extract_trait_bounds_relationships)]
#[visibility(extract_visibility)]
#[documentation(extract_documentation)]
fn trait_definition() {}

/// Handler for union definitions (declarative form)
#[entity_handler(entity_type = Union, capture = "union", language = "rust")]
#[name(capture = "name")]
#[qualified_name(standard)]
#[metadata(default)]
#[relationships(none)]
#[visibility(extract_visibility)]
#[documentation(extract_documentation)]
fn union_definition() {}

/// Handler for struct fields (declarative form)
#[entity_handler(entity_type = Property, capture = "field", language = "rust")]
#[name(capture = "name")]
#[qualified_name(standard)]
#[metadata(default)]
#[relationships(none)]
#[visibility(extract_visibility)]
#[documentation(none)]
fn struct_field() {}

/// Handler for enum variants (declarative form)
/// Enum variants inherit visibility from enum
#[entity_handler(entity_type = EnumVariant, capture = "variant", language = "rust")]
#[name(capture = "name")]
#[qualified_name(standard)]
#[metadata(default)]
#[relationships(none)]
#[visibility(none)]
#[documentation(none)]
fn enum_variant() {}

/// Handler for type aliases (declarative form)
#[entity_handler(entity_type = TypeAlias, capture = "type_alias", language = "rust")]
#[name(capture = "name")]
#[qualified_name(standard)]
#[metadata(default)]
#[relationships(none)]
#[visibility(extract_visibility)]
#[documentation(extract_documentation)]
fn type_alias() {}
