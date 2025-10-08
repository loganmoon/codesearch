#![warn(warnings)]
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]

use codesearch_core::{error::Result, CodeEntity};
use std::path::Path;

// All internal modules are private
mod extraction_framework;
mod rust;

// Public module for qualified name building
pub mod qualified_name;

/// Trait for extracting code entities from source files
pub trait Extractor: Send + Sync {
    /// Extract entities from source code
    fn extract(&self, source: &str, file_path: &Path) -> Result<Vec<CodeEntity>>;
}

/// Create an appropriate extractor for a file based on its extension
///
/// Returns Ok(None) if the file type is not supported, Err if extractor creation fails
pub fn create_extractor(
    file_path: &Path,
    repository_id: &str,
) -> Result<Option<Box<dyn Extractor>>> {
    let Some(extension) = file_path.extension().and_then(|e| e.to_str()) else {
        return Ok(None);
    };

    match extension.to_lowercase().as_str() {
        "rs" => Ok(Some(Box::new(rust::RustExtractor::new(
            repository_id.to_string(),
        )?))),
        // Future language support can be added here:
        // "py" => create_python_extractor(repository_id),
        // "js" | "jsx" => create_javascript_extractor(repository_id),
        // "ts" | "tsx" => create_typescript_extractor(repository_id),
        // "go" => create_go_extractor(repository_id),
        _ => Ok(None),
    }
}

/// Get the language identifier from a file path
///
/// This is a utility function for determining language from file extension
pub fn detect_language(file_path: &Path) -> Option<&'static str> {
    let extension = file_path.extension()?.to_str()?;

    match extension.to_lowercase().as_str() {
        "rs" => Some("rust"),
        "py" => Some("python"),
        "js" | "jsx" => Some("javascript"),
        "ts" | "tsx" => Some("typescript"),
        "go" => Some("go"),
        _ => None,
    }
}
