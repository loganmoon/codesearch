//! Rust language direct extractor module
//!
//! This module provides direct extraction of Rust entities and relationships
//! using tree-sitter's native capabilities.

pub(crate) mod entities;
pub(crate) mod handlers;
pub(crate) mod queries;

use crate::extraction_framework::{
    GenericExtractor, LanguageConfiguration, LanguageConfigurationBuilder,
};
use crate::Extractor;
use codesearch_core::error::Result;
use codesearch_core::CodeEntity;
use std::path::Path;

/// Rust language extractor
pub struct RustExtractor {
    repository_id: String,
    config: LanguageConfiguration,
}

impl RustExtractor {
    /// Create a new Rust extractor
    pub fn new(repository_id: String) -> Result<Self> {
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
            .add_extractor("impl", queries::IMPL_QUERY, Box::new(handlers::handle_impl))
            .add_extractor(
                "impl_trait",
                queries::IMPL_TRAIT_QUERY,
                Box::new(handlers::handle_impl_trait),
            )
            .add_extractor(
                "module",
                queries::MODULE_QUERY,
                Box::new(handlers::handle_module),
            )
            .add_extractor(
                "constant",
                queries::CONSTANT_QUERY,
                Box::new(handlers::handle_constant),
            )
            .add_extractor(
                "type_alias",
                queries::TYPE_ALIAS_QUERY,
                Box::new(handlers::handle_type_alias),
            )
            .add_extractor(
                "macro",
                queries::MACRO_QUERY,
                Box::new(handlers::handle_macro),
            )
            .build()?;

        Ok(Self {
            repository_id,
            config,
        })
    }
}

impl Extractor for RustExtractor {
    fn extract(&self, source: &str, file_path: &Path) -> Result<Vec<CodeEntity>> {
        // Create extractor with reference to owned config
        let mut extractor = GenericExtractor::new(&self.config, self.repository_id.clone())?;
        extractor.extract(source, file_path)
    }
}
