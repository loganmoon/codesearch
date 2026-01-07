//! TSX-specific entity handlers
//!
//! TSX (TypeScript with JSX) handlers use the `Tsx` extractor for correct
//! language labeling while sharing the same extraction logic as TypeScript.

use crate::common::js_ts_shared::Tsx;
use crate::define_handler;
use codesearch_core::Visibility;

use super::common::{
    arrow_function_metadata, derive_class_expression_name, derive_function_expression_name,
    derive_index_signature_name, derive_module_name_from_ctx, enum_metadata,
    extract_extends_relationships, extract_interface_extends_relationships, function_metadata,
};

// ==================== Module ====================

define_handler!(Tsx, handle_tsx_module_impl, "program", Module,
    name_ctx_fn: derive_module_name_from_ctx,
    visibility: Visibility::Public);

// ==================== Functions ====================

define_handler!(Tsx, handle_tsx_function_declaration_impl, "function", Function,
    metadata: function_metadata);

define_handler!(Tsx, handle_tsx_function_expression_impl, "function", Function,
    name_ctx_fn: derive_function_expression_name,
    metadata: function_metadata);

define_handler!(Tsx, handle_tsx_arrow_function_impl, "function", Function,
    metadata: arrow_function_metadata);

// ==================== Classes ====================

define_handler!(Tsx, handle_tsx_class_declaration_impl, "class", Class,
    relationships: extract_extends_relationships);

define_handler!(Tsx, handle_tsx_class_expression_impl, "class", Class,
    name_ctx_fn: derive_class_expression_name,
    relationships: extract_extends_relationships);

// ==================== Methods ====================

define_handler!(Tsx, handle_tsx_method_impl, "method", Method,
    metadata: function_metadata);

// ==================== Properties ====================

define_handler!(Tsx, handle_tsx_property_impl, "property", Property);

// ==================== Variables ====================

define_handler!(Tsx, handle_tsx_const_impl, "const", Constant,
    metadata: super::common::const_metadata);

define_handler!(Tsx, handle_tsx_let_impl, "let", Variable);

define_handler!(Tsx, handle_tsx_var_impl, "var", Variable);

// ==================== TypeScript-Specific ====================

// Interface
define_handler!(Tsx, handle_tsx_interface_impl, "interface", Interface,
    relationships: extract_interface_extends_relationships);

// Type alias
define_handler!(Tsx, handle_tsx_type_alias_impl, "type_alias", TypeAlias);

// Namespace (produces Module entity)
define_handler!(Tsx, handle_tsx_namespace_impl, "namespace", Module);

// Enum member
define_handler!(Tsx, handle_tsx_enum_member_impl, "enum_member", EnumVariant);

// Enum - with const detection metadata
define_handler!(Tsx, handle_tsx_enum_impl, "enum", Enum,
    metadata: enum_metadata);

// Interface property - always Public visibility
define_handler!(Tsx, handle_tsx_interface_property_impl, "interface_property", Property,
    visibility: Visibility::Public);

// Interface method - always Public visibility
define_handler!(Tsx, handle_tsx_interface_method_impl, "interface_method", Method,
    visibility: Visibility::Public);

// Call signature - static name "()", always Public
define_handler!(Tsx, handle_tsx_call_signature_impl, "call_signature", Method,
    name: "()",
    visibility: Visibility::Public);

// Construct signature - static name "new()", always Public
define_handler!(Tsx, handle_tsx_construct_signature_impl, "construct_signature", Method,
    name: "new()",
    visibility: Visibility::Public);

// Index signature - derived name from type, always Public
define_handler!(Tsx, handle_tsx_index_signature_impl, "index_signature", Property,
    name_fn: derive_index_signature_name,
    visibility: Visibility::Public);

// Abstract method - always Public (must be overridden by subclasses)
define_handler!(Tsx, handle_tsx_abstract_method_impl, "method", Method,
    visibility: Visibility::Public);

// Ambient function declaration - declare function foo(): T
define_handler!(Tsx, handle_tsx_ambient_function_impl, "function", Function,
    visibility: Visibility::Public);

// Ambient const declaration - declare const FOO: T
define_handler!(Tsx, handle_tsx_ambient_const_impl, "const", Constant,
    visibility: Visibility::Public,
    metadata: super::common::const_metadata);

// Ambient let declaration - declare let foo: T
define_handler!(Tsx, handle_tsx_ambient_let_impl, "let", Variable,
    visibility: Visibility::Public);

// Ambient var declaration - declare var foo: T
define_handler!(Tsx, handle_tsx_ambient_var_impl, "var", Variable,
    visibility: Visibility::Public);

// Ambient class declaration - declare class Foo { ... }
define_handler!(Tsx, handle_tsx_ambient_class_impl, "class", Class,
    visibility: Visibility::Public);

// TODO #186: Constructor parameter property - public x: number in constructor
// Needs special qualified name handling to skip constructor scope
// define_handler!(Tsx, handle_tsx_parameter_property_impl, "property", Property);
