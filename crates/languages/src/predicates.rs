//! Query predicate evaluation
//!
//! Tree-sitter automatically evaluates built-in predicates like `#eq?` and `#match?`,
//! but custom predicates like `#not-has-child?` and `#has-ancestor?` must be evaluated
//! manually. This module provides the infrastructure for proper predicate evaluation.

use codesearch_core::error::{Error, Result};
use tree_sitter::{Node, Query, QueryMatch, QueryPredicate, QueryPredicateArg};

/// Trait for evaluating tree-sitter query predicates
///
/// Implement this trait to provide custom predicate evaluation logic.
pub trait PredicateEvaluator {
    /// Evaluate a single predicate against a query match
    ///
    /// Returns `Ok(true)` if the predicate passes, `Ok(false)` if it fails,
    /// or an error if the predicate is malformed or unknown.
    fn evaluate<'a>(
        &self,
        predicate: &QueryPredicate,
        query: &Query,
        match_: &QueryMatch<'a, 'a>,
        source: &[u8],
    ) -> Result<bool>;

    /// Evaluate all general predicates for a pattern
    ///
    /// Returns `Ok(true)` only if ALL predicates pass.
    /// Short-circuits on first failure.
    fn evaluate_all_for_pattern<'a>(
        &self,
        query: &Query,
        pattern_index: usize,
        match_: &QueryMatch<'a, 'a>,
        source: &[u8],
    ) -> Result<bool> {
        for predicate in query.general_predicates(pattern_index) {
            if !self.evaluate(predicate, query, match_, source)? {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

/// Standard predicate evaluator implementing common custom predicates
///
/// Supports:
/// - `#has-child?` / `#not-has-child?` - Check if a node has a child with a field name
/// - `#has-ancestor?` / `#not-has-ancestor?` - Check if a node has an ancestor of a kind
///
/// Note: Built-in predicates like `#eq?` and `#match?` are handled automatically
/// by tree-sitter and don't appear in `general_predicates()`.
pub struct StandardPredicates;

impl StandardPredicates {
    /// Find the captured node for a given capture index
    fn find_capture_node<'a>(
        &self,
        match_: &QueryMatch<'a, 'a>,
        capture_index: u32,
    ) -> Option<Node<'a>> {
        match_
            .captures
            .iter()
            .find(|c| c.index == capture_index)
            .map(|c| c.node)
    }

    /// Resolve a capture argument to its index
    fn resolve_capture_index(&self, arg: &QueryPredicateArg) -> Result<u32> {
        match arg {
            QueryPredicateArg::Capture(index) => Ok(*index),
            QueryPredicateArg::String(s) => Err(Error::entity_extraction(format!(
                "Expected capture reference, got string: {s}"
            ))),
        }
    }

    /// Extract a string argument from a predicate arg
    fn extract_string_arg<'a>(&self, arg: &'a QueryPredicateArg) -> Result<&'a str> {
        match arg {
            QueryPredicateArg::String(s) => Ok(s),
            QueryPredicateArg::Capture(idx) => Err(Error::entity_extraction(format!(
                "Expected string argument, got capture @{idx}"
            ))),
        }
    }

    /// Evaluate `#has-child?` or `#not-has-child?` predicate
    ///
    /// Syntax: `(#not-has-child? @capture field_name)`
    /// Checks if the captured node has a child with the specified field name.
    fn eval_has_child<'a>(
        &self,
        predicate: &QueryPredicate,
        match_: &QueryMatch<'a, 'a>,
        negate: bool,
    ) -> Result<bool> {
        // Expect exactly 2 args: @capture and field_name
        if predicate.args.len() != 2 {
            return Err(Error::entity_extraction(format!(
                "#{}has-child? expects 2 arguments, got {}",
                if negate { "not-" } else { "" },
                predicate.args.len()
            )));
        }

        let capture_index = self.resolve_capture_index(&predicate.args[0])?;
        let field_name = self.extract_string_arg(&predicate.args[1])?;

        let Some(node) = self.find_capture_node(match_, capture_index) else {
            // If capture not found, not-has-child passes (nothing to check)
            return Ok(negate);
        };

        let has_child = node.child_by_field_name(field_name).is_some();
        Ok(if negate { !has_child } else { has_child })
    }

    /// Evaluate `#has-ancestor?` or `#not-has-ancestor?` predicate
    ///
    /// Syntax: `(#not-has-ancestor? @capture node_kind)`
    /// Checks if the captured node has an ancestor of the specified kind.
    fn eval_has_ancestor<'a>(
        &self,
        predicate: &QueryPredicate,
        match_: &QueryMatch<'a, 'a>,
        negate: bool,
    ) -> Result<bool> {
        // Expect exactly 2 args: @capture and node_kind
        if predicate.args.len() != 2 {
            return Err(Error::entity_extraction(format!(
                "#{}has-ancestor? expects 2 arguments, got {}",
                if negate { "not-" } else { "" },
                predicate.args.len()
            )));
        }

        let capture_index = self.resolve_capture_index(&predicate.args[0])?;
        let ancestor_kind = self.extract_string_arg(&predicate.args[1])?;

        let Some(node) = self.find_capture_node(match_, capture_index) else {
            return Ok(negate);
        };

        let has_ancestor = self.find_ancestor_of_kind(node, ancestor_kind).is_some();
        Ok(if negate { !has_ancestor } else { has_ancestor })
    }

    /// Find an ancestor node of a specific kind
    fn find_ancestor_of_kind<'a>(&self, node: Node<'a>, kind: &str) -> Option<Node<'a>> {
        let mut current = node;
        while let Some(parent) = current.parent() {
            if parent.kind() == kind {
                return Some(parent);
            }
            current = parent;
        }
        None
    }
}

