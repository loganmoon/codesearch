//! TypeScript-specific utilities
//!
//! This module contains TypeScript-specific extraction functions for
//! type references from type annotations.

use crate::common::import_map::{resolve_reference, ImportMap};
use crate::common::node_to_text;
use codesearch_core::entities::{ReferenceType, SourceLocation, SourceReference};
use std::collections::HashSet;
use std::sync::OnceLock;
use streaming_iterator::StreamingIterator;
use tree_sitter::{Node, Query, QueryCursor};

// ============================================================================
// Cached Tree-Sitter Queries
// ============================================================================

/// Query source for extracting type references
const TS_TYPE_REFS_QUERY_SOURCE: &str = r#"
    ; Type identifiers
    (type_identifier) @type_ref

    ; Nested type identifiers (qualified types like Foo.Bar)
    (nested_type_identifier) @scoped_type_ref
"#;

/// Cached tree-sitter query for type reference extraction
static TS_TYPE_REFS_QUERY: OnceLock<Option<Query>> = OnceLock::new();

/// Get or initialize the cached type references query
fn ts_type_refs_query() -> Option<&'static Query> {
    TS_TYPE_REFS_QUERY
        .get_or_init(|| {
            let language = tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();
            match Query::new(&language, TS_TYPE_REFS_QUERY_SOURCE) {
                Ok(query) => Some(query),
                Err(e) => {
                    tracing::error!(
                        "Failed to compile TypeScript type refs query: {e}. This is a bug."
                    );
                    None
                }
            }
        })
        .as_ref()
}

/// Extract type references from TypeScript type annotations
///
/// This extracts type identifiers from:
/// - Parameter type annotations
/// - Return type annotations
/// - Variable type annotations
/// - Generic type arguments
///
/// Returns a list of `SourceReference` with resolved qualified names and locations.
pub fn extract_type_references(
    function_node: Node,
    source: &str,
    import_map: &ImportMap,
    parent_scope: Option<&str>,
) -> Vec<SourceReference> {
    let Some(query) = ts_type_refs_query() else {
        return Vec::new();
    };

    let mut cursor = QueryCursor::new();
    let mut type_refs = Vec::new();
    let mut seen = HashSet::new();

    let mut matches = cursor.matches(query, function_node, source.as_bytes());
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
                            if let Ok(source_ref) = SourceReference::builder()
                                .target(resolved)
                                .simple_name(type_name) // simple_name from AST node
                                .is_external(false) // TS doesn't track external refs
                                .location(SourceLocation::from_tree_sitter_node(capture.node))
                                .ref_type(ReferenceType::TypeUsage)
                                .build()
                            {
                                type_refs.push(source_ref);
                            }
                        }
                    }
                }
                "scoped_type_ref" => {
                    if let Ok(full_path) = node_to_text(capture.node, source) {
                        // Scoped types are already qualified
                        // Extract simple name from the last segment of the path
                        let simple_name = full_path
                            .rsplit('.')
                            .next()
                            .unwrap_or(&full_path)
                            .to_string();
                        if seen.insert(full_path.clone()) {
                            if let Ok(source_ref) = SourceReference::builder()
                                .target(full_path)
                                .simple_name(simple_name) // last segment of path
                                .is_external(false) // TS doesn't track external refs
                                .location(SourceLocation::from_tree_sitter_node(capture.node))
                                .ref_type(ReferenceType::TypeUsage)
                                .build()
                            {
                                type_refs.push(source_ref);
                            }
                        }
                    }
                }
                _ => {
                    tracing::trace!(
                        kind = capture_name,
                        "Unhandled capture name in type refs query"
                    );
                }
            }
        }
    }

    type_refs
}

/// Check if a type name is a TypeScript primitive type
pub fn is_ts_primitive(name: &str) -> bool {
    name.eq_ignore_ascii_case("string")
        || name.eq_ignore_ascii_case("number")
        || name.eq_ignore_ascii_case("boolean")
        || name.eq_ignore_ascii_case("any")
        || name.eq_ignore_ascii_case("never")
        || name.eq_ignore_ascii_case("void")
        || name.eq_ignore_ascii_case("object")
        || name.eq_ignore_ascii_case("null")
        || name.eq_ignore_ascii_case("undefined")
        || name.eq_ignore_ascii_case("symbol")
        || name.eq_ignore_ascii_case("bigint")
        || name.eq_ignore_ascii_case("unknown")
        || name.eq_ignore_ascii_case("array")
        || name.eq_ignore_ascii_case("function")
        || name.eq_ignore_ascii_case("promise")
        || name.eq_ignore_ascii_case("readonly")
        || name.eq_ignore_ascii_case("record")
        || name.eq_ignore_ascii_case("partial")
        || name.eq_ignore_ascii_case("required")
        || name.eq_ignore_ascii_case("pick")
        || name.eq_ignore_ascii_case("omit")
        || name.eq_ignore_ascii_case("exclude")
        || name.eq_ignore_ascii_case("extract")
        || name.eq_ignore_ascii_case("returntype")
        || name.eq_ignore_ascii_case("parameters")
}
