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
use codesearch_core::entities::{CodeEntityBuilder, Language};
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

    /// Convert EntityData to CodeEntity
    fn convert_to_code_entity(&self, entity: EntityData, file_path: &Path) -> CodeEntity {
        let entity_type = entity.variant.entity_type();
        let metadata = entity.variant.into_metadata();
        let signature = entity.variant.extract_signature();
        let location = entity.location.clone();

        CodeEntityBuilder::default()
            .entity_id(format!("{}#{}", file_path.display(), entity.qualified_name))
            .name(entity.name)
            .qualified_name(entity.qualified_name)
            .entity_type(entity_type)
            .location(entity.location)
            .visibility(entity.visibility)
            .documentation_summary(entity.documentation)
            .content(entity.content)
            .dependencies(entity.dependencies)
            .metadata(metadata)
            .signature(signature)
            .language(Language::Rust)
            .build()
            .unwrap_or_else(|e| {
                tracing::error!("Failed to build CodeEntity: {}", e);
                // Return a minimal valid entity on error
                // Only set required fields
                CodeEntityBuilder::default()
                    .entity_id("error".to_string())
                    .name("error".to_string())
                    .qualified_name("error".to_string())
                    .entity_type(entity_type)
                    .location(location)
                    .language(Language::Rust)
                    .file_path(file_path.to_path_buf())
                    .line_range((0, 0))
                    .build()
                    .unwrap_or_else(|build_err| {
                        tracing::error!("Failed to build minimal CodeEntity: {}", build_err);
                        panic!("Cannot create minimal CodeEntity: {}", build_err);
                    })
            })
    }
}

impl Extractor for RustExtractor {
    fn extract(&self, source: &str, file_path: &Path) -> Result<Vec<CodeEntity>> {
        // Create a new extractor each time since extract requires &mut self
        // This is necessary because GenericExtractor::extract takes &mut self
        let mut extractor = Self::create_inner_extractor()?;
        let entities = extractor.extract(source, file_path)?;

        // Convert EntityData to CodeEntity
        Ok(entities
            .into_iter()
            .map(|entity| self.convert_to_code_entity(entity, file_path))
            .collect())
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
