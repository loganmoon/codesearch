//! Handler for extracting Rust extern blocks
//!
//! This module processes tree-sitter query matches for Rust extern blocks
//! and their contents (foreign functions and statics).

#![deny(warnings)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]

use crate::common::entity_building::{extract_common_components, ExtractionContext};
use crate::rust::handler_impls::common::{
    extract_function_parameters, extract_preceding_doc_comments, extract_visibility_from_node,
    find_capture_node, find_child_by_kind, node_to_text, require_capture_node,
};
use codesearch_core::entities::{
    EntityMetadata, EntityType, FunctionSignature, Language, SourceLocation, Visibility,
};
use codesearch_core::entity_id::generate_entity_id;
use codesearch_core::error::Result;
use codesearch_core::CodeEntity;
use std::path::Path;
use tree_sitter::{Node, Query, QueryMatch};

/// Process an extern block query match and extract entity data
#[allow(clippy::too_many_arguments)]
pub fn handle_extern_block_impl(
    query_match: &QueryMatch,
    query: &Query,
    source: &str,
    file_path: &Path,
    repository_id: &str,
    package_name: Option<&str>,
    source_root: Option<&Path>,
    repo_root: &Path,
) -> Result<Vec<CodeEntity>> {
    // Get the extern_block node
    let extern_block_node = require_capture_node(query_match, query, "extern_block")?;

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

    // Extract common components for the extern block
    let components = extract_common_components(&ctx, "abi", extern_block_node, "rust")?;

    // Extract the ABI string (e.g., "C", "system", etc.)
    let abi = find_capture_node(query_match, query, "abi")
        .and_then(|node| node_to_text(node, source).ok())
        .map(|s| s.trim_matches('"').to_string())
        .unwrap_or_else(|| "C".to_string());

    // Build qualified name for the extern block
    let extern_block_qualified_name = if let Some(parent_scope) = &components.parent_scope {
        format!("{parent_scope}::extern \"{abi}\"")
    } else if let Some(pkg) = package_name {
        format!("{pkg}::extern \"{abi}\"")
    } else {
        format!("extern \"{abi}\"")
    };

    // Extract content
    let content = node_to_text(extern_block_node, source).ok();

    // Build metadata
    let mut metadata = EntityMetadata::default();
    metadata.attributes.insert("abi".to_string(), abi.clone());

    // Generate entity_id for the extern block
    let file_path_str = file_path.to_str().unwrap_or("");
    let entity_id = generate_entity_id(repository_id, file_path_str, &extern_block_qualified_name);

    // Build the extern block entity
    let extern_block_entity = CodeEntity {
        entity_id,
        repository_id: repository_id.to_string(),
        entity_type: EntityType::ExternBlock,
        name: format!("extern \"{abi}\""),
        qualified_name: extern_block_qualified_name.clone(),
        path_entity_identifier: None,
        parent_scope: components.parent_scope.clone(),
        dependencies: Vec::new(),
        documentation_summary: None,
        file_path: file_path.to_path_buf(),
        language: Language::Rust,
        content,
        metadata,
        signature: None,
        visibility: None, // Extern blocks don't have visibility themselves
        location: SourceLocation::from_tree_sitter_node(extern_block_node),
        relationships: Default::default(),
    };

    let mut entities = vec![extern_block_entity];

    // Extract items inside the extern block
    if let Some(body_node) = find_capture_node(query_match, query, "extern_body") {
        let extern_items = extract_extern_items(
            body_node,
            source,
            file_path,
            repository_id,
            package_name,
            &extern_block_qualified_name,
        );
        entities.extend(extern_items);
    }

    Ok(entities)
}

/// Extract function and static declarations from an extern block
fn extract_extern_items(
    body_node: Node,
    source: &str,
    file_path: &Path,
    repository_id: &str,
    package_name: Option<&str>,
    extern_block_qualified_name: &str,
) -> Vec<CodeEntity> {
    let mut entities = Vec::new();
    let mut cursor = body_node.walk();

    for child in body_node.children(&mut cursor) {
        match child.kind() {
            "function_signature_item" => {
                if let Some(entity) = extract_extern_function(
                    child,
                    source,
                    file_path,
                    repository_id,
                    package_name,
                    extern_block_qualified_name,
                ) {
                    entities.push(entity);
                }
            }
            "static_item" | "foreign_static" => {
                if let Some(entity) = extract_extern_static(
                    child,
                    source,
                    file_path,
                    repository_id,
                    package_name,
                    extern_block_qualified_name,
                ) {
                    entities.push(entity);
                }
            }
            _ => {}
        }
    }

    entities
}

