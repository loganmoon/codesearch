//! Common utilities for module entity extraction
//!
//! These utilities are shared across JavaScript, TypeScript, and Python
//! module handlers for deriving module names and qualified names from file paths.

use std::path::Path;

/// Derive module name from file path
///
/// The module name is the file name without extension.
/// e.g., "/src/utils/helpers.js" -> "helpers"
pub fn derive_module_name(file_path: &Path) -> String {
    file_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("module")
        .to_string()
}

/// Derive qualified name for the module from file path
///
/// Uses the file path relative to source root to build the qualified name.
/// Falls back to repo_root if source_root doesn't match (never uses absolute paths).
///
/// e.g., "/project/src/utils/helpers.js" relative to "/project/src" -> "utils.helpers"
/// e.g., "/project/other/file.js" relative to "/project" (repo_root) -> "other.file"
pub fn derive_qualified_name(
    file_path: &Path,
    source_root: Option<&Path>,
    repo_root: &Path,
    separator: &str,
) -> String {
    // First try source_root (package-specific), then fall back to repo_root
    let relative = source_root
        .and_then(|root| file_path.strip_prefix(root).ok())
        .or_else(|| file_path.strip_prefix(repo_root).ok())
        .unwrap_or_else(|| {
            // Should never happen if repo_root is correct, but handle gracefully
            tracing::warn!(
                "File path {} not under repo_root {}",
                file_path.display(),
                repo_root.display()
            );
            file_path
        });

    build_qualified_name_from_relative(relative, separator)
}

/// Derive qualified name from a path relative to some root
///
/// This is the core logic shared between qualified_name and path_entity_identifier.
fn build_qualified_name_from_relative(relative: &Path, separator: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();

    for component in relative.components() {
        if let std::path::Component::Normal(s) = component {
            if let Some(s) = s.to_str() {
                // Skip file extension for the last component
                let name = if relative.extension().is_some()
                    && relative.file_name() == Some(std::ffi::OsStr::new(s))
                {
                    s.rsplit('.').next_back().unwrap_or(s)
                } else {
                    s
                };
                parts.push(name);
            }
        }
    }

    parts.join(separator)
}

/// Derive path-based entity identifier from file path
///
/// Always uses repo_root to create a repo-relative path identifier.
/// This is used for import resolution where we need file-path-based lookups.
///
/// e.g., "/project/src/utils/helpers.js" relative to "/project" -> "src.utils.helpers"
pub fn derive_path_entity_identifier(
    file_path: &Path,
    repo_root: &Path,
    separator: &str,
) -> String {
    let relative = file_path.strip_prefix(repo_root).unwrap_or_else(|_| {
        tracing::warn!(
            "File path {} not under repo_root {}",
            file_path.display(),
            repo_root.display()
        );
        file_path
    });

    build_qualified_name_from_relative(relative, separator)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_derive_module_name_js() {
        let path = PathBuf::from("/src/utils/helpers.js");
        assert_eq!(derive_module_name(&path), "helpers");
    }

    #[test]
    fn test_derive_module_name_ts() {
        let path = PathBuf::from("/src/components/Button.tsx");
        assert_eq!(derive_module_name(&path), "Button");
    }

    #[test]
    fn test_derive_module_name_py() {
        let path = PathBuf::from("/app/models/user.py");
        assert_eq!(derive_module_name(&path), "user");
    }

    #[test]
    fn test_derive_module_name_no_extension() {
        let path = PathBuf::from("/src/index");
        assert_eq!(derive_module_name(&path), "index");
    }

    #[test]
    fn test_derive_qualified_name_with_source_root() {
        let path = PathBuf::from("/project/src/utils/helpers.js");
        let source_root = PathBuf::from("/project/src");
        let repo_root = PathBuf::from("/project");
        assert_eq!(
            derive_qualified_name(&path, Some(&source_root), &repo_root, "."),
            "utils.helpers"
        );
    }

    #[test]
    fn test_derive_qualified_name_fallback_to_repo_root() {
        // When source_root doesn't match, falls back to repo_root
        let path = PathBuf::from("/project/other/file.js");
        let source_root = PathBuf::from("/project/src"); // doesn't match
        let repo_root = PathBuf::from("/project");
        assert_eq!(
            derive_qualified_name(&path, Some(&source_root), &repo_root, "."),
            "other.file"
        );
    }

    #[test]
    fn test_derive_qualified_name_without_source_root() {
        let path = PathBuf::from("/project/src/utils/helpers.js");
        let repo_root = PathBuf::from("/project");
        assert_eq!(
            derive_qualified_name(&path, None, &repo_root, "."),
            "src.utils.helpers"
        );
    }

    #[test]
    fn test_derive_qualified_name_python_separator() {
        let path = PathBuf::from("/project/app/models/user.py");
        let repo_root = PathBuf::from("/project");
        assert_eq!(
            derive_qualified_name(&path, None, &repo_root, "."),
            "app.models.user"
        );
    }

    #[test]
    fn test_derive_qualified_name_single_file() {
        let path = PathBuf::from("/project/main.py");
        let repo_root = PathBuf::from("/project");
        assert_eq!(derive_qualified_name(&path, None, &repo_root, "."), "main");
    }

    #[test]
    fn test_derive_path_entity_identifier() {
        let path = PathBuf::from("/project/src/utils/helpers.js");
        let repo_root = PathBuf::from("/project");
        assert_eq!(
            derive_path_entity_identifier(&path, &repo_root, "."),
            "src.utils.helpers"
        );
    }

    #[test]
    fn test_derive_path_entity_identifier_nested() {
        let path = PathBuf::from("/project/packages/core/src/index.ts");
        let repo_root = PathBuf::from("/project");
        assert_eq!(
            derive_path_entity_identifier(&path, &repo_root, "."),
            "packages.core.src.index"
        );
    }
}
