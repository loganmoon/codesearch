//! Language extractor implementations for JavaScript and TypeScript
//!
//! This module provides implementations of the [`LanguageExtractors`] trait
//! for JavaScript and TypeScript, enabling the use of the generic
//! [`define_handler!`] macro for these languages.

use crate::common::language_extractors::LanguageExtractors;
use codesearch_core::entities::{Language, Visibility};
use tree_sitter::Node;

use super::handlers::common::extract_preceding_doc_comments;
use super::visibility::extract_visibility;

/// JavaScript language extractor
///
/// Provides JavaScript-specific extraction behavior:
/// - Visibility based on `export` keyword
/// - JSDoc-style documentation comments (`/** */`)
///
/// # Example
///
/// ```ignore
/// define_handler!(JavaScript, handle_function_impl, "function", Function,
///     metadata: function_metadata);
/// ```
pub struct JavaScript;

impl LanguageExtractors for JavaScript {
    const LANGUAGE: Language = Language::JavaScript;
    const LANG_STR: &'static str = "javascript";

    fn extract_visibility(node: Node, source: &str) -> Visibility {
        extract_visibility(node, source)
    }

    fn extract_docs(node: Node, source: &str) -> Option<String> {
        extract_preceding_doc_comments(node, source)
    }
}

/// TypeScript language extractor
///
/// Provides TypeScript-specific extraction behavior:
/// - Visibility based on `export` keyword (same as JavaScript)
/// - JSDoc-style documentation comments (`/** */`)
///
/// # Example
///
/// ```ignore
/// define_handler!(TypeScript, handle_interface_impl, "interface", Interface,
///     relationships: extract_extends);
/// ```
pub struct TypeScript;

impl LanguageExtractors for TypeScript {
    const LANGUAGE: Language = Language::TypeScript;
    const LANG_STR: &'static str = "typescript";

    fn extract_visibility(node: Node, source: &str) -> Visibility {
        extract_visibility(node, source)
    }

    fn extract_docs(node: Node, source: &str) -> Option<String> {
        extract_preceding_doc_comments(node, source)
    }
}

/// TSX language extractor (TypeScript with JSX)
///
/// TSX uses the same extraction behavior as TypeScript but requires
/// a separate tree-sitter parser to handle JSX syntax.
///
/// # Example
///
/// ```ignore
/// define_handler!(Tsx, handle_component_impl, "function", Function,
///     metadata: component_metadata);
/// ```
pub struct Tsx;

impl LanguageExtractors for Tsx {
    const LANGUAGE: Language = Language::TypeScript;
    const LANG_STR: &'static str = "tsx";

    fn extract_visibility(node: Node, source: &str) -> Visibility {
        extract_visibility(node, source)
    }

    fn extract_docs(node: Node, source: &str) -> Option<String> {
        extract_preceding_doc_comments(node, source)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_javascript_constants() {
        assert_eq!(JavaScript::LANGUAGE, Language::JavaScript);
        assert_eq!(JavaScript::LANG_STR, "javascript");
    }

    #[test]
    fn test_typescript_constants() {
        assert_eq!(TypeScript::LANGUAGE, Language::TypeScript);
        assert_eq!(TypeScript::LANG_STR, "typescript");
    }
}
