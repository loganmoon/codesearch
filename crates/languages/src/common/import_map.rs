//! Import mapping for qualified name resolution
//!
//! This module provides language-agnostic import resolution functionality
//! to convert bare identifiers to fully qualified names.

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

/// Parse imports from a file's AST root
///
/// This function dispatches to language-specific parsing based on the language parameter.
pub fn parse_file_imports(root: Node, source: &str, language: Language) -> ImportMap {
    match language {
        Language::Rust => parse_rust_imports(root, source),
        Language::JavaScript => parse_js_imports(root, source),
        Language::TypeScript => parse_ts_imports(root, source),
        Language::Python => parse_python_imports(root, source),
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
/// - `import { foo } from './bar';` → ("foo", "./bar.foo")
/// - `import { foo as bar } from './baz';` → ("bar", "./baz.foo")
/// - `import foo from './bar';` → ("foo", "./bar.default")
/// - `import * as foo from './bar';` → ("foo", "./bar")
pub fn parse_js_imports(root: Node, source: &str) -> ImportMap {
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
            if let Ok(source_path) = capture.node.utf8_text(source.as_bytes()) {
                // Remove quotes from source path
                let source_path = source_path.trim_matches(|c| c == '"' || c == '\'');

                // Get the parent import_statement to extract specifiers
                if let Some(import_stmt) = capture.node.parent() {
                    parse_js_import_specifiers(import_stmt, source, source_path, &mut import_map);
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
pub fn parse_ts_imports(root: Node, source: &str) -> ImportMap {
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
            if let Ok(source_path) = capture.node.utf8_text(source.as_bytes()) {
                let source_path = source_path.trim_matches(|c| c == '"' || c == '\'');

                if let Some(import_stmt) = capture.node.parent() {
                    parse_js_import_specifiers(import_stmt, source, source_path, &mut import_map);
                }
            }
        }
    }

    import_map
}

/// Parse Python import statements
///
/// Handles:
/// - `from os.path import join` → ("join", "os.path.join")
/// - `from os.path import join as j` → ("j", "os.path.join")
/// - `import os.path` → ("os", "os")
/// - `import os.path as osp` → ("osp", "os.path")
pub fn parse_python_imports(root: Node, source: &str) -> ImportMap {
    let mut import_map = ImportMap::new(".");

    let query_source = r#"
        (import_from_statement
          module_name: (dotted_name) @module
          name: (dotted_name) @name)

        (import_from_statement
          module_name: (dotted_name) @from_module
          (aliased_import
            name: (dotted_name) @aliased_name
            alias: (identifier) @alias))

        (import_statement
          name: (dotted_name) @import_name)

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

        // Process based on which captures are present
        let module = captures
            .iter()
            .find(|c| query.capture_names().get(c.index as usize).copied() == Some("module"));
        let name = captures
            .iter()
            .find(|c| query.capture_names().get(c.index as usize).copied() == Some("name"));
        let from_module = captures
            .iter()
            .find(|c| query.capture_names().get(c.index as usize).copied() == Some("from_module"));
        let aliased_name = captures
            .iter()
            .find(|c| query.capture_names().get(c.index as usize).copied() == Some("aliased_name"));
        let alias = captures
            .iter()
            .find(|c| query.capture_names().get(c.index as usize).copied() == Some("alias"));
        let import_name = captures
            .iter()
            .find(|c| query.capture_names().get(c.index as usize).copied() == Some("import_name"));
        let import_aliased_name = captures.iter().find(|c| {
            query.capture_names().get(c.index as usize).copied() == Some("import_aliased_name")
        });
        let import_alias = captures
            .iter()
            .find(|c| query.capture_names().get(c.index as usize).copied() == Some("import_alias"));

        // from module import name
        if let (Some(m), Some(n)) = (module, name) {
            if let (Ok(mod_text), Ok(name_text)) = (
                m.node.utf8_text(source.as_bytes()),
                n.node.utf8_text(source.as_bytes()),
            ) {
                let full_path = format!("{mod_text}.{name_text}");
                import_map.add(name_text, &full_path);
            }
        }

        // from module import name as alias
        if let (Some(m), Some(n), Some(a)) = (from_module, aliased_name, alias) {
            if let (Ok(mod_text), Ok(name_text), Ok(alias_text)) = (
                m.node.utf8_text(source.as_bytes()),
                n.node.utf8_text(source.as_bytes()),
                a.node.utf8_text(source.as_bytes()),
            ) {
                let full_path = format!("{mod_text}.{name_text}");
                import_map.add(alias_text, &full_path);
            }
        }

        // import module
        if let Some(n) = import_name {
            if let Ok(name_text) = n.node.utf8_text(source.as_bytes()) {
                // For `import os.path`, the local name is just `os`
                let local_name = name_text.split('.').next().unwrap_or(name_text);
                import_map.add(local_name, local_name);
            }
        }

        // import module as alias
        if let (Some(n), Some(a)) = (import_aliased_name, import_alias) {
            if let (Ok(name_text), Ok(alias_text)) = (
                n.node.utf8_text(source.as_bytes()),
                a.node.utf8_text(source.as_bytes()),
            ) {
                import_map.add(alias_text, name_text);
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

        let import_map = parse_python_imports(tree.root_node(), source);

        assert_eq!(import_map.resolve("join"), Some("os.path.join"));
    }

    #[test]
    fn test_parse_python_from_import_alias() {
        let source = "from os.path import join as j";
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_python::LANGUAGE.into())
            .ok();
        let tree = parser.parse(source, None).unwrap();

        let import_map = parse_python_imports(tree.root_node(), source);

        assert_eq!(import_map.resolve("j"), Some("os.path.join"));
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

        let import_map = parse_file_imports(tree.root_node(), source, Language::Rust);

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
}
