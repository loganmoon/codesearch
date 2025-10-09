//! Handler for extracting Rust impl blocks and their methods
//!
//! This module processes tree-sitter query matches for Rust impl blocks
//! (both inherent and trait implementations) and extracts both the impl
//! block itself and all methods within it as separate entities.

#![deny(warnings)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]

use crate::qualified_name::build_qualified_name_from_ast;
use crate::rust::handlers::common::{
    extract_function_modifiers, extract_function_parameters, extract_generics_from_node,
    extract_preceding_doc_comments, find_capture_node, node_to_text, require_capture_node,
};
use crate::rust::handlers::constants::{capture_names, node_kinds, special_idents};
use codesearch_core::entities::{
    CodeEntityBuilder, EntityMetadata, EntityType, FunctionSignature, Language, SourceLocation,
    Visibility,
};
use codesearch_core::entity_id::generate_entity_id;
use codesearch_core::error::{Error, Result};
use codesearch_core::CodeEntity;
use std::path::Path;
use tree_sitter::{Node, Query, QueryMatch};

/// Process an inherent impl block query match and extract entities
pub fn handle_impl(
    query_match: &QueryMatch,
    query: &Query,
    source: &str,
    file_path: &Path,
    repository_id: &str,
) -> Result<Vec<CodeEntity>> {
    let impl_node = require_capture_node(query_match, query, capture_names::IMPL)?;

    // Extract the type this impl is for
    let for_type = find_capture_node(query_match, query, capture_names::TYPE)
        .and_then(|node| node_to_text(node, source).ok())
        .unwrap_or_else(|| special_idents::ANONYMOUS.to_string());

    // Extract generics
    let generics = find_capture_node(query_match, query, capture_names::GENERICS)
        .map(|node| extract_generics_from_node(node, source))
        .unwrap_or_default();

    // Build qualified name for the impl block
    let parent_scope = build_qualified_name_from_ast(impl_node, source, "rust");
    let impl_qualified_name = if parent_scope.is_empty() {
        for_type.clone()
    } else {
        format!("{parent_scope}::{for_type}")
    };

    // Extract all methods from impl body
    let impl_body = find_capture_node(query_match, query, capture_names::IMPL_BODY);
    let mut entities = Vec::new();

    if let Some(body_node) = impl_body {
        let methods = extract_impl_methods(
            body_node,
            source,
            file_path,
            repository_id,
            &impl_qualified_name,
            &for_type,
            None, // No trait for inherent impl
        )?;
        entities.extend(methods);
    }

    // Create the impl block entity itself
    let location = SourceLocation::from_tree_sitter_node(impl_node);
    let content = node_to_text(impl_node, source).ok();
    let documentation = extract_preceding_doc_comments(impl_node, source);

    let file_path_str = file_path
        .to_str()
        .ok_or_else(|| Error::entity_extraction("Invalid file path"))?;
    let entity_id = generate_entity_id(repository_id, file_path_str, &impl_qualified_name);

    let mut metadata = EntityMetadata {
        is_generic: !generics.is_empty(),
        generic_params: generics.clone(),
        ..Default::default()
    };

    metadata
        .attributes
        .insert("for_type".to_string(), for_type.clone());

    let impl_entity = CodeEntityBuilder::default()
        .entity_id(entity_id)
        .repository_id(repository_id.to_string())
        .name(for_type)
        .qualified_name(impl_qualified_name.clone())
        .parent_scope(if parent_scope.is_empty() {
            None
        } else {
            Some(parent_scope)
        })
        .entity_type(EntityType::Impl)
        .location(location)
        .visibility(Visibility::Private) // Impl blocks don't have visibility
        .documentation_summary(documentation)
        .content(content)
        .metadata(metadata)
        .language(Language::Rust)
        .file_path(file_path.to_path_buf())
        .build()
        .map_err(|e| Error::entity_extraction(format!("Failed to build impl entity: {e}")))?;

    // Insert impl block entity at the beginning
    entities.insert(0, impl_entity);

    Ok(entities)
}

