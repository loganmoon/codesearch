//! Handlers for extracting Rust type definitions (struct, enum, trait)
//!
//! This module processes tree-sitter query matches for various Rust type
//! definitions and builds EntityData instances.

#![deny(warnings)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]

use crate::common::entity_building::{
    build_entity, extract_common_components, EntityDetails, ExtractionContext,
};
use crate::rust::entities::{FieldInfo, VariantInfo};
use crate::rust::handler_impls::common::{
    build_generic_bounds_map, extract_function_parameters, extract_generics_with_bounds,
    extract_preceding_doc_comments, extract_visibility, extract_visibility_from_node,
    extract_where_clause_bounds, find_capture_node, find_child_by_kind, format_generic_param,
    get_file_import_map, is_primitive_type, merge_parsed_generics, node_to_text,
    require_capture_node, ParsedGenerics, RustResolutionContext,
};
use crate::rust::handler_impls::constants::{capture_names, keywords, node_kinds, punctuation};
use codesearch_core::entities::{
    EntityMetadata, EntityRelationshipData, EntityType, FunctionSignature, Language, ReferenceType,
    SourceLocation, SourceReference, Visibility,
};
use codesearch_core::entity_id::generate_entity_id;
use codesearch_core::error::Result;
use codesearch_core::CodeEntity;
use std::collections::HashSet;
use std::path::Path;
use tracing::warn;
use tree_sitter::{Node, Query, QueryMatch};

// ============================================================================
// Type Handler Implementations
// ============================================================================

/// Generic extraction function that handles common patterns
fn extract_type_entity(
    ctx: &ExtractionContext,
    capture_name: &str,
    entity_type: EntityType,
    build_metadata: impl FnOnce(&ExtractionContext) -> (EntityMetadata, EntityRelationshipData),
) -> Result<CodeEntity> {
    let main_node = require_capture_node(ctx.query_match, ctx.query, capture_name)?;
    let (metadata, relationships) = build_metadata(ctx);
    build_entity_data(ctx, main_node, entity_type, metadata, relationships)
}

// ============================================================================
// Public API
// ============================================================================

/// Process a struct query match and extract entity data
#[allow(clippy::too_many_arguments)]
pub fn handle_struct_impl(
    query_match: &QueryMatch,
    query: &Query,
    source: &str,
    file_path: &Path,
    repository_id: &str,
    package_name: Option<&str>,
    source_root: Option<&Path>,
    repo_root: &Path,
) -> Result<Vec<CodeEntity>> {
    let ctx = ExtractionContext {
        query_match,
        query,
        source,
        file_path,
        repository_id,
        package_name,
        source_root,
        repo_root,
    };

    // Build ImportMap from file's imports for type resolution
    let struct_node = require_capture_node(query_match, query, capture_names::STRUCT)?;
    let import_map = get_file_import_map(struct_node, source);

    // Extract common components for parent_scope
    let components = extract_common_components(&ctx, capture_names::NAME, struct_node, "rust")?;

    // Derive module path from file path for qualified name resolution
    let module_path =
        source_root.and_then(|root| crate::rust::module_path::derive_module_path(file_path, root));

    // Build resolution context for qualified name normalization
    let resolution_ctx = RustResolutionContext {
        import_map: &import_map,
        parent_scope: components.parent_scope.as_deref(),
        package_name,
        current_module: module_path.as_deref(),
    };

    extract_type_entity(&ctx, capture_names::STRUCT, EntityType::Struct, |ctx| {
        // Extract generics with parsed bounds
        let parsed_generics = extract_generics_with_where(ctx, &resolution_ctx);

        // Build backward-compatible generic_params
        let generics: Vec<String> = parsed_generics
            .params
            .iter()
            .map(format_generic_param)
            .collect();

        // Build generic_bounds map
        let generic_bounds = build_generic_bounds_map(&parsed_generics);

        let derives = extract_derives(ctx);
        let (fields, is_tuple) = extract_struct_fields(ctx);

        let mut metadata = EntityMetadata::default();
        metadata.generic_params = generics;
        metadata.generic_bounds = generic_bounds;
        metadata.is_generic = !metadata.generic_params.is_empty();
        metadata.decorators = derives;

        // Store struct-specific info in attributes
        if is_tuple {
            metadata
                .attributes
                .insert("struct_type".to_string(), "tuple".to_string());
        }

        // Extract uses_types for relationships
        let mut uses_types_refs = if !fields.is_empty() {
            // Store field info as JSON in attributes
            if let Ok(json) = serde_json::to_string(&fields) {
                metadata.attributes.insert("fields".to_string(), json);
            }
            extract_field_type_refs(&fields, &resolution_ctx)
        } else {
            Vec::new()
        };

        // Add trait bounds to uses_types
        for trait_ref in &parsed_generics.bound_trait_refs {
            if !uses_types_refs.contains(trait_ref) {
                uses_types_refs.push(trait_ref.clone());
            }
        }

        // Store targets in attributes for backward compatibility
        if !uses_types_refs.is_empty() {
            let targets: Vec<&str> = uses_types_refs.iter().map(|r| r.target.as_str()).collect();
            if let Ok(json) = serde_json::to_string(&targets) {
                metadata.attributes.insert("uses_types".to_string(), json);
            }
        }

        // Build typed relationships
        // Note: imports are NOT stored here. Per the spec (R-IMPORTS), imports are
        // a module-level relationship. They are collected by module_handlers.
        let relationships = EntityRelationshipData {
            uses_types: resolved_refs_to_source_refs(&uses_types_refs),
            ..Default::default()
        };

        (metadata, relationships)
    })
    .map(|data| vec![data])
}

