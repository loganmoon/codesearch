//! TypeScript-specific entity handlers

use crate::common::js_ts_shared::TypeScript;
use crate::define_handler;
use codesearch_core::Visibility;

use super::common::{
    derive_index_signature_name, enum_metadata, extract_interface_extends_relationships,
};

// Interface
define_handler!(TypeScript, handle_interface_impl, "interface", Interface,
    relationships: extract_interface_extends_relationships);

// Type alias
define_handler!(TypeScript, handle_type_alias_impl, "type_alias", TypeAlias);

// Namespace (produces Module entity)
define_handler!(TypeScript, handle_namespace_impl, "namespace", Module);

// Enum member
define_handler!(
    TypeScript,
    handle_enum_member_impl,
    "enum_member",
    EnumVariant
);

// Enum - with const detection metadata
define_handler!(TypeScript, handle_enum_impl, "enum", Enum,
    metadata: enum_metadata);

// Interface property - always Public visibility
define_handler!(TypeScript, handle_interface_property_impl, "interface_property", Property,
    visibility: Visibility::Public);

// Interface method - always Public visibility
define_handler!(TypeScript, handle_interface_method_impl, "interface_method", Method,
    visibility: Visibility::Public);

// Call signature - static name "()", always Public
define_handler!(TypeScript, handle_call_signature_impl, "call_signature", Method,
    name: "()",
    visibility: Visibility::Public);

// Construct signature - static name "new()", always Public
define_handler!(TypeScript, handle_construct_signature_impl, "construct_signature", Method,
    name: "new()",
    visibility: Visibility::Public);

// Index signature - derived name from type, always Public
define_handler!(TypeScript, handle_index_signature_impl, "index_signature", Property,
    name_fn: derive_index_signature_name,
    visibility: Visibility::Public);

// Abstract method - always Public (must be overridden by subclasses)
define_handler!(TypeScript, handle_abstract_method_impl, "method", Method,
    visibility: Visibility::Public);

// Ambient function declaration - declare function foo(): T
define_handler!(TypeScript, handle_ambient_function_impl, "function", Function,
    visibility: Visibility::Public);

// Ambient const declaration - declare const FOO: T
// Uses const metadata to mark it as const
define_handler!(TypeScript, handle_ambient_const_impl, "const", Constant,
    visibility: Visibility::Public,
    metadata: super::common::const_metadata);

// Ambient let declaration - declare let foo: T
define_handler!(TypeScript, handle_ambient_let_impl, "let", Variable,
    visibility: Visibility::Public);

// Ambient var declaration - declare var foo: T
define_handler!(TypeScript, handle_ambient_var_impl, "var", Variable,
    visibility: Visibility::Public);

// Ambient class declaration - declare class Foo { ... }
define_handler!(TypeScript, handle_ambient_class_impl, "class", Class,
    visibility: Visibility::Public);

// TODO #186: Constructor parameter property - public x: number in constructor
// Needs special qualified name handling to skip constructor scope
// define_handler!(TypeScript, handle_parameter_property_impl, "property", Property);
