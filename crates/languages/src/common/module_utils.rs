//! Common utilities for module entity extraction
//!
//! These utilities are shared across JavaScript, TypeScript, and Python
//! module handlers for deriving module names and qualified names from file paths.

use std::path::Path;

/// Derive module name from file path
///
/// The module name is the file name without extension.
/// For TypeScript declaration files, also strips the `.d` suffix.
/// For index files (barrel files in JS/TS) in subdirectories, returns the parent directory name.
/// e.g., "/src/utils/helpers.js" -> "helpers"
/// e.g., "/src/types/ambient.d.ts" -> "ambient"
/// e.g., "/src/models/index.ts" -> "models"
/// e.g., "/src/index.ts" -> "index" (no parent dir name to use)
pub fn derive_module_name(file_path: &Path) -> String {
    let stem = file_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("module");

    // Strip .d suffix for TypeScript declaration files
    let name = stem.strip_suffix(".d").unwrap_or(stem);

    // For index files with JS/TS extensions, use the parent directory name
    // This is the barrel file convention in JavaScript/TypeScript
    // Only apply when there's a meaningful parent directory name
    if name == "index" && is_js_ts_file(file_path) {
        if let Some(parent_name) = file_path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
        {
            // Only use parent name if it's non-empty
            if !parent_name.is_empty() {
                return parent_name.to_string();
            }
        }
    }

    name.to_string()
}

/// Check if a file is a JavaScript or TypeScript file based on extension
fn is_js_ts_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            matches!(
                ext,
                "js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs" | "mts" | "cts"
            )
        })
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
/// Treats `index.*` JS/TS files as representing their parent directory (barrel file convention),
/// but only when there's a parent directory (not for root-level index files).
fn build_qualified_name_from_relative(relative: &Path, separator: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();

    // Check if this is a JS/TS index file (barrel file convention)
    // Only treat as barrel if it's in a subdirectory (has parent components)
    let is_index = is_js_ts_file(relative)
        && relative
            .file_stem()
            .and_then(|s| s.to_str())
            .is_some_and(|name| name == "index" || name == "index.d");

    // Count components to check if there are parent directories
    let component_count = relative
        .components()
        .filter(|c| matches!(c, std::path::Component::Normal(_)))
        .count();

    // Only treat as barrel index if there's at least one directory above it
    let is_barrel_index = is_index && component_count > 1;

    for component in relative.components() {
        if let std::path::Component::Normal(s) = component {
            if let Some(s) = s.to_str() {
                // Skip file extension for the last component
                let is_last = relative.file_name() == Some(std::ffi::OsStr::new(s));

                if is_last {
                    // For barrel index files, skip adding "index" - directory name is sufficient
                    if is_barrel_index {
                        continue;
                    }
                    // Strip extension from file name
                    if relative.extension().is_some() {
                        let name = s.rsplit('.').next_back().unwrap_or(s);
                        parts.push(name);
                        continue;
                    }
                }
                parts.push(s);
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
        // index.ts files are treated as barrel files, so "index" is omitted
        assert_eq!(
            derive_path_entity_identifier(&path, &repo_root, "."),
            "packages.core.src"
        );
    }

    #[test]
    fn test_derive_module_name_index_file() {
        // index.ts should use parent directory name
        let path = PathBuf::from("/project/models/index.ts");
        assert_eq!(derive_module_name(&path), "models");
    }

    #[test]
    fn test_derive_qualified_name_index_file() {
        // index.ts should not include "index" in qualified name
        let path = PathBuf::from("/project/models/index.ts");
        let repo_root = PathBuf::from("/project");
        assert_eq!(
            derive_qualified_name(&path, None, &repo_root, "."),
            "models"
        );
    }
}
