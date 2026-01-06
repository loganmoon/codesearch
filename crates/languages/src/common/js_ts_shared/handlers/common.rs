//! Common utilities for JavaScript/TypeScript entity handlers

use crate::common::entity_building::ExtractionContext;
use codesearch_core::entities::{
    EntityMetadata, EntityRelationshipData, SourceLocation, SourceReference,
};
use im::HashMap as ImHashMap;
use tree_sitter::Node;

use super::super::visibility::{is_async, is_generator, is_getter, is_setter, is_static_member};

/// Extract documentation comments preceding a node
///
/// For JavaScript/TypeScript, looks for JSDoc-style comments (/* * */)
/// and single-line comments (//).
pub(crate) fn extract_preceding_doc_comments(node: Node, source: &str) -> Option<String> {
    let mut doc_lines = Vec::new();
    let mut current = node.prev_sibling();

    while let Some(sibling) = current {
        // Limit doc collection to prevent unbounded resource consumption
        if doc_lines.len() >= 100 {
            break;
        }

        match sibling.kind() {
            "comment" => {
                if let Ok(text) = crate::common::node_to_text(sibling, source) {
                    // Handle JSDoc comments: /** ... */
                    if text.starts_with("/**") && text.ends_with("*/") {
                        let content = &text[3..text.len() - 2];
                        // Clean up JSDoc formatting
                        for line in content.lines() {
                            let trimmed = line.trim().trim_start_matches('*').trim();
                            if !trimmed.is_empty() {
                                doc_lines.push(trimmed.to_string());
                            }
                        }
                    }
                    // Handle single-line doc comments: // ...
                    else if let Some(content) = text.strip_prefix("//") {
                        let content = content.trim();
                        if !content.is_empty() {
                            doc_lines.push(content.to_string());
                        }
                    }
                }
            }
            _ => break, // Stop at non-comment nodes
        }
        current = sibling.prev_sibling();
    }

    if doc_lines.is_empty() {
        None
    } else {
        // Reverse since we collected from bottom to top
        doc_lines.reverse();
        Some(doc_lines.join("\n"))
    }
}

/// Build common entity metadata for JavaScript/TypeScript functions/methods
///
/// Uses `attributes` HashMap for JS-specific boolean flags:
/// - `is_generator`, `is_getter`, `is_setter`, `is_arrow`
pub(crate) fn build_js_metadata(
    is_static: bool,
    is_async_fn: bool,
    is_generator_fn: bool,
    is_getter_fn: bool,
    is_setter_fn: bool,
    is_arrow: bool,
) -> EntityMetadata {
    let mut attributes = ImHashMap::new();

    // Store JS-specific flags in attributes
    if is_generator_fn {
        attributes.insert("is_generator".to_string(), "true".to_string());
    }
    if is_getter_fn {
        attributes.insert("is_getter".to_string(), "true".to_string());
    }
    if is_setter_fn {
        attributes.insert("is_setter".to_string(), "true".to_string());
    }
    if is_arrow {
        attributes.insert("is_arrow".to_string(), "true".to_string());
    }

    EntityMetadata {
        is_static,
        is_async: is_async_fn,
        attributes,
        ..Default::default()
    }
}

// =============================================================================
// Metadata helper functions for use with define_handler! macro
// =============================================================================

/// Build metadata for regular function declarations/expressions
pub(crate) fn function_metadata(node: Node, _source: &str) -> EntityMetadata {
    build_js_metadata(
        false,
        is_async(node),
        is_generator(node),
        false,
        false,
        false,
    )
}

/// Build metadata for arrow functions
pub(crate) fn arrow_function_metadata(node: Node, _source: &str) -> EntityMetadata {
    build_js_metadata(false, is_async(node), false, false, false, true)
}

/// Build metadata for class methods
pub(crate) fn method_metadata(node: Node, _source: &str) -> EntityMetadata {
    build_js_metadata(
        is_static_member(node),
        is_async(node),
        is_generator(node),
        is_getter(node),
        is_setter(node),
        false,
    )
}

/// Build metadata for const declarations
pub(crate) fn const_metadata(_node: Node, _source: &str) -> EntityMetadata {
    EntityMetadata {
        is_const: true,
        ..Default::default()
    }
}

