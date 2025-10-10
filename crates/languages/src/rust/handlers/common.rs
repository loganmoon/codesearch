//! Common utilities shared between handler modules
//!
//! This module provides shared functionality for AST traversal,
//! text extraction, and documentation processing.

#![deny(warnings)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]

use crate::rust::handlers::constants::{
    capture_names, doc_prefixes, node_kinds, punctuation, visibility_keywords,
};
use codesearch_core::entities::Visibility;
use codesearch_core::error::{Error, Result};
use tree_sitter::{Node, Query, QueryMatch};

// ============================================================================
// Node Finding and Text Extraction
// ============================================================================

/// Find a capture node by name in a query match
pub fn find_capture_node<'a>(
    query_match: &'a QueryMatch,
    query: &Query,
    capture_name: &str,
) -> Option<Node<'a>> {
    query_match.captures.iter().find_map(|c| {
        query
            .capture_names()
            .get(c.index as usize)
            .filter(|&n| *n == capture_name)
            .map(|_| c.node)
    })
}

/// Convert a node to text with error handling
pub fn node_to_text(node: Node, source: &str) -> Result<String> {
    node.utf8_text(source.as_bytes())
        .map(|s| s.to_string())
        .map_err(|e| Error::entity_extraction(format!("Failed to extract text: {e}")))
}

/// Find a required capture node or return an error
pub fn require_capture_node<'a>(
    query_match: &'a QueryMatch,
    query: &Query,
    capture_name: &str,
) -> Result<Node<'a>> {
    find_capture_node(query_match, query, capture_name)
        .ok_or_else(|| Error::entity_extraction(format!("{capture_name} node not found")))
}

// ============================================================================
// Visibility Extraction
// ============================================================================

/// Extract visibility from a captured visibility modifier node
pub fn extract_visibility(query_match: &QueryMatch, query: &Query) -> Visibility {
    let Some(vis_node) = find_capture_node(query_match, query, capture_names::VIS) else {
        return Visibility::Private;
    };

    // Check if this is a visibility_modifier node
    if vis_node.kind() != node_kinds::VISIBILITY_MODIFIER {
        return Visibility::Private;
    }

    // Walk through the visibility modifier's children
    let mut cursor = vis_node.walk();
    let has_public_keyword = vis_node.children(&mut cursor).any(|child| {
        matches!(
            child.kind(),
            visibility_keywords::PUB
                | visibility_keywords::CRATE
                | visibility_keywords::SUPER
                | visibility_keywords::SELF
                | visibility_keywords::IN
                | node_kinds::SCOPED_IDENTIFIER
                | node_kinds::IDENTIFIER
        )
    });

    if has_public_keyword {
        Visibility::Public
    } else {
        Visibility::Private
    }
}

// ============================================================================
// Documentation Extraction
// ============================================================================

/// Extract documentation comments preceding a node
pub fn extract_preceding_doc_comments(node: Node, source: &str) -> Option<String> {
    let doc_lines = collect_doc_lines(node, source);

    if doc_lines.is_empty() {
        None
    } else {
        Some(doc_lines.join("\n"))
    }
}

/// Maximum number of documentation lines to collect to prevent unbounded resource consumption
const MAX_DOC_LINES: usize = 1000;

/// Collect documentation lines from preceding siblings
fn collect_doc_lines(node: Node, source: &str) -> Vec<String> {
    let mut doc_lines = Vec::new();
    let mut current = node.prev_sibling();

    while let Some(sibling) = current {
        // Prevent unbounded resource consumption
        if doc_lines.len() >= MAX_DOC_LINES {
            break;
        }

        match sibling.kind() {
            node_kinds::LINE_COMMENT => {
                if let Some(doc_text) = extract_line_doc_text(sibling, source) {
                    doc_lines.push(doc_text);
                }
            }
            node_kinds::BLOCK_COMMENT => {
                if let Some(doc_text) = extract_block_doc_text(sibling, source) {
                    doc_lines.push(doc_text);
                }
            }
            node_kinds::ATTRIBUTE_ITEM => {
                // Continue through attributes
            }
            _ => break, // Stop at non-doc/non-attribute nodes
        }
        current = sibling.prev_sibling();
    }

    // Reverse once at the end instead of inserting at position 0 each time
    doc_lines.reverse();
    doc_lines
}

