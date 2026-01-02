//! TypeScript type entity handler implementations

use crate::common::{
    import_map::{get_ast_root, parse_file_imports, resolve_reference},
    node_to_text, require_capture_node,
};
use crate::javascript::{module_path::derive_module_path, utils::extract_jsdoc_comments};
use crate::typescript::utils::{extract_type_references, is_ts_primitive};
use codesearch_core::{
    entities::{
        CodeEntityBuilder, EntityMetadata, EntityRelationshipData, EntityType, FunctionSignature,
        Language, ReferenceType, SourceLocation, SourceReference, Visibility,
    },
    entity_id::generate_entity_id,
    error::Result,
    CodeEntity,
};
use std::path::Path;
use tree_sitter::{Node, Query, QueryMatch};

/// Handle class declarations (reuse JavaScript with type enhancement)
#[allow(clippy::too_many_arguments)]
pub fn handle_class_impl(
    query_match: &QueryMatch,
    query: &Query,
    source: &str,
    file_path: &Path,
    repository_id: &str,
    package_name: Option<&str>,
    source_root: Option<&Path>,
    repo_root: &Path,
) -> Result<Vec<CodeEntity>> {
    // Reuse JavaScript class handler (includes extends_resolved)
    let mut entities = crate::javascript::handler_impls::handle_class_impl(
        query_match,
        query,
        source,
        file_path,
        repository_id,
        package_name,
        source_root,
        repo_root,
    )?;

    // Get the class node to extract implements clause
    let class_node = require_capture_node(query_match, query, "class")?;

    // Derive module path for qualified name resolution
    let module_path = source_root.and_then(|root| derive_module_path(file_path, root));

    // Build import map for interface resolution
    let root = get_ast_root(class_node);
    let import_map = parse_file_imports(root, source, Language::TypeScript, module_path.as_deref());

    // Build parent_scope for reference resolution
    let scope_result =
        crate::qualified_name::build_qualified_name_from_ast(class_node, source, "typescript");
    let parent_scope = if scope_result.parent_scope.is_empty() {
        None
    } else {
        Some(scope_result.parent_scope.as_str())
    };

    // Extract implements clause (TypeScript-specific)
    let implements_raw = extract_implements_types(class_node, source)?;

    // Build SourceReference objects for implements relationships
    let implements_refs: Vec<SourceReference> = implements_raw
        .iter()
        .filter_map(|type_name| {
            let resolved = resolve_reference(type_name, &import_map, parent_scope, ".");
            SourceReference::builder()
                .target(resolved)
                .simple_name(type_name.clone())
                .is_external(false) // TS doesn't track external refs yet
                .location(SourceLocation::default())
                .ref_type(ReferenceType::Implements)
                .build()
                .ok()
        })
        .collect();

    // Extract type references used in the class body
    let type_refs = extract_type_references(class_node, source, &import_map, parent_scope);

    // Update language and add TypeScript-specific relationship data
    for entity in &mut entities {
        entity.language = Language::TypeScript;

        // Add implements to relationship data
        if !implements_refs.is_empty() {
            entity.relationships.implements = implements_refs.clone();
        }

        // Add type references for USES relationships
        if !type_refs.is_empty() {
            entity.relationships.uses_types.extend(type_refs.clone());
        }
    }

    Ok(entities)
}

/// Handle method declarations (reuse JavaScript with type enhancement)
#[allow(clippy::too_many_arguments)]
pub fn handle_method_impl(
    query_match: &QueryMatch,
    query: &Query,
    source: &str,
    file_path: &Path,
    repository_id: &str,
    package_name: Option<&str>,
    source_root: Option<&Path>,
    repo_root: &Path,
) -> Result<Vec<CodeEntity>> {
    // Reuse JavaScript method handler
    let mut entities = crate::javascript::handler_impls::handle_method_impl(
        query_match,
        query,
        source,
        file_path,
        repository_id,
        package_name,
        source_root,
        repo_root,
    )?;

    // Update language to TypeScript
    for entity in &mut entities {
        entity.language = Language::TypeScript;
    }

    Ok(entities)
}

