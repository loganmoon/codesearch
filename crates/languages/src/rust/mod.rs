//! Rust language direct extractor module
//!
//! This module provides direct extraction of Rust entities and relationships
//! using tree-sitter's native capabilities.

pub(crate) mod entities;
pub(crate) mod handlers;
pub(crate) mod queries;

use crate::extraction_framework::{GenericExtractor, LanguageConfigurationBuilder};
use crate::transport::EntityData;
use crate::Extractor;
use codesearch_core::entities::Language;
use codesearch_core::error::Result;
use codesearch_core::CodeEntity;
use std::path::Path;

/// Rust language extractor
pub struct RustExtractor {
    inner: GenericExtractor<'static>,
}

impl RustExtractor {
    /// Create a new Rust extractor
    pub fn new() -> Result<Self> {
        // Just create an empty RustExtractor since we'll create extractors on-demand
        let inner = Self::create_inner_extractor()?;
        Ok(Self { inner })
    }

    /// Create an inner GenericExtractor (used internally)
    fn create_inner_extractor() -> Result<GenericExtractor<'static>> {
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
        let config_ptr = Box::leak(Box::new(config));

        GenericExtractor::new(config_ptr)
    }
}

impl Extractor for RustExtractor {
    fn extract(&self, source: &str, file_path: &Path) -> Result<Vec<CodeEntity>> {
        // Create a new extractor each time since extract requires &mut self
        // This is necessary because GenericExtractor::extract takes &mut self
        let mut extractor = Self::create_inner_extractor()?;
        let entities = extractor.extract(source, file_path)?;

        // Convert EntityData to CodeEntity using the From trait
        Ok(entities.into_iter().map(CodeEntity::from).collect())
    }
}

// Keep the old function for backward compatibility during transition
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