/// Process an enum query match and extract entity data
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
    let ctx = ExtractionContext {
        query_match,
        query,
        source,
        file_path,
        repository_id,
        package_name,
        source_root,
        repo_root,
    };

    // Build ImportMap from file's imports for type resolution
    let enum_node = require_capture_node(query_match, query, capture_names::ENUM)?;
    let import_map = get_file_import_map(enum_node, source);

    // Extract common components for parent_scope
    let components = extract_common_components(&ctx, capture_names::NAME, enum_node, "rust")?;

    // Derive module path from file path for qualified name resolution
    let module_path =
        source_root.and_then(|root| crate::rust::module_path::derive_module_path(file_path, root));

    // Build resolution context for qualified name normalization
    let resolution_ctx = RustResolutionContext {
        import_map: &import_map,
        parent_scope: components.parent_scope.as_deref(),
        package_name,
        current_module: module_path.as_deref(),
    };

    extract_type_entity(&ctx, capture_names::ENUM, EntityType::Enum, |ctx| {
        // Extract generics with parsed bounds
        let parsed_generics = extract_generics_with_where(ctx, &resolution_ctx);

        // Build backward-compatible generic_params
        let generics: Vec<String> = parsed_generics
            .params
            .iter()
            .map(format_generic_param)
            .collect();

        // Build generic_bounds map
        let generic_bounds = build_generic_bounds_map(&parsed_generics);

        let derives = extract_derives(ctx);
        let variants = extract_enum_variants(ctx);

        let mut metadata = EntityMetadata::default();
        metadata.generic_params = generics;
        metadata.generic_bounds = generic_bounds;
        metadata.is_generic = !metadata.generic_params.is_empty();
        metadata.decorators = derives;

        // Extract uses_types for relationships
        let mut uses_types_refs = if !variants.is_empty() {
            // Store variant info as JSON in attributes
            if let Ok(json) = serde_json::to_string(&variants) {
                metadata.attributes.insert("variants".to_string(), json);
            }
            extract_variant_type_refs(&variants, &resolution_ctx)
        } else {
            Vec::new()
        };

        // Add trait bounds to uses_types
        for trait_ref in &parsed_generics.bound_trait_refs {
            if !uses_types_refs.contains(trait_ref) {
                uses_types_refs.push(trait_ref.clone());
            }
        }

        // Store targets in attributes for backward compatibility
        if !uses_types_refs.is_empty() {
            let targets: Vec<&str> = uses_types_refs.iter().map(|r| r.target.as_str()).collect();
            if let Ok(json) = serde_json::to_string(&targets) {
                metadata.attributes.insert("uses_types".to_string(), json);
            }
        }

        // Build typed relationships
        // Note: imports are NOT stored here. Per the spec (R-IMPORTS), imports are
        // a module-level relationship. They are collected by module_handlers.
        let relationships = EntityRelationshipData {
            uses_types: resolved_refs_to_source_refs(&uses_types_refs),
            ..Default::default()
        };

        (metadata, relationships)
    })
    .map(|data| vec![data])
}

