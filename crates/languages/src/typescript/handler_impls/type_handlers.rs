//! TypeScript type entity handler implementations

#![deny(warnings)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]

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
    _package_name: Option<&str>,
    source_root: Option<&Path>,
    repo_root: &Path,
) -> Result<Vec<CodeEntity>> {
    // Reuse JavaScript class handler (includes extends_resolved)
    // Note: TypeScript qualified names are based on file paths only, not package names
    // per spec rule Q-MODULE-FILE and Q-ITEM-MODULE
    let mut entities = crate::javascript::handler_impls::handle_class_impl(
        query_match,
        query,
        source,
        file_path,
        repository_id,
        None, // TypeScript doesn't use package name in qualified names
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
            match SourceReference::builder()
                .target(resolved)
                .simple_name(type_name.clone())
                .is_external(false) // TS doesn't track external refs yet
                .location(SourceLocation::default())
                .ref_type(ReferenceType::Implements)
                .build()
            {
                Ok(ref_) => Some(ref_),
                Err(e) => {
                    tracing::warn!(type_name = %type_name, "Failed to build implements reference: {e}");
                    None
                }
            }
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
    _package_name: Option<&str>,
    source_root: Option<&Path>,
    repo_root: &Path,
) -> Result<Vec<CodeEntity>> {
    // Get the method node to extract TypeScript-specific modifiers
    let method_node = require_capture_node(query_match, query, "method")?;

    // Reuse JavaScript method handler
    // Note: TypeScript qualified names are based on file paths only, not package names
    // per spec rule Q-MODULE-FILE and Q-ITEM-MODULE
    let mut entities = crate::javascript::handler_impls::handle_method_impl(
        query_match,
        query,
        source,
        file_path,
        repository_id,
        None, // TypeScript doesn't use package name in qualified names
        source_root,
        repo_root,
    )?;

    // Extract TypeScript-specific visibility from accessibility modifier
    let visibility = extract_method_visibility(method_node);

    // Update language and add TypeScript-specific properties
    for entity in &mut entities {
        entity.language = Language::TypeScript;
        entity.visibility = Some(visibility);
    }

    Ok(entities)
}

/// Extract visibility from a method node
fn extract_method_visibility(method_node: Node) -> Visibility {
    for child in method_node.children(&mut method_node.walk()) {
        if child.kind() == "accessibility_modifier" {
            // Look at the actual modifier keyword inside
            if let Some(modifier_child) = child.children(&mut child.walk()).next() {
                return match modifier_child.kind() {
                    "private" => Visibility::Private,
                    "protected" => Visibility::Protected,
                    "public" => Visibility::Public,
                    _ => Visibility::Public,
                };
            }
        }
    }
    // Default visibility for methods is public in TypeScript
    Visibility::Public
}

/// Handle class field/property declarations
///
/// Extracts Property entities from `public_field_definition` nodes.
/// Supports visibility modifiers (public, private, protected) and readonly.
#[allow(clippy::too_many_arguments)]
pub fn handle_field_impl(
    query_match: &QueryMatch,
    query: &Query,
    source: &str,
    file_path: &Path,
    repository_id: &str,
    _package_name: Option<&str>,
    source_root: Option<&Path>,
    repo_root: &Path,
) -> Result<Vec<CodeEntity>> {
    let field_node = require_capture_node(query_match, query, "field")?;
    let name_node = require_capture_node(query_match, query, "name")?;

    // Get the field name - handle both regular and private fields
    let name = node_to_text(name_node, source)?;

    // Derive module path
    let module_path = source_root
        .and_then(|root| derive_module_path(file_path, root))
        .or_else(|| derive_module_path(file_path, repo_root));

    // Build qualified name from AST (includes class parent scope)
    let scope_result =
        crate::qualified_name::build_qualified_name_from_ast(field_node, source, "typescript");

    // Compose full qualified name: module.class.field
    let full_qualified_name = match (&module_path, scope_result.parent_scope.is_empty()) {
        (Some(module), false) => format!("{module}.{}.{name}", scope_result.parent_scope),
        (Some(module), true) => format!("{module}.{name}"),
        (None, false) => format!("{}.{name}", scope_result.parent_scope),
        (None, true) => name.clone(),
    };

    // Parent scope includes module path
    let parent_scope = match (&module_path, scope_result.parent_scope.is_empty()) {
        (Some(module), false) => format!("{module}.{}", scope_result.parent_scope),
        (Some(module), true) => module.clone(),
        (None, false) => scope_result.parent_scope.clone(),
        (None, true) => String::new(),
    };

    // Extract visibility from field node
    let visibility = extract_field_visibility(field_node);

    // Check for readonly modifier
    let is_readonly = field_node
        .children(&mut field_node.walk())
        .any(|c| c.kind() == "readonly");

    // Check for optional marker (?)
    let is_optional = field_node
        .children(&mut field_node.walk())
        .any(|c| c.kind() == "?");

    // Check for static modifier
    let is_static = field_node
        .children(&mut field_node.walk())
        .any(|c| c.kind() == "static");

    // Build metadata
    let mut metadata = EntityMetadata::default();
    if is_readonly {
        metadata
            .attributes
            .insert("readonly".to_string(), "true".to_string());
    }
    if is_optional {
        metadata
            .attributes
            .insert("optional".to_string(), "true".to_string());
    }
    if is_static {
        metadata
            .attributes
            .insert("static".to_string(), "true".to_string());
    }

    // Generate entity ID
    let file_path_str = file_path
        .to_str()
        .ok_or_else(|| codesearch_core::error::Error::entity_extraction("Invalid file path"))?;
    let entity_id = generate_entity_id(repository_id, file_path_str, &full_qualified_name);

    // Build the entity
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
        .entity_type(EntityType::Property)
        .location(SourceLocation::from_tree_sitter_node(field_node))
        .visibility(Some(visibility))
        .content(Some(node_to_text(field_node, source)?))
        .metadata(metadata)
        .language(Language::TypeScript)
        .file_path(file_path.to_path_buf())
        .build()
        .map_err(|e| {
            codesearch_core::error::Error::entity_extraction(format!(
                "Failed to build Property entity: {e}"
            ))
        })?;

    Ok(vec![entity])
}

/// Extract visibility from a field node
fn extract_field_visibility(field_node: Node) -> Visibility {
    for child in field_node.children(&mut field_node.walk()) {
        if child.kind() == "accessibility_modifier" {
            // Look at the actual modifier keyword inside
            if let Some(modifier_child) = child.children(&mut child.walk()).next() {
                return match modifier_child.kind() {
                    "private" => Visibility::Private,
                    "protected" => Visibility::Protected,
                    "public" => Visibility::Public,
                    _ => Visibility::Public,
                };
            }
        }
        // Private property identifiers (#name) are always private
        if child.kind() == "private_property_identifier" {
            return Visibility::Private;
        }
    }
    // Default visibility for class fields is public in TypeScript
    Visibility::Public
}

/// Handle interface property signatures
///
/// Extracts Property entities from `property_signature` nodes in interfaces.
#[allow(clippy::too_many_arguments)]
pub fn handle_interface_property_impl(
    query_match: &QueryMatch,
    query: &Query,
    source: &str,
    file_path: &Path,
    repository_id: &str,
    _package_name: Option<&str>,
    source_root: Option<&Path>,
    repo_root: &Path,
) -> Result<Vec<CodeEntity>> {
    let property_node = require_capture_node(query_match, query, "property")?;
    let name_node = require_capture_node(query_match, query, "name")?;
    let name = node_to_text(name_node, source)?;

    // Derive module path
    let module_path = source_root
        .and_then(|root| derive_module_path(file_path, root))
        .or_else(|| derive_module_path(file_path, repo_root));

    // Build qualified name from AST (includes interface parent scope)
    let scope_result =
        crate::qualified_name::build_qualified_name_from_ast(property_node, source, "typescript");

    // Compose full qualified name: module.interface.property
    let full_qualified_name = match (&module_path, scope_result.parent_scope.is_empty()) {
        (Some(module), false) => format!("{module}.{}.{name}", scope_result.parent_scope),
        (Some(module), true) => format!("{module}.{name}"),
        (None, false) => format!("{}.{name}", scope_result.parent_scope),
        (None, true) => name.clone(),
    };

    // Parent scope includes module path
    let parent_scope = match (&module_path, scope_result.parent_scope.is_empty()) {
        (Some(module), false) => format!("{module}.{}", scope_result.parent_scope),
        (Some(module), true) => module.clone(),
        (None, false) => scope_result.parent_scope.clone(),
        (None, true) => String::new(),
    };

    // Check for readonly modifier
    let is_readonly = property_node
        .children(&mut property_node.walk())
        .any(|c| c.kind() == "readonly");

    // Check for optional marker (?)
    let is_optional = property_node
        .children(&mut property_node.walk())
        .any(|c| c.kind() == "?");

    // Build metadata
    let mut metadata = EntityMetadata::default();
    if is_readonly {
        metadata
            .attributes
            .insert("readonly".to_string(), "true".to_string());
    }
    if is_optional {
        metadata
            .attributes
            .insert("optional".to_string(), "true".to_string());
    }

    // Generate entity ID
    let file_path_str = file_path
        .to_str()
        .ok_or_else(|| codesearch_core::error::Error::entity_extraction("Invalid file path"))?;
    let entity_id = generate_entity_id(repository_id, file_path_str, &full_qualified_name);

    // Interface members are always public
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
        .entity_type(EntityType::Property)
        .location(SourceLocation::from_tree_sitter_node(property_node))
        .visibility(Some(Visibility::Public))
        .content(Some(node_to_text(property_node, source)?))
        .metadata(metadata)
        .language(Language::TypeScript)
        .file_path(file_path.to_path_buf())
        .build()
        .map_err(|e| {
            codesearch_core::error::Error::entity_extraction(format!(
                "Failed to build interface Property entity: {e}"
            ))
        })?;

    Ok(vec![entity])
}

/// Handle interface method signatures
///
/// Extracts Method entities from `method_signature` nodes in interfaces.
#[allow(clippy::too_many_arguments)]
pub fn handle_interface_method_impl(
    query_match: &QueryMatch,
    query: &Query,
    source: &str,
    file_path: &Path,
    repository_id: &str,
    _package_name: Option<&str>,
    source_root: Option<&Path>,
    repo_root: &Path,
) -> Result<Vec<CodeEntity>> {
    let method_node = require_capture_node(query_match, query, "method")?;
    let name_node = require_capture_node(query_match, query, "name")?;
    let name = node_to_text(name_node, source)?;

    // Derive module path
    let module_path = source_root
        .and_then(|root| derive_module_path(file_path, root))
        .or_else(|| derive_module_path(file_path, repo_root));

    // Build qualified name from AST (includes interface parent scope)
    let scope_result =
        crate::qualified_name::build_qualified_name_from_ast(method_node, source, "typescript");

    // Compose full qualified name: module.interface.method
    let full_qualified_name = match (&module_path, scope_result.parent_scope.is_empty()) {
        (Some(module), false) => format!("{module}.{}.{name}", scope_result.parent_scope),
        (Some(module), true) => format!("{module}.{name}"),
        (None, false) => format!("{}.{name}", scope_result.parent_scope),
        (None, true) => name.clone(),
    };

    // Parent scope includes module path
    let parent_scope = match (&module_path, scope_result.parent_scope.is_empty()) {
        (Some(module), false) => format!("{module}.{}", scope_result.parent_scope),
        (Some(module), true) => module.clone(),
        (None, false) => scope_result.parent_scope.clone(),
        (None, true) => String::new(),
    };

    // Generate entity ID
    let file_path_str = file_path
        .to_str()
        .ok_or_else(|| codesearch_core::error::Error::entity_extraction("Invalid file path"))?;
    let entity_id = generate_entity_id(repository_id, file_path_str, &full_qualified_name);

    let metadata = EntityMetadata::default();

    // Interface methods are always public
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
        .entity_type(EntityType::Method)
        .location(SourceLocation::from_tree_sitter_node(method_node))
        .visibility(Some(Visibility::Public))
        .content(Some(node_to_text(method_node, source)?))
        .metadata(metadata)
        .language(Language::TypeScript)
        .file_path(file_path.to_path_buf())
        .build()
        .map_err(|e| {
            codesearch_core::error::Error::entity_extraction(format!(
                "Failed to build interface Method entity: {e}"
            ))
        })?;

    Ok(vec![entity])
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
    repo_root: &Path,
) -> Result<Vec<CodeEntity>> {
    let interface_node = require_capture_node(query_match, query, "interface")?;

    // Extract name
    let name_node = require_capture_node(query_match, query, "name")?;
    let name = node_to_text(name_node, source)?;

    // Derive module path from file path (for TypeScript, qualified names are file-based per Q-MODULE-FILE)
    let module_path = source_root
        .and_then(|root| derive_module_path(file_path, root))
        .or_else(|| derive_module_path(file_path, repo_root));

    // Build qualified name from AST (for any parent scope like namespace)
    let scope_result =
        crate::qualified_name::build_qualified_name_from_ast(interface_node, source, "typescript");
    let ast_scope = if scope_result.parent_scope.is_empty() {
        None
    } else {
        Some(scope_result.parent_scope.clone())
    };

    // Compose qualified name: module.ast_scope.name (per TypeScript spec Q-ITEM-MODULE)
    let full_qualified_name = match (&module_path, &ast_scope) {
        (Some(module), Some(scope)) => format!("{module}.{scope}.{name}"),
        (Some(module), None) => format!("{module}.{name}"),
        (None, Some(scope)) => format!("{scope}.{name}"),
        (None, None) => name.clone(),
    };

    // Parent scope includes module path
    let parent_scope = match (&module_path, &ast_scope) {
        (Some(module), Some(scope)) => format!("{module}.{scope}"),
        (Some(module), None) => module.clone(),
        (None, Some(scope)) => scope.clone(),
        (None, None) => String::new(),
    };

    // Extract generics (type_parameters)
    let generics = extract_generics(interface_node, source)?;

    // Build import map for type resolution (reuse module_path from above)
    let root = get_ast_root(interface_node);
    let import_map = parse_file_imports(root, source, Language::TypeScript, module_path.as_deref());

    // Extract extended interfaces (raw names) for EXTENDS_INTERFACE relationships
    let extends_types = extract_extends_types(interface_node, source)?;
    let extends_names: std::collections::HashSet<&str> =
        extends_types.iter().map(|s| s.as_str()).collect();

    // Extract type references used in the interface body
    // Filter out types that are in the extends clause to avoid USES relationships for them
    let type_refs: Vec<SourceReference> = extract_type_references(
        interface_node,
        source,
        &import_map,
        if parent_scope.is_empty() {
            None
        } else {
            Some(parent_scope.as_str())
        },
    )
    .into_iter()
    .filter(|r| !extends_names.contains(r.simple_name()))
    .collect();

    // Extract JSDoc documentation
    let documentation = extract_jsdoc_comments(interface_node, source);

    // Build metadata
    let metadata = EntityMetadata::default();

    // Build relationship data
    let mut relationships = EntityRelationshipData::default();

    // Build supertraits (interface extends interface = EXTENDS_INTERFACE)
    for type_name in &extends_types {
        let resolved = resolve_reference(
            type_name,
            &import_map,
            if parent_scope.is_empty() {
                None
            } else {
                Some(parent_scope.as_str())
            },
            ".",
        );
        match SourceReference::builder()
            .target(resolved)
            .simple_name(type_name.clone())
            .is_external(false)
            .location(SourceLocation::default())
            .ref_type(ReferenceType::Extends)
            .build()
        {
            Ok(extends_ref) => relationships.supertraits.push(extends_ref),
            Err(e) => {
                tracing::warn!(type_name = %type_name, "Failed to build extends reference: {e}");
            }
        }
    }

    // Add type references for USES relationships (excludes extends types)
    relationships.uses_types = type_refs;

    // Generate entity_id
    let file_path_str = file_path
        .to_str()
        .ok_or_else(|| codesearch_core::error::Error::entity_extraction("Invalid file path"))?;
    let entity_id = generate_entity_id(repository_id, file_path_str, &full_qualified_name);

    // Check if exported
    let is_exported = is_node_exported(interface_node);
    let visibility = if is_exported {
        Visibility::Public
    } else {
        Visibility::Private
    };

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
        .visibility(Some(visibility))
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
    repo_root: &Path,
) -> Result<Vec<CodeEntity>> {
    let type_alias_node = require_capture_node(query_match, query, "type_alias")?;

    // Extract name
    let name_node = require_capture_node(query_match, query, "name")?;
    let name = node_to_text(name_node, source)?;

    // Derive module path from file path (for TypeScript, qualified names are file-based per Q-MODULE-FILE)
    let module_path = source_root
        .and_then(|root| derive_module_path(file_path, root))
        .or_else(|| derive_module_path(file_path, repo_root));

    // Build qualified name from AST (for any parent scope like namespace)
    let scope_result =
        crate::qualified_name::build_qualified_name_from_ast(type_alias_node, source, "typescript");
    let ast_scope = if scope_result.parent_scope.is_empty() {
        None
    } else {
        Some(scope_result.parent_scope.clone())
    };

    // Compose qualified name: module.ast_scope.name (per TypeScript spec Q-ITEM-MODULE)
    let full_qualified_name = match (&module_path, &ast_scope) {
        (Some(module), Some(scope)) => format!("{module}.{scope}.{name}"),
        (Some(module), None) => format!("{module}.{name}"),
        (None, Some(scope)) => format!("{scope}.{name}"),
        (None, None) => name.clone(),
    };

    // Parent scope includes module path
    let parent_scope = match (&module_path, &ast_scope) {
        (Some(module), Some(scope)) => format!("{module}.{scope}"),
        (Some(module), None) => module.clone(),
        (None, Some(scope)) => scope.clone(),
        (None, None) => String::new(),
    };

    // Extract generics
    let generics = extract_generics(type_alias_node, source)?;

    // Extract type value from the node itself
    let type_value = extract_type_value(type_alias_node, source)?;

    // Build import map for type resolution (reuse module_path from above)
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

    // Check if exported
    let is_exported = is_node_exported(type_alias_node);
    let visibility = if is_exported {
        Visibility::Public
    } else {
        Visibility::Private
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
        .visibility(Some(visibility))
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
    repo_root: &Path,
) -> Result<Vec<CodeEntity>> {
    let enum_node = require_capture_node(query_match, query, "enum")?;

    // Extract name
    let name_node = require_capture_node(query_match, query, "name")?;
    let name = node_to_text(name_node, source)?;

    // Derive module path from file path (for TypeScript, qualified names are file-based per Q-MODULE-FILE)
    let module_path = source_root
        .and_then(|root| derive_module_path(file_path, root))
        .or_else(|| derive_module_path(file_path, repo_root));

    // Build qualified name from AST (for any parent scope like namespace)
    let scope_result =
        crate::qualified_name::build_qualified_name_from_ast(enum_node, source, "typescript");
    let ast_scope = if scope_result.parent_scope.is_empty() {
        None
    } else {
        Some(scope_result.parent_scope.clone())
    };

    // Compose qualified name: module.ast_scope.name (per TypeScript spec Q-ITEM-MODULE)
    let full_qualified_name = match (&module_path, &ast_scope) {
        (Some(module), Some(scope)) => format!("{module}.{scope}.{name}"),
        (Some(module), None) => format!("{module}.{name}"),
        (None, Some(scope)) => format!("{scope}.{name}"),
        (None, None) => name.clone(),
    };

    // Parent scope includes module path
    let parent_scope = match (&module_path, &ast_scope) {
        (Some(module), Some(scope)) => format!("{module}.{scope}"),
        (Some(module), None) => module.clone(),
        (None, Some(scope)) => scope.clone(),
        (None, None) => String::new(),
    };

    // Extract enum members with their values
    let member_info = extract_enum_member_info(enum_node, source)?;

    // Extract JSDoc documentation
    let documentation = extract_jsdoc_comments(enum_node, source);

    // Check if exported
    let is_exported = is_node_exported(enum_node);
    let visibility = if is_exported {
        Visibility::Public
    } else {
        Visibility::Private
    };

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
        .visibility(Some(visibility))
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

    // Build member entities (passing parent visibility for inheritance)
    let member_entities = build_enum_member_entities(
        &member_info,
        &full_qualified_name,
        file_path,
        repository_id,
        Some(visibility), // Members inherit visibility from parent enum
    )?;

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
                    _ => {
                        tracing::trace!(kind = type_child.kind(), "Unhandled extends type node");
                    }
                }
            }
        }
    }

    Ok(types)
}

