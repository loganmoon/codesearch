//! Common utilities for JavaScript/TypeScript entity handlers

use crate::common::entity_building::ExtractionContext;
use crate::common::{find_capture_node, node_to_text};
use codesearch_core::entities::{
    EntityMetadata, EntityRelationshipData, ReferenceType, SourceLocation, SourceReference,
};
use codesearch_core::error::{Error, Result};
use im::HashMap as ImHashMap;
use tree_sitter::Node;

use super::super::visibility::{is_async, is_generator, is_getter, is_setter, is_static_member};

// =============================================================================
// Documentation extraction
// =============================================================================

/// Extract JSDoc-style documentation comments preceding a node
pub(crate) fn extract_preceding_doc_comments(node: Node, source: &str) -> Option<String> {
    let mut doc_lines = Vec::new();
    let mut current = node.prev_sibling();

    while let Some(sibling) = current {
        if doc_lines.len() >= 100 {
            break;
        }
        if sibling.kind() != "comment" {
            break;
        }
        if let Ok(text) = node_to_text(sibling, source) {
            if text.starts_with("/**") && text.ends_with("*/") {
                // JSDoc comment
                for line in text[3..text.len() - 2].lines() {
                    let trimmed = line.trim().trim_start_matches('*').trim();
                    if !trimmed.is_empty() {
                        doc_lines.push(trimmed.to_string());
                    }
                }
            } else if let Some(content) = text.strip_prefix("//") {
                let content = content.trim();
                if !content.is_empty() {
                    doc_lines.push(content.to_string());
                }
            }
        }
        current = sibling.prev_sibling();
    }

    if doc_lines.is_empty() {
        None
    } else {
        doc_lines.reverse();
        Some(doc_lines.join("\n"))
    }
}

// =============================================================================
// Metadata helpers for define_handler! macro
// =============================================================================

pub(crate) fn function_metadata(node: Node, _source: &str) -> EntityMetadata {
    let mut attributes = ImHashMap::new();
    if is_generator(node) {
        attributes.insert("is_generator".to_string(), "true".to_string());
    }
    EntityMetadata {
        is_async: is_async(node),
        attributes,
        ..Default::default()
    }
}

pub(crate) fn arrow_function_metadata(node: Node, _source: &str) -> EntityMetadata {
    let mut attributes = ImHashMap::new();
    attributes.insert("is_arrow".to_string(), "true".to_string());
    EntityMetadata {
        is_async: is_async(node),
        attributes,
        ..Default::default()
    }
}

pub(crate) fn method_metadata(node: Node, _source: &str) -> EntityMetadata {
    let mut attributes = ImHashMap::new();
    if is_generator(node) {
        attributes.insert("is_generator".to_string(), "true".to_string());
    }
    if is_getter(node) {
        attributes.insert("is_getter".to_string(), "true".to_string());
    }
    if is_setter(node) {
        attributes.insert("is_setter".to_string(), "true".to_string());
    }
    EntityMetadata {
        is_static: is_static_member(node),
        is_async: is_async(node),
        attributes,
        ..Default::default()
    }
}

pub(crate) fn const_metadata(_node: Node, _source: &str) -> EntityMetadata {
    EntityMetadata {
        is_const: true,
        ..Default::default()
    }
}

pub(crate) fn property_metadata(node: Node, source: &str) -> EntityMetadata {
    let mut attributes = ImHashMap::new();
    if node
        .child_by_field_name("name")
        .is_some_and(|n| source[n.byte_range()].starts_with('#'))
    {
        attributes.insert("is_private".to_string(), "true".to_string());
    }
    if node.child_by_field_name("value").is_some() {
        attributes.insert("has_initializer".to_string(), "true".to_string());
    }
    EntityMetadata {
        is_static: is_static_member(node),
        attributes,
        ..Default::default()
    }
}

pub(crate) fn enum_metadata(node: Node, source: &str) -> EntityMetadata {
    let is_const = source[node.byte_range()].trim_start().starts_with("const");
    EntityMetadata {
        is_const,
        ..Default::default()
    }
}

// =============================================================================
// Relationship extraction helpers
// =============================================================================

