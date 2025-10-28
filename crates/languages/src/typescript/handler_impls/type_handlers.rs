//! TypeScript type entity handler implementations

use crate::common::{node_to_text, require_capture_node};
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

/// Handle class declarations (reuse JavaScript with type enhancement)
pub fn handle_class_impl(
    query_match: &QueryMatch,
    query: &Query,
    source: &str,
    file_path: &Path,
    repository_id: &str,
) -> Result<Vec<CodeEntity>> {
    // Reuse JavaScript class handler
    let mut entities = crate::javascript::handler_impls::handle_class_impl(
        query_match,
        query,
        source,
        file_path,
        repository_id,
    )?;

    // Update language to TypeScript
    for entity in &mut entities {
        entity.language = Language::TypeScript;
    }

    Ok(entities)
}

/// Handle method declarations (reuse JavaScript with type enhancement)
pub fn handle_method_impl(
    query_match: &QueryMatch,
    query: &Query,
    source: &str,
    file_path: &Path,
    repository_id: &str,
) -> Result<Vec<CodeEntity>> {
    // Reuse JavaScript method handler
    let mut entities = crate::javascript::handler_impls::handle_method_impl(
        query_match,
        query,
        source,
        file_path,
        repository_id,
    )?;

    // Update language to TypeScript
    for entity in &mut entities {
        entity.language = Language::TypeScript;
    }

    Ok(entities)
}

/// Handle interface declarations
pub fn handle_interface_impl(
    query_match: &QueryMatch,
    query: &Query,
    source: &str,
    file_path: &Path,
    repository_id: &str,
) -> Result<Vec<CodeEntity>> {
    let interface_node = require_capture_node(query_match, query, "interface")?;

    // Extract name
    let name_node = require_capture_node(query_match, query, "name")?;
    let name = node_to_text(name_node, source)?;

    // Build qualified name
    let qualified_name =
        crate::qualified_name::build_qualified_name_from_ast(interface_node, source, "typescript");
    let full_qualified_name = if qualified_name.is_empty() {
        name.clone()
    } else {
        format!("{qualified_name}.{name}")
    };

    // Extract generics (type_parameters)
    let generics = extract_generics(interface_node, source)?;

    // Extract extended interfaces
    let extends = extract_extends_clause(interface_node, source)?;

    // Extract JSDoc documentation
    let documentation = extract_jsdoc_comments(interface_node, source);

    // Build metadata
    let mut metadata = EntityMetadata::default();
    if let Some(extends_text) = extends {
        metadata
            .attributes
            .insert("extends".to_string(), extends_text);
    }

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
        .entity_type(EntityType::Interface)
        .location(SourceLocation::from_tree_sitter_node(interface_node))
        .visibility(Visibility::Public)
        .documentation_summary(documentation)
        .content(node_to_text(interface_node, source).ok())
        .metadata(metadata)
        .signature(if !generics.is_empty() {
            Some(FunctionSignature {
                parameters: Vec::new(),
                return_type: None,
                generics,
                is_async: false,
            })
        } else {
            None
        })
        .language(Language::TypeScript)
        .file_path(file_path.to_path_buf())
        .build()
        .map_err(|e| {
            codesearch_core::error::Error::entity_extraction(format!(
                "Failed to build CodeEntity: {e}"
            ))
        })?;

    Ok(vec![entity])
}

/// Handle type alias declarations
pub fn handle_type_alias_impl(
    query_match: &QueryMatch,
    query: &Query,
    source: &str,
    file_path: &Path,
    repository_id: &str,
) -> Result<Vec<CodeEntity>> {
    let type_alias_node = require_capture_node(query_match, query, "type_alias")?;

    // Extract name
    let name_node = require_capture_node(query_match, query, "name")?;
    let name = node_to_text(name_node, source)?;

    // Build qualified name
    let qualified_name =
        crate::qualified_name::build_qualified_name_from_ast(type_alias_node, source, "typescript");
    let full_qualified_name = if qualified_name.is_empty() {
        name.clone()
    } else {
        format!("{qualified_name}.{name}")
    };

    // Extract generics
    let generics = extract_generics(type_alias_node, source)?;

    // Extract type value from the node itself
    let type_value = extract_type_value(type_alias_node, source)?;

    // Extract JSDoc documentation
    let documentation = extract_jsdoc_comments(type_alias_node, source);

    // Build metadata
    let mut metadata = EntityMetadata::default();
    if let Some(type_text) = type_value {
        metadata
            .attributes
            .insert("type_value".to_string(), type_text);
    }

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
        .entity_type(EntityType::TypeAlias)
        .location(SourceLocation::from_tree_sitter_node(type_alias_node))
        .visibility(Visibility::Public)
        .documentation_summary(documentation)
        .content(node_to_text(type_alias_node, source).ok())
        .metadata(metadata)
        .signature(if !generics.is_empty() {
            Some(FunctionSignature {
                parameters: Vec::new(),
                return_type: None,
                generics,
                is_async: false,
            })
        } else {
            None
        })
        .language(Language::TypeScript)
        .file_path(file_path.to_path_buf())
        .build()
        .map_err(|e| {
            codesearch_core::error::Error::entity_extraction(format!(
                "Failed to build CodeEntity: {e}"
            ))
        })?;

    Ok(vec![entity])
}

