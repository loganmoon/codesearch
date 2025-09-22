//! Rust-specific entity variants and builders
//!
//! This module provides Rust-specific entity types that work with the
//! generic EntityBuilder in the parent builders module.

use codesearch_core::entities::{EntityMetadata, EntityType, FunctionSignature, Visibility};
use im::HashMap;
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

impl RustEntityVariant {
    /// Convert the variant to an EntityType
    #[allow(dead_code)]
    pub fn to_entity_type(&self) -> EntityType {
        match self {
            RustEntityVariant::Function { .. } => EntityType::Function,
            RustEntityVariant::Struct { .. } => EntityType::Struct,
            RustEntityVariant::Trait { .. } => EntityType::Trait,
            RustEntityVariant::Enum { .. } => EntityType::Enum,
            RustEntityVariant::Impl { .. } => EntityType::Method, // Impl blocks are treated as Method type
            RustEntityVariant::Module { .. } => EntityType::Module,
            RustEntityVariant::Constant { .. } => EntityType::Constant,
            RustEntityVariant::TypeAlias { .. } => EntityType::TypeAlias,
            RustEntityVariant::Macro { .. } => EntityType::Macro,
        }
    }

    /// Convert the variant to EntityMetadata
    #[allow(dead_code)]
    pub fn to_metadata(&self) -> EntityMetadata {
        let mut metadata = EntityMetadata::default();
        let mut attributes = HashMap::new();

        match self {
            RustEntityVariant::Function {
                is_async,
                is_unsafe,
                is_const,
                generics,
                parameters,
                return_type,
            } => {
                metadata.is_async = *is_async;
                metadata.is_const = *is_const;
                metadata.is_generic = !generics.is_empty();
                metadata.generic_params = generics.clone();

                attributes.insert("is_async".to_string(), is_async.to_string());
                attributes.insert("is_unsafe".to_string(), is_unsafe.to_string());
                attributes.insert("is_const".to_string(), is_const.to_string());

                if *is_unsafe {
                    attributes.insert("unsafe".to_string(), "true".to_string());
                }

                if let Some(ret) = return_type {
                    attributes.insert("return_type".to_string(), ret.clone());
                }

                if !parameters.is_empty() {
                    let params_str = parameters
                        .iter()
                        .map(|(name, ty)| format!("{name}: {ty}"))
                        .collect::<Vec<_>>()
                        .join(", ");
                    attributes.insert("parameters".to_string(), params_str);
                }
            }
            RustEntityVariant::Struct {
                generics,
                derives,
                fields,
                is_tuple,
            } => {
                metadata.is_generic = !generics.is_empty();
                metadata.generic_params = generics.clone();

                attributes.insert("field_count".to_string(), fields.len().to_string());
                attributes.insert("is_generic".to_string(), (!generics.is_empty()).to_string());
                attributes.insert("is_tuple".to_string(), is_tuple.to_string());

                if !derives.is_empty() {
                    attributes.insert("derives".to_string(), derives.join(","));
                }

                if !generics.is_empty() {
                    attributes.insert("generic_params".to_string(), generics.join(","));
                }

                for derive in derives {
                    attributes.insert(format!("derive_{derive}"), "true".to_string());
                }
            }
            RustEntityVariant::Trait {
                generics,
                bounds,
                associated_types,
                methods,
            } => {
                metadata.is_generic = !generics.is_empty();
                metadata.generic_params = generics.clone();

                attributes.insert("method_count".to_string(), methods.len().to_string());
                attributes.insert(
                    "associated_type_count".to_string(),
                    associated_types.len().to_string(),
                );

                if !bounds.is_empty() {
                    attributes.insert("bounds".to_string(), bounds.join(" + "));
                }

                if !associated_types.is_empty() {
                    attributes.insert("associated_types".to_string(), associated_types.join(","));
                }

                if !methods.is_empty() {
                    attributes.insert("methods".to_string(), methods.join(","));
                }
            }
            RustEntityVariant::Enum {
                generics,
                derives,
                variants,
            } => {
                metadata.is_generic = !generics.is_empty();
                metadata.generic_params = generics.clone();

                attributes.insert("variant_count".to_string(), variants.len().to_string());
                attributes.insert("is_generic".to_string(), (!generics.is_empty()).to_string());

                if !derives.is_empty() {
                    attributes.insert("derives".to_string(), derives.join(","));
                }

                let variant_names: Vec<String> = variants.iter().map(|v| v.name.clone()).collect();
                if !variant_names.is_empty() {
                    attributes.insert("variants".to_string(), variant_names.join(","));
                }

                for derive in derives {
                    attributes.insert(format!("derive_{derive}"), "true".to_string());
                }
            }
            RustEntityVariant::Impl {
                for_type,
                trait_impl,
                generics,
            } => {
                metadata.is_generic = !generics.is_empty();
                metadata.generic_params = generics.clone();

                attributes.insert("for_type".to_string(), for_type.clone());

                if let Some(trait_name) = trait_impl {
                    attributes.insert("implements_trait".to_string(), trait_name.clone());
                }

                if !generics.is_empty() {
                    attributes.insert("generic_params".to_string(), generics.join(","));
                }
            }
            RustEntityVariant::Module { is_pub, items } => {
                attributes.insert("is_public".to_string(), is_pub.to_string());
                attributes.insert("item_count".to_string(), items.len().to_string());

                if !items.is_empty() {
                    attributes.insert("items".to_string(), items.join(","));
                }
            }
            RustEntityVariant::Constant {
                const_type,
                value,
                is_static,
                is_mut,
            } => {
                metadata.is_const = !is_static;
                metadata.is_static = *is_static;

                if *is_mut {
                    attributes.insert("mutable".to_string(), "true".to_string());
                }

                if let Some(ty) = const_type {
                    attributes.insert("type".to_string(), ty.clone());
                }

                if let Some(val) = value {
                    attributes.insert("value".to_string(), val.clone());
                }
            }
            RustEntityVariant::TypeAlias {
                aliased_type,
                generics,
            } => {
                metadata.is_generic = !generics.is_empty();
                metadata.generic_params = generics.clone();

                attributes.insert("aliased_type".to_string(), aliased_type.clone());

                if !generics.is_empty() {
                    attributes.insert("generic_params".to_string(), generics.join(","));
                }
            }
            RustEntityVariant::Macro { macro_type, export } => {
                let macro_type_str = match macro_type {
                    MacroType::Declarative => "declarative",
                    MacroType::Proc => "proc_macro",
                    MacroType::Derive => "derive",
                    MacroType::Attribute => "attribute",
                };

                attributes.insert("macro_type".to_string(), macro_type_str.to_string());
                attributes.insert("exported".to_string(), export.to_string());
            }
        }

        metadata.attributes = attributes;
        metadata
    }

    /// Extract function signature if applicable
    #[allow(dead_code)]
    pub fn extract_signature(&self) -> Option<FunctionSignature> {
        match self {
            RustEntityVariant::Function {
                is_async,
                generics,
                parameters,
                return_type,
                ..
            } => Some(FunctionSignature {
                parameters: parameters
                    .iter()
                    .map(|(name, ty)| (name.clone(), Some(ty.clone())))
                    .collect(),
                return_type: return_type.clone(),
                is_async: *is_async,
                generics: generics.clone(),
            }),
            _ => None,
        }
    }
}