/// Process a trait impl block query match and extract entities
pub fn handle_impl_trait(
    query_match: &QueryMatch,
    query: &Query,
    source: &str,
    file_path: &Path,
    repository_id: &str,
) -> Result<Vec<CodeEntity>> {
    let impl_node = require_capture_node(query_match, query, capture_names::IMPL_TRAIT)?;

    // Extract the type this impl is for
    let for_type = find_capture_node(query_match, query, capture_names::TYPE)
        .and_then(|node| node_to_text(node, source).ok())
        .unwrap_or_else(|| special_idents::ANONYMOUS.to_string());

    // Extract the trait being implemented
    let trait_name = find_capture_node(query_match, query, capture_names::TRAIT)
        .and_then(|node| node_to_text(node, source).ok())
        .unwrap_or_else(|| special_idents::ANONYMOUS.to_string());

    // Extract generics
    let generics = find_capture_node(query_match, query, capture_names::GENERICS)
        .map(|node| extract_generics_from_node(node, source))
        .unwrap_or_default();

    // Build qualified name: "Trait for Type" or parent::Trait for Type
    let parent_scope = build_qualified_name_from_ast(impl_node, source, "rust");
    let impl_qualified_name = if parent_scope.is_empty() {
        format!("{trait_name} for {for_type}")
    } else {
        format!("{parent_scope}::{trait_name} for {for_type}")
    };

    // Extract all methods from impl body
    let impl_body = find_capture_node(query_match, query, capture_names::IMPL_BODY);
    let mut entities = Vec::new();

    if let Some(body_node) = impl_body {
        let methods = extract_impl_methods(
            body_node,
            source,
            file_path,
            repository_id,
            &impl_qualified_name,
            &for_type,
            Some(&trait_name),
        )?;
        entities.extend(methods);
    }

    // Create the impl block entity itself
    let location = SourceLocation::from_tree_sitter_node(impl_node);
    let content = node_to_text(impl_node, source).ok();
    let documentation = extract_preceding_doc_comments(impl_node, source);

    let file_path_str = file_path
        .to_str()
        .ok_or_else(|| Error::entity_extraction("Invalid file path"))?;
    let entity_id = generate_entity_id(repository_id, file_path_str, &impl_qualified_name);

    let mut metadata = EntityMetadata {
        is_generic: !generics.is_empty(),
        generic_params: generics.clone(),
        ..Default::default()
    };

    metadata
        .attributes
        .insert("for_type".to_string(), for_type.clone());
    metadata
        .attributes
        .insert("implements_trait".to_string(), trait_name.clone());

    let impl_entity = CodeEntityBuilder::default()
        .entity_id(entity_id)
        .repository_id(repository_id.to_string())
        .name(format!("{trait_name} for {for_type}"))
        .qualified_name(impl_qualified_name.clone())
        .parent_scope(if parent_scope.is_empty() {
            None
        } else {
            Some(parent_scope)
        })
        .entity_type(EntityType::Impl)
        .location(location)
        .visibility(Visibility::Private) // Impl blocks don't have visibility
        .documentation_summary(documentation)
        .content(content)
        .metadata(metadata)
        .language(Language::Rust)
        .file_path(file_path.to_path_buf())
        .build()
        .map_err(|e| Error::entity_extraction(format!("Failed to build impl entity: {e}")))?;

    // Insert impl block entity at the beginning
    entities.insert(0, impl_entity);

    Ok(entities)
}

/// Extract all methods and associated constants from an impl block body
fn extract_impl_methods(
    body_node: Node,
    source: &str,
    file_path: &Path,
    repository_id: &str,
    impl_qualified_name: &str,
    for_type: &str,
    trait_name: Option<&str>,
) -> Result<Vec<CodeEntity>> {
    let mut entities = Vec::new();
    let mut cursor = body_node.walk();

    for child in body_node.children(&mut cursor) {
        match child.kind() {
            node_kinds::FUNCTION_ITEM => {
                if let Ok(method) = extract_method(
                    child,
                    source,
                    file_path,
                    repository_id,
                    impl_qualified_name,
                    for_type,
                    trait_name,
                ) {
                    entities.push(method);
                }
            }
            "const_item" => {
                if let Ok(constant) = extract_associated_constant(
                    child,
                    source,
                    file_path,
                    repository_id,
                    impl_qualified_name,
                    for_type,
                    trait_name,
                ) {
                    entities.push(constant);
                }
            }
            _ => {}
        }
    }

    Ok(entities)
}

