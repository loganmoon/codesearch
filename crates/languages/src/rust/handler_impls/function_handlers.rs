//! Handler for extracting Rust function definitions
//!
//! This module processes tree-sitter query matches for Rust functions
//! and builds EntityData instances.

#![deny(warnings)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]

use crate::common::entity_building::{
    build_entity, extract_common_components, EntityDetails, ExtractionContext,
};
use crate::common::import_map::{parse_file_imports, ImportMap};
use crate::rust::handler_impls::common::{
    extract_function_calls, extract_function_modifiers, extract_function_parameters,
    extract_generics_from_node, extract_local_var_types, extract_preceding_doc_comments,
    extract_type_references, extract_visibility, find_capture_node, node_to_text,
    require_capture_node,
};
use crate::rust::handler_impls::constants::capture_names;
use codesearch_core::entities::{EntityMetadata, EntityType, FunctionSignature, Language};
use codesearch_core::error::Result;
use codesearch_core::CodeEntity;
use std::path::Path;
use tree_sitter::{Node, Query, QueryMatch};

/// Process a function query match and extract entity data
pub fn handle_function_impl(
    query_match: &QueryMatch,
    query: &Query,
    source: &str,
    file_path: &Path,
    repository_id: &str,
) -> Result<Vec<CodeEntity>> {
    // Get the function node for location and content
    let function_node = require_capture_node(query_match, query, capture_names::FUNCTION)?;

    // Skip functions inside impl blocks - those are handled by the impl extractor
    if let Some(parent) = function_node.parent() {
        if parent.kind() == "declaration_list" {
            // Check if the declaration_list is inside an impl_item
            if let Some(grandparent) = parent.parent() {
                if grandparent.kind() == "impl_item" {
                    return Ok(Vec::new());
                }
            }
        }
    }

    // Create extraction context
    let ctx = ExtractionContext {
        query_match,
        query,
        source,
        file_path,
        repository_id,
    };

    // Extract common components
    let components = extract_common_components(&ctx, capture_names::NAME, function_node, "rust")?;

    // Extract Rust-specific: visibility, documentation, content
    let visibility = extract_visibility(query_match, query);
    let documentation = extract_preceding_doc_comments(function_node, source);
    let content = node_to_text(function_node, source).ok();

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

    // Build ImportMap from file's imports for qualified name resolution
    let import_map = get_file_import_map(function_node, source);

    // Extract local variable types for method call resolution
    let local_vars = extract_local_var_types(function_node, source);

    // Extract function calls from the function body with qualified name resolution
    let calls = extract_function_calls(
        function_node,
        source,
        &import_map,
        components.parent_scope.as_deref(),
        &local_vars,
    );

    // Extract type references for USES relationships
    let type_refs = extract_type_references(
        function_node,
        source,
        &import_map,
        components.parent_scope.as_deref(),
    );

    // Build metadata
    let mut metadata = EntityMetadata {
        is_async,
        is_const,
        generic_params: generics.clone(),
        is_generic: !generics.is_empty(),
        ..Default::default()
    };

    // Add unsafe as an attribute if applicable
    if is_unsafe {
        metadata
            .attributes
            .insert("unsafe".to_string(), "true".to_string());
    }

    // Store function calls if any exist
    if !calls.is_empty() {
        if let Ok(json) = serde_json::to_string(&calls) {
            metadata.attributes.insert("calls".to_string(), json);
        }
    }

    // Store type references for USES relationships
    if !type_refs.is_empty() {
        if let Ok(json) = serde_json::to_string(&type_refs) {
            metadata.attributes.insert("uses_types".to_string(), json);
        }
    }

    // Build the entity using the shared helper
    let entity = build_entity(
        components,
        EntityDetails {
            entity_type: EntityType::Function,
            language: Language::Rust,
            visibility,
            documentation,
            content,
            metadata,
            signature: Some(FunctionSignature {
                parameters: parameters
                    .iter()
                    .map(|(name, ty)| (name.clone(), Some(ty.clone())))
                    .collect(),
                return_type,
                is_async,
                generics,
            }),
        },
    )?;

    Ok(vec![entity])
}

/// Get the ImportMap for a file by walking up to the AST root
fn get_file_import_map(node: Node, source: &str) -> ImportMap {
    // Walk up to the root node
    let mut current = node;
    while let Some(parent) = current.parent() {
        current = parent;
    }

    // Parse imports from the root
    parse_file_imports(current, source, Language::Rust)
}