/// Extract individual type names from implements clause (TypeScript classes)
///
/// For classes: `class Foo implements IBar, IBaz` -> ["IBar", "IBaz"]
///
/// Tree structure:
/// ```text
/// class_declaration
///   class_heritage
///     implements_clause
///       type_identifier (IBar)
///       type_identifier (IBaz)
/// ```
fn extract_implements_types(node: Node, source: &str) -> Result<Vec<String>> {
    let mut types = Vec::new();

    // Helper to extract type identifiers from a node
    fn extract_types_from_clause(clause: Node, source: &str, types: &mut Vec<String>) {
        for type_child in clause.named_children(&mut clause.walk()) {
            match type_child.kind() {
                "type_identifier" => {
                    if let Ok(type_name) = node_to_text(type_child, source) {
                        if !is_ts_primitive(&type_name) {
                            types.push(type_name);
                        }
                    }
                }
                "generic_type" => {
                    // Extract base type from generic like `IHandler<T>`
                    if let Some(base) = type_child.child_by_field_name("name") {
                        if let Ok(type_name) = node_to_text(base, source) {
                            if !is_ts_primitive(&type_name) {
                                types.push(type_name);
                            }
                        }
                    }
                }
                "nested_type_identifier" => {
                    // Qualified type like `Namespace.IType`
                    if let Ok(type_name) = node_to_text(type_child, source) {
                        types.push(type_name);
                    }
                }
                _ => {
                    tracing::trace!(kind = type_child.kind(), "Unhandled implements type node");
                }
            }
        }
    }

    for child in node.children(&mut node.walk()) {
        match child.kind() {
            // Direct implements_clause at class level
            "implements_clause" => {
                extract_types_from_clause(child, source, &mut types);
            }
            // class_heritage wraps implements_clause
            "class_heritage" => {
                for heritage_child in child.named_children(&mut child.walk()) {
                    if heritage_child.kind() == "implements_clause" {
                        extract_types_from_clause(heritage_child, source, &mut types);
                    }
                }
            }
            _ => {}
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
                    _ => {
                        tracing::trace!(kind = member.kind(), "Unhandled enum member node");
                    }
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
    parent_visibility: Option<Visibility>,
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
                .visibility(parent_visibility) // Members inherit visibility from parent
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

/// Check if a node is exported (has an export_statement ancestor)
fn is_node_exported(node: Node) -> bool {
    let mut current = Some(node);
    while let Some(n) = current {
        if n.kind() == "export_statement" {
            return true;
        }
        current = n.parent();
    }
    false
}

/// Test-only wrapper for extract_implements_types
#[cfg(test)]
pub fn test_extract_implements_types(
    node: tree_sitter::Node,
    source: &str,
) -> codesearch_core::error::Result<Vec<String>> {
    extract_implements_types(node, source)
}
