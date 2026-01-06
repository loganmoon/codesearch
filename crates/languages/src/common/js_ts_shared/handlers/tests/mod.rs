//! Test suite for JavaScript and TypeScript entity handlers
//!
//! These tests verify that:
//! - JavaScript handlers produce entities with `Language::JavaScript`
//! - TypeScript handlers produce entities with `Language::TypeScript`

#![allow(clippy::expect_used)]

use crate::common::entity_building::ExtractionContext;
use crate::common::js_ts_shared::queries;
use codesearch_core::entities::Language;
use codesearch_core::error::{Error, Result};
use codesearch_core::CodeEntity;
use std::path::Path;
use streaming_iterator::StreamingIterator;
use tree_sitter::{Parser, Query, QueryCursor};

fn parse_error(file: &str, message: impl Into<String>) -> Error {
    Error::Parse {
        file: file.to_string(),
        message: message.into(),
    }
}

/// Extract entities using a JavaScript handler
fn extract_js_entities<F>(source: &str, query_str: &str, handler: F) -> Result<Vec<CodeEntity>>
where
    F: Fn(&ExtractionContext) -> Result<Vec<CodeEntity>>,
{
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_javascript::LANGUAGE.into())
        .map_err(|e| parse_error("test.js", e.to_string()))?;

    let tree = parser
        .parse(source, None)
        .ok_or_else(|| parse_error("test.js", "Failed to parse"))?;
    let query = Query::new(&tree_sitter_javascript::LANGUAGE.into(), query_str)
        .map_err(|e| parse_error("test.js", e.to_string()))?;

    let mut cursor = QueryCursor::new();
    let mut matches_iter = cursor.matches(&query, tree.root_node(), source.as_bytes());

    let path = Path::new("test.js");
    let repository_id = "test-repo-id";
    let repo_root = Path::new("/test-repo");

    let mut all_entities = Vec::new();
    while let Some(query_match) = matches_iter.next() {
        let ctx = ExtractionContext {
            query_match,
            query: &query,
            source,
            file_path: path,
            repository_id,
            package_name: None,
            source_root: None,
            repo_root,
        };
        let entities = handler(&ctx)?;
        all_entities.extend(entities);
    }

    Ok(all_entities)
}

/// Extract entities using a TypeScript handler
fn extract_ts_entities<F>(source: &str, query_str: &str, handler: F) -> Result<Vec<CodeEntity>>
where
    F: Fn(&ExtractionContext) -> Result<Vec<CodeEntity>>,
{
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
        .map_err(|e| parse_error("test.ts", e.to_string()))?;

    let tree = parser
        .parse(source, None)
        .ok_or_else(|| parse_error("test.ts", "Failed to parse"))?;
    let query = Query::new(
        &tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        query_str,
    )
    .map_err(|e| parse_error("test.ts", e.to_string()))?;

    let mut cursor = QueryCursor::new();
    let mut matches_iter = cursor.matches(&query, tree.root_node(), source.as_bytes());

    let path = Path::new("test.ts");
    let repository_id = "test-repo-id";
    let repo_root = Path::new("/test-repo");

    let mut all_entities = Vec::new();
    while let Some(query_match) = matches_iter.next() {
        let ctx = ExtractionContext {
            query_match,
            query: &query,
            source,
            file_path: path,
            repository_id,
            package_name: None,
            source_root: None,
            repo_root,
        };
        let entities = handler(&ctx)?;
        all_entities.extend(entities);
    }

    Ok(all_entities)
}

mod language_labeling_tests {
    use super::*;
    use crate::common::js_ts_shared::handlers;

    #[test]
    fn test_js_function_has_javascript_language() {
        let source = "function hello() {}";
        let entities = extract_js_entities(
            source,
            queries::FUNCTION_DECLARATION_QUERY,
            handlers::handle_function_declaration_impl,
        )
        .expect("extraction should succeed");

        assert_eq!(entities.len(), 1);
        assert_eq!(
            entities[0].language,
            Language::JavaScript,
            "JavaScript function handler should produce Language::JavaScript"
        );
    }

