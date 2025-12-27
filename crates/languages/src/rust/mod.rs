//! Rust language extractor module

pub(crate) mod entities;
pub(crate) mod handler_impls;
pub mod module_path;
pub(crate) mod queries;

use crate::qualified_name::{ScopeConfiguration, ScopePattern};
use codesearch_languages_macros::define_language_extractor;

/// Scope patterns for Rust qualified name building
const RUST_SCOPE_PATTERNS: &[ScopePattern] = &[
    ScopePattern {
        node_kind: "mod_item",
        field_name: "name",
    },
    ScopePattern {
        node_kind: "impl_item",
        field_name: "type",
    },
];

inventory::submit! {
    ScopeConfiguration {
        language: "rust",
        separator: "::",
        patterns: RUST_SCOPE_PATTERNS,
    }
}

define_language_extractor! {
    language: Rust,
    tree_sitter: tree_sitter_rust::LANGUAGE,
    extensions: ["rs"],

    entities: {
        function => {
            query: queries::FUNCTION_QUERY,
            handler: handler_impls::handle_function_impl
        },
        r#struct => {
            query: queries::STRUCT_QUERY,
            handler: handler_impls::handle_struct_impl
        },
        r#enum => {
            query: queries::ENUM_QUERY,
            handler: handler_impls::handle_enum_impl
        },
        r#trait => {
            query: queries::TRAIT_QUERY,
            handler: handler_impls::handle_trait_impl
        },
        r#impl => {
            query: queries::IMPL_QUERY,
            handler: handler_impls::handle_impl_impl
        },
        impl_trait => {
            query: queries::IMPL_TRAIT_QUERY,
            handler: handler_impls::handle_impl_trait_impl
        },
        module => {
            query: queries::MODULE_QUERY,
            handler: handler_impls::handle_module_impl
        },
        constant => {
            query: queries::CONSTANT_QUERY,
            handler: handler_impls::handle_constant_impl
        },
        type_alias => {
            query: queries::TYPE_ALIAS_QUERY,
            handler: handler_impls::handle_type_alias_impl
        },
        r#macro => {
            query: queries::MACRO_QUERY,
            handler: handler_impls::handle_macro_impl
        },
        crate_root => {
            query: queries::CRATE_ROOT_QUERY,
            handler: handler_impls::handle_crate_root_impl
        }
    }
}
