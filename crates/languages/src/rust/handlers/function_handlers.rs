//! Handler for extracting Rust function definitions
//!
//! This module processes tree-sitter query matches for Rust functions
//! and builds EntityData instances.

#![warn(warnings)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]

use crate::rust::handlers::common::{
    extract_generics_from_node, extract_preceding_doc_comments, extract_visibility,
    find_capture_node, node_to_text,
};
use crate::rust::handlers::constants::{
    capture_names, function_modifiers, keywords, node_kinds, punctuation, special_idents,
};
use codesearch_core::entities::{
    CodeEntityBuilder, EntityMetadata, EntityType, FunctionSignature, Language, SourceLocation,
    Visibility,
};
use codesearch_core::entity_id::ScopeContext;
use codesearch_core::error::{Error, Result};
use codesearch_core::CodeEntity;
use std::path::Path;
use tree_sitter::{Query, QueryMatch};

/// Process a function query match and extract entity data
#[allow(dead_code)]
pub fn handle_function(
    query_match: &QueryMatch,
    query: &Query,
    source: &str,
    file_path: &Path,
) -> Result<Vec<CodeEntity>> {
    let scope_context = ScopeContext::new();

    // Extract function name
    let name = find_capture_node(query_match, query, capture_names::NAME)
        .and_then(|node| node_to_text(node, source).ok())
        .unwrap_or_else(|| special_idents::ANONYMOUS.to_string());

    // Extract visibility by checking AST structure
    let visibility = extract_visibility(query_match, query);

    // Extract and parse modifiers
    let (is_async, is_unsafe, is_const) = extract_function_modifiers(query_match, query);

    // Extract generics
    let generics = find_capture_node(query_match, query, capture_names::GENERICS)
        .map(|node| extract_generics_from_node(node, source))
        .unwrap_or_default();

    // Extract parameters
    let parameters = extract_parameters(query_match, query, source)?;

    // Extract return type
    let return_type = find_capture_node(query_match, query, capture_names::RETURN)
        .and_then(|node| node_to_text(node, source).ok());

    // Get the function node for location and content
    let function_node = find_capture_node(query_match, query, capture_names::FUNCTION)
        .ok_or_else(|| Error::entity_extraction("Function node not found"))?;

    let location = SourceLocation::from_tree_sitter_node(function_node);
    let content = node_to_text(function_node, source)?.to_string();

    // Extract doc comments if any
    let documentation = extract_preceding_doc_comments(function_node, source);

    // Build qualified name
    let qualified_name = scope_context.build_qualified_name(&name);

    // Build the entity using a dedicated function
    let entity = build_function_entity(FunctionEntityComponents {
        name,
        qualified_name,
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

/// Extract function parameters by walking the AST
#[allow(dead_code)]
fn extract_parameters(
    query_match: &QueryMatch,
    query: &Query,
    source: &str,
) -> Result<Vec<(String, String)>> {
    let Some(params_node) = find_capture_node(query_match, query, capture_names::PARAMS) else {
        return Ok(Vec::new());
    };

    let mut parameters = Vec::new();
    let mut cursor = params_node.walk();

    // Walk through children to find parameter nodes
    for child in params_node.children(&mut cursor) {
        // Skip punctuation like parentheses and commas
        if matches!(
            child.kind(),
            punctuation::OPEN_PAREN | punctuation::CLOSE_PAREN | punctuation::COMMA
        ) {
            continue;
        }

        // Handle different parameter types
        match child.kind() {
            node_kinds::PARAMETER => {
                // Extract the full parameter as a unit
                if let Some((pattern, param_type)) = extract_parameter_parts(child, source)? {
                    parameters.push((pattern, param_type));
                }
            }
            node_kinds::SELF_PARAMETER => {
                // Handle self, &self, &mut self
                let text = node_to_text(child, source)?;
                parameters.push((keywords::SELF.to_string(), text));
            }
            node_kinds::VARIADIC_PARAMETER => {
                // Handle ... parameters (for extern functions)
                let text = node_to_text(child, source)?;
                parameters.push((special_idents::VARIADIC.to_string(), text));
            }
            _ => {}
        }
    }

    Ok(parameters)
}

/// Extract pattern and type parts from a parameter node
#[allow(dead_code)]
fn extract_parameter_parts(
    node: tree_sitter::Node,
    source: &str,
) -> Result<Option<(String, String)>> {
    let full_text = node_to_text(node, source)?;

    // Simple split on colon for most cases
    if let Some(colon_pos) = full_text.find(':') {
        let pattern = full_text[..colon_pos].trim().to_string();
        let param_type = full_text[colon_pos + 1..].trim().to_string();
        return Ok(Some((pattern, param_type)));
    }

    // No colon means no type annotation (rare in Rust)
    if !full_text.trim().is_empty() {
        Ok(Some((full_text, String::new())))
    } else {
        Ok(None)
    }
}

// ===== Helper Functions =====

/// Extract function modifiers (async, unsafe, const)
#[allow(dead_code)]
fn extract_function_modifiers(query_match: &QueryMatch, query: &Query) -> (bool, bool, bool) {
    let Some(modifiers_node) = find_capture_node(query_match, query, capture_names::MODIFIERS)
    else {
        return (false, false, false);
    };

    let mut has_async = false;
    let mut has_unsafe = false;
    let mut has_const = false;
    let mut cursor = modifiers_node.walk();

    for child in modifiers_node.children(&mut cursor) {
        match child.kind() {
            function_modifiers::ASYNC => has_async = true,
            function_modifiers::UNSAFE => has_unsafe = true,
            function_modifiers::CONST => has_const = true,
            _ => {}
        }
    }

    (has_async, has_unsafe, has_const)
}

/// Components for building a function entity
#[allow(dead_code)]
struct FunctionEntityComponents {
    name: String,
    qualified_name: String,
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
#[allow(dead_code)]
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
        .entity_id(format!(
            "{}#{}",
            components.file_path.display(),
            components.qualified_name
        ))
        .name(components.name)
        .qualified_name(components.qualified_name)
        .entity_type(EntityType::Function)
        .location(components.location.clone())
        .visibility(components.visibility)
        .documentation_summary(components.documentation)
        .content(components.content)
        .metadata(metadata)
        .signature(Some(signature))
        .language(Language::Rust)
        .file_path(components.file_path)
        .line_range((components.location.start_line, components.location.end_line))
        .build()
        .map_err(|e| Error::entity_extraction(format!("Failed to build CodeEntity: {e}")))
}
