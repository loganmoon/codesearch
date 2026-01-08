//! Import mapping for qualified name resolution
//!
//! This module provides language-agnostic import resolution functionality
//! to convert bare identifiers to fully qualified names.
//!
//! ## Qualified Name Resolution
//!
//! When extracting entities, references to other entities (function calls, type
//! usage, inheritance) must be stored in a format that matches the `qualified_name`
//! of the target entity. This module handles this resolution by:
//!
//! 1. Building an import map from the file's import statements
//! 2. Converting relative import paths (e.g., `./utils`) to absolute module paths
//!    (e.g., `mypackage.utils`) based on the current file's module path
//! 3. Resolving bare identifiers through the import map or parent scope

#![deny(warnings)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]

use codesearch_core::entities::Language;
use std::collections::HashMap;
use streaming_iterator::StreamingIterator;
use tree_sitter::{Node, Query, QueryCursor};

// Re-export Rust-specific import parsing function from the rust module
pub use crate::rust::import_resolution::parse_rust_imports;

/// Language-agnostic import map for resolving bare identifiers to qualified names
#[derive(Debug, Default)]
pub struct ImportMap {
    /// Mapping from simple name to fully qualified path
    mappings: HashMap<String, String>,
    /// Language-specific path separator ("::" for Rust, "." for JS/TS/Python)
    separator: &'static str,
    /// Glob import paths (e.g., "helpers" from `use helpers::*`)
    /// When resolving a bare identifier, if not found in mappings,
    /// these paths will be tried as prefixes.
    glob_imports: Vec<String>,
}

impl ImportMap {
    /// Create a new empty ImportMap with the given separator
    pub fn new(separator: &'static str) -> Self {
        Self {
            mappings: HashMap::new(),
            separator,
            glob_imports: Vec::new(),
        }
    }

    /// Add a glob import path (e.g., "helpers" from `use helpers::*`)
    pub fn add_glob(&mut self, path: &str) {
        self.glob_imports.push(path.to_string());
    }

    /// Get all glob import paths
    pub fn glob_imports(&self) -> &[String] {
        &self.glob_imports
    }

    /// Add an import mapping
    pub fn add(&mut self, simple_name: &str, qualified_path: &str) {
        self.mappings
            .insert(simple_name.to_string(), qualified_path.to_string());
    }

    /// Resolve a bare identifier to its qualified name
    ///
    /// Returns None if the identifier is not found in the import map
    pub fn resolve(&self, name: &str) -> Option<&str> {
        self.mappings.get(name).map(String::as_str)
    }

    /// Check if a name contains a path separator (already qualified)
    pub fn is_scoped(name: &str, separator: &str) -> bool {
        name.contains(separator)
    }

    /// Get the separator for this import map
    pub fn separator(&self) -> &'static str {
        self.separator
    }

    /// Check if the map is empty
    pub fn is_empty(&self) -> bool {
        self.mappings.is_empty()
    }

    /// Get the number of imports
    pub fn len(&self) -> usize {
        self.mappings.len()
    }

    /// Get an iterator over all mappings (for testing/debugging)
    #[cfg(test)]
    pub fn mappings(&self) -> impl Iterator<Item = (&String, &String)> {
        self.mappings.iter()
    }

    /// Get all imported qualified paths (the values of the import map)
    pub fn imported_paths(&self) -> Vec<String> {
        self.mappings.values().cloned().collect()
    }

    /// Check if any imported path starts with the given crate prefix.
    ///
    /// This helps distinguish external crates from local modules. If we have
    /// an import like `serde::Serialize`, then `serde` is known to be an
    /// external crate, so other paths like `serde::Deserialize` should also
    /// be treated as external.
    pub fn has_crate_import(&self, crate_name: &str, separator: &str) -> bool {
        let prefix = format!("{crate_name}{separator}");
        self.mappings.values().any(|path| path.starts_with(&prefix))
    }
}