/// Extract a function declaration from an extern block
fn extract_extern_function(
    node: Node,
    source: &str,
    file_path: &Path,
    repository_id: &str,
    package_name: Option<&str>,
    extern_block_qualified_name: &str,
) -> Option<CodeEntity> {
    // Find the function name
    let name = find_child_by_kind(node, "identifier").and_then(|n| node_to_text(n, source).ok())?;

    // Build qualified name - function goes in the module, not the extern block
    // But parent_scope is the extern block for CONTAINS relationship
    let qualified_name = if let Some(pkg) = package_name {
        format!("{pkg}::{name}")
    } else {
        name.clone()
    };

    // Extract visibility
    let visibility = find_child_by_kind(node, "visibility_modifier")
        .map(extract_visibility_from_node)
        .unwrap_or(Visibility::Private);

    // Extract parameters
    let parameters: Vec<(String, Option<String>)> = find_child_by_kind(node, "parameters")
        .and_then(|params_node| extract_function_parameters(params_node, source).ok())
        .unwrap_or_default()
        .into_iter()
        .map(|(name, type_str)| (name, Some(type_str)))
        .collect();

    // Extract return type
    let return_type = find_child_by_kind(node, "return_type")
        .and_then(|n| node_to_text(n, source).ok())
        .map(|s| s.trim_start_matches("->").trim().to_string());

    // Build signature
    let signature = FunctionSignature {
        parameters,
        return_type,
        is_async: false,
        generics: Vec::new(),
    };

    // Build metadata
    let mut metadata = EntityMetadata::default();
    metadata
        .attributes
        .insert("extern".to_string(), "true".to_string());

    // Extract documentation
    let documentation_summary = extract_preceding_doc_comments(node, source);

    // Get location and content
    let location = SourceLocation::from_tree_sitter_node(node);
    let content = node_to_text(node, source).ok();

    // Generate entity_id
    let file_path_str = file_path.to_str()?;
    let entity_id = generate_entity_id(repository_id, file_path_str, &qualified_name);

    Some(CodeEntity {
        entity_id,
        repository_id: repository_id.to_string(),
        entity_type: EntityType::Function,
        name,
        qualified_name,
        path_entity_identifier: None,
        parent_scope: Some(extern_block_qualified_name.to_string()),
        dependencies: Vec::new(),
        documentation_summary,
        file_path: file_path.to_path_buf(),
        language: Language::Rust,
        content,
        metadata,
        signature: Some(signature),
        visibility: Some(visibility),
        location,
        relationships: Default::default(),
    })
}

/// Extract a static declaration from an extern block
fn extract_extern_static(
    node: Node,
    source: &str,
    file_path: &Path,
    repository_id: &str,
    package_name: Option<&str>,
    extern_block_qualified_name: &str,
) -> Option<CodeEntity> {
    // Find the static name
    let name = find_child_by_kind(node, "identifier").and_then(|n| node_to_text(n, source).ok())?;

    // Build qualified name - static goes in the module, not the extern block
    // But parent_scope is the extern block for CONTAINS relationship
    let qualified_name = if let Some(pkg) = package_name {
        format!("{pkg}::{name}")
    } else {
        name.clone()
    };

    // Extract visibility
    let visibility = find_child_by_kind(node, "visibility_modifier")
        .map(extract_visibility_from_node)
        .unwrap_or(Visibility::Private);

    // Check for mutable_specifier
    let is_mut = find_child_by_kind(node, "mutable_specifier").is_some();

    // Extract type
    let static_type = find_child_by_kind(node, "type").and_then(|n| node_to_text(n, source).ok());

    // Build metadata
    let mut metadata = EntityMetadata {
        is_static: true,
        ..Default::default()
    };
    metadata
        .attributes
        .insert("extern".to_string(), "true".to_string());

    if let Some(ty) = static_type {
        metadata.attributes.insert("type".to_string(), ty);
    }

    if is_mut {
        metadata
            .attributes
            .insert("mutable".to_string(), "true".to_string());
    }

    // Extract documentation
    let documentation_summary = extract_preceding_doc_comments(node, source);

    // Get location and content
    let location = SourceLocation::from_tree_sitter_node(node);
    let content = node_to_text(node, source).ok();

    // Generate entity_id
    let file_path_str = file_path.to_str()?;
    let entity_id = generate_entity_id(repository_id, file_path_str, &qualified_name);

    Some(CodeEntity {
        entity_id,
        repository_id: repository_id.to_string(),
        entity_type: EntityType::Static,
        name,
        qualified_name,
        path_entity_identifier: None,
        parent_scope: Some(extern_block_qualified_name.to_string()),
        dependencies: Vec::new(),
        documentation_summary,
        file_path: file_path.to_path_buf(),
        language: Language::Rust,
        content,
        metadata,
        signature: None,
        visibility: Some(visibility),
        location,
        relationships: Default::default(),
    })
}
