//! Module path derivation for Python files
//!
//! Derives the Python module path from a file path relative to the source root.

use std::path::Path;

/// Derive Python module path from file path relative to source root
///
/// Converts a file path to its corresponding Python module path based on
/// Python's package system conventions.
///
/// # Returns
/// - `None` for package root `__init__.py` files directly in the source root
/// - `Some(module_path)` for other files, with `.` as separator
///
/// # Examples
/// - `__init__.py` -> `None` (package root)
/// - `utils.py` -> `Some("utils")`
/// - `utils/__init__.py` -> `Some("utils")`
/// - `utils/helpers.py` -> `Some("utils.helpers")`
/// - `utils/network/client.py` -> `Some("utils.network.client")`
pub fn derive_module_path(file_path: &Path, source_root: &Path) -> Option<String> {
    let relative = file_path.strip_prefix(source_root).ok()?;

    // Get directory components
    let parent = relative.parent()?;
    let mut components: Vec<&str> = parent
        .components()
        .filter_map(|c| c.as_os_str().to_str())
        .collect();

    // Get the filename without extension
    let filename = file_path.file_stem()?.to_str()?;

    // __init__.py marks a package, not a separate module
    if filename != "__init__" {
        components.push(filename);
    }

    if components.is_empty() {
        None
    } else {
        Some(components.join("."))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_root_init_py() {
        let root = PathBuf::from("/project/src");
        let file = PathBuf::from("/project/src/__init__.py");
        assert_eq!(derive_module_path(&file, &root), None);
    }

    #[test]
    fn test_simple_module() {
        let root = PathBuf::from("/project/src");
        let file = PathBuf::from("/project/src/utils.py");
        assert_eq!(derive_module_path(&file, &root), Some("utils".to_string()));
    }

    #[test]
    fn test_package_init() {
        let root = PathBuf::from("/project/src");
        let file = PathBuf::from("/project/src/utils/__init__.py");
        assert_eq!(derive_module_path(&file, &root), Some("utils".to_string()));
    }

    #[test]
    fn test_nested_module() {
        let root = PathBuf::from("/project/src");
        let file = PathBuf::from("/project/src/utils/helpers.py");
        assert_eq!(
            derive_module_path(&file, &root),
            Some("utils.helpers".to_string())
        );
    }

    #[test]
    fn test_deeply_nested_module() {
        let root = PathBuf::from("/project/src");
        let file = PathBuf::from("/project/src/utils/network/client.py");
        assert_eq!(
            derive_module_path(&file, &root),
            Some("utils.network.client".to_string())
        );
    }

    #[test]
    fn test_deeply_nested_init() {
        let root = PathBuf::from("/project/src");
        let file = PathBuf::from("/project/src/utils/network/__init__.py");
        assert_eq!(
            derive_module_path(&file, &root),
            Some("utils.network".to_string())
        );
    }

    #[test]
    fn test_file_outside_source_root() {
        let root = PathBuf::from("/project/src");
        let file = PathBuf::from("/other/file.py");
        assert_eq!(derive_module_path(&file, &root), None);
    }

    #[test]
    fn test_type_stub() {
        let root = PathBuf::from("/project/src");
        let file = PathBuf::from("/project/src/types.pyi");
        assert_eq!(derive_module_path(&file, &root), Some("types".to_string()));
    }
}
