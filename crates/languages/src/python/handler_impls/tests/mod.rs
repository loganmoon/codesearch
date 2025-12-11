//! Test suite for Python extraction handlers

mod class_tests;
mod function_tests;

use crate::python::queries;
use codesearch_core::{error::Result, CodeEntity};
use std::path::Path;
use streaming_iterator::StreamingIterator;
use tree_sitter::{Parser, Query, QueryCursor};

/// Helper to extract entities from source code using a handler
pub fn extract_with_handler<F>(source: &str, query_str: &str, handler: F) -> Result<Vec<CodeEntity>>
where
    F: Fn(
        &tree_sitter::QueryMatch,
        &Query,
        &str,
        &Path,
        &str,
        Option<&str>,
        Option<&Path>,
    ) -> Result<Vec<CodeEntity>>,
{
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_python::LANGUAGE.into())
        .unwrap();

    let tree = parser.parse(source, None).unwrap();
    let query = Query::new(&tree_sitter_python::LANGUAGE.into(), query_str).unwrap();

    let mut cursor = QueryCursor::new();
    let mut matches_iter = cursor.matches(&query, tree.root_node(), source.as_bytes());

    let path = Path::new("test.py");
    let repository_id = "test-repo-id";

    let mut all_entities = Vec::new();
    while let Some(query_match) = matches_iter.next() {
        let entities = handler(query_match, &query, source, path, repository_id, None, None)
            .expect("Handler should not fail during test extraction");
        all_entities.extend(entities);
    }

    Ok(all_entities)
}
