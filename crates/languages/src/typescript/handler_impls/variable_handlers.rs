//! Handler for extracting TypeScript variable and constant declarations
//!
//! This module processes tree-sitter query matches for lexical_declaration (const/let)
//! and variable_declaration (var) nodes and builds Constant/Variable entities.

#![deny(warnings)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]

use crate::common::{
    entity_building::{build_entity, CommonEntityComponents, EntityDetails},
    find_capture_node,
    module_utils::derive_path_entity_identifier,
    node_to_text, require_capture_node,
};
use crate::javascript::module_path::derive_module_path;
use crate::qualified_name::build_qualified_name_from_ast;
use codesearch_core::{
    entities::{EntityMetadata, EntityType, Language, SourceLocation, Visibility},
    entity_id::generate_entity_id,
    error::Result,
    CodeEntity,
};
use std::path::Path;
use tree_sitter::{Node, Query, QueryMatch};

/// Handle variable declarations (const, let, var)
#[allow(clippy::too_many_arguments)]
pub fn handle_variable_impl(
    query_match: &QueryMatch,
    query: &Query,
    source: &str,
    file_path: &Path,
    repository_id: &str,
    _package_name: Option<&str>,
    source_root: Option<&Path>,
    repo_root: &Path,
) -> Result<Vec<CodeEntity>> {
    let declaration_node = require_capture_node(query_match, query, "declaration")?;

    // Skip if not at module level (inside function bodies, etc.)
    if !is_module_level(declaration_node) {
        return Ok(vec![]);
    }

    // Check if this is a destructuring pattern or simple identifier
    if let Some(pattern_node) = find_capture_node(query_match, query, "destructure_pattern") {
        return handle_destructuring_pattern(
            declaration_node,
            pattern_node,
            source,
            file_path,
            repository_id,
            source_root,
            repo_root,
        );
    }

    // Simple identifier case
    let name_node = require_capture_node(query_match, query, "name")?;

    // Skip if the value is an arrow function or function expression
    // (those are handled by the function handler to create Function entities)
    if let Some(value_node) = find_capture_node(query_match, query, "value") {
        let value_kind = value_node.kind();
        if value_kind == "arrow_function"
            || value_kind == "function_expression"
            || value_kind == "function"
        {
            return Ok(vec![]);
        }
    }

    // Extract name
    let name = node_to_text(name_node, source)?;

    // Determine if it's const (Constant) or let/var (Variable)
    let is_const = is_const_declaration(declaration_node);
    let entity_type = if is_const {
        EntityType::Constant
    } else {
        EntityType::Variable
    };

    // Derive module path from file path (for TypeScript, qualified names are file-based)
    let module_path = source_root
        .and_then(|root| derive_module_path(file_path, root))
        .or_else(|| {
            // Fallback: use repo_root if source_root not available
            derive_module_path(file_path, repo_root)
        });

    // Build qualified name from AST (for any parent scope like class/namespace)
    let scope_result = build_qualified_name_from_ast(declaration_node, source, "typescript");
    let ast_scope = if scope_result.parent_scope.is_empty() {
        None
    } else {
        Some(scope_result.parent_scope.clone())
    };

    // Compose qualified name: module.ast_scope.name (per TypeScript spec Q-ITEM-MODULE)
    let qualified_name = match (&module_path, &ast_scope) {
        (Some(module), Some(scope)) => format!("{module}.{scope}.{name}"),
        (Some(module), None) => format!("{module}.{name}"),
        (None, Some(scope)) => format!("{scope}.{name}"),
        (None, None) => name.clone(),
    };

    // Parent scope includes module path
    let parent_scope = match (&module_path, &ast_scope) {
        (Some(module), Some(scope)) => Some(format!("{module}.{scope}")),
        (Some(module), None) => Some(module.clone()),
        (None, Some(scope)) => Some(scope.clone()),
        (None, None) => None,
    };

    // Check if exported
    let is_exported = is_exported_declaration(declaration_node);
    let visibility = if is_exported {
        Visibility::Public
    } else {
        Visibility::Private
    };

    // Generate entity ID
    let file_path_str = file_path.to_string_lossy();
    let entity_id = generate_entity_id(repository_id, &file_path_str, &qualified_name);

    // Build path_entity_identifier
    let path_module = derive_path_entity_identifier(file_path, repo_root, ".");
    let path_entity_identifier = if let Some(ref scope) = parent_scope {
        format!("{path_module}.{scope}.{name}")
    } else {
        format!("{path_module}.{name}")
    };

    // Get location
    let location = SourceLocation::from_tree_sitter_node(declaration_node);

    // Create components
    let components = CommonEntityComponents {
        entity_id,
        repository_id: repository_id.to_string(),
        name,
        qualified_name,
        path_entity_identifier: Some(path_entity_identifier),
        parent_scope,
        file_path: file_path.to_path_buf(),
        location,
    };

    // Build the entity
    let entity = build_entity(
        components,
        EntityDetails {
            entity_type,
            language: Language::TypeScript,
            visibility: Some(visibility),
            documentation: None,
            content: node_to_text(declaration_node, source).ok(),
            metadata: EntityMetadata::default(),
            signature: None,
            relationships: Default::default(),
        },
    )?;

    Ok(vec![entity])
}

