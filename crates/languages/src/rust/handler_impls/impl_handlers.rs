//! Handler for extracting Rust impl blocks and their methods
//!
//! This module processes tree-sitter query matches for Rust impl blocks
//! (both inherent and trait implementations) and extracts both the impl
//! block itself and all methods within it as separate entities.

#![deny(warnings)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]

use crate::common::import_map::{parse_file_imports, resolve_rust_reference, ImportMap};
use crate::qualified_name::build_qualified_name_from_ast;
use crate::rust::handler_impls::common::{
    build_generic_bounds_map, extract_function_calls, extract_function_modifiers,
    extract_function_parameters, extract_generics_from_node, extract_generics_with_bounds,
    extract_local_var_types, extract_preceding_doc_comments, extract_type_references,
    extract_where_clause_bounds, find_capture_node, format_generic_param, merge_parsed_generics,
    node_to_text, require_capture_node,
};
use crate::rust::handler_impls::constants::{capture_names, node_kinds, special_idents};
use codesearch_core::entities::{
    CodeEntityBuilder, EntityMetadata, EntityType, FunctionSignature, Language, SourceLocation,
    Visibility,
};
use codesearch_core::entity_id::generate_entity_id;
use codesearch_core::error::{Error, Result};
use codesearch_core::CodeEntity;
use std::path::Path;
use tree_sitter::{Node, Query, QueryMatch};

/// Compose a full prefix from package, module, and AST scope components
///
/// Joins non-empty components with the separator. Used to build the full
/// qualified name prefix for impl blocks.
fn compose_full_prefix(
    package: Option<&str>,
    module: Option<&str>,
    scope: &str,
    separator: &str,
) -> String {
    let mut parts: Vec<&str> = Vec::new();
    if let Some(pkg) = package {
        if !pkg.is_empty() {
            parts.push(pkg);
        }
    }
    if let Some(mod_path) = module {
        if !mod_path.is_empty() {
            parts.push(mod_path);
        }
    }
    if !scope.is_empty() {
        parts.push(scope);
    }
    parts.join(separator)
}

