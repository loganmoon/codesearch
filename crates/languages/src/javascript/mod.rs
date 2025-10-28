//! JavaScript language extractor module

pub(crate) mod handler_impls;
pub(crate) mod queries;

use codesearch_languages_macros::define_language_extractor;

define_language_extractor! {
    language: JavaScript,
    tree_sitter: tree_sitter_javascript::LANGUAGE,
    extensions: ["js", "jsx"],

    entities: {
        function => {
            query: queries::FUNCTION_QUERY,
            handler: handler_impls::handle_function_impl,
        },
        arrow_function => {
            query: queries::ARROW_FUNCTION_QUERY,
            handler: handler_impls::handle_arrow_function_impl,
        },
        class => {
            query: queries::CLASS_QUERY,
            handler: handler_impls::handle_class_impl,
        },
        method => {
            query: queries::METHOD_QUERY,
            handler: handler_impls::handle_method_impl,
        }
    }
}
