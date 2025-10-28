//! JavaScript class handler implementations

use crate::common::{
    find_capture_node,
    js_ts_common::{extract_jsdoc_comments, extract_parameters},
    node_to_text, require_capture_node,
};
use codesearch_core::{
    entities::{
        CodeEntityBuilder, EntityMetadata, EntityType, FunctionSignature, Language, SourceLocation,
        Visibility,
    },
    entity_id::generate_entity_id,
    error::Result,
    CodeEntity,
};
use std::path::Path;
use tree_sitter::{Query, QueryMatch};

/// Handle class declarations
pub fn handle_class_impl(
    query_match: &QueryMatch,
    query: &Query,
    source: &str,
    file_path: &Path,
    repository_id: &str,
) -> Result<Vec<CodeEntity>> {
    let class_node = require_capture_node(query_match, query, "class")?;

    // Extract name
    let name_node = require_capture_node(query_match, query, "name")?;
    let name = node_to_text(name_node, source)?;

    // Build qualified name
    let qualified_name =
        crate::qualified_name::build_qualified_name_from_ast(class_node, source, "javascript");
    let full_qualified_name = if qualified_name.is_empty() {
        name.clone()
    } else {
        format!("{qualified_name}.{name}")
    };

    // Extract extends clause if present
    let extends = if let Some(extends_node) = find_capture_node(query_match, query, "extends") {
        node_to_text(extends_node, source).ok()
    } else {
        None
    };

    // Extract JSDoc documentation
    let documentation = extract_jsdoc_comments(class_node, source);

    // Build metadata
    let mut metadata = EntityMetadata::default();
    if let Some(extends_text) = extends {
        metadata
            .attributes
            .insert("extends".to_string(), extends_text);
    }

    // Generate entity_id
    let file_path_str = file_path.to_str().unwrap_or_default();
    let entity_id = generate_entity_id(repository_id, file_path_str, &full_qualified_name);

    // Build entity
    let entity = CodeEntityBuilder::default()
        .entity_id(entity_id)
        .repository_id(repository_id.to_string())
        .name(name)
        .qualified_name(full_qualified_name.clone())
        .parent_scope(if qualified_name.is_empty() {
            None
        } else {
            Some(qualified_name)
        })
        .entity_type(EntityType::Class)
        .location(SourceLocation::from_tree_sitter_node(class_node))
        .visibility(Visibility::Public)
        .documentation_summary(documentation)
        .content(node_to_text(class_node, source).ok())
        .metadata(metadata)
        .language(Language::JavaScript)
        .file_path(file_path.to_path_buf())
        .build()
        .map_err(|e| {
            codesearch_core::error::Error::entity_extraction(format!(
                "Failed to build CodeEntity: {e}"
            ))
        })?;

    Ok(vec![entity])
}

/// Handle class methods
pub fn handle_method_impl(
    query_match: &QueryMatch,
    query: &Query,
    source: &str,
    file_path: &Path,
    repository_id: &str,
) -> Result<Vec<CodeEntity>> {
    let method_node = require_capture_node(query_match, query, "method")?;

    // Extract name
    let name_node = require_capture_node(query_match, query, "name")?;
    let name = node_to_text(name_node, source)?;

    // Build qualified name (methods need to find their parent class)
    let qualified_name =
        crate::qualified_name::build_qualified_name_from_ast(method_node, source, "javascript");
    let full_qualified_name = if qualified_name.is_empty() {
        name.clone()
    } else {
        format!("{qualified_name}.{name}")
    };

    // Extract parameters
    let parameters = if let Some(params_node) = find_capture_node(query_match, query, "params") {
        extract_parameters(params_node, source)?
    } else {
        Vec::new()
    };

    // Check for static and async modifiers
    let mut is_static = false;
    let mut is_async = false;

    for child in method_node.children(&mut method_node.walk()) {
        match child.kind() {
            "static" => is_static = true,
            "async" => is_async = true,
            _ => {}
        }
    }

    // Extract JSDoc documentation
    let documentation = extract_jsdoc_comments(method_node, source);

    // Build metadata
    let mut metadata = EntityMetadata {
        is_async,
        ..EntityMetadata::default()
    };
    if is_static {
        metadata
            .attributes
            .insert("static".to_string(), "true".to_string());
    }

    // Build signature
    let signature = FunctionSignature {
        parameters,
        return_type: None,
        generics: Vec::new(),
        is_async,
    };

    // Generate entity_id
    let file_path_str = file_path.to_str().unwrap_or_default();
    let entity_id = generate_entity_id(repository_id, file_path_str, &full_qualified_name);

    // Build entity
    let entity = CodeEntityBuilder::default()
        .entity_id(entity_id)
        .repository_id(repository_id.to_string())
        .name(name)
        .qualified_name(full_qualified_name.clone())
        .parent_scope(if qualified_name.is_empty() {
            None
        } else {
            Some(qualified_name)
        })
        .entity_type(EntityType::Method)
        .location(SourceLocation::from_tree_sitter_node(method_node))
        .visibility(Visibility::Public)
        .documentation_summary(documentation)
        .content(node_to_text(method_node, source).ok())
        .metadata(metadata)
        .signature(Some(signature))
        .language(Language::JavaScript)
        .file_path(file_path.to_path_buf())
        .build()
        .map_err(|e| {
            codesearch_core::error::Error::entity_extraction(format!(
                "Failed to build CodeEntity: {e}"
            ))
        })?;

    Ok(vec![entity])
}