/// Process a trait query match and extract entity data
#[allow(clippy::too_many_arguments)]
pub fn handle_trait_impl(
    query_match: &QueryMatch,
    query: &Query,
    source: &str,
    file_path: &Path,
    repository_id: &str,
    package_name: Option<&str>,
    source_root: Option<&Path>,
    repo_root: &Path,
) -> Result<Vec<CodeEntity>> {
    let ctx = ExtractionContext {
        query_match,
        query,
        source,
        file_path,
        repository_id,
        package_name,
        source_root,
        repo_root,
    };

    // Build ImportMap from file's imports for type resolution
    let trait_node = require_capture_node(query_match, query, capture_names::TRAIT)?;
    let import_map = get_file_import_map(trait_node, source);

    // Extract common components for parent_scope
    let components = extract_common_components(&ctx, capture_names::NAME, trait_node, "rust")?;

    // Derive module path from file path for qualified name resolution
    let module_path =
        source_root.and_then(|root| crate::rust::module_path::derive_module_path(file_path, root));

    // Build resolution context for qualified name normalization
    let resolution_ctx = RustResolutionContext {
        import_map: &import_map,
        parent_scope: components.parent_scope.as_deref(),
        package_name,
        current_module: module_path.as_deref(),
    };

    let trait_entity = extract_type_entity(&ctx, capture_names::TRAIT, EntityType::Trait, |ctx| {
        // Extract generics with parsed bounds
        let parsed_generics = extract_generics_with_where(ctx, &resolution_ctx);

        // Build backward-compatible generic_params
        let generics: Vec<String> = parsed_generics
            .params
            .iter()
            .map(format_generic_param)
            .collect();

        // Build generic_bounds map
        let generic_bounds = build_generic_bounds_map(&parsed_generics);

        // Extract supertrait bounds (trait Foo: Bar + Baz)
        let bounds = extract_trait_bounds(ctx);
        let (associated_types, methods) = extract_trait_members(ctx);
        let is_unsafe = check_trait_is_unsafe(ctx);

        let mut metadata = EntityMetadata::default();
        metadata.generic_params = generics;
        metadata.generic_bounds = generic_bounds;
        metadata.is_generic = !metadata.generic_params.is_empty();
        metadata.is_abstract = true; // Traits are abstract by nature

        // Add unsafe attribute if applicable
        if is_unsafe {
            metadata
                .attributes
                .insert("unsafe".to_string(), "true".to_string());
        }

        // Store trait-specific info in attributes
        if !bounds.is_empty() {
            metadata
                .attributes
                .insert("bounds".to_string(), bounds.join(" + "));
        }
        if !associated_types.is_empty() {
            metadata
                .attributes
                .insert("associated_types".to_string(), associated_types.join(","));
        }
        if !methods.is_empty() {
            metadata
                .attributes
                .insert("methods".to_string(), methods.join(","));
        }

        // Store resolved supertraits separately for EXTENDS_INTERFACE relationships
        // NOTE: Lifetimes are now excluded at query time (in extract_trait_bounds),
        // so no filtering is needed here
        // bounds are bare identifiers from AST, use as both name and simple_name
        let supertrait_refs: Vec<ResolvedReference> = bounds
            .iter()
            .map(|b| resolution_ctx.resolve(b, b))
            .collect();
        let supertraits: Vec<String> = supertrait_refs.iter().map(|r| r.target.clone()).collect();
        if !supertraits.is_empty() {
            match serde_json::to_string(&supertraits) {
                Ok(json) => {
                    metadata.attributes.insert("supertraits".to_string(), json);
                }
                Err(e) => {
                    warn!("Failed to serialize supertraits: {e}");
                }
            }
        }

        // Build uses_types from generic bounds only (supertraits are stored separately)
        let uses_types_refs: Vec<ResolvedReference> = parsed_generics.bound_trait_refs.clone();
        if !uses_types_refs.is_empty() {
            let targets: Vec<&str> = uses_types_refs.iter().map(|r| r.target.as_str()).collect();
            match serde_json::to_string(&targets) {
                Ok(json) => {
                    metadata.attributes.insert("uses_types".to_string(), json);
                }
                Err(e) => {
                    warn!("Failed to serialize uses_types: {e}");
                }
            }
        }

        // Build typed relationships
        // Note: imports are NOT stored here. Per the spec (R-IMPORTS), imports are
        // a module-level relationship. They are collected by module_handlers.
        let relationships = EntityRelationshipData {
            uses_types: resolved_refs_to_source_refs(&uses_types_refs),
            supertraits,
            ..Default::default()
        };

        (metadata, relationships)
    })?;

    let mut entities = vec![trait_entity.clone()];

    // Extract trait methods as separate entities
    if let Some(body_node) = find_capture_node(query_match, query, capture_names::TRAIT_BODY) {
        let trait_methods = extract_trait_method_entities(
            body_node,
            source,
            file_path,
            repository_id,
            &trait_entity.qualified_name,
        );
        entities.extend(trait_methods);
    }

    Ok(entities)
}

