//! TSX language extractor module
//!
//! TSX (TypeScript with JSX) requires a separate parser from plain TypeScript
//! because it has JSX syntax that the standard TypeScript grammar cannot handle.

use crate::common::js_ts_shared::handlers as ts_handlers;
use crate::common::js_ts_shared::module_path;
use crate::common::js_ts_shared::queries as ts_queries;
use crate::common::js_ts_shared::TS_SCOPE_PATTERNS as SCOPE_PATTERNS;
use codesearch_languages_macros::define_language_extractor;

define_language_extractor! {
    language: Tsx,
    tree_sitter: tree_sitter_typescript::LANGUAGE_TSX,
    extensions: ["tsx"],

    fqn: {
        family: ModuleBased,
        module_path_fn: module_path::derive_module_path,
    },

    entities: {
        // File-level module entity
        module => {
            query: ts_queries::MODULE_QUERY,
            handler: ts_handlers::handle_tsx_module_impl
        },
        // Shared entity types (using TSX-specific handlers for correct Language labeling)
        function_decl => {
            query: ts_queries::FUNCTION_DECLARATION_QUERY,
            handler: ts_handlers::handle_tsx_function_declaration_impl
        },
        function_expr => {
            query: ts_queries::FUNCTION_EXPRESSION_QUERY,
            handler: ts_handlers::handle_tsx_function_expression_impl
        },
        arrow_function => {
            query: ts_queries::ARROW_FUNCTION_QUERY,
            handler: ts_handlers::handle_tsx_arrow_function_impl
        },
        class_decl => {
            query: ts_queries::TS_CLASS_DECLARATION_QUERY,
            handler: ts_handlers::handle_tsx_class_declaration_impl
        },
        class_expr => {
            query: ts_queries::TS_CLASS_EXPRESSION_QUERY,
            handler: ts_handlers::handle_tsx_class_expression_impl
        },
        method => {
            query: ts_queries::METHOD_QUERY,
            handler: ts_handlers::handle_tsx_method_impl
        },
        property => {
            query: ts_queries::TS_PROPERTY_QUERY,
            handler: ts_handlers::handle_tsx_property_impl
        },
        constant => {
            query: ts_queries::CONST_QUERY,
            handler: ts_handlers::handle_tsx_const_impl
        },
        let_var => {
            query: ts_queries::LET_QUERY,
            handler: ts_handlers::handle_tsx_let_impl
        },
        var => {
            query: ts_queries::VAR_QUERY,
            handler: ts_handlers::handle_tsx_var_impl
        },

        // TypeScript-specific entity types
        interface => {
            query: ts_queries::INTERFACE_QUERY,
            handler: ts_handlers::handle_tsx_interface_impl
        },
        interface_property => {
            query: ts_queries::INTERFACE_PROPERTY_QUERY,
            handler: ts_handlers::handle_tsx_interface_property_impl
        },
        interface_method => {
            query: ts_queries::INTERFACE_METHOD_QUERY,
            handler: ts_handlers::handle_tsx_interface_method_impl
        },
        index_signature => {
            query: ts_queries::INDEX_SIGNATURE_QUERY,
            handler: ts_handlers::handle_tsx_index_signature_impl
        },
        call_signature => {
            query: ts_queries::CALL_SIGNATURE_QUERY,
            handler: ts_handlers::handle_tsx_call_signature_impl
        },
        construct_signature => {
            query: ts_queries::CONSTRUCT_SIGNATURE_QUERY,
            handler: ts_handlers::handle_tsx_construct_signature_impl
        },
        type_alias => {
            query: ts_queries::TYPE_ALIAS_QUERY,
            handler: ts_handlers::handle_tsx_type_alias_impl
        },
        r#enum => {
            query: ts_queries::ENUM_QUERY,
            handler: ts_handlers::handle_tsx_enum_impl
        },
        enum_member => {
            query: ts_queries::ENUM_MEMBER_QUERY,
            handler: ts_handlers::handle_tsx_enum_member_impl
        },
        namespace => {
            query: ts_queries::NAMESPACE_QUERY,
            handler: ts_handlers::handle_tsx_namespace_impl
        },
        abstract_method => {
            query: ts_queries::ABSTRACT_METHOD_QUERY,
            handler: ts_handlers::handle_tsx_abstract_method_impl
        },
        ambient_function => {
            query: ts_queries::AMBIENT_FUNCTION_QUERY,
            handler: ts_handlers::handle_tsx_ambient_function_impl
        },
        ambient_const => {
            query: ts_queries::AMBIENT_CONST_QUERY,
            handler: ts_handlers::handle_tsx_ambient_const_impl
        },
        ambient_let => {
            query: ts_queries::AMBIENT_LET_QUERY,
            handler: ts_handlers::handle_tsx_ambient_let_impl
        },
        ambient_var => {
            query: ts_queries::AMBIENT_VAR_QUERY,
            handler: ts_handlers::handle_tsx_ambient_var_impl
        },
        ambient_class => {
            query: ts_queries::AMBIENT_CLASS_QUERY,
            handler: ts_handlers::handle_tsx_ambient_class_impl
        }
        // TODO #186: Parameter property extraction needs special qualified name handling
        // to skip the constructor scope. Disabled for now.
        // parameter_property => {
        //     query: ts_queries::PARAMETER_PROPERTY_QUERY,
        //     handler: ts_handlers::handle_tsx_parameter_property_impl
        // }
    }
}
