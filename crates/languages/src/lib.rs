#![warn(warnings)]
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]

mod rust;

// Unified entity builders
mod generic_entities;

// Transport model for intermediate entity representation
mod transport;

// Generic data-driven extractor framework
mod extraction_framework;

use codesearch_core::error::Result;

// Re-export only the public API
pub use extraction_framework::GenericExtractor;
pub use transport::EntityData;

// Re-export Language from core for convenience
pub use codesearch_core::entities::Language;

/// Create an extractor for the specified language
pub fn create_extractor(language: Language) -> Result<GenericExtractor<'static>> {
    match language {
        Language::Rust => rust::create_rust_extractor(),
        Language::Python => {
            Err(codesearch_core::error::Error::NotImplemented(
                "Python extractor not yet implemented".to_string(),
            ))
        }
        Language::JavaScript => {
            Err(codesearch_core::error::Error::NotImplemented(
                "JavaScript extractor not yet implemented".to_string(),
            ))
        }
        Language::TypeScript => {
            Err(codesearch_core::error::Error::NotImplemented(
                "TypeScript extractor not yet implemented".to_string(),
            ))
        }
        Language::Go => {
            Err(codesearch_core::error::Error::NotImplemented(
                "Go extractor not yet implemented".to_string(),
            ))
        }
        Language::Java | Language::CSharp | Language::Cpp => {
            Err(codesearch_core::error::Error::NotImplemented(
                format!("{} extractor not yet implemented", language),
            ))
        }
        Language::Unknown => {
            Err(codesearch_core::error::Error::InvalidInput(
                "Cannot create extractor for unknown language".to_string(),
            ))
        }
    }
}