// ============================================================================
// Core Extraction Functions
// ============================================================================

/// Build entity data from extracted information
fn build_entity_data(
    ctx: &ExtractionContext,
    main_node: Node,
    entity_type: EntityType,
    metadata: EntityMetadata,
    relationships: EntityRelationshipData,
) -> Result<CodeEntity> {
    // Extract common components using the shared helper
    let components = extract_common_components(ctx, capture_names::NAME, main_node, "rust")?;

    // Extract Rust-specific: visibility, documentation, content
    let visibility = extract_visibility(ctx.query_match, ctx.query);
    let documentation = extract_preceding_doc_comments(main_node, ctx.source);
    let content = node_to_text(main_node, ctx.source).ok();

    // Build the entity using the shared helper
    build_entity(
        components,
        EntityDetails {
            entity_type,
            language: Language::Rust,
            visibility: Some(visibility),
            documentation,
            content,
            metadata,
            signature: None,
            relationships,
        },
    )
}

// ============================================================================
// Generic Parameter Extraction
// ============================================================================

/// Extract generic parameters with parsed bounds
fn extract_generics_with_where(
    ctx: &ExtractionContext,
    resolution_ctx: &RustResolutionContext,
) -> ParsedGenerics {
    // Extract inline generics
    let mut parsed_generics =
        find_capture_node(ctx.query_match, ctx.query, capture_names::GENERICS)
            .map(|node| extract_generics_with_bounds(node, ctx.source, resolution_ctx))
            .unwrap_or_default();

    // Merge where clause bounds if present
    if let Some(where_node) = find_capture_node(ctx.query_match, ctx.query, capture_names::WHERE) {
        let where_bounds = extract_where_clause_bounds(where_node, ctx.source, resolution_ctx);
        merge_parsed_generics(&mut parsed_generics, where_bounds);
    }

    parsed_generics
}

// ============================================================================
// Derive Attribute Extraction
// ============================================================================

/// Extract derive attributes
fn extract_derives(ctx: &ExtractionContext) -> Vec<String> {
    ctx.query_match
        .captures
        .first()
        .map(|capture| {
            let mut derives = Vec::new();
            let mut current = capture.node.prev_sibling();

            // Walk backwards through siblings to find attributes
            while let Some(node) = current {
                if node.kind() == node_kinds::ATTRIBUTE_ITEM {
                    if let Ok(text) = node_to_text(node, ctx.source) {
                        // Simple pattern matching for #[derive(...)]
                        if text.contains("derive(") {
                            // Extract content between parentheses - use split for UTF-8 safety
                            if let Some(after_open) = text.split_once('(') {
                                if let Some((derive_content, _)) = after_open.1.rsplit_once(')') {
                                    // Split by comma and clean up
                                    derives.extend(
                                        derive_content
                                            .split(',')
                                            .map(|s| s.trim().to_string())
                                            .filter(|s| !s.is_empty()),
                                    );
                                }
                            }
                        }
                    }
                }
                current = node.prev_sibling();
            }

            derives
        })
        .unwrap_or_default()
}

// ============================================================================
// Struct Field Extraction
// ============================================================================

/// Extract struct fields
fn extract_struct_fields(ctx: &ExtractionContext) -> (Vec<FieldInfo>, bool) {
    find_capture_node(ctx.query_match, ctx.query, capture_names::FIELDS)
        .map(|node| {
            let is_tuple = node.kind() == node_kinds::ORDERED_FIELD_DECLARATION_LIST;
            let fields = if is_tuple {
                parse_tuple_fields(node, ctx.source)
            } else {
                parse_named_fields(node, ctx.source)
            };
            (fields, is_tuple)
        })
        .unwrap_or((Vec::new(), false))
}

