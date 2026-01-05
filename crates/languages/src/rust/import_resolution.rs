//! Rust import parsing
//!
//! This module provides Rust-specific logic for parsing use declarations.
//! Reference resolution has been moved to the generic `reference_resolution` module.

#![deny(warnings)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]

use crate::common::import_map::ImportMap;
use crate::common::language_path::LanguagePath;
use crate::common::path_config::RUST_PATH_CONFIG;
use streaming_iterator::StreamingIterator;
use tracing::{debug, error};
use tree_sitter::{Node, Query, QueryCursor};

/// Extract UTF-8 text from a tree-sitter node, logging on failure
fn node_text<'a>(node: Node<'a>, source: &'a [u8], context: &str) -> Option<&'a str> {
    match node.utf8_text(source) {
        Ok(text) => Some(text),
        Err(e) => {
            debug!(
                error = %e,
                node_kind = node.kind(),
                start = node.start_position().row,
                context = context,
                "Failed to extract UTF-8 text from node"
            );
            None
        }
    }
}

/// Parse Rust use declarations
///
/// Handles:
/// - `use std::io::Read;` -> ("Read", "std::io::Read")
/// - `use std::io::{Read, Write};` -> [("Read", "std::io::Read"), ("Write", "std::io::Write")]
/// - `use std::io::Read as MyRead;` -> ("MyRead", "std::io::Read")
/// - `use helpers::*;` -> stores "helpers" as glob import for fallback resolution
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

        (use_declaration
          argument: (use_wildcard
            (scoped_identifier) @wildcard_scope))

        (use_declaration
          argument: (use_wildcard
            (identifier) @wildcard_simple))
    "#;

    let language = tree_sitter_rust::LANGUAGE.into();
    let query = match Query::new(&language, query_source) {
        Ok(q) => q,
        Err(e) => {
            error!(error = %e, "Failed to compile Rust import parsing query");
            return import_map;
        }
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
                    let path_cap = query_match
                        .captures
                        .iter()
                        .find(|c| {
                            query.capture_names().get(c.index as usize).copied() == Some("path")
                        })
                        .map(|c| c.node);

                    if let (Some(path_node), Some(alias_text)) = (
                        path_cap,
                        node_text(capture.node, source.as_bytes(), "use alias"),
                    ) {
                        if let Some(path_text) =
                            node_text(path_node, source.as_bytes(), "use alias path")
                        {
                            import_map.add(alias_text, path_text);
                        }
                    }
                }
                "scoped_path" => {
                    // use std::io::Read - extract the last segment as simple name
                    if let Some(full_path) =
                        node_text(capture.node, source.as_bytes(), "scoped import path")
                    {
                        let parsed = LanguagePath::parse(full_path, &RUST_PATH_CONFIG);
                        if let Some(simple_name) = parsed.simple_name() {
                            if simple_name == "*" {
                                // Store glob import base path for fallback resolution
                                // The base path is all segments except the last (*)
                                let segments = parsed.segments();
                                if segments.len() > 1 {
                                    let base = LanguagePath::builder(&RUST_PATH_CONFIG)
                                        .segments(segments[..segments.len() - 1].iter().cloned())
                                        .build();
                                    import_map.add_glob(&base.to_qualified_name());
                                }
                            } else {
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
                        if let Some(base_path) =
                            node_text(base_path_cap.node, source.as_bytes(), "use list base path")
                        {
                            parse_rust_use_list(capture.node, source, base_path, &mut import_map);
                        }
                    }
                }
                "simple_import" => {
                    // use identifier - rare but valid
                    if let Some(name) = node_text(capture.node, source.as_bytes(), "simple import")
                    {
                        import_map.add(name, name);
                    }
                }
                "wildcard_scope" => {
                    // use helpers::* - scoped wildcard import
                    if let Some(base_path) =
                        node_text(capture.node, source.as_bytes(), "wildcard scope")
                    {
                        import_map.add_glob(base_path);
                    }
                }
                "wildcard_simple" => {
                    // use ident::* - simple identifier wildcard
                    if let Some(base_path) =
                        node_text(capture.node, source.as_bytes(), "wildcard simple")
                    {
                        import_map.add_glob(base_path);
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
    let base_path_parsed = LanguagePath::parse(base_path, &RUST_PATH_CONFIG);
    let mut cursor = list_node.walk();

    for child in list_node.children(&mut cursor) {
        match child.kind() {
            "identifier" => {
                if let Some(name) = node_text(child, source.as_bytes(), "use list identifier") {
                    let full_path = LanguagePath::builder(&RUST_PATH_CONFIG)
                        .segments(base_path_parsed.segments().iter().cloned())
                        .segment(name)
                        .build();
                    import_map.add(name, &full_path.to_qualified_name());
                }
            }
            "use_as_clause" => {
                // Handle `Read as R` or `tcp::connect as tcp_connect` within a use list
                // use_as_clause has named fields: path (identifier or scoped_identifier) and alias (identifier)
                let path_node = child.child_by_field_name("path");
                let alias_node = child.child_by_field_name("alias");

                if let (Some(p_node), Some(a_node)) = (path_node, alias_node) {
                    if let (Some(path_text), Some(alias_text)) = (
                        node_text(p_node, source.as_bytes(), "use_as_clause path"),
                        node_text(a_node, source.as_bytes(), "use_as_clause alias"),
                    ) {
                        let path_parsed = LanguagePath::parse(path_text, &RUST_PATH_CONFIG);
                        let full_path = LanguagePath::builder(&RUST_PATH_CONFIG)
                            .segments(base_path_parsed.segments().iter().cloned())
                            .segments(path_parsed.segments().iter().cloned())
                            .build();
                        import_map.add(alias_text, &full_path.to_qualified_name());
                    }
                }
            }
            "scoped_identifier" => {
                // Handle nested paths like `io::Read` within a use list
                if let Some(scoped_path) =
                    node_text(child, source.as_bytes(), "use list scoped_identifier")
                {
                    let scoped_parsed = LanguagePath::parse(scoped_path, &RUST_PATH_CONFIG);
                    let full_path = LanguagePath::builder(&RUST_PATH_CONFIG)
                        .segments(base_path_parsed.segments().iter().cloned())
                        .segments(scoped_parsed.segments().iter().cloned())
                        .build();
                    if let Some(simple_name) = scoped_parsed.simple_name() {
                        import_map.add(simple_name, &full_path.to_qualified_name());
                    }
                }
            }
            "self" => {
                // Handle `use foo::{self}` - imports the base path itself
                if let Some(simple_name) = base_path_parsed.simple_name() {
                    import_map.add(simple_name, base_path);
                }
            }
            "scoped_use_list" => {
                // Handle nested use groups like `http::{get as http_get, post}`
                // The child has structure: path: (identifier/scoped_identifier), list: (use_list)
                if let (Some(path_node), Some(nested_list_node)) = (
                    child.child_by_field_name("path"),
                    child.child_by_field_name("list"),
                ) {
                    if let Some(path_text) =
                        node_text(path_node, source.as_bytes(), "scoped_use_list path")
                    {
                        let path_parsed = LanguagePath::parse(path_text, &RUST_PATH_CONFIG);
                        let nested_base = LanguagePath::builder(&RUST_PATH_CONFIG)
                            .segments(base_path_parsed.segments().iter().cloned())
                            .segments(path_parsed.segments().iter().cloned())
                            .build();
                        parse_rust_use_list(
                            nested_list_node,
                            source,
                            &nested_base.to_qualified_name(),
                            import_map,
                        );
                    }
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    // ========================================================================
    // Tests for parse_rust_imports
    // ========================================================================

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
    fn test_parse_rust_glob_import() {
        let source = "use helpers::*;";
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&tree_sitter_rust::LANGUAGE.into()).ok();
        let tree = parser.parse(source, None).unwrap();

        let import_map = parse_rust_imports(tree.root_node(), source);

        assert!(!import_map.glob_imports().is_empty());
        assert_eq!(import_map.glob_imports()[0], "helpers");
    }

    #[test]
    fn test_parse_rust_nested_glob_import() {
        let source = "use std::collections::*;";
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&tree_sitter_rust::LANGUAGE.into()).ok();
        let tree = parser.parse(source, None).unwrap();

        let import_map = parse_rust_imports(tree.root_node(), source);

        assert!(!import_map.glob_imports().is_empty());
        assert_eq!(import_map.glob_imports()[0], "std::collections");
    }

    #[test]
    fn test_parse_rust_nested_use_with_renaming() {
        let source = r#"
use network::{
    http::{get as http_get, post as http_post},
    tcp::connect as tcp_connect,
};
"#;
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&tree_sitter_rust::LANGUAGE.into()).ok();
        let tree = parser.parse(source, None).unwrap();

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
}
