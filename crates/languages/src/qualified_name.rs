//! Qualified name building via Tree-sitter parent traversal

use tree_sitter::Node;

/// Configuration for extracting scope names from AST nodes
struct ScopePattern {
    node_kind: &'static str,
    field_name: &'static str,
}

/// Scope extraction configurations for each language
const SCOPE_CONFIGS: &[(&str, &[ScopePattern])] = &[
    (
        "rust",
        &[
            ScopePattern {
                node_kind: "mod_item",
                field_name: "name",
            },
            ScopePattern {
                node_kind: "impl_item",
                field_name: "type",
            },
        ],
    ),
    (
        "python",
        &[
            ScopePattern {
                node_kind: "class_definition",
                field_name: "name",
            },
            ScopePattern {
                node_kind: "function_definition",
                field_name: "name",
            },
        ],
    ),
    (
        "javascript",
        &[
            ScopePattern {
                node_kind: "class_declaration",
                field_name: "name",
            },
            ScopePattern {
                node_kind: "function_declaration",
                field_name: "name",
            },
        ],
    ),
    (
        "typescript",
        &[
            ScopePattern {
                node_kind: "class_declaration",
                field_name: "name",
            },
            ScopePattern {
                node_kind: "function_declaration",
                field_name: "name",
            },
            ScopePattern {
                node_kind: "interface_declaration",
                field_name: "name",
            },
        ],
    ),
    (
        "go",
        &[
            ScopePattern {
                node_kind: "type_declaration",
                field_name: "name",
            },
            ScopePattern {
                node_kind: "method_declaration",
                field_name: "receiver",
            },
        ],
    ),
];

/// Get scope separator for a language
fn get_separator(language: &str) -> &'static str {
    match language {
        "rust" => "::",
        "javascript" | "typescript" | "python" => ".",
        "go" => ".",
        _ => "::",
    }
}

/// Build qualified name by traversing AST parents to find scope containers
pub fn build_qualified_name_from_ast(node: Node, source: &str, language: &str) -> String {
    let mut scope_parts = Vec::new();
    let mut current = node;

    // Get patterns for this language
    let patterns = SCOPE_CONFIGS
        .iter()
        .find(|(lang, _)| *lang == language)
        .map(|(_, patterns)| *patterns)
        .unwrap_or(&[]);

    // Walk up the tree collecting scope names
    while let Some(parent) = current.parent() {
        let scope_name = extract_scope_name_generic(parent, source, patterns);

        if let Some(name) = scope_name {
            scope_parts.push(name);
        }

        current = parent;
    }

    // Reverse to get root-to-leaf order
    scope_parts.reverse();
    scope_parts.join(get_separator(language))
}

/// Extract scope name using pattern configuration
fn extract_scope_name_generic(
    node: Node,
    source: &str,
    patterns: &[ScopePattern],
) -> Option<String> {
    for pattern in patterns {
        if node.kind() == pattern.node_kind {
            return node
                .child_by_field_name(pattern.field_name)
                .and_then(|n| n.utf8_text(source.as_bytes()).ok())
                .map(|s| s.to_string());
        }
    }
    None
}
