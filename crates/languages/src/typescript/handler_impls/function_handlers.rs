//! TypeScript function handler implementations

use crate::common::{find_capture_node, node_to_text};
use codesearch_core::{error::Result, CodeEntity};
use std::path::Path;
use tree_sitter::{Node, Query, QueryMatch};

/// Handle regular function declarations with TypeScript type annotations
pub fn handle_function_impl(
    query_match: &QueryMatch,
    query: &Query,
    source: &str,
    file_path: &Path,
    repository_id: &str,
) -> Result<Vec<CodeEntity>> {
    // Start with JavaScript extraction
    let mut entities = crate::javascript::handler_impls::handle_function_impl(
        query_match,
        query,
        source,
        file_path,
        repository_id,
    )?;

    // Enhance with TypeScript type information
    if let Some(entity) = entities.first_mut() {
        enhance_with_type_annotations(entity, query_match, query, source)?;
        // Update language to TypeScript
        entity.language = codesearch_core::entities::Language::TypeScript;
    }

    Ok(entities)
}

/// Handle arrow functions with TypeScript type annotations
pub fn handle_arrow_function_impl(
    query_match: &QueryMatch,
    query: &Query,
    source: &str,
    file_path: &Path,
    repository_id: &str,
) -> Result<Vec<CodeEntity>> {
    // Start with JavaScript extraction
    let mut entities = crate::javascript::handler_impls::handle_arrow_function_impl(
        query_match,
        query,
        source,
        file_path,
        repository_id,
    )?;

    // Enhance with TypeScript type information
    if let Some(entity) = entities.first_mut() {
        enhance_with_type_annotations(entity, query_match, query, source)?;
        // Update language to TypeScript
        entity.language = codesearch_core::entities::Language::TypeScript;
    }

    Ok(entities)
}

/// Enhance a function entity with TypeScript type annotations
fn enhance_with_type_annotations(
    entity: &mut CodeEntity,
    query_match: &QueryMatch,
    query: &Query,
    source: &str,
) -> Result<()> {
    if let Some(function_node) = find_capture_node(query_match, query, "function")
        .or_else(|| find_capture_node(query_match, query, "arrow_function"))
    {
        // Extract return type annotation
        let return_type = find_type_annotation(function_node, source)?;

        // Extract parameters with types
        let parameters = if let Some(params_node) = find_capture_node(query_match, query, "params")
        {
            extract_typescript_parameters(params_node, source)?
        } else {
            Vec::new()
        };

        // Check for async
        let is_async = entity
            .signature
            .as_ref()
            .map(|s| s.is_async)
            .unwrap_or(false);

        // Rebuild signature with TypeScript information
        entity.signature = Some(codesearch_core::entities::FunctionSignature {
            parameters,
            return_type,
            generics: Vec::new(),
            is_async,
        });
    }

    Ok(())
}

/// Find type annotation for a function (looks for return type after parameters)
fn find_type_annotation(function_node: Node, source: &str) -> Result<Option<String>> {
    for child in function_node.children(&mut function_node.walk()) {
        if child.kind() == "type_annotation" {
            return Ok(Some(node_to_text(child, source)?));
        }
    }
    Ok(None)
}

/// Extract parameters from TypeScript formal_parameters node
///
/// For TypeScript, parameters are typically `required_parameter` or `optional_parameter` nodes
/// which have their name and type as children.
fn extract_typescript_parameters(
    params_node: Node,
    source: &str,
) -> Result<Vec<(String, Option<String>)>> {
    let mut parameters = Vec::new();

    for child in params_node.named_children(&mut params_node.walk()) {
        match child.kind() {
            "required_parameter" | "optional_parameter" => {
                // Extract parameter name and type
                let name = if let Some(pattern) = child.child_by_field_name("pattern") {
                    node_to_text(pattern, source)?
                } else {
                    continue;
                };

                let type_annotation = if let Some(type_node) = child.child_by_field_name("type") {
                    Some(node_to_text(type_node, source)?)
                } else {
                    None
                };

                parameters.push((name, type_annotation));
            }
            "rest_pattern" => {
                // Handle rest parameters (...args)
                if let Some(name_node) = child.named_child(0) {
                    let param_name = format!("...{}", node_to_text(name_node, source)?);
                    parameters.push((param_name, None));
                }
            }
            // Fallback for simple identifiers (JavaScript-style parameters)
            "identifier" => {
                let param_name = node_to_text(child, source)?;
                parameters.push((param_name, None));
            }
            "assignment_pattern" => {
                // Handle default parameters
                if let Some(name_node) = child.child_by_field_name("left") {
                    let param_name = node_to_text(name_node, source)?;
                    parameters.push((param_name, None));
                }
            }
            "object_pattern" | "array_pattern" => {
                // Handle destructuring parameters
                let param_text = node_to_text(child, source)?;
                parameters.push((param_text, None));
            }
            _ => {}
        }
    }

    Ok(parameters)
}
