//! Function handlers for Rust
//!
//! Handles extraction of free functions, associated functions, and constants/statics.
//!
//! Most handlers use declarative form with `#[entity_handler]` attributes.

use crate::entity_handler;
use crate::extract_context::ExtractContext;
use crate::handler_registry::HandlerRegistration;
use crate::handlers::rust::building_blocks::{
    build_entity_with_custom_qn, build_inherent_method_qn, extract_documentation,
    extract_function_metadata, extract_function_relationships, extract_macro_visibility,
    extract_visibility,
};
use codesearch_core::entities::{EntityMetadata, EntityRelationshipData, EntityType};
use codesearch_core::error::Result;
use codesearch_core::CodeEntity;

/// Handler for free functions (not inside impl blocks) - declarative form
#[entity_handler(entity_type = Function, capture = "func", language = "rust")]
#[name(capture = "name")]
#[qualified_name(standard)]
#[metadata(extract_function_metadata)]
#[relationships(extract_function_relationships)]
#[visibility(extract_visibility)]
#[documentation(extract_documentation)]
fn free_function() {}

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

/// Handler for constants (declarative form)
#[entity_handler(entity_type = Constant, capture = "const", language = "rust")]
#[name(capture = "name")]
#[qualified_name(standard)]
#[metadata(default)]
#[relationships(none)]
#[visibility(extract_visibility)]
#[documentation(extract_documentation)]
fn constant() {}

/// Handler for statics (declarative form)
#[entity_handler(entity_type = Static, capture = "static", language = "rust")]
#[name(capture = "name")]
#[qualified_name(standard)]
#[metadata(default)]
#[relationships(none)]
#[visibility(extract_visibility)]
#[documentation(extract_documentation)]
fn static_item() {}

/// Handler for module declarations (declarative form)
#[entity_handler(entity_type = Module, capture = "module", language = "rust")]
#[name(capture = "name")]
#[qualified_name(standard)]
#[metadata(default)]
#[relationships(none)]
#[visibility(extract_visibility)]
#[documentation(extract_documentation)]
fn module_declaration() {}

/// Handler for macro definitions (declarative form)
/// Macros use #[macro_export] attribute instead of visibility modifiers
#[entity_handler(entity_type = Macro, capture = "macro", language = "rust")]
#[name(capture = "name")]
#[qualified_name(standard)]
#[metadata(default)]
#[relationships(none)]
#[visibility(extract_macro_visibility)]
#[documentation(extract_documentation)]
fn macro_definition() {}
