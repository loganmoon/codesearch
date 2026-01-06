//! Handler for extracting Rust function definitions
//!
//! This module processes tree-sitter query matches for Rust functions
//! and builds EntityData instances.

#![deny(warnings)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]

use crate::common::entity_building::{
    build_entity, extract_common_components, EntityDetails, ExtractionContext,
};
use crate::common::path_config::RUST_PATH_CONFIG;
use crate::rust::handler_impls::common::{
    build_generic_bounds_map, extract_function_calls, extract_function_modifiers,
    extract_function_parameters, extract_generics_with_bounds, extract_local_var_types,
    extract_preceding_doc_comments, extract_type_references, extract_visibility,
    extract_where_clause_bounds, find_capture_node, format_generic_param, get_file_import_map,
    get_rust_edge_case_registry, merge_parsed_generics, node_to_text, require_capture_node,
    RustResolutionContext,
};
use crate::rust::handler_impls::constants::capture_names;
use codesearch_core::entities::{
    EntityMetadata, EntityRelationshipData, EntityType, FunctionSignature, Language, ReferenceType,
    SourceLocation, SourceReference,
};
use codesearch_core::error::Result;
use codesearch_core::CodeEntity;

/// Process a function query match and extract entity data
pub(crate) fn handle_function_impl(ctx: &ExtractionContext) -> Result<Vec<CodeEntity>> {
    // Get the function node for location and content
    let function_node = require_capture_node(ctx.query_match, ctx.query, capture_names::FUNCTION)?;

    // Skip functions inside impl blocks - those are handled by the impl extractor
    if let Some(parent) = function_node.parent() {
        if parent.kind() == "declaration_list" {
            // Check if the declaration_list is inside an impl_item
            if let Some(grandparent) = parent.parent() {
                if grandparent.kind() == "impl_item" {
                    return Ok(Vec::new());
                }
            }
        }
    }

    // Extract common components
    let components = extract_common_components(ctx, capture_names::NAME, function_node, "rust")?;

    // Extract Rust-specific: visibility, documentation, content
    let visibility = extract_visibility(ctx.query_match, ctx.query);
    let documentation = extract_preceding_doc_comments(function_node, ctx.source);
    let content = node_to_text(function_node, ctx.source).ok();

    // Extract and parse modifiers
    let (is_async, is_unsafe, is_const) =
        find_capture_node(ctx.query_match, ctx.query, capture_names::MODIFIERS)
            .map(extract_function_modifiers)
            .unwrap_or((false, false, false));

    // Build ImportMap from file's imports for qualified name resolution
    let import_map = get_file_import_map(function_node, ctx.source);

    // Derive current module path for super:: resolution
    // This needs to include inline modules (from parent_scope) not just file-level modules
    // For example, if parent_scope is "test_crate::child::grandchild" and package is "test_crate",
    // current_module should be "child::grandchild" for proper super:: resolution
    let current_module = match (components.parent_scope.as_deref(), ctx.package_name) {
        (Some(parent), Some(pkg)) if !pkg.is_empty() => {
            let prefix = format!("{pkg}::");
            if let Some(rest) = parent.strip_prefix(&prefix) {
                if rest.is_empty() {
                    None
                } else {
                    Some(rest.to_string())
                }
            } else if parent == pkg {
                // Parent is exactly the package name (function at crate root)
                None
            } else {
                // Parent doesn't have package prefix, use as-is
                Some(parent.to_string())
            }
        }
        (Some(parent), _) => Some(parent.to_string()),
        _ => None,
    };

    // Build resolution context for qualified name normalization
    let edge_case_registry = get_rust_edge_case_registry();
    let resolution_ctx = RustResolutionContext {
        import_map: &import_map,
        parent_scope: components.parent_scope.as_deref(),
        package_name: ctx.package_name,
        current_module: current_module.as_deref(),
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

    // Build backward-compatible generic_params (raw strings)
    let generics: Vec<String> = parsed_generics
        .params
        .iter()
        .map(format_generic_param)
        .collect();

    // Build generic_bounds map
    let generic_bounds = build_generic_bounds_map(&parsed_generics);

    // Extract parameters
    let parameters = find_capture_node(ctx.query_match, ctx.query, capture_names::PARAMS)
        .map(|params_node| extract_function_parameters(params_node, ctx.source))
        .transpose()?
        .unwrap_or_default();

    // Extract return type
    let return_type = find_capture_node(ctx.query_match, ctx.query, capture_names::RETURN)
        .and_then(|node| node_to_text(node, ctx.source).ok());

    // Extract local variable types for method call resolution
    let local_vars = extract_local_var_types(function_node, ctx.source);

    // Extract function calls from the function body with qualified name resolution
    let calls = extract_function_calls(
        function_node,
        ctx.source,
        &resolution_ctx,
        &local_vars,
        &generic_bounds,
    );

    // Extract type references for USES relationships
    let mut type_refs = extract_type_references(function_node, ctx.source, &resolution_ctx);

    // Add trait bounds to type references (they also create USES relationships)
    let func_location = SourceLocation::from_tree_sitter_node(function_node);
    for trait_ref in &parsed_generics.bound_trait_refs {
        if !type_refs.iter().any(|r| r.target() == trait_ref.target) {
            // Use simple_name from ResolvedReference
            if let Ok(source_ref) = SourceReference::builder()
                .target(trait_ref.target.clone())
                .simple_name(trait_ref.simple_name.clone())
                .is_external(trait_ref.is_external)
                .location(func_location.clone())
                .ref_type(ReferenceType::TypeUsage)
                .build()
            {
                type_refs.push(source_ref);
            }
        }
    }

    // Build metadata
    let mut metadata = EntityMetadata {
        is_async,
        is_const,
        generic_params: generics.clone(),
        generic_bounds,
        is_generic: !generics.is_empty(),
        ..Default::default()
    };

    // Add unsafe as an attribute if applicable
    if is_unsafe {
        metadata
            .attributes
            .insert("unsafe".to_string(), "true".to_string());
    }

    // Build typed relationship data
    // Note: imports are NOT stored here. Per the spec (R-IMPORTS), imports are
    // a module-level relationship. They are collected by module_handlers.
    let relationships = EntityRelationshipData {
        calls,
        uses_types: type_refs,
        ..Default::default()
    };

    // Build the entity using the shared helper
    let entity = build_entity(
        components,
        EntityDetails {
            entity_type: EntityType::Function,
            language: Language::Rust,
            visibility: Some(visibility),
            documentation,
            content,
            metadata,
            signature: Some(FunctionSignature {
                parameters: parameters
                    .iter()
                    .map(|(name, ty)| (name.clone(), Some(ty.clone())))
                    .collect(),
                return_type,
                is_async,
                generics,
            }),
            relationships,
        },
    )?;

    Ok(vec![entity])
}
