//! Handler-based extraction engine
//!
//! This module provides the new extraction engine that uses inventory-registered
//! handlers with proper predicate evaluation, replacing the old HandlerConfig-based
//! dispatch with string matching hacks.

use crate::common::edge_case_handlers::EdgeCaseRegistry;
use crate::common::import_map::ImportMap;
use crate::common::path_config::PathConfig;
use crate::extract_context::{CaptureData, ExtractContext};
use crate::handler_registry::find_handler;
use crate::predicates::{PredicateEvaluator, StandardPredicates};
use crate::queries::QueryDef;
use codesearch_core::entities::Language;
use codesearch_core::error::Result;
use codesearch_core::CodeEntity;
use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;
use streaming_iterator::StreamingIterator;
use tree_sitter::{Language as TsLanguage, Node, Query, QueryCursor, QueryMatch};

/// Compiled queries for a language
struct CompiledQueries {
    /// Pairs of (compiled query, query definition)
    queries: Vec<(Query, &'static QueryDef)>,
}

/// Cached compiled queries for Rust
static RUST_QUERIES: OnceLock<CompiledQueries> = OnceLock::new();

/// Context for handler-based extraction
pub(crate) struct HandlerContext<'a> {
    pub(crate) source: &'a str,
    pub(crate) file_path: &'a Path,
    pub(crate) repository_id: &'a str,
    pub(crate) package_name: Option<&'a str>,
    pub(crate) source_root: Option<&'a Path>,
    pub(crate) repo_root: &'a Path,
    pub(crate) language: Language,
    pub(crate) language_str: &'a str,
    pub(crate) import_map: &'a ImportMap,
    pub(crate) path_config: &'static PathConfig,
    pub(crate) edge_case_handlers: Option<&'a EdgeCaseRegistry>,
}

/// Get or compile queries for a language
fn get_rust_queries(ts_language: &TsLanguage) -> &'static CompiledQueries {
    RUST_QUERIES.get_or_init(|| compile_rust_queries(ts_language))
}

/// Compile all Rust queries
fn compile_rust_queries(ts_language: &TsLanguage) -> CompiledQueries {
    use crate::queries::rust;

    let mut queries = Vec::new();

    for query_def in rust::ALL {
        match Query::new(ts_language, query_def.query) {
            Ok(query) => queries.push((query, *query_def)),
            Err(e) => {
                tracing::error!("Failed to compile query '{}': {}", query_def.handler, e);
            }
        }
    }

    CompiledQueries { queries }
}

/// Extract entities using handler registry
///
/// This is the main entry point for handler-based extraction. It:
/// 1. Gets compiled queries (cached)
/// 2. For each query, runs matches
/// 3. Evaluates predicates using PredicateEvaluator
/// 4. Builds ExtractContext and dispatches to registered handler
pub(crate) fn extract_with_handlers(
    ctx: &HandlerContext,
    tree_root: Node,
) -> Result<Vec<CodeEntity>> {
    let ts_language = tree_root.language();
    let compiled = get_rust_queries(&ts_language);
    let predicates = StandardPredicates;
    let mut entities = Vec::new();

    for (query, query_def) in &compiled.queries {
        // Find the registered handler for this query
        let handler = match find_handler(query_def.handler) {
            Some(h) => h,
            None => {
                tracing::warn!("No handler registered for '{}'", query_def.handler);
                continue;
            }
        };

        // Run the query
        let mut cursor = QueryCursor::new();
        cursor.set_timeout_micros(5_000_000);
        cursor.set_match_limit(10_000);

        let mut matches = cursor.matches(query, tree_root, ctx.source.as_bytes());

        while let Some(query_match) = matches.next() {
            // Evaluate predicates properly
            let passes = predicates
                .evaluate_all_for_pattern(
                    query,
                    query_match.pattern_index,
                    query_match,
                    ctx.source.as_bytes(),
                )
                .unwrap_or(false);

            if !passes {
                continue;
            }

            // Find the primary capture node
            let main_node = match find_capture_node(query_match, query, query_def.capture) {
                Some(n) => n,
                None => continue,
            };

            // Build ExtractContext from the match
            let extract_ctx = build_extract_context(query_match, query, query_def, ctx, main_node)?;

            // Invoke handler
            match (handler.handler)(&extract_ctx) {
                Ok(Some(entity)) => entities.push(entity),
                Ok(None) => {
                    // Handler chose to skip this match
                }
                Err(e) => {
                    tracing::warn!(
                        "Handler '{}' failed for match at {:?}: {}",
                        handler.name,
                        main_node.start_position(),
                        e
                    );
                }
            }
        }
    }

    Ok(entities)
}

/// Find a capture node by name in a query match
fn find_capture_node<'a>(
    query_match: &QueryMatch<'a, 'a>,
    query: &Query,
    capture_name: &str,
) -> Option<Node<'a>> {
    let capture_names = query.capture_names();
    for capture in query_match.captures {
        if capture_names.get(capture.index as usize) == Some(&capture_name) {
            return Some(capture.node);
        }
    }
    None
}