/// Determine if a function should be typed as a Method
/// A function is a method if it has a self parameter OR returns Self
fn is_method(parameters: &[(String, String)], return_type: &Option<String>) -> bool {
    // Check for self parameter (any variant: self, &self, &mut self, mut self)
    let has_self_param = parameters.iter().any(|(name, _)| {
        name == "self" || name.starts_with("&self") || name.starts_with("mut self")
    });

    // Check for Self return type
    let returns_self = return_type
        .as_ref()
        .map(|rt| rt.contains("Self"))
        .unwrap_or(false);

    has_self_param || returns_self
}

/// Extract an associated constant from an impl block
fn extract_associated_constant(
    const_node: Node,
    source: &str,
    file_path: &Path,
    repository_id: &str,
    impl_qualified_name: &str,
    for_type: &str,
    trait_name: Option<&str>,
) -> Result<CodeEntity> {
    // Extract constant name
    let name = const_node
        .child_by_field_name("name")
        .and_then(|n| node_to_text(n, source).ok())
        .unwrap_or_else(|| special_idents::ANONYMOUS.to_string());

    // Build qualified name based on impl type
    let qualified_name = if let Some(trait_name) = trait_name {
        format!("<{for_type} as {trait_name}>::{name}")
    } else {
        format!("{for_type}::{name}")
    };

    // Extract visibility
    let visibility = extract_method_visibility(const_node);

    // Extract type
    let const_type = const_node
        .child_by_field_name("type")
        .and_then(|n| node_to_text(n, source).ok());

    // Extract value
    let value = const_node
        .child_by_field_name("value")
        .and_then(|n| node_to_text(n, source).ok());

    // Extract documentation
    let documentation = extract_preceding_doc_comments(const_node, source);

    // Get location and content
    let location = SourceLocation::from_tree_sitter_node(const_node);
    let content = node_to_text(const_node, source).ok();

    // Generate entity_id
    let file_path_str = file_path
        .to_str()
        .ok_or_else(|| Error::entity_extraction("Invalid file path"))?;
    let entity_id = generate_entity_id(repository_id, file_path_str, &qualified_name);

    // Build metadata
    let mut metadata = EntityMetadata {
        is_const: true,
        ..Default::default()
    };

    if let Some(const_type_str) = &const_type {
        metadata
            .attributes
            .insert("type".to_string(), const_type_str.clone());
    }

    if let Some(value_str) = &value {
        metadata
            .attributes
            .insert("value".to_string(), value_str.clone());
    }

    CodeEntityBuilder::default()
        .entity_id(entity_id)
        .repository_id(repository_id.to_string())
        .name(name)
        .qualified_name(qualified_name)
        .parent_scope(Some(impl_qualified_name.to_string()))
        .entity_type(EntityType::Constant)
        .location(location)
        .visibility(visibility)
        .documentation_summary(documentation)
        .content(content)
        .metadata(metadata)
        .language(Language::Rust)
        .file_path(file_path.to_path_buf())
        .build()
        .map_err(|e| Error::entity_extraction(format!("Failed to build constant entity: {e}")))
}

/// Find the function_modifiers node in a function_item node
fn find_modifiers_node(node: Node) -> Option<Node> {
    let mut cursor = node.walk();
    #[allow(clippy::manual_find)]
    for child in node.children(&mut cursor) {
        if child.kind() == "function_modifiers" {
            return Some(child);
        }
    }
    None
}

/// Find the parameters node in a function_item node
fn find_parameters_node(node: Node) -> Option<Node> {
    let mut cursor = node.walk();
    #[allow(clippy::manual_find)]
    for child in node.children(&mut cursor) {
        if child.kind() == "parameters" {
            return Some(child);
        }
    }
    None
}

