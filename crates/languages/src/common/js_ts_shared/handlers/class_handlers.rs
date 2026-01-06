//! Class entity handlers for JavaScript and TypeScript

use crate::common::entity_building::{
    build_entity, extract_common_components, EntityDetails, ExtractionContext,
};
use codesearch_core::entities::{
    EntityMetadata, EntityRelationshipData, EntityType, Language, SourceReference,
};
use codesearch_core::error::Result;
use codesearch_core::CodeEntity;
use std::path::Path;
use tree_sitter::{Query, QueryMatch};

use super::super::visibility::extract_visibility;
use super::common::{extract_main_node, extract_preceding_doc_comments, node_to_text};

/// Handle class declaration extraction
///
/// Handles:
/// - `class Foo {}`
/// - `class Foo extends Bar {}`
/// - `export class Foo {}`
#[allow(clippy::too_many_arguments)]
pub fn handle_class_declaration_impl(
    query_match: &QueryMatch,
    query: &Query,
    source: &str,
    file_path: &Path,
    repository_id: &str,
    package_name: Option<&str>,
    source_root: Option<&Path>,
    repo_root: &Path,
) -> Result<Vec<CodeEntity>> {
    let node = match extract_main_node(query_match, query, &["class"]) {
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

    // Extract extends clause if present
    let mut relationships = EntityRelationshipData::default();
    if let Some(extends_index) = query.capture_index_for_name("extends") {
        for capture in query_match.captures {
            if capture.index == extends_index {
                let extends_name = &source[capture.node.byte_range()];
                // Add to extends Vec using SourceReference
                if let Ok(source_ref) = SourceReference::builder()
                    .target(extends_name.to_string())
                    .simple_name(extends_name.to_string())
                    .build()
                {
                    relationships.extends.push(source_ref);
                }
                break;
            }
        }
    }

    let metadata = EntityMetadata::default();

    let entity = build_entity(
        components,
        EntityDetails {
            entity_type: EntityType::Class,
            language: Language::JavaScript,
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

/// Handle class expression extraction
///
/// Handles:
/// - `const Foo = class {}`
/// - `const Foo = class Bar {}`
/// - `let Foo = class extends Base {}`
#[allow(clippy::too_many_arguments)]
pub fn handle_class_expression_impl(
    query_match: &QueryMatch,
    query: &Query,
    source: &str,
    file_path: &Path,
    repository_id: &str,
    package_name: Option<&str>,
    source_root: Option<&Path>,
    repo_root: &Path,
) -> Result<Vec<CodeEntity>> {
    let node = match extract_main_node(query_match, query, &["class"]) {
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

    // For class expressions, the name comes from the variable, not the class
    let components = extract_common_components(&ctx, "name", node, "javascript")?;

    // Extract JS-specific details
    let visibility = extract_visibility(node, source);
    let documentation = extract_preceding_doc_comments(node, source);
    let content = node_to_text(node, source).ok();

    // Extract extends clause if present
    let mut relationships = EntityRelationshipData::default();
    if let Some(extends_index) = query.capture_index_for_name("extends") {
        for capture in query_match.captures {
            if capture.index == extends_index {
                let extends_name = &source[capture.node.byte_range()];
                // Add to extends Vec using SourceReference
                if let Ok(source_ref) = SourceReference::builder()
                    .target(extends_name.to_string())
                    .simple_name(extends_name.to_string())
                    .build()
                {
                    relationships.extends.push(source_ref);
                }
                break;
            }
        }
    }

    let metadata = EntityMetadata::default();

    let entity = build_entity(
        components,
        EntityDetails {
            entity_type: EntityType::Class,
            language: Language::JavaScript,
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
