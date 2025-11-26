//! JavaScript function handler implementations

use crate::common::{
    entity_building::{build_entity, extract_common_components, EntityDetails, ExtractionContext},
    find_capture_node,
    js_ts_common::{extract_jsdoc_comments, extract_parameters},
    node_to_text, require_capture_node,
};
use codesearch_core::{
    entities::{EntityMetadata, EntityType, FunctionSignature, Language, Visibility},
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

    let ctx = ExtractionContext {
        query_match,
        query,
        source,
        file_path,
        repository_id,
    };

    // Extract common components (name, qualified_name, entity_id, location)
    let components = extract_common_components(&ctx, "name", function_node, "javascript")?;

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

    // Build entity using shared helper
    let entity = build_entity(
        components,
        EntityDetails {
            entity_type: EntityType::Function,
            language: Language::JavaScript,
            visibility: Visibility::Public,
            documentation,
            content: node_to_text(function_node, source).ok(),
            metadata: EntityMetadata {
                is_async,
                ..EntityMetadata::default()
            },
            signature: Some(FunctionSignature {
                parameters,
                return_type: None,
                generics: Vec::new(),
                is_async,
            }),
        },
    )?;

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
    use crate::common::entity_building::CommonEntityComponents;
    use codesearch_core::entities::SourceLocation;

    let arrow_function_node = require_capture_node(query_match, query, "arrow_function")?;

    // Arrow functions need special name extraction from parent context
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

    // Generate entity_id
    let file_path_str = file_path.to_str().unwrap_or_default();
    let entity_id = codesearch_core::entity_id::generate_entity_id(
        repository_id,
        file_path_str,
        &full_qualified_name,
    );

    // Build entity using shared helper (with manually constructed components)
    let components = CommonEntityComponents {
        entity_id,
        repository_id: repository_id.to_string(),
        name,
        qualified_name: full_qualified_name,
        parent_scope: if qualified_name.is_empty() {
            None
        } else {
            Some(qualified_name)
        },
        file_path: file_path.to_path_buf(),
        location: SourceLocation::from_tree_sitter_node(arrow_function_node),
    };

    let entity = build_entity(
        components,
        EntityDetails {
            entity_type: EntityType::Function,
            language: Language::JavaScript,
            visibility: Visibility::Public,
            documentation,
            content: node_to_text(arrow_function_node, source).ok(),
            metadata: EntityMetadata {
                is_async,
                ..EntityMetadata::default()
            },
            signature: Some(FunctionSignature {
                parameters,
                return_type: None,
                generics: Vec::new(),
                is_async,
            }),
        },
    )?;

    Ok(vec![entity])
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

/// Extract name from arrow function by finding parent context
fn extract_arrow_function_name(arrow_function_node: Node, source: &str) -> Result<String> {
    let mut current = arrow_function_node.parent();
    while let Some(node) = current {
        match node.kind() {
            // Handle variable declarations: const foo = () => {}
            "variable_declarator" => {
                for child in node.named_children(&mut node.walk()) {
                    if child.kind() == "identifier" {
                        return node_to_text(child, source);
                    }
                }
            }
            // Handle object properties: { handler: () => {} }
            "pair" => {
                if let Some(key) = node.child_by_field_name("key") {
                    return node_to_text(key, source);
                }
            }
            // Handle class fields: class Foo { handler = () => {} }
            "public_field_definition" | "field_definition" => {
                if let Some(property) = node.child_by_field_name("property") {
                    return node_to_text(property, source);
                }
            }
            // Handle export defaults: export default () => {}
            "export_statement" => {
                if let Some(declaration) = node.child_by_field_name("declaration") {
                    if declaration.id() == arrow_function_node.id() {
                        return Ok("default".to_string());
                    }
                }
            }
            _ => {}
        }
        current = node.parent();
    }

    Err(codesearch_core::error::Error::entity_extraction(
        "Could not find name for arrow function".to_string(),
    ))
}
