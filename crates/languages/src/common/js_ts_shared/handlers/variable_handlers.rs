//! Variable and constant entity handlers for JavaScript and TypeScript

use crate::common::entity_building::{
    build_entity, extract_common_components, EntityDetails, ExtractionContext,
};
use codesearch_core::entities::{EntityMetadata, EntityType, Language};
use codesearch_core::error::Result;
use codesearch_core::CodeEntity;
use std::path::Path;
use tree_sitter::{Query, QueryMatch};

use super::super::visibility::extract_visibility;
use super::common::{extract_main_node, extract_preceding_doc_comments, node_to_text};

/// Handle const declaration extraction
///
/// Handles:
/// - `const foo = 1`
/// - `export const foo = 1`
///
/// Note: Function expressions and arrow functions are handled separately.
#[allow(clippy::too_many_arguments)]
pub fn handle_const_impl(
    query_match: &QueryMatch,
    query: &Query,
    source: &str,
    file_path: &Path,
    repository_id: &str,
    package_name: Option<&str>,
    source_root: Option<&Path>,
    repo_root: &Path,
) -> Result<Vec<CodeEntity>> {
    let node = match extract_main_node(query_match, query, &["const"]) {
        Some(n) => n,
        None => return Ok(Vec::new()),
    };

    // Create extraction context
    let ctx = ExtractionContext {
        query_match,
        query,
        source,
        file_path,
        repository_id,
        package_name,
        source_root,
        repo_root,
    };

    // Extract common components
    let components = extract_common_components(&ctx, "name", node, "javascript")?;

    // Extract JS-specific details
    let visibility = extract_visibility(node, source);
    let documentation = extract_preceding_doc_comments(node, source);
    let content = node_to_text(node, source).ok();

    let metadata = EntityMetadata {
        is_const: true,
        ..Default::default()
    };

    let entity = build_entity(
        components,
        EntityDetails {
            entity_type: EntityType::Constant,
            language: Language::JavaScript,
            visibility: Some(visibility),
            documentation,
            content,
            metadata,
            signature: None,
            relationships: Default::default(),
        },
    )?;

    Ok(vec![entity])
}

/// Handle let declaration extraction
///
/// Handles:
/// - `let foo = 1`
/// - `let foo`
/// - `export let foo = 1`
#[allow(clippy::too_many_arguments)]
pub fn handle_let_impl(
    query_match: &QueryMatch,
    query: &Query,
    source: &str,
    file_path: &Path,
    repository_id: &str,
    package_name: Option<&str>,
    source_root: Option<&Path>,
    repo_root: &Path,
) -> Result<Vec<CodeEntity>> {
    let node = match extract_main_node(query_match, query, &["let"]) {
        Some(n) => n,
        None => return Ok(Vec::new()),
    };

    // Create extraction context
    let ctx = ExtractionContext {
        query_match,
        query,
        source,
        file_path,
        repository_id,
        package_name,
        source_root,
        repo_root,
    };

    // Extract common components
    let components = extract_common_components(&ctx, "name", node, "javascript")?;

    // Extract JS-specific details
    let visibility = extract_visibility(node, source);
    let documentation = extract_preceding_doc_comments(node, source);
    let content = node_to_text(node, source).ok();

    let metadata = EntityMetadata::default();

    let entity = build_entity(
        components,
        EntityDetails {
            entity_type: EntityType::Variable,
            language: Language::JavaScript,
            visibility: Some(visibility),
            documentation,
            content,
            metadata,
            signature: None,
            relationships: Default::default(),
        },
    )?;

    Ok(vec![entity])
}

/// Handle var declaration extraction
///
/// Handles:
/// - `var foo = 1`
/// - `var foo`
/// - `export var foo = 1`
#[allow(clippy::too_many_arguments)]
pub fn handle_var_impl(
    query_match: &QueryMatch,
    query: &Query,
    source: &str,
    file_path: &Path,
    repository_id: &str,
    package_name: Option<&str>,
    source_root: Option<&Path>,
    repo_root: &Path,
) -> Result<Vec<CodeEntity>> {
    let node = match extract_main_node(query_match, query, &["var"]) {
        Some(n) => n,
        None => return Ok(Vec::new()),
    };

    // Create extraction context
    let ctx = ExtractionContext {
        query_match,
        query,
        source,
        file_path,
        repository_id,
        package_name,
        source_root,
        repo_root,
    };

    // Extract common components
    let components = extract_common_components(&ctx, "name", node, "javascript")?;

    // Extract JS-specific details
    let visibility = extract_visibility(node, source);
    let documentation = extract_preceding_doc_comments(node, source);
    let content = node_to_text(node, source).ok();

    let metadata = EntityMetadata::default();

    let entity = build_entity(
        components,
        EntityDetails {
            entity_type: EntityType::Variable,
            language: Language::JavaScript,
            visibility: Some(visibility),
            documentation,
            content,
            metadata,
            signature: None,
            relationships: Default::default(),
        },
    )?;

    Ok(vec![entity])
}
