//! JavaScript language extractor module

use crate::common::js_ts_shared::handlers as js_handlers;
use crate::common::js_ts_shared::module_path;
use crate::common::js_ts_shared::queries as js_queries;
use crate::common::js_ts_shared::SCOPE_PATTERNS;
use codesearch_languages_macros::define_language_extractor;

define_language_extractor! {
    language: JavaScript,
    tree_sitter: tree_sitter_javascript::LANGUAGE,
    extensions: ["js", "jsx"],

    fqn: {
        family: ModuleBased,
        module_path_fn: module_path::derive_module_path,
    },

    entities: {
        // File-level module entity
        module => {
            query: js_queries::MODULE_QUERY,
            handler: js_handlers::handle_module_impl
        },
        function_decl => {
            query: js_queries::FUNCTION_DECLARATION_QUERY,
            handler: js_handlers::handle_function_declaration_impl
        },
        function_expr => {
            query: js_queries::FUNCTION_EXPRESSION_QUERY,
            handler: js_handlers::handle_function_expression_impl
        },
        arrow_function => {
            query: js_queries::ARROW_FUNCTION_QUERY,
            handler: js_handlers::handle_arrow_function_impl
        },
        class_decl => {
            query: js_queries::CLASS_DECLARATION_QUERY,
            handler: js_handlers::handle_class_declaration_impl
        },
        class_expr => {
            query: js_queries::CLASS_EXPRESSION_QUERY,
            handler: js_handlers::handle_class_expression_impl
        },
        method => {
            query: js_queries::METHOD_QUERY,
            handler: js_handlers::handle_method_impl
        },
        property => {
            query: js_queries::PROPERTY_QUERY,
            handler: js_handlers::handle_property_impl
        },
        constant => {
            query: js_queries::CONST_QUERY,
            handler: js_handlers::handle_const_impl
        },
        let_var => {
            query: js_queries::LET_QUERY,
            handler: js_handlers::handle_let_impl
        },
        var => {
            query: js_queries::VAR_QUERY,
            handler: js_handlers::handle_var_impl
        }
    }
}