impl PredicateEvaluator for StandardPredicates {
    fn evaluate<'a>(
        &self,
        predicate: &QueryPredicate,
        _query: &Query,
        match_: &QueryMatch<'a, 'a>,
        _source: &[u8],
    ) -> Result<bool> {
        match predicate.operator.as_ref() {
            "has-child?" => self.eval_has_child(predicate, match_, false),
            "not-has-child?" => self.eval_has_child(predicate, match_, true),
            "has-ancestor?" => self.eval_has_ancestor(predicate, match_, false),
            "not-has-ancestor?" => self.eval_has_ancestor(predicate, match_, true),
            // Note: eq?, not-eq?, match?, not-match? are built-in and handled by tree-sitter
            unknown => Err(Error::entity_extraction(format!(
                "Unknown predicate: #{unknown}. \
                 Built-in predicates (eq?, match?, etc.) are handled automatically by tree-sitter."
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_query_and_match(
        source: &str,
        query_str: &str,
    ) -> Option<(tree_sitter::Tree, Query)> {
        let language: tree_sitter::Language = tree_sitter_rust::LANGUAGE.into();
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&language).ok()?;
        let tree = parser.parse(source, None)?;
        let query = Query::new(&language, query_str).ok()?;
        Some((tree, query))
    }

    #[test]
    fn test_general_predicates_are_parsed() {
        // First verify that tree-sitter parses our custom predicates
        let language: tree_sitter::Language = tree_sitter_rust::LANGUAGE.into();

        // In tree-sitter, predicates must be attached to a pattern.
        // A pattern with predicates is a single unit.
        // Note: The query creates a pattern and the predicate is attached to it.
        let query_str = "((impl_item) @impl (#not-has-child? @impl trait))";

        let query = Query::new(&language, query_str).expect("Query should parse");

        // Check that the predicate is in general_predicates
        for pattern_idx in 0..query.pattern_count() {
            let preds = query.general_predicates(pattern_idx);
            eprintln!("Pattern {pattern_idx}: {} general predicates", preds.len());
            for pred in preds {
                eprintln!("  operator: {}, args: {:?}", pred.operator, pred.args);
            }
        }

        // The predicate should be in general_predicates for pattern 0
        assert_eq!(query.pattern_count(), 1, "Expected 1 pattern");
        let preds = query.general_predicates(0);
        assert_eq!(preds.len(), 1, "Expected 1 general predicate");
        assert_eq!(preds[0].operator.as_ref(), "not-has-child?");
    }

    #[test]
    fn test_not_has_child_trait_field() {
        // Test that #not-has-child? correctly filters trait impls from inherent impls
        let source = r#"
            impl MyStruct {
                fn inherent_method(&self) {}
            }

            impl MyTrait for MyStruct {
                fn trait_method(&self) {}
            }
        "#;

        // Query that captures impl blocks and uses #not-has-child? to filter
        // NOTE: Predicates must be wrapped with the pattern in parentheses
        let query_str = "((impl_item) @impl (#not-has-child? @impl trait))";

        let Some((tree, query)) = create_test_query_and_match(source, query_str) else {
            panic!("Failed to create test query");
        };

        let mut cursor = tree_sitter::QueryCursor::new();
        let root = tree.root_node();
        let predicates = StandardPredicates;

        let mut matches: Vec<_> = vec![];
        {
            use streaming_iterator::StreamingIterator;
            let mut iter = cursor.matches(&query, root, source.as_bytes());
            while let Some(m) = iter.next() {
                // Evaluate predicates
                let passes = predicates
                    .evaluate_all_for_pattern(&query, m.pattern_index, m, source.as_bytes())
                    .expect("predicate evaluation failed");

                if passes && !m.captures.is_empty() {
                    matches.push(m.captures[0].node.start_byte());
                }
            }
        }

        // Should only match the inherent impl, not the trait impl
        assert_eq!(
            matches.len(),
            1,
            "Expected exactly 1 match (inherent impl only)"
        );

        // Verify it's the inherent impl by checking it doesn't contain "MyTrait"
        let matched_text = &source[matches[0]..];
        assert!(
            matched_text.starts_with("impl MyStruct"),
            "Expected inherent impl, got: {matched_text}"
        );
    }

    #[test]
    fn test_has_ancestor_filters_nested_functions() {
        let source = r#"
            fn top_level() {}

            impl MyStruct {
                fn in_impl(&self) {}
            }
        "#;

        // Query for functions NOT inside impl blocks
        // NOTE: Predicates must be wrapped with the pattern in parentheses
        let query_str =
            "((function_item name: (identifier) @name) @func (#not-has-ancestor? @func impl_item))";

        let Some((tree, query)) = create_test_query_and_match(source, query_str) else {
            panic!("Failed to create test query");
        };

        let mut cursor = tree_sitter::QueryCursor::new();
        let root = tree.root_node();
        let predicates = StandardPredicates;

        let mut matched_names: Vec<String> = vec![];
        {
            use streaming_iterator::StreamingIterator;
            let mut iter = cursor.matches(&query, root, source.as_bytes());
            while let Some(m) = iter.next() {
                let passes = predicates
                    .evaluate_all_for_pattern(&query, m.pattern_index, m, source.as_bytes())
                    .expect("predicate evaluation failed");
                if passes {
                    // Find the @name capture
                    for capture in m.captures {
                        if query.capture_names()[capture.index as usize] == "name" {
                            if let Ok(text) = capture.node.utf8_text(source.as_bytes()) {
                                matched_names.push(text.to_string());
                            }
                        }
                    }
                }
            }
        }

        assert_eq!(matched_names, vec!["top_level"]);
    }

    #[test]
    fn test_unknown_predicate_returns_error() {
        let predicate = QueryPredicate {
            operator: "unknown-predicate?".into(),
            args: vec![].into(),
        };

        let language: tree_sitter::Language = tree_sitter_rust::LANGUAGE.into();
        let query = Query::new(&language, "(function_item) @func").unwrap();
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&language).unwrap();
        let tree = parser.parse("fn test() {}", None).unwrap();

        let mut cursor = tree_sitter::QueryCursor::new();
        {
            use streaming_iterator::StreamingIterator;
            let mut matches = cursor.matches(&query, tree.root_node(), "fn test() {}".as_bytes());
            if let Some(m) = matches.next() {
                let predicates = StandardPredicates;
                let result = predicates.evaluate(&predicate, &query, m, "fn test() {}".as_bytes());
                assert!(result.is_err());
                assert!(result
                    .unwrap_err()
                    .to_string()
                    .contains("Unknown predicate"));
            }
        }
    }
}
