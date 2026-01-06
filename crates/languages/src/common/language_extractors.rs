//! Language-agnostic entity extraction framework
//!
//! This module provides a trait-based approach for defining language-specific
//! extraction behavior, combined with a generic extraction function and macro
//! for concise handler definitions.
//!
//! # Architecture
//!
//! - [`LanguageExtractors`] trait: Defines language-specific behavior (visibility, docs)
//! - [`extract_entity`] function: Generic extraction using trait implementations
//! - [`define_handler!`] macro: Concise handler definitions for any language
//!
//! # Adding a New Language
//!
//! 1. Create a unit struct for the language (e.g., `pub struct MyLanguage;`)
//! 2. Implement `LanguageExtractors` for it
//! 3. Use `define_handler!(MyLanguage, ...)` to create handlers

use crate::common::entity_building::{
    build_entity, extract_common_components, EntityDetails, ExtractionContext,
};
use crate::common::node_to_text;
use codesearch_core::entities::{
    EntityMetadata, EntityRelationshipData, EntityType, Language, Visibility,
};
use codesearch_core::error::Result;
use codesearch_core::CodeEntity;
use tree_sitter::{Node, Query, QueryMatch};

/// Trait defining language-specific extraction behavior
///
/// Implement this trait for each supported language to provide:
/// - The language enum variant and string identifier
/// - Visibility extraction logic
/// - Documentation extraction logic
///
/// # Example
///
/// ```ignore
/// pub struct MyLanguage;
///
/// impl LanguageExtractors for MyLanguage {
///     const LANGUAGE: Language = Language::MyLanguage;
///     const LANG_STR: &'static str = "mylanguage";
///
///     fn extract_visibility(node: Node, source: &str) -> Visibility {
///         // Language-specific visibility extraction
///     }
///
///     fn extract_docs(node: Node, source: &str) -> Option<String> {
///         // Language-specific doc comment extraction
///     }
/// }
/// ```
pub trait LanguageExtractors {
    /// The Language enum variant for this language
    const LANGUAGE: Language;

    /// String identifier used for qualified name building (e.g., "rust", "javascript")
    const LANG_STR: &'static str;

    /// Extract visibility from an AST node
    ///
    /// Different languages have different visibility conventions:
    /// - Rust: `pub`, `pub(crate)`, `pub(super)`, etc.
    /// - JS/TS: `export` keyword
    /// - Python: `_` prefix convention
    fn extract_visibility(node: Node, source: &str) -> Visibility;

    /// Extract documentation comments from an AST node
    ///
    /// Different languages have different doc comment styles:
    /// - Rust: `///` and `//!`
    /// - JS/TS: `/** */` JSDoc
    /// - Python: `"""docstrings"""`
    fn extract_docs(node: Node, source: &str) -> Option<String>;
}

/// Extract the main captured node from a query match
///
/// Looks for captures matching any of the provided names.
pub fn extract_main_node<'a>(
    query_match: &QueryMatch<'a, 'a>,
    query: &Query,
    capture_names: &[&str],
) -> Option<Node<'a>> {
    for name in capture_names {
        if let Some(index) = query.capture_index_for_name(name) {
            for capture in query_match.captures {
                if capture.index == index {
                    return Some(capture.node);
                }
            }
        }
    }
    None
}