/// Extract a single method from an impl block
fn extract_method(
    method_node: Node,
    source: &str,
    file_path: &Path,
    repository_id: &str,
    impl_qualified_name: &str,
    for_type: &str,
    trait_name: Option<&str>,
) -> Result<CodeEntity> {
    // Extract method name
    let name = find_method_name(method_node, source)
        .unwrap_or_else(|| special_idents::ANONYMOUS.to_string());

    // Build qualified name based on impl type
    let qualified_name = if let Some(trait_name) = trait_name {
        format!("<{for_type} as {trait_name}>::{name}")
    } else {
        format!("{for_type}::{name}")
    };

    // Extract visibility
    let visibility = extract_method_visibility(method_node);

    // Extract modifiers by finding the function_modifiers node
    let (is_async, is_unsafe, is_const) = find_modifiers_node(method_node)
        .map(extract_function_modifiers)
        .unwrap_or((false, false, false));

    // Extract generics
    let generics = extract_method_generics(method_node, source);

    // Extract parameters by finding the parameters node
    let parameters = find_parameters_node(method_node)
        .map(|params_node| extract_function_parameters(params_node, source))
        .transpose()?
        .unwrap_or_default();

    // Extract return type
    let return_type = extract_method_return_type(method_node, source);

    // Extract documentation
    let documentation = extract_preceding_doc_comments(method_node, source);

    // Get location and content
    let location = SourceLocation::from_tree_sitter_node(method_node);
    let content = node_to_text(method_node, source).ok();

    // Generate entity_id
    let file_path_str = file_path
        .to_str()
        .ok_or_else(|| Error::entity_extraction("Invalid file path"))?;
    let entity_id = generate_entity_id(repository_id, file_path_str, &qualified_name);

    // Build metadata
    let mut metadata = EntityMetadata {
        is_async,
        is_const,
        is_generic: !generics.is_empty(),
        generic_params: generics.clone(),
        ..Default::default()
    };

    if is_unsafe {
        metadata
            .attributes
            .insert("unsafe".to_string(), "true".to_string());
    }

    // Build signature
    let signature = FunctionSignature {
        parameters: parameters
            .iter()
            .map(|(name, ty)| (name.clone(), Some(ty.clone())))
            .collect(),
        return_type: return_type.clone(),
        is_async,
        generics: generics.clone(),
    };

    // Determine entity type: Method (has self or returns Self) or Function (associated function)
    let entity_type = if is_method(&parameters, &return_type) {
        EntityType::Method
    } else {
        EntityType::Function
    };

    CodeEntityBuilder::default()
        .entity_id(entity_id)
        .repository_id(repository_id.to_string())
        .name(name)
        .qualified_name(qualified_name)
        .parent_scope(Some(impl_qualified_name.to_string()))
        .entity_type(entity_type)
        .location(location)
        .visibility(visibility)
        .documentation_summary(documentation)
        .content(content)
        .metadata(metadata)
        .signature(Some(signature))
        .language(Language::Rust)
        .file_path(file_path.to_path_buf())
        .build()
        .map_err(|e| Error::entity_extraction(format!("Failed to build method entity: {e}")))
}

/// Find method name in a function_item node
fn find_method_name(node: Node, source: &str) -> Option<String> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == node_kinds::IDENTIFIER {
            return node_to_text(child, source).ok();
        }
    }
    None
}

/// Extract visibility from a method node
fn extract_method_visibility(node: Node) -> Visibility {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == node_kinds::VISIBILITY_MODIFIER {
            return Visibility::Public;
        }
    }
    Visibility::Private
}

/// Extract generic parameters from a method node
fn extract_method_generics(node: Node, source: &str) -> Vec<String> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "type_parameters" {
            return extract_generics_from_node(child, source);
        }
    }
    Vec::new()
}

/// Extract return type from a method node
fn extract_method_return_type(node: Node, source: &str) -> Option<String> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "->" {
            // Return type follows the arrow
            if let Some(sibling) = child.next_sibling() {
                return node_to_text(sibling, source).ok();
            }
        }
    }
    None
}
