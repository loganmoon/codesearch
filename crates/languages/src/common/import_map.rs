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

// Re-export Rust-specific functions from the rust module for backward compatibility
pub use crate::rust::import_resolution::{
    normalize_rust_path, parse_rust_imports, parse_trait_impl_short_form, resolve_rust_reference,
};

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

    /// Get all imported qualified paths with Rust path normalization
    ///
    /// Normalizes crate::, self::, and super:: prefixes to absolute paths.
    /// This is needed because import statements store paths with relative prefixes,
    /// but entity qualified_names use absolute paths (package::module::name).
    pub fn imported_paths_normalized(
        &self,
        package_name: Option<&str>,
        current_module: Option<&str>,
    ) -> Vec<String> {
        self.mappings
            .values()
            .map(|path| normalize_rust_path(path, package_name, current_module))
            .collect()
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

/// Resolve a reference to a qualified name using import map and scope context
///
/// Resolution order:
/// 1. If already scoped (contains separator), use as-is
/// 2. Try import map
/// 3. Try parent_scope::name
/// 4. Mark as external::name
pub fn resolve_reference(
    name: &str,
    import_map: &ImportMap,
    parent_scope: Option<&str>,
    separator: &str,
) -> String {
    // Already scoped? Use as-is
    if ImportMap::is_scoped(name, separator) {
        return name.to_string();
    }

    // Try import map
    if let Some(resolved) = import_map.resolve(name) {
        return resolved.to_string();
    }

    // Try parent scope
    if let Some(scope) = parent_scope {
        if !scope.is_empty() {
            return format!("{scope}{separator}{name}");
        }
    }

    // Mark as external
    format!("external{separator}{name}")
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
pub fn resolve_relative_import(current_module_path: &str, import_path: &str) -> Option<String> {
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
pub fn parse_file_imports(
    root: Node,
    source: &str,
    language: Language,
    current_module_path: Option<&str>,
) -> ImportMap {
    match language {
        Language::Rust => parse_rust_imports(root, source),
        Language::JavaScript => parse_js_imports(root, source, current_module_path),
        Language::TypeScript => parse_ts_imports(root, source, current_module_path),
        Language::Python => parse_python_imports(root, source, current_module_path),
        Language::Go => ImportMap::new("."), // Go not fully implemented
        Language::Java => ImportMap::new("."), // Java not implemented
        Language::CSharp => ImportMap::new("."), // C# not implemented
        Language::Cpp => ImportMap::new("::"), // C++ not implemented
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
pub fn parse_js_imports(root: Node, source: &str, current_module_path: Option<&str>) -> ImportMap {
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
pub fn parse_ts_imports(root: Node, source: &str, current_module_path: Option<&str>) -> ImportMap {
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

/// Resolve a Python-style relative import to an absolute module path
///
/// Python relative imports use leading dots:
/// - `.` means current package
/// - `..` means parent package
/// - `.module` means sibling module
///
/// # Arguments
/// * `current_module_path` - The module path of the current file (e.g., "mypackage.utils")
/// * `relative_text` - The relative import text (e.g., ".", ".helpers", "..")
/// * `import_name` - The name being imported (e.g., "foo", "bar")
///
/// # Returns
/// The fully resolved qualified name
fn resolve_python_relative_import(
    current_module_path: &str,
    relative_text: &str,
    import_name: &str,
) -> String {
    // Count leading dots
    let dot_count = relative_text.chars().take_while(|c| *c == '.').count();

    // Get the module part after the dots (if any)
    let module_suffix = relative_text.trim_start_matches('.');

    // Split current module into parts
    let mut parts: Vec<&str> = current_module_path.split('.').collect();

    // Pop current module name and one additional level for each extra dot
    // One dot = same package (pop current module)
    // Two dots = parent package (pop current module and parent)
    for _ in 0..dot_count {
        if !parts.is_empty() {
            parts.pop();
        }
    }

    // Add the module suffix if present
    if !module_suffix.is_empty() {
        for segment in module_suffix.split('.') {
            if !segment.is_empty() {
                parts.push(segment);
            }
        }
    }

    // Add the imported name
    parts.push(import_name);

    parts.join(".")
}

/// Parse Python import statements
///
/// Handles absolute imports:
/// - `from os.path import join` → ("join", "external.os.path.join") - stdlib
/// - `from os.path import join as j` → ("j", "external.os.path.join")
/// - `import os.path` → ("os", "external.os")
/// - `import os.path as osp` → ("osp", "external.os.path")
///
/// Handles relative imports when current_module_path is provided:
/// - `from . import foo` in `mypackage.utils` → ("foo", "mypackage.foo")
/// - `from .helpers import bar` in `mypackage.utils` → ("bar", "mypackage.helpers.bar")
/// - `from ..core import baz` in `mypackage.sub.utils` → ("baz", "mypackage.core.baz")
///
/// # Arguments
/// * `root` - The AST root node
/// * `source` - The source code
/// * `current_module_path` - The module path of the current file (e.g., "mypackage.utils")
pub fn parse_python_imports(
    root: Node,
    source: &str,
    current_module_path: Option<&str>,
) -> ImportMap {
    let mut import_map = ImportMap::new(".");

    // Query for various Python import patterns
    // Note: relative_import captures imports like `from . import foo` or `from ..utils import bar`
    let query_source = r#"
        ; Absolute imports: from module import name
        (import_from_statement
          module_name: (dotted_name) @module
          name: (dotted_name) @name)

        ; Absolute imports with alias: from module import name as alias
        (import_from_statement
          module_name: (dotted_name) @from_module
          (aliased_import
            name: (dotted_name) @aliased_name
            alias: (identifier) @alias))

        ; Relative imports: from . import name OR from .module import name
        (import_from_statement
          module_name: (relative_import) @relative_module
          name: (dotted_name) @rel_name)

        ; Relative imports with alias
        (import_from_statement
          module_name: (relative_import) @rel_from_module
          (aliased_import
            name: (dotted_name) @rel_aliased_name
            alias: (identifier) @rel_alias))

        ; import module
        (import_statement
          name: (dotted_name) @import_name)

        ; import module as alias
        (import_statement
          (aliased_import
            name: (dotted_name) @import_aliased_name
            alias: (identifier) @import_alias))
    "#;

    let language = tree_sitter_python::LANGUAGE.into();
    let query = match Query::new(&language, query_source) {
        Ok(q) => q,
        Err(_) => return import_map,
    };

    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, root, source.as_bytes());

    while let Some(query_match) = matches.next() {
        let captures: Vec<_> = query_match.captures.iter().collect();

        // Helper to find a capture by name
        let find_capture = |cap_name: &str| {
            captures
                .iter()
                .find(|c| query.capture_names().get(c.index as usize).copied() == Some(cap_name))
        };

        // Absolute imports: from module import name
        if let (Some(m), Some(n)) = (find_capture("module"), find_capture("name")) {
            if let (Ok(mod_text), Ok(name_text)) = (
                m.node.utf8_text(source.as_bytes()),
                n.node.utf8_text(source.as_bytes()),
            ) {
                // External/stdlib import - prefix with external
                let full_path = format!("external.{mod_text}.{name_text}");
                import_map.add(name_text, &full_path);
            }
        }

        // Absolute imports with alias: from module import name as alias
        if let (Some(m), Some(n), Some(a)) = (
            find_capture("from_module"),
            find_capture("aliased_name"),
            find_capture("alias"),
        ) {
            if let (Ok(mod_text), Ok(name_text), Ok(alias_text)) = (
                m.node.utf8_text(source.as_bytes()),
                n.node.utf8_text(source.as_bytes()),
                a.node.utf8_text(source.as_bytes()),
            ) {
                let full_path = format!("external.{mod_text}.{name_text}");
                import_map.add(alias_text, &full_path);
            }
        }

        // Relative imports: from . import name OR from .module import name
        if let (Some(rel_mod), Some(n)) =
            (find_capture("relative_module"), find_capture("rel_name"))
        {
            if let (Ok(rel_text), Ok(name_text)) = (
                rel_mod.node.utf8_text(source.as_bytes()),
                n.node.utf8_text(source.as_bytes()),
            ) {
                if let Some(module_path) = current_module_path {
                    let resolved = resolve_python_relative_import(module_path, rel_text, name_text);
                    import_map.add(name_text, &resolved);
                } else {
                    // No module path, use as-is
                    import_map.add(name_text, &format!("{rel_text}.{name_text}"));
                }
            }
        }

        // Relative imports with alias
        if let (Some(rel_mod), Some(n), Some(a)) = (
            find_capture("rel_from_module"),
            find_capture("rel_aliased_name"),
            find_capture("rel_alias"),
        ) {
            if let (Ok(rel_text), Ok(name_text), Ok(alias_text)) = (
                rel_mod.node.utf8_text(source.as_bytes()),
                n.node.utf8_text(source.as_bytes()),
                a.node.utf8_text(source.as_bytes()),
            ) {
                if let Some(module_path) = current_module_path {
                    let resolved = resolve_python_relative_import(module_path, rel_text, name_text);
                    import_map.add(alias_text, &resolved);
                } else {
                    import_map.add(alias_text, &format!("{rel_text}.{name_text}"));
                }
            }
        }

        // import module (absolute)
        if let Some(n) = find_capture("import_name") {
            if let Ok(name_text) = n.node.utf8_text(source.as_bytes()) {
                // For `import os.path`, the local name is just `os`
                let local_name = name_text.split('.').next().unwrap_or(name_text);
                // External import
                import_map.add(local_name, &format!("external.{local_name}"));
            }
        }

        // import module as alias (absolute)
        if let (Some(n), Some(a)) = (
            find_capture("import_aliased_name"),
            find_capture("import_alias"),
        ) {
            if let (Ok(name_text), Ok(alias_text)) = (
                n.node.utf8_text(source.as_bytes()),
                a.node.utf8_text(source.as_bytes()),
            ) {
                import_map.add(alias_text, &format!("external.{name_text}"));
            }
        }
    }

    import_map
}

/// Get the AST root node from any node in the tree
pub fn get_ast_root(node: Node) -> Node {
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
    fn test_resolve_reference() {
        let mut map = ImportMap::new("::");
        map.add("Read", "std::io::Read");

        // Already scoped - use as-is
        assert_eq!(
            resolve_reference("std::fmt::Display", &map, None, "::"),
            "std::fmt::Display"
        );

        // Found in imports
        assert_eq!(resolve_reference("Read", &map, None, "::"), "std::io::Read");

        // Not in imports, use parent scope
        assert_eq!(
            resolve_reference("foo", &map, Some("my_module"), "::"),
            "my_module::foo"
        );

        // Not found anywhere - mark as external
        assert_eq!(resolve_reference("bar", &map, None, "::"), "external::bar");
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
    fn test_parse_python_from_import() {
        let source = "from os.path import join";
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_python::LANGUAGE.into())
            .ok();
        let tree = parser.parse(source, None).unwrap();

        let import_map = parse_python_imports(tree.root_node(), source, None);

        // Absolute imports are marked as external
        assert_eq!(import_map.resolve("join"), Some("external.os.path.join"));
    }

    #[test]
    fn test_parse_python_from_import_alias() {
        let source = "from os.path import join as j";
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_python::LANGUAGE.into())
            .ok();
        let tree = parser.parse(source, None).unwrap();

        let import_map = parse_python_imports(tree.root_node(), source, None);

        // Absolute imports are marked as external
        assert_eq!(import_map.resolve("j"), Some("external.os.path.join"));
        assert_eq!(import_map.resolve("join"), None);
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

        // Verify resolution works
        assert_eq!(
            resolve_reference("Read", &import_map, None, "::"),
            "std::io::Read"
        );
        assert_eq!(
            resolve_reference("helper", &import_map, None, "::"),
            "crate::utils::helper"
        );
        assert_eq!(
            resolve_reference("unknown", &import_map, Some("my_module"), "::"),
            "my_module::unknown"
        );
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

    #[test]
    fn test_resolution_priority() {
        // Test that resolution follows the priority: scoped > imports > parent_scope > external
        let mut map = ImportMap::new("::");
        map.add("Foo", "imported::Foo");

        // 1. Already scoped takes priority
        assert_eq!(
            resolve_reference("other::Foo", &map, Some("parent"), "::"),
            "other::Foo"
        );

        // 2. Import mapping takes priority over parent scope
        assert_eq!(
            resolve_reference("Foo", &map, Some("parent"), "::"),
            "imported::Foo"
        );

        // 3. Parent scope used when not in imports
        assert_eq!(
            resolve_reference("Bar", &map, Some("parent"), "::"),
            "parent::Bar"
        );

        // 4. External fallback when nothing else matches
        assert_eq!(resolve_reference("Baz", &map, None, "::"), "external::Baz");
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
    fn test_resolve_python_relative_single_dot() {
        // from . import foo in mypackage.utils -> mypackage.foo
        let result = resolve_python_relative_import("mypackage.utils", ".", "foo");
        assert_eq!(result, "mypackage.foo");
    }

    #[test]
    fn test_resolve_python_relative_double_dot() {
        // from .. import foo in mypackage.sub.utils -> mypackage.foo
        let result = resolve_python_relative_import("mypackage.sub.utils", "..", "foo");
        assert_eq!(result, "mypackage.foo");
    }

    #[test]
    fn test_resolve_python_relative_with_module() {
        // from .helpers import bar in mypackage.utils -> mypackage.helpers.bar
        let result = resolve_python_relative_import("mypackage.utils", ".helpers", "bar");
        assert_eq!(result, "mypackage.helpers.bar");
    }

    #[test]
    fn test_resolve_python_relative_parent_with_module() {
        // from ..core import baz in mypackage.sub.utils -> mypackage.core.baz
        let result = resolve_python_relative_import("mypackage.sub.utils", "..core", "baz");
        assert_eq!(result, "mypackage.core.baz");
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

    // ========================================================================
    // Tests for Rust path normalization (crate::, self::, super::)
    // ========================================================================

    #[test]
    fn test_normalize_rust_path_crate_with_package() {
        // crate::foo::Bar with package "mypackage" -> mypackage::foo::Bar
        assert_eq!(
            normalize_rust_path("crate::foo::Bar", Some("mypackage"), Some("utils")),
            "mypackage::foo::Bar"
        );
    }

    #[test]
    fn test_normalize_rust_path_crate_without_package() {
        // crate::foo::Bar without package -> foo::Bar
        assert_eq!(
            normalize_rust_path("crate::foo::Bar", None, Some("utils")),
            "foo::Bar"
        );
    }

    #[test]
    fn test_normalize_rust_path_self_with_module() {
        // self::helper in mypackage::utils::network -> mypackage::utils::network::helper
        assert_eq!(
            normalize_rust_path("self::helper", Some("mypackage"), Some("utils::network")),
            "mypackage::utils::network::helper"
        );
    }

    #[test]
    fn test_normalize_rust_path_self_without_module() {
        // self::helper with package but no module -> mypackage::helper
        assert_eq!(
            normalize_rust_path("self::helper", Some("mypackage"), None),
            "mypackage::helper"
        );
    }

    #[test]
    fn test_normalize_rust_path_super_with_parent() {
        // super::other in mypackage::utils::network -> mypackage::utils::other
        assert_eq!(
            normalize_rust_path("super::other", Some("mypackage"), Some("utils::network")),
            "mypackage::utils::other"
        );
    }

    #[test]
    fn test_normalize_rust_path_super_at_root() {
        // super::other in mypackage::utils (single-level module) -> mypackage::other
        assert_eq!(
            normalize_rust_path("super::other", Some("mypackage"), Some("utils")),
            "mypackage::other"
        );
    }

    #[test]
    fn test_normalize_rust_path_super_without_module() {
        // super::other without module context -> mypackage::other
        assert_eq!(
            normalize_rust_path("super::other", Some("mypackage"), None),
            "mypackage::other"
        );
    }

    #[test]
    fn test_normalize_rust_path_not_relative() {
        // std::io::Read is not a relative path, should be returned as-is
        assert_eq!(
            normalize_rust_path("std::io::Read", Some("mypackage"), Some("utils")),
            "std::io::Read"
        );
    }

    #[test]
    fn test_resolve_rust_reference_crate_path() {
        let map = ImportMap::new("::");
        // crate:: path should be normalized, not passed through
        assert_eq!(
            resolve_rust_reference(
                "crate::utils::helper",
                "helper",
                &map,
                None,
                Some("mypackage"),
                Some("network")
            )
            .target,
            "mypackage::utils::helper"
        );
    }

    #[test]
    fn test_resolve_rust_reference_self_path() {
        let map = ImportMap::new("::");
        // self:: path should be normalized
        assert_eq!(
            resolve_rust_reference(
                "self::helper",
                "helper",
                &map,
                None,
                Some("mypackage"),
                Some("utils::network")
            )
            .target,
            "mypackage::utils::network::helper"
        );
    }

    #[test]
    fn test_resolve_rust_reference_super_path() {
        let map = ImportMap::new("::");
        // super:: path should be normalized
        assert_eq!(
            resolve_rust_reference(
                "super::other",
                "other",
                &map,
                None,
                Some("mypackage"),
                Some("utils::network")
            )
            .target,
            "mypackage::utils::other"
        );
    }

    #[test]
    fn test_resolve_rust_reference_falls_back_to_standard() {
        let mut map = ImportMap::new("::");
        map.add("Read", "std::io::Read");

        // Non-relative paths should use standard resolution
        assert_eq!(
            resolve_rust_reference("Read", "Read", &map, None, Some("mypackage"), Some("utils"))
                .target,
            "std::io::Read"
        );

        // Already scoped paths pass through
        assert_eq!(
            resolve_rust_reference(
                "std::fmt::Display",
                "Display",
                &map,
                None,
                Some("mypackage"),
                Some("utils")
            )
            .target,
            "std::fmt::Display"
        );
    }

    #[test]
    fn test_resolve_rust_reference_with_parent_scope() {
        let map = ImportMap::new("::");
        // When not a crate::/self::/super:: path and not in imports,
        // should fall back to parent_scope::name
        assert_eq!(
            resolve_rust_reference(
                "MyType",
                "MyType",
                &map,
                Some("parent::module"),
                Some("pkg"),
                Some("mod")
            )
            .target,
            "parent::module::MyType"
        );
    }

    // ========================================================================
    // Tests for chained super:: paths (super::super::foo)
    // ========================================================================

    #[test]
    fn test_normalize_rust_path_double_super() {
        // super::super::thing in mypackage::a::b::c should resolve to mypackage::a::thing
        assert_eq!(
            normalize_rust_path("super::super::thing", Some("mypackage"), Some("a::b::c")),
            "mypackage::a::thing"
        );
    }

    #[test]
    fn test_normalize_rust_path_triple_super() {
        // super::super::super::thing in mypackage::a::b::c::d should resolve to mypackage::a::thing
        assert_eq!(
            normalize_rust_path(
                "super::super::super::thing",
                Some("mypackage"),
                Some("a::b::c::d")
            ),
            "mypackage::a::thing"
        );
    }

    #[test]
    fn test_normalize_rust_path_super_exceeds_depth() {
        // super::super::super in mypackage::a::b (only 2 levels) should go to package root
        assert_eq!(
            normalize_rust_path(
                "super::super::super::thing",
                Some("mypackage"),
                Some("a::b")
            ),
            "mypackage::thing"
        );
    }

    #[test]
    fn test_normalize_rust_path_super_exactly_matches_depth() {
        // super::super in mypackage::a::b (exactly 2 levels) should go to package root
        assert_eq!(
            normalize_rust_path("super::super::thing", Some("mypackage"), Some("a::b")),
            "mypackage::thing"
        );
    }

    // ========================================================================
    // Tests for import map result normalization
    // ========================================================================

    #[test]
    fn test_resolve_rust_reference_normalizes_import_map_crate_prefix() {
        // When import map returns crate::Error (from `use crate::Error;`),
        // the result should be normalized to package::Error
        let mut map = ImportMap::new("::");
        map.add("Error", "crate::Error");

        assert_eq!(
            resolve_rust_reference("Error", "Error", &map, None, Some("anyhow"), Some("error"))
                .target,
            "anyhow::Error"
        );
    }

    #[test]
    fn test_resolve_rust_reference_normalizes_import_map_self_prefix() {
        // When import map returns self::helper (from `use self::helper;`),
        // the result should be normalized
        let mut map = ImportMap::new("::");
        map.add("helper", "self::helper");

        assert_eq!(
            resolve_rust_reference(
                "helper",
                "helper",
                &map,
                None,
                Some("mypackage"),
                Some("utils")
            )
            .target,
            "mypackage::utils::helper"
        );
    }

    #[test]
    fn test_resolve_rust_reference_normalizes_import_map_super_prefix() {
        // When import map returns super::types::Foo (from `use super::types::Foo;`),
        // the result should be normalized
        let mut map = ImportMap::new("::");
        map.add("Foo", "super::types::Foo");

        assert_eq!(
            resolve_rust_reference(
                "Foo",
                "Foo",
                &map,
                None,
                Some("mypackage"),
                Some("utils::helpers")
            )
            .target,
            "mypackage::utils::types::Foo"
        );
    }

    #[test]
    fn test_resolve_rust_reference_no_normalization_for_absolute() {
        // When import map returns an absolute path (std::io::Read),
        // no normalization should occur
        let mut map = ImportMap::new("::");
        map.add("Read", "std::io::Read");

        assert_eq!(
            resolve_rust_reference("Read", "Read", &map, None, Some("mypackage"), Some("utils"))
                .target,
            "std::io::Read"
        );
    }

    // ========================================================================
    // Tests for has_crate_import
    // ========================================================================

    #[test]
    fn test_has_crate_import_exists() {
        let mut map = ImportMap::new("::");
        map.add("Serialize", "serde::Serialize");
        map.add("Deserialize", "serde::Deserialize");

        assert!(map.has_crate_import("serde", "::"));
    }

    #[test]
    fn test_has_crate_import_not_exists() {
        let mut map = ImportMap::new("::");
        map.add("Serialize", "serde::Serialize");

        assert!(!map.has_crate_import("tokio", "::"));
    }

    #[test]
    fn test_has_crate_import_partial_match() {
        let mut map = ImportMap::new("::");
        map.add("Foo", "serde_json::Foo");

        // "serde" should not match "serde_json"
        assert!(!map.has_crate_import("serde", "::"));
        assert!(map.has_crate_import("serde_json", "::"));
    }

    // ========================================================================
    // Tests for external crate detection in resolve_rust_reference
    // ========================================================================

    #[test]
    fn test_resolve_rust_reference_detects_external_crate_from_imports() {
        // If we have `use serde::Serialize;` in imports, then
        // `serde::Deserialize` should be recognized as external
        let mut map = ImportMap::new("::");
        map.add("Serialize", "serde::Serialize");

        // This should NOT be prefixed with my_crate since serde is a known external
        let resolved = resolve_rust_reference(
            "serde::Deserialize",
            "Deserialize",
            &map,
            None,
            Some("my_crate"),
            None,
        );
        assert_eq!(resolved.target, "serde::Deserialize");
        assert!(resolved.is_external);
    }

    #[test]
    fn test_resolve_rust_reference_prefixes_unknown_scoped_path() {
        // Unknown scoped paths like `utils::helper` should get package prefix
        let map = ImportMap::new("::");

        let resolved = resolve_rust_reference(
            "utils::helper",
            "helper",
            &map,
            None,
            Some("my_crate"),
            None,
        );
        assert_eq!(resolved.target, "my_crate::utils::helper");
        assert!(!resolved.is_external);
    }

    #[test]
    fn test_resolve_rust_reference_keeps_own_crate_prefix() {
        // Paths already starting with the package name should stay as-is
        let map = ImportMap::new("::");

        let resolved = resolve_rust_reference(
            "my_crate::utils::helper",
            "helper",
            &map,
            None,
            Some("my_crate"),
            None,
        );
        assert_eq!(resolved.target, "my_crate::utils::helper");
        assert!(!resolved.is_external);
    }

    #[test]
    fn test_resolve_rust_reference_std_is_external() {
        // std paths are always external
        let map = ImportMap::new("::");

        let resolved =
            resolve_rust_reference("std::io::Read", "Read", &map, None, Some("my_crate"), None);
        assert_eq!(resolved.target, "std::io::Read");
        assert!(resolved.is_external);
    }

    #[test]
    fn test_resolve_rust_reference_core_is_external() {
        // core paths are always external
        let map = ImportMap::new("::");

        let resolved = resolve_rust_reference(
            "core::fmt::Display",
            "Display",
            &map,
            None,
            Some("my_crate"),
            None,
        );
        assert_eq!(resolved.target, "core::fmt::Display");
        assert!(resolved.is_external);
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
