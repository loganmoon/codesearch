//! Visibility extraction for JavaScript and TypeScript
//!
//! JavaScript and TypeScript determine visibility differently than languages
//! with explicit visibility keywords:
//!
//! - **ES Modules**: `export` keyword makes an entity public
//! - **Private fields/methods**: `#` prefix (ES2022) makes them private
//! - **TypeScript modifiers**: `public`, `private`, `protected` keywords
//!
//! This module provides utilities to extract visibility from AST nodes.

use codesearch_core::Visibility;
use tree_sitter::Node;

/// Extract visibility for a JavaScript/TypeScript node
///
/// Rules (in order of precedence):
/// 1. Private fields/methods (name starts with #) -> Private
/// 2. TypeScript visibility modifiers (public/private/protected) -> as specified
/// 3. Exported declarations -> Public
/// 4. Non-exported declarations -> Private (module-scoped)
///
/// # Arguments
/// * `node` - The AST node to check
/// * `source` - The source code for extracting node text
pub(crate) fn extract_visibility(node: Node, source: &str) -> Visibility {
    // Check for ECMAScript private fields/methods (# prefix)
    if is_private_identifier(node, source) {
        return Visibility::Private;
    }

    // Check for TypeScript visibility modifiers
    if let Some(vis) = extract_ts_visibility_modifier(node) {
        return vis;
    }

    // Check if the declaration is exported
    if is_exported(node) {
        Visibility::Public
    } else {
        // Non-exported module-level declarations are private to the module
        Visibility::Private
    }
}

/// Check if a node represents a private identifier (starts with #)
///
/// In ES2022, private class members use the # prefix:
/// ```javascript
/// class Foo {
///     #privateField = 42;
///     #privateMethod() { ... }
/// }
/// ```
fn is_private_identifier(node: Node, source: &str) -> bool {
    // Look for the name node
    if let Some(name_node) = node.child_by_field_name("name") {
        let name_text = &source[name_node.byte_range()];
        return name_text.starts_with('#');
    }

    // For property_identifier nodes directly
    if node.kind() == "private_property_identifier" {
        return true;
    }

    false
}

/// Extract TypeScript visibility modifier from a node
///
/// TypeScript class members can have explicit visibility:
/// ```typescript
/// class Foo {
///     public publicMethod() { ... }
///     private privateMethod() { ... }
///     protected protectedMethod() { ... }
/// }
/// ```
fn extract_ts_visibility_modifier(node: Node) -> Option<Visibility> {
    // TypeScript modifiers are typically in an "accessibility_modifier" child
    // or in modifiers for the node
    let mut cursor = node.walk();

    for child in node.children(&mut cursor) {
        match child.kind() {
            "accessibility_modifier" | "public" => {
                // Need to look at the actual text
                let parent_source_needed = true;
                if parent_source_needed {
                    // Check children for the actual modifier keyword
                    let mut inner_cursor = child.walk();
                    for inner in child.children(&mut inner_cursor) {
                        match inner.kind() {
                            "public" => return Some(Visibility::Public),
                            "private" => return Some(Visibility::Private),
                            "protected" => return Some(Visibility::Protected),
                            _ => {}
                        }
                    }
                }
            }
            "private" => return Some(Visibility::Private),
            "protected" => return Some(Visibility::Protected),
            _ => {}
        }
    }

    None
}

/// Check if a node is exported (directly or as part of an export statement)
///
/// Handles:
/// - `export function foo() {}` - direct export
/// - `export class Foo {}` - direct export
/// - `export const foo = 1` - direct export
/// - `export default function() {}` - default export
/// - `export { foo }` - named re-export (not handled here, handled at import level)
pub(crate) fn is_exported(node: Node) -> bool {
    // Check if the node itself is an export statement
    if node.kind() == "export_statement" {
        return true;
    }

    // Check if parent is an export statement
    if let Some(parent) = node.parent() {
        if parent.kind() == "export_statement" {
            return true;
        }

        // Handle lexical_declaration inside export: export const foo = 1
        if parent.kind() == "lexical_declaration" {
            if let Some(grandparent) = parent.parent() {
                if grandparent.kind() == "export_statement" {
                    return true;
                }
            }
        }

        // Handle variable_declaration inside export
        if parent.kind() == "variable_declaration" {
            if let Some(grandparent) = parent.parent() {
                if grandparent.kind() == "export_statement" {
                    return true;
                }
            }
        }

        // Handle variable_declarator -> variable_declaration -> export_statement
        if parent.kind() == "variable_declarator" {
            if let Some(grandparent) = parent.parent() {
                if grandparent.kind() == "lexical_declaration"
                    || grandparent.kind() == "variable_declaration"
                {
                    if let Some(great_grandparent) = grandparent.parent() {
                        if great_grandparent.kind() == "export_statement" {
                            return true;
                        }
                    }
                }
            }
        }
    }

    false
}

/// Check if a class member is static
pub(crate) fn is_static_member(node: Node) -> bool {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "static" {
            return true;
        }
    }
    false
}

/// Check if a function/method is async
pub(crate) fn is_async(node: Node) -> bool {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "async" {
            return true;
        }
    }
    false
}

/// Check if a function is a generator (has *)
pub(crate) fn is_generator(node: Node) -> bool {
    // Generator functions have node kind "generator_function" or "generator_function_declaration"
    // Or method_definition with a "*" child
    if node.kind().contains("generator") {
        return true;
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "*" {
            return true;
        }
    }
    false
}

/// Check if a method is a getter
pub(crate) fn is_getter(node: Node) -> bool {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "get" {
            return true;
        }
    }
    false
}

/// Check if a method is a setter
pub(crate) fn is_setter(node: Node) -> bool {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "set" {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: Full tests require tree-sitter parsing which is tested in integration tests.
    // These are basic unit tests for the module structure.

    #[test]
    fn test_visibility_variants() {
        // Verify all visibility variants are accessible
        let _ = Visibility::Public;
        let _ = Visibility::Private;
        let _ = Visibility::Protected;
    }
}
