//! Handler for extracting Rust impl blocks and their methods
//!
//! This module processes tree-sitter query matches for Rust impl blocks
//! (both inherent and trait implementations) and extracts both the impl
//! block itself and all methods within it as separate entities.

#![deny(warnings)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]

use crate::common::entity_building::ExtractionContext;
use crate::common::path_config::RUST_PATH_CONFIG;
use crate::common::reference_resolution::{resolve_reference, ResolutionContext};
use crate::qualified_name::build_qualified_name_from_ast;
use crate::rust::handler_impls::common::{
    build_generic_bounds_map, extract_function_calls, extract_function_modifiers,
    extract_function_parameters, extract_generics_from_node, extract_generics_with_bounds,
    extract_local_var_types, extract_preceding_doc_comments, extract_type_alias_map,
    extract_type_references, extract_visibility_from_node, extract_where_clause_bounds,
    find_capture_node, find_child_by_kind, format_generic_param, get_file_import_map,
    get_rust_edge_case_registry, merge_parsed_generics, node_to_text, require_capture_node,
    resolve_type_alias_chain, RustResolutionContext,
};
use crate::rust::handler_impls::constants::{capture_names, node_kinds, special_idents};
use codesearch_core::entities::{
    CodeEntityBuilder, EntityMetadata, EntityRelationshipData, EntityType, FunctionSignature,
    Language, ReferenceType, SourceLocation, SourceReference, Visibility,
};
use codesearch_core::entity_id::generate_entity_id;
use codesearch_core::error::{Error, Result};
use codesearch_core::CodeEntity;
use std::path::Path;
use tracing::debug;
use tree_sitter::Node;

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
pub(crate) fn handle_impl_impl(ctx: &ExtractionContext) -> Result<Vec<CodeEntity>> {
    let impl_node = require_capture_node(ctx.query_match, ctx.query, capture_names::IMPL)?;

    // Skip trait implementations - they will be handled by handle_impl_trait
    // Check if this impl block has a "trait" field (indicating "impl Trait for Type")
    if impl_node.child_by_field_name("trait").is_some() {
        return Ok(Vec::new());
    }

    // Extract the type this impl is for
    let for_type_raw = find_capture_node(ctx.query_match, ctx.query, capture_names::TYPE)
        .and_then(|node| node_to_text(node, ctx.source).ok())
        .unwrap_or_else(|| special_idents::ANONYMOUS.to_string());

    // Derive module path from file path for qualified name resolution
    let module_path = ctx
        .source_root
        .and_then(|root| crate::rust::module_path::derive_module_path(ctx.file_path, root));

    // Build ImportMap from file's imports for qualified name resolution
    let import_map = get_file_import_map(impl_node, ctx.source);

    // Resolve for_type through imports (strip generics first for resolution)
    // Use resolve_reference to handle crate::, self::, super:: prefixes
    let for_type_base = for_type_raw
        .split('<')
        .next()
        .unwrap_or(&for_type_raw)
        .trim();

    // Create resolution context for type resolution
    let edge_case_registry = get_rust_edge_case_registry();
    let type_resolution_ctx = ResolutionContext {
        import_map: &import_map,
        parent_scope: None,
        package_name: ctx.package_name,
        current_module: module_path.as_deref(),
        path_config: &RUST_PATH_CONFIG,
        edge_case_handlers: Some(&edge_case_registry),
    };

    // for_type_base is the simple name from the AST
    let for_type_resolved_initial =
        resolve_reference(for_type_base, for_type_base, &type_resolution_ctx);

    // Check if the type is a type alias and resolve to the underlying concrete type.
    // This ensures `impl Settings` (where Settings = RawConfig) has methods named
    // RawConfig::new instead of Settings::new.
    let for_type_resolved_ref = {
        // Get AST root for type alias extraction
        let root = crate::common::import_map::get_ast_root(impl_node);

        // Extract type aliases from the file
        let type_aliases = extract_type_alias_map(root, ctx.source);

        // Try to resolve the base type name through the alias chain
        if let Some(concrete_type) = resolve_type_alias_chain(for_type_base, &type_aliases, 10) {
            // Build the qualified name for the concrete type
            // The concrete_type comes from the type alias resolution
            resolve_reference(&concrete_type, &concrete_type, &type_resolution_ctx)
        } else {
            for_type_resolved_initial
        }
    };
    let for_type_resolved = for_type_resolved_ref.target.clone();

    // Keep original for display, but store resolved for relationships
    let for_type = for_type_raw.clone();

    // Build qualified name context
    let scope_result = build_qualified_name_from_ast(impl_node, ctx.source, "rust");
    let parent_scope = scope_result.parent_scope;

    // Build full prefix including package, module, and AST scope
    let full_prefix = compose_full_prefix(
        ctx.package_name,
        module_path.as_deref(),
        &parent_scope,
        "::",
    );

    // Build resolution context for qualified name normalization
    let resolution_ctx = RustResolutionContext {
        import_map: &import_map,
        parent_scope: Some(parent_scope.as_str()),
        package_name: ctx.package_name,
        current_module: module_path.as_deref(),
        path_config: &RUST_PATH_CONFIG,
        edge_case_handlers: Some(&edge_case_registry),
    };

    // Extract generics with parsed bounds
    let mut parsed_generics =
        find_capture_node(ctx.query_match, ctx.query, capture_names::GENERICS)
            .map(|node| extract_generics_with_bounds(node, ctx.source, &resolution_ctx))
            .unwrap_or_default();

    // Merge where clause bounds if present
    if let Some(where_node) = find_capture_node(ctx.query_match, ctx.query, capture_names::WHERE) {
        let where_bounds = extract_where_clause_bounds(where_node, ctx.source, &resolution_ctx);
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
    let impl_body = find_capture_node(ctx.query_match, ctx.query, capture_names::IMPL_BODY);
    let mut entities = Vec::new();

    if let Some(body_node) = impl_body {
        let impl_ctx = ImplContext {
            qualified_name: &impl_qualified_name,
            for_type_resolved: &for_type_resolved,
            trait_name_resolved: None, // No trait for inherent impl
            generics: &generics,
            package_name: ctx.package_name,
            module_path: module_path.as_deref(),
        };
        let methods = extract_impl_methods(
            body_node,
            ctx.source,
            ctx.file_path,
            ctx.repository_id,
            &impl_ctx,
        )?;
        entities.extend(methods);
    }

    // Create the impl block entity itself
    let location = SourceLocation::from_tree_sitter_node(impl_node);
    let content = node_to_text(impl_node, ctx.source).ok();
    let documentation = extract_preceding_doc_comments(impl_node, ctx.source);

    let file_path_str = ctx
        .file_path
        .to_str()
        .ok_or_else(|| Error::entity_extraction("Invalid file path"))?;
    let entity_id = generate_entity_id(ctx.repository_id, file_path_str, &impl_qualified_name);

    let mut metadata = EntityMetadata {
        is_generic: !generics.is_empty(),
        generic_params: generics.clone(),
        generic_bounds,
        ..Default::default()
    };

    metadata
        .attributes
        .insert("for_type".to_string(), for_type.clone());

    // Build typed relationship data for inherent impl
    // Note: imports are NOT stored here. Per the spec (R-IMPORTS), imports are
    // a module-level relationship. They are collected by module_handlers.

    // Convert trait bound refs to SourceReference for uses_types
    let impl_location = SourceLocation::from_tree_sitter_node(impl_node);
    let uses_types: Vec<SourceReference> = parsed_generics
        .bound_trait_refs
        .iter()
        .filter_map(|trait_ref| {
            SourceReference::builder()
                .target(trait_ref.target.clone())
                .simple_name(trait_ref.simple_name.clone())
                .is_external(trait_ref.is_external)
                .location(impl_location.clone())
                .ref_type(codesearch_core::entities::ReferenceType::TypeUsage)
                .build()
                .ok()
        })
        .collect();

    let relationships = EntityRelationshipData {
        uses_types,
        for_type: Some(
            SourceReference::builder()
                .target(for_type_resolved_ref.target.clone())
                .simple_name(for_type_resolved_ref.simple_name.clone())
                .is_external(for_type_resolved_ref.is_external)
                .location(impl_location.clone())
                .ref_type(ReferenceType::TypeUsage)
                .build()?,
        ),
        ..Default::default()
    };

    let impl_entity = CodeEntityBuilder::default()
        .entity_id(entity_id)
        .repository_id(ctx.repository_id.to_string())
        .name(for_type)
        .qualified_name(impl_qualified_name.clone())
        .parent_scope(if full_prefix.is_empty() {
            None
        } else {
            Some(full_prefix)
        })
        .entity_type(EntityType::Impl)
        .location(location)
        .visibility(None) // Impl blocks don't have visibility
        .documentation_summary(documentation)
        .content(content)
        .metadata(metadata)
        .language(Language::Rust)
        .file_path(ctx.file_path.to_path_buf())
        .relationships(relationships)
        .build()
        .map_err(|e| Error::entity_extraction(format!("Failed to build impl entity: {e}")))?;

    // Insert impl block entity at the beginning
    entities.insert(0, impl_entity);

    Ok(entities)
}

/// Process a trait impl block query match and extract entities
pub(crate) fn handle_impl_trait_impl(ctx: &ExtractionContext) -> Result<Vec<CodeEntity>> {
    let impl_node = require_capture_node(ctx.query_match, ctx.query, capture_names::IMPL_TRAIT)?;

    // Extract the type this impl is for
    let for_type_raw = find_capture_node(ctx.query_match, ctx.query, capture_names::TYPE)
        .and_then(|node| node_to_text(node, ctx.source).ok())
        .unwrap_or_else(|| special_idents::ANONYMOUS.to_string());

    // Extract the trait being implemented
    let trait_name_raw = find_capture_node(ctx.query_match, ctx.query, capture_names::TRAIT)
        .and_then(|node| node_to_text(node, ctx.source).ok())
        .unwrap_or_else(|| special_idents::ANONYMOUS.to_string());

    // Derive module path from file path for qualified name resolution
    let module_path = ctx
        .source_root
        .and_then(|root| crate::rust::module_path::derive_module_path(ctx.file_path, root));

    // Build ImportMap from file's imports for qualified name resolution
    let import_map = get_file_import_map(impl_node, ctx.source);

    // Resolve for_type through imports (strip generics first for resolution)
    let for_type_base = for_type_raw
        .split('<')
        .next()
        .unwrap_or(&for_type_raw)
        .trim();

    // Create resolution context for type/trait resolution
    let edge_case_registry = get_rust_edge_case_registry();
    let type_resolution_ctx = ResolutionContext {
        import_map: &import_map,
        parent_scope: None,
        package_name: ctx.package_name,
        current_module: module_path.as_deref(),
        path_config: &RUST_PATH_CONFIG,
        edge_case_handlers: Some(&edge_case_registry),
    };

    // for_type_base is the simple name from the AST
    let for_type_resolved_ref =
        resolve_reference(for_type_base, for_type_base, &type_resolution_ctx);
    let for_type_resolved = for_type_resolved_ref.target.clone();

    // Resolve trait_name through imports (strip generics first for resolution)
    let trait_name_base = trait_name_raw
        .split('<')
        .next()
        .unwrap_or(&trait_name_raw)
        .trim();
    // trait_name_base is the simple name from the AST
    let trait_name_resolved_ref =
        resolve_reference(trait_name_base, trait_name_base, &type_resolution_ctx);
    let trait_name_resolved = trait_name_resolved_ref.target.clone();

    // Keep original for display
    let for_type = for_type_raw.clone();
    let trait_name = trait_name_raw.clone();

    // Build qualified name context
    let scope_result = build_qualified_name_from_ast(impl_node, ctx.source, "rust");
    let parent_scope = scope_result.parent_scope;

    // Build full prefix including package, module, and AST scope
    let full_prefix = compose_full_prefix(
        ctx.package_name,
        module_path.as_deref(),
        &parent_scope,
        "::",
    );

    // Build resolution context for qualified name normalization
    let resolution_ctx = RustResolutionContext {
        import_map: &import_map,
        parent_scope: Some(parent_scope.as_str()),
        package_name: ctx.package_name,
        current_module: module_path.as_deref(),
        path_config: &RUST_PATH_CONFIG,
        edge_case_handlers: Some(&edge_case_registry),
    };

    // Extract generics with parsed bounds
    let mut parsed_generics =
        find_capture_node(ctx.query_match, ctx.query, capture_names::GENERICS)
            .map(|node| extract_generics_with_bounds(node, ctx.source, &resolution_ctx))
            .unwrap_or_default();

    // Merge where clause bounds if present
    if let Some(where_node) = find_capture_node(ctx.query_match, ctx.query, capture_names::WHERE) {
        let where_bounds = extract_where_clause_bounds(where_node, ctx.source, &resolution_ctx);
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
    let impl_body = find_capture_node(ctx.query_match, ctx.query, capture_names::IMPL_BODY);
    let mut entities = Vec::new();

    if let Some(body_node) = impl_body {
        let impl_ctx = ImplContext {
            qualified_name: &impl_qualified_name,
            for_type_resolved: &for_type_resolved,
            trait_name_resolved: Some(&trait_name_resolved),
            generics: &generics,
            package_name: ctx.package_name,
            module_path: module_path.as_deref(),
        };
        let methods = extract_impl_methods(
            body_node,
            ctx.source,
            ctx.file_path,
            ctx.repository_id,
            &impl_ctx,
        )?;
        entities.extend(methods);
    }

    // Create the impl block entity itself
    let location = SourceLocation::from_tree_sitter_node(impl_node);
    let content = node_to_text(impl_node, ctx.source).ok();
    let documentation = extract_preceding_doc_comments(impl_node, ctx.source);

    let file_path_str = ctx
        .file_path
        .to_str()
        .ok_or_else(|| Error::entity_extraction("Invalid file path"))?;
    let entity_id = generate_entity_id(ctx.repository_id, file_path_str, &impl_qualified_name);

    let mut metadata = EntityMetadata {
        is_generic: !generics.is_empty(),
        generic_params: generics.clone(),
        generic_bounds,
        ..Default::default()
    };

    metadata
        .attributes
        .insert("for_type".to_string(), for_type.clone());

    // Build typed relationship data for trait impl
    // Note: imports are NOT stored here. Per the spec (R-IMPORTS), imports are
    // a module-level relationship. They are collected by module_handlers.

    // Convert trait bound refs to SourceReference for uses_types
    let uses_types: Vec<SourceReference> = parsed_generics
        .bound_trait_refs
        .iter()
        .filter_map(|trait_ref| {
            SourceReference::builder()
                .target(trait_ref.target.clone())
                .simple_name(trait_ref.simple_name.clone())
                .is_external(trait_ref.is_external)
                .location(location.clone())
                .ref_type(codesearch_core::entities::ReferenceType::TypeUsage)
                .build()
                .ok()
        })
        .collect();

    let relationships = EntityRelationshipData {
        uses_types,
        implements_trait: Some(
            SourceReference::builder()
                .target(trait_name_resolved_ref.target.clone())
                .simple_name(trait_name_resolved_ref.simple_name.clone())
                .is_external(trait_name_resolved_ref.is_external)
                .location(location.clone())
                .ref_type(ReferenceType::Extends)
                .build()?,
        ),
        for_type: Some(
            SourceReference::builder()
                .target(for_type_resolved_ref.target.clone())
                .simple_name(for_type_resolved_ref.simple_name.clone())
                .is_external(for_type_resolved_ref.is_external)
                .location(location.clone())
                .ref_type(ReferenceType::TypeUsage)
                .build()?,
        ),
        ..Default::default()
    };

    let impl_entity = CodeEntityBuilder::default()
        .entity_id(entity_id)
        .repository_id(ctx.repository_id.to_string())
        .name(format!("{trait_name} for {for_type}"))
        .qualified_name(impl_qualified_name.clone())
        .parent_scope(if full_prefix.is_empty() {
            None
        } else {
            Some(full_prefix)
        })
        .entity_type(EntityType::Impl)
        .location(location)
        .visibility(None) // Impl blocks don't have visibility
        .documentation_summary(documentation)
        .content(content)
        .metadata(metadata)
        .language(Language::Rust)
        .file_path(ctx.file_path.to_path_buf())
        .relationships(relationships)
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
                match extract_method(child, source, file_path, repository_id, impl_ctx) {
                    Ok(method) => entities.push(method),
                    Err(e) => debug!(
                        "Failed to extract method in impl block at {}:{}: {e}",
                        file_path.display(),
                        child.start_position().row + 1
                    ),
                }
            }
            "const_item" => {
                match extract_associated_constant(child, source, file_path, repository_id, impl_ctx)
                {
                    Ok(constant) => entities.push(constant),
                    Err(e) => debug!(
                        "Failed to extract associated constant in impl block at {}:{}: {e}",
                        file_path.display(),
                        child.start_position().row + 1
                    ),
                }
            }
            "type_item" => {
                match extract_associated_type(child, source, file_path, repository_id, impl_ctx) {
                    Ok(type_alias) => entities.push(type_alias),
                    Err(e) => debug!(
                        "Failed to extract associated type in impl block at {}:{}: {e}",
                        file_path.display(),
                        child.start_position().row + 1
                    ),
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
    /// Package name for path normalization
    package_name: Option<&'a str>,
    /// Module path for path normalization
    module_path: Option<&'a str>,
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
    /// The visibility of the entity (None for entities where visibility doesn't apply)
    visibility: Option<Visibility>,
    /// Entity-specific metadata (async, const, generics, etc.)
    metadata: EntityMetadata,
    /// Function signature if this is a method or associated function
    signature: Option<FunctionSignature>,
    /// Typed relationship data (calls, uses_types, imports, call_aliases)
    relationships: EntityRelationshipData,
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
        .relationships(components.relationships)
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
            // Inherent impl uses UFCS format: <Type>::constant
            format!("<{}>{bounds_suffix}::{name}", impl_ctx.for_type_resolved)
        }
    };

    // Extract visibility
    // Trait impl constants are effectively public (they can't have visibility modifiers)
    // Inherent impl constants use the explicit visibility or default to private
    let visibility = if impl_ctx.trait_name_resolved.is_some() {
        Visibility::Public
    } else {
        extract_method_visibility(const_node)
    };

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
            visibility: Some(visibility),
            metadata,
            signature: None,
            relationships: EntityRelationshipData::default(),
        },
    )
}

