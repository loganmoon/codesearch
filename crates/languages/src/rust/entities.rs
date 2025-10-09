//! Rust-specific entity variants and builders
//!
//! This module provides Rust-specific entity types that work with the
//! generic EntityBuilder in the parent builders module.

use codesearch_core::entities::Visibility;
use serde::{Deserialize, Serialize};

/// Rust-specific entity variants
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RustEntityVariant {
    Function {
        is_async: bool,
        is_unsafe: bool,
        is_const: bool,
        generics: Vec<String>,
        parameters: Vec<(String, String)>, // (name, type)
        return_type: Option<String>,
    },
    Struct {
        generics: Vec<String>,
        derives: Vec<String>,
        fields: Vec<FieldInfo>,
        is_tuple: bool,
    },
    Trait {
        generics: Vec<String>,
        bounds: Vec<String>,
        associated_types: Vec<String>,
        methods: Vec<String>,
    },
    Enum {
        generics: Vec<String>,
        derives: Vec<String>,
        variants: Vec<VariantInfo>,
    },
    Impl {
        for_type: String,
        trait_impl: Option<String>,
        generics: Vec<String>,
    },
    Module {
        is_pub: bool,
        items: Vec<String>,
    },
    Constant {
        const_type: Option<String>,
        value: Option<String>,
        is_static: bool,
        is_mut: bool,
    },
    TypeAlias {
        aliased_type: String,
        generics: Vec<String>,
    },
    Macro {
        macro_type: MacroType,
        export: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldInfo {
    pub name: String,
    pub field_type: String,
    pub visibility: Visibility,
    pub attributes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VariantInfo {
    pub name: String,
    pub fields: Vec<FieldInfo>,
    pub discriminant: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MacroType {
    Declarative,
    Proc,
    Derive,
    Attribute,
}
