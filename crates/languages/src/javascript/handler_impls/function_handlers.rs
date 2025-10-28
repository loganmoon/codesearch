//! JavaScript function handler implementations

use crate::common::{find_capture_node, node_to_text, require_capture_node};
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
use tree_sitter::{Node, Query, QueryMatch};

/// Handle regular function declarations
pub fn handle_function_impl(
    query_match: &QueryMatch,
    query: &Query,
    source: &str,
    file_path: &Path,
    repository_id: &str,
) -> Result<Vec<CodeEntity>> {
    let function_node = require_capture_node(query_match, query, "function")?;

    // Extract name
    let name_node = require_capture_node(query_match, query, "name")?;
    let name = node_to_text(name_node, source)?;

    // Build qualified name (JavaScript uses "." separator)
    let qualified_name =
        crate::qualified_name::build_qualified_name_from_ast(function_node, source, "javascript");
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

    // Check for async modifier
    let is_async = function_node
        .children(&mut function_node.walk())
        .any(|child| child.kind() == "async");

    // Extract JSDoc documentation
    let documentation = extract_jsdoc_comments(function_node, source);

    // Build metadata
    let metadata = EntityMetadata {
        is_async,
        ..EntityMetadata::default()
    };

    // Build signature
    let signature = FunctionSignature {
        parameters,
        return_type: None, // JavaScript has no type annotations
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
        .entity_type(EntityType::Function)
        .location(SourceLocation::from_tree_sitter_node(function_node))
        .visibility(Visibility::Public)
        .documentation_summary(documentation)
        .content(node_to_text(function_node, source).ok())
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

/// Handle arrow functions assigned to variables
pub fn handle_arrow_function_impl(
    query_match: &QueryMatch,
    query: &Query,
    source: &str,
    file_path: &Path,
    repository_id: &str,
) -> Result<Vec<CodeEntity>> {
    let arrow_function_node = require_capture_node(query_match, query, "arrow_function")?;

    // Extract name from parent variable_declarator
    let name = extract_arrow_function_name(arrow_function_node, source)?;

    // Build qualified name
    let qualified_name = crate::qualified_name::build_qualified_name_from_ast(
        arrow_function_node,
        source,
        "javascript",
    );
    let full_qualified_name = if qualified_name.is_empty() {
        name.clone()
    } else {
        format!("{qualified_name}.{name}")
    };

    // Extract parameters from the arrow function node
    let parameters = extract_arrow_function_parameters(arrow_function_node, source)?;

    // Check for async modifier
    let is_async = arrow_function_node
        .children(&mut arrow_function_node.walk())
        .any(|child| child.kind() == "async");

    // Extract JSDoc documentation
    let documentation = extract_jsdoc_comments(arrow_function_node, source);

    // Build metadata
    let metadata = EntityMetadata {
        is_async,
        ..EntityMetadata::default()
    };

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
        .entity_type(EntityType::Function)
        .location(SourceLocation::from_tree_sitter_node(arrow_function_node))
        .visibility(Visibility::Public)
        .documentation_summary(documentation)
        .content(node_to_text(arrow_function_node, source).ok())
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

/// Extract parameters from a formal_parameters node
fn extract_parameters(params_node: Node, source: &str) -> Result<Vec<(String, Option<String>)>> {
    let mut parameters = Vec::new();

    for child in params_node.named_children(&mut params_node.walk()) {
        match child.kind() {
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
            "rest_pattern" => {
                // Handle rest parameters (...args)
                if let Some(name_node) = child.named_child(0) {
                    let param_name = format!("...{}", node_to_text(name_node, source)?);
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

/// Extract parameters from an arrow function node
fn extract_arrow_function_parameters(
    arrow_function_node: Node,
    source: &str,
) -> Result<Vec<(String, Option<String>)>> {
    // Find the parameter part of the arrow function
    // It can be: identifier, formal_parameters, or missing (for () => syntax)
    for child in arrow_function_node.named_children(&mut arrow_function_node.walk()) {
        match child.kind() {
            "identifier" => {
                // Single parameter without parentheses: x => x * 2
                let param_name = node_to_text(child, source)?;
                return Ok(vec![(param_name, None)]);
            }
            "formal_parameters" => {
                // Parameters with parentheses: (a, b) => a + b
                return extract_parameters(child, source);
            }
            _ => {}
        }
    }

    // No parameters found (e.g., () => ...)
    Ok(Vec::new())
}

/// Extract name from arrow function by finding parent variable_declarator
fn extract_arrow_function_name(arrow_function_node: Node, source: &str) -> Result<String> {
    // Walk up to find variable_declarator
    let mut current = arrow_function_node.parent();
    while let Some(node) = current {
        if node.kind() == "variable_declarator" {
            // Find the name child (identifier)
            for child in node.named_children(&mut node.walk()) {
                if child.kind() == "identifier" {
                    return node_to_text(child, source);
                }
            }
        }
        current = node.parent();
    }

    Err(codesearch_core::error::Error::entity_extraction(
        "Could not find variable name for arrow function".to_string(),
    ))
}

/// Extract JSDoc comments preceding a node
fn extract_jsdoc_comments(node: Node, source: &str) -> Option<String> {
    let mut doc_lines = Vec::new();
    let mut current = node.prev_sibling();

    while let Some(sibling) = current {
        if sibling.kind() == "comment" {
            if let Ok(text) = node_to_text(sibling, source) {
                if text.starts_with("/**") && text.ends_with("*/") {
                    // Extract JSDoc content
                    let content = text
                        .trim_start_matches("/**")
                        .trim_end_matches("*/")
                        .lines()
                        .map(|line| line.trim().trim_start_matches('*').trim())
                        .filter(|line| !line.is_empty())
                        .collect::<Vec<_>>()
                        .join("\n");
                    doc_lines.push(content);
                    break;
                }
            }
        } else if sibling.kind() != "expression_statement" {
            break;
        }
        current = sibling.prev_sibling();
    }

    if doc_lines.is_empty() {
        None
    } else {
        Some(doc_lines.join("\n"))
    }
}
