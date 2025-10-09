//! Qualified name building via Tree-sitter parent traversal

use tree_sitter::Node;

/// Build qualified name by traversing AST parents to find scope containers
pub fn build_qualified_name_from_ast(node: Node, source: &str, language: &str) -> String {
    let mut scope_parts = Vec::new();
    let mut current = node;

    // Walk up the tree collecting scope names
    while let Some(parent) = current.parent() {
        let scope_name = match language {
            "rust" => extract_rust_scope_name(parent, source),
            "python" => extract_python_scope_name(parent, source),
            "javascript" | "typescript" => extract_js_scope_name(parent, source),
            "go" => extract_go_scope_name(parent, source),
            _ => None,
        };

        if let Some(name) = scope_name {
            scope_parts.push(name);
        }

        current = parent;
    }

    // Reverse to get root-to-leaf order
    scope_parts.reverse();
    scope_parts.join("::")
}

fn extract_rust_scope_name(node: Node, source: &str) -> Option<String> {
    match node.kind() {
        "mod_item" => {
            // Find name child
            node.child_by_field_name("name")
                .and_then(|n| n.utf8_text(source.as_bytes()).ok())
                .map(|s| s.to_string())
        }
        "impl_item" => {
            // Find type child
            node.child_by_field_name("type")
                .and_then(|n| n.utf8_text(source.as_bytes()).ok())
                .map(|s| s.to_string())
        }
        _ => None,
    }
}

fn extract_python_scope_name(node: Node, source: &str) -> Option<String> {
    match node.kind() {
        "class_definition" => node
            .child_by_field_name("name")
            .and_then(|n| n.utf8_text(source.as_bytes()).ok())
            .map(|s| s.to_string()),
        "function_definition" => {
            // Include nested functions in path
            node.child_by_field_name("name")
                .and_then(|n| n.utf8_text(source.as_bytes()).ok())
                .map(|s| s.to_string())
        }
        _ => None,
    }
}

fn extract_js_scope_name(node: Node, source: &str) -> Option<String> {
    match node.kind() {
        "class_declaration" => node
            .child_by_field_name("name")
            .and_then(|n| n.utf8_text(source.as_bytes()).ok())
            .map(|s| s.to_string()),
        "object" => {
            // Objects assigned to variables require parent assignment checking
            None
        }
        _ => None,
    }
}

fn extract_go_scope_name(node: Node, source: &str) -> Option<String> {
    match node.kind() {
        "type_declaration" => node
            .child_by_field_name("name")
            .and_then(|n| n.utf8_text(source.as_bytes()).ok())
            .map(|s| s.to_string()),
        "method_declaration" => {
            // Extract receiver type
            node.child_by_field_name("receiver")
                .and_then(|r| r.child_by_field_name("type"))
                .and_then(|n| n.utf8_text(source.as_bytes()).ok())
                .map(|s| s.to_string())
        }
        _ => None,
    }
}
