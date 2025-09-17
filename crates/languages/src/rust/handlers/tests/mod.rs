//! Comprehensive test suite for Rust extraction handlers

#![cfg(test)]

mod edge_cases;
mod enum_tests;
mod fixtures;
mod function_tests;
mod struct_tests;
mod trait_tests;

use crate::rust::queries;
use crate::transport::EntityData;
use codesearch_core::error::Result;
use std::path::Path;
use streaming_iterator::StreamingIterator;
use tree_sitter::{Parser, Query, QueryCursor};

/// Helper to extract entities from source code using a handler
fn extract_with_handler<F>(source: &str, query_str: &str, handler: F) -> Result<Vec<EntityData>>
where
    F: Fn(&tree_sitter::QueryMatch, &Query, &str, &Path) -> Result<Vec<EntityData>>,
{
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .unwrap();

    let tree = parser.parse(source, None).unwrap();
    let query = Query::new(&tree_sitter_rust::LANGUAGE.into(), query_str).unwrap();

    let mut cursor = QueryCursor::new();
    let mut matches_iter = cursor.matches(&query, tree.root_node(), source.as_bytes());

    let path = Path::new("test.rs");

    let mut all_entities = Vec::new();
    while let Some(query_match) = matches_iter.next() {
        if let Ok(entities) = handler(query_match, &query, source, path) {
            all_entities.extend(entities);
        }
    }

    Ok(all_entities)
}
