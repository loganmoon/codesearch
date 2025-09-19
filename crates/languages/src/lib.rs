#![warn(warnings)]
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]

use codesearch_core::{error::Result, CodeEntity};
use std::path::Path;

// All internal modules are private
mod extraction_framework;
mod generic_entities;
mod rust;

/// Trait for extracting code entities from source files
pub trait Extractor: Send + Sync {
    /// Extract entities from source code
    fn extract(&self, source: &str, file_path: &Path) -> Result<Vec<CodeEntity>>;
}

/// Create an appropriate extractor for a file based on its extension
///
/// Returns None if the file type is not supported
pub fn create_extractor(file_path: &Path) -> Option<Box<dyn Extractor>> {
    let extension = file_path.extension()?.to_str()?;

    match extension.to_lowercase().as_str() {
        "rs" => match rust::RustExtractor::new() {
            Ok(extractor) => Some(Box::new(extractor)),
            Err(e) => {
                tracing::error!("Failed to create Rust extractor: {}", e);
                None
            }
        },
        // Future language support can be added here:
        // "py" => create_python_extractor(),
        // "js" | "jsx" => create_javascript_extractor(),
        // "ts" | "tsx" => create_typescript_extractor(),
        // "go" => create_go_extractor(),
        _ => None,
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
