//! Language-agnostic entity building utilities
//!
//! This module provides shared functionality for extracting common entity
//! components and building CodeEntity instances across all languages.

use crate::common::{find_capture_node, node_to_text};
use crate::qualified_name::build_qualified_name_from_ast;
use codesearch_core::entities::{
    CodeEntityBuilder, EntityMetadata, EntityType, FunctionSignature, Language, SourceLocation,
    Visibility,
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
    pub qualified_name: String,
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
}

/// Entity-specific details for building a CodeEntity
///
/// Bundles the type-specific information needed to complete entity construction.
pub struct EntityDetails {
    pub entity_type: EntityType,
    pub language: Language,
    pub visibility: Visibility,
    pub documentation: Option<String>,
    pub content: Option<String>,
    pub metadata: EntityMetadata,
    pub signature: Option<FunctionSignature>,
}

/// Extract common entity components in a language-agnostic way
///
/// This function handles:
/// - Name extraction from query captures
/// - Qualified name building via AST traversal (using language-specific separator)
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
    let parent_scope = build_qualified_name_from_ast(main_node, ctx.source, language);

    // Get separator for this language to build full qualified name
    let separator = get_separator_for_language(language);
    let qualified_name = if parent_scope.is_empty() {
        name.clone()
    } else {
        format!("{parent_scope}{separator}{name}")
    };

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
        parent_scope: if parent_scope.is_empty() {
            None
        } else {
            Some(parent_scope)
        },
        file_path: ctx.file_path.to_path_buf(),
        location,
    })
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
        .build()
        .map_err(|e| Error::entity_extraction(format!("Failed to build CodeEntity: {e}")))
}

/// Get the separator for a language (for building qualified names)
///
/// This is a fallback for cases where the inventory lookup hasn't been done yet.
/// The primary source of truth is the ScopeConfiguration registered via inventory.
fn get_separator_for_language(language: &str) -> &'static str {
    use crate::qualified_name::ScopeConfiguration;

    inventory::iter::<ScopeConfiguration>()
        .find(|config| config.language == language)
        .map(|config| config.separator)
        .unwrap_or(match language {
            "rust" => "::",
            _ => ".",
        })
}
