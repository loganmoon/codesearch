//! Python language extractor module

pub(crate) mod handler_impls;
pub mod module_path;
pub(crate) mod queries;

use crate::qualified_name::{ScopeConfiguration, ScopePattern};
use codesearch_languages_macros::define_language_extractor;

/// Scope patterns for Python qualified name building
const PYTHON_SCOPE_PATTERNS: &[ScopePattern] = &[
    ScopePattern {
        node_kind: "class_definition",
        field_name: "name",
    },
    ScopePattern {
        node_kind: "function_definition",
        field_name: "name",
    },
];

inventory::submit! {
    ScopeConfiguration {
        language: "python",
        separator: ".",
        patterns: PYTHON_SCOPE_PATTERNS,
    }
}

define_language_extractor! {
    language: Python,
    tree_sitter: tree_sitter_python::LANGUAGE,
    extensions: ["py", "pyi"],

    entities: {
        function => {
            query: queries::FUNCTION_QUERY,
            handler: handler_impls::handle_function_impl
        },
        class => {
            query: queries::CLASS_QUERY,
            handler: handler_impls::handle_class_impl
        },
        method => {
            query: queries::METHOD_QUERY,
            handler: handler_impls::handle_method_impl
        }
    }
}
