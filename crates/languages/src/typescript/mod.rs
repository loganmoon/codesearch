//! TypeScript language extractor module

pub(crate) mod handler_impls;
pub(crate) mod queries;
pub(crate) mod utils;

use crate::qualified_name::{ScopeConfiguration, ScopePattern};
use codesearch_languages_macros::define_language_extractor;

/// Scope patterns for TypeScript qualified name building
const TYPESCRIPT_SCOPE_PATTERNS: &[ScopePattern] = &[
    ScopePattern {
        node_kind: "class_declaration",
        field_name: "name",
    },
    ScopePattern {
        node_kind: "function_declaration",
        field_name: "name",
    },
    ScopePattern {
        node_kind: "interface_declaration",
        field_name: "name",
    },
];

inventory::submit! {
    ScopeConfiguration {
        language: "typescript",
        separator: ".",
        patterns: TYPESCRIPT_SCOPE_PATTERNS,
    }
}

define_language_extractor! {
    language: TypeScript,
    tree_sitter: tree_sitter_typescript::LANGUAGE_TYPESCRIPT,
    extensions: ["ts", "tsx"],

    entities: {
        function => {
            query: queries::FUNCTION_QUERY,
            handler: handler_impls::handle_function_impl
        },
        arrow_function => {
            query: queries::ARROW_FUNCTION_QUERY,
            handler: handler_impls::handle_arrow_function_impl
        },
        class => {
            query: queries::CLASS_QUERY,
            handler: handler_impls::handle_class_impl
        },
        method => {
            query: queries::METHOD_QUERY,
            handler: handler_impls::handle_method_impl
        },
        interface => {
            query: queries::INTERFACE_QUERY,
            handler: handler_impls::handle_interface_impl
        },
        type_alias => {
            query: queries::TYPE_ALIAS_QUERY,
            handler: handler_impls::handle_type_alias_impl
        },
        r#enum => {
            query: queries::ENUM_QUERY,
            handler: handler_impls::handle_enum_impl
        }
    }
}