/// Parse named fields from a struct
fn parse_named_fields(node: Node, source: &str) -> Vec<FieldInfo> {
    let mut cursor = node.walk();

    node.children(&mut cursor)
        .filter(|child| child.kind() == node_kinds::FIELD_DECLARATION)
        .filter_map(|child| {
            // Get the full field text and parse it
            node_to_text(child, source).ok().and_then(|text| {
                // Check for visibility - distinguish pub, pub(crate), pub(super), etc.
                let trimmed = text.trim_start();
                let visibility = if trimmed.starts_with("pub(") {
                    // pub(crate), pub(super), pub(in path) -> Internal
                    Visibility::Internal
                } else if trimmed.starts_with("pub") {
                    // Just pub -> Public
                    Visibility::Public
                } else {
                    Visibility::Private
                };

                // Find field name and type separated by colon
                // Use split_once for UTF-8 safety
                if let Some((name_part, type_part)) = text.split_once(':') {
                    // Extract the field name by taking the last word before the colon
                    // This handles pub, pub(crate), pub(super), etc.
                    let field_name = name_part
                        .split_whitespace()
                        .last()
                        .unwrap_or(name_part.trim())
                        .to_string();
                    let type_part = type_part.trim().trim_end_matches(',');

                    Some(FieldInfo {
                        name: field_name,
                        field_type: type_part.to_string(),
                        visibility,
                        attributes: Vec::new(),
                    })
                } else {
                    None
                }
            })
        })
        .collect()
}

/// Parse tuple fields from a struct
fn parse_tuple_fields(node: Node, source: &str) -> Vec<FieldInfo> {
    let mut cursor = node.walk();
    let mut fields = Vec::new();
    let mut index = 0;
    let mut next_visibility = Visibility::Private;

    for child in node.children(&mut cursor) {
        match child.kind() {
            // Skip punctuation
            punctuation::OPEN_PAREN | punctuation::CLOSE_PAREN | punctuation::COMMA => continue,

            // Track visibility for next field
            node_kinds::VISIBILITY_MODIFIER => {
                next_visibility = extract_visibility_from_node(child);
            }

            // Process type nodes
            _ => {
                if let Ok(type_text) = node_to_text(child, source) {
                    let trimmed = type_text.trim();
                    if !trimmed.is_empty() {
                        fields.push(FieldInfo {
                            name: index.to_string(),
                            field_type: trimmed.to_string(),
                            visibility: next_visibility,
                            attributes: Vec::new(),
                        });
                        index += 1;
                        next_visibility = Visibility::Private;
                    }
                }
            }
        }
    }

    fields
}

// ============================================================================
// Enum Variant Extraction
// ============================================================================

/// Extract enum variants
fn extract_enum_variants(ctx: &ExtractionContext) -> Vec<VariantInfo> {
    find_capture_node(ctx.query_match, ctx.query, capture_names::ENUM_BODY)
        .map(|node| {
            let mut cursor = node.walk();
            node.children(&mut cursor)
                .filter(|child| child.kind() == node_kinds::ENUM_VARIANT)
                .filter_map(|child| parse_enum_variant(child, ctx.source))
                .collect()
        })
        .unwrap_or_default()
}

