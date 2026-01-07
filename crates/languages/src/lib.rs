#![deny(warnings)]
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]

use codesearch_core::{error::Result, CodeEntity};
use std::path::Path;

// Internal modules (private implementation details)
mod extraction_framework;
mod javascript;
mod python;
mod tsx;
mod typescript;

// Public language-specific modules (for external use)
pub mod rust;

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

/// Factory function type for creating extractors
///
/// Arguments:
/// - `repository_id` - Repository identifier
/// - `package_name` - Optional package/crate name from manifest
/// - `source_root` - Optional source root for module path derivation
/// - `repo_root` - Repository root for deriving repo-relative paths
pub type ExtractorFactory =
    fn(&str, Option<&str>, Option<&Path>, &Path) -> Result<Box<dyn Extractor>>;

/// Language descriptor for automatic registration
pub struct LanguageDescriptor {
    pub name: &'static str,
    pub extensions: &'static [&'static str],
    /// Factory function that creates an extractor
    pub factory: ExtractorFactory,
}

inventory::collect!(LanguageDescriptor);

/// Create an appropriate extractor for a file based on its extension
///
/// Returns Ok(None) if the file type is not supported, Err if extractor creation fails
///
/// # Arguments
/// * `file_path` - Path to the file to extract from
/// * `repository_id` - Repository identifier
/// * `package_name` - Optional package/crate name from manifest
/// * `source_root` - Optional source root for module path derivation
pub fn create_extractor(
    file_path: &Path,
    repository_id: &str,
    package_name: Option<&str>,
    source_root: Option<&Path>,
    repo_root: &Path,
) -> Result<Option<Box<dyn Extractor>>> {
    let Some(extension) = file_path.extension().and_then(|e| e.to_str()) else {
        return Ok(None);
    };

    let ext_lower = extension.to_lowercase();

    // Find matching language descriptor
    for descriptor in inventory::iter::<LanguageDescriptor> {
        if descriptor.extensions.contains(&ext_lower.as_str()) {
            return Ok(Some((descriptor.factory)(
                repository_id,
                package_name,
                source_root,
                repo_root,
            )?));
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