/// Build metadata for class properties/fields
///
/// Extracts:
/// - `is_static`: Whether the property has the `static` modifier
/// - `is_private` (attribute): Whether the property name starts with `#`
/// - `has_initializer` (attribute): Whether the property has an initial value
pub(crate) fn property_metadata(node: Node, source: &str) -> EntityMetadata {
    let is_static = is_static_member(node);

    // Check if it's a private field (name starts with #)
    let is_private = node
        .child_by_field_name("name")
        .is_some_and(|name_node| source[name_node.byte_range()].starts_with('#'));

    // Check if there's an initializer (value field exists)
    let has_initializer = node.child_by_field_name("value").is_some();

    let mut attributes = ImHashMap::new();
    if is_private {
        attributes.insert("is_private".to_string(), "true".to_string());
    }
    if has_initializer {
        attributes.insert("has_initializer".to_string(), "true".to_string());
    }

    EntityMetadata {
        is_static,
        attributes,
        ..Default::default()
    }
}

// =============================================================================
// Relationship helper functions for use with define_handler! macro
// =============================================================================

/// Extract extends relationships from a class declaration
///
/// Populates `relationships.extends` which becomes INHERITS_FROM in Neo4j.
pub(crate) fn extract_extends_relationships(
    ctx: &ExtractionContext,
    _node: Node,
) -> EntityRelationshipData {
    let mut relationships = EntityRelationshipData::default();

    if let Some(extends_index) = ctx.query.capture_index_for_name("extends") {
        for capture in ctx.query_match.captures {
            if capture.index == extends_index {
                let extends_name = &ctx.source[capture.node.byte_range()];
                if let Ok(source_ref) = SourceReference::builder()
                    .target(extends_name.to_string())
                    .simple_name(extends_name.to_string())
                    .build()
                {
                    relationships.extends.push(source_ref);
                }
            }
        }
    }

    relationships
}

/// Extract extends relationships from an interface declaration
///
/// Populates `relationships.extended_types` which becomes EXTENDS_INTERFACE in Neo4j.
pub(crate) fn extract_interface_extends_relationships(
    ctx: &ExtractionContext,
    _node: Node,
) -> EntityRelationshipData {
    let mut relationships = EntityRelationshipData::default();

    // Look for the extends_clause capture, which contains all extended types
    if let Some(extends_clause_index) = ctx.query.capture_index_for_name("extends_clause") {
        for capture in ctx.query_match.captures {
            if capture.index == extends_clause_index {
                // Walk the extends_type_clause to find all type_identifier children
                extract_type_identifiers_from_extends(capture.node, ctx.source, &mut relationships);
            }
        }
    }

    relationships
}

/// Walk an extends_type_clause node to extract all type identifiers
fn extract_type_identifiers_from_extends(
    node: Node,
    source: &str,
    relationships: &mut EntityRelationshipData,
) {
    #[cfg(test)]
    eprintln!(
        "  extract_types: node={}, child_count={}",
        node.kind(),
        node.child_count()
    );

    // Use index-based iteration to avoid cursor reuse issues
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            #[cfg(test)]
            eprintln!("    [{i}] child kind={}", child.kind());

            match child.kind() {
                "type_identifier" => {
                    let type_name = &source[child.byte_range()];
                    #[cfg(test)]
                    eprintln!("    Building SourceReference for type: {type_name}");
                    match SourceReference::builder()
                        .target(type_name.to_string())
                        .simple_name(type_name.to_string())
                        .location(SourceLocation::default())
                        .ref_type(codesearch_core::ReferenceType::Extends)
                        .build()
                    {
                        Ok(source_ref) => {
                            #[cfg(test)]
                            eprintln!("    Successfully built SourceReference");
                            relationships.extended_types.push(source_ref);
                        }
                        Err(_e) => {
                            #[cfg(test)]
                            eprintln!("    Failed to build SourceReference: {_e:?}");
                        }
                    }
                }
                // Generic types like Foo<Bar> - extract the base type
                "generic_type" => {
                    if let Some(name_node) = child.child_by_field_name("name") {
                        let type_name = &source[name_node.byte_range()];
                        if let Ok(source_ref) = SourceReference::builder()
                            .target(type_name.to_string())
                            .simple_name(type_name.to_string())
                            .location(SourceLocation::default())
                            .ref_type(codesearch_core::ReferenceType::Extends)
                            .build()
                        {
                            relationships.extended_types.push(source_ref);
                        }
                    }
                }
                // Recursively handle nested structures (but skip simple tokens)
                "extends" | "," => {
                    // Skip keyword and punctuation nodes
                }
                _ => {
                    extract_type_identifiers_from_extends(child, source, relationships);
                }
            }
        }
    }
}
