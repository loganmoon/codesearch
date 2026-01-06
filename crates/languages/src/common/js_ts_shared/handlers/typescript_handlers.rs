//! TypeScript-specific entity handlers

use crate::common::entity_building::{
    build_entity, extract_common_components, extract_common_components_with_name, EntityDetails,
    ExtractionContext,
};
use crate::common::js_ts_shared::TypeScript;
use crate::common::language_extractors::extract_main_node;
use crate::common::node_to_text;
use crate::define_handler;
use codesearch_core::entities::{EntityMetadata, EntityType, Language};
use codesearch_core::error::Result;
use codesearch_core::CodeEntity;

use super::super::visibility::extract_visibility;
use super::common::{extract_interface_extends_relationships, extract_preceding_doc_comments};

define_handler!(TypeScript, handle_interface_impl, "interface", Interface, relationships: extract_interface_extends_relationships);
define_handler!(TypeScript, handle_type_alias_impl, "type_alias", TypeAlias);
define_handler!(TypeScript, handle_namespace_impl, "namespace", Module);
define_handler!(
    TypeScript,
    handle_enum_member_impl,
    "enum_member",
    EnumVariant
);

/// Handle interface property signature extraction
///
/// Interface members are always Public in TypeScript.
pub(crate) fn handle_interface_property_impl(ctx: &ExtractionContext) -> Result<Vec<CodeEntity>> {
    let node = match extract_main_node(ctx.query_match, ctx.query, &["interface_property"]) {
        Some(n) => n,
        None => return Ok(Vec::new()),
    };

    let components = extract_common_components(ctx, "name", node, "typescript")?;
    let documentation = extract_preceding_doc_comments(node, ctx.source);
    let content = node_to_text(node, ctx.source).ok();

    // Interface members are always Public
    let entity = build_entity(
        components,
        EntityDetails {
            entity_type: EntityType::Property,
            language: Language::TypeScript,
            visibility: Some(codesearch_core::Visibility::Public),
            documentation,
            content,
            metadata: EntityMetadata::default(),
            signature: None,
            relationships: Default::default(),
        },
    )?;

    Ok(vec![entity])
}

/// Handle interface method signature extraction
///
/// Interface members are always Public in TypeScript.
pub(crate) fn handle_interface_method_impl(ctx: &ExtractionContext) -> Result<Vec<CodeEntity>> {
    let node = match extract_main_node(ctx.query_match, ctx.query, &["interface_method"]) {
        Some(n) => n,
        None => return Ok(Vec::new()),
    };

    let components = extract_common_components(ctx, "name", node, "typescript")?;
    let documentation = extract_preceding_doc_comments(node, ctx.source);
    let content = node_to_text(node, ctx.source).ok();

    // Interface members are always Public
    let entity = build_entity(
        components,
        EntityDetails {
            entity_type: EntityType::Method,
            language: Language::TypeScript,
            visibility: Some(codesearch_core::Visibility::Public),
            documentation,
            content,
            metadata: EntityMetadata::default(),
            signature: None,
            relationships: Default::default(),
        },
    )?;

    Ok(vec![entity])
}

/// Handle enum declaration extraction
///
/// This handler has custom logic to detect const enums,
/// so it cannot use the macro.
pub(crate) fn handle_enum_impl(ctx: &ExtractionContext) -> Result<Vec<CodeEntity>> {
    let node = match extract_main_node(ctx.query_match, ctx.query, &["enum"]) {
        Some(n) => n,
        None => return Ok(Vec::new()),
    };

    let components = extract_common_components(ctx, "name", node, "typescript")?;

    let visibility = extract_visibility(node, ctx.source);
    let documentation = extract_preceding_doc_comments(node, ctx.source);
    let content = node_to_text(node, ctx.source).ok();

    // Check if it's a const enum
    let is_const = node.child_by_field_name("const").is_some()
        || ctx.source[node.byte_range()]
            .trim_start()
            .starts_with("const");

    let metadata = EntityMetadata {
        is_const,
        ..Default::default()
    };

    let entity = build_entity(
        components,
        EntityDetails {
            entity_type: EntityType::Enum,
            language: Language::TypeScript,
            visibility: Some(visibility),
            documentation,
            content,
            metadata,
            signature: None,
            relationships: Default::default(),
        },
    )?;

    Ok(vec![entity])
}

