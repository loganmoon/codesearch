//! TypeScript language extractor module

pub(crate) mod handler_impls;
pub(crate) mod queries;

use codesearch_languages_macros::define_language_extractor;

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
