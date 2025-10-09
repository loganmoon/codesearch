//! Handler for extracting Rust function definitions
//!
//! This module processes tree-sitter query matches for Rust functions
//! and builds EntityData instances.

#![deny(warnings)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]

use crate::qualified_name::build_qualified_name_from_ast;
use crate::rust::handlers::common::{
    extract_function_modifiers, extract_function_parameters, extract_generics_from_node,
    extract_preceding_doc_comments, extract_visibility, find_capture_node, node_to_text,
};
use crate::rust::handlers::constants::{capture_names, special_idents};
use codesearch_core::entities::{
    CodeEntityBuilder, EntityMetadata, EntityType, FunctionSignature, Language, SourceLocation,
    Visibility,
};
use codesearch_core::entity_id::generate_entity_id;
use codesearch_core::error::{Error, Result};
use codesearch_core::CodeEntity;
use std::path::Path;
use tree_sitter::{Query, QueryMatch};

/// Process a function query match and extract entity data
pub fn handle_function(
    query_match: &QueryMatch,
    query: &Query,
    source: &str,
    file_path: &Path,
    repository_id: &str,
) -> Result<Vec<CodeEntity>> {
    // Extract function name
    let name = find_capture_node(query_match, query, capture_names::NAME)
        .and_then(|node| node_to_text(node, source).ok())
        .unwrap_or_else(|| special_idents::ANONYMOUS.to_string());

    // Get the function node for location and content
    let function_node = find_capture_node(query_match, query, capture_names::FUNCTION)
        .ok_or_else(|| Error::entity_extraction("Function node not found"))?;

    // Build qualified name via parent traversal
    let parent_scope = build_qualified_name_from_ast(function_node, source, "rust");
    let qualified_name = if parent_scope.is_empty() {
        name.clone()
    } else {
        format!("{parent_scope}::{name}")
    };

    // Generate entity_id from repository + file_path + qualified name
    let file_path_str = file_path
        .to_str()
        .ok_or_else(|| Error::entity_extraction("Invalid file path"))?;
    let entity_id = generate_entity_id(repository_id, file_path_str, &qualified_name);

    // Extract visibility by checking AST structure
    let visibility = extract_visibility(query_match, query);

    // Extract and parse modifiers
    let (is_async, is_unsafe, is_const) =
        find_capture_node(query_match, query, capture_names::MODIFIERS)
            .map(extract_function_modifiers)
            .unwrap_or((false, false, false));

    // Extract generics
    let generics = find_capture_node(query_match, query, capture_names::GENERICS)
        .map(|node| extract_generics_from_node(node, source))
        .unwrap_or_default();

    // Extract parameters
    let parameters = find_capture_node(query_match, query, capture_names::PARAMS)
        .map(|params_node| extract_function_parameters(params_node, source))
        .transpose()?
        .unwrap_or_default();

    // Extract return type
    let return_type = find_capture_node(query_match, query, capture_names::RETURN)
        .and_then(|node| node_to_text(node, source).ok());

    let location = SourceLocation::from_tree_sitter_node(function_node);
    let content = node_to_text(function_node, source)?.to_string();

    // Extract doc comments if any
    let documentation = extract_preceding_doc_comments(function_node, source);

    // Build the entity using a dedicated function
    let entity = build_function_entity(FunctionEntityComponents {
        entity_id,
        repository_id: repository_id.to_string(),
        name,
        qualified_name,
        parent_scope: if parent_scope.is_empty() {
            None
        } else {
            Some(parent_scope)
        },
        file_path: file_path.to_path_buf(),
        location,
        visibility,
        is_async,
        is_unsafe,
        is_const,
        generics,
        parameters,
        return_type,
        documentation,
        content: Some(content),
    })?;

    Ok(vec![entity])
}

/// Components for building a function entity
struct FunctionEntityComponents {
    entity_id: String,
    repository_id: String,
    name: String,
    qualified_name: String,
    parent_scope: Option<String>,
    file_path: std::path::PathBuf,
    location: SourceLocation,
    visibility: Visibility,
    is_async: bool,
    is_unsafe: bool,
    is_const: bool,
    generics: Vec<String>,
    parameters: Vec<(String, String)>,
    return_type: Option<String>,
    documentation: Option<String>,
    content: Option<String>,
}

/// Build a function entity from extracted components
fn build_function_entity(components: FunctionEntityComponents) -> Result<CodeEntity> {
    let mut metadata = EntityMetadata {
        is_async: components.is_async,
        is_const: components.is_const,
        generic_params: components.generics.clone(),
        is_generic: !components.generics.is_empty(),
        ..Default::default()
    };

    // Add unsafe as an attribute if applicable
    if components.is_unsafe {
        metadata
            .attributes
            .insert("unsafe".to_string(), "true".to_string());
    }

    let signature = FunctionSignature {
        parameters: components
            .parameters
            .iter()
            .map(|(name, ty)| (name.clone(), Some(ty.clone())))
            .collect(),
        return_type: components.return_type.clone(),
        is_async: components.is_async,
        generics: components.generics.clone(),
    };

    CodeEntityBuilder::default()
        .entity_id(components.entity_id)
        .repository_id(components.repository_id)
        .name(components.name)
        .qualified_name(components.qualified_name)
        .parent_scope(components.parent_scope)
        .entity_type(EntityType::Function)
        .location(components.location.clone())
        .visibility(components.visibility)
        .documentation_summary(components.documentation)
        .content(components.content)
        .metadata(metadata)
        .signature(Some(signature))
        .language(Language::Rust)
        .file_path(components.file_path)
        .build()
        .map_err(|e| Error::entity_extraction(format!("Failed to build CodeEntity: {e}")))
}