/// Handle interface declarations
#[allow(unused_variables)]
#[allow(clippy::too_many_arguments)]
pub fn handle_interface_impl(
    query_match: &QueryMatch,
    query: &Query,
    source: &str,
    file_path: &Path,
    repository_id: &str,
    package_name: Option<&str>,
    source_root: Option<&Path>,
    _repo_root: &Path,
) -> Result<Vec<CodeEntity>> {
    let interface_node = require_capture_node(query_match, query, "interface")?;

    // Extract name
    let name_node = require_capture_node(query_match, query, "name")?;
    let name = node_to_text(name_node, source)?;

    // Build qualified name
    let scope_result =
        crate::qualified_name::build_qualified_name_from_ast(interface_node, source, "typescript");
    let parent_scope = scope_result.parent_scope;
    let full_qualified_name = if parent_scope.is_empty() {
        name.clone()
    } else {
        format!("{parent_scope}.{name}")
    };

    // Extract generics (type_parameters)
    let generics = extract_generics(interface_node, source)?;

    // Extract extended interfaces (raw names)
    let extends = extract_extends_clause(interface_node, source)?;

    // Derive module path for qualified name resolution
    let module_path = source_root.and_then(|root| derive_module_path(file_path, root));

    // Build import map for type resolution
    let root = get_ast_root(interface_node);
    let import_map = parse_file_imports(root, source, Language::TypeScript, module_path.as_deref());

    // Extract type references used in the interface body
    let type_refs = extract_type_references(
        interface_node,
        source,
        &import_map,
        if parent_scope.is_empty() {
            None
        } else {
            Some(parent_scope.as_str())
        },
    );

    // Extract JSDoc documentation
    let documentation = extract_jsdoc_comments(interface_node, source);

    // Build metadata
    let metadata = EntityMetadata::default();

    // Build relationship data
    let mut relationships = EntityRelationshipData::default();

    // Build supertraits (interface extends interface = EXTENDS_INTERFACE)
    if extends.is_some() {
        let extends_types = extract_extends_types(interface_node, source)?;
        for type_name in extends_types {
            let resolved = resolve_reference(
                &type_name,
                &import_map,
                if parent_scope.is_empty() {
                    None
                } else {
                    Some(parent_scope.as_str())
                },
                ".",
            );
            if let Ok(extends_ref) = SourceReference::builder()
                .target(resolved)
                .simple_name(type_name)
                .is_external(false)
                .location(SourceLocation::default())
                .ref_type(ReferenceType::Extends)
                .build()
            {
                relationships.supertraits.push(extends_ref);
            }
        }
    }

    // Add type references for USES relationships
    relationships.uses_types = type_refs;

    // Generate entity_id
    let file_path_str = file_path
        .to_str()
        .ok_or_else(|| codesearch_core::error::Error::entity_extraction("Invalid file path"))?;
    let entity_id = generate_entity_id(repository_id, file_path_str, &full_qualified_name);

    // Build entity
    let entity = CodeEntityBuilder::default()
        .entity_id(entity_id)
        .repository_id(repository_id.to_string())
        .name(name)
        .qualified_name(full_qualified_name)
        .parent_scope(if parent_scope.is_empty() {
            None
        } else {
            Some(parent_scope)
        })
        .entity_type(EntityType::Interface)
        .location(SourceLocation::from_tree_sitter_node(interface_node))
        .visibility(Some(Visibility::Public))
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
        .relationships(relationships)
        .build()
        .map_err(|e| {
            codesearch_core::error::Error::entity_extraction(format!(
                "Failed to build CodeEntity: {e}"
            ))
        })?;

    Ok(vec![entity])
}

/// Handle type alias declarations
#[allow(unused_variables)]
#[allow(clippy::too_many_arguments)]
pub fn handle_type_alias_impl(
    query_match: &QueryMatch,
    query: &Query,
    source: &str,
    file_path: &Path,
    repository_id: &str,
    package_name: Option<&str>,
    source_root: Option<&Path>,
    _repo_root: &Path,
) -> Result<Vec<CodeEntity>> {
    let type_alias_node = require_capture_node(query_match, query, "type_alias")?;

    // Extract name
    let name_node = require_capture_node(query_match, query, "name")?;
    let name = node_to_text(name_node, source)?;

    // Build qualified name
    let scope_result =
        crate::qualified_name::build_qualified_name_from_ast(type_alias_node, source, "typescript");
    let parent_scope = scope_result.parent_scope;
    let full_qualified_name = if parent_scope.is_empty() {
        name.clone()
    } else {
        format!("{parent_scope}.{name}")
    };

    // Extract generics
    let generics = extract_generics(type_alias_node, source)?;

    // Extract type value from the node itself
    let type_value = extract_type_value(type_alias_node, source)?;

    // Derive module path for qualified name resolution
    let module_path = source_root.and_then(|root| derive_module_path(file_path, root));

    // Build import map for type resolution
    let root = get_ast_root(type_alias_node);
    let import_map = parse_file_imports(root, source, Language::TypeScript, module_path.as_deref());

    // Extract type references used in the type alias
    let type_refs = extract_type_references(
        type_alias_node,
        source,
        &import_map,
        if parent_scope.is_empty() {
            None
        } else {
            Some(parent_scope.as_str())
        },
    );

    // Extract JSDoc documentation
    let documentation = extract_jsdoc_comments(type_alias_node, source);

    // Build metadata
    let mut metadata = EntityMetadata::default();
    if let Some(type_text) = type_value {
        metadata
            .attributes
            .insert("type_value".to_string(), type_text);
    }

    // Build relationship data with uses_types
    let relationships = EntityRelationshipData {
        uses_types: type_refs,
        ..Default::default()
    };

    // Generate entity_id
    let file_path_str = file_path
        .to_str()
        .ok_or_else(|| codesearch_core::error::Error::entity_extraction("Invalid file path"))?;
    let entity_id = generate_entity_id(repository_id, file_path_str, &full_qualified_name);

    // Build entity
    let entity = CodeEntityBuilder::default()
        .entity_id(entity_id)
        .repository_id(repository_id.to_string())
        .name(name)
        .qualified_name(full_qualified_name)
        .parent_scope(if parent_scope.is_empty() {
            None
        } else {
            Some(parent_scope)
        })
        .entity_type(EntityType::TypeAlias)
        .location(SourceLocation::from_tree_sitter_node(type_alias_node))
        .visibility(Some(Visibility::Public))
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
        .relationships(relationships)
        .build()
        .map_err(|e| {
            codesearch_core::error::Error::entity_extraction(format!(
                "Failed to build CodeEntity: {e}"
            ))
        })?;

    Ok(vec![entity])
}