    #[test]
    fn test_ts_function_has_typescript_language() {
        let source = "function hello(): void {}";
        let entities = extract_ts_entities(
            source,
            queries::FUNCTION_DECLARATION_QUERY,
            handlers::handle_ts_function_declaration_impl,
        )
        .expect("extraction should succeed");

        assert_eq!(entities.len(), 1);
        assert_eq!(
            entities[0].language,
            Language::TypeScript,
            "TypeScript function handler should produce Language::TypeScript"
        );
    }

    #[test]
    fn test_js_arrow_function_has_javascript_language() {
        let source = "const hello = () => {};";
        let entities = extract_js_entities(
            source,
            queries::ARROW_FUNCTION_QUERY,
            handlers::handle_arrow_function_impl,
        )
        .expect("extraction should succeed");

        assert_eq!(entities.len(), 1);
        assert_eq!(
            entities[0].language,
            Language::JavaScript,
            "JavaScript arrow function handler should produce Language::JavaScript"
        );
    }

    #[test]
    fn test_ts_arrow_function_has_typescript_language() {
        let source = "const hello = (): void => {};";
        let entities = extract_ts_entities(
            source,
            queries::ARROW_FUNCTION_QUERY,
            handlers::handle_ts_arrow_function_impl,
        )
        .expect("extraction should succeed");

        assert_eq!(entities.len(), 1);
        assert_eq!(
            entities[0].language,
            Language::TypeScript,
            "TypeScript arrow function handler should produce Language::TypeScript"
        );
    }

    #[test]
    fn test_js_const_has_javascript_language() {
        let source = "const x = 42;";
        let entities =
            extract_js_entities(source, queries::CONST_QUERY, handlers::handle_const_impl)
                .expect("extraction should succeed");

        assert_eq!(entities.len(), 1);
        assert_eq!(
            entities[0].language,
            Language::JavaScript,
            "JavaScript const handler should produce Language::JavaScript"
        );
    }

    #[test]
    fn test_ts_const_has_typescript_language() {
        let source = "const x: number = 42;";
        let entities =
            extract_ts_entities(source, queries::CONST_QUERY, handlers::handle_ts_const_impl)
                .expect("extraction should succeed");

        assert_eq!(entities.len(), 1);
        assert_eq!(
            entities[0].language,
            Language::TypeScript,
            "TypeScript const handler should produce Language::TypeScript"
        );
    }

    #[test]
    fn test_js_let_has_javascript_language() {
        let source = "let x = 42;";
        let entities = extract_js_entities(source, queries::LET_QUERY, handlers::handle_let_impl)
            .expect("extraction should succeed");

        assert_eq!(entities.len(), 1);
        assert_eq!(
            entities[0].language,
            Language::JavaScript,
            "JavaScript let handler should produce Language::JavaScript"
        );
    }

    #[test]
    fn test_ts_let_has_typescript_language() {
        let source = "let x: number = 42;";
        let entities =
            extract_ts_entities(source, queries::LET_QUERY, handlers::handle_ts_let_impl)
                .expect("extraction should succeed");

        assert_eq!(entities.len(), 1);
        assert_eq!(
            entities[0].language,
            Language::TypeScript,
            "TypeScript let handler should produce Language::TypeScript"
        );
    }

    #[test]
    fn test_ts_interface_has_typescript_language() {
        let source = "interface Person { name: string; }";
        let entities = extract_ts_entities(
            source,
            queries::INTERFACE_QUERY,
            handlers::handle_interface_impl,
        )
        .expect("extraction should succeed");

        assert_eq!(entities.len(), 1);
        assert_eq!(
            entities[0].language,
            Language::TypeScript,
            "TypeScript interface handler should produce Language::TypeScript"
        );
    }

    #[test]
    fn test_ts_enum_has_typescript_language() {
        let source = "enum Color { Red, Green, Blue }";
        let entities = extract_ts_entities(source, queries::ENUM_QUERY, handlers::handle_enum_impl)
            .expect("extraction should succeed");

        assert_eq!(entities.len(), 1);
        assert_eq!(
            entities[0].language,
            Language::TypeScript,
            "TypeScript enum handler should produce Language::TypeScript"
        );
    }

