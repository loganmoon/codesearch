//! Qualified name building via Tree-sitter parent traversal

use std::path::Path;
use tree_sitter::Node;

/// Configuration for extracting scope names from AST nodes
#[derive(Debug)]
pub struct ScopePattern {
    pub node_kind: &'static str,
    pub field_name: &'static str,
}

/// Function type for deriving module path from file path
pub type ModulePathFn = fn(&Path, &Path) -> Option<String>;

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
    /// Optional function for deriving module path from file path
    /// Takes (file_path, source_root) and returns the module path
    pub module_path_fn: Option<ModulePathFn>,
    /// Path configuration for relative prefix handling
    pub path_config: &'static crate::common::path_config::PathConfig,
    /// Optional edge case handlers for language-specific resolution quirks
    pub edge_case_handlers:
        Option<&'static [&'static dyn crate::common::edge_case_handlers::EdgeCaseHandler]>,
}

inventory::collect!(ScopeConfiguration);

/// Result of building a qualified name, including the separator for the language
pub struct QualifiedNameResult {
    /// The parent scope (without the current entity's name)
    pub parent_scope: String,
    /// The separator for this language (e.g., "::" for Rust, "." for Python)
    pub separator: &'static str,
}

/// Build parent scope by traversing AST parents to find scope containers
///
/// Returns the parent scope path (without the current entity's name) and the
/// language-specific separator. The caller should combine these with the entity
/// name to form the full qualified name.
pub fn build_qualified_name_from_ast(
    node: Node,
    source: &str,
    language: &str,
) -> QualifiedNameResult {
    build_qualified_name_with_skip(node, source, language, &[])
}

/// Build parent scope with optional scope filtering
///
/// Same as `build_qualified_name_from_ast` but allows skipping specific AST node
/// kinds during scope traversal. For example, skipping `method_definition` nodes
/// places parameter properties directly under their enclosing class rather than
/// under the constructor method.
///
/// # Arguments
/// * `node` - The AST node to start from
/// * `source` - The source code
/// * `language` - Language identifier for scope configuration lookup
/// * `skip_kinds` - AST node kinds to skip during scope traversal (e.g., `&["method_definition"]`)
pub fn build_qualified_name_with_skip(
    node: Node,
    source: &str,
    language: &str,
    skip_kinds: &[&str],
) -> QualifiedNameResult {
    let mut scope_parts = Vec::new();
    let mut current = node;

    // Find configuration for this language via inventory lookup
    let config = inventory::iter::<ScopeConfiguration>().find(|config| config.language == language);

    let (patterns, separator) = match config {
        Some(cfg) => (cfg.patterns, cfg.separator),
        None => {
            tracing::warn!(
                "No ScopeConfiguration registered for language '{language}'. \
                 Using default separator and empty patterns. \
                 Ensure the language module registers its configuration via inventory."
            );
            (
                &[] as &[ScopePattern],
                match language {
                    "rust" => "::",
                    _ => ".",
                },
            )
        }
    };

    // Walk up the tree collecting scope names
    while let Some(parent) = current.parent() {
        // Skip nodes whose kind is in the skip list
        if !skip_kinds.contains(&parent.kind()) {
            let scope_name = extract_scope_name_generic(parent, source, patterns);

            if let Some(name) = scope_name {
                scope_parts.push(name);
            }
        }

        current = parent;
    }

    // Reverse to get root-to-leaf order
    scope_parts.reverse();
    QualifiedNameResult {
        parent_scope: scope_parts.join(separator),
        separator,
    }
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

/// Derive module path for a language using registered configuration
///
/// Looks up the module path function via inventory and calls it if found.
/// Returns None if no configuration exists or no module_path_fn is registered.
pub fn derive_module_path_for_language(
    file_path: &Path,
    source_root: &Path,
    language: &str,
) -> Option<String> {
    let config = inventory::iter::<ScopeConfiguration>().find(|c| c.language == language);

    let Some(cfg) = config else {
        tracing::trace!("No ScopeConfiguration found for language '{language}'");
        return None;
    };

    let Some(module_path_fn) = cfg.module_path_fn else {
        tracing::trace!("No module_path_fn registered for language '{language}'");
        return None;
    };

    let result = module_path_fn(file_path, source_root);
    if result.is_none() {
        tracing::trace!(
            "module_path_fn returned None for {language} file: {}",
            file_path.display()
        );
    }
    result
}
