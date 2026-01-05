//! Python language extractor module

pub(crate) mod handler_impls;
pub mod module_path;
pub(crate) mod queries;
pub mod utils;

use crate::qualified_name::ScopePattern;
use codesearch_languages_macros::define_language_extractor;

/// Scope patterns for Python qualified name building
///
/// These patterns identify AST nodes that contribute to qualified names.
/// Used by the macro-generated ScopeConfiguration.
const SCOPE_PATTERNS: &[ScopePattern] = &[
    ScopePattern {
        node_kind: "class_definition",
        field_name: "name",
    },
    ScopePattern {
        node_kind: "function_definition",
        field_name: "name",
    },
];

define_language_extractor! {
    language: Python,
    tree_sitter: tree_sitter_python::LANGUAGE,
    extensions: ["py", "pyi"],

    fqn: {
        separator: ".",
        module_path_fn: module_path::derive_module_path,
    },

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
        },
        module => {
            query: queries::MODULE_QUERY,
            handler: handler_impls::handle_module_impl
        }
    }
}
