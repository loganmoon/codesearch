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
    build_entity, extract_common_components, extract_common_components_with_name, EntityDetails,
    ExtractionContext,
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

/// Entity extraction with visibility override
///
/// Same as `extract_entity` but uses a static visibility value instead of
/// extracting it from the AST. Useful for interface members which are always Public.
pub fn extract_entity_with_visibility<L: LanguageExtractors>(
    ctx: &ExtractionContext,
    capture: &str,
    entity_type: EntityType,
    visibility: Visibility,
    metadata_fn: fn(Node, &str) -> EntityMetadata,
    relationships_fn: fn(&ExtractionContext, Node) -> EntityRelationshipData,
) -> Result<Vec<CodeEntity>> {
    let node = match extract_main_node(ctx.query_match, ctx.query, &[capture]) {
        Some(n) => n,
        None => return Ok(Vec::new()),
    };

    let components = extract_common_components(ctx, "name", node, L::LANG_STR)?;
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

/// Entity extraction with static name and visibility override
///
/// Uses a provided name string instead of extracting from a capture.
/// Useful for call signatures (`()`), construct signatures (`new()`), etc.
pub fn extract_entity_with_name<L: LanguageExtractors>(
    ctx: &ExtractionContext,
    capture: &str,
    entity_type: EntityType,
    name: &str,
    visibility: Visibility,
    metadata_fn: fn(Node, &str) -> EntityMetadata,
    relationships_fn: fn(&ExtractionContext, Node) -> EntityRelationshipData,
) -> Result<Vec<CodeEntity>> {
    let node = match extract_main_node(ctx.query_match, ctx.query, &[capture]) {
        Some(n) => n,
        None => return Ok(Vec::new()),
    };

    let components = extract_common_components_with_name(ctx, name, node, L::LANG_STR)?;
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

/// Entity extraction with name derivation function and visibility override
///
/// Uses a function to derive the name from the AST node.
/// Useful for index signatures where name is derived from the index type.
pub fn extract_entity_with_name_fn<L: LanguageExtractors>(
    ctx: &ExtractionContext,
    capture: &str,
    entity_type: EntityType,
    name_fn: fn(Node, &str) -> String,
    visibility: Visibility,
    metadata_fn: fn(Node, &str) -> EntityMetadata,
    relationships_fn: fn(&ExtractionContext, Node) -> EntityRelationshipData,
) -> Result<Vec<CodeEntity>> {
    let node = match extract_main_node(ctx.query_match, ctx.query, &[capture]) {
        Some(n) => n,
        None => return Ok(Vec::new()),
    };

    let name = name_fn(node, ctx.source);
    let components = extract_common_components_with_name(ctx, &name, node, L::LANG_STR)?;
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

/// Entity extraction with context-aware name function
///
/// Uses a function that receives the full ExtractionContext to derive the name.
/// Useful for:
/// - Module handlers that derive name from file path
/// - Function expressions that try multiple capture names
///
/// The name function can return an error if it cannot derive a valid name.
pub fn extract_entity_with_name_ctx_fn<L: LanguageExtractors>(
    ctx: &ExtractionContext,
    capture: &str,
    entity_type: EntityType,
    name_ctx_fn: fn(&ExtractionContext, Node) -> Result<String>,
    visibility_override: Option<Visibility>,
    metadata_fn: fn(Node, &str) -> EntityMetadata,
    relationships_fn: fn(&ExtractionContext, Node) -> EntityRelationshipData,
) -> Result<Vec<CodeEntity>> {
    let node = match extract_main_node(ctx.query_match, ctx.query, &[capture]) {
        Some(n) => n,
        None => return Ok(Vec::new()),
    };

    let name = name_ctx_fn(ctx, node)?;
    let components = extract_common_components_with_name(ctx, &name, node, L::LANG_STR)?;
    let visibility = visibility_override.unwrap_or_else(|| L::extract_visibility(node, ctx.source));
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

/// Entity extraction for module/file-level entities
///
/// Unlike other entity types, modules derive their qualified name from the file path
/// rather than from AST scope traversal. This function handles that special case.
///
/// The name function receives the ExtractionContext to derive the module name from file path.
pub fn extract_module_entity<L: LanguageExtractors>(
    ctx: &ExtractionContext,
    capture: &str,
    name_fn: fn(&ExtractionContext, Node) -> codesearch_core::error::Result<String>,
) -> Result<Vec<CodeEntity>> {
    use crate::common::module_utils;
    use codesearch_core::entity_id::generate_entity_id;

    let node = match extract_main_node(ctx.query_match, ctx.query, &[capture]) {
        Some(n) => n,
        None => return Ok(Vec::new()),
    };

    let name = name_fn(ctx, node)?;

    // For modules, derive qualified_name from file path, not AST scope
    let qualified_name = module_utils::derive_qualified_name(
        ctx.file_path,
        ctx.source_root,
        ctx.repo_root,
        ".", // JS/TS use dot separator
    );

    // Build path_entity_identifier (repo-relative path for import resolution)
    let path_entity_identifier =
        module_utils::derive_path_entity_identifier(ctx.file_path, ctx.repo_root, ".");

    // Generate entity ID
    let file_path_str = ctx.file_path.to_string_lossy();
    let entity_id = generate_entity_id(ctx.repository_id, &file_path_str, &qualified_name);

    // Get location from node
    let location = codesearch_core::entities::SourceLocation::from_tree_sitter_node(node);

    // Build components manually for module entities
    let components = crate::common::entity_building::CommonEntityComponents {
        entity_id,
        repository_id: ctx.repository_id.to_string(),
        name,
        qualified_name,
        path_entity_identifier: Some(path_entity_identifier),
        parent_scope: None, // Module is the top-level entity
        file_path: ctx.file_path.to_path_buf(),
        location,
    };

    let entity = build_entity(
        components,
        EntityDetails {
            entity_type: EntityType::Module,
            language: L::LANGUAGE,
            visibility: Some(Visibility::Public), // Modules are always public
            documentation: None,
            content: None, // Don't include full file content for performance
            metadata: EntityMetadata::default(),
            signature: None,
            relationships: EntityRelationshipData::default(),
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
///
/// // Handler with visibility override (for interface members)
/// define_handler!(TypeScript, handle_interface_property_impl, "interface_property", Property,
///     visibility: Visibility::Public);
///
/// // Handler with static name and visibility (for call/construct signatures)
/// define_handler!(TypeScript, handle_call_signature_impl, "call_signature", Method,
///     name: "()",
///     visibility: Visibility::Public);
///
/// // Handler with name derivation function and visibility (for index signatures)
/// define_handler!(TypeScript, handle_index_signature_impl, "index_signature", Property,
///     name_fn: derive_index_signature_name,
///     visibility: Visibility::Public);
///
/// // Handler with context-aware name function (for module handlers)
/// define_handler!(JavaScript, handle_module_impl, "program", Module,
///     name_ctx_fn: derive_module_name_from_ctx,
///     visibility: Visibility::Public);
///
/// // Handler with context-aware name function and metadata (for function expressions)
/// define_handler!(JavaScript, handle_function_expression_impl, "function", Function,
///     name_ctx_fn: derive_function_expression_name,
///     metadata: function_metadata);
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

    // =========================================================================
    // Visibility override variants
    // =========================================================================

    // With visibility override only
    (
        $lang:ty,
        $fn_name:ident,
        $capture:expr,
        $entity_type:ident,
        visibility: $visibility:expr
    ) => {
        pub(crate) fn $fn_name(
            ctx: &$crate::common::entity_building::ExtractionContext,
        ) -> codesearch_core::error::Result<Vec<codesearch_core::CodeEntity>> {
            $crate::common::language_extractors::extract_entity_with_visibility::<$lang>(
                ctx,
                $capture,
                codesearch_core::entities::EntityType::$entity_type,
                $visibility,
                $crate::common::language_extractors::default_metadata,
                $crate::common::language_extractors::no_relationships,
            )
        }
    };

    // With visibility override and metadata
    (
        $lang:ty,
        $fn_name:ident,
        $capture:expr,
        $entity_type:ident,
        visibility: $visibility:expr,
        metadata: $metadata_fn:expr
    ) => {
        pub(crate) fn $fn_name(
            ctx: &$crate::common::entity_building::ExtractionContext,
        ) -> codesearch_core::error::Result<Vec<codesearch_core::CodeEntity>> {
            $crate::common::language_extractors::extract_entity_with_visibility::<$lang>(
                ctx,
                $capture,
                codesearch_core::entities::EntityType::$entity_type,
                $visibility,
                $metadata_fn,
                $crate::common::language_extractors::no_relationships,
            )
        }
    };

    // =========================================================================
    // Static name variants (for call/construct signatures)
    // =========================================================================

    // With static name and visibility
    (
        $lang:ty,
        $fn_name:ident,
        $capture:expr,
        $entity_type:ident,
        name: $name:expr,
        visibility: $visibility:expr
    ) => {
        pub(crate) fn $fn_name(
            ctx: &$crate::common::entity_building::ExtractionContext,
        ) -> codesearch_core::error::Result<Vec<codesearch_core::CodeEntity>> {
            $crate::common::language_extractors::extract_entity_with_name::<$lang>(
                ctx,
                $capture,
                codesearch_core::entities::EntityType::$entity_type,
                $name,
                $visibility,
                $crate::common::language_extractors::default_metadata,
                $crate::common::language_extractors::no_relationships,
            )
        }
    };

    // =========================================================================
    // Name function variants (for index signatures)
    // =========================================================================

    // With name function and visibility
    (
        $lang:ty,
        $fn_name:ident,
        $capture:expr,
        $entity_type:ident,
        name_fn: $name_fn:expr,
        visibility: $visibility:expr
    ) => {
        pub(crate) fn $fn_name(
            ctx: &$crate::common::entity_building::ExtractionContext,
        ) -> codesearch_core::error::Result<Vec<codesearch_core::CodeEntity>> {
            $crate::common::language_extractors::extract_entity_with_name_fn::<$lang>(
                ctx,
                $capture,
                codesearch_core::entities::EntityType::$entity_type,
                $name_fn,
                $visibility,
                $crate::common::language_extractors::default_metadata,
                $crate::common::language_extractors::no_relationships,
            )
        }
    };

    // =========================================================================
    // Context-aware name function variants (for module handlers, function expressions)
    // =========================================================================

    // With context-aware name function only (uses trait visibility)
    (
        $lang:ty,
        $fn_name:ident,
        $capture:expr,
        $entity_type:ident,
        name_ctx_fn: $name_ctx_fn:expr
    ) => {
        pub(crate) fn $fn_name(
            ctx: &$crate::common::entity_building::ExtractionContext,
        ) -> codesearch_core::error::Result<Vec<codesearch_core::CodeEntity>> {
            $crate::common::language_extractors::extract_entity_with_name_ctx_fn::<$lang>(
                ctx,
                $capture,
                codesearch_core::entities::EntityType::$entity_type,
                $name_ctx_fn,
                None,
                $crate::common::language_extractors::default_metadata,
                $crate::common::language_extractors::no_relationships,
            )
        }
    };

    // With context-aware name function and visibility
    (
        $lang:ty,
        $fn_name:ident,
        $capture:expr,
        $entity_type:ident,
        name_ctx_fn: $name_ctx_fn:expr,
        visibility: $visibility:expr
    ) => {
        pub(crate) fn $fn_name(
            ctx: &$crate::common::entity_building::ExtractionContext,
        ) -> codesearch_core::error::Result<Vec<codesearch_core::CodeEntity>> {
            $crate::common::language_extractors::extract_entity_with_name_ctx_fn::<$lang>(
                ctx,
                $capture,
                codesearch_core::entities::EntityType::$entity_type,
                $name_ctx_fn,
                Some($visibility),
                $crate::common::language_extractors::default_metadata,
                $crate::common::language_extractors::no_relationships,
            )
        }
    };

    // With context-aware name function and metadata (uses trait visibility)
    (
        $lang:ty,
        $fn_name:ident,
        $capture:expr,
        $entity_type:ident,
        name_ctx_fn: $name_ctx_fn:expr,
        metadata: $metadata_fn:expr
    ) => {
        pub(crate) fn $fn_name(
            ctx: &$crate::common::entity_building::ExtractionContext,
        ) -> codesearch_core::error::Result<Vec<codesearch_core::CodeEntity>> {
            $crate::common::language_extractors::extract_entity_with_name_ctx_fn::<$lang>(
                ctx,
                $capture,
                codesearch_core::entities::EntityType::$entity_type,
                $name_ctx_fn,
                None,
                $metadata_fn,
                $crate::common::language_extractors::no_relationships,
            )
        }
    };

    // With context-aware name function, visibility, and metadata
    (
        $lang:ty,
        $fn_name:ident,
        $capture:expr,
        $entity_type:ident,
        name_ctx_fn: $name_ctx_fn:expr,
        visibility: $visibility:expr,
        metadata: $metadata_fn:expr
    ) => {
        pub(crate) fn $fn_name(
            ctx: &$crate::common::entity_building::ExtractionContext,
        ) -> codesearch_core::error::Result<Vec<codesearch_core::CodeEntity>> {
            $crate::common::language_extractors::extract_entity_with_name_ctx_fn::<$lang>(
                ctx,
                $capture,
                codesearch_core::entities::EntityType::$entity_type,
                $name_ctx_fn,
                Some($visibility),
                $metadata_fn,
                $crate::common::language_extractors::no_relationships,
            )
        }
    };

    // With context-aware name function and relationships (uses trait visibility)
    (
        $lang:ty,
        $fn_name:ident,
        $capture:expr,
        $entity_type:ident,
        name_ctx_fn: $name_ctx_fn:expr,
        relationships: $rel_fn:expr
    ) => {
        pub(crate) fn $fn_name(
            ctx: &$crate::common::entity_building::ExtractionContext,
        ) -> codesearch_core::error::Result<Vec<codesearch_core::CodeEntity>> {
            $crate::common::language_extractors::extract_entity_with_name_ctx_fn::<$lang>(
                ctx,
                $capture,
                codesearch_core::entities::EntityType::$entity_type,
                $name_ctx_fn,
                None,
                $crate::common::language_extractors::default_metadata,
                $rel_fn,
            )
        }
    };

    // =========================================================================
    // Module entity variant (file-level entities with path-based qualified names)
    // =========================================================================

    // Module entity with context-aware name function
    (
        $lang:ty,
        $fn_name:ident,
        $capture:expr,
        module_name_fn: $name_fn:expr
    ) => {
        pub(crate) fn $fn_name(
            ctx: &$crate::common::entity_building::ExtractionContext,
        ) -> codesearch_core::error::Result<Vec<codesearch_core::CodeEntity>> {
            $crate::common::language_extractors::extract_module_entity::<$lang>(
                ctx, $capture, $name_fn,
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