/// Extract an associated type from a trait impl block
///
/// Associated types are extracted using UFCS notation: `<Type as Trait>::AssocType`
fn extract_associated_type(
    type_node: Node,
    source: &str,
    file_path: &Path,
    repository_id: &str,
    impl_ctx: &ImplContext,
) -> Result<CodeEntity> {
    // Extract type alias name
    let name = type_node
        .child_by_field_name("name")
        .and_then(|n| node_to_text(n, source).ok())
        .unwrap_or_else(|| special_idents::ANONYMOUS.to_string());

    // Build qualified name using UFCS format for trait impls
    let qualified_name = if let Some(trait_name) = impl_ctx.trait_name_resolved {
        format!("<{} as {trait_name}>::{name}", impl_ctx.for_type_resolved)
    } else {
        // Inherent type aliases (rare) use <Type>::Name format
        format!("<{}>::{name}", impl_ctx.for_type_resolved)
    };

    // Associated types in trait impls are effectively public
    // (they can only appear in trait impls, and trait impl items are public)
    let visibility = Visibility::Public;

    // Extract the aliased type
    let aliased_type = type_node
        .child_by_field_name("type")
        .and_then(|n| node_to_text(n, source).ok());

    // Build metadata
    let mut metadata = EntityMetadata::default();
    if let Some(aliased_type_str) = &aliased_type {
        metadata
            .attributes
            .insert("aliased_type".to_string(), aliased_type_str.clone());
    }

    // Get documentation and content
    let documentation = extract_preceding_doc_comments(type_node, source);
    let location = SourceLocation::from_tree_sitter_node(type_node);
    let content = node_to_text(type_node, source).ok();

    // Generate entity_id
    let file_path_str = file_path
        .to_str()
        .ok_or_else(|| Error::entity_extraction("Invalid file path"))?;
    let entity_id = generate_entity_id(repository_id, file_path_str, &qualified_name);

    // Associated types belong to the impl block (for CONTAINS relationship)
    let parent_scope = impl_ctx.qualified_name.to_string();

    CodeEntityBuilder::default()
        .entity_id(entity_id)
        .repository_id(repository_id.to_string())
        .name(name)
        .qualified_name(qualified_name)
        .parent_scope(Some(parent_scope))
        .entity_type(EntityType::TypeAlias)
        .location(location)
        .visibility(Some(visibility))
        .documentation_summary(documentation)
        .content(content)
        .metadata(metadata)
        .language(Language::Rust)
        .file_path(file_path.to_path_buf())
        .relationships(EntityRelationshipData::default())
        .build()
        .map_err(|e| {
            Error::entity_extraction(format!("Failed to build associated type entity: {e}"))
        })
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
            // Inherent impl uses UFCS format: <Type>::method
            format!("<{}>{bounds_suffix}::{name}", impl_ctx.for_type_resolved)
        }
    };

    // Extract visibility
    // Trait impl methods are effectively public (they can't have visibility modifiers)
    // Inherent impl methods use the explicit visibility or default to private
    let visibility = if impl_ctx.trait_name_resolved.is_some() {
        Visibility::Public
    } else {
        extract_method_visibility(method_node)
    };

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

    // Build resolution context for qualified name normalization
    // Use module path as parent_scope (not impl block name) so bare function calls
    // like `async_callee()` resolve to `module::async_callee` not `impl_block::async_callee`
    let edge_case_registry = get_rust_edge_case_registry();
    let resolution_ctx = RustResolutionContext {
        import_map: &import_map,
        parent_scope: impl_ctx.module_path,
        package_name: impl_ctx.package_name,
        current_module: impl_ctx.module_path,
        path_config: &RUST_PATH_CONFIG,
        edge_case_handlers: Some(&edge_case_registry),
    };

    // Extract local variable types for method call resolution
    let local_vars = extract_local_var_types(method_node, source);

    // For methods, pass empty generic bounds (method-level generics with bounds not yet extracted)
    let method_generic_bounds = im::HashMap::new();

    // Extract function calls from the method body with qualified name resolution
    let calls = extract_function_calls(
        method_node,
        source,
        &resolution_ctx,
        &local_vars,
        &method_generic_bounds,
    );

    // Extract type references for USES relationships
    let type_refs = extract_type_references(method_node, source, &resolution_ctx);

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

    // Build typed relationship data
    // Note: imports are NOT stored here. Per the spec (R-IMPORTS), imports are
    // a module-level relationship. They are collected by module_handlers.

    // Compute call_aliases for trait impl methods
    // For methods like "<TypeFQN as TraitFQN>::method", we add "<TypeFQN>::method" as an alias
    // This enables UFCS resolution: calls like `type.method()` resolve to the trait impl method
    // when there's no inherent method with the same name
    let call_aliases = if impl_ctx.trait_name_resolved.is_some() {
        vec![format!("<{}>::{}", impl_ctx.for_type_resolved, name)]
    } else {
        Vec::new()
    };

    let relationships = EntityRelationshipData {
        calls,
        uses_types: type_refs,
        call_aliases,
        ..Default::default()
    };

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
            visibility: Some(visibility),
            metadata,
            signature: Some(signature),
            relationships,
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
            return extract_visibility_from_node(child);
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
