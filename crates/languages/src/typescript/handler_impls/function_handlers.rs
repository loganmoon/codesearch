//! TypeScript function handler implementations

use crate::common::{
    find_capture_node,
    import_map::{get_ast_root, parse_file_imports},
    node_to_text,
};
use crate::javascript::module_path::derive_module_path;
use crate::typescript::utils::extract_type_references;
use codesearch_core::{
    entities::{Language, SourceReference},
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
    package_name: Option<&str>,
    source_root: Option<&Path>,
    repo_root: &Path,
) -> Result<Vec<CodeEntity>> {
    // Start with JavaScript extraction
    let mut entities = crate::javascript::handler_impls::handle_function_impl(
        query_match,
        query,
        source,
        file_path,
        repository_id,
        package_name,
        source_root,
        repo_root,
    )?;

    // Derive module path for qualified name resolution
    let module_path = source_root.and_then(|root| derive_module_path(file_path, root));

    // Enhance with TypeScript type information and update language for all entities
    for entity in &mut entities {
        enhance_with_type_annotations(entity, query_match, query, source, module_path.as_deref())?;
        entity.language = codesearch_core::entities::Language::TypeScript;
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
    package_name: Option<&str>,
    source_root: Option<&Path>,
    repo_root: &Path,
) -> Result<Vec<CodeEntity>> {
    // Start with JavaScript extraction
    let mut entities = crate::javascript::handler_impls::handle_arrow_function_impl(
        query_match,
        query,
        source,
        file_path,
        repository_id,
        package_name,
        source_root,
        repo_root,
    )?;

    // Derive module path for qualified name resolution
    let module_path = source_root.and_then(|root| derive_module_path(file_path, root));

    // Enhance with TypeScript type information and update language for all entities
    for entity in &mut entities {
        enhance_with_type_annotations(entity, query_match, query, source, module_path.as_deref())?;
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

        // Merge TypeScript type references with any existing references
        if !ts_type_refs.is_empty() {
            // Get existing references (if any)
            let mut all_refs: Vec<SourceReference> = entity
                .metadata
                .attributes
                .get("references")
                .and_then(|s| serde_json::from_str(s).ok())
                .unwrap_or_default();

            // Add TypeScript type references (deduplicating by target)
            let mut seen: std::collections::HashSet<String> =
                all_refs.iter().map(|r| r.target().to_string()).collect();
            for type_ref in ts_type_refs.iter() {
                if seen.insert(type_ref.target().to_string()) {
                    all_refs.push(type_ref.clone());
                }
            }

            // Store combined references with locations
            if let Ok(json) = serde_json::to_string(&all_refs) {
                entity
                    .metadata
                    .attributes
                    .insert("references".to_string(), json);
            }

            // Also store simplified uses_types list for backward compatibility
            let mut all_type_targets: Vec<String> = entity
                .metadata
                .attributes
                .get("uses_types")
                .and_then(|s| serde_json::from_str(s).ok())
                .unwrap_or_default();
            let mut seen_targets: std::collections::HashSet<_> =
                all_type_targets.iter().cloned().collect();
            for type_ref in &ts_type_refs {
                if seen_targets.insert(type_ref.target().to_string()) {
                    all_type_targets.push(type_ref.target().to_string());
                }
            }
            if let Ok(json) = serde_json::to_string(&all_type_targets) {
                entity
                    .metadata
                    .attributes
                    .insert("uses_types".to_string(), json);
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
            _ => {}
        }
    }

    Ok(parameters)
}
