//! TypeScript function handler implementations

#![deny(warnings)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]

use crate::common::{
    find_capture_node,
    import_map::{get_ast_root, parse_file_imports},
    node_to_text,
};
use crate::javascript::module_path::derive_module_path;
use crate::typescript::utils::{extract_call_references, extract_type_references};
use codesearch_core::{
    entities::{Language, Visibility},
    error::Result,
    CodeEntity,
};
use std::path::Path;
use tracing::debug;
use tree_sitter::{Node, Query, QueryMatch};

/// Handle regular function declarations with TypeScript type annotations
#[allow(clippy::too_many_arguments)]
pub fn handle_function_impl(
    query_match: &QueryMatch,
    query: &Query,
    source: &str,
    file_path: &Path,
    repository_id: &str,
    _package_name: Option<&str>,
    source_root: Option<&Path>,
    repo_root: &Path,
) -> Result<Vec<CodeEntity>> {
    // Start with JavaScript extraction
    // Note: TypeScript qualified names are based on file paths only, not package names
    // per spec rule Q-MODULE-FILE and Q-ITEM-MODULE
    let mut entities = crate::javascript::handler_impls::handle_function_impl(
        query_match,
        query,
        source,
        file_path,
        repository_id,
        None, // TypeScript doesn't use package name in qualified names
        source_root,
        repo_root,
    )?;

    // Derive module path for qualified name resolution
    let module_path = source_root.and_then(|root| derive_module_path(file_path, root));

    // Check if the function is exported
    let function_node = find_capture_node(query_match, query, "function");
    let is_exported = function_node.map(|n| is_node_exported(n)).unwrap_or(false);

    // Enhance with TypeScript type information and update language for all entities
    for entity in &mut entities {
        enhance_with_type_annotations(entity, query_match, query, source, module_path.as_deref())?;
        entity.language = codesearch_core::entities::Language::TypeScript;

        // Set visibility based on export status (per V-EXPORT and V-MODULE-PRIVATE)
        entity.visibility = Some(if is_exported {
            Visibility::Public
        } else {
            Visibility::Private
        });
    }

    Ok(entities)
}

/// Handle arrow functions with TypeScript type annotations
#[allow(clippy::too_many_arguments)]
pub fn handle_arrow_function_impl(
    query_match: &QueryMatch,
    query: &Query,
    source: &str,
    file_path: &Path,
    repository_id: &str,
    _package_name: Option<&str>,
    source_root: Option<&Path>,
    repo_root: &Path,
) -> Result<Vec<CodeEntity>> {
    // Start with JavaScript extraction
    // Note: TypeScript qualified names are based on file paths only, not package names
    // per spec rule Q-MODULE-FILE and Q-ITEM-MODULE
    let mut entities = crate::javascript::handler_impls::handle_arrow_function_impl(
        query_match,
        query,
        source,
        file_path,
        repository_id,
        None, // TypeScript doesn't use package name in qualified names
        source_root,
        repo_root,
    )?;

    // Derive module path for qualified name resolution
    let module_path = source_root
        .and_then(|root| derive_module_path(file_path, root))
        .or_else(|| derive_module_path(file_path, repo_root));

    // Check if the arrow function is exported
    let arrow_node = find_capture_node(query_match, query, "arrow_function");
    let is_exported = arrow_node.map(|n| is_node_exported(n)).unwrap_or(false);

    // Enhance with TypeScript type information and update language for all entities
    for entity in &mut entities {
        enhance_with_type_annotations(entity, query_match, query, source, module_path.as_deref())?;
        entity.language = codesearch_core::entities::Language::TypeScript;

        // Set visibility based on export status (per V-EXPORT and V-MODULE-PRIVATE)
        entity.visibility = Some(if is_exported {
            Visibility::Public
        } else {
            Visibility::Private
        });

        // Fix qualified name to include module path (JS handler doesn't include it)
        if let Some(ref module) = module_path {
            // Only prepend if not already included
            if !entity.qualified_name.starts_with(module) {
                let old_qn = entity.qualified_name.clone();
                entity.qualified_name = format!("{module}.{old_qn}");
                // Also update parent_scope to include module path
                entity.parent_scope = Some(match &entity.parent_scope {
                    Some(scope) => format!("{module}.{scope}"),
                    None => module.clone(),
                });
            }
        }
    }

    Ok(entities)
}