/// Process an inherent impl block query match and extract entities
#[allow(clippy::too_many_arguments)]
pub fn handle_impl_impl(
    query_match: &QueryMatch,
    query: &Query,
    source: &str,
    file_path: &Path,
    repository_id: &str,
    package_name: Option<&str>,
    source_root: Option<&Path>,
    _repo_root: &Path,
) -> Result<Vec<CodeEntity>> {
    let impl_node = require_capture_node(query_match, query, capture_names::IMPL)?;

    // Skip trait implementations - they will be handled by handle_impl_trait
    // Check if this impl block has a "trait" field (indicating "impl Trait for Type")
    if impl_node.child_by_field_name("trait").is_some() {
        return Ok(Vec::new());
    }

    // Extract the type this impl is for
    let for_type_raw = find_capture_node(query_match, query, capture_names::TYPE)
        .and_then(|node| node_to_text(node, source).ok())
        .unwrap_or_else(|| special_idents::ANONYMOUS.to_string());

    // Derive module path from file path for qualified name resolution
    let module_path =
        source_root.and_then(|root| crate::rust::module_path::derive_module_path(file_path, root));

    // Build ImportMap from file's imports for qualified name resolution
    let import_map = get_file_import_map(impl_node, source);

    // Resolve for_type through imports (strip generics first for resolution)
    // Use resolve_rust_reference to handle crate::, self::, super:: prefixes
    let for_type_base = for_type_raw
        .split('<')
        .next()
        .unwrap_or(&for_type_raw)
        .trim();
    let for_type_resolved = resolve_rust_reference(
        for_type_base,
        &import_map,
        None,
        package_name,
        module_path.as_deref(),
    );

    // Keep original for display, but store resolved for relationships
    let for_type = for_type_raw.clone();

    // Build qualified name context
    let scope_result = build_qualified_name_from_ast(impl_node, source, "rust");
    let parent_scope = scope_result.parent_scope;

    // Build full prefix including package, module, and AST scope
    let full_prefix =
        compose_full_prefix(package_name, module_path.as_deref(), &parent_scope, "::");

    // Extract generics with parsed bounds
    let mut parsed_generics = find_capture_node(query_match, query, capture_names::GENERICS)
        .map(|node| extract_generics_with_bounds(node, source, &import_map, Some(&parent_scope)))
        .unwrap_or_default();

    // Merge where clause bounds if present
    if let Some(where_node) = find_capture_node(query_match, query, capture_names::WHERE) {
        let where_bounds =
            extract_where_clause_bounds(where_node, source, &import_map, Some(&parent_scope));
        merge_parsed_generics(&mut parsed_generics, where_bounds);
    }

    // Build backward-compatible generic_params
    let generics: Vec<String> = parsed_generics
        .params
        .iter()
        .map(format_generic_param)
        .collect();

    // Build generic_bounds map
    let generic_bounds = build_generic_bounds_map(&parsed_generics);
    // Build generic bounds suffix for disambiguation
    let has_bounds = generics.iter().any(|g| g.contains(':'));
    let bounds_suffix = if has_bounds {
        format!(" where {}", generics.join(", "))
    } else {
        String::new()
    };

    let impl_qualified_name = if full_prefix.is_empty() {
        format!("impl {for_type_resolved}{bounds_suffix}")
    } else {
        format!("{full_prefix}::impl {for_type_resolved}{bounds_suffix}")
    };

    // Extract all methods from impl body
    let impl_body = find_capture_node(query_match, query, capture_names::IMPL_BODY);
    let mut entities = Vec::new();

    if let Some(body_node) = impl_body {
        let impl_ctx = ImplContext {
            qualified_name: &impl_qualified_name,
            for_type_resolved: &for_type_resolved,
            trait_name_resolved: None, // No trait for inherent impl
            generics: &generics,
        };
        let methods = extract_impl_methods(body_node, source, file_path, repository_id, &impl_ctx)?;
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
        generic_bounds,
        ..Default::default()
    };

    metadata
        .attributes
        .insert("for_type".to_string(), for_type.clone());

    // Store the resolved type name for relationship resolution
    metadata
        .attributes
        .insert("implements".to_string(), for_type_resolved.clone());

    // Add trait bounds to uses_types for relationship resolution
    if !parsed_generics.bound_trait_refs.is_empty() {
        if let Ok(json) = serde_json::to_string(&parsed_generics.bound_trait_refs) {
            metadata.attributes.insert("uses_types".to_string(), json);
        }
    }

    let impl_entity = CodeEntityBuilder::default()
        .entity_id(entity_id)
        .repository_id(repository_id.to_string())
        .name(for_type)
        .qualified_name(impl_qualified_name.clone())
        .parent_scope(if full_prefix.is_empty() {
            None
        } else {
            Some(full_prefix)
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
#[allow(clippy::too_many_arguments)]
pub fn handle_impl_trait_impl(
    query_match: &QueryMatch,
    query: &Query,
    source: &str,
    file_path: &Path,
    repository_id: &str,
    package_name: Option<&str>,
    source_root: Option<&Path>,
    _repo_root: &Path,
) -> Result<Vec<CodeEntity>> {
    let impl_node = require_capture_node(query_match, query, capture_names::IMPL_TRAIT)?;

    // Extract the type this impl is for
    let for_type_raw = find_capture_node(query_match, query, capture_names::TYPE)
        .and_then(|node| node_to_text(node, source).ok())
        .unwrap_or_else(|| special_idents::ANONYMOUS.to_string());

    // Extract the trait being implemented
    let trait_name_raw = find_capture_node(query_match, query, capture_names::TRAIT)
        .and_then(|node| node_to_text(node, source).ok())
        .unwrap_or_else(|| special_idents::ANONYMOUS.to_string());

    // Derive module path from file path for qualified name resolution
    let module_path =
        source_root.and_then(|root| crate::rust::module_path::derive_module_path(file_path, root));

    // Build ImportMap from file's imports for qualified name resolution
    let import_map = get_file_import_map(impl_node, source);

    // Resolve for_type through imports (strip generics first for resolution)
    let for_type_base = for_type_raw
        .split('<')
        .next()
        .unwrap_or(&for_type_raw)
        .trim();
    let for_type_resolved = resolve_rust_reference(
        for_type_base,
        &import_map,
        None,
        package_name,
        module_path.as_deref(),
    );

    // Resolve trait_name through imports (strip generics first for resolution)
    let trait_name_base = trait_name_raw
        .split('<')
        .next()
        .unwrap_or(&trait_name_raw)
        .trim();
    let trait_name_resolved = resolve_rust_reference(
        trait_name_base,
        &import_map,
        None,
        package_name,
        module_path.as_deref(),
    );

    // Keep original for display
    let for_type = for_type_raw.clone();
    let trait_name = trait_name_raw.clone();

    // Build qualified name context
    let scope_result = build_qualified_name_from_ast(impl_node, source, "rust");
    let parent_scope = scope_result.parent_scope;

    // Build full prefix including package, module, and AST scope
    let full_prefix =
        compose_full_prefix(package_name, module_path.as_deref(), &parent_scope, "::");

    // Extract generics with parsed bounds
    let mut parsed_generics = find_capture_node(query_match, query, capture_names::GENERICS)
        .map(|node| extract_generics_with_bounds(node, source, &import_map, Some(&parent_scope)))
        .unwrap_or_default();

    // Merge where clause bounds if present
    if let Some(where_node) = find_capture_node(query_match, query, capture_names::WHERE) {
        let where_bounds =
            extract_where_clause_bounds(where_node, source, &import_map, Some(&parent_scope));
        merge_parsed_generics(&mut parsed_generics, where_bounds);
    }

    // Build backward-compatible generic_params
    let generics: Vec<String> = parsed_generics
        .params
        .iter()
        .map(format_generic_param)
        .collect();

    // Build generic_bounds map
    let generic_bounds = build_generic_bounds_map(&parsed_generics);
    // Build generic bounds suffix for disambiguation
    let has_bounds = generics.iter().any(|g| g.contains(':'));
    let bounds_suffix = if has_bounds {
        format!(" where {}", generics.join(", "))
    } else {
        String::new()
    };

    let impl_qualified_name = if full_prefix.is_empty() {
        format!("<{for_type_resolved} as {trait_name_resolved}{bounds_suffix}>")
    } else {
        format!("{full_prefix}::<{for_type_resolved} as {trait_name_resolved}{bounds_suffix}>")
    };

    // Extract all methods from impl body
    let impl_body = find_capture_node(query_match, query, capture_names::IMPL_BODY);
    let mut entities = Vec::new();

    if let Some(body_node) = impl_body {
        let impl_ctx = ImplContext {
            qualified_name: &impl_qualified_name,
            for_type_resolved: &for_type_resolved,
            trait_name_resolved: Some(&trait_name_resolved),
            generics: &generics,
        };
        let methods = extract_impl_methods(body_node, source, file_path, repository_id, &impl_ctx)?;
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
        generic_bounds,
        ..Default::default()
    };

    metadata
        .attributes
        .insert("for_type".to_string(), for_type.clone());

    // Store the resolved names directly for relationship resolution
    metadata
        .attributes
        .insert("implements".to_string(), for_type_resolved.clone());
    metadata
        .attributes
        .insert("implements_trait".to_string(), trait_name_resolved.clone());

    // Add trait bounds to uses_types for relationship resolution
    if !parsed_generics.bound_trait_refs.is_empty() {
        if let Ok(json) = serde_json::to_string(&parsed_generics.bound_trait_refs) {
            metadata.attributes.insert("uses_types".to_string(), json);
        }
    }

    let impl_entity = CodeEntityBuilder::default()
        .entity_id(entity_id)
        .repository_id(repository_id.to_string())
        .name(format!("{trait_name} for {for_type}"))
        .qualified_name(impl_qualified_name.clone())
        .parent_scope(if full_prefix.is_empty() {
            None
        } else {
            Some(full_prefix)
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
    impl_ctx: &ImplContext,
) -> Result<Vec<CodeEntity>> {
    let mut entities = Vec::new();
    let mut cursor = body_node.walk();

    for child in body_node.children(&mut cursor) {
        match child.kind() {
            node_kinds::FUNCTION_ITEM => {
                if let Ok(method) =
                    extract_method(child, source, file_path, repository_id, impl_ctx)
                {
                    entities.push(method);
                }
            }
            "const_item" => {
                if let Ok(constant) =
                    extract_associated_constant(child, source, file_path, repository_id, impl_ctx)
                {
                    entities.push(constant);
                }
            }
            _ => {}
        }
    }

    Ok(entities)
}

/// Context information about the impl block containing a method or constant
///
/// This struct groups impl-block-specific information to avoid passing
/// too many parameters to entity extraction functions.
struct ImplContext<'a> {
    /// The qualified name of the impl block itself
    qualified_name: &'a str,
    /// The type being implemented for, resolved to FQN (e.g., "crate::module::Container")
    for_type_resolved: &'a str,
    /// Optional trait name for trait implementations, resolved to FQN
    trait_name_resolved: Option<&'a str>,
    /// Generic parameters with bounds (e.g., ["T: Clone", "U"])
    /// Used to disambiguate impl blocks with different bounds
    generics: &'a [String],
}

/// Components for building impl block member entities (methods, associated constants)
///
/// This struct encapsulates the extracted components needed to build a CodeEntity
/// for members of impl blocks. It's used as a parameter to `build_impl_entity` to
/// avoid repetitive entity construction code.
struct ImplEntityComponents {
    /// The simple name of the entity (e.g., "method_name")
    name: String,
    /// The fully qualified name including impl context (e.g., "Type::method_name" or "<Type as Trait>::method_name")
    qualified_name: String,
    /// The type of entity (Method, Function, or Constant)
    entity_type: EntityType,
    /// The visibility of the entity
    visibility: Visibility,
    /// Entity-specific metadata (async, const, generics, etc.)
    metadata: EntityMetadata,
    /// Function signature if this is a method or associated function
    signature: Option<FunctionSignature>,
}

/// Build a CodeEntity for impl block members (methods, associated constants)
fn build_impl_entity(
    node: Node,
    source: &str,
    file_path: &Path,
    repository_id: &str,
    impl_qualified_name: &str,
    components: ImplEntityComponents,
) -> Result<CodeEntity> {
    // Extract documentation
    let documentation = extract_preceding_doc_comments(node, source);

    // Get location and content
    let location = SourceLocation::from_tree_sitter_node(node);
    let content = node_to_text(node, source).ok();

    // Generate entity_id
    let file_path_str = file_path
        .to_str()
        .ok_or_else(|| Error::entity_extraction("Invalid file path"))?;
    let entity_id = generate_entity_id(repository_id, file_path_str, &components.qualified_name);

    CodeEntityBuilder::default()
        .entity_id(entity_id)
        .repository_id(repository_id.to_string())
        .name(components.name)
        .qualified_name(components.qualified_name)
        .parent_scope(Some(impl_qualified_name.to_string()))
        .entity_type(components.entity_type)
        .location(location)
        .visibility(components.visibility)
        .documentation_summary(documentation)
        .content(content)
        .metadata(components.metadata)
        .signature(components.signature)
        .language(Language::Rust)
        .file_path(file_path.to_path_buf())
        .build()
        .map_err(|e| Error::entity_extraction(format!("Failed to build entity: {e}")))
}

/// Determine if a function should be typed as a Method
///
/// A function is classified as a method if it has a `self` parameter OR returns `Self`.
///
/// ## Semantic Choice
/// This classification diverges from standard Rust semantics where functions like
/// `fn new() -> Self` are considered "associated functions" rather than methods.
/// However, for semantic code search purposes, we classify functions returning `Self`
/// as methods because they are conceptually instance-related operations.
///
/// ## Implementation Notes
/// - Uses word-boundary matching to avoid false positives (e.g., `SelfService`, `SelfReference`)
/// - Handles common `Self` variations: `Self`, `Option<Self>`, `Result<Self, E>`, etc.
fn is_method(parameters: &[(String, String)], return_type: &Option<String>) -> bool {
    // Check for self parameter (any variant: self, &self, &mut self, mut self)
    let has_self_param = parameters.iter().any(|(name, _)| {
        name == "self" || name.starts_with("&self") || name.starts_with("mut self")
    });

    // Check for Self return type using word-boundary matching
    let returns_self = return_type.as_ref().is_some_and(|rt| {
        rt.split(|c: char| !c.is_alphanumeric() && c != '_')
            .any(|token| token == "Self")
    });

    has_self_param || returns_self
}

/// Extract an associated constant from an impl block
fn extract_associated_constant(
    const_node: Node,
    source: &str,
    file_path: &Path,
    repository_id: &str,
    impl_ctx: &ImplContext,
) -> Result<CodeEntity> {
    // Extract constant name
    let name = const_node
        .child_by_field_name("name")
        .and_then(|n| node_to_text(n, source).ok())
        .unwrap_or_else(|| special_idents::ANONYMOUS.to_string());

    // Build qualified name based on impl type using resolved FQNs
    // Include generic bounds to disambiguate impl blocks with different constraints
    let qualified_name = {
        let has_bounds = impl_ctx.generics.iter().any(|g| g.contains(':'));
        let bounds_suffix = if has_bounds {
            format!(" where {}", impl_ctx.generics.join(", "))
        } else {
            String::new()
        };

        if let Some(trait_name) = impl_ctx.trait_name_resolved {
            format!(
                "<{} as {trait_name}{bounds_suffix}>::{name}",
                impl_ctx.for_type_resolved
            )
        } else {
            format!("{}{bounds_suffix}::{name}", impl_ctx.for_type_resolved)
        }
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

    // Build the entity using the common helper
    build_impl_entity(
        const_node,
        source,
        file_path,
        repository_id,
        impl_ctx.qualified_name,
        ImplEntityComponents {
            name,
            qualified_name,
            entity_type: EntityType::Constant,
            visibility,
            metadata,
            signature: None,
        },
    )
}

/// Find a child node by kind
#[allow(clippy::manual_find)]
fn find_child_by_kind<'a>(node: Node<'a>, kind: &str) -> Option<Node<'a>> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == kind {
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
    impl_ctx: &ImplContext,
) -> Result<CodeEntity> {
    // Extract method name
    let name = find_method_name(method_node, source)
        .unwrap_or_else(|| special_idents::ANONYMOUS.to_string());

    // Build qualified name based on impl type using resolved FQNs
    // Include generic bounds to disambiguate impl blocks with different constraints
    // For trait impls: <TypeFQN as TraitFQN>::method or <TypeFQN as TraitFQN where T: Clone>::method
    // For inherent impls: TypeFQN::method or TypeFQN where T: Clone::method
    let qualified_name = {
        // Check if any generics have bounds (contain ':')
        let has_bounds = impl_ctx.generics.iter().any(|g| g.contains(':'));
        let bounds_suffix = if has_bounds {
            format!(" where {}", impl_ctx.generics.join(", "))
        } else {
            String::new()
        };

        if let Some(trait_name) = impl_ctx.trait_name_resolved {
            format!(
                "<{} as {trait_name}{bounds_suffix}>::{name}",
                impl_ctx.for_type_resolved
            )
        } else {
            format!("{}{bounds_suffix}::{name}", impl_ctx.for_type_resolved)
        }
    };

    // Extract visibility
    let visibility = extract_method_visibility(method_node);

    // Extract modifiers by finding the function_modifiers node
    let (is_async, is_unsafe, is_const) = find_child_by_kind(method_node, "function_modifiers")
        .map(extract_function_modifiers)
        .unwrap_or((false, false, false));

    // Extract generics
    let generics = extract_method_generics(method_node, source);

    // Extract parameters by finding the parameters node
    let parameters = find_child_by_kind(method_node, "parameters")
        .map(|params_node| extract_function_parameters(params_node, source))
        .transpose()?
        .unwrap_or_default();

    // Extract return type
    let return_type = extract_method_return_type(method_node, source);

    // Build ImportMap from file's imports for qualified name resolution
    let import_map = get_file_import_map(method_node, source);

    // Extract local variable types for method call resolution
    let local_vars = extract_local_var_types(method_node, source);

    // Extract function calls from the method body with qualified name resolution
    let calls = extract_function_calls(
        method_node,
        source,
        &import_map,
        Some(impl_ctx.qualified_name),
        &local_vars,
    );

    // Extract type references for USES relationships
    let type_refs = extract_type_references(
        method_node,
        source,
        &import_map,
        Some(impl_ctx.qualified_name),
    );

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

    // Build the entity using the common helper
    build_impl_entity(
        method_node,
        source,
        file_path,
        repository_id,
        impl_ctx.qualified_name,
        ImplEntityComponents {
            name,
            qualified_name,
            entity_type,
            visibility,
            metadata,
            signature: Some(signature),
        },
    )
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

/// Get the ImportMap for a file by walking up to the AST root
fn get_file_import_map(node: Node, source: &str) -> ImportMap {
    // Walk up to the root node
    let mut current = node;
    while let Some(parent) = current.parent() {
        current = parent;
    }

    // Parse imports from the root
    // Note: Rust import parsing already stores absolute paths (crate::, std::, etc.)
    // so no module_path resolution is needed
    parse_file_imports(current, source, Language::Rust, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compose_full_prefix_all_components() {
        assert_eq!(
            compose_full_prefix(Some("pkg"), Some("mod"), "scope", "::"),
            "pkg::mod::scope"
        );
    }

    #[test]
    fn test_compose_full_prefix_package_and_module() {
        assert_eq!(
            compose_full_prefix(Some("pkg"), Some("mod"), "", "::"),
            "pkg::mod"
        );
    }

    #[test]
    fn test_compose_full_prefix_package_and_scope() {
        assert_eq!(
            compose_full_prefix(Some("pkg"), None, "scope", "::"),
            "pkg::scope"
        );
    }

    #[test]
    fn test_compose_full_prefix_module_and_scope() {
        assert_eq!(
            compose_full_prefix(None, Some("mod"), "scope", "::"),
            "mod::scope"
        );
    }

    #[test]
    fn test_compose_full_prefix_only_scope() {
        assert_eq!(compose_full_prefix(None, None, "scope", "::"), "scope");
    }

    #[test]
    fn test_compose_full_prefix_empty_strings_filtered() {
        // Empty strings should be filtered out, not produce "::" artifacts
        assert_eq!(
            compose_full_prefix(Some(""), Some("mod"), "scope", "::"),
            "mod::scope"
        );
        assert_eq!(
            compose_full_prefix(Some("pkg"), Some(""), "scope", "::"),
            "pkg::scope"
        );
        assert_eq!(
            compose_full_prefix(Some("pkg"), Some("mod"), "", "::"),
            "pkg::mod"
        );
    }

    #[test]
    fn test_compose_full_prefix_all_empty() {
        assert_eq!(compose_full_prefix(None, None, "", "::"), "");
        assert_eq!(compose_full_prefix(Some(""), Some(""), "", "::"), "");
    }
}
