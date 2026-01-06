//! Common utilities for JavaScript/TypeScript entity handlers

use codesearch_core::entities::EntityMetadata;
use im::HashMap as ImHashMap;
use tree_sitter::{Node, Query, QueryMatch};

// Re-export node_to_text for use by other handlers
pub(crate) use crate::common::node_to_text;

/// Extract entity name from a query match
///
/// Looks for a capture named "name" in the query match.
pub(crate) fn extract_name<'a>(
    query_match: &QueryMatch<'a, 'a>,
    query: &Query,
    source: &'a str,
) -> Option<&'a str> {
    let name_index = query.capture_index_for_name("name")?;
    for capture in query_match.captures {
        if capture.index == name_index {
            return Some(&source[capture.node.byte_range()]);
        }
    }
    None
}

/// Extract the main captured node from a query match
///
/// Looks for captures like @function, @class, @method, etc.
pub(crate) fn extract_main_node<'a>(
    query_match: &QueryMatch<'a, 'a>,
    query: &Query,
    capture_names: &[&str],
) -> Option<Node<'a>> {
    for name in capture_names {
        if let Some(index) = query.capture_index_for_name(name) {
            for capture in query_match.captures {
                if capture.index == index {
                    return Some(capture.node);
                }
            }
        }
    }
    None
}

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
