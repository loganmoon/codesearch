//! Qualified name building via Tree-sitter parent traversal

use tree_sitter::Node;

/// Configuration for extracting scope names from AST nodes
#[derive(Debug)]
pub struct ScopePattern {
    pub node_kind: &'static str,
    pub field_name: &'static str,
}

/// Language-specific scope configuration for qualified name building
///
/// Register this via inventory to add scope support for a new language
/// without modifying this module.
pub struct ScopeConfiguration {
    /// Language identifier (e.g., "rust", "python", "javascript")
    pub language: &'static str,
    /// Separator between scope parts (e.g., "::" for Rust, "." for Python)
    pub separator: &'static str,
    /// Patterns for identifying scope containers in the AST
    pub patterns: &'static [ScopePattern],
}

inventory::collect!(ScopeConfiguration);

/// Build qualified name by traversing AST parents to find scope containers
pub fn build_qualified_name_from_ast(node: Node, source: &str, language: &str) -> String {
    let mut scope_parts = Vec::new();
    let mut current = node;

    // Find configuration for this language via inventory lookup
    let config = inventory::iter::<ScopeConfiguration>().find(|config| config.language == language);

    let (patterns, separator) = match config {
        Some(cfg) => (cfg.patterns, cfg.separator),
        None => (&[] as &[ScopePattern], "::"),
    };

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
    scope_parts.join(separator)
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