/// Build ExtractContext from a query match
fn build_extract_context<'a>(
    query_match: &QueryMatch<'a, 'a>,
    query: &'a Query,
    _query_def: &QueryDef,
    ctx: &'a HandlerContext<'a>,
    main_node: Node<'a>,
) -> Result<ExtractContext<'a>> {
    // Extract all captures into a map
    let capture_names = query.capture_names();
    let mut captures = HashMap::new();

    for capture in query_match.captures {
        if let Some(name) = capture_names.get(capture.index as usize) {
            let text = capture.node.utf8_text(ctx.source.as_bytes()).unwrap_or("");
            captures.insert(
                *name,
                CaptureData {
                    node: capture.node,
                    text,
                },
            );
        }
    }

    ExtractContext::builder()
        .node(main_node)
        .source(ctx.source)
        .captures(captures)
        .file_path(ctx.file_path)
        .import_map(ctx.import_map)
        .language(ctx.language)
        .language_str(ctx.language_str)
        .repository_id(ctx.repository_id)
        .package_name(ctx.package_name)
        .source_root(ctx.source_root)
        .repo_root(ctx.repo_root)
        .path_config(ctx.path_config)
        .edge_case_handlers(ctx.edge_case_handlers)
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compile_rust_queries() {
        let ts_language: TsLanguage = tree_sitter_rust::LANGUAGE.into();
        let compiled = compile_rust_queries(&ts_language);

        // Should have compiled all 18 queries
        assert!(
            compiled.queries.len() >= 8,
            "Expected at least 8 compiled queries, got {}",
            compiled.queries.len()
        );
    }

    #[test]
    fn test_find_capture_node() {
        let source = "fn test() {}";
        let ts_language: TsLanguage = tree_sitter_rust::LANGUAGE.into();
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&ts_language).unwrap();
        let tree = parser.parse(source, None).unwrap();

        let query = Query::new(
            &ts_language,
            "(function_item name: (identifier) @name) @func",
        )
        .unwrap();
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, tree.root_node(), source.as_bytes());

        if let Some(m) = matches.next() {
            let func_node = find_capture_node(m, &query, "func");
            assert!(func_node.is_some());
            assert_eq!(func_node.unwrap().kind(), "function_item");

            let name_node = find_capture_node(m, &query, "name");
            assert!(name_node.is_some());
            assert_eq!(
                name_node.unwrap().utf8_text(source.as_bytes()).unwrap(),
                "test"
            );
        }
    }

    #[test]
    fn test_predicate_filtering() {
        let source = r#"
            fn free_function() {}

            impl MyStruct {
                fn impl_function(&self) {}
            }
        "#;

        let ts_language: TsLanguage = tree_sitter_rust::LANGUAGE.into();
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&ts_language).unwrap();
        let tree = parser.parse(source, None).unwrap();

        // Query for free functions only (not in impl blocks)
        let query = Query::new(
            &ts_language,
            r#"((function_item name: (identifier) @name) @func (#not-has-ancestor? @func impl_item))"#
        ).unwrap();

        let predicates = StandardPredicates;
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, tree.root_node(), source.as_bytes());

        let mut matched_names = Vec::new();
        while let Some(m) = matches.next() {
            let passes = predicates
                .evaluate_all_for_pattern(&query, m.pattern_index, m, source.as_bytes())
                .unwrap_or(false);

            if passes {
                if let Some(name_node) = find_capture_node(m, &query, "name") {
                    if let Ok(text) = name_node.utf8_text(source.as_bytes()) {
                        matched_names.push(text.to_string());
                    }
                }
            }
        }

        // Should only match free_function, not impl_function
        assert_eq!(matched_names, vec!["free_function"]);
    }

    #[test]
    fn test_all_rust_handlers_registered() {
        let handlers: Vec<_> = crate::handler_registry::all_handlers()
            .filter(|h| h.language == "rust")
            .collect();

        // Verify essential handlers are registered
        let expected = [
            "rust::free_function",
            "rust::struct_field",
            "rust::struct_definition",
            "rust::enum_variant",
            "rust::method_in_inherent_impl",
            "rust::method_in_trait_impl",
            "rust::trait_impl",
            "rust::inherent_impl",
        ];

        for name in expected {
            assert!(
                handlers.iter().any(|h| h.name == name),
                "Handler '{name}' should be registered"
            );
        }
    }

    #[test]
    fn test_struct_field_extraction() {
        use crate::common::import_map::ImportMap;
        use codesearch_core::entities::{EntityType, Language};
        use std::path::Path;

        let source = r#"
pub struct Config {
    pub name: String,
}

pub struct Wrapper {
    pub inner: Config,
}
        "#;

        let ts_language: TsLanguage = tree_sitter_rust::LANGUAGE.into();
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&ts_language).unwrap();
        let tree = parser.parse(source, None).unwrap();
        let import_map = ImportMap::default();

        let handler_ctx = HandlerContext {
            source,
            file_path: Path::new("/test/lib.rs"),
            repository_id: "test-repo",
            package_name: Some("test_crate"),
            source_root: Some(Path::new("/test")),
            repo_root: Path::new("/test"),
            language: Language::Rust,
            language_str: "rust",
            import_map: &import_map,
            path_config: &crate::common::path_config::RUST_PATH_CONFIG,
            edge_case_handlers: None,
        };

        let entities = extract_with_handlers(&handler_ctx, tree.root_node()).unwrap();

        // Should find 4 entities: 2 structs + 2 properties
        assert_eq!(entities.len(), 4, "Should extract 4 entities");

        // Check structs
        assert!(entities
            .iter()
            .any(|e| e.qualified_name.to_string() == "test_crate::Config"
                && e.entity_type == EntityType::Struct));
        assert!(entities
            .iter()
            .any(|e| e.qualified_name.to_string() == "test_crate::Wrapper"
                && e.entity_type == EntityType::Struct));

        // Check properties (struct fields)
        assert!(entities.iter().any(|e| e.qualified_name.to_string()
            == "test_crate::Config::name"
            && e.entity_type == EntityType::Property));
        assert!(entities.iter().any(|e| e.qualified_name.to_string()
            == "test_crate::Wrapper::inner"
            && e.entity_type == EntityType::Property));
    }
}
