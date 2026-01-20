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
use anyhow::{bail, Result};
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

/// Type alias mappings from alias name to target type name
///
/// This stores simple name -> target mappings, e.g.:
/// - "Settings" -> "AppConfig"
/// - "AppConfig" -> "Config"
/// - "Config" -> "RawConfig"
pub type TypeAliasMap = std::collections::HashMap<String, String>;

/// Parse Rust type alias declarations to build a mapping
///
/// For each `type Alias = Target;`, extracts the alias name and target type name.
/// Only handles simple type aliases (not generic aliases) for now.
///
/// # Example
/// ```text
/// type Config = RawConfig;
/// type AppConfig = Config;
/// ```
/// Returns: {"Config" -> "RawConfig", "AppConfig" -> "Config"}
///
/// # Errors
/// Returns an error if the tree-sitter query fails to compile (indicates a bug).
pub fn parse_rust_type_aliases(root: Node, source: &str) -> Result<TypeAliasMap> {
    let mut alias_map = TypeAliasMap::new();

    let query_source = r#"
        (type_item
          name: (type_identifier) @alias_name
          type: (_) @target_type)
    "#;

    let query = Query::new(&tree_sitter_rust::LANGUAGE.into(), query_source)
        .map_err(|e| anyhow::anyhow!("Failed to compile type alias query: {e}"))?;

    let alias_idx = query.capture_index_for_name("alias_name");
    let target_idx = query.capture_index_for_name("target_type");

    let (Some(alias_idx), Some(target_idx)) = (alias_idx, target_idx) else {
        bail!("Missing capture indices in type alias query (query: alias_name={alias_idx:?}, target_type={target_idx:?})");
    };

    let source_bytes = source.as_bytes();
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, root, source_bytes);

    while let Some(m) = matches.next() {
        let mut alias_name: Option<&str> = None;
        let mut target_type: Option<&str> = None;

        for capture in m.captures {
            if capture.index == alias_idx {
                alias_name = node_text(capture.node, source_bytes, "type_alias_name");
            } else if capture.index == target_idx {
                // For the target type, extract just the simple name if it's a type identifier
                // For more complex types (generics, references), use the full text
                let target_node = capture.node;
                match target_node.kind() {
                    "type_identifier" => {
                        target_type = node_text(target_node, source_bytes, "type_alias_target");
                    }
                    "scoped_type_identifier" => {
                        // For scoped types like `mod::Type`, use the full path
                        target_type = node_text(target_node, source_bytes, "type_alias_target");
                    }
                    _ => {
                        // For generic types, references, etc., skip for now
                        // These require more complex handling
                        debug!(
                            target_kind = target_node.kind(),
                            "Skipping complex type alias target"
                        );
                    }
                }
            }
        }

        if let (Some(alias), Some(target)) = (alias_name, target_type) {
            debug!(alias = alias, target = target, "Parsed type alias");
            alias_map.insert(alias.to_string(), target.to_string());
        }
    }

    Ok(alias_map)
}

/// Resolve a type name through the alias chain to find the canonical type
///
/// Follows the chain of type aliases until no more aliases are found.
/// Returns the original name if it's not an alias.
///
/// # Example
/// Given aliases: {"Settings" -> "AppConfig", "AppConfig" -> "Config", "Config" -> "RawConfig"}
/// - resolve_type_alias_chain("Settings", &map) -> "RawConfig"
/// - resolve_type_alias_chain("RawConfig", &map) -> "RawConfig"
///
/// Handles cycles defensively with a seen set to prevent infinite loops.
pub fn resolve_type_alias_chain(type_name: &str, alias_map: &TypeAliasMap) -> String {
    let mut current = type_name.to_string();
    let mut seen = std::collections::HashSet::new();

    while let Some(target) = alias_map.get(&current) {
        if !seen.insert(current.clone()) {
            // Cycle detected, return current value
            debug!(
                type_name = type_name,
                current = current,
                "Cycle detected in type alias chain"
            );
            break;
        }
        current = target.clone();
    }

    current
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

    // ========================================================================
    // Tests for parse_rust_type_aliases
    // ========================================================================

    #[test]
    fn test_parse_type_aliases() {
        let source = r#"
type Config = RawConfig;
type AppConfig = Config;
type Settings = AppConfig;
"#;
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&tree_sitter_rust::LANGUAGE.into()).ok();
        let tree = parser.parse(source, None).unwrap();

        let alias_map = parse_rust_type_aliases(tree.root_node(), source).unwrap();

        assert_eq!(alias_map.get("Config"), Some(&"RawConfig".to_string()));
        assert_eq!(alias_map.get("AppConfig"), Some(&"Config".to_string()));
        assert_eq!(alias_map.get("Settings"), Some(&"AppConfig".to_string()));
        assert_eq!(alias_map.get("RawConfig"), None);
    }

    #[test]
    fn test_resolve_type_alias_chain() {
        let mut alias_map = TypeAliasMap::new();
        alias_map.insert("Settings".to_string(), "AppConfig".to_string());
        alias_map.insert("AppConfig".to_string(), "Config".to_string());
        alias_map.insert("Config".to_string(), "RawConfig".to_string());

        assert_eq!(
            resolve_type_alias_chain("Settings", &alias_map),
            "RawConfig"
        );
        assert_eq!(
            resolve_type_alias_chain("AppConfig", &alias_map),
            "RawConfig"
        );
        assert_eq!(resolve_type_alias_chain("Config", &alias_map), "RawConfig");
        assert_eq!(
            resolve_type_alias_chain("RawConfig", &alias_map),
            "RawConfig"
        );
        assert_eq!(
            resolve_type_alias_chain("UnknownType", &alias_map),
            "UnknownType"
        );
    }

    #[test]
    fn test_resolve_type_alias_chain_cycle_detection() {
        let mut alias_map = TypeAliasMap::new();
        alias_map.insert("A".to_string(), "B".to_string());
        alias_map.insert("B".to_string(), "C".to_string());
        alias_map.insert("C".to_string(), "A".to_string()); // Creates a cycle

        // Should not hang, should return some value in the cycle
        let result = resolve_type_alias_chain("A", &alias_map);
        // The exact result depends on cycle detection order, but it should be one of A, B, or C
        assert!(result == "A" || result == "B" || result == "C");
    }
}
