//! Handler for extracting Rust function definitions
//!
//! This module processes tree-sitter query matches for Rust functions
//! and builds EntityData instances.

#![deny(warnings)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]

use crate::rust::handlers::common::{
    build_entity, extract_common_components, extract_function_modifiers,
    extract_function_parameters, extract_generics_from_node, find_capture_node, node_to_text,
    require_capture_node,
};
use crate::rust::handlers::constants::capture_names;
use codesearch_core::entities::{EntityMetadata, EntityType, FunctionSignature};
use codesearch_core::error::Result;
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
    // Get the function node for location and content
    let function_node = require_capture_node(query_match, query, capture_names::FUNCTION)?;

    // Extract common components
    let components = extract_common_components(
        query_match,
        query,
        source,
        file_path,
        repository_id,
        capture_names::NAME,
        function_node,
    )?;

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

    // Build signature
    let signature = FunctionSignature {
        parameters: parameters
            .iter()
            .map(|(name, ty)| (name.clone(), Some(ty.clone())))
            .collect(),
        return_type: return_type.clone(),
        is_async,
        generics: generics.clone(),
    };

    // Build the entity using the common helper
    let entity = build_entity(components, EntityType::Function, metadata, Some(signature))?;

    Ok(vec![entity])
}