/// Collect type identifiers from a node tree, returning SourceReferences
fn collect_type_refs(node: Node, source: &str, ref_type: ReferenceType) -> Vec<SourceReference> {
    let mut refs = Vec::new();
    collect_type_refs_recursive(node, source, ref_type, &mut refs);
    refs
}

fn collect_type_refs_recursive(
    node: Node,
    source: &str,
    ref_type: ReferenceType,
    refs: &mut Vec<SourceReference>,
) {
    match node.kind() {
        "type_identifier" | "identifier" => {
            let name = &source[node.byte_range()];
            if let Ok(r) = SourceReference::builder()
                .target(name.to_string())
                .simple_name(name.to_string())
                .location(SourceLocation::from_tree_sitter_node(node))
                .ref_type(ref_type)
                .build()
            {
                refs.push(r);
            }
        }
        "generic_type" => {
            // Extract base type name from generic: Foo<T> -> Foo
            if let Some(name_node) = node.child_by_field_name("name") {
                let name = &source[name_node.byte_range()];
                if let Ok(r) = SourceReference::builder()
                    .target(name.to_string())
                    .simple_name(name.to_string())
                    .location(SourceLocation::from_tree_sitter_node(name_node))
                    .ref_type(ref_type)
                    .build()
                {
                    refs.push(r);
                }
            }
        }
        _ => {
            for i in 0..node.child_count() {
                if let Some(child) = node.child(i) {
                    collect_type_refs_recursive(child, source, ref_type, refs);
                }
            }
        }
    }
}

/// Extract extends/implements relationships from class heritage and USES from class body
pub(crate) fn extract_extends_relationships(
    ctx: &ExtractionContext,
    node: Node,
) -> EntityRelationshipData {
    let mut rels = EntityRelationshipData::default();

    if let Some(heritage) = find_capture_node(ctx.query_match, ctx.query, "heritage") {
        for i in 0..heritage.child_count() {
            if let Some(child) = heritage.child(i) {
                match child.kind() {
                    "extends_clause" => {
                        if let Some(value) = child.child_by_field_name("value") {
                            rels.extends =
                                collect_type_refs(value, ctx.source, ReferenceType::Extends);
                        }
                    }
                    "implements_clause" => {
                        rels.implements =
                            collect_type_refs(child, ctx.source, ReferenceType::Implements);
                    }
                    _ => {}
                }
            }
        }
    }

    // Extract USES from class body type references
    if let Some(body) = find_child_by_kind(node, "class_body") {
        rels.uses_types = extract_class_type_uses(body, ctx.source);
    }

    rels
}

/// Extract extends relationships from interface declaration and USES from interface body
pub(crate) fn extract_interface_extends_relationships(
    ctx: &ExtractionContext,
    node: Node,
) -> EntityRelationshipData {
    let mut rels = EntityRelationshipData::default();
    if let Some(extends_clause) = find_capture_node(ctx.query_match, ctx.query, "extends_clause") {
        rels.extended_types = collect_type_refs(extends_clause, ctx.source, ReferenceType::Extends);
    }

    // Extract USES from interface body type references
    if let Some(body) = find_child_by_kind(node, "interface_body")
        .or_else(|| find_child_by_kind(node, "object_type"))
    {
        rels.uses_types = extract_interface_type_uses(body, ctx.source);
    }

    rels
}

/// Find a child node by its kind
fn find_child_by_kind<'a>(node: Node<'a>, kind: &str) -> Option<Node<'a>> {
    let mut cursor = node.walk();
    let result = node
        .children(&mut cursor)
        .find(|child| child.kind() == kind);
    result
}

/// TypeScript/JavaScript primitive types to skip in type reference extraction
const TS_PRIMITIVE_TYPES: &[&str] = &[
    "string",
    "number",
    "boolean",
    "void",
    "null",
    "undefined",
    "never",
    "any",
    "unknown",
    "object",
    "symbol",
    "bigint",
    "Array",
    "Promise",
    "Map",
    "Set",
    "Record",
    "Partial",
    "Required",
    "Readonly",
    "Pick",
    "Omit",
    "Exclude",
    "Extract",
    "NonNullable",
    "ReturnType",
    "Parameters",
    "InstanceType",
];

