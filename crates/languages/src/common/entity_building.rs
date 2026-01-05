//! Language-agnostic entity building utilities
//!
//! This module provides shared functionality for extracting common entity
//! components and building CodeEntity instances across all languages.

use crate::common::{find_capture_node, node_to_text};
use crate::qualified_name::build_qualified_name_from_ast;
use codesearch_core::entities::{
    CodeEntityBuilder, EntityMetadata, EntityRelationshipData, EntityType, FunctionSignature,
    Language, SourceLocation, Visibility,
};
use codesearch_core::entity_id::generate_entity_id;
use codesearch_core::error::{Error, Result};
use codesearch_core::CodeEntity;
use std::path::Path;
use tree_sitter::{Node, Query, QueryMatch};

/// Common components extracted from all entity types
///
/// These are the universal components needed for every CodeEntity,
/// regardless of language or entity type.
pub struct CommonEntityComponents {
    pub entity_id: String,
    pub repository_id: String,
    pub name: String,
    /// Semantic, package-relative qualified name (e.g., "jotai.utils.helpers.formatNumber")
    pub qualified_name: String,
    /// File-path-based identifier for import resolution (e.g., "website.src.pages.index.formatNumber")
    pub path_entity_identifier: Option<String>,
    pub parent_scope: Option<String>,
    pub file_path: std::path::PathBuf,
    pub location: SourceLocation,
}

/// Context for entity extraction containing query and source information
///
/// Bundles the common parameters needed across entity extraction functions.
pub struct ExtractionContext<'a> {
    pub query_match: &'a QueryMatch<'a, 'a>,
    pub query: &'a Query,
    pub source: &'a str,
    pub file_path: &'a Path,
    pub repository_id: &'a str,
    /// Package/crate name from manifest (e.g., "codesearch_core" from Cargo.toml)
    pub package_name: Option<&'a str>,
    /// Source root path for module path derivation (e.g., "/project/src")
    pub source_root: Option<&'a Path>,
    /// Repository root path for repo-relative path generation
    pub repo_root: &'a Path,
}

/// Entity-specific details for building a CodeEntity
///
/// Bundles the type-specific information needed to complete entity construction.
pub struct EntityDetails {
    pub entity_type: EntityType,
    pub language: Language,
    pub visibility: Option<Visibility>,
    pub documentation: Option<String>,
    pub content: Option<String>,
    pub metadata: EntityMetadata,
    pub signature: Option<FunctionSignature>,
    /// Typed relationship data for graph resolution.
    /// Defaults to empty if not provided.
    pub relationships: EntityRelationshipData,
}

