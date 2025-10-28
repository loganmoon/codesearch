#![deny(warnings)]
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]

use codesearch_core::{error::Result, CodeEntity};
use std::path::Path;

// All internal modules are private
mod extraction_framework;
mod javascript;
mod rust;
mod typescript;

#[cfg(test)]
mod test_language;

// Public modules
pub mod common;
pub mod qualified_name;

/// Trait for extracting code entities from source files
pub trait Extractor: Send + Sync {
    /// Extract entities from source code
    fn extract(&self, source: &str, file_path: &Path) -> Result<Vec<CodeEntity>>;
}

/// Language descriptor for automatic registration
pub struct LanguageDescriptor {
    pub name: &'static str,
    pub extensions: &'static [&'static str],
    pub factory: fn(&str) -> Result<Box<dyn Extractor>>,
}

inventory::collect!(LanguageDescriptor);

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

    let ext_lower = extension.to_lowercase();

    // Find matching language descriptor
    for descriptor in inventory::iter::<LanguageDescriptor> {
        if descriptor.extensions.contains(&ext_lower.as_str()) {
            return Ok(Some((descriptor.factory)(repository_id)?));
        }
    }

    Ok(None)
}

/// Get the language identifier from a file path
///
/// This is a utility function for determining language from file extension
pub fn detect_language(file_path: &Path) -> Option<&'static str> {
    let extension = file_path.extension()?.to_str()?;
    let ext_lower = extension.to_lowercase();

    for descriptor in inventory::iter::<LanguageDescriptor> {
        if descriptor.extensions.contains(&ext_lower.as_str()) {
            return Some(descriptor.name);
        }
    }

    None
}
