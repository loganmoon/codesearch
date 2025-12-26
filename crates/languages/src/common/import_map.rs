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

/// Language-agnostic import map for resolving bare identifiers to qualified names
#[derive(Debug, Default)]
pub struct ImportMap {
    /// Mapping from simple name to fully qualified path
    mappings: HashMap<String, String>,
    /// Language-specific path separator ("::" for Rust, "." for JS/TS/Python)
    separator: &'static str,
}

impl ImportMap {
    /// Create a new empty ImportMap with the given separator
    pub fn new(separator: &'static str) -> Self {
        Self {
            mappings: HashMap::new(),
            separator,
        }
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

/// Normalize Rust-relative paths (crate::, self::, super::) to absolute qualified names
///
/// # Arguments
/// * `path` - The path to normalize (e.g., "crate::foo::Bar", "self::utils::Helper",
///   "super::super::other")
/// * `package_name` - The current crate name (e.g., "codesearch_core"). If None or empty,
///   the package prefix is omitted from the result.
/// * `current_module` - The current module path (e.g., "entities::error"). Required for
///   accurate self:: and super:: resolution; if None, those prefixes resolve at package root.
///
/// # Returns
/// The normalized absolute path, or the original path if not a relative path.
///
/// # Notes
/// - Supports chained super:: prefixes (e.g., `super::super::foo` navigates up two levels)
/// - When context is missing, gracefully degrades to partial resolution
pub fn normalize_rust_path(
    path: &str,
    package_name: Option<&str>,
    current_module: Option<&str>,
) -> String {
    if let Some(rest) = path.strip_prefix("crate::") {
        // crate:: -> package_name::rest
        match package_name {
            Some(pkg) if !pkg.is_empty() => format!("{pkg}::{rest}"),
            _ => rest.to_string(),
        }
    } else if let Some(rest) = path.strip_prefix("self::") {
        // self:: -> package_name::current_module::rest
        match (package_name, current_module) {
            (Some(pkg), Some(module)) if !pkg.is_empty() && !module.is_empty() => {
                format!("{pkg}::{module}::{rest}")
            }
            (Some(pkg), _) if !pkg.is_empty() => format!("{pkg}::{rest}"),
            (_, Some(module)) if !module.is_empty() => format!("{module}::{rest}"),
            _ => rest.to_string(),
        }
    } else if path.starts_with("super::") {
        // super:: -> navigate up from current_module (supports chained super::super::)
        let mut remaining = path;
        let mut levels_up = 0;

        // Count how many super:: prefixes we have
        while let Some(rest) = remaining.strip_prefix("super::") {
            levels_up += 1;
            remaining = rest;
        }

        if let Some(module) = current_module {
            let parts: Vec<&str> = module.split("::").collect();
            if parts.len() > levels_up {
                // Navigate up by levels_up
                let parent = parts[..parts.len() - levels_up].join("::");
                match package_name {
                    Some(pkg) if !pkg.is_empty() => format!("{pkg}::{parent}::{remaining}"),
                    _ => format!("{parent}::{remaining}"),
                }
            } else {
                // At or beyond root level, super:: goes to package root
                match package_name {
                    Some(pkg) if !pkg.is_empty() => format!("{pkg}::{remaining}"),
                    _ => remaining.to_string(),
                }
            }
        } else {
            // No module context, return with package prefix if available
            match package_name {
                Some(pkg) if !pkg.is_empty() => format!("{pkg}::{remaining}"),
                _ => remaining.to_string(),
            }
        }
    } else {
        // Not a relative path, return as-is
        path.to_string()
    }
}

/// Resolve a Rust reference with path normalization
///
/// This extends resolve_reference() to handle crate::, self::, super:: prefixes.
///
/// Resolution order:
/// 1. If path starts with crate::/self::/super::, normalize it and return
/// 2. Otherwise, delegate to resolve_reference() which handles:
///    - Scoped paths (contains ::) - used as-is
///    - Import map lookup
///    - parent_scope::name fallback
///    - external::name marker for unresolved references
/// 3. Normalize the result if it contains Rust-relative prefixes (from import map)
pub fn resolve_rust_reference(
    name: &str,
    import_map: &ImportMap,
    parent_scope: Option<&str>,
    package_name: Option<&str>,
    current_module: Option<&str>,
) -> String {
    // First normalize any Rust-relative paths
    if name.starts_with("crate::") || name.starts_with("self::") || name.starts_with("super::") {
        return normalize_rust_path(name, package_name, current_module);
    }

    // Delegate to standard resolution
    let resolved = resolve_reference(name, import_map, parent_scope, "::");

    // Normalize result if it contains Rust-relative prefixes (e.g., from import map)
    if resolved.starts_with("crate::")
        || resolved.starts_with("self::")
        || resolved.starts_with("super::")
    {
        normalize_rust_path(&resolved, package_name, current_module)
    } else {
        resolved
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

/// Parse Rust use declarations
///
/// Handles:
/// - `use std::io::Read;` → ("Read", "std::io::Read")
/// - `use std::io::{Read, Write};` → [("Read", "std::io::Read"), ("Write", "std::io::Write")]
/// - `use std::io::Read as MyRead;` → ("MyRead", "std::io::Read")
/// - `use std::io::*;` → skipped (glob imports out of scope)
pub fn parse_rust_imports(root: Node, source: &str) -> ImportMap {
    let mut import_map = ImportMap::new("::");

    let query_source = r#"
        (use_declaration
          argument: (use_as_clause
            path: (_) @path
            alias: (identifier) @alias))

        (use_declaration
          argument: (scoped_identifier) @scoped_path)

        (use_declaration
          argument: (scoped_use_list
            path: (_) @base_path
            list: (use_list) @use_list))

        (use_declaration
          argument: (identifier) @simple_import)
    "#;

    let language = tree_sitter_rust::LANGUAGE.into();
    let query = match Query::new(&language, query_source) {
        Ok(q) => q,
        Err(_) => return import_map,
    };

    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, root, source.as_bytes());

    while let Some(query_match) = matches.next() {
        for capture in query_match.captures {
            let capture_name = query
                .capture_names()
                .get(capture.index as usize)
                .copied()
                .unwrap_or("");

            match capture_name {
                "alias" => {
                    // use X as Y - the alias name
                    if let (Some(path_cap), Ok(alias_text)) = (
                        query_match
                            .captures
                            .iter()
                            .find(|c| {
                                query.capture_names().get(c.index as usize).copied() == Some("path")
                            })
                            .map(|c| c.node),
                        capture.node.utf8_text(source.as_bytes()),
                    ) {
                        if let Ok(path_text) = path_cap.utf8_text(source.as_bytes()) {
                            import_map.add(alias_text, path_text);
                        }
                    }
                }
                "scoped_path" => {
                    // use std::io::Read - extract the last segment as simple name
                    if let Ok(full_path) = capture.node.utf8_text(source.as_bytes()) {
                        if let Some(simple_name) = full_path.rsplit("::").next() {
                            // Skip glob imports
                            if simple_name != "*" {
                                import_map.add(simple_name, full_path);
                            }
                        }
                    }
                }
                "use_list" => {
                    // use std::io::{Read, Write} - process the list
                    if let Some(base_path_cap) = query_match.captures.iter().find(|c| {
                        query.capture_names().get(c.index as usize).copied() == Some("base_path")
                    }) {
                        if let Ok(base_path) = base_path_cap.node.utf8_text(source.as_bytes()) {
                            parse_rust_use_list(capture.node, source, base_path, &mut import_map);
                        }
                    }
                }
                "simple_import" => {
                    // use identifier - rare but valid
                    if let Ok(name) = capture.node.utf8_text(source.as_bytes()) {
                        import_map.add(name, name);
                    }
                }
                _ => {}
            }
        }
    }

    import_map
}

/// Parse items in a Rust use list (e.g., `{Read, Write, BufReader as BR}`)
fn parse_rust_use_list(list_node: Node, source: &str, base_path: &str, import_map: &mut ImportMap) {
    let mut cursor = list_node.walk();

    for child in list_node.children(&mut cursor) {
        match child.kind() {
            "identifier" => {
                if let Ok(name) = child.utf8_text(source.as_bytes()) {
                    let full_path = format!("{base_path}::{name}");
                    import_map.add(name, &full_path);
                }
            }
            "use_as_clause" => {
                // Handle `Read as R` within a use list
                let mut path = None;
                let mut alias = None;
                let mut inner_cursor = child.walk();

                for inner_child in child.children(&mut inner_cursor) {
                    match inner_child.kind() {
                        "identifier" if path.is_none() => {
                            path = inner_child.utf8_text(source.as_bytes()).ok();
                        }
                        "identifier" if path.is_some() => {
                            alias = inner_child.utf8_text(source.as_bytes()).ok();
                        }
                        _ => {}
                    }
                }

                if let (Some(p), Some(a)) = (path, alias) {
                    let full_path = format!("{base_path}::{p}");
                    import_map.add(a, &full_path);
                }
            }
            "scoped_identifier" => {
                // Handle nested paths like `io::Read` within a use list
                if let Ok(scoped_path) = child.utf8_text(source.as_bytes()) {
                    let full_path = format!("{base_path}::{scoped_path}");
                    if let Some(simple_name) = scoped_path.rsplit("::").next() {
                        import_map.add(simple_name, &full_path);
                    }
                }
            }
            "self" => {
                // Handle `use foo::{self}` - imports the base path itself
                if let Some(simple_name) = base_path.rsplit("::").next() {
                    import_map.add(simple_name, base_path);
                }
            }
            _ => {}
        }
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
                &map,
                None,
                Some("mypackage"),
                Some("network")
            ),
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
                &map,
                None,
                Some("mypackage"),
                Some("utils::network")
            ),
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
                &map,
                None,
                Some("mypackage"),
                Some("utils::network")
            ),
            "mypackage::utils::other"
        );
    }

    #[test]
    fn test_resolve_rust_reference_falls_back_to_standard() {
        let mut map = ImportMap::new("::");
        map.add("Read", "std::io::Read");

        // Non-relative paths should use standard resolution
        assert_eq!(
            resolve_rust_reference("Read", &map, None, Some("mypackage"), Some("utils")),
            "std::io::Read"
        );

        // Already scoped paths pass through
        assert_eq!(
            resolve_rust_reference(
                "std::fmt::Display",
                &map,
                None,
                Some("mypackage"),
                Some("utils")
            ),
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
                &map,
                Some("parent::module"),
                Some("pkg"),
                Some("mod")
            ),
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
            resolve_rust_reference("Error", &map, None, Some("anyhow"), Some("error")),
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
            resolve_rust_reference("helper", &map, None, Some("mypackage"), Some("utils")),
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
            resolve_rust_reference("Foo", &map, None, Some("mypackage"), Some("utils::helpers")),
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
            resolve_rust_reference("Read", &map, None, Some("mypackage"), Some("utils")),
            "std::io::Read"
        );
    }
}