/// Handle function expressions (named and anonymous)
/// For: `const onClick = function handleClick() {}` or `const onHover = function() {}`
/// NOTE: Currently disabled - causes timeout issues
#[allow(clippy::too_many_arguments)]
#[allow(dead_code)]
pub fn handle_function_expression_impl(
    query_match: &QueryMatch,
    query: &Query,
    source: &str,
    file_path: &Path,
    repository_id: &str,
    _package_name: Option<&str>,
    source_root: Option<&Path>,
    repo_root: &Path,
) -> Result<Vec<CodeEntity>> {
    use codesearch_core::entities::{
        CodeEntityBuilder, EntityRelationshipData, EntityType, FunctionSignature, SourceLocation,
    };
    use codesearch_core::entity_id::generate_entity_id;

    let func_expr_node = crate::common::require_capture_node(query_match, query, "func_expr")?;

    // Get function name: prefer internal name, fall back to variable name from parent
    let func_name = find_capture_node(query_match, query, "func_name")
        .and_then(|n| node_to_text(n, source).ok());

    // If no internal name, try to find the variable name by traversing up
    let var_name = if func_name.is_none() {
        find_parent_variable_name(func_expr_node, source)
    } else {
        None
    };

    let name = func_name.or(var_name).ok_or_else(|| {
        codesearch_core::error::Error::entity_extraction("Could not extract function name")
    })?;

    // Derive module path for qualified name
    let module_path = source_root
        .and_then(|root| derive_module_path(file_path, root))
        .or_else(|| derive_module_path(file_path, repo_root));

    // Build qualified name
    let qualified_name = match &module_path {
        Some(module) => format!("{module}.{name}"),
        None => name.clone(),
    };

    // Parent scope is the module
    let parent_scope = module_path.clone();

    // Build import map for type reference resolution
    let root = get_ast_root(func_expr_node);
    let import_map = parse_file_imports(root, source, Language::TypeScript, module_path.as_deref());

    // Extract parameters with types
    let parameters = if let Some(params_node) = find_capture_node(query_match, query, "params") {
        extract_typescript_parameters(params_node, source)?
    } else {
        Vec::new()
    };

    // Extract return type annotation
    let return_type = find_type_annotation(func_expr_node, source)?;

    // Check if exported by traversing up to find export_statement
    let is_exported = is_node_exported(func_expr_node);

    // Check for async
    let is_async = func_expr_node
        .children(&mut func_expr_node.walk())
        .any(|c| c.kind() == "async");

    // Extract type references
    let type_refs =
        extract_type_references(func_expr_node, source, &import_map, parent_scope.as_deref());

    // Extract call references
    let call_refs =
        extract_call_references(func_expr_node, source, &import_map, parent_scope.as_deref());

    // Build relationships
    let relationships = EntityRelationshipData {
        uses_types: type_refs,
        calls: call_refs,
        ..Default::default()
    };

    // Generate entity_id
    let file_path_str = file_path
        .to_str()
        .ok_or_else(|| codesearch_core::error::Error::entity_extraction("Invalid file path"))?;
    let entity_id = generate_entity_id(repository_id, file_path_str, &qualified_name);

    // Build path_entity_identifier
    let path_entity_identifier = file_path
        .file_stem()
        .and_then(|s| s.to_str())
        .map(|stem| format!("{stem}.{name}"))
        .unwrap_or_else(|| name.clone());

    let entity = CodeEntityBuilder::default()
        .entity_id(entity_id)
        .repository_id(repository_id.to_string())
        .name(name)
        .qualified_name(qualified_name)
        .path_entity_identifier(path_entity_identifier)
        .parent_scope(parent_scope)
        .entity_type(EntityType::Function)
        .location(SourceLocation::from_tree_sitter_node(func_expr_node))
        .visibility(Some(if is_exported {
            Visibility::Public
        } else {
            Visibility::Private
        }))
        .documentation_summary(None)
        .content(node_to_text(func_expr_node, source).ok())
        .metadata(codesearch_core::entities::EntityMetadata {
            is_async,
            ..Default::default()
        })
        .signature(Some(FunctionSignature {
            parameters,
            return_type,
            generics: Vec::new(),
            is_async,
        }))
        .language(Language::TypeScript)
        .relationships(relationships)
        .build()
        .map_err(|e| codesearch_core::error::Error::EntityExtraction(e.to_string()))?;

    Ok(vec![entity])
}

