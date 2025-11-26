//! Handlers for extracting Rust type definitions (struct, enum, trait)
//!
//! This module processes tree-sitter query matches for various Rust type
//! definitions and builds EntityData instances.

#![deny(warnings)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]

use crate::rust::entities::{FieldInfo, VariantInfo};
use crate::rust::handler_impls::common::{
    build_entity, extract_common_components, extract_generics_from_node, find_capture_node,
    node_to_text, require_capture_node,
};
use crate::rust::handler_impls::constants::{capture_names, keywords, node_kinds, punctuation};
use codesearch_core::entities::{EntityMetadata, EntityType, Visibility};
use codesearch_core::error::Result;
use codesearch_core::CodeEntity;
use std::path::Path;
use tree_sitter::{Node, Query, QueryMatch};

// ============================================================================
// Extraction Context
// ============================================================================

/// Context for type extraction operations
struct ExtractionContext<'a, 'b> {
    query_match: &'a QueryMatch<'a, 'b>,
    query: &'a Query,
    source: &'a str,
    file_path: &'a Path,
    repository_id: &'a str,
}

impl<'a, 'b> ExtractionContext<'a, 'b> {
    fn new(
        query_match: &'a QueryMatch<'a, 'b>,
        query: &'a Query,
        source: &'a str,
        file_path: &'a Path,
        repository_id: &'a str,
    ) -> Self {
        Self {
            query_match,
            query,
            source,
            file_path,
            repository_id,
        }
    }
}

// ============================================================================
// Type Handler Implementations
// ============================================================================

/// Generic extraction function that handles common patterns
fn extract_type_entity(
    ctx: &ExtractionContext,
    capture_name: &str,
    entity_type: EntityType,
    build_metadata: impl FnOnce(&ExtractionContext) -> EntityMetadata,
) -> Result<CodeEntity> {
    let main_node = require_capture_node(ctx.query_match, ctx.query, capture_name)?;
    let metadata = build_metadata(ctx);
    build_entity_data(ctx, main_node, entity_type, metadata)
}

// ============================================================================
// Public API
// ============================================================================

/// Process a struct query match and extract entity data
pub fn handle_struct_impl(
    query_match: &QueryMatch,
    query: &Query,
    source: &str,
    file_path: &Path,
    repository_id: &str,
) -> Result<Vec<CodeEntity>> {
    let ctx = ExtractionContext::new(query_match, query, source, file_path, repository_id);
    extract_type_entity(&ctx, capture_names::STRUCT, EntityType::Struct, |ctx| {
        let generics = extract_generics(ctx);
        let derives = extract_derives(ctx);
        let (fields, is_tuple) = extract_struct_fields(ctx);

        let mut metadata = EntityMetadata::default();
        metadata.generic_params = generics;
        metadata.is_generic = !metadata.generic_params.is_empty();
        metadata.decorators = derives;

        // Store struct-specific info in attributes
        if is_tuple {
            metadata
                .attributes
                .insert("struct_type".to_string(), "tuple".to_string());
        }

        // Store field info as JSON in attributes
        if !fields.is_empty() {
            if let Ok(json) = serde_json::to_string(&fields) {
                metadata.attributes.insert("fields".to_string(), json);
            }
        }

        metadata
    })
    .map(|data| vec![data])
}

/// Process an enum query match and extract entity data
pub fn handle_enum_impl(
    query_match: &QueryMatch,
    query: &Query,
    source: &str,
    file_path: &Path,
    repository_id: &str,
) -> Result<Vec<CodeEntity>> {
    let ctx = ExtractionContext::new(query_match, query, source, file_path, repository_id);
    extract_type_entity(&ctx, capture_names::ENUM, EntityType::Enum, |ctx| {
        let generics = extract_generics(ctx);
        let derives = extract_derives(ctx);
        let variants = extract_enum_variants(ctx);

        let mut metadata = EntityMetadata::default();
        metadata.generic_params = generics;
        metadata.is_generic = !metadata.generic_params.is_empty();
        metadata.decorators = derives;

        // Store variant info as JSON in attributes
        if !variants.is_empty() {
            if let Ok(json) = serde_json::to_string(&variants) {
                metadata.attributes.insert("variants".to_string(), json);
            }
        }

        metadata
    })
    .map(|data| vec![data])
}

/// Process a trait query match and extract entity data
pub fn handle_trait_impl(
    query_match: &QueryMatch,
    query: &Query,
    source: &str,
    file_path: &Path,
    repository_id: &str,
) -> Result<Vec<CodeEntity>> {
    let ctx = ExtractionContext::new(query_match, query, source, file_path, repository_id);
    extract_type_entity(&ctx, capture_names::TRAIT, EntityType::Trait, |ctx| {
        let generics = extract_generics(ctx);
        let bounds = extract_trait_bounds(ctx);
        let (associated_types, methods) = extract_trait_members(ctx);
        let is_unsafe = check_trait_is_unsafe(ctx);

        let mut metadata = EntityMetadata::default();
        metadata.generic_params = generics;
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

        metadata
    })
    .map(|data| vec![data])
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
) -> Result<CodeEntity> {
    // Extract common components using the shared helper
    let components = extract_common_components(
        ctx.query_match,
        ctx.query,
        ctx.source,
        ctx.file_path,
        ctx.repository_id,
        capture_names::NAME,
        main_node,
    )?;

    // Build the entity using the common helper
    build_entity(components, entity_type, metadata, None)
}

// ============================================================================
// Generic Parameter Extraction
// ============================================================================

/// Extract generic parameters
fn extract_generics(ctx: &ExtractionContext) -> Vec<String> {
    find_capture_node(ctx.query_match, ctx.query, capture_names::GENERICS)
        .map(|node| extract_generics_from_node(node, ctx.source))
        .unwrap_or_default()
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
                // Check for visibility
                let visibility = if text.trim_start().starts_with("pub") {
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
                next_visibility = Visibility::Public;
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
    find_capture_node(ctx.query_match, ctx.query, capture_names::BOUNDS)
        .map(|node| {
            let mut cursor = node.walk();
            node.children(&mut cursor)
                .filter_map(|child| {
                    match child.kind() {
                        node_kinds::TYPE_IDENTIFIER
                        | node_kinds::SCOPED_TYPE_IDENTIFIER
                        | node_kinds::LIFETIME => node_to_text(child, ctx.source).ok(),
                        punctuation::PLUS => None, // Skip operators
                        _ => None,
                    }
                })
                .collect()
        })
        .unwrap_or_default()
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