/// Handle enum declarations
pub fn handle_enum_impl(
    query_match: &QueryMatch,
    query: &Query,
    source: &str,
    file_path: &Path,
    repository_id: &str,
) -> Result<Vec<CodeEntity>> {
    let enum_node = require_capture_node(query_match, query, "enum")?;

    // Extract name
    let name_node = require_capture_node(query_match, query, "name")?;
    let name = node_to_text(name_node, source)?;

    // Build qualified name
    let qualified_name =
        crate::qualified_name::build_qualified_name_from_ast(enum_node, source, "typescript");
    let full_qualified_name = if qualified_name.is_empty() {
        name.clone()
    } else {
        format!("{qualified_name}.{name}")
    };

    // Extract enum members from the node itself
    let members = extract_enum_members_from_node(enum_node, source)?;

    // Extract JSDoc documentation
    let documentation = extract_jsdoc_comments(enum_node, source);

    // Build metadata
    let mut metadata = EntityMetadata::default();
    if !members.is_empty() {
        metadata
            .attributes
            .insert("members".to_string(), members.join(", "));
    }

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
        .entity_type(EntityType::Enum)
        .location(SourceLocation::from_tree_sitter_node(enum_node))
        .visibility(Visibility::Public)
        .documentation_summary(documentation)
        .content(node_to_text(enum_node, source).ok())
        .metadata(metadata)
        .language(Language::TypeScript)
        .file_path(file_path.to_path_buf())
        .build()
        .map_err(|e| {
            codesearch_core::error::Error::entity_extraction(format!(
                "Failed to build CodeEntity: {e}"
            ))
        })?;

    Ok(vec![entity])
}

/// Extract generic type parameters from a node
fn extract_generics(node: Node, source: &str) -> Result<Vec<String>> {
    let mut generics = Vec::new();

    for child in node.children(&mut node.walk()) {
        if child.kind() == "type_parameters" {
            for param in child.named_children(&mut child.walk()) {
                if param.kind() == "type_parameter" {
                    if let Some(name_node) = param.child_by_field_name("name") {
                        generics.push(node_to_text(name_node, source)?);
                    }
                }
            }
        }
    }

    Ok(generics)
}

/// Extract extends clause from a node
fn extract_extends_clause(node: Node, source: &str) -> Result<Option<String>> {
    for child in node.children(&mut node.walk()) {
        if child.kind() == "extends_clause" || child.kind() == "class_heritage" {
            return Ok(Some(node_to_text(child, source)?));
        }
    }
    Ok(None)
}

/// Extract type value from type alias node
fn extract_type_value(type_alias_node: Node, source: &str) -> Result<Option<String>> {
    for child in type_alias_node.children(&mut type_alias_node.walk()) {
        // Look for the type value after the '=' token
        if child.kind() == "=" {
            if let Some(next) = child.next_sibling() {
                return Ok(Some(node_to_text(next, source)?));
            }
        }
    }
    Ok(None)
}

/// Extract enum members from enum node
fn extract_enum_members_from_node(enum_node: Node, source: &str) -> Result<Vec<String>> {
    let mut members = Vec::new();

    // Find the enum_body child
    for child in enum_node.children(&mut enum_node.walk()) {
        if child.kind() == "enum_body" {
            for member in child.named_children(&mut child.walk()) {
                if member.kind() == "enum_assignment" || member.kind() == "property_identifier" {
                    if let Some(name_node) = member.child_by_field_name("name") {
                        members.push(node_to_text(name_node, source)?);
                    } else {
                        members.push(node_to_text(member, source)?);
                    }
                }
            }
        }
    }

    Ok(members)
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