    #[test]
    fn test_ts_function_expression_exported() {
        let source = r#"export const onClick = function handleClick(event: Event): void {
    console.log("Clicked", event);
};"#;
        let entities = extract_ts_entities(
            source,
            queries::FUNCTION_EXPRESSION_QUERY,
            handlers::handle_ts_function_expression_impl,
        )
        .expect("extraction should succeed");

        eprintln!("Found {} entities:", entities.len());
        for e in &entities {
            eprintln!("  - {} ({})", e.name, e.qualified_name);
        }

        assert_eq!(entities.len(), 1, "Should find 1 function expression");
        assert_eq!(
            entities[0].name, "handleClick",
            "Should use function's own name"
        );
    }

    #[test]
    fn test_ts_function_expression_anonymous() {
        let source = r#"export const onHover = function(event: Event): void {
    console.log("Hovered", event);
};"#;
        let entities = extract_ts_entities(
            source,
            queries::FUNCTION_EXPRESSION_QUERY,
            handlers::handle_ts_function_expression_impl,
        )
        .expect("extraction should succeed");

        assert_eq!(entities.len(), 1, "Should find 1 function expression");
        assert_eq!(
            entities[0].name, "onHover",
            "Should use variable name for anonymous function"
        );
    }

    #[test]
    fn test_ts_function_expression_iife() {
        let source = r#"const result = (function initialize(): number {
    return 42;
})();"#;
        let entities = extract_ts_entities(
            source,
            queries::FUNCTION_EXPRESSION_QUERY,
            handlers::handle_ts_function_expression_impl,
        )
        .expect("extraction should succeed");

        assert_eq!(entities.len(), 1, "Should find 1 IIFE");
        assert_eq!(entities[0].name, "initialize", "Should use function's name");
    }

    #[test]
    fn test_interface_extends_extraction() {
        let source = r#"export interface Auditable extends Entity, Timestamped {
    createdBy: string;
}"#;

        // Debug: Print the query and check captures
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
            .expect("failed to set language");
        let tree = parser.parse(source, None).expect("failed to parse");
        let query = Query::new(
            &tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            queries::INTERFACE_QUERY,
        )
        .expect("failed to create query");

        eprintln!("\nQuery capture names:");
        for i in 0..query.capture_names().len() {
            eprintln!("  [{i}] {}", query.capture_names()[i]);
        }

        let mut cursor = QueryCursor::new();
        let mut matches_iter = cursor.matches(&query, tree.root_node(), source.as_bytes());

        eprintln!("\nQuery matches:");
        while let Some(query_match) = matches_iter.next() {
            eprintln!("  Match pattern: {}", query_match.pattern_index);
            for capture in query_match.captures {
                let name = query.capture_names()[capture.index as usize];
                let text = capture
                    .node
                    .utf8_text(source.as_bytes())
                    .unwrap_or("<error>");
                let truncated = if text.len() > 50 {
                    format!("{}...", &text[..50])
                } else {
                    text.to_string()
                };
                eprintln!("    @{name} (idx={}) = '{truncated}'", capture.index);
            }
        }

        let entities = extract_ts_entities(
            source,
            queries::INTERFACE_QUERY,
            handlers::handle_interface_impl,
        )
        .expect("extraction should succeed");

        eprintln!("\nFound {} entities:", entities.len());
        for e in &entities {
            eprintln!("  - {} ({})", e.name, e.qualified_name);
            eprintln!("    Extended types: {:?}", e.relationships.extended_types);
        }

        // Filter to unique interfaces (there may be duplicate matches)
        assert!(!entities.is_empty(), "Should find at least 1 interface");
        let interface = &entities[0];

        // Should have 2 supertraits: Entity and Timestamped
        assert_eq!(
            interface.relationships.extended_types.len(),
            2,
            "Should have 2 extended interfaces"
        );
    }
}