/// Handle enum declarations
#[allow(unused_variables)]
#[allow(clippy::too_many_arguments)]
pub fn handle_enum_impl(
    query_match: &QueryMatch,
    query: &Query,
    source: &str,
    file_path: &Path,
    repository_id: &str,
    package_name: Option<&str>,
    source_root: Option<&Path>,
    _repo_root: &Path,
) -> Result<Vec<CodeEntity>> {
    let enum_node = require_capture_node(query_match, query, "enum")?;

    // Extract name
    let name_node = require_capture_node(query_match, query, "name")?;
    let name = node_to_text(name_node, source)?;

    // Build qualified name
    let scope_result =
        crate::qualified_name::build_qualified_name_from_ast(enum_node, source, "typescript");
    let parent_scope = scope_result.parent_scope;
    let full_qualified_name = if parent_scope.is_empty() {
        name.clone()
    } else {
        format!("{parent_scope}.{name}")
    };

    // Extract enum members with their values
    let member_info = extract_enum_member_info(enum_node, source)?;

    // Extract JSDoc documentation
    let documentation = extract_jsdoc_comments(enum_node, source);

    // Build metadata (no longer storing members as JSON)
    let metadata = EntityMetadata::default();

    // Generate entity_id
    let file_path_str = file_path
        .to_str()
        .ok_or_else(|| codesearch_core::error::Error::entity_extraction("Invalid file path"))?;
    let entity_id = generate_entity_id(repository_id, file_path_str, &full_qualified_name);

    // Build enum entity
    let enum_entity = CodeEntityBuilder::default()
        .entity_id(entity_id)
        .repository_id(repository_id.to_string())
        .name(name.clone())
        .qualified_name(full_qualified_name.clone())
        .parent_scope(if parent_scope.is_empty() {
            None
        } else {
            Some(parent_scope)
        })
        .entity_type(EntityType::Enum)
        .location(SourceLocation::from_tree_sitter_node(enum_node))
        .visibility(Some(Visibility::Public))
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

    // Build member entities
    let member_entities =
        build_enum_member_entities(&member_info, &full_qualified_name, file_path, repository_id)?;

    // Return enum + members
    let mut entities = vec![enum_entity];
    entities.extend(member_entities);
    Ok(entities)
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

/// Extract extends clause from a node (returns the full text)
fn extract_extends_clause(node: Node, source: &str) -> Result<Option<String>> {
    for child in node.children(&mut node.walk()) {
        if child.kind() == "extends_clause" || child.kind() == "class_heritage" {
            return Ok(Some(node_to_text(child, source)?));
        }
    }
    Ok(None)
}

/// Extract individual type names from extends clause
///
/// For interfaces: `interface Foo extends Bar, Baz` -> ["Bar", "Baz"]
/// For classes: `class Foo extends Bar` -> ["Bar"]
fn extract_extends_types(node: Node, source: &str) -> Result<Vec<String>> {
    let mut types = Vec::new();

    for child in node.children(&mut node.walk()) {
        if child.kind() == "extends_clause" || child.kind() == "extends_type_clause" {
            // Look for type identifiers within the extends clause
            for type_child in child.named_children(&mut child.walk()) {
                match type_child.kind() {
                    "type_identifier" => {
                        let type_name = node_to_text(type_child, source)?;
                        if !is_ts_primitive(&type_name) {
                            types.push(type_name);
                        }
                    }
                    "generic_type" => {
                        // Extract base type from generic like `Array<T>`
                        if let Some(base) = type_child.child_by_field_name("name") {
                            let type_name = node_to_text(base, source)?;
                            if !is_ts_primitive(&type_name) {
                                types.push(type_name);
                            }
                        }
                    }
                    "nested_type_identifier" => {
                        // Qualified type like `Namespace.Type`
                        types.push(node_to_text(type_child, source)?);
                    }
                    _ => {}
                }
            }
        }
    }

    Ok(types)
}

/// Extract individual type names from implements clause (TypeScript classes)
///
/// For classes: `class Foo implements IBar, IBaz` -> ["IBar", "IBaz"]
fn extract_implements_types(node: Node, source: &str) -> Result<Vec<String>> {
    let mut types = Vec::new();

    for child in node.children(&mut node.walk()) {
        // In TypeScript AST, implements clause might be in class_heritage
        if child.kind() == "implements_clause" || child.kind() == "class_heritage" {
            for type_child in child.named_children(&mut child.walk()) {
                match type_child.kind() {
                    "type_identifier" => {
                        let type_name = node_to_text(type_child, source)?;
                        if !is_ts_primitive(&type_name) {
                            types.push(type_name);
                        }
                    }
                    "generic_type" => {
                        // Extract base type from generic like `IHandler<T>`
                        if let Some(base) = type_child.child_by_field_name("name") {
                            let type_name = node_to_text(base, source)?;
                            if !is_ts_primitive(&type_name) {
                                types.push(type_name);
                            }
                        }
                    }
                    "nested_type_identifier" => {
                        // Qualified type like `Namespace.IType`
                        types.push(node_to_text(type_child, source)?);
                    }
                    _ => {}
                }
            }
        }
    }

    Ok(types)
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

/// Information about a TypeScript enum member
struct EnumMemberInfo {
    name: String,
    value: Option<String>,
    location: SourceLocation,
}

/// Extract enum members with their values from enum node
fn extract_enum_member_info(enum_node: Node, source: &str) -> Result<Vec<EnumMemberInfo>> {
    let mut members = Vec::new();

    // Find the enum_body child
    for child in enum_node.children(&mut enum_node.walk()) {
        if child.kind() == "enum_body" {
            for member in child.named_children(&mut child.walk()) {
                match member.kind() {
                    "enum_assignment" => {
                        // Member with explicit value: `Foo = 1`
                        if let Some(name_node) = member.child_by_field_name("name") {
                            let name = node_to_text(name_node, source)?;
                            let value = member
                                .child_by_field_name("value")
                                .and_then(|v| node_to_text(v, source).ok());
                            members.push(EnumMemberInfo {
                                name,
                                value,
                                location: SourceLocation::from_tree_sitter_node(member),
                            });
                        }
                    }
                    "property_identifier" => {
                        // Member without value: `Foo`
                        let name = node_to_text(member, source)?;
                        members.push(EnumMemberInfo {
                            name,
                            value: None,
                            location: SourceLocation::from_tree_sitter_node(member),
                        });
                    }
                    _ => {}
                }
            }
        }
    }

    Ok(members)
}

/// Build EnumVariant entities for TypeScript enum members
fn build_enum_member_entities(
    members: &[EnumMemberInfo],
    parent_qualified_name: &str,
    file_path: &Path,
    repository_id: &str,
) -> Result<Vec<CodeEntity>> {
    let file_path_str = file_path
        .to_str()
        .ok_or_else(|| codesearch_core::error::Error::entity_extraction("Invalid file path"))?;

    members
        .iter()
        .map(|member| {
            let qualified_name = format!("{parent_qualified_name}.{}", member.name);
            let entity_id = generate_entity_id(repository_id, file_path_str, &qualified_name);

            // Build content representation
            let content = match &member.value {
                Some(val) => format!("{} = {val}", member.name),
                None => member.name.clone(),
            };

            // Build metadata with value if present
            let mut metadata = EntityMetadata::default();
            if let Some(val) = &member.value {
                metadata.attributes.insert("value".to_string(), val.clone());
            }

            CodeEntityBuilder::default()
                .entity_id(entity_id)
                .repository_id(repository_id.to_string())
                .name(member.name.clone())
                .qualified_name(qualified_name)
                .parent_scope(Some(parent_qualified_name.to_string()))
                .entity_type(EntityType::EnumVariant)
                .location(member.location.clone())
                .visibility(None) // Members inherit visibility from parent
                .content(Some(content))
                .metadata(metadata)
                .language(Language::TypeScript)
                .file_path(file_path.to_path_buf())
                .build()
                .map_err(|e| {
                    codesearch_core::error::Error::entity_extraction(format!(
                        "Failed to build EnumVariant entity: {e}"
                    ))
                })
        })
        .collect()
}
