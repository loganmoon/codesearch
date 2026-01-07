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
