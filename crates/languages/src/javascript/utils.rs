//! JavaScript and TypeScript shared utilities
//!
//! This module contains functions shared between JavaScript and TypeScript
//! entity extraction, including parameter extraction and JSDoc parsing.

use crate::common::node_to_text;
use codesearch_core::error::Result;
use tree_sitter::Node;

/// Extract parameters from a formal_parameters node (JavaScript-style)
///
/// This function handles JavaScript parameter patterns including:
/// - Simple identifiers: `function foo(a, b) {}`
/// - Default parameters: `function foo(a = 1) {}`
/// - Rest parameters: `function foo(...args) {}`
/// - Destructuring: `function foo({x, y}) {}`
pub fn extract_parameters(
    params_node: Node,
    source: &str,
) -> Result<Vec<(String, Option<String>)>> {
    let mut parameters = Vec::new();

    for child in params_node.named_children(&mut params_node.walk()) {
        match child.kind() {
            "identifier" => {
                let param_name = node_to_text(child, source)?;
                parameters.push((param_name, None));
            }
            "assignment_pattern" => {
                // Handle default parameters
                if let Some(name_node) = child.child_by_field_name("left") {
                    let param_name = node_to_text(name_node, source)?;
                    parameters.push((param_name, None));
                }
            }
            "rest_pattern" => {
                // Handle rest parameters (...args)
                if let Some(name_node) = child.named_child(0) {
                    let param_name = format!("...{}", node_to_text(name_node, source)?);
                    parameters.push((param_name, None));
                }
            }
            "object_pattern" | "array_pattern" => {
                // Handle destructuring parameters
                let param_text = node_to_text(child, source)?;
                parameters.push((param_text, None));
            }
            _ => {}
        }
    }

    Ok(parameters)
}

/// Extract JSDoc comments preceding a node
///
/// This function walks backward from the given node to find JSDoc-style
/// comments (/** ... */) and extracts their content.
pub fn extract_jsdoc_comments(node: Node, source: &str) -> Option<String> {
    let mut doc_lines = Vec::new();
    let mut current = node.prev_sibling();

    while let Some(sibling) = current {
        if sibling.kind() == "comment" {
            if let Ok(text) = node_to_text(sibling, source) {
                if text.starts_with("/**") && text.ends_with("*/") {
                    // Extract JSDoc content
                    let content = text
                        .trim_start_matches("/**")
                        .trim_end_matches("*/")
                        .lines()
                        .map(|line| line.trim().trim_start_matches('*').trim())
                        .filter(|line| !line.is_empty())
                        .collect::<Vec<_>>()
                        .join("\n");
                    doc_lines.push(content);
                    break;
                }
            }
        } else if sibling.kind() != "expression_statement" {
            break;
        }
        current = sibling.prev_sibling();
    }

    if doc_lines.is_empty() {
        None
    } else {
        Some(doc_lines.join("\n"))
    }
}
