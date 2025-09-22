//! Unified builder for code entities
//!
//! This module provides a language-agnostic enum abstraction for code entities of various languages. Each
//! supported language should have a submodule of languages with an entities.rs file defining its specific
//! entity variants, tree-sitter queries, and handlers. See `rust` as the cannonical example.

use codesearch_core::entities::Language;
use serde::{Deserialize, Serialize};

// Import language-specific variants
use crate::rust::entities::RustEntityVariant;

mod error;

/// Language-agnostic entity variant that wraps language-specific variants
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EntityVariant {
    Rust(RustEntityVariant),
    // Future: Python(PythonEntityVariant),
    // Future: TypeScript(TypeScriptEntityVariant),
}
impl EntityVariant {
    /// Get the entity type for this variant
    #[allow(dead_code)]
    pub fn entity_type(&self) -> codesearch_core::entities::EntityType {
        match self {
            EntityVariant::Rust(rust_variant) => rust_variant.to_entity_type(),
        }
    }

    /// Get the language for this variant
    #[allow(dead_code)]
    pub fn language(&self) -> Language {
        match self {
            EntityVariant::Rust(_) => Language::Rust,
        }
    }

    /// Convert the variant to EntityMetadata
    #[allow(dead_code)]
    pub fn to_metadata(&self) -> codesearch_core::entities::EntityMetadata {
        match self {
            EntityVariant::Rust(rust_variant) => rust_variant.to_metadata(),
        }
    }

    /// Extract function signature if applicable
    #[allow(dead_code)]
    pub fn extract_signature(&self) -> Option<codesearch_core::entities::FunctionSignature> {
        match self {
            EntityVariant::Rust(rust_variant) => rust_variant.extract_signature(),
        }
    }
}
