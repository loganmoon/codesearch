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
    extract_generics_from_node, extract_preceding_doc_comments, find_capture_node, node_to_text,
    require_capture_node,
};
use crate::rust::handlers::constants::{
    capture_names, function_modifiers, keywords, node_kinds, punctuation, special_idents,
};
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
        .entity_type(EntityType::Method) // Per entities.rs:95
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
        .entity_type(EntityType::Method) // Per entities.rs:95
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

/// Extract all methods from an impl block body
fn extract_impl_methods(
    body_node: Node,
    source: &str,
    file_path: &Path,
    repository_id: &str,
    impl_qualified_name: &str,
    for_type: &str,
    trait_name: Option<&str>,
) -> Result<Vec<CodeEntity>> {
    let mut methods = Vec::new();
    let mut cursor = body_node.walk();

    for child in body_node.children(&mut cursor) {
        if child.kind() == node_kinds::FUNCTION_ITEM {
            if let Ok(method) = extract_method(
                child,
                source,
                file_path,
                repository_id,
                impl_qualified_name,
                for_type,
                trait_name,
            ) {
                methods.push(method);
            }
        }
    }

    Ok(methods)
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

    // Extract modifiers
    let (is_async, is_unsafe, is_const) = extract_method_modifiers(method_node);

    // Extract generics
    let generics = extract_method_generics(method_node, source);

    // Extract parameters
    let parameters = extract_method_parameters(method_node, source)?;

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

    CodeEntityBuilder::default()
        .entity_id(entity_id)
        .repository_id(repository_id.to_string())
        .name(name)
        .qualified_name(qualified_name)
        .parent_scope(Some(impl_qualified_name.to_string()))
        .entity_type(EntityType::Function)
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

/// Extract function modifiers (async, unsafe, const) from a method node
fn extract_method_modifiers(node: Node) -> (bool, bool, bool) {
    let mut has_async = false;
    let mut has_unsafe = false;
    let mut has_const = false;
    let mut cursor = node.walk();

    for child in node.children(&mut cursor) {
        match child.kind() {
            "function_modifiers" => {
                // Found a function_modifiers node, walk its children
                let mut mod_cursor = child.walk();
                for mod_child in child.children(&mut mod_cursor) {
                    match mod_child.kind() {
                        function_modifiers::ASYNC => has_async = true,
                        function_modifiers::UNSAFE => has_unsafe = true,
                        function_modifiers::CONST => has_const = true,
                        _ => {}
                    }
                }
            }
            // Also check for direct keyword children
            function_modifiers::ASYNC => has_async = true,
            function_modifiers::UNSAFE => has_unsafe = true,
            function_modifiers::CONST => has_const = true,
            _ => {}
        }
    }

    (has_async, has_unsafe, has_const)
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

/// Extract parameters from a method node
fn extract_method_parameters(node: Node, source: &str) -> Result<Vec<(String, String)>> {
    let mut cursor = node.walk();
    let params_node = node
        .children(&mut cursor)
        .find(|c| c.kind() == "parameters");

    let Some(params) = params_node else {
        return Ok(Vec::new());
    };

    let mut parameters = Vec::new();
    let mut param_cursor = params.walk();

    for child in params.children(&mut param_cursor) {
        if matches!(
            child.kind(),
            punctuation::OPEN_PAREN | punctuation::CLOSE_PAREN | punctuation::COMMA
        ) {
            continue;
        }

        match child.kind() {
            node_kinds::PARAMETER => {
                if let Some((pattern, param_type)) = extract_parameter_parts(child, source)? {
                    parameters.push((pattern, param_type));
                }
            }
            node_kinds::SELF_PARAMETER => {
                let text = node_to_text(child, source)?;
                parameters.push((keywords::SELF.to_string(), text));
            }
            node_kinds::VARIADIC_PARAMETER => {
                let text = node_to_text(child, source)?;
                parameters.push((special_idents::VARIADIC.to_string(), text));
            }
            _ => {}
        }
    }

    Ok(parameters)
}

/// Extract pattern and type parts from a parameter node
fn extract_parameter_parts(node: Node, source: &str) -> Result<Option<(String, String)>> {
    let full_text = node_to_text(node, source)?;

    if let Some(colon_pos) = full_text.find(':') {
        let pattern = full_text[..colon_pos].trim().to_string();
        let param_type = full_text[colon_pos + 1..].trim().to_string();
        return Ok(Some((pattern, param_type)));
    }

    if !full_text.trim().is_empty() {
        Ok(Some((full_text, String::new())))
    } else {
        Ok(None)
    }
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