/// Check if a declaration is at module level (not inside a function/class/etc.)
fn is_module_level(node: tree_sitter::Node) -> bool {
    let mut current = node.parent();
    while let Some(parent) = current {
        match parent.kind() {
            // These are scope-introducing nodes that mean we're NOT at module level
            "function_declaration"
            | "function_expression"
            | "arrow_function"
            | "method_definition"
            | "class_body"
            | "block" => {
                // But if the block is directly under program or export_statement, it's still module level
                if parent.kind() == "block" {
                    if let Some(grandparent) = parent.parent() {
                        if grandparent.kind() == "program"
                            || grandparent.kind() == "export_statement"
                        {
                            current = parent.parent();
                            continue;
                        }
                    }
                }
                return false;
            }
            // program is the root, so we're at module level
            "program" => return true,
            // export_statement wraps exported declarations at module level
            "export_statement" => {
                // Check if the export_statement is directly under program
                if let Some(grandparent) = parent.parent() {
                    if grandparent.kind() == "program" {
                        return true;
                    }
                }
                current = parent.parent();
                continue;
            }
            _ => {
                current = parent.parent();
            }
        }
    }
    // If we reach here without hitting program, assume not module level
    false
}

/// Check if a declaration is a const declaration
fn is_const_declaration(node: tree_sitter::Node) -> bool {
    // For lexical_declaration, we need to check the first child for "const" or "let"
    if node.kind() == "lexical_declaration" {
        for child in node.children(&mut node.walk()) {
            if child.kind() == "const" {
                return true;
            }
            if child.kind() == "let" {
                return false;
            }
        }
    }
    // variable_declaration is always var (not const)
    false
}

/// Check if a declaration is exported or ambient (declare keyword)
/// Ambient declarations are always public in TypeScript
fn is_exported_declaration(node: tree_sitter::Node) -> bool {
    let mut current = Some(node);
    while let Some(n) = current {
        match n.kind() {
            "export_statement" | "ambient_declaration" => return true,
            _ => current = n.parent(),
        }
    }
    false
}