/// Generic entity extraction function using language extractors
///
/// This function handles the common extraction pattern:
/// 1. Extract main node from query match
/// 2. Extract common components (name, location, qualified name)
/// 3. Extract visibility using language-specific logic
/// 4. Extract documentation using language-specific logic
/// 5. Build metadata and relationships using provided functions
/// 6. Assemble and return the entity
///
/// # Type Parameters
///
/// * `L` - Language extractor implementing [`LanguageExtractors`]
///
/// # Arguments
///
/// * `ctx` - Extraction context with query match and source
/// * `capture` - Name of the capture for the main node
/// * `entity_type` - The type of entity being extracted
/// * `metadata_fn` - Function to build entity metadata
/// * `relationships_fn` - Function to build entity relationships
pub fn extract_entity<L: LanguageExtractors>(
    ctx: &ExtractionContext,
    capture: &str,
    entity_type: EntityType,
    metadata_fn: fn(Node, &str) -> EntityMetadata,
    relationships_fn: fn(&ExtractionContext, Node) -> EntityRelationshipData,
) -> Result<Vec<CodeEntity>> {
    let node = match extract_main_node(ctx.query_match, ctx.query, &[capture]) {
        Some(n) => n,
        None => return Ok(Vec::new()),
    };

    let components = extract_common_components(ctx, "name", node, L::LANG_STR)?;
    let visibility = L::extract_visibility(node, ctx.source);
    let documentation = L::extract_docs(node, ctx.source);
    let content = node_to_text(node, ctx.source).ok();
    let metadata = metadata_fn(node, ctx.source);
    let relationships = relationships_fn(ctx, node);

    let entity = build_entity(
        components,
        EntityDetails {
            entity_type,
            language: L::LANGUAGE,
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

/// Default metadata function that returns empty metadata
pub fn default_metadata(_node: Node, _source: &str) -> EntityMetadata {
    EntityMetadata::default()
}

/// Default relationships function that returns empty relationships
pub fn no_relationships(_ctx: &ExtractionContext, _node: Node) -> EntityRelationshipData {
    EntityRelationshipData::default()
}

/// Define an entity handler using the language extractors framework
///
/// This macro generates handler functions that use the generic extraction
/// infrastructure with language-specific behavior provided by trait implementations.
///
/// # Variants
///
/// ```ignore
/// // Basic handler with default metadata and no relationships
/// define_handler!(JavaScript, handle_let_impl, "let", Variable);
///
/// // Handler with custom metadata
/// define_handler!(JavaScript, handle_function_impl, "function", Function,
///     metadata: function_metadata);
///
/// // Handler with custom relationships
/// define_handler!(JavaScript, handle_class_impl, "class", Class,
///     relationships: extract_extends);
///
/// // Handler with both custom metadata and relationships
/// define_handler!(JavaScript, handle_method_impl, "method", Method,
///     metadata: method_metadata,
///     relationships: extract_implements);
/// ```
#[macro_export]
macro_rules! define_handler {
    // Basic: default metadata, no relationships
    (
        $lang:ty,
        $fn_name:ident,
        $capture:expr,
        $entity_type:ident
    ) => {
        pub(crate) fn $fn_name(
            ctx: &$crate::common::entity_building::ExtractionContext,
        ) -> codesearch_core::error::Result<Vec<codesearch_core::CodeEntity>> {
            $crate::common::language_extractors::extract_entity::<$lang>(
                ctx,
                $capture,
                codesearch_core::entities::EntityType::$entity_type,
                $crate::common::language_extractors::default_metadata,
                $crate::common::language_extractors::no_relationships,
            )
        }
    };

    // With custom metadata
    (
        $lang:ty,
        $fn_name:ident,
        $capture:expr,
        $entity_type:ident,
        metadata: $metadata_fn:expr
    ) => {
        pub(crate) fn $fn_name(
            ctx: &$crate::common::entity_building::ExtractionContext,
        ) -> codesearch_core::error::Result<Vec<codesearch_core::CodeEntity>> {
            $crate::common::language_extractors::extract_entity::<$lang>(
                ctx,
                $capture,
                codesearch_core::entities::EntityType::$entity_type,
                $metadata_fn,
                $crate::common::language_extractors::no_relationships,
            )
        }
    };

    // With custom relationships
    (
        $lang:ty,
        $fn_name:ident,
        $capture:expr,
        $entity_type:ident,
        relationships: $rel_fn:expr
    ) => {
        pub(crate) fn $fn_name(
            ctx: &$crate::common::entity_building::ExtractionContext,
        ) -> codesearch_core::error::Result<Vec<codesearch_core::CodeEntity>> {
            $crate::common::language_extractors::extract_entity::<$lang>(
                ctx,
                $capture,
                codesearch_core::entities::EntityType::$entity_type,
                $crate::common::language_extractors::default_metadata,
                $rel_fn,
            )
        }
    };

    // With both custom metadata and relationships
    (
        $lang:ty,
        $fn_name:ident,
        $capture:expr,
        $entity_type:ident,
        metadata: $metadata_fn:expr,
        relationships: $rel_fn:expr
    ) => {
        pub(crate) fn $fn_name(
            ctx: &$crate::common::entity_building::ExtractionContext,
        ) -> codesearch_core::error::Result<Vec<codesearch_core::CodeEntity>> {
            $crate::common::language_extractors::extract_entity::<$lang>(
                ctx,
                $capture,
                codesearch_core::entities::EntityType::$entity_type,
                $metadata_fn,
                $rel_fn,
            )
        }
    };
}

pub use define_handler;

#[cfg(test)]
mod tests {
    use super::*;

    // Test that the trait is object-safe (can be used with generics)
    fn _assert_trait_bounds<L: LanguageExtractors>() {
        let _: Language = L::LANGUAGE;
        let _: &str = L::LANG_STR;
    }
}
