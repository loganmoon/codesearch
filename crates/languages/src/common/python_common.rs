//! Python-specific shared utilities for entity extraction

use super::node_to_text;
use codesearch_core::error::Result;
use tree_sitter::Node;

/// Extract parameters from a Python parameters node
///
/// Handles:
/// - Simple identifiers (self, name)
/// - Typed parameters (name: Type)
/// - Default parameters (name=value)
/// - Typed default parameters (name: Type = value)
/// - Variadic (*args, **kwargs)
/// - Positional-only separator (/) - Python 3.8+
/// - Keyword-only marker (*) - Python 3.0+
pub fn extract_python_parameters(
    params_node: Node,
    source: &str,
) -> Result<Vec<(String, Option<String>)>> {
    let mut parameters = Vec::new();

    for child in params_node.named_children(&mut params_node.walk()) {
        match child.kind() {
            "identifier" => {
                let name = node_to_text(child, source)?;
                parameters.push((name, None));
            }
            "typed_parameter" => {
                // In tree-sitter-python, typed_parameter has identifier as first child
                // and type annotation as the "type" field
                if let Some(name) = child.named_child(0).and_then(|n| {
                    if n.kind() == "identifier" {
                        node_to_text(n, source).ok()
                    } else {
                        None
                    }
                }) {
                    let type_hint = child
                        .child_by_field_name("type")
                        .and_then(|n| node_to_text(n, source).ok());
                    parameters.push((name, type_hint));
                }
            }
            "default_parameter" => {
                // default_parameter structure: identifier = value
                if let Some(name) = child.named_child(0).and_then(|n| {
                    if n.kind() == "identifier" {
                        node_to_text(n, source).ok()
                    } else {
                        None
                    }
                }) {
                    parameters.push((name, None));
                }
            }
            "typed_default_parameter" => {
                // typed_default_parameter structure: identifier : type = value
                if let Some(name) = child.named_child(0).and_then(|n| {
                    if n.kind() == "identifier" {
                        node_to_text(n, source).ok()
                    } else {
                        None
                    }
                }) {
                    let type_hint = child
                        .child_by_field_name("type")
                        .and_then(|n| node_to_text(n, source).ok());
                    parameters.push((name, type_hint));
                }
            }
            "list_splat_pattern" => {
                let name = child
                    .named_child(0)
                    .and_then(|n| node_to_text(n, source).ok())
                    .map(|n| format!("*{n}"))
                    .unwrap_or_else(|| "*args".to_string());
                parameters.push((name, None));
            }
            "dictionary_splat_pattern" => {
                let name = child
                    .named_child(0)
                    .and_then(|n| node_to_text(n, source).ok())
                    .map(|n| format!("**{n}"))
                    .unwrap_or_else(|| "**kwargs".to_string());
                parameters.push((name, None));
            }
            // Python 3.8+ positional-only separator: def f(a, /, b)
            // Parameters before / are positional-only, represented as "/" marker
            "positional_separator" => {
                parameters.push(("/".to_string(), None));
            }
            // Python 3.0+ keyword-only marker: def f(*, a, b)
            // A bare * without a name indicates keyword-only parameters follow
            "keyword_separator" => {
                parameters.push(("*".to_string(), None));
            }
            _ => {}
        }
    }

    Ok(parameters)
}

/// Extract docstring from a function or class body
///
/// Python docstrings are the first expression in the body if it's a string literal.
pub fn extract_docstring(node: Node, source: &str) -> Option<String> {
    let body = node.child_by_field_name("body")?;
    let first_stmt = body.named_child(0)?;

    if first_stmt.kind() == "expression_statement" {
        let expr = first_stmt.named_child(0)?;
        if expr.kind() == "string" {
            let text = node_to_text(expr, source).ok()?;
            return Some(normalize_docstring(&text));
        }
    }

    None
}

/// Normalize a docstring by stripping quotes and whitespace
fn normalize_docstring(text: &str) -> String {
    text.trim_start_matches("\"\"\"")
        .trim_start_matches("'''")
        .trim_start_matches('"')
        .trim_start_matches('\'')
        .trim_end_matches("\"\"\"")
        .trim_end_matches("'''")
        .trim_end_matches('"')
        .trim_end_matches('\'')
        .trim()
        .to_string()
}

/// Extract decorators from a function or class definition
///
/// Python decorators appear on the parent `decorated_definition` node.
pub fn extract_decorators(node: Node, source: &str) -> Vec<String> {
    let mut decorators = Vec::new();

    // Check if parent is a decorated_definition
    if let Some(parent) = node.parent() {
        if parent.kind() == "decorated_definition" {
            for child in parent.named_children(&mut parent.walk()) {
                if child.kind() == "decorator" {
                    if let Ok(text) = node_to_text(child, source) {
                        let decorator = text.trim_start_matches('@').trim().to_string();
                        decorators.push(decorator);
                    }
                }
            }
        }
    }

    decorators
}

/// Check if a function node is async
pub fn is_async_function(node: Node) -> bool {
    for child in node.children(&mut node.walk()) {
        if child.kind() == "async" {
            return true;
        }
        // Stop after reaching the function keyword
        if child.kind() == "def" {
            break;
        }
    }
    false
}

/// Extract base classes from a class definition
pub fn extract_base_classes(node: Node, source: &str) -> Vec<String> {
    let mut bases = Vec::new();

    if let Some(superclasses) = node.child_by_field_name("superclasses") {
        for child in superclasses.named_children(&mut superclasses.walk()) {
            if let Ok(text) = node_to_text(child, source) {
                bases.push(text);
            }
        }
    }

    bases
}

/// Extract return type annotation from a function
pub fn extract_return_type(node: Node, source: &str) -> Option<String> {
    node.child_by_field_name("return_type")
        .and_then(|n| node_to_text(n, source).ok())
}

/// Filter self/cls from method parameters for display
pub fn filter_self_parameter(
    params: Vec<(String, Option<String>)>,
) -> Vec<(String, Option<String>)> {
    params
        .into_iter()
        .filter(|(name, _)| name != "self" && name != "cls")
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_docstring_triple_double() {
        assert_eq!(
            normalize_docstring("\"\"\"Hello world\"\"\""),
            "Hello world"
        );
    }

    #[test]
    fn test_normalize_docstring_triple_single() {
        assert_eq!(normalize_docstring("'''Hello world'''"), "Hello world");
    }

    #[test]
    fn test_normalize_docstring_with_whitespace() {
        assert_eq!(
            normalize_docstring("\"\"\"  \n  Hello world  \n  \"\"\""),
            "Hello world"
        );
    }
}