/// Handle destructuring patterns in variable declarations
/// e.g., `const { a, b } = obj` or `const { x: renamed } = obj`
#[allow(clippy::too_many_arguments)]
fn handle_destructuring_pattern(
    declaration_node: Node,
    pattern_node: Node,
    source: &str,
    file_path: &Path,
    repository_id: &str,
    source_root: Option<&Path>,
    repo_root: &Path,
) -> Result<Vec<CodeEntity>> {
    let mut entities = Vec::new();

    // Determine if it's const or let/var
    let is_const = is_const_declaration(declaration_node);
    let entity_type = if is_const {
        EntityType::Constant
    } else {
        EntityType::Variable
    };

    // Check if exported
    let is_exported = is_exported_declaration(declaration_node);
    let visibility = if is_exported {
        Visibility::Public
    } else {
        Visibility::Private
    };

    // Derive module path
    let module_path = source_root
        .and_then(|root| derive_module_path(file_path, root))
        .or_else(|| derive_module_path(file_path, repo_root));

    // Build qualified name from AST (for any parent scope like class/namespace)
    let scope_result = build_qualified_name_from_ast(declaration_node, source, "typescript");
    let ast_scope = if scope_result.parent_scope.is_empty() {
        None
    } else {
        Some(scope_result.parent_scope.clone())
    };

    // Extract names from the object pattern
    let names = extract_names_from_object_pattern(pattern_node, source)?;

    for name in names {
        // Compose qualified name: module.ast_scope.name
        let qualified_name = match (&module_path, &ast_scope) {
            (Some(module), Some(scope)) => format!("{module}.{scope}.{name}"),
            (Some(module), None) => format!("{module}.{name}"),
            (None, Some(scope)) => format!("{scope}.{name}"),
            (None, None) => name.clone(),
        };

        // Parent scope includes module path
        let parent_scope = match (&module_path, &ast_scope) {
            (Some(module), Some(scope)) => Some(format!("{module}.{scope}")),
            (Some(module), None) => Some(module.clone()),
            (None, Some(scope)) => Some(scope.clone()),
            (None, None) => None,
        };

        // Generate entity ID
        let file_path_str = file_path.to_string_lossy();
        let entity_id = generate_entity_id(repository_id, &file_path_str, &qualified_name);

        // Build path_entity_identifier
        let path_module = derive_path_entity_identifier(file_path, repo_root, ".");
        let path_entity_identifier = if let Some(ref scope) = parent_scope {
            format!("{path_module}.{scope}.{name}")
        } else {
            format!("{path_module}.{name}")
        };

        // Get location from declaration node (not ideal but works)
        let location = SourceLocation::from_tree_sitter_node(declaration_node);

        // Create components
        let components = CommonEntityComponents {
            entity_id,
            repository_id: repository_id.to_string(),
            name,
            qualified_name,
            path_entity_identifier: Some(path_entity_identifier),
            parent_scope,
            file_path: file_path.to_path_buf(),
            location,
        };

        // Build the entity
        let entity = build_entity(
            components,
            EntityDetails {
                entity_type,
                language: Language::TypeScript,
                visibility: Some(visibility),
                documentation: None,
                content: node_to_text(declaration_node, source).ok(),
                metadata: EntityMetadata::default(),
                signature: None,
                relationships: Default::default(),
            },
        )?;

        entities.push(entity);
    }

    Ok(entities)
}

/// Extract variable names from an object_pattern node
/// Handles: { a, b }, { x: renamed }, { a: { nested } }
fn extract_names_from_object_pattern(pattern_node: Node, source: &str) -> Result<Vec<String>> {
    let mut names = Vec::new();

    for child in pattern_node.named_children(&mut pattern_node.walk()) {
        match child.kind() {
            // Shorthand property: { a }
            "shorthand_property_identifier_pattern" => {
                if let Ok(name) = node_to_text(child, source) {
                    names.push(name);
                }
            }
            // Pair pattern: { x: renamed } or { x: { nested } }
            "pair_pattern" => {
                // Get the value (right side of colon)
                if let Some(value) = child.child_by_field_name("value") {
                    match value.kind() {
                        "identifier" => {
                            if let Ok(name) = node_to_text(value, source) {
                                names.push(name);
                            }
                        }
                        // Nested object pattern - recursively extract
                        "object_pattern" => {
                            names.extend(extract_names_from_object_pattern(value, source)?);
                        }
                        _ => {
                            tracing::trace!(
                                kind = value.kind(),
                                "Unhandled pair pattern value type"
                            );
                        }
                    }
                }
            }
            // Rest element: { ...rest }
            "rest_pattern" => {
                if let Some(name_node) = child.named_child(0) {
                    if let Ok(name) = node_to_text(name_node, source) {
                        names.push(name);
                    }
                }
            }
            _ => {
                tracing::trace!(kind = child.kind(), "Unhandled object pattern child type");
            }
        }
    }

    Ok(names)
}