/// Check if a type name is a primitive/built-in type
fn is_ts_primitive_type(name: &str) -> bool {
    TS_PRIMITIVE_TYPES.contains(&name)
}

/// Extract type usage references from a class body, filtering primitives
fn extract_class_type_uses(body: Node, source: &str) -> Vec<SourceReference> {
    let mut refs = Vec::new();
    let mut seen = std::collections::HashSet::new();
    extract_type_uses_recursive(body, source, &mut refs, &mut seen);
    refs
}

/// Extract type usage references from an interface body, filtering primitives
fn extract_interface_type_uses(body: Node, source: &str) -> Vec<SourceReference> {
    let mut refs = Vec::new();
    let mut seen = std::collections::HashSet::new();
    extract_type_uses_recursive(body, source, &mut refs, &mut seen);
    refs
}

/// Recursively extract type identifiers, filtering primitives and deduplicating
fn extract_type_uses_recursive(
    node: Node,
    source: &str,
    refs: &mut Vec<SourceReference>,
    seen: &mut std::collections::HashSet<String>,
) {
    match node.kind() {
        "type_identifier" => {
            let name = &source[node.byte_range()];
            if !is_ts_primitive_type(name) && seen.insert(name.to_string()) {
                if let Ok(r) = SourceReference::builder()
                    .target(name.to_string())
                    .simple_name(name.to_string())
                    .location(SourceLocation::from_tree_sitter_node(node))
                    .ref_type(ReferenceType::TypeUsage)
                    .build()
                {
                    refs.push(r);
                }
            }
        }
        "generic_type" => {
            // Extract base type name from generic: Foo<T> -> Foo
            if let Some(name_node) = node.child_by_field_name("name") {
                let name = &source[name_node.byte_range()];
                if !is_ts_primitive_type(name) && seen.insert(name.to_string()) {
                    if let Ok(r) = SourceReference::builder()
                        .target(name.to_string())
                        .simple_name(name.to_string())
                        .location(SourceLocation::from_tree_sitter_node(name_node))
                        .ref_type(ReferenceType::TypeUsage)
                        .build()
                    {
                        refs.push(r);
                    }
                }
            }
            // Also process type arguments
            if let Some(type_args) = node.child_by_field_name("type_arguments") {
                extract_type_uses_recursive(type_args, source, refs, seen);
            }
        }
        _ => {
            // Recurse into children
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                extract_type_uses_recursive(child, source, refs, seen);
            }
        }
    }
}

// =============================================================================
// Name derivation helpers for define_handler! macro
// =============================================================================

pub(crate) fn derive_function_expression_name(
    ctx: &ExtractionContext,
    _node: Node,
) -> Result<String> {
    find_capture_node(ctx.query_match, ctx.query, "fn_name")
        .or_else(|| find_capture_node(ctx.query_match, ctx.query, "name"))
        .and_then(|n| node_to_text(n, ctx.source).ok())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| Error::entity_extraction("Could not derive function expression name"))
}

pub(crate) fn derive_class_expression_name(ctx: &ExtractionContext, _node: Node) -> Result<String> {
    find_capture_node(ctx.query_match, ctx.query, "class_name")
        .or_else(|| find_capture_node(ctx.query_match, ctx.query, "name"))
        .and_then(|n| node_to_text(n, ctx.source).ok())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| Error::entity_extraction("Could not derive class expression name"))
}

pub(crate) fn derive_index_signature_name(node: Node, source: &str) -> String {
    // Find first type identifier in the index signature
    fn find_type(node: Node, source: &str) -> Option<String> {
        if matches!(node.kind(), "predefined_type" | "type_identifier") {
            return Some(source[node.byte_range()].to_string());
        }
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i) {
                if let Some(found) = find_type(child, source) {
                    return Some(found);
                }
            }
        }
        None
    }
    find_type(node, source)
        .map(|t| format!("[{t}]"))
        .unwrap_or_else(|| "[index]".to_string())
}
