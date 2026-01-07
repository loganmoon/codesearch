//! TypeScript language extractor module

use crate::common::js_ts_shared::handlers as ts_handlers;
use crate::common::js_ts_shared::module_path;
use crate::common::js_ts_shared::queries as ts_queries;
use crate::common::js_ts_shared::TS_SCOPE_PATTERNS as SCOPE_PATTERNS;
use codesearch_languages_macros::define_language_extractor;

define_language_extractor! {
    language: TypeScript,
    tree_sitter: tree_sitter_typescript::LANGUAGE_TYPESCRIPT,
    extensions: ["ts", "tsx"],

    fqn: {
        family: ModuleBased,
        module_path_fn: module_path::derive_module_path,
    },

    entities: {
        // File-level module entity
        module => {
            query: ts_queries::MODULE_QUERY,
            handler: ts_handlers::handle_ts_module_impl
        },
        // Shared entity types (using TypeScript-specific handlers for correct Language labeling)
        function_decl => {
            query: ts_queries::FUNCTION_DECLARATION_QUERY,
            handler: ts_handlers::handle_ts_function_declaration_impl
        },
        function_expr => {
            query: ts_queries::FUNCTION_EXPRESSION_QUERY,
            handler: ts_handlers::handle_ts_function_expression_impl
        },
        arrow_function => {
            query: ts_queries::ARROW_FUNCTION_QUERY,
            handler: ts_handlers::handle_ts_arrow_function_impl
        },
        class_decl => {
            query: ts_queries::TS_CLASS_DECLARATION_QUERY,
            handler: ts_handlers::handle_ts_class_declaration_impl
        },
        class_expr => {
            query: ts_queries::TS_CLASS_EXPRESSION_QUERY,
            handler: ts_handlers::handle_ts_class_expression_impl
        },
        method => {
            query: ts_queries::METHOD_QUERY,
            handler: ts_handlers::handle_ts_method_impl
        },
        property => {
            query: ts_queries::TS_PROPERTY_QUERY,
            handler: ts_handlers::handle_ts_property_impl
        },
        constant => {
            query: ts_queries::CONST_QUERY,
            handler: ts_handlers::handle_ts_const_impl
        },
        let_var => {
            query: ts_queries::LET_QUERY,
            handler: ts_handlers::handle_ts_let_impl
        },
        var => {
            query: ts_queries::VAR_QUERY,
            handler: ts_handlers::handle_ts_var_impl
        },

        // TypeScript-specific entity types
        interface => {
            query: ts_queries::INTERFACE_QUERY,
            handler: ts_handlers::handle_interface_impl
        },
        interface_property => {
            query: ts_queries::INTERFACE_PROPERTY_QUERY,
            handler: ts_handlers::handle_interface_property_impl
        },
        interface_method => {
            query: ts_queries::INTERFACE_METHOD_QUERY,
            handler: ts_handlers::handle_interface_method_impl
        },
        index_signature => {
            query: ts_queries::INDEX_SIGNATURE_QUERY,
            handler: ts_handlers::handle_index_signature_impl
        },
        call_signature => {
            query: ts_queries::CALL_SIGNATURE_QUERY,
            handler: ts_handlers::handle_call_signature_impl
        },
        construct_signature => {
            query: ts_queries::CONSTRUCT_SIGNATURE_QUERY,
            handler: ts_handlers::handle_construct_signature_impl
        },
        type_alias => {
            query: ts_queries::TYPE_ALIAS_QUERY,
            handler: ts_handlers::handle_type_alias_impl
        },
        r#enum => {
            query: ts_queries::ENUM_QUERY,
            handler: ts_handlers::handle_enum_impl
        },
        enum_member => {
            query: ts_queries::ENUM_MEMBER_QUERY,
            handler: ts_handlers::handle_enum_member_impl
        },
        namespace => {
            query: ts_queries::NAMESPACE_QUERY,
            handler: ts_handlers::handle_namespace_impl
        },
        abstract_method => {
            query: ts_queries::ABSTRACT_METHOD_QUERY,
            handler: ts_handlers::handle_abstract_method_impl
        }
    }
}
