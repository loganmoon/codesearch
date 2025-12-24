//! Test suite for TypeScript extraction handlers

mod utils_tests;

use codesearch_core::{error::Result, CodeEntity};
use std::path::Path;
use streaming_iterator::StreamingIterator;
use tree_sitter::{Parser, Query, QueryCursor};

/// Helper to extract entities from TypeScript source code using a handler
#[allow(dead_code)]
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
        &Path,
    ) -> Result<Vec<CodeEntity>>,
{
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
        .unwrap();

    let tree = parser.parse(source, None).unwrap();
    let query = Query::new(
        &tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        query_str,
    )
    .unwrap();

    let mut cursor = QueryCursor::new();
    let mut matches_iter = cursor.matches(&query, tree.root_node(), source.as_bytes());

    let path = Path::new("test.ts");
    let repository_id = "test-repo-id";
    let repo_root = Path::new("/test-repo");

    let mut all_entities = Vec::new();
    while let Some(query_match) = matches_iter.next() {
        let entities = handler(
            query_match,
            &query,
            source,
            path,
            repository_id,
            None,
            None,
            repo_root,
        )
        .expect("Handler should not fail during test extraction");
        all_entities.extend(entities);
    }

    Ok(all_entities)
}