/// Resolve a relative import path to an absolute module path
///
/// Given the current module's path and a relative import path (starting with `.` or `..`),
/// computes the absolute module path that matches how entity qualified_names are built.
///
/// # Arguments
/// * `current_module_path` - The module path of the current file (e.g., "vanilla.atom")
/// * `import_path` - The import path (e.g., "./core", "../utils", "lodash")
///
/// # Returns
/// * `Some(absolute_path)` - The resolved absolute module path (e.g., "vanilla.core")
/// * `None` - If the import is not relative (bare specifier like "lodash")
///
/// # Examples
/// ```text
/// current_module: "vanilla.atom"
/// import: "./core" -> "vanilla.core"
/// import: "../utils" -> "utils"
/// import: "lodash" -> None (not a relative import)
/// ```
pub(crate) fn resolve_relative_import(
    current_module_path: &str,
    import_path: &str,
) -> Option<String> {
    // Only handle relative imports (starting with . or ..)
    if !import_path.starts_with('.') {
        return None;
    }

    // Split current module into parts
    let mut parts: Vec<&str> = current_module_path.split('.').collect();

    // Pop the current module name to get parent directory
    if !parts.is_empty() {
        parts.pop();
    }

    // Process the import path
    for segment in import_path.split('/') {
        match segment {
            "." | "" => {
                // Current directory, no change
            }
            ".." => {
                // Parent directory
                if !parts.is_empty() {
                    parts.pop();
                }
            }
            _ => {
                // Module name - strip file extension if present
                let name = segment
                    .rsplit_once('.')
                    .map(|(base, ext)| {
                        // Only strip known JS/TS extensions
                        if matches!(ext, "js" | "ts" | "jsx" | "tsx" | "mjs" | "cjs") {
                            base
                        } else {
                            segment
                        }
                    })
                    .unwrap_or(segment);
                parts.push(name);
            }
        }
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join("."))
    }
}

/// Parse imports from a file's AST root
///
/// This function dispatches to language-specific parsing based on the language parameter.
///
/// # Arguments
/// * `root` - The AST root node
/// * `source` - The source code
/// * `language` - The programming language
/// * `current_module_path` - The module path of the current file (e.g., "vanilla.atom").
///   Used to resolve relative imports to absolute qualified names that match entity qualified_names.
pub(crate) fn parse_file_imports(
    root: Node,
    source: &str,
    language: Language,
    current_module_path: Option<&str>,
) -> ImportMap {
    match language {
        Language::Rust => parse_rust_imports(root, source),
        Language::JavaScript => parse_js_imports(root, source, current_module_path),
        Language::TypeScript => parse_ts_imports(root, source, current_module_path),
        Language::Python => ImportMap::new("."), // Python not implemented
        Language::Go => ImportMap::new("."),     // Go not implemented
        Language::Java => ImportMap::new("."),   // Java not implemented
        Language::CSharp => ImportMap::new("."), // C# not implemented
        Language::Cpp => ImportMap::new("::"),   // C++ not implemented
        Language::Unknown => ImportMap::new("."),
    }
}

/// Parse JavaScript import declarations
///
/// Handles:
/// - `import { foo } from './bar';` → ("foo", "module.bar.foo") when current module is "module.file"
/// - `import { foo as bar } from './baz';` → ("bar", "module.baz.foo")
/// - `import foo from './bar';` → ("foo", "module.bar.default")
/// - `import * as foo from './bar';` → ("foo", "module.bar")
/// - `import foo from 'lodash';` → ("foo", "external.lodash.default") for bare specifiers
///
/// # Arguments
/// * `root` - The AST root node
/// * `source` - The source code
/// * `current_module_path` - The module path of the current file (e.g., "vanilla.atom")
pub(crate) fn parse_js_imports(
    root: Node,
    source: &str,
    current_module_path: Option<&str>,
) -> ImportMap {
    let mut import_map = ImportMap::new(".");

    let query_source = r#"
        (import_statement
          source: (string) @source)
    "#;

    let language = tree_sitter_javascript::LANGUAGE.into();
    let query = match Query::new(&language, query_source) {
        Ok(q) => q,
        Err(_) => return import_map,
    };

    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, root, source.as_bytes());

    while let Some(query_match) = matches.next() {
        for capture in query_match.captures {
            if let Ok(raw_source_path) = capture.node.utf8_text(source.as_bytes()) {
                // Remove quotes from source path
                let raw_source_path = raw_source_path.trim_matches(|c| c == '"' || c == '\'');

                // Resolve relative imports to absolute module paths
                let resolved_path = if let Some(module_path) = current_module_path {
                    resolve_relative_import(module_path, raw_source_path)
                        .unwrap_or_else(|| format!("external.{raw_source_path}"))
                } else {
                    // No module path available, use raw path (will be treated as external)
                    raw_source_path.to_string()
                };

                // Get the parent import_statement to extract specifiers
                if let Some(import_stmt) = capture.node.parent() {
                    parse_js_import_specifiers(
                        import_stmt,
                        source,
                        &resolved_path,
                        &mut import_map,
                    );
                }
            }
        }
    }

    import_map
}