/// Extract documentation text from a line comment
fn extract_line_doc_text(node: Node, source: &str) -> Option<String> {
    node_to_text(node, source).ok().and_then(|text| {
        if text.starts_with(doc_prefixes::LINE_OUTER) {
            Some(
                text.trim_start_matches(doc_prefixes::LINE_OUTER)
                    .trim()
                    .to_string(),
            )
        } else if text.starts_with(doc_prefixes::LINE_INNER) {
            Some(
                text.trim_start_matches(doc_prefixes::LINE_INNER)
                    .trim()
                    .to_string(),
            )
        } else {
            None
        }
    })
}

/// Extract documentation text from a block comment
fn extract_block_doc_text(node: Node, source: &str) -> Option<String> {
    node_to_text(node, source).ok().and_then(|text| {
        if text.starts_with(doc_prefixes::BLOCK_OUTER_START) {
            Some(
                text.trim_start_matches(doc_prefixes::BLOCK_OUTER_START)
                    .trim_end_matches(doc_prefixes::BLOCK_END)
                    .trim()
                    .to_string(),
            )
        } else if text.starts_with(doc_prefixes::BLOCK_INNER_START) {
            Some(
                text.trim_start_matches(doc_prefixes::BLOCK_INNER_START)
                    .trim_end_matches(doc_prefixes::BLOCK_END)
                    .trim()
                    .to_string(),
            )
        } else {
            None
        }
    })
}

// ============================================================================
// Generic Parameter Extraction
// ============================================================================

/// Extract generic parameters from a type_parameters node
pub fn extract_generics_from_node(node: Node, source: &str) -> Vec<String> {
    let mut generics = Vec::new();
    let mut cursor = node.walk();

    for child in node.children(&mut cursor) {
        match child.kind() {
            // Skip punctuation
            punctuation::OPEN_ANGLE | punctuation::CLOSE_ANGLE | punctuation::COMMA => continue,

            // Handle various parameter types
            node_kinds::TYPE_PARAMETER
            | node_kinds::LIFETIME_PARAMETER
            | node_kinds::CONST_PARAMETER
            | node_kinds::TYPE_IDENTIFIER
            | node_kinds::LIFETIME
            | node_kinds::CONSTRAINED_TYPE_PARAMETER
            | node_kinds::OPTIONAL_TYPE_PARAMETER => {
                if let Ok(text) = node_to_text(child, source) {
                    generics.push(text);
                }
            }

            _ => {}
        }
    }

    generics
}

// ============================================================================
// Function Parameter and Modifier Extraction
// ============================================================================

/// Extract parameters from a function parameters node
pub fn extract_function_parameters(
    params_node: Node,
    source: &str,
) -> Result<Vec<(String, String)>> {
    use crate::rust::handlers::constants::{keywords, special_idents};

    let mut parameters = Vec::new();
    let mut cursor = params_node.walk();

    for child in params_node.children(&mut cursor) {
        // Skip punctuation like parentheses and commas
        if matches!(
            child.kind(),
            punctuation::OPEN_PAREN | punctuation::CLOSE_PAREN | punctuation::COMMA
        ) {
            continue;
        }

        // Handle different parameter types
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
///
/// # UTF-8 Safety
/// Uses `split_once(':')` instead of byte-index splitting to ensure UTF-8
/// character boundaries are respected. This prevents panics when parameter
/// names or types contain multi-byte Unicode characters.
///
/// For example, with a parameter like `名前: String`, using byte indices from
/// `find(':')` could panic if the split point falls within the multi-byte
/// character sequence. `split_once` safely handles this by operating on
/// character boundaries.
pub fn extract_parameter_parts(node: Node, source: &str) -> Result<Option<(String, String)>> {
    let full_text = node_to_text(node, source)?;

    // Use split_once for safe UTF-8 boundary handling
    if let Some((pattern, param_type)) = full_text.split_once(':') {
        return Ok(Some((
            pattern.trim().to_string(),
            param_type.trim().to_string(),
        )));
    }

    // No colon means no type annotation (rare in Rust)
    if !full_text.trim().is_empty() {
        Ok(Some((full_text, String::new())))
    } else {
        Ok(None)
    }
}

/// Extract function modifiers (async, unsafe, const) from a modifiers node
pub fn extract_function_modifiers(modifiers_node: Node) -> (bool, bool, bool) {
    use crate::rust::handlers::constants::function_modifiers;

    let mut has_async = false;
    let mut has_unsafe = false;
    let mut has_const = false;
    let mut cursor = modifiers_node.walk();

    for child in modifiers_node.children(&mut cursor) {
        match child.kind() {
            function_modifiers::ASYNC => has_async = true,
            function_modifiers::UNSAFE => has_unsafe = true,
            function_modifiers::CONST => has_const = true,
            _ => {}
        }
    }

    (has_async, has_unsafe, has_const)
}
