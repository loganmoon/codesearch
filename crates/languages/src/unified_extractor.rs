//! Unified extractor that routes to appropriate language-specific extractors

use crate::{create_extractor, EntityData, Language};
use codesearch_core::error::Result;
use std::path::Path;
use tracing::debug;

/// Public trait for entity extraction
pub trait Extractor: Send {
    /// Extract entities from the given file
    fn extract(&mut self, path: &Path, content: &str) -> Result<Vec<EntityData>>;
}

/// Unified extractor that creates extractors on demand
pub struct UnifiedExtractor;

impl UnifiedExtractor {
    /// Create a new unified extractor
    pub fn new() -> Self {
        Self
    }

    /// Detect language from file extension
    fn detect_language(path: &Path) -> Option<Language> {
        let extension = path.extension()?.to_str()?;
        match extension {
            "rs" => Some(Language::Rust),
            "py" => Some(Language::Python),
            "js" | "mjs" | "cjs" => Some(Language::JavaScript),
            "ts" | "tsx" => Some(Language::TypeScript),
            "go" => Some(Language::Go),
            "java" => Some(Language::Java),
            "cs" => Some(Language::CSharp),
            "cpp" | "cc" | "cxx" | "c++" => Some(Language::Cpp),
            _ => None,
        }
    }
}

impl Default for UnifiedExtractor {
    fn default() -> Self {
        Self::new()
    }
}

impl Extractor for UnifiedExtractor {
    fn extract(&mut self, path: &Path, content: &str) -> Result<Vec<EntityData>> {
        // Detect language from file extension
        let language = Self::detect_language(path).unwrap_or(Language::Unknown);

        if language == Language::Unknown {
            debug!("Unknown language for file: {:?}", path);
            return Ok(Vec::new());
        }

        // Create extractor on demand for the specific language
        match create_extractor(language) {
            Ok(mut extractor) => extractor.extract(content, path),
            Err(e) => {
                debug!("No extractor available for language {:?}: {}", language, e);
                Ok(Vec::new())
            }
        }
    }
}