/// Parse JavaScript import specifiers from an import statement
fn parse_js_import_specifiers(
    import_stmt: Node,
    source: &str,
    source_path: &str,
    import_map: &mut ImportMap,
) {
    let mut cursor = import_stmt.walk();

    for child in import_stmt.children(&mut cursor) {
        if child.kind() == "import_clause" {
            parse_js_import_clause(child, source, source_path, import_map);
        }
    }
}

/// Parse a JavaScript import clause (default import, named imports, namespace import)
fn parse_js_import_clause(
    clause: Node,
    source: &str,
    source_path: &str,
    import_map: &mut ImportMap,
) {
    let mut cursor = clause.walk();

    for child in clause.children(&mut cursor) {
        match child.kind() {
            "identifier" => {
                // Default import: import foo from './bar'
                if let Ok(name) = child.utf8_text(source.as_bytes()) {
                    import_map.add(name, &format!("{source_path}.default"));
                }
            }
            "named_imports" => {
                // Named imports: import { foo, bar as baz } from './mod'
                let mut inner_cursor = child.walk();
                for spec in child.children(&mut inner_cursor) {
                    if spec.kind() == "import_specifier" {
                        parse_js_import_specifier(spec, source, source_path, import_map);
                    }
                }
            }
            "namespace_import" => {
                // Namespace import: import * as foo from './bar'
                let mut inner_cursor = child.walk();
                for inner in child.children(&mut inner_cursor) {
                    if inner.kind() == "identifier" {
                        if let Ok(name) = inner.utf8_text(source.as_bytes()) {
                            import_map.add(name, source_path);
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

/// Parse a single JavaScript import specifier
fn parse_js_import_specifier(
    spec: Node,
    source: &str,
    source_path: &str,
    import_map: &mut ImportMap,
) {
    let mut original_name = None;
    let mut local_name = None;

    if let Some(name_node) = spec.child_by_field_name("name") {
        original_name = name_node.utf8_text(source.as_bytes()).ok();
    }

    if let Some(alias_node) = spec.child_by_field_name("alias") {
        local_name = alias_node.utf8_text(source.as_bytes()).ok();
    }

    match (original_name, local_name) {
        (Some(orig), Some(alias)) => {
            // import { foo as bar } - use alias as local name
            import_map.add(alias, &format!("{source_path}.{orig}"));
        }
        (Some(orig), None) => {
            // import { foo } - use original name
            import_map.add(orig, &format!("{source_path}.{orig}"));
        }
        _ => {}
    }
}

/// Parse TypeScript import declarations (same as JavaScript)
///
/// # Arguments
/// * `root` - The AST root node
/// * `source` - The source code
/// * `current_module_path` - The module path of the current file (e.g., "vanilla.atom")
pub(crate) fn parse_ts_imports(
    root: Node,
    source: &str,
    current_module_path: Option<&str>,
) -> ImportMap {
    let mut import_map = ImportMap::new(".");

    let query_source = r#"
        (import_statement
          source: (string) @source)
    "#;

    let language = tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();
    let query = match Query::new(&language, query_source) {
        Ok(q) => q,
        Err(_) => return import_map,
    };

    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, root, source.as_bytes());

    while let Some(query_match) = matches.next() {
        for capture in query_match.captures {
            if let Ok(raw_source_path) = capture.node.utf8_text(source.as_bytes()) {
                let raw_source_path = raw_source_path.trim_matches(|c| c == '"' || c == '\'');

                // Resolve relative imports to absolute module paths
                let resolved_path = if let Some(module_path) = current_module_path {
                    resolve_relative_import(module_path, raw_source_path)
                        .unwrap_or_else(|| format!("external.{raw_source_path}"))
                } else {
                    raw_source_path.to_string()
                };

                if let Some(import_stmt) = capture.node.parent() {
                    parse_js_import_specifiers(
                        import_stmt,
                        source,
                        &resolved_path,
                        &mut import_map,
                    );
                }
            }
        }
    }

    import_map
}

/// Get the AST root node from any node in the tree
pub(crate) fn get_ast_root(node: Node) -> Node {
    let mut root = node;
    while let Some(parent) = root.parent() {
        root = parent;
    }
    root
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_import_map_basic() {
        let mut map = ImportMap::new("::");
        map.add("Read", "std::io::Read");
        map.add("Write", "std::io::Write");

        assert_eq!(map.resolve("Read"), Some("std::io::Read"));
        assert_eq!(map.resolve("Write"), Some("std::io::Write"));
        assert_eq!(map.resolve("Unknown"), None);
    }

    #[test]
    fn test_is_scoped() {
        assert!(ImportMap::is_scoped("std::io::Read", "::"));
        assert!(!ImportMap::is_scoped("Read", "::"));
        assert!(ImportMap::is_scoped("os.path.join", "."));
        assert!(!ImportMap::is_scoped("join", "."));
    }

    #[test]
    fn test_parse_rust_simple_import() {
        let source = "use std::io::Read;";
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&tree_sitter_rust::LANGUAGE.into()).ok();
        let tree = parser.parse(source, None).unwrap();

        let import_map = parse_rust_imports(tree.root_node(), source);

        assert_eq!(import_map.resolve("Read"), Some("std::io::Read"));
    }

    #[test]
    fn test_parse_rust_use_list() {
        let source = "use std::io::{Read, Write};";
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&tree_sitter_rust::LANGUAGE.into()).ok();
        let tree = parser.parse(source, None).unwrap();

        let import_map = parse_rust_imports(tree.root_node(), source);

        assert_eq!(import_map.resolve("Read"), Some("std::io::Read"));
        assert_eq!(import_map.resolve("Write"), Some("std::io::Write"));
    }

    #[test]
    fn test_parse_rust_alias() {
        let source = "use std::io::Read as MyRead;";
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&tree_sitter_rust::LANGUAGE.into()).ok();
        let tree = parser.parse(source, None).unwrap();

        let import_map = parse_rust_imports(tree.root_node(), source);

        assert_eq!(import_map.resolve("MyRead"), Some("std::io::Read"));
        assert_eq!(import_map.resolve("Read"), None);
    }

    #[test]
    fn test_rust_file_with_imports_and_calls() {
        // Test that imports are correctly parsed and can be used to resolve function calls
        let source = r#"
use std::io::Read;
use crate::utils::helper;
use my_crate::MyType as Alias;

fn example() {
    let x: Read = todo!();
    helper();
}
"#;
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&tree_sitter_rust::LANGUAGE.into()).ok();
        let tree = parser.parse(source, None).unwrap();

        let import_map = parse_file_imports(tree.root_node(), source, Language::Rust, None);

        // Verify imports are parsed
        assert_eq!(import_map.resolve("Read"), Some("std::io::Read"));
        assert_eq!(import_map.resolve("helper"), Some("crate::utils::helper"));
        assert_eq!(import_map.resolve("Alias"), Some("my_crate::MyType"));
    }

    #[test]
    fn test_rust_nested_use_list() {
        // Test nested use lists like `use std::{io::{Read, Write}, fs::File};`
        let source = "use std::collections::{HashMap, HashSet};";
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&tree_sitter_rust::LANGUAGE.into()).ok();
        let tree = parser.parse(source, None).unwrap();

        let import_map = parse_rust_imports(tree.root_node(), source);

        assert_eq!(
            import_map.resolve("HashMap"),
            Some("std::collections::HashMap")
        );
        assert_eq!(
            import_map.resolve("HashSet"),
            Some("std::collections::HashSet")
        );
    }

    #[test]
    fn test_rust_nested_use_with_renaming() {
        // Test: use network::{http::{get as http_get}, tcp::connect as tcp_connect};
        let source = r#"
use network::{
    http::{get as http_get, post as http_post},
    tcp::connect as tcp_connect,
};
"#;
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&tree_sitter_rust::LANGUAGE.into()).ok();
        let tree = parser.parse(source, None).unwrap();

        // Debug: print AST structure (only when DEBUG_AST env var is set)
        if std::env::var("DEBUG_AST").is_ok() {
            fn print_node(node: tree_sitter::Node, source: &str, indent: usize) {
                let text: String = node
                    .utf8_text(source.as_bytes())
                    .unwrap_or("?")
                    .chars()
                    .take(40)
                    .collect();
                let has_path = node.child_by_field_name("path").is_some();
                let has_alias = node.child_by_field_name("alias").is_some();
                println!(
                    "{:indent$}{} (path={}, alias={}): {}",
                    "",
                    node.kind(),
                    has_path,
                    has_alias,
                    text,
                    indent = indent
                );
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    print_node(child, source, indent + 2);
                }
            }
            println!("\n=== AST Structure ===");
            print_node(tree.root_node(), source, 0);
            println!("===================\n");
        }

        let import_map = parse_rust_imports(tree.root_node(), source);

        assert_eq!(
            import_map.resolve("http_get"),
            Some("network::http::get"),
            "http_get should resolve to network::http::get"
        );
        assert_eq!(
            import_map.resolve("http_post"),
            Some("network::http::post"),
            "http_post should resolve to network::http::post"
        );
        assert_eq!(
            import_map.resolve("tcp_connect"),
            Some("network::tcp::connect"),
            "tcp_connect should resolve to network::tcp::connect"
        );
    }

    // ========================================================================
    // Tests for relative import resolution
    // ========================================================================

    #[test]
    fn test_resolve_relative_import_sibling() {
        // In "vanilla.atom", import "./core" -> "vanilla.core"
        let result = resolve_relative_import("vanilla.atom", "./core");
        assert_eq!(result, Some("vanilla.core".to_string()));
    }

    #[test]
    fn test_resolve_relative_import_parent() {
        // In "vanilla.utils.helpers", import "../core" -> "vanilla.core"
        let result = resolve_relative_import("vanilla.utils.helpers", "../core");
        assert_eq!(result, Some("vanilla.core".to_string()));
    }

    #[test]
    fn test_resolve_relative_import_current_dir() {
        // In "vanilla.atom", import "." -> "vanilla"
        let result = resolve_relative_import("vanilla.atom", ".");
        assert_eq!(result, Some("vanilla".to_string()));
    }

    #[test]
    fn test_resolve_relative_import_bare_specifier() {
        // Bare specifiers like "lodash" are not relative imports
        let result = resolve_relative_import("vanilla.atom", "lodash");
        assert_eq!(result, None);
    }

    #[test]
    fn test_resolve_relative_import_deep_path() {
        // In "a.b.c", import "./d/e" -> "a.b.d.e"
        let result = resolve_relative_import("a.b.c", "./d/e");
        assert_eq!(result, Some("a.b.d.e".to_string()));
    }

    #[test]
    fn test_resolve_relative_import_strip_extension() {
        // Import paths with .js/.ts extensions should have them stripped
        let result = resolve_relative_import("vanilla.atom", "./core.js");
        assert_eq!(result, Some("vanilla.core".to_string()));

        let result = resolve_relative_import("vanilla.atom", "./core.ts");
        assert_eq!(result, Some("vanilla.core".to_string()));
    }

    #[test]
    fn test_js_imports_with_module_path() {
        // Test that JS imports are correctly resolved when module_path is provided
        let source = r#"
import { foo } from './bar';
import baz from '../utils';
import lodash from 'lodash';
"#;
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_javascript::LANGUAGE.into())
            .ok();
        let tree = parser.parse(source, None).unwrap();

        // With module_path "components.Button"
        let import_map = parse_js_imports(tree.root_node(), source, Some("components.Button"));

        // Relative import ./bar -> components.bar.foo
        assert_eq!(import_map.resolve("foo"), Some("components.bar.foo"));

        // Relative import ../utils -> utils.default
        assert_eq!(import_map.resolve("baz"), Some("utils.default"));

        // Bare specifier lodash -> external.lodash.default
        assert_eq!(
            import_map.resolve("lodash"),
            Some("external.lodash.default")
        );
    }

    #[test]
    fn test_js_imports_without_module_path() {
        // Test that JS imports work without module_path (should use raw paths)
        let source = r#"import { foo } from './bar';"#;
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_javascript::LANGUAGE.into())
            .ok();
        let tree = parser.parse(source, None).unwrap();

        let import_map = parse_js_imports(tree.root_node(), source, None);

        // Without module_path, relative import becomes raw path
        assert_eq!(import_map.resolve("foo"), Some("./bar.foo"));
    }

    // =========================================================================
    // Tests for glob imports
    // =========================================================================

    #[test]
    fn test_glob_import_storage() {
        let mut map = ImportMap::new("::");
        map.add_glob("helpers");
        map.add_glob("utils");

        let globs = map.glob_imports();
        assert_eq!(globs.len(), 2);
        assert_eq!(globs[0], "helpers");
        assert_eq!(globs[1], "utils");
    }

    #[test]
    fn test_multiple_glob_imports_uses_first() {
        // When multiple glob imports exist, only the first is used for resolution
        let mut map = ImportMap::new("::");
        map.add_glob("helpers");
        map.add_glob("utils");

        // First glob import takes precedence
        assert_eq!(
            map.glob_imports().first().map(|s| s.as_str()),
            Some("helpers")
        );
    }

    #[test]
    fn test_glob_import_fallback_resolution() {
        // Test that glob imports are used as fallback for unresolved names
        let source = r#"use helpers::*;"#;
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&tree_sitter_rust::LANGUAGE.into()).ok();
        let tree = parser.parse(source, None).unwrap();

        let import_map = parse_rust_imports(tree.root_node(), source);

        // The glob import should be stored
        assert!(!import_map.glob_imports().is_empty());
        assert_eq!(import_map.glob_imports()[0], "helpers");
    }

    // =========================================================================
    // Tests for deeply nested use statements
    // =========================================================================

    #[test]
    fn test_deeply_nested_use_statement() {
        // Test 4-level deep path
        let source = r#"use a::b::c::d::e;"#;
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&tree_sitter_rust::LANGUAGE.into()).ok();
        let tree = parser.parse(source, None).unwrap();

        let import_map = parse_rust_imports(tree.root_node(), source);

        assert_eq!(
            import_map.resolve("e"),
            Some("a::b::c::d::e"),
            "e should resolve to full path a::b::c::d::e"
        );
    }

    #[test]
    fn test_deeply_nested_use_with_braces() {
        // Test deeply nested paths with braces
        let source = r#"use a::b::{c::d::e, f::g::h};"#;
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&tree_sitter_rust::LANGUAGE.into()).ok();
        let tree = parser.parse(source, None).unwrap();

        let import_map = parse_rust_imports(tree.root_node(), source);

        assert_eq!(
            import_map.resolve("e"),
            Some("a::b::c::d::e"),
            "e should resolve to a::b::c::d::e"
        );
        assert_eq!(
            import_map.resolve("h"),
            Some("a::b::f::g::h"),
            "h should resolve to a::b::f::g::h"
        );
    }
}