/// Handle index signature extraction
///
/// Index signatures like `[key: string]: T` produce Property entities.
/// The name is derived from the index parameter type (e.g., `[string]`).
pub(crate) fn handle_index_signature_impl(ctx: &ExtractionContext) -> Result<Vec<CodeEntity>> {
    let node = match extract_main_node(ctx.query_match, ctx.query, &["index_signature"]) {
        Some(n) => n,
        None => return Ok(Vec::new()),
    };

    // Derive the name from the index parameter type
    // Index signature structure: [param: type]: value_type
    // We want to extract the param type (e.g., "string" or "number")
    let name = derive_index_signature_name(node, ctx.source);

    let components = extract_common_components_with_name(ctx, &name, node, "typescript")?;
    let documentation = extract_preceding_doc_comments(node, ctx.source);
    let content = node_to_text(node, ctx.source).ok();

    // Interface members are always Public
    let entity = build_entity(
        components,
        EntityDetails {
            entity_type: EntityType::Property,
            language: Language::TypeScript,
            visibility: Some(codesearch_core::Visibility::Public),
            documentation,
            content,
            metadata: EntityMetadata::default(),
            signature: None,
            relationships: Default::default(),
        },
    )?;

    Ok(vec![entity])
}

/// Derive the name for an index signature from the index parameter type
///
/// Extracts the type of the index parameter (e.g., "string" from `[key: string]: T`)
/// and returns it in brackets like `[string]`.
fn derive_index_signature_name(node: tree_sitter::Node, source: &str) -> String {
    // Look for the index parameter type
    // Tree structure: index_signature > ... > type_annotation > predefined_type/type_identifier
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            // Look for type annotation within the index signature
            if child.kind() == ":" {
                // The type should be next
                if let Some(type_node) = node.child(i + 1) {
                    if let Some(type_name) = get_simple_type_name(type_node, source) {
                        return format!("[{type_name}]");
                    }
                }
            }
            // Try finding type_annotation child
            if child.kind() == "type_annotation" {
                if let Some(type_child) = child.child(1) {
                    // Skip the ':'
                    if let Some(type_name) = get_simple_type_name(type_child, source) {
                        return format!("[{type_name}]");
                    }
                }
            }
        }
    }
    // Fallback: try to find any predefined_type or type_identifier
    if let Some(type_name) = find_first_type_in_node(node, source) {
        return format!("[{type_name}]");
    }
    "[index]".to_string()
}

/// Get a simple type name from a type node
fn get_simple_type_name(node: tree_sitter::Node, source: &str) -> Option<String> {
    match node.kind() {
        "predefined_type" | "type_identifier" => Some(source[node.byte_range()].to_string()),
        _ => None,
    }
}

/// Find the first type identifier in a node tree
fn find_first_type_in_node(node: tree_sitter::Node, source: &str) -> Option<String> {
    if node.kind() == "predefined_type" || node.kind() == "type_identifier" {
        return Some(source[node.byte_range()].to_string());
    }
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            if let Some(found) = find_first_type_in_node(child, source) {
                return Some(found);
            }
        }
    }
    None
}

/// Handle call signature extraction
///
/// Call signatures like `(): T` produce Method entities with name `()`.
pub(crate) fn handle_call_signature_impl(ctx: &ExtractionContext) -> Result<Vec<CodeEntity>> {
    let node = match extract_main_node(ctx.query_match, ctx.query, &["call_signature"]) {
        Some(n) => n,
        None => return Ok(Vec::new()),
    };

    // Call signatures use "()" as the name
    let components = extract_common_components_with_name(ctx, "()", node, "typescript")?;
    let documentation = extract_preceding_doc_comments(node, ctx.source);
    let content = node_to_text(node, ctx.source).ok();

    // Interface members are always Public
    let entity = build_entity(
        components,
        EntityDetails {
            entity_type: EntityType::Method,
            language: Language::TypeScript,
            visibility: Some(codesearch_core::Visibility::Public),
            documentation,
            content,
            metadata: EntityMetadata::default(),
            signature: None,
            relationships: Default::default(),
        },
    )?;

    Ok(vec![entity])
}

/// Handle construct signature extraction
///
/// Construct signatures like `new(): T` produce Method entities with name `new()`.
pub(crate) fn handle_construct_signature_impl(ctx: &ExtractionContext) -> Result<Vec<CodeEntity>> {
    let node = match extract_main_node(ctx.query_match, ctx.query, &["construct_signature"]) {
        Some(n) => n,
        None => return Ok(Vec::new()),
    };

    // Construct signatures use "new()" as the name
    let components = extract_common_components_with_name(ctx, "new()", node, "typescript")?;
    let documentation = extract_preceding_doc_comments(node, ctx.source);
    let content = node_to_text(node, ctx.source).ok();

    // Interface members are always Public
    let entity = build_entity(
        components,
        EntityDetails {
            entity_type: EntityType::Method,
            language: Language::TypeScript,
            visibility: Some(codesearch_core::Visibility::Public),
            documentation,
            content,
            metadata: EntityMetadata::default(),
            signature: None,
            relationships: Default::default(),
        },
    )?;

    Ok(vec![entity])
}
