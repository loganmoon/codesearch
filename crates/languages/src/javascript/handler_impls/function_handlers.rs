//! JavaScript function handler implementations

use crate::common::{
    entity_building::{build_entity, extract_common_components, EntityDetails, ExtractionContext},
    find_capture_node,
    import_map::{get_ast_root, parse_file_imports},
    node_to_text, require_capture_node,
};
use crate::javascript::{
    module_path::derive_module_path,
    utils::{
        extract_function_calls, extract_jsdoc_comments, extract_parameters,
        extract_type_references_from_jsdoc,
    },
};
use codesearch_core::{
    entities::{
        EntityMetadata, EntityType, FunctionSignature, Language, SourceLocation, Visibility,
    },
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
    package_name: Option<&str>,
    source_root: Option<&Path>,
) -> Result<Vec<CodeEntity>> {
    let function_node = require_capture_node(query_match, query, "function")?;

    let ctx = ExtractionContext {
        query_match,
        query,
        source,
        file_path,
        repository_id,
        package_name,
        source_root,
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

    // Derive module path for qualified name resolution
    let module_path = source_root.and_then(|root| derive_module_path(file_path, root));

    // Build import map from file's imports for qualified name resolution
    let root = get_ast_root(function_node);
    let import_map = parse_file_imports(root, source, Language::JavaScript, module_path.as_deref());

    // Extract function calls from the function body with qualified name resolution
    let calls = extract_function_calls(
        function_node,
        source,
        &import_map,
        components.parent_scope.as_deref(),
    );

    // Extract type references from JSDoc for USES relationships
    let type_refs = extract_type_references_from_jsdoc(
        documentation.as_deref(),
        &import_map,
        components.parent_scope.as_deref(),
    );

    // Build metadata
    let mut metadata = EntityMetadata {
        is_async,
        ..EntityMetadata::default()
    };

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

    // Build entity using shared helper
    let entity = build_entity(
        components,
        EntityDetails {
            entity_type: EntityType::Function,
            language: Language::JavaScript,
            visibility: Visibility::Public,
            documentation,
            content: node_to_text(function_node, source).ok(),
            metadata,
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
#[allow(unused_variables)]
pub fn handle_arrow_function_impl(
    query_match: &QueryMatch,
    query: &Query,
    source: &str,
    file_path: &Path,
    repository_id: &str,
    package_name: Option<&str>,
    source_root: Option<&Path>,
) -> Result<Vec<CodeEntity>> {
    use crate::common::entity_building::CommonEntityComponents;

    let arrow_function_node = require_capture_node(query_match, query, "arrow_function")?;

    // Arrow functions need special name extraction from parent context
    // For anonymous arrow functions (callbacks, etc.), use a synthetic name with line number
    let name = extract_arrow_function_name(arrow_function_node, source).unwrap_or_else(|_| {
        format!(
            "<anonymous@{}>",
            arrow_function_node.start_position().row + 1
        )
    });

    // Build qualified name
    let scope_result = crate::qualified_name::build_qualified_name_from_ast(
        arrow_function_node,
        source,
        "javascript",
    );
    let parent_scope = scope_result.parent_scope;
    let full_qualified_name = if parent_scope.is_empty() {
        name.clone()
    } else {
        format!("{parent_scope}.{name}")
    };

    // Extract parameters from the arrow function node
    let parameters = extract_arrow_function_parameters(arrow_function_node, source)?;

    // Check for async modifier
    let is_async = arrow_function_node
        .children(&mut arrow_function_node.walk())
        .any(|child| child.kind() == "async");

    // Extract JSDoc documentation
    let documentation = extract_jsdoc_comments(arrow_function_node, source);

    // Derive module path for qualified name resolution
    let module_path = source_root.and_then(|root| derive_module_path(file_path, root));

    // Build import map from file's imports for qualified name resolution
    let root = get_ast_root(arrow_function_node);
    let import_map = parse_file_imports(root, source, Language::JavaScript, module_path.as_deref());

    // Extract function calls from the function body with qualified name resolution
    let calls = extract_function_calls(
        arrow_function_node,
        source,
        &import_map,
        if parent_scope.is_empty() {
            None
        } else {
            Some(parent_scope.as_str())
        },
    );

    // Extract type references from JSDoc for USES relationships
    let type_refs = extract_type_references_from_jsdoc(
        documentation.as_deref(),
        &import_map,
        if parent_scope.is_empty() {
            None
        } else {
            Some(parent_scope.as_str())
        },
    );

    // Build metadata
    let mut metadata = EntityMetadata {
        is_async,
        ..EntityMetadata::default()
    };

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

    // Generate entity_id
    let file_path_str = file_path
        .to_str()
        .ok_or_else(|| codesearch_core::error::Error::entity_extraction("Invalid file path"))?;
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
        parent_scope: if parent_scope.is_empty() {
            None
        } else {
            Some(parent_scope)
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
            metadata,
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
