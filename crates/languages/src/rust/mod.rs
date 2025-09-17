//! Rust language direct extractor module
//!
//! This module provides direct extraction of Rust entities and relationships
//! using tree-sitter's native capabilities.

pub(crate) mod entities;
pub(crate) mod handlers;
pub(crate) mod queries;

pub use entities::{FieldInfo, MacroType, RustEntityVariant, VariantInfo};

use crate::extraction_framework::{GenericExtractor, LanguageConfigurationBuilder};
use codesearch_core::error::Result;

/// Create a Rust language extractor with configured handlers
pub fn create_rust_extractor() -> Result<GenericExtractor<'static>> {
    let language = tree_sitter_rust::LANGUAGE.into();

    let config = LanguageConfigurationBuilder::new(language)
        .add_extractor(
            "function",
            queries::FUNCTION_QUERY,
            Box::new(handlers::handle_function),
        )
        .add_extractor(
            "struct",
            queries::STRUCT_QUERY,
            Box::new(handlers::handle_struct),
        )
        .add_extractor("enum", queries::ENUM_QUERY, Box::new(handlers::handle_enum))
        .add_extractor(
            "trait",
            queries::TRAIT_QUERY,
            Box::new(handlers::handle_trait),
        )
        .build()?;

    // Store the config in a static location to ensure it lives long enough
    // This is a temporary solution - in production, the config should be managed differently
    let config_ptr = Box::leak(Box::new(config));

    GenericExtractor::new(config_ptr)
}
