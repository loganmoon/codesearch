//! TypeScript-specific utilities
//!
//! This module contains TypeScript-specific extraction functions for
//! type references from type annotations.

use crate::common::import_map::{resolve_reference, ImportMap};
use crate::common::node_to_text;
use std::collections::HashSet;
use streaming_iterator::StreamingIterator;
use tree_sitter::{Node, Query, QueryCursor};

/// Extract type references from TypeScript type annotations
///
/// This extracts type identifiers from:
/// - Parameter type annotations
/// - Return type annotations
/// - Variable type annotations
/// - Generic type arguments
///
/// Returns a list of resolved qualified names.
pub fn extract_type_references(
    function_node: Node,
    source: &str,
    import_map: &ImportMap,
    parent_scope: Option<&str>,
) -> Vec<String> {
    let query_source = r#"
        ; Type identifiers
        (type_identifier) @type_ref

        ; Nested type identifiers (qualified types like Foo.Bar)
        (nested_type_identifier) @scoped_type_ref
    "#;

    let language = tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();
    let query = match Query::new(&language, query_source) {
        Ok(q) => q,
        Err(_) => return Vec::new(),
    };

    let mut cursor = QueryCursor::new();
    let mut type_refs = Vec::new();
    let mut seen = HashSet::new();

    let mut matches = cursor.matches(&query, function_node, source.as_bytes());
    while let Some(query_match) = matches.next() {
        for capture in query_match.captures {
            let capture_name = query
                .capture_names()
                .get(capture.index as usize)
                .copied()
                .unwrap_or("");

            match capture_name {
                "type_ref" => {
                    if let Ok(type_name) = node_to_text(capture.node, source) {
                        // Skip TypeScript primitive types
                        if is_ts_primitive(&type_name) {
                            continue;
                        }
                        // Resolve through imports
                        let resolved = resolve_reference(&type_name, import_map, parent_scope, ".");
                        if seen.insert(resolved.clone()) {
                            type_refs.push(resolved);
                        }
                    }
                }
                "scoped_type_ref" => {
                    if let Ok(full_path) = node_to_text(capture.node, source) {
                        // Scoped types are already qualified
                        if seen.insert(full_path.clone()) {
                            type_refs.push(full_path);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    type_refs
}

/// Check if a type name is a TypeScript primitive type
pub fn is_ts_primitive(name: &str) -> bool {
    matches!(
        name.to_lowercase().as_str(),
        "string"
            | "number"
            | "boolean"
            | "any"
            | "never"
            | "void"
            | "object"
            | "null"
            | "undefined"
            | "symbol"
            | "bigint"
            | "unknown"
            | "array"
            | "function"
            | "promise"
            | "readonly"
            | "record"
            | "partial"
            | "required"
            | "pick"
            | "omit"
            | "exclude"
            | "extract"
            | "returntype"
            | "parameters"
    )
}