/// Extract common entity components in a language-agnostic way
///
/// This function handles:
/// - Name extraction from query captures
/// - Qualified name building via AST traversal (using language-specific separator)
/// - Module path derivation from file path (when source_root is provided)
/// - Entity ID generation
/// - Source location extraction
///
/// Language-specific concerns (visibility, documentation, parameters) should be
/// handled by the caller.
///
/// # Arguments
/// * `ctx` - Extraction context containing query match, query, source, file path, and repository ID
/// * `name_capture` - Name of the capture containing the entity name
/// * `main_node` - The main AST node for this entity
/// * `language` - Language identifier for qualified name building (e.g., "rust", "python")
pub fn extract_common_components(
    ctx: &ExtractionContext,
    name_capture: &str,
    main_node: Node,
    language: &str,
) -> Result<CommonEntityComponents> {
    // Extract name from capture, defaulting to empty string if not found
    let name = find_capture_node(ctx.query_match, ctx.query, name_capture)
        .and_then(|node| node_to_text(node, ctx.source).ok())
        .unwrap_or_default();

    if name.is_empty() {
        return Err(Error::entity_extraction(format!(
            "Could not extract name from capture '{name_capture}'"
        )));
    }

    // Build qualified name via parent traversal using language-specific separator
    let scope_result = build_qualified_name_from_ast(main_node, ctx.source, language);
    let ast_scope = scope_result.parent_scope;
    let separator = scope_result.separator;

    // Derive module path from file path (if source_root is available)
    let module_prefix = ctx.source_root.and_then(|root| {
        crate::qualified_name::derive_module_path_for_language(ctx.file_path, root, language)
    });

    // Compose fully qualified name: package::module::ast_scope::name
    let qualified_name = compose_qualified_name(
        ctx.package_name,
        module_prefix.as_deref(),
        &ast_scope,
        &name,
        separator,
    );

    // Calculate parent_scope (everything except the final name)
    let parent_scope = compose_qualified_name(
        ctx.package_name,
        module_prefix.as_deref(),
        &ast_scope,
        "", // empty name to get just the parent scope
        separator,
    );

    // Generate path_entity_identifier using repo-relative path (for import resolution)
    let path_module = crate::common::module_utils::derive_path_entity_identifier(
        ctx.file_path,
        ctx.repo_root,
        separator,
    );
    let path_entity_identifier = compose_qualified_name(
        None, // No package prefix for path-based identifier
        Some(&path_module),
        &ast_scope,
        &name,
        separator,
    );

    // Generate entity_id from repository + file_path + qualified name
    let file_path_str = ctx
        .file_path
        .to_str()
        .ok_or_else(|| Error::entity_extraction("Invalid file path"))?;
    let entity_id = generate_entity_id(ctx.repository_id, file_path_str, &qualified_name);

    // Get location
    let location = SourceLocation::from_tree_sitter_node(main_node);

    Ok(CommonEntityComponents {
        entity_id,
        repository_id: ctx.repository_id.to_string(),
        name,
        qualified_name,
        path_entity_identifier: Some(path_entity_identifier),
        parent_scope: if parent_scope.is_empty() {
            None
        } else {
            Some(parent_scope)
        },
        file_path: ctx.file_path.to_path_buf(),
        location,
    })
}

/// Compose a fully qualified name from components
///
/// Joins non-empty components with the separator. If name is empty, returns
/// just the scope parts (useful for calculating parent_scope).
///
/// # Arguments
/// * `package` - Optional package/crate name (e.g., "codesearch_core")
/// * `module` - Optional module path from file location (e.g., "entities")
/// * `scope` - AST-derived scope from parent traversal (e.g., "MyStruct")
/// * `name` - The entity name (e.g., "my_method")
/// * `separator` - Language-specific separator (e.g., "::" for Rust, "." for Python)
fn compose_qualified_name(
    package: Option<&str>,
    module: Option<&str>,
    scope: &str,
    name: &str,
    separator: &str,
) -> String {
    let mut parts: Vec<&str> = Vec::with_capacity(4);

    if let Some(pkg) = package {
        if !pkg.is_empty() {
            parts.push(pkg);
        }
    }

    if let Some(mod_path) = module {
        if !mod_path.is_empty() {
            parts.push(mod_path);
        }
    }

    if !scope.is_empty() {
        parts.push(scope);
    }

    if !name.is_empty() {
        parts.push(name);
    }

    parts.join(separator)
}

/// Build a CodeEntity from common components and entity-specific data
///
/// This is the final assembly step that combines:
/// - Universal components (from extract_common_components)
/// - Entity-specific details (type, language, visibility, documentation, etc.)
///
/// # Arguments
/// * `components` - Common components from extract_common_components
/// * `details` - Entity-specific details (type, language, visibility, etc.)
pub fn build_entity(
    components: CommonEntityComponents,
    details: EntityDetails,
) -> Result<CodeEntity> {
    CodeEntityBuilder::default()
        .entity_id(components.entity_id)
        .repository_id(components.repository_id)
        .name(components.name)
        .qualified_name(components.qualified_name)
        .path_entity_identifier(components.path_entity_identifier)
        .parent_scope(components.parent_scope)
        .entity_type(details.entity_type)
        .location(components.location)
        .visibility(details.visibility)
        .documentation_summary(details.documentation)
        .content(details.content)
        .metadata(details.metadata)
        .signature(details.signature)
        .language(details.language)
        .file_path(components.file_path)
        .relationships(details.relationships)
        .build()
        .map_err(|e| Error::entity_extraction(format!("Failed to build CodeEntity: {e}")))
}
