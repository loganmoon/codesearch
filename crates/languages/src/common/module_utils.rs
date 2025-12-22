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
/// e.g., "/src/utils/helpers.js" relative to "/src" -> "utils.helpers"
pub fn derive_qualified_name(
    file_path: &Path,
    source_root: Option<&Path>,
    separator: &str,
) -> String {
    let relative = source_root
        .and_then(|root| file_path.strip_prefix(root).ok())
        .unwrap_or(file_path);

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
        let root = PathBuf::from("/project/src");
        assert_eq!(
            derive_qualified_name(&path, Some(&root), "."),
            "utils.helpers"
        );
    }

    #[test]
    fn test_derive_qualified_name_without_source_root() {
        let path = PathBuf::from("src/utils/helpers.js");
        assert_eq!(derive_qualified_name(&path, None, "."), "src.utils.helpers");
    }

    #[test]
    fn test_derive_qualified_name_python_separator() {
        let path = PathBuf::from("/project/app/models/user.py");
        let root = PathBuf::from("/project");
        assert_eq!(
            derive_qualified_name(&path, Some(&root), "."),
            "app.models.user"
        );
    }

    #[test]
    fn test_derive_qualified_name_single_file() {
        let path = PathBuf::from("/project/main.py");
        let root = PathBuf::from("/project");
        assert_eq!(derive_qualified_name(&path, Some(&root), "."), "main");
    }
}