/// Parse a single enum variant
fn parse_enum_variant(node: Node, source: &str) -> Option<VariantInfo> {
    let text = node_to_text(node, source).ok()?;

    // Extract variant name (first identifier)
    let name = text
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .find(|s| !s.is_empty())?;

    // Check for discriminant (= value) - use split_once for UTF-8 safety
    let discriminant = text
        .split_once('=')
        .map(|(_, value)| value.trim().trim_end_matches(',').to_string());

    // Extract fields based on variant type
    let fields = if text.contains('(') && text.contains(')') {
        // Tuple variant - extract fields between parentheses - use split for UTF-8 safety
        if let Some(after_paren) = text.split_once('(') {
            if let Some((fields_text, _)) = after_paren.1.split_once(')') {
                if !fields_text.trim().is_empty() {
                    fields_text
                        .split(',')
                        .enumerate()
                        .map(|(i, field)| FieldInfo {
                            name: i.to_string(),
                            field_type: field.trim().to_string(),
                            visibility: Visibility::Private,
                            attributes: Vec::new(),
                        })
                        .collect()
                } else {
                    Vec::new()
                }
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        }
    } else if text.contains('{') && text.contains('}') {
        // Struct variant - parse named fields
        let mut cursor = node.walk();
        let fields = node
            .children(&mut cursor)
            .find(|child| child.kind() == node_kinds::FIELD_DECLARATION_LIST)
            .map(|child| parse_named_fields(child, source))
            .unwrap_or_default();
        fields
    } else {
        Vec::new()
    };

    Some(VariantInfo {
        name: name.to_string(),
        fields,
        discriminant,
    })
}

// ============================================================================
// Trait Member Extraction
// ============================================================================

/// Extract trait bounds
fn extract_trait_bounds(ctx: &ExtractionContext) -> Vec<String> {
    let Some(bounds_node) = find_capture_node(ctx.query_match, ctx.query, capture_names::BOUNDS)
    else {
        return Vec::new();
    };

    // Query for type identifiers within trait bounds
    // NOTE: We exclude (lifetime) intentionally - lifetimes are not trait bounds
    // and should not create EXTENDS_INTERFACE relationships
    let query_source = r#"
        [(type_identifier) (scoped_type_identifier)] @bound
    "#;

    let language = tree_sitter_rust::LANGUAGE.into();
    let query = match tree_sitter::Query::new(&language, query_source) {
        Ok(q) => q,
        Err(e) => {
            warn!(
                "Failed to compile tree-sitter query for trait bounds: {e}. \
                 This indicates a bug in the query definition."
            );
            return Vec::new();
        }
    };

    let mut cursor = tree_sitter::QueryCursor::new();
    let mut bounds = Vec::new();

    let mut matches = cursor.matches(&query, bounds_node, ctx.source.as_bytes());
    while let Some(m) = streaming_iterator::StreamingIterator::next(&mut matches) {
        for capture in m.captures {
            if let Ok(text) = capture.node.utf8_text(ctx.source.as_bytes()) {
                bounds.push(text.to_string());
            }
        }
    }

    bounds
}

/// Extract trait members (associated types and methods)
fn extract_trait_members(ctx: &ExtractionContext) -> (Vec<String>, Vec<String>) {
    find_capture_node(ctx.query_match, ctx.query, capture_names::TRAIT_BODY)
        .map(|node| {
            let mut cursor = node.walk();
            let mut associated_types = Vec::new();
            let mut methods = Vec::new();

            for child in node.children(&mut cursor) {
                match child.kind() {
                    node_kinds::ASSOCIATED_TYPE => {
                        if let Some(type_name) = extract_associated_type_name(child, ctx.source) {
                            associated_types.push(type_name);
                        }
                    }
                    node_kinds::FUNCTION_SIGNATURE_ITEM | node_kinds::FUNCTION_ITEM => {
                        if let Some(method_name) = extract_method_name(child, ctx.source) {
                            methods.push(method_name);
                        }
                    }
                    _ => {}
                }
            }

            (associated_types, methods)
        })
        .unwrap_or((Vec::new(), Vec::new()))
}

/// Extract associated type name
fn extract_associated_type_name(node: Node, source: &str) -> Option<String> {
    let mut cursor = node.walk();
    let children: Vec<_> = node.children(&mut cursor).collect();
    children
        .into_iter()
        .find(|child| child.kind() == node_kinds::TYPE_IDENTIFIER)
        .and_then(|child| node_to_text(child, source).ok())
}

/// Extract method name
fn extract_method_name(node: Node, source: &str) -> Option<String> {
    let mut cursor = node.walk();
    let children: Vec<_> = node.children(&mut cursor).collect();
    children
        .into_iter()
        .find(|child| {
            child.kind() == node_kinds::IDENTIFIER
                && node_to_text(*child, source)
                    .ok()
                    .filter(|text| text != keywords::FN)
                    .is_some()
        })
        .and_then(|child| node_to_text(child, source).ok())
}

/// Extract trait methods as separate CodeEntity objects
fn extract_trait_method_entities(
    body_node: Node,
    source: &str,
    file_path: &Path,
    repository_id: &str,
    trait_qualified_name: &str,
) -> Vec<CodeEntity> {
    let mut entities = Vec::new();
    let mut cursor = body_node.walk();

    for child in body_node.children(&mut cursor) {
        match child.kind() {
            node_kinds::FUNCTION_SIGNATURE_ITEM | node_kinds::FUNCTION_ITEM => {
                if let Some(method_entity) = extract_single_trait_method(
                    child,
                    source,
                    file_path,
                    repository_id,
                    trait_qualified_name,
                ) {
                    entities.push(method_entity);
                }
            }
            _ => {}
        }
    }

    entities
}

/// Extract a single trait method as a CodeEntity
fn extract_single_trait_method(
    method_node: Node,
    source: &str,
    file_path: &Path,
    repository_id: &str,
    trait_qualified_name: &str,
) -> Option<CodeEntity> {
    // Extract method name
    let method_name = extract_method_name(method_node, source)?;

    // Build qualified name: TraitName::method_name
    let qualified_name = format!("{trait_qualified_name}::{method_name}");

    // Extract parameters - convert to (String, Option<String>) format
    let parameters: Vec<(String, Option<String>)> = find_child_by_kind(method_node, "parameters")
        .and_then(|params_node| extract_function_parameters(params_node, source).ok())
        .unwrap_or_default()
        .into_iter()
        .map(|(name, type_str)| (name, Some(type_str)))
        .collect();

    // Extract return type
    let return_type = find_child_by_kind(method_node, "return_type")
        .and_then(|node| node_to_text(node, source).ok())
        .map(|s: String| s.trim_start_matches("->").trim().to_string());

    // Check if it's async (look for async keyword in function_modifiers)
    let is_async = {
        let mut cursor = method_node.walk();
        let children: Vec<_> = method_node.children(&mut cursor).collect();
        children.iter().any(|c| {
            if c.kind() == "function_modifiers" {
                let mut c_cursor = c.walk();
                let mods: Vec<_> = c.children(&mut c_cursor).collect();
                mods.iter().any(|m| m.kind() == "async")
            } else {
                false
            }
        })
    };

    // Determine if method has body (function_item) or just signature (function_signature_item)
    let has_body = method_node.kind() == node_kinds::FUNCTION_ITEM;

    // Build signature
    let signature = FunctionSignature {
        parameters,
        return_type,
        is_async,
        generics: Vec::new(),
    };

    // Build metadata
    let mut metadata = EntityMetadata {
        is_async,
        is_abstract: !has_body, // Methods without body are abstract
        ..Default::default()
    };

    // Store that this is a trait method
    metadata
        .attributes
        .insert("trait_method".to_string(), "true".to_string());

    // Extract documentation
    let documentation_summary = extract_preceding_doc_comments(method_node, source);

    // Get location and content
    let location = SourceLocation::from_tree_sitter_node(method_node);
    let content = node_to_text(method_node, source).ok();

    // Generate entity_id
    let file_path_str = file_path.to_str()?;
    let entity_id = generate_entity_id(repository_id, file_path_str, &qualified_name);

    Some(CodeEntity {
        entity_id,
        repository_id: repository_id.to_string(),
        entity_type: EntityType::Method,
        name: method_name,
        qualified_name,
        path_entity_identifier: None,
        parent_scope: Some(trait_qualified_name.to_string()),
        dependencies: Vec::new(),
        documentation_summary,
        file_path: file_path.to_path_buf(),
        language: Language::Rust,
        content,
        metadata,
        signature: Some(signature),
        visibility: None, // Trait methods don't have visibility - it's derived from the trait
        location,
        relationships: Default::default(), // Trait method signatures don't have relationship data
    })
}

/// Check if a trait has the unsafe modifier
fn check_trait_is_unsafe(ctx: &ExtractionContext) -> bool {
    // Get the trait node (this is the trait_item node)
    if let Ok(trait_node) = require_capture_node(ctx.query_match, ctx.query, capture_names::TRAIT) {
        // The trait_item node contains the entire trait definition
        // Check its children for the 'unsafe' keyword
        let mut cursor = trait_node.walk();
        for child in trait_node.children(&mut cursor) {
            if child.kind() == "unsafe" {
                return true;
            }
            // Stop when we reach 'trait' keyword - unsafe should come before it
            if child.kind() == "trait" {
                break;
            }
        }
    }
    false
}

// ============================================================================
// Type Resolution Helpers
// ============================================================================

use crate::rust::import_resolution::ResolvedReference;

/// Convert a list of resolved references to SourceReferences
///
/// Used to populate the typed `uses_types` field in EntityRelationshipData.
/// Location is set to a default value since we don't have precise source locations
/// for field types in the current extraction.
fn resolved_refs_to_source_refs(refs: &[ResolvedReference]) -> Vec<SourceReference> {
    refs.iter()
        .map(|r| {
            SourceReference::new(
                r.target.clone(),
                r.simple_name.clone(),
                r.is_external,
                SourceLocation::default(),
                ReferenceType::TypeUsage,
            )
        })
        .collect()
}

/// Extract and resolve field types for USES relationships
fn extract_field_type_refs(
    fields: &[FieldInfo],
    ctx: &RustResolutionContext,
) -> Vec<ResolvedReference> {
    let mut seen = HashSet::new();
    let mut result = Vec::new();

    for field in fields {
        for type_name in extract_type_names_from_field_type(&field.field_type) {
            if !is_primitive_type(&type_name) {
                // type_name from string parsing is the simple_name
                let resolved = ctx.resolve(&type_name, &type_name);
                if seen.insert(resolved.clone()) {
                    result.push(resolved);
                }
            }
        }
    }

    result
}

/// Extract and resolve types from enum variant fields for USES relationships
fn extract_variant_type_refs(
    variants: &[VariantInfo],
    ctx: &RustResolutionContext,
) -> Vec<ResolvedReference> {
    let mut seen = HashSet::new();
    let mut result = Vec::new();

    for variant in variants {
        for field in &variant.fields {
            for type_name in extract_type_names_from_field_type(&field.field_type) {
                if !is_primitive_type(&type_name) {
                    // type_name from string parsing is the simple_name
                    let resolved = ctx.resolve(&type_name, &type_name);
                    if seen.insert(resolved.clone()) {
                        result.push(resolved);
                    }
                }
            }
        }
    }

    result
}

/// Extract type names from a field type string
///
/// Handles common patterns:
/// - Simple types: `Foo` -> ["Foo"]
/// - Generic types: `Vec<Foo>` -> ["Vec", "Foo"]
/// - References: `&Foo` or `&mut Foo` -> ["Foo"]
/// - Option/Result: `Option<Bar>` -> ["Option", "Bar"]
/// - Tuples: `(Foo, Bar)` -> ["Foo", "Bar"]
/// - Paths: `std::io::Error` -> ["std::io::Error"]
fn extract_type_names_from_field_type(field_type: &str) -> Vec<String> {
    let mut types = Vec::new();
    let mut current = String::new();
    let mut depth: u32 = 0;

    // Remove leading & and &mut
    let cleaned = field_type
        .trim()
        .trim_start_matches('&')
        .trim_start_matches("mut ")
        .trim();

    for ch in cleaned.chars() {
        match ch {
            '<' | '(' | '[' => {
                if depth == 0 && !current.is_empty() {
                    let trimmed = current.trim().to_string();
                    if is_valid_type_name(&trimmed) {
                        types.push(trimmed);
                    }
                    current.clear();
                }
                depth += 1;
            }
            '>' | ')' | ']' => {
                depth = depth.saturating_sub(1);
                if depth == 0 && !current.is_empty() {
                    let trimmed = current.trim().to_string();
                    if is_valid_type_name(&trimmed) {
                        types.push(trimmed);
                    }
                    current.clear();
                }
            }
            ',' | ' ' if depth <= 1 => {
                if !current.is_empty() {
                    let trimmed = current.trim().to_string();
                    if is_valid_type_name(&trimmed) {
                        types.push(trimmed);
                    }
                    current.clear();
                }
            }
            _ => {
                current.push(ch);
            }
        }
    }

    // Don't forget the last type
    if !current.is_empty() {
        let trimmed = current.trim().to_string();
        if is_valid_type_name(&trimmed) {
            types.push(trimmed);
        }
    }

    types
}

/// Check if a string is a valid type name (not empty, not a keyword, not punctuation)
fn is_valid_type_name(name: &str) -> bool {
    !name.is_empty()
        && !name.chars().all(|c| !c.is_alphanumeric())
        && name
            .chars()
            .next()
            .is_some_and(|c| c.is_alphabetic() || c == '_')
}

/// Extract type references from a type expression string.
///
/// This is a public wrapper around `extract_type_names_from_field_type` that filters
/// primitives and resolves type names using the provided context.
///
/// Used by type_alias_handlers to extract USES relationships.
pub(crate) fn extract_type_refs_from_type_expr(
    type_expr: &str,
    ctx: &RustResolutionContext,
    generic_params: &[String],
) -> Vec<ResolvedReference> {
    let mut seen = HashSet::new();
    let mut result = Vec::new();

    for type_name in extract_type_names_from_field_type(type_expr) {
        // Skip primitives
        if is_primitive_type(&type_name) {
            continue;
        }
        // Skip generic type parameters (e.g., T, U, etc.)
        if generic_params.contains(&type_name) {
            continue;
        }
        // Resolve through imports - type_name from string parsing is the simple_name
        let resolved = ctx.resolve(&type_name, &type_name);
        if seen.insert(resolved.clone()) {
            result.push(resolved);
        }
    }

    result
}