/// Find the variable name from a parent variable_declarator node
#[allow(dead_code)]
fn find_parent_variable_name(node: Node, source: &str) -> Option<String> {
    let mut current = node.parent();
    while let Some(parent) = current {
        if parent.kind() == "variable_declarator" {
            if let Some(name_node) = parent.child_by_field_name("name") {
                if name_node.kind() == "identifier" {
                    return node_to_text(name_node, source).ok();
                }
            }
        }
        // Stop if we hit something that shouldn't contain a variable declarator
        if parent.kind() == "program" || parent.kind() == "class_body" {
            break;
        }
        current = parent.parent();
    }
    None
}

/// Check if a node is exported (has an export_statement or ambient_declaration ancestor)
/// Ambient declarations (declare keyword) are always public in TypeScript
fn is_node_exported(node: Node) -> bool {
    let mut current = Some(node);
    while let Some(n) = current {
        match n.kind() {
            "export_statement" | "ambient_declaration" => return true,
            _ => current = n.parent(),
        }
    }
    false
}

/// Enhance a function entity with TypeScript type annotations
fn enhance_with_type_annotations(
    entity: &mut CodeEntity,
    query_match: &QueryMatch,
    query: &Query,
    source: &str,
    module_path: Option<&str>,
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

        // Build import map for type reference resolution
        let root = get_ast_root(function_node);
        let import_map = parse_file_imports(root, source, Language::TypeScript, module_path);

        // Extract type references from TypeScript type annotations
        let ts_type_refs = extract_type_references(
            function_node,
            source,
            &import_map,
            entity.parent_scope.as_deref(),
        );

        // Add type references to relationships.uses_types for USES relationship resolution
        if !ts_type_refs.is_empty() {
            // Deduplicate by target
            let mut seen: std::collections::HashSet<String> = entity
                .relationships
                .uses_types
                .iter()
                .map(|r| r.target().to_string())
                .collect();

            for type_ref in ts_type_refs {
                if seen.insert(type_ref.target().to_string()) {
                    entity.relationships.uses_types.push(type_ref);
                }
            }
        }

        // Extract function call references from the function body
        let call_refs = extract_call_references(
            function_node,
            source,
            &import_map,
            entity.parent_scope.as_deref(),
        );

        // Add call references to relationships.calls for CALLS relationship resolution
        if !call_refs.is_empty() {
            let mut seen: std::collections::HashSet<String> = entity
                .relationships
                .calls
                .iter()
                .map(|r| r.target().to_string())
                .collect();

            for call_ref in call_refs {
                if seen.insert(call_ref.target().to_string()) {
                    entity.relationships.calls.push(call_ref);
                }
            }
        }

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
                    debug!(
                        "Parameter at line {} has no 'pattern' field, skipping",
                        child.start_position().row + 1
                    );
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
            _ => {
                tracing::trace!(kind = child.kind(), "Unhandled parameter node type");
            }
        }
    }

    Ok(parameters)
}
